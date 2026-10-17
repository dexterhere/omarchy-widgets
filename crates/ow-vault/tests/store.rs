#![allow(clippy::unwrap_used, clippy::expect_used)]
use ow_vault::{Profile, Vault, VaultError};
use std::os::unix::fs::PermissionsExt;

const PW: &[u8] = b"correct horse battery";
const SECRET: &str = "FAKE-SECRET-VALUE-9f3a";

fn mode(p: &std::path::Path) -> u32 {
    std::fs::metadata(p).unwrap().permissions().mode() & 0o777
}

fn fresh() -> (tempfile::TempDir, std::path::PathBuf) {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("vault.owv");
    (d, p)
}

#[test]
fn create_set_reopen_get_remove() {
    let (_d, p) = fresh();
    let mut v = Vault::create(&p, PW, Profile::Minimum).unwrap();
    v.set("github-main", SECRET).unwrap();
    drop(v);
    let mut v = Vault::open(&p, PW).unwrap();
    assert_eq!(v.get("github-main"), Some(SECRET));
    assert_eq!(v.names().collect::<Vec<_>>(), ["github-main"]);
    assert!(v.remove("github-main").unwrap());
    assert!(!v.remove("github-main").unwrap());
    assert_eq!(Vault::open(&p, PW).unwrap().get("github-main"), None);
}

#[test]
fn file_is_private_and_leaves_no_temp_files() {
    let (d, p) = fresh();
    let mut v = Vault::create(&p, PW, Profile::Minimum).unwrap();
    v.set("a", SECRET).unwrap();
    assert_eq!(mode(&p), 0o600);
    let mut names: Vec<_> = std::fs::read_dir(d.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    assert_eq!(names, ["vault.lock", "vault.owv"]);
}

/// Fake-secret leak test for the file: neither the value, the name nor the passphrase
/// appears anywhere in any file the vault wrote.
#[test]
fn secret_name_and_passphrase_are_not_in_any_file() {
    let (d, p) = fresh();
    let mut v = Vault::create(&p, PW, Profile::Minimum).unwrap();
    v.set("very-private-name", SECRET).unwrap();
    for e in std::fs::read_dir(d.path()).unwrap() {
        let bytes = std::fs::read(e.unwrap().path()).unwrap();
        for needle in [SECRET.as_bytes(), b"very-private-name", PW] {
            assert!(!bytes.windows(needle.len()).any(|w| w == needle));
        }
    }
}

#[test]
fn wrong_passphrase_and_weak_passphrase() {
    let (_d, p) = fresh();
    assert_eq!(
        Vault::create(&p, b"short", Profile::Minimum).err(),
        Some(VaultError::WeakPassphrase)
    );
    Vault::create(&p, PW, Profile::Minimum).unwrap();
    assert_eq!(
        Vault::open(&p, b"wrong horse battery").err(),
        Some(VaultError::WrongKey)
    );
    assert_eq!(
        Vault::create(&p, PW, Profile::Minimum).err(),
        Some(VaultError::Exists)
    );
}

#[test]
fn tampered_file_fails_to_open() {
    let (_d, p) = fresh();
    let mut v = Vault::create(&p, PW, Profile::Minimum).unwrap();
    v.set("a", SECRET).unwrap();
    let mut b = std::fs::read(&p).unwrap();
    let last = b.len() - 1;
    b[last] ^= 1;
    std::fs::write(&p, &b).unwrap();
    assert!(Vault::open(&p, PW).is_err());
}

#[test]
fn insecure_and_symlinked_files_are_refused() {
    let (d, p) = fresh();
    Vault::create(&p, PW, Profile::Minimum).unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(Vault::open(&p, PW).err(), Some(VaultError::Insecure));
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600)).unwrap();
    let link = d.path().join("link.owv");
    std::os::unix::fs::symlink(&p, &link).unwrap();
    assert!(Vault::open(&link, PW).is_err());
}

#[test]
fn rotation_changes_passphrase_and_data_key() {
    let (_d, p) = fresh();
    let mut v = Vault::create(&p, PW, Profile::Minimum).unwrap();
    v.set("a", SECRET).unwrap();
    let old = std::fs::read(&p).unwrap();
    v.rotate_passphrase(b"another long passphrase", Profile::Minimum)
        .unwrap();
    assert_eq!(Vault::open(&p, PW).err(), Some(VaultError::WrongKey));
    assert_eq!(
        Vault::open(&p, b"another long passphrase")
            .unwrap()
            .get("a"),
        Some(SECRET)
    );
    // Archived pre-rotation header and wrapped key spliced onto the new payload must fail
    // even with the OLD passphrase.
    let new = std::fs::read(&p).unwrap();
    let hl = 4 + 1 + 1 + 16 + 17 + 24 + 48;
    let mut spliced = old[..hl].to_vec();
    spliced.extend_from_slice(&new[hl..]);
    std::fs::write(&p, &spliced).unwrap();
    assert!(Vault::open(&p, PW).is_err());
}

#[test]
fn name_and_size_limits() {
    let (_d, p) = fresh();
    let mut v = Vault::create(&p, PW, Profile::Minimum).unwrap();
    assert_eq!(v.set("Bad Name", "x").err(), Some(VaultError::InvalidName));
    assert_eq!(
        v.set("ok", &"x".repeat(16 * 1024 + 1)).err(),
        Some(VaultError::TooLarge)
    );
    v.set("ok", &"x".repeat(16 * 1024)).unwrap();
}

#[test]
fn concurrent_writer_is_refused() {
    let (_d, p) = fresh();
    let mut v = Vault::create(&p, PW, Profile::Minimum).unwrap();
    let lock = std::fs::File::open(p.with_extension("lock")).unwrap();
    rustix::fs::flock(&lock, rustix::fs::FlockOperation::NonBlockingLockExclusive).unwrap();
    assert_eq!(v.set("a", "x").err(), Some(VaultError::Busy));
}
