use crate::format::{
    Header, Key32, MAX_FILE, MAX_RECORDS, MAX_VALUE, Mode, Profile, Records, VaultError, decode,
    encode, new_key, random, valid_name,
};
use rustix::fs::{CWD, OFlags, RenameFlags, renameat_with};
use std::ffi::OsString;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use zeroize::Zeroizing;

const MIN_PASSPHRASE: usize = 12;

/// An unlocked vault. Holds the master and data keys in zeroizing buffers for as long as it
/// lives; drop it to lock. Every mutation re-reads the file under the write lock, so a stale
/// handle fails with `Stale` instead of overwriting newer state.
pub struct Vault {
    path: PathBuf,
    header: Header,
    master: Key32,
    data_key: Key32,
    records: Records,
}

fn io<E>(_: E) -> VaultError {
    VaultError::Io
}

/// `<path><suffix>` appended to the full file name (never replaces an extension, so
/// sidecar files cannot alias the vault itself).
fn sidecar(path: &Path, suffix: &str) -> PathBuf {
    let mut s: OsString = path.as_os_str().to_owned();
    s.push(suffix);
    PathBuf::from(s)
}

fn is_private_regular(md: &std::fs::Metadata) -> bool {
    md.is_file() && md.mode() & 0o077 == 0 && md.uid() == rustix::process::geteuid().as_raw()
}

fn open_nofollow(path: &Path, opts: &mut OpenOptions) -> std::io::Result<File> {
    opts.custom_flags((OFlags::NOFOLLOW | OFlags::NONBLOCK).bits() as i32)
        .open(path)
}

/// Exclusive writer lock, held for the duration of one mutation.
fn lock(path: &Path) -> Result<File, VaultError> {
    let p = sidecar(path, ".lock");
    let f = open_nofollow(
        &p,
        OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .mode(0o600),
    )
    .map_err(io)?;
    if !is_private_regular(&f.metadata().map_err(io)?) {
        // Pre-existing file with a loose mode: tighten once, then re-check ownership/type.
        f.set_permissions(std::fs::Permissions::from_mode(0o600))
            .map_err(io)?;
        if !is_private_regular(&f.metadata().map_err(io)?) {
            return Err(VaultError::Insecure);
        }
    }
    rustix::fs::flock(&f, rustix::fs::FlockOperation::NonBlockingLockExclusive)
        .map_err(|_| VaultError::Busy)?;
    Ok(f)
}

/// Read with O_NOFOLLOW, check the open descriptor (regular, ours, no group/other access).
fn read_private(path: &Path) -> Result<Vec<u8>, VaultError> {
    let f = open_nofollow(path, OpenOptions::new().read(true)).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            VaultError::NotFound
        } else {
            VaultError::Io
        }
    })?;
    if !is_private_regular(&f.metadata().map_err(io)?) {
        return Err(VaultError::Insecure);
    }
    let mut buf = Vec::new();
    f.take(MAX_FILE as u64 + 1)
        .read_to_end(&mut buf)
        .map_err(io)?;
    if buf.len() > MAX_FILE {
        return Err(VaultError::TooLarge);
    }
    Ok(buf)
}

enum Commit {
    /// Renamed and the directory synced.
    Durable,
    /// Renamed, but the directory fsync failed: the new file is visible, durability unknown.
    NotSynced,
}

/// Unique temp file (0600, create_new) in the same directory, fsync, rename, fsync directory.
/// With `replace == false` the rename refuses to overwrite (`Exists`), closing the window
/// between an existence check and the rename.
fn write_atomic(path: &Path, bytes: &[u8], replace: bool) -> Result<Commit, VaultError> {
    let tmp = sidecar(path, &format!(".{}.tmp", hex(&random::<8>()?)));
    let mut f = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&tmp)
        .map_err(io)?;
    let res = f.write_all(bytes).and_then(|()| f.sync_all());
    let res = res.map_err(io).and_then(|()| {
        let flags = if replace {
            RenameFlags::empty()
        } else {
            RenameFlags::NOREPLACE
        };
        renameat_with(CWD, &tmp, CWD, path, flags).map_err(|e| {
            if e == rustix::io::Errno::EXIST {
                VaultError::Exists
            } else {
                VaultError::Io
            }
        })
    });
    if let Err(e) = res {
        let _ = std::fs::remove_file(&tmp); // our own unique temp file only
        return Err(e);
    }
    let dir = match path.parent() {
        Some(d) if !d.as_os_str().is_empty() => d,
        _ => Path::new("."),
    };
    Ok(if File::open(dir).and_then(|d| d.sync_all()).is_ok() {
        Commit::Durable
    } else {
        Commit::NotSynced
    })
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn check_passphrase(p: &[u8]) -> Result<(), VaultError> {
    if p.len() < MIN_PASSPHRASE {
        Err(VaultError::WeakPassphrase)
    } else {
        Ok(())
    }
}

