//! The one way the daemon reads a user-editable file that steers security decisions
//! (`grants.toml`, `config.toml`).
use std::io::Read;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReadError {
    /// Not a regular file we own, or accessible by group/others.
    Insecure,
    /// Unreadable, not UTF-8, a symlink or a FIFO.
    Io,
    /// Longer than the cap.
    TooLarge,
}

/// `None` = the file does not exist. Opened with O_NOFOLLOW|O_NONBLOCK and checked on the open
/// descriptor, so a swapped path or a FIFO cannot bypass the check or hang startup. One byte
/// over `cap` is read so that an oversize file is an error, not a silent truncation.
pub(crate) fn read_private(path: &Path, cap: u64) -> Result<Option<String>, ReadError> {
    let flags = rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::NONBLOCK;
    let f = match std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(flags.bits() as i32)
        .open(path)
    {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(ReadError::Io),
    };
    let md = f.metadata().map_err(|_| ReadError::Io)?;
    if !md.is_file() || md.mode() & 0o077 != 0 || md.uid() != rustix::process::geteuid().as_raw() {
        return Err(ReadError::Insecure);
    }
    let mut text = String::new();
    f.take(cap + 1)
        .read_to_string(&mut text)
        .map_err(|_| ReadError::Io)?;
    if text.len() as u64 > cap {
        return Err(ReadError::TooLarge);
    }
    Ok(Some(text))
}
