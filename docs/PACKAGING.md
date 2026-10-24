# Packaging and installing

Three ways to get `owd` (the daemon) and `ow` (the CLI) onto a machine, from most to least official. None of them is verified as an installed unit on a real desktop yet (see "What is verified").

| Way | For | Signed | File |
|---|---|---|---|
| Release package | Everyone, once `v0.1.0` exists | Signed tag only; refuses to build otherwise | `packaging/PKGBUILD` |
| Developer-preview package | Arch users who want to try today | **No.** You trust the branch | `packaging/PKGBUILD-git` |
| Local installer | Anyone without pacman | No | `packaging/install-local.sh` |

## Why there are two PKGBUILDs

`PKGBUILD` is the release recipe. It builds only from a tag whose signature verifies, and `prepare()` refuses to run until the maintainer pins a signing key fingerprint in `validpgpkeys` (with an empty list, `makepkg` would accept a valid signature from any key in your keyring). No key and no tag exist yet (RELEASE_CHECKLIST B1), so it cannot build today, on purpose.

`PKGBUILD-git` exists so people can try the daemon now. It builds the tip of a branch (default `develop`, override with `_branch=main`), unsigned, with `Cargo.lock` enforced. That is the same trust as `git clone && cargo build`: whatever is on the branch when you build is what you get. It `provides`/`conflicts` with `omarchy-widgets`, so the release package replaces it later. Do not hand it to others as "the release".

Both set `options=(!lto)`: makepkg's default LTO flags make the C objects of the `ring` crate unlinkable with rust-lld (seen 2026-10-24).

## What a package installs (and does not)

```
/usr/bin/owd
/usr/bin/ow
/usr/lib/systemd/user/owd.service
/usr/share/licenses/<pkg>/LICENSE, /usr/share/doc/<pkg>/{LICENSE,TRY_IT.md,SECURITY.md,threat-model.md}
```

A package must not touch `$HOME`, and this one does not. The `.install` script only **prints** what you run yourself, as your normal user:

```bash
install -d -m 0700 ~/.local/share/owd      # the unit needs this directory; see below
systemctl --user enable --now owd.service
ow doctor
```

## Build and install the developer-preview package

```bash
cd packaging
makepkg -p PKGBUILD-git          # builds omarchy-widgets-git-<ver>-x86_64.pkg.tar.zst; installs nothing
bsdtar -tvf omarchy-widgets-git-*.pkg.tar.zst     # look at what it would install
sudo pacman -U omarchy-widgets-git-*.pkg.tar.zst  # you run this, deliberately
```

To test a local checkout without the network, point the source at it: `_src="git+file:///abs/path/to/repo#branch=NAME" makepkg -p PKGBUILD-git`. `makepkg --nocheck` skips the test suite (CI runs it). Build in a scratch directory with `PKGDEST`, `SRCDEST` and `BUILDDIR` set if you do not want files next to the recipe.

## Install without pacman

```bash
cargo build --release --locked -p owd -p ow
packaging/install-local.sh            # DRY RUN: prints exactly what it would do, changes nothing
packaging/install-local.sh --yes      # do it
```

Installs `owd` and `ow` to `$PREFIX/bin` (default `~/.local/bin`; `--prefix DIR` or `PREFIX=`), the unit to `$XDG_CONFIG_HOME/systemd/user/owd.service` with `ExecStart` (and the data path) rewritten to your locations, and creates `~/.local/share/owd` and `~/.config/owd` with mode 0700. It never runs `systemctl` or `sudo`; it prints the commands. It refuses to overwrite a file it did not create, or one you changed, unless you pass `--force`; it refuses a symlinked prefix, bin, unit, data or config directory; it checks every target before writing anything, so a refusal never leaves half an install. It records an uninstall manifest at `$XDG_DATA_HOME/omarchy-widgets/install-manifest`.

## Verify

```bash
ow doctor            # read-only; OK / WARN / FAIL / SKIP per check, a fix line each, exit 1 on any FAIL
ow doctor --json     # same, for scripts
```

`ow doctor` looks at files and asks the running daemon for `status` and `vault.status`. It does not call `systemctl`, so "unit enabled" means "the enable link exists". Its plugin check is `SKIP` in `--config-dir` mode and a plain folder check otherwise. Details: `docs/DAEMON_SPEC.md` ("`ow doctor`").

Unit file checks (read the file only):

```bash
systemd-analyze --user security --offline=yes packaging/owd.service   # exposure 2.3 OK (2026-10-24)
systemd-analyze --user verify packaging/owd.service                   # only: the binary path does not exist yet
```

## The data directory

`ProtectHome=read-only` makes the whole home read-only, so the vault directory is opened with `ReadWritePaths=%h/.local/share/owd`, with no `-` prefix: if the directory is missing the service fails to start with a namespace error (226/NAMESPACE) instead of running with a read-only vault directory. systemd has no `DataDirectory=`, and an `ExecStartPre=` that creates the directory would run inside the same read-only sandbox, so the directory is created outside the unit: by the installer, by you (`install -d -m 0700 ~/.local/share/owd`), or by `ow vault init`. If you move `XDG_DATA_HOME`, change the path with `systemctl --user edit owd.service`.

## Uninstall

- Package: `systemctl --user disable --now owd.service`, then `sudo pacman -R omarchy-widgets-git`. Your data is left alone; delete `~/.local/share/owd`, `~/.config/owd`, `~/.local/state/owd` yourself if you want it gone (that deletes your vault).
- Local installer: `systemctl --user disable --now owd.service`, then `packaging/install-local.sh --uninstall` (dry run) and `--uninstall --yes`. It removes only files listed in the manifest whose content still matches (a file you edited is kept unless `--force`), then removes directories it created if they are empty. A data directory that holds a vault is never removed.

## What is verified, and what is not

Verified (2026-10-24, evidence in `docs/RELEASE_CHECKLIST.md`): `PKGBUILD-git` builds with `makepkg` in `/tmp` and the package has the expected files, modes and release-build binaries; the unit passes `systemd-analyze --user verify` and scores 2.3 OK; `install-local.sh` passes its test (`packaging/tests/install-local.test.sh`, fake HOME in `/tmp`); `ow doctor` passes its tests against throwaway trees and daemons.

Not verified: nothing was ever **installed** with pacman, and the unit has never been **run as an installed unit** against a real home directory (RELEASE_CHECKLIST B4). `systemctl` was not used in any of this work. Not tested: the release `PKGBUILD` end to end (no key, no tag), `namcap` (not installed on the build machine), other distributions, a machine with `XDG_DATA_HOME` moved, and whether the systemd user manager sees the same `XDG_*` variables as your shell.