fn check_new_profile(p: Profile) -> Result<(), VaultError> {
    // Minimum exists so old files can still be opened; new vaults use Standard or Strong.
    if p == Profile::Minimum {
        Err(VaultError::ProfileNotAllowed)
    } else {
        Ok(())
    }
}

impl Vault {
    /// New passphrase vault. Passphrase bytes are used exactly as given (no normalization,
    /// no trimming; callers strip one trailing newline when reading stdin).
    pub fn create(path: &Path, passphrase: &[u8], profile: Profile) -> Result<Vault, VaultError> {
        check_new_profile(profile)?;
        Self::create_inner(path, passphrase, profile)
    }

    fn create_inner(path: &Path, passphrase: &[u8], profile: Profile) -> Result<Vault, VaultError> {
        check_passphrase(passphrase)?;
        let _lock = lock(path)?;
        let salt = random::<16>()?;
        let v = Vault {
            path: path.to_owned(),
            header: Header {
                file_id: random::<16>()?,
                mode: Mode::Passphrase { profile, salt },
            },
            master: profile.derive(passphrase, &salt)?,
            data_key: new_key()?,
            records: Records::new(),
        };
        // NOREPLACE: refuses to clobber a file that appeared at any point, not just now.
        match v.save(&v.data_key, &v.records, false)? {
            Commit::Durable => Ok(v),
            Commit::NotSynced => Err(VaultError::Io),
        }
    }

    pub fn open(path: &Path, passphrase: &[u8]) -> Result<Vault, VaultError> {
        let bytes = read_private(path)?;
        let mut master = None;
        let (header, data_key, records) = decode(&bytes, |h| {
            let Mode::Passphrase { profile, salt } = &h.mode;
            let k = profile.derive(passphrase, salt)?;
            master = Some(k.clone());
            Ok(k)
        })?;
        let master = master.ok_or(VaultError::Corrupt)?;
        Ok(Vault {
            path: path.to_owned(),
            header,
            master,
            data_key,
            records,
        })
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        self.records.get(name).map(|v| v.as_str())
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.records.keys().map(String::as_str)
    }

    pub fn set(&mut self, name: &str, value: &str) -> Result<(), VaultError> {
        if !valid_name(name) {
            return Err(VaultError::InvalidName);
        }
        if value.is_empty() {
            return Err(VaultError::EmptyValue);
        }
        if value.len() > MAX_VALUE {
            return Err(VaultError::TooLarge);
        }
        self.mutate(|r| {
            r.insert(name.to_owned(), Zeroizing::new(value.to_owned()));
            if r.len() > MAX_RECORDS {
                Err(VaultError::TooLarge)
            } else {
                Ok(true)
            }
        })
        .map(drop)
    }

    pub fn remove(&mut self, name: &str) -> Result<bool, VaultError> {
        self.mutate(|r| Ok(r.remove(name).is_some()))
    }

    /// New passphrase, new salt, new profile and a NEW DATA KEY; everything is re-sealed so an
    /// archived old header plus the old passphrase cannot open the new file.
    pub fn rotate_passphrase(&mut self, new: &[u8], profile: Profile) -> Result<(), VaultError> {
        check_new_profile(profile)?;
        check_passphrase(new)?;
        let _lock = lock(&self.path)?;
        let (_, _, records) = self.current()?;
        let salt = random::<16>()?;
        let next = Vault {
            path: self.path.clone(),
            header: Header {
                file_id: self.header.file_id,
                mode: Mode::Passphrase { profile, salt },
            },
            master: profile.derive(new, &salt)?,
            data_key: new_key()?,
            records,
        };
        let commit = next.save(&next.data_key, &next.records, true);
        // The rename may have happened even if a later step failed; keep memory in step with disk.
        if commit.is_ok() {
            *self = next;
        }
        commit.and_then(durable)
    }

    /// Re-read the file under the lock using the master key we hold. A rotation by another
    /// handle changes the master key, so decoding fails and we report `Stale`.
    fn current(&self) -> Result<(Header, Key32, Records), VaultError> {
        let bytes = read_private(&self.path)?;
        decode(&bytes, |_| Ok(self.master.clone())).map_err(|e| match e {
            VaultError::WrongKey => VaultError::Stale,
            other => other,
        })
    }

