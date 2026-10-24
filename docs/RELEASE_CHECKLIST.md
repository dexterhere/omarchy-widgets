# Release checklist: v0.1.0 and `alpha` widgets

What must be true before the first tagged release (`v0.1.0`), and before the website marks `clock`, `battery` and `system-monitor` as `alpha` (today all three are `planned`). State is from evidence gathered on 2026-10-24 (Omarchy 4.0.6, Quickshell 0.3.2, Hyprland 0.56.2). Walk-through that produced most of the evidence: [TRY_IT.md](TRY_IT.md).

States: **DONE** (shown by a command or a measurement), **PARTIAL**, **OPEN** (not started or not possible yet), **PERSON** (only the maintainer can do it).

## A. Already shown to work

| Item | State | Evidence |
|---|---|---|
| Workspace builds, tests and lints | DONE | `cargo fmt --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings` clean; `cargo test --workspace --locked`: 351 passed, 0 failed |
| `owd` serves `system.time`, `system.cpu`, `system.memory`, `system.load`, `system.temp`, `battery.status` | DONE | Live pushes read by the plugin in a sandbox (TRY_IT step 8) |
| `ow install` against the live website and raw.githubusercontent.com, for clock, battery, system-monitor | DONE | Real fetch, validation, consent prompt, grant written, running daemon reloaded; also `list`, `remove`, reinstall, refusals |
| Plugin draws the installed widgets from the live daemon | DONE | Offscreen PNG: clock `10:32 / Sat 24 Oct`, battery `100% Full`, CPU `10.3%`, memory `73.9%`; a broken widget shows `Widget unavailable` |
| Click pass-through of the Bottom canvas | DONE | Person, 2026-10-24, eDP-1 and HDMI-A-1 (S1) |
| Lock screen hides the Bottom canvas | DONE | Person, 2026-10-24 (S1) |

## B. Before `v0.1.0`

| # | Item | State | Evidence and what is left |
|---|---|---|---|
| B1 | Package builds from a signed tag | OPEN, PERSON | `packaging/PKGBUILD` has `validpgpkeys=()` and `prepare()` refuses to build. The maintainer must create a release key, put its 40-hex fingerprint in the PKGBUILD, publish the public key, and sign the tag with `git tag -s v0.1.0`. Nothing is tagged yet (`git tag` is empty). |
| B2 | Package contents are right | PARTIAL | 2026-10-24: `makepkg --nocheck --noconfirm -f` (everything under `/tmp`, nothing installed) built `omarchy-widgets-git` from `packaging/PKGBUILD-git`; `bsdtar -tvf` lists exactly `/usr/bin/owd` and `/usr/bin/ow` (0755, stripped release builds from `target/release`), `/usr/lib/systemd/user/owd.service` (0644, identical to the repo file), licence and docs under `/usr/share/{licenses,doc}`, and a `.INSTALL` that prints text only. Found and fixed on the way: makepkg LTO broke linking `ring` (`options=(!lto)`). LEFT: the release `PKGBUILD` cannot be built until B1; `namcap` is not installed on the build machine; `pacman -Qlp` was not used (it needs no root but `bsdtar` shows the same list). See `docs/PACKAGING.md`. |
| B3 | The unit file is valid and tight | DONE | 2026-10-24: `systemd-analyze --user security --offline=yes packaging/owd.service` -> **2.3 OK** (before and after the data-directory fix); `systemd-analyze --user verify` -> only `/usr/bin/owd is not executable` (expected before install). The header comment now states the measured score, the data-directory mechanism and every deliberately absent directive. Both commands only read the file. This says nothing about behaviour as an installed unit: that is B4. |
| B4 | The unit works as an **installed** unit | OPEN, PERSON | Verified so far only as a transient unit with throwaway directories. Safe way to verify, by the maintainer, once: (1) install the package from B2 on a spare machine or VM, not the daily desktop; (2) `systemctl --user start owd` then `ow status` must answer and `journalctl --user -u owd` must show `listening`; (3) `systemctl --user stop owd` must remove `$XDG_RUNTIME_DIR/owd/owd.sock`; (4) confirm the daemon can still read `~/.config/owd/grants.toml` and that `ow install` while it runs reloads grants (`ReadWritePaths` and `ProtectHome=read-only` could block this); (5) create `~/.local/share/owd` first (`install -d -m 0700`, or `packaging/install-local.sh`): the unit now lists it in `ReadWritePaths=` WITHOUT a `-` prefix, so a missing directory fails the start with 226/NAMESPACE; (6) run `ow doctor`. Items (1) to (6) have still not been done by anyone. |
| B5 | The daemon starts by itself with the session | PARTIAL | Decided: a package does not enable services or touch `$HOME`; the `.install` message and `docs/PACKAGING.md` tell the person to run `systemctl --user enable --now owd.service`, and `ow doctor` warns when the enable link is missing (file check only). Not verified on a real session (B4). |
| B6 | Plugin loadable from its own repo | PARTIAL, PERSON | Done: `scripts/mirror-plugin.sh` exports `plugin/` (minus tests and scratch) from a git ref plus README, LICENSE and `MIRROR.md`; it validates with `omarchy plugin validate` and the static check, and `scripts/tests/mirror-plugin.test.sh` covers its refusals. Recipe, one-time repo setup and retraction: `docs/PLUGIN_RELEASE.md`. Left: create `dexterhere/omarchy-widget-shell` with the protections, push the first tagged release, and run `omarchy plugin add` on it once (nobody has: the command in the mirror README is untested). |
| B7 | In-shell enable test with the person present | OPEN, PERSON | Never run in the real shell. See section E. |
| B8 | Escape and keyboard focus on an overlay surface | DONE | 2026-10-24, measured by a person: a click gives an `OnDemand` overlay surface the keyboard and Escape reaches it (SPIKES S2). Procedure: TRY_IT step 10c. |
| B9 | Security review pass | OPEN | CLAUDE.md and `docs/HANDOFF.md`: an external review of the vault and the protocol is a gate for public beta. Not started. Before that, the maintainer should run the in-repo pass in section F. |
| B10 | `cargo deny check` and `cargo audit` green on the tagged commit | PARTIAL | Both run in CI; not run in this check. Run them on the release commit. |
| B11 | Actions pinned | PARTIAL | `ci.yml` pins cargo-deny-action by SHA; `docs/HANDOFF.md` says other actions use tags. Pin all of them before a release workflow handles a signing key. |
| B12 | Name and host | OPEN, PERSON | The project name is a placeholder. The compiled-in registry host is `omarchy-widgets-web.vercel.app`; `omarchy-widgets.vercel.app` serves the same bytes today (HTTP 200, 89901 bytes each). Pick one, then change `DEFAULT_REGISTRY` in `crates/ow/src/install/mod.rs` and the tests that count it. |
| B13 | Plugin test flake | OPEN | `plugin/tests/run-all.sh` reported one failing check on two full runs while the machine load average was about 10: `bridge burst` ("endless valid lines hit the byte-rate watchdog"). It passes when run alone (`plugin/tests/run-bridge-tests.sh burst`). No plugin code was changed. Make the test tolerant of load, or measure the bytes sent instead of the timing, before using `run-all.sh` as a release gate. |
| B14 | Release notes say what is and is not protected | OPEN | Reuse the three "What this does not mean" lines of the install summary. Never call code widgets sandboxed. |

