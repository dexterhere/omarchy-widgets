use crate::conn;
use ow_core::Paths;
use rustix::fs::FileType;
use std::fs::DirBuilder;
use std::io::{self, ErrorKind};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

const MAX_CONNECTIONS: usize = 32;

pub struct Server {
    listener: UnixListener,
    path: PathBuf,
    active: Arc<AtomicUsize>,
}

/// Disable core dumps and ptrace/proc-mem access to this process.
pub fn harden_process() -> io::Result<()> {
    use rustix::process::{Resource, Rlimit, setrlimit};
    rustix::process::set_dumpable_behavior(rustix::process::DumpableBehavior::NotDumpable)?;
    setrlimit(
        Resource::Core,
        Rlimit {
            current: Some(0),
            maximum: Some(0),
        },
    )?;
    Ok(())
}

fn check_dir(dir: &std::path::Path) -> io::Result<()> {
    let md = std::fs::symlink_metadata(dir)?;
    let euid = rustix::process::geteuid().as_raw();
    if !md.is_dir() {
        return Err(io::Error::other(
            "socket dir is not a directory (or is a symlink)",
        ));
    }
    if md.uid() != euid {
        return Err(io::Error::other(
            "socket dir is not owned by the current user",
        ));
    }
    if md.mode() & 0o077 != 0 {
        return Err(io::Error::other("socket dir is accessible by group/others"));
    }
    Ok(())
}

impl Server {
    /// Create the socket directory (0700), refuse unsafe ones, replace a stale socket,
    /// refuse to start if another daemon is alive, bind, and chmod the socket 0600.
    pub fn bind(paths: &Paths) -> io::Result<Server> {
        let mut b = DirBuilder::new();
        b.recursive(true).mode(0o700);
        b.create(&paths.runtime)?;
        check_dir(&paths.runtime)?;
        let path = paths.socket();
        match std::fs::symlink_metadata(&path) {
            Ok(md) => {
                if FileType::from_raw_mode(md.mode()) != FileType::Socket {
                    return Err(io::Error::other("socket path exists and is not a socket"));
                }
                if UnixStream::connect(&path).is_ok() {
                    return Err(io::Error::new(
                        ErrorKind::AddrInUse,
                        "owd is already running",
                    ));
                }
                std::fs::remove_file(&path)?;
            }
            Err(e) if e.kind() == ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
        // The 0700 directory protects the window between bind and chmod.
        let listener = UnixListener::bind(&path)?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
        listener.set_nonblocking(true)?;
        Ok(Server {
            listener,
            path,
            active: Arc::new(AtomicUsize::new(0)),
        })
    }

    pub fn socket_path(&self) -> &std::path::Path {
        &self.path
    }

    /// Accept loop until `stop` is set. Thread per connection, capped.
    // ponytail: polling accept + threads; move to tokio when providers need async.
    pub fn run(&self, stop: &AtomicBool) {
        while !stop.load(Ordering::Relaxed) {
            match self.listener.accept() {
                Ok((stream, _)) => self.spawn(stream),
                Err(e) if e.kind() == ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(25));
                }
                Err(_) => std::thread::sleep(Duration::from_millis(100)),
            }
        }
    }

    fn spawn(&self, stream: UnixStream) {
        if self.active.fetch_add(1, Ordering::AcqRel) >= MAX_CONNECTIONS {
            self.active.fetch_sub(1, Ordering::AcqRel);
            return; // drop: over the connection cap
        }
        let active = Arc::clone(&self.active);
        std::thread::spawn(move || {
            let _ = stream.set_nonblocking(false);
            conn::serve(stream);
            active.fetch_sub(1, Ordering::AcqRel);
        });
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}
