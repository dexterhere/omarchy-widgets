use crate::format::{
    Header, Key32, MAX_FILE, MAX_RECORDS, MAX_VALUE, Mode, Profile, Records, VaultError, decode,
    encode, new_key, random, valid_name,
};
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use zeroize::Zeroizing;

const MIN_PASSPHRASE: usize = 12;

/// An unlocked vault. Holds the master and data keys in zeroizing buffers for as long as it
/// lives; drop it to lock.
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

/// Exclusive writer lock, held for the duration of one mutation.
fn lock(path: &Path) -> Result<File, VaultError> {
    let f = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .mode(0o600)
        .open(path.with_extension("lock"))
        .map_err(io)?;
    rustix::fs::flock(&f, rustix::fs::FlockOperation::NonBlockingLockExclusive)
        .map_err(|_| VaultError::Busy)?;
    Ok(f)
}

/// Read with O_NOFOLLOW, check the open descriptor (regular, ours, no group/other access).
fn read_private(path: &Path) -> Result<Vec<u8>, VaultError> {
    let flags = rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::NONBLOCK;
    let f = OpenOptions::new()
        .read(true)
        .custom_flags(flags.bits() as i32)
        .open(path)
        .map_err(io)?;
    let md = f.metadata().map_err(io)?;
    if !md.is_file() || md.mode() & 0o077 != 0 || md.uid() != rustix::process::geteuid().as_raw() {
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

/// temp file (0600) in the same directory, fsync, rename, fsync directory.
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), VaultError> {
    let tmp = path.with_extension("tmp");
    let _ = std::fs::remove_file(&tmp);
    let mut f = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&tmp)
        .map_err(io)?;
    let res = f
        .write_all(bytes)
        .and_then(|()| f.sync_all())
        .and_then(|()| std::fs::rename(&tmp, path));
    if res.is_err() {
        let _ = std::fs::remove_file(&tmp);
        return Err(VaultError::Io);
    }
    if let Some(dir) = path.parent() {
        File::open(dir).and_then(|d| d.sync_all()).map_err(io)?;
    }
    Ok(())
}

fn check_passphrase(p: &[u8]) -> Result<(), VaultError> {
    if p.len() < MIN_PASSPHRASE {
        Err(VaultError::WeakPassphrase)
    } else {
        Ok(())
    }
}

impl Vault {
    /// New passphrase vault. Passphrase bytes are used exactly as given (no normalization,
    /// no trimming; callers strip one trailing newline when reading stdin).
    pub fn create(path: &Path, passphrase: &[u8], profile: Profile) -> Result<Vault, VaultError> {
        check_passphrase(passphrase)?;
        let _lock = lock(path)?;
        if std::fs::symlink_metadata(path).is_ok() {
            return Err(VaultError::Exists);
        }
        let salt = random::<16>()?;
        let header = Header {
            file_id: random::<16>()?,
            mode: Mode::Passphrase { profile, salt },
        };
        let v = Vault {
            path: path.to_owned(),
            master: profile.derive(passphrase, &salt)?,
            data_key: new_key()?,
            records: Records::new(),
            header,
        };
        v.save(&v.records)?;
        Ok(v)
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

    /// Takes the value as bytes the caller already owns; it is stored zeroizing.
    pub fn set(&mut self, name: &str, value: &str) -> Result<(), VaultError> {
        if !valid_name(name) {
            return Err(VaultError::InvalidName);
        }
        if value.len() > MAX_VALUE {
            return Err(VaultError::TooLarge);
        }
        let mut next = self.records.clone();
        next.insert(name.to_owned(), Zeroizing::new(value.to_owned()));
        if next.len() > MAX_RECORDS {
            return Err(VaultError::TooLarge);
        }
        let _lock = lock(&self.path)?;
        self.save(&next)?;
        self.records = next;
        Ok(())
    }

    pub fn remove(&mut self, name: &str) -> Result<bool, VaultError> {
        let mut next = self.records.clone();
        if next.remove(name).is_none() {
            return Ok(false);
        }
        let _lock = lock(&self.path)?;
        self.save(&next)?;
        self.records = next;
        Ok(true)
    }

    /// New passphrase, new salt, new profile and a NEW DATA KEY; everything is re-sealed so an
    /// archived old header plus the old passphrase cannot open the new file.
    pub fn rotate_passphrase(&mut self, new: &[u8], profile: Profile) -> Result<(), VaultError> {
        check_passphrase(new)?;
        let _lock = lock(&self.path)?;
        let salt = random::<16>()?;
        let next = Vault {
            path: self.path.clone(),
            header: Header {
                file_id: self.header.file_id,
                mode: Mode::Passphrase { profile, salt },
            },
            master: profile.derive(new, &salt)?,
            data_key: new_key()?,
            records: self.records.clone(),
        };
        next.save(&next.records)?;
        *self = next;
        Ok(())
    }

    fn save(&self, records: &Records) -> Result<(), VaultError> {
        let bytes = encode(&self.header, &self.master, &self.data_key, records)?;
        write_atomic(&self.path, &bytes)
    }
}
