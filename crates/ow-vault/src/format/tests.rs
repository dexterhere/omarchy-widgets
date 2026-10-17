#![allow(clippy::unwrap_used, clippy::expect_used)]
use super::*;

fn header() -> Header {
    Header {
        file_id: [7; 16],
        mode: Mode::Passphrase {
            profile: Profile::Minimum,
            salt: [9; 16],
        },
    }
}

fn recs() -> Records {
    let mut r = Records::new();
    r.insert("github-main".into(), Zeroizing::new("tok-123".into()));
    r.insert("mail.imap".into(), Zeroizing::new("pw ünï".into()));
    r
}

const MK: [u8; 32] = [1; 32];
const DK: [u8; 32] = [2; 32];

fn good() -> Vec<u8> {
    encode(&header(), &MK, &DK, &recs()).unwrap()
}

fn try_open(b: &[u8]) -> Result<Records, VaultError> {
    decode(b, |_| Ok(Zeroizing::new(MK))).map(|(_, _, r)| r)
}

#[test]
fn round_trip() {
    let b = good();
    let r = try_open(&b).unwrap();
    assert_eq!(r, recs());
}

#[test]
fn every_byte_flip_is_rejected() {
    let b = good();
    for i in 0..b.len() {
        let mut m = b.clone();
        m[i] ^= 0x01;
        assert!(try_open(&m).is_err(), "flip at byte {i} was accepted");
    }
}

#[test]
fn truncation_and_trailing_bytes_rejected() {
    let b = good();
    for n in 0..b.len() {
        assert!(try_open(&b[..n]).is_err(), "truncated to {n}");
    }
    let mut t = b.clone();
    t.push(0);
    assert_eq!(try_open(&t), Err(VaultError::Corrupt));
}

#[test]
fn wrong_master_key_is_wrong_key() {
    let b = good();
    let e = decode(&b, |_| Ok(Zeroizing::new([3u8; 32]))).err().unwrap();
    assert_eq!(e, VaultError::WrongKey);
}

#[test]
fn length_field_bounds() {
    let b = good();
    let at = header().to_bytes().len() + 24 + WRAPPED_LEN + 24;
    for bad in [0u32, 17, MAX_PLAINTEXT as u32 + TAG as u32 + 1, u32::MAX] {
        let mut m = b.clone();
        m[at..at + 4].copy_from_slice(&bad.to_be_bytes());
        assert_eq!(try_open(&m), Err(VaultError::Corrupt), "len {bad}");
    }
}

#[test]
fn unknown_profile_and_mode_rejected_before_kdf() {
    let mut b = good();
    b[5] = 9; // mode
    let called = std::cell::Cell::new(false);
    let r = decode(&b, |_| {
        called.set(true);
        Ok(Zeroizing::new(MK))
    });
    assert!(r.is_err() && !called.get());
    let mut b = good();
    b[4 + 1 + 1 + 16] = 99; // profile
    let r = decode(&b, |_| {
        called.set(true);
        Ok(Zeroizing::new(MK))
    });
    assert!(
        r.is_err() && !called.get(),
        "KDF would have run on an unknown profile"
    );
}

#[test]
fn payload_cannot_be_spliced_onto_another_header_or_older_wrap() {
    let a = encode(&header(), &MK, &DK, &recs()).unwrap();
    let hl = header().to_bytes().len();
    let wrapped_end = hl + 24 + WRAPPED_LEN;
    // Same master key and same data key, but a re-wrap (fresh nonce): old payload must not open.
    let b = encode(&header(), &MK, &DK, &recs()).unwrap();
    let mut spliced = b[..wrapped_end].to_vec();
    spliced.extend_from_slice(&a[wrapped_end..]);
    assert!(try_open(&spliced).is_err());
    // Different file_id.
    let mut h2 = header();
    h2.file_id = [8; 16];
    let c = encode(&h2, &MK, &DK, &recs()).unwrap();
    let mut spliced = c[..wrapped_end].to_vec();
    spliced.extend_from_slice(&a[wrapped_end..]);
    assert!(try_open(&spliced).is_err());
}

#[test]
fn noncanonical_payloads_rejected() {
    let hb = header().to_bytes();
    for plain in [
        br#"{"a":"1","a":"2"}"#.as_slice(),
        br#"{ "a":"1" }"#,
        br#"{"b":"1","a":"2"}"#,
        br#"{"A":"1"}"#,
        br#"{"a":1}"#,
        br#"[]"#,
    ] {
        let (wn, wct) = seal(&MK, &hb, &DK).unwrap();
        let mut out = hb.clone();
        out.extend_from_slice(&wn);
        out.extend_from_slice(&wct);
        let (pn, pct) = seal(&DK, &out, plain).unwrap();
        out.extend_from_slice(&pn);
        out.extend_from_slice(&(pct.len() as u32).to_be_bytes());
        out.extend_from_slice(&pct);
        assert_eq!(
            try_open(&out),
            Err(VaultError::Corrupt),
            "{}",
            String::from_utf8_lossy(plain)
        );
    }
}

#[test]
fn nonces_are_fresh_on_every_seal() {
    let a = good();
    let b = good();
    let hl = header().to_bytes().len();
    assert_ne!(a[hl..hl + 24], b[hl..hl + 24], "wrap nonce reused");
    let pl = hl + 24 + WRAPPED_LEN;
    assert_ne!(a[pl..pl + 24], b[pl..pl + 24], "payload nonce reused");
}

#[test]
fn name_rules() {
    for ok in ["a", "github-main", "x.y_z-1"] {
        assert!(valid_name(ok));
    }
    for bad in ["", "A", "a b", "a/b", "é", &"a".repeat(65)] {
        assert!(!valid_name(bad), "{bad}");
    }
}

proptest::proptest! {
    #[test]
    fn arbitrary_records_round_trip(
        m in proptest::collection::btree_map("[a-z0-9._-]{1,64}", ".{0,200}", 0..20)
    ) {
        let r: Records = m.into_iter().map(|(k, v)| (k, Zeroizing::new(v))).collect();
        let b = encode(&header(), &MK, &DK, &r).unwrap();
        proptest::prop_assert_eq!(try_open(&b).unwrap(), r);
    }
}
