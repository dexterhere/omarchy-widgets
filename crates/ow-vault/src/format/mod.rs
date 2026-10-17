//! Pure encode/decode of the vault file. No file IO here.
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{Key, XChaCha20Poly1305, XNonce};
use std::collections::BTreeMap;
use zeroize::Zeroizing;

pub const MAX_NAME: usize = 64;
pub const MAX_VALUE: usize = 16 * 1024;
pub const MAX_RECORDS: usize = 1024;
const MAX_PLAINTEXT: usize = 1024 * 1024;
const TAG: usize = 16;
const MAGIC: &[u8; 4] = b"OWV0";
const VERSION: u8 = 0;
const WRAPPED_LEN: usize = 32 + TAG;
/// magic + version + mode + file_id
const FIXED_HEADER: usize = 4 + 1 + 1 + 16;
pub(crate) const MAX_FILE: usize =
    FIXED_HEADER + 17 + 24 + WRAPPED_LEN + 24 + 4 + MAX_PLAINTEXT + TAG;

pub type Key32 = Zeroizing<[u8; 32]>;
/// Values are zeroized. Names are labels, not secrets, and are kept as plain strings.
pub type Records = BTreeMap<String, Zeroizing<String>>;

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum VaultError {
    #[error("vault file is corrupt or has been modified")]
    Corrupt,
    #[error("wrong key or passphrase, or the vault was modified")]
    WrongKey,
    #[error("value, name or vault is too large")]
    TooLarge,
    #[error("invalid secret name")]
    InvalidName,
    #[error("passphrase is too short")]
    WeakPassphrase,
    #[error("vault already exists")]
    Exists,
    #[error("the vault changed on disk or was rotated by another handle; reopen it")]
    Stale,
    #[error("this Argon2 profile is not allowed for new vaults or rotation")]
    ProfileNotAllowed,
    #[error("vault is in use by another process")]
    Busy,
    #[error("vault file is not private (mode/owner/type)")]
    Insecure,
    #[error("vault io error")]
    Io,
    #[error("no vault at that path")]
    NotFound,
    #[error("secret value must not be empty")]
    EmptyValue,
    #[error("entropy source failed")]
    Entropy,
}

/// Allow-listed Argon2id parameters (see the table in VAULT_FORMAT.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Profile {
    /// 19 MiB, t=2. Minimum; import and tests only.
    Minimum = 1,
    /// 64 MiB, t=3. Default for new vaults.
    Standard = 2,
    /// 256 MiB, t=3.
    Strong = 3,
}

impl Profile {
    pub(crate) fn from_u8(b: u8) -> Option<Self> {
        match b {
            1 => Some(Profile::Minimum),
            2 => Some(Profile::Standard),
            3 => Some(Profile::Strong),
            _ => None,
        }
    }

    fn cost(self) -> (u32, u32, u32) {
        match self {
            Profile::Minimum => (19_456, 2, 1),
            Profile::Standard => (65_536, 3, 1),
            Profile::Strong => (262_144, 3, 1),
        }
    }