## C. Before the website marks clock, battery and system-monitor `alpha`

| # | Item | State | Evidence and what is left |
|---|---|---|---|
| C1 | Registry entries carry real pinned commits | DONE | All seven have 40-hex commits that resolve on raw.githubusercontent.com (clock `471d822b`, battery `cc583259`, system-monitor `dc131190`). |
| C2 | `ow install` works against those commits | DONE | See section A. |
| C3 | Per-file sha256 in the index | OPEN (web change) | The index has no hash fields (checked: no key containing `sha` or `hash` on any entry). `ow` says so in the install summary ("The registry index carries no file hashes") and computes hashes itself from what it fetched, so today the pin is the commit only. Web repo change, not made here: in `scripts/build-registry.ts` add `"sha256": {"manifest.json": "<64 hex>", "widget.json": "<64 hex>", "layout.json": "<64 hex>"}` to each entry, computed over the exact bytes it validated; update `registry/README.md` and `src/lib/registry/types.ts`. `ow` already verifies them when present. After that, test that a tampered file is refused. Hashes `ow` computed in this run, for comparison once the index has them: clock `layout 00dd840e…, manifest f5e36d09…, widget c3b228fb…`; battery `layout 36a3f204…, manifest 2f4179e4…, widget 3c93a5b4…`; system-monitor `layout 6e1c2030…, manifest 69ebcac5…, widget cbb76766…`. |
| C4 | Status moves from `planned` to `alpha` with `installable: true` | OPEN (web change) | Today `ow install` refuses without `--allow-planned` and prints "only planned ... will not show data", which is wrong for these three (the daemon serves their data). Flip the three entries only after B4 to B7, so "alpha" is true on a normal machine. |
| C5 | The three widgets work on a machine without a battery or with a different CPU sensor | PARTIAL | Battery answers `unavailable` on desktops by design (`docs/HANDOFF.md`); not tested on a desktop PC. `system.temp` read 67 and 68 degrees here; machines without that sensor are untested. |
| C6 | Catalog snapshot matches the website | PARTIAL | `crates/ow/data/providers.json` already lists the `serving` fields for `system` and `battery`, so no false "not served" warning appeared for these three. The website does not publish `providers.json` at a URL (404), so drift cannot be detected from `ow`; add a version to the catalog (web change) and a CI check. |
| C7 | Docker widget stays `planned` | DONE | The summary already warns "The daemon does not serve yet: docker.containers.*"; the tile says `not served`. |

