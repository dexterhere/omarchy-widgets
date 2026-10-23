# Open questions

Each entry: question, why it matters, proposed experiment, status, answer.

1. Does `omarchy plugin validate` accept extra top-level manifest keys? (S5) Status: open, unverified.
2. Can one plugin depend on or import another? (S6) Status: open, unverified.
3. Can a service plugin create a Bottom-layer surface with free-form input regions without stealing focus or affecting lock behavior? (S1, S2) Status: open, unverified.
4. Which theme singletons and components can third-party QML import, and are they stable? (S8) Status: open, unverified.
5. How far does the manifest settings form go? (S9) Status: open, unverified.
6. What do `omarchy.monitor` and `omarchy.agents` already expose? (S10) Status: open, unverified.
7. Does `omarchy plugin add` accept a SHA or subdirectory? (S7) Status: open, unverified.
8. Can the shell run headless in CI to render previews? (S11) Status: open, unverified.
9. What does the live marketplace contain, and where are the gaps? (S12) Status: open, unverified.
10. Is the name acceptable to the Omarchy maintainers? (human task) Status: open, unverified.
11. Keyring behavior under Omarchy 4 autologin: what can another same-uid process read? (S4) Status: open, unverified.
12. Persistent Unix socket from Quickshell, and reconnect across `rescanPlugins`? (S3) Status: open, unverified.
13. Is refusal of a different-uid socket peer enforced? Not integration-tested yet: it needs a second uid (try `unshare` or a container). Status: open.
14. Audit-flood residual: a same-uid client can flood denied requests and push real entries out of the audit log after five rotations (about 6 MiB). Status: open.
15. Does the systemd user unit behave as intended when installed? It is analysed offline (exposure 2.5) and as a transient unit, not as an installed unit under `systemctl --user`. Status: open.
16. Authentication open questions (Q1 to Q12) are listed in docs/AUTH_PLAN.md section 8.2: key release details, minimum widget identity for the first token-backed release (D-1), the HTTP/TLS client choice, proxy handling, minimum poll interval, multiple accounts, multi-part secrets, daemon-side vault writes, the Widget Center credential dialog, classic token handling, expiry reporting, GitHub Enterprise. Status: open.