    pub(crate) fn derive(self, passphrase: &[u8], salt: &[u8; 16]) -> Result<Key32, VaultError> {
        use argon2::{Algorithm, Argon2, Params, Version};
        let (m, t, p) = self.cost();
        let params = Params::new(m, t, p, Some(32)).map_err(|_| VaultError::Corrupt)?;
        let mut out = Zeroizing::new([0u8; 32]);
        let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
        // Caller-owned memory so the password-derived state is wiped when it drops.
        let mut blocks: Zeroizing<Vec<argon2::Block>> =
            Zeroizing::new(vec![argon2::Block::default(); argon.params().block_count()]);
        argon
            .hash_password_into_with_memory(passphrase, salt, out.as_mut(), blocks.as_mut_slice())
            .map_err(|_| VaultError::Corrupt)?;
        Ok(out)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Mode {
    Passphrase { profile: Profile, salt: [u8; 16] },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Header {
    pub file_id: [u8; 16],
    pub mode: Mode,
}

impl Header {
    pub(crate) fn to_bytes(&self) -> Vec<u8> {
        let mut b = Vec::with_capacity(FIXED_HEADER + 17);
        b.extend_from_slice(MAGIC);
        b.push(VERSION);
        let Mode::Passphrase { profile, salt } = &self.mode;
        b.push(2);
        b.extend_from_slice(&self.file_id);
        b.push(*profile as u8);
        b.extend_from_slice(salt);
        b
    }
}

pub(crate) fn random<const N: usize>() -> Result<[u8; N], VaultError> {
    let mut b = [0u8; N];
    getrandom::fill(&mut b).map_err(|_| VaultError::Entropy)?;
    Ok(b)
}

pub(crate) fn new_key() -> Result<Key32, VaultError> {
    let mut k = Zeroizing::new([0u8; 32]);
    getrandom::fill(k.as_mut()).map_err(|_| VaultError::Entropy)?;
    Ok(k)
}

/// Borrow the key (no stack copy); the cipher zeroizes its own state on drop.
fn cipher(key: &[u8; 32]) -> Result<XChaCha20Poly1305, VaultError> {
    let k = <&Key>::try_from(key.as_slice()).map_err(|_| VaultError::Corrupt)?;
    Ok(XChaCha20Poly1305::new(k))
}

fn seal(key: &[u8; 32], aad: &[u8], plaintext: &[u8]) -> Result<([u8; 24], Vec<u8>), VaultError> {
    let nonce = random::<24>()?; // fresh for every seal
    let c = cipher(key)?;
    let ct = c
        .encrypt(
            &XNonce::from(nonce),
            Payload {
                msg: plaintext,
                aad,
            },
        )
        .map_err(|_| VaultError::Corrupt)?;
    Ok((nonce, ct))
}

fn open(
    key: &[u8; 32],
    nonce: &[u8; 24],
    aad: &[u8],
    ct: &[u8],
) -> Result<Zeroizing<Vec<u8>>, VaultError> {
    let c = cipher(key)?;
    c.decrypt(&XNonce::from(*nonce), Payload { msg: ct, aad })
        .map(Zeroizing::new)
        .map_err(|_| VaultError::WrongKey)
}

pub(crate) fn valid_name(n: &str) -> bool {
    (1..=MAX_NAME).contains(&n.len())
        && n.bytes()
            .all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'.' | b'_' | b'-'))
}

/// Canonical payload: compact JSON, sorted keys. Decoding re-encodes and compares, which
/// rejects duplicate keys, whitespace, reordering and any non-canonical form in one check.
fn encode_records(r: &Records) -> Result<Zeroizing<Vec<u8>>, VaultError> {
    let plain: BTreeMap<&str, &str> = r.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    let out = Zeroizing::new(serde_json::to_vec(&plain).map_err(|_| VaultError::Corrupt)?);
    if out.len() > MAX_PLAINTEXT {
        return Err(VaultError::TooLarge);
    }
    Ok(out)
}

fn decode_records(bytes: &[u8]) -> Result<Records, VaultError> {
    let recs: Records = serde_json::from_slice(bytes).map_err(|_| VaultError::Corrupt)?;
    if recs.len() > MAX_RECORDS
        || recs
            .iter()
            .any(|(k, v)| !valid_name(k) || v.len() > MAX_VALUE)
        || encode_records(&recs)?.as_slice() != bytes
    {
        return Err(VaultError::Corrupt);
    }
    Ok(recs)
}

/// Seal a whole vault. A fresh data key is supplied by the caller (always new on rotation).
pub(crate) fn encode(
    header: &Header,
    master: &[u8; 32],
    data_key: &[u8; 32],
    records: &Records,
) -> Result<Vec<u8>, VaultError> {
    let hb = header.to_bytes();
    let (wn, wct) = seal(master, &hb, data_key)?;
    let mut out = hb;
    out.extend_from_slice(&wn);
    out.extend_from_slice(&wct);
    let plain = encode_records(records)?;
    let (pn, pct) = seal(data_key, &out, &plain)?; // AAD = header || wrapped key
    out.extend_from_slice(&pn);
    out.extend_from_slice(
        &u32::try_from(pct.len())
            .map_err(|_| VaultError::TooLarge)?
            .to_be_bytes(),
    );
    out.extend_from_slice(&pct);
    Ok(out)
}

struct Cursor<'a>(&'a [u8]);

impl<'a> Cursor<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], VaultError> {
        if self.0.len() < n {
            return Err(VaultError::Corrupt);
        }
        let (a, b) = self.0.split_at(n);
        self.0 = b;
        Ok(a)
    }

    fn arr<const N: usize>(&mut self) -> Result<[u8; N], VaultError> {
        self.take(N)?.try_into().map_err(|_| VaultError::Corrupt)
    }
}

pub(crate) fn parse_header(bytes: &[u8]) -> Result<Header, VaultError> {
    if bytes.len() > MAX_FILE {
        return Err(VaultError::TooLarge);
    }
    let mut c = Cursor(bytes);
    if &c.arr::<4>()? != MAGIC || c.arr::<1>()?[0] != VERSION {
        return Err(VaultError::Corrupt);
    }
    let mode = c.arr::<1>()?[0];
    let file_id = c.arr::<16>()?;
    let mode = match mode {
        2 => {
            let profile = Profile::from_u8(c.arr::<1>()?[0]).ok_or(VaultError::Corrupt)?;
            Mode::Passphrase {
                profile,
                salt: c.arr::<16>()?,
            }
        }
        _ => return Err(VaultError::Corrupt),
    };
    Ok(Header { file_id, mode })
}

/// Open a vault. `master_for` runs only after the header passed structural validation
/// (known mode, allow-listed profile) and before any decryption.
pub(crate) fn decode(
    bytes: &[u8],
    master_for: impl FnOnce(&Header) -> Result<Key32, VaultError>,
) -> Result<(Header, Key32, Records), VaultError> {
    let header = parse_header(bytes)?;
    let hlen = header.to_bytes().len();
    let mut c = Cursor(bytes);
    c.take(hlen)?;
    let wn = c.arr::<24>()?;
    let wct = c.take(WRAPPED_LEN)?;
    let wrapped_end = hlen + 24 + WRAPPED_LEN;
    let pn = c.arr::<24>()?;
    let plen = u32::from_be_bytes(c.arr::<4>()?) as usize;
    if !(2 + TAG..=MAX_PLAINTEXT + TAG).contains(&plen) {
        return Err(VaultError::Corrupt);
    }
    let pct = c.take(plen)?;
    if !c.0.is_empty() {
        return Err(VaultError::Corrupt); // trailing bytes
    }
    let master = master_for(&header)?;
    let dk = open(&master, &wn, &bytes[..hlen], wct)?;
    let data_key: Key32 =
        Zeroizing::new(dk.as_slice().try_into().map_err(|_| VaultError::Corrupt)?);
    let plain = open(&data_key, &pn, &bytes[..wrapped_end], pct)?;
    let records = decode_records(&plain)?;
    Ok((header, data_key, records))
}

#[cfg(test)]
mod tests;
