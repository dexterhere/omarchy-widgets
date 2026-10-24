//! Who is on the other end of a Unix-domain socket: the one place that compares peer credentials
//! with our own uid, so the daemon and the CLI cannot drift apart and the rule has a test.
use std::os::fd::AsFd;

/// True only when the kernel reports the peer's uid and it is ours. A failed lookup is "no":
/// an unknown peer is never trusted.
pub fn peer_is_self(socket: &impl AsFd) -> bool {
    same_uid(
        rustix::net::sockopt::socket_peercred(socket)
            .ok()
            .map(|c| c.uid.as_raw()),
        rustix::process::geteuid().as_raw(),
    )
}

fn same_uid(peer: Option<u32>, ours: u32) -> bool {
    peer == Some(ours)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_our_own_uid_passes_and_a_failed_lookup_fails_closed() {
        assert!(same_uid(Some(1000), 1000));
        assert!(!same_uid(Some(0), 1000));
        assert!(!same_uid(Some(1001), 1000));
        assert!(!same_uid(None, 1000));
        // Root asking about itself must not pass a check that was made for uid 1000.
        assert!(!same_uid(Some(1000), 0));
    }

    #[test]
    fn a_socket_from_our_own_process_is_ours() {
        let (a, b) = std::os::unix::net::UnixStream::pair().unwrap();
        assert!(peer_is_self(&a) && peer_is_self(&b));
        // Not a socket at all: the credential lookup fails and so does the check.
        let f = std::fs::File::open("/dev/null").unwrap();
        assert!(!peer_is_self(&f));
    }
}
