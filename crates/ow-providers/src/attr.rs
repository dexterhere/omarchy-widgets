//! One bounded, non-blocking way to read a sysfs or procfs file.
//!
//! A hostile or broken tree can contain a FIFO (blocks `open`/`read` forever), a symlink to
//! `/dev/zero` (never ends) or a directory. So: open with O_NONBLOCK | O_NOFOLLOW (only the final
//! component; directories on the way, such as the `BAT0` symlink, resolve normally), require a
//! regular file, and read at most `max` bytes; longer content is a `Format` error.
use ow_core::ProviderError;
use rustix::fs::OFlags;
use std::fs::{File, OpenOptions};
use std::io::Read;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

/// Real sysfs attributes are below one page.
pub(crate) const SYSFS_MAX: u64 = 4096;
/// /proc/stat grows with the CPU count.
pub(crate) const PROC_MAX: u64 = 256 * 1024;

pub(crate) fn read(path: &Path, max: u64) -> Result<String, ProviderError> {
    let flags = (OFlags::NONBLOCK | OFlags::NOFOLLOW).bits() as i32;
    let f: File = OpenOptions::new()
        .read(true)
        .custom_flags(flags)
        .open(path)
        .map_err(|_| ProviderError::Unavailable)?;
    if !f.metadata().is_ok_and(|m| m.is_file()) {
        return Err(ProviderError::Format);
    }
    let mut s = String::new();
    Read::take(f, max + 1)
        .read_to_string(&mut s)
        .map_err(|_| ProviderError::Format)?;
    if s.len() as u64 > max {
        return Err(ProviderError::Format);
    }
    Ok(s)
}

#[cfg(test)]
pub(crate) mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use rustix::fs::{CWD, FileType, Mode, mknodat};
    use std::sync::mpsc;
    use std::time::Duration;

    /// Run `f` on a thread; fail the test if it does not finish in 2 s.
    pub(crate) fn within_2s<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(f());
        });
        rx.recv_timeout(Duration::from_secs(2))
            .expect("read hung: not bounded or not non-blocking")
    }

    /// Replace `p` by each hostile kind in turn, calling `check` after each.
    pub(crate) fn hostile(p: &Path, mut check: impl FnMut()) {
        let _ = std::fs::remove_file(p);
        mknodat(CWD, p, FileType::Fifo, Mode::RWXU, 0).unwrap();
        check();
        std::fs::remove_file(p).unwrap();
        std::os::unix::fs::symlink("/dev/zero", p).unwrap();
        check();
        std::fs::remove_file(p).unwrap();
        std::fs::create_dir(p).unwrap();
        check();
        std::fs::remove_dir(p).unwrap();
        std::fs::write(p, "x".repeat(5000)).unwrap();
        check();
    }

    #[test]
    fn reads_regular_files_and_follows_directory_symlinks() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir(d.path().join("real")).unwrap();
        std::fs::write(d.path().join("real/f"), "42\n").unwrap();
        std::os::unix::fs::symlink(d.path().join("real"), d.path().join("link")).unwrap();
        assert_eq!(read(&d.path().join("link/f"), SYSFS_MAX).unwrap(), "42\n");
        assert_eq!(
            read(&d.path().join("nope"), SYSFS_MAX),
            Err(ProviderError::Unavailable)
        );
    }

    #[test]
    fn hostile_files_return_promptly_as_errors() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("attr");
        let p2 = p.clone();
        hostile(&p, move || {
            let q = p2.clone();
            assert!(within_2s(move || read(&q, SYSFS_MAX)).is_err());
        });
    }

    /// A huge regular file must be cut off after `max` bytes, not read to the end.
    #[test]
    fn huge_file_is_cut_off_not_slurped() {
        // Sparse file on the disk filesystem, not tmpfs (RAM-backed on some machines).
        let d = tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR")).unwrap();
        let p = d.path().join("big");
        std::fs::File::create(&p).unwrap().set_len(1 << 30).unwrap(); // 1 GiB of zeros
        let t = std::time::Instant::now();
        assert_eq!(
            within_2s(move || read(&p, SYSFS_MAX)),
            Err(ProviderError::Format)
        );
        assert!(t.elapsed() < Duration::from_millis(200));
    }

    /// Only the final component is not followed: an attribute that is a symlink is refused.
    #[test]
    fn symlinked_attribute_file_is_refused() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("real"), "1").unwrap();
        std::os::unix::fs::symlink(d.path().join("real"), d.path().join("l")).unwrap();
        assert!(read(&d.path().join("l"), SYSFS_MAX).is_err());
        assert!(read(&d.path().join("real"), SYSFS_MAX).is_ok());
    }
}