## D. Plugin in its own repo (script done, repo not created)

State: built as a manual script, not a workflow (DECISIONS PLUGIN-MIRROR); see `docs/PLUGIN_RELEASE.md`. The proposal below is kept for the reasoning; where it says "mirror workflow" and "deploy key", the decision is the opposite.

**Why.** `omarchy plugin add <url>` runs `git clone`, requires `manifest.json` at the clone root, and always follows the default branch (S7). Our plugin's manifest is `plugin/manifest.json` inside the main repo, so it cannot be installed that way. It also cannot be pinned, so `omarchy plugin update` moves users to the latest default branch.

**Proposed split.**

- New repo, for example `dexterhere/omarchy-widgets-plugin` (name is the maintainer's choice; the plugin id `io.github.dexterhere.widget-shell` stays). Its root holds `manifest.json`, `*.qml`, `*.js`, `qmldir`, `relative/`. Not the tests.
- The main repo stays the source of truth. The plugin tests (`plugin/tests/`) stay here. To keep them out of the mirror, either move them to `plugin-tests/` (they copy `plugin/` and delete `tests/` already) or filter them in the mirror step.
- Mirror workflow, in the main repo, runs on a signed tag `v*` only (not on every push, so users on the default branch of the mirror only ever see released code): check out the tag, run `plugin/tests/run-all.sh`, copy `plugin/` without `tests/` into a clean checkout of the mirror, commit with the tag name in the message, push the commit and the tag. Use a deploy key limited to the mirror repo, stored as a repository secret; never a personal token. `git subtree split --prefix=plugin` is the zero-dependency alternative but carries the tests and gives different commit ids each time, so prefer the copy.
- The mirror's default branch is what users get, so protect it: no direct pushes, only the workflow.
- Document two install paths: `omarchy plugin add <mirror url> --enable` (person) and the version check: `ow doctor` (not built) should compare the installed plugin's `manifest.json` `version` with the protocol the daemon speaks.

**Creating the repo, the deploy key and the secret are human-only tasks** (see CLAUDE.md).

## E. In-shell enable test (with the person present)

Never done. Do it once, on the real desktop, with the maintainer at the keyboard, after B6:

1. Back up `~/.config/omarchy/shell.json` and note its `stat`.
2. Have the daemon running (B5) and the three widgets installed with `ow install`.
3. `omarchy plugin add <mirror url> --enable` (the maintainer reads the code first; it runs unsandboxed).
4. Look for: the three tiles on the Bottom layer on each monitor; clicks on the wallpaper still work; the plugin does not call `Qt.quit()`; `quickshell` for the shell is still the same process; Omarchy logs show no errors from the plugin.
5. Rollback: `omarchy plugin disable` and `omarchy plugin remove` for this id, restore the backup. Also confirm that stopping `owd` shows the offline tile and starting it recovers by itself.
6. Check the things the design marks "untested": `service` and `overlay` property injection, that `keepLoaded` keeps the socket across `rescanPlugins`, theme re-read on `omarchy theme set`, behavior with a fullscreen window.

## F. Security review pass (in-repo, before the external one)

| Item | State |
|---|---|
| Install path: host allow-list (only the registry host and raw.githubusercontent.com), size caps, strict JSON (duplicate keys, depth), id and path checks | DONE by tests (`crates/ow/tests/install_*.rs`); the real fetch of three widgets raised no refusal |
| Consent: summary shown before any write; `--yes` needs the access hash and is refused with a non-default registry | DONE by tests; prompt exercised on a pty |
| Grants file is mode 0600 and edited atomically | DONE (checked: `600`) |
| Grant is limited to the declared topics and fields | DONE (summary matches `grants.toml` exactly) |
| A same-user program can claim any widget id | KNOWN, stated in the install summary and the threat model |
| Plugin never executes widget content: unknown node types rejected | DONE (`Widget unavailable` tile; static check in `run-all.sh`) |
| Fuzz targets run on the release commit | OPEN |
| External review of vault and protocol | OPEN, PERSON (gate for public beta) |

## G. Release steps (maintainer)

1. Everything in B marked DONE or consciously deferred in the release notes.
2. `git tag -s v0.1.0`, push the tag (human action).
3. Build the package from the tag, check contents (B2), install on a spare machine (B4).
4. Mirror the plugin with `scripts/mirror-plugin.sh` (`docs/PLUGIN_RELEASE.md`), run section E.
5. Web repo: add file hashes (C3), flip the three entries to `alpha` (C4), refresh `crates/ow/data/providers.json` if the catalog changed.
