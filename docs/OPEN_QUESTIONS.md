# Open questions

Each entry: question, why it matters, proposed experiment, status, answer.

1. Does `omarchy plugin validate` accept extra top-level manifest keys? (S5) Status: answered yes (S5, docs/SPIKES.md).
2. Can one plugin depend on or import another? (S6) Status: answered: possible but unsupported (S6).
3. Can a service plugin create a Bottom-layer surface with free-form input regions without stealing focus or affecting lock behavior? (S1, S2) Status: partly answered (S1, S2): layer, focus and mask measured; click pass-through and lock open, see 18, 19.
4. Which theme singletons and components can third-party QML import, and are they stable? (S8) Status: answered (S8).
5. How far does the manifest settings form go? (S9) Status: answered: schema declared, no renderer shipped (S9).
6. What do `omarchy.monitor` and `omarchy.agents` already expose? (S10) Status: answered (S10).
7. Does `omarchy plugin add` accept a SHA or subdirectory? (S7) Status: answered: no (S7).
8. Can the shell run headless in CI to render previews? (S11) Status: open, unverified.
9. What does the live marketplace contain, and where are the gaps? (S12) Status: not measurable offline (S12), see 21.
10. Is the name acceptable to the Omarchy maintainers? (human task) Status: open, unverified.
11. Keyring behavior under Omarchy 4 autologin: what can another same-uid process read? (S4) Status: open, unverified.
12. Persistent Unix socket from Quickshell, and reconnect across `rescanPlugins`? (S3) Status: answered for Socket, reload and reconnect (S3); real rescanPlugins open, see 20.
13. Is refusal of a different-uid socket peer enforced? Not integration-tested yet: it needs a second uid (try `unshare` or a container). Status: open.
14. Audit-flood residual: a same-uid client can flood denied requests and push real entries out of the audit log after five rotations (about 6 MiB). Status: open.
15. Does the systemd user unit behave as intended when installed? It is analysed offline (exposure 2.5) and as a transient unit, not as an installed unit under `systemctl --user`. Status: open.
16. Authentication open questions (Q1 to Q12) are listed in docs/AUTH_PLAN.md section 8.2: key release details, minimum widget identity for the first token-backed release (D-1), the HTTP/TLS client choice, proxy handling, minimum poll interval, multiple accounts, multi-part secrets, daemon-side vault writes, the Widget Center credential dialog, classic token handling, expiry reporting, GitHub Enterprise. Status: open.
17. Is the official marketplace's automated validation (`omacom/omarchy-plugin-marketplace`) as tolerant of an `ow` manifest block and of data-only plugins as the local `omarchy plugin validate`? (S5 follow-up.) Needs the marketplace validator or a test submission, a human step. Status: open.
18. Edit mode keyboard focus: does an Overlay surface with `Exclusive` keyboard interactivity receive Escape reliably on Hyprland 0.56, and is `OnDemand` enough? Not run on the live session because it would grab the user's keyboard. Proposed experiment: with a person at the keyboard, run `spikes/s2` with `keyboardFocus: Exclusive`. (S2.) Status: open.
19. Click pass-through and lock behavior of the Bottom canvas (the plugin canvas in `plugin/DesktopCanvas.qml` now exists; one live run on 2026-10-24 showed an `owd-canvas` surface at level 1 on eDP-1, clicks not tried): with a person present, click empty wallpaper outside the S1 box (must reach the desktop) and lock the session while it runs. Also test a fullscreen window over the canvas. (S1.) Status: open.
20. Does a `keepLoaded` service really keep its socket across `omarchy-shell shell rescanPlugins`, and do `service` and `overlay` entries of one plugin share the injected `service` property? Read from `shell.qml`, not run inside the real shell. Needs enabling a test plugin in the live shell, so it needs Dexter's approval. (S3, S6.) Status: open.
21. Marketplace reality (S12): no catalog data exists locally. Needs network reads of the official catalog feed, Plugin Depot and omarchyplugins.com. Status: open, needs consent for network.
22. Should code widgets ever be loaded by the renderer from `$XDG_DATA_HOME/owd/widgets` via `Qt.createComponent` (works, S6), bypassing Omarchy's add/enable review? Product and trust decision for Dexter. Status: open.
23. Does Plugin Depot support pinned-SHA installs we can rely on? (S7.) The brief says so; it is not installed here. Status: open, needs network.
24. Does the shell restyle on `omarchy theme set` for values read from `colors.toml` by our adapter (the shell loads that file only at startup)? Re-read trigger via `Color.accent` changes is untested. (S8.) Status: open.
25. `ow install`: how does the catalog snapshot in `crates/ow/data/providers.json` stay in sync with the website's `registry/providers.json`? A drift test against the web repo is not possible from this repo. Proposal: a release checklist item plus a catalog `version` field (HANDOFF web changes). Status: open.
26. `ow install`: should the CLI repeat the registry build's default-branch reachability check (via api.github.com, 60 requests per hour unauthenticated)? Without it a hostile index could name a commit from another repo of the fork network. Today the index is trusted for this. Status: open, needs a decision on a second host and the rate limit.
27. `ow install`: is a signed index (or signed per-widget manifests) wanted before the first non-concept widget ships? Status: open; no signing exists anywhere in the project yet.
28. `ow install`: `removed` is refused even with `--allow-planned` (a deliberate tightening of the brief's wording). Confirm. Status: open, needs Dexter.
29. `ow install` does not enable, place or show the widget (no bridge/canvas yet), and there is no `ow update`. Re-installing the same id with a new commit is the update path and shows the access diff. Status: open until the canvas exists.
30. Does `omarchy-shell` inject `shell`, `manifest`, `service` into the `plugin/` service and overlay entries as read from `shell.qml`, and does the service survive `rescanPlugins` with its socket and tiles? Needs the plugin enabled in the live shell, so it needs Dexter's approval. (SHELL-PLUGIN.) Status: open.
31. Should the renderer report visibility so `owd` stops polling for hidden or covered widgets (fullscreen window over the monitor, lock screen)? The protocol has no visibility op today. Status: open.
32. Does `SplitParser` have an internal line limit? Answered 2026-10-24: no. With the marker `"
"` a peer that never sends a newline grew the shell to 6 GB in 9 s. An empty marker delivers each chunk (about 230 KB) as it arrives and keeps memory flat, so the bridge frames lines itself with a 64 KiB cap and a 4 MiB per second watchdog (`plugin/Bridge.qml`, measured about 100 MiB peak). Residual: Quickshell decodes each chunk as UTF-8 separately, so a multi-byte character split at a chunk boundary shows as a replacement character. Ask upstream for a native line limit and per-read byte access. Status: mitigated, upstream request open.
33. The in-QML validator re-implements the registry and `ow install` checks (Unicode `Cn` unassigned code points and exact NFC rules are approximated). Should the schema be shared as data and tested for parity (golden files) across the three implementations? Status: open.
34. The clock, battery and temperature topics were not served when the plugin was first built. Answered 2026-10-24: after merging develop, `plugin/tests/run-e2e.sh` shows live clock, battery and system-monitor tiles from the real `owd`. Status: answered.
