use crate::EgressError;
use zeroize::Zeroizing;

const MAX_LEN: usize = 4096;

/// An opaque bearer secret. Deliberately no `Debug`, `Display`, `Clone` or `Serialize`; the
/// bytes are wiped on drop. Only `Egress` reads it, to build the `Authorization` header.
pub struct Credential(Zeroizing<Vec<u8>>);

impl Credential {
    /// Accepts visible ASCII only (0x21..=0x7e), 1 to 4096 bytes, which covers GitHub tokens and
    /// rules out CR, LF, NUL, space and non-ASCII. The input is wiped even when refused.
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self, EgressError> {
        let bytes = Zeroizing::new(bytes);
        if bytes.is_empty()
            || bytes.len() > MAX_LEN
            || !bytes.iter().all(|b| (0x21..=0x7e).contains(b))
        {
            return Err(EgressError::Blocked);
        }
        Ok(Self(bytes))
    }

    pub(crate) fn expose(&self) -> &[u8] {
        &self.0
    }
}