    /// Lock, take the latest on-disk records, apply `f`, write. `f` returns whether anything
    /// changed. Memory is updated whenever the new file became visible.
    fn mutate(
        &mut self,
        f: impl FnOnce(&mut Records) -> Result<bool, VaultError>,
    ) -> Result<bool, VaultError> {
        let _lock = lock(&self.path)?;
        let (header, data_key, mut records) = self.current()?;
        let changed = f(&mut records)?;
        if !changed {
            self.records = records;
            return Ok(false);
        }
        let probe = Vault {
            path: self.path.clone(),
            header,
            master: self.master.clone(),
            data_key,
            records: Records::new(),
        };
        let commit = probe.save(&probe.data_key, &records, true);
        if commit.is_ok() {
            self.header = probe.header;
            self.data_key = probe.data_key;
            self.records = records;
        }
        commit.and_then(durable).map(|()| true)
    }

    fn save(
        &self,
        data_key: &Key32,
        records: &Records,
        replace: bool,
    ) -> Result<Commit, VaultError> {
        let bytes = encode(&self.header, &self.master, data_key, records)?;
        write_atomic(&self.path, &bytes, replace)
    }
}

/// What can be known without the passphrase (the header is not secret).
#[derive(Debug, PartialEq, Eq)]
pub struct Info {
    pub mode: &'static str,
    pub profile: Profile,
}

pub fn inspect(path: &Path) -> Result<Info, VaultError> {
    let bytes = read_private(path)?;
    match crate::format::parse_header(&bytes)?.mode {
        Mode::Passphrase { profile, .. } => Ok(Info {
            mode: "passphrase",
            profile,
        }),
    }
}

/// Remove the vault file and any leftover temp copies of it (`<name>.<16 hex>.tmp`, left by
/// a crash mid-write). The lock file is kept: unlinking a lock someone may be waiting on
/// would let two writers hold different inodes. Copies made elsewhere (backups, snapshots,
/// dotfile repos) are not touched and stay decryptable with the passphrase.
pub fn wipe(path: &Path) -> Result<(), VaultError> {
    if std::fs::symlink_metadata(path).is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound) {
        return Err(VaultError::NotFound);
    }
    let _lock = lock(path)?;
    std::fs::remove_file(path).map_err(io)?;
    let (Some(dir), Some(name)) = (path.parent(), path.file_name().and_then(|n| n.to_str())) else {
        return Ok(());
    };
    let dir = if dir.as_os_str().is_empty() {
        Path::new(".")
    } else {
        dir
    };
    for e in std::fs::read_dir(dir).map_err(io)?.flatten() {
        let f = e.file_name();
        let f = f.to_string_lossy();
        let is_tmp = f
            .strip_prefix(name)
            .and_then(|r| r.strip_prefix('.'))
            .and_then(|r| r.strip_suffix(".tmp"))
            .is_some_and(|h| h.len() == 16 && h.bytes().all(|b| b.is_ascii_hexdigit()));
        if is_tmp {
            let _ = std::fs::remove_file(e.path());
        }
    }
    Ok(())
}

fn durable(c: Commit) -> Result<(), VaultError> {
    match c {
        Commit::Durable => Ok(()),
        Commit::NotSynced => Err(VaultError::Io),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    const PW: &[u8] = b"correct horse battery";

    #[test]
    fn rotation_replaces_the_data_key() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("v.owv");
        let mut v = Vault::create_inner(&p, PW, Profile::Minimum).unwrap();
        let before = v.data_key.clone();
        v.rotate_passphrase(b"another long passphrase", Profile::Standard)
            .unwrap();
        assert_ne!(*before, *v.data_key);
    }

    #[test]
    fn vault_named_like_its_sidecars_is_safe() {
        let d = tempfile::tempdir().unwrap();
        for name in ["v.tmp", ".lock", "v.lock"] {
            let p = d.path().join(name);
            let mut v = Vault::create_inner(&p, PW, Profile::Minimum).unwrap();
            v.set("a", "x").unwrap();
            assert_eq!(Vault::open(&p, PW).unwrap().get("a"), Some("x"), "{name}");
        }
    }

    #[test]
    fn unrelated_neighbour_file_survives_and_relative_path_works() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("vault.tmp"), b"user data").unwrap();
        let p = d.path().join("vault.owv");
        let mut v = Vault::create_inner(&p, PW, Profile::Minimum).unwrap();
        v.set("a", "x").unwrap();
        assert_eq!(
            std::fs::read(d.path().join("vault.tmp")).unwrap(),
            b"user data"
        );
        // Bare relative file name: parent() is "" and must not break the directory sync.
        let cwd = std::env::current_dir().unwrap();
        std::env::set_current_dir(d.path()).unwrap();
        let r = Vault::create_inner(Path::new("rel.owv"), PW, Profile::Minimum);
        std::env::set_current_dir(cwd).unwrap();
        assert!(r.is_ok());
    }
}
