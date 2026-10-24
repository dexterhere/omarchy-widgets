# Try it safely (10 minutes)

This guide lets you run the whole chain on your own machine without risking your desktop: the background service (`owd`), the installer (`ow install`) and the desktop plugin that draws widgets. Everything happens in one throwaway folder under `/tmp`. Your real shell, your `~/.config`, your keyring and your Hyprland setup are never touched.

Last run end to end by the maintainers: 2026-10-24, Omarchy 4.0.6, Quickshell 0.3.2, against the live website. Steps 1 to 9 passed. The three checks in step 10 need a person and are listed with their results so far.

## Words used here

| Word | Meaning |
|---|---|
| `owd` | The daemon: a small background program. It reads your system data (time, CPU, battery) and hands each widget only what you allowed. |
| `ow` | The command line tool. `ow install` downloads a widget and asks you before it gets any access. |
| Spec widget | A widget made of three small JSON text files. It contains no program code. |
| Grant | Your written permission for one widget to read named data, for example `battery.status`. |
| Sandbox | The throwaway folder this guide makes. Deleting it undoes everything. |
| Offscreen | Drawing into a picture file instead of onto your screen. |

## What you need

- Omarchy (Arch Linux with Quickshell). Quickshell is only needed for step 8.
- `git`, `cargo` (Rust) and `curl`.
- Internet. `ow install` downloads widget files from `raw.githubusercontent.com` and the list of widgets from `omarchy-widgets-web.vercel.app`.

## Rules for this guide

- Use **one terminal** and keep it open. Do not open a second one for steps 2 to 9, because the safety settings only live in the terminal where you set them.
- Do not run `omarchy plugin add`, `systemctl --user` or anything that restarts your shell. Nothing here needs them.
- If a command asks for `sudo`, stop. None of these steps need it.

## 1. Get the code and build it (about 2 minutes)

```bash
git clone https://github.com/dexterhere/omarchy-widgets.git
cd omarchy-widgets
cargo build --release -p owd -p ow --locked
```

You should see: `Finished release profile [optimized] target(s) in ...`. This proves the code builds on your machine. It writes only inside the `omarchy-widgets` folder.

## 2. Make the sandbox

```bash
S=$(mktemp -d -p /tmp ow-try.XXXX)
mkdir -p $S/config $S/data $S/state $S/run && chmod 700 $S/run
cat > $S/env.sh <<EOF
export XDG_CONFIG_HOME=$S/config XDG_DATA_HOME=$S/data XDG_STATE_HOME=$S/state XDG_RUNTIME_DIR=$S/run
EOF
. $S/env.sh
echo "$XDG_DATA_HOME"
```

You should see a path like `/tmp/ow-try.Ab3d/data`.

**Stop here if it does not start with `/tmp/ow-try.`** The last line is your safety check: it proves every program you start from now on writes into the sandbox and not into your home folder. (Keep the path short: socket paths are limited to 108 characters.)

## 3. Start the daemon

```bash
./target/release/owd >"$S/owd.log" 2>&1 &
echo $! > "$S/owd.pid"
sleep 1
cat "$S/owd.log"
./target/release/ow status
```

You should see:

```
owd 0.0.0 listening
{"v":0,"id":1,"ok":true,"daemonVersion":"0.0.0","protocol":0}
```

Now ask `ow` to check the wiring (it only reads and prints):

```bash
./target/release/ow doctor
```

Expect `OK` for `state-dir`, `socket-dir`, `daemon`, `version` and `vault-state`, and `WARN` for the things this guide deliberately does not set up: the data and config directories (they appear at step 4), `vault-file`, `unit` and `plugin`. Each `WARN` line has a `fix:` line; none of them is needed here. `ow doctor` exits 0 as long as nothing says `FAIL`. It shows only how many secrets the vault holds; add `--names` if you want the names, and think before pasting that into a bug report. (The `plugin` line looks at the plugin folder under your real home, read-only; it changes nothing.)

This proves the daemon runs and answers on a private socket inside the sandbox. The vault (encrypted password store) is not needed for these widgets, so we do not create one.

## 4. Install the clock

```bash
./target/release/ow install io.github.dexterhere.clock --allow-planned
```

`--allow-planned` is needed because the website still lists this widget as `planned` (not yet released). Without it `ow` refuses, which is on purpose (step 7).

`ow` downloads the three files, checks them, then prints what the widget will be allowed to do and waits. You should see:

```
Clock 0.1.0 (io.github.dexterhere.clock)
  licence MIT, status planned, review tier community
  source https://github.com/dexterhere/omarchy-widget-clock at commit 471d822bf16e39e4a7ad08176097e2f5626c8874
  registry omarchy-widgets-web.vercel.app
  NOTE: this widget is only planned. It will not show data until the needed provider exists.

This is a spec widget: JSON only. Nothing in it is run. It cannot declare network, file or program access, and this one declares none.

It will be allowed to read, through the daemon and field by field:
  provider system: topics system.time; fields hm, hms, date, tz

Credentials: none involved.

What this does not mean:
  - The daemon knows a widget only by the id it claims. Another program running as you could claim this id too, so the grant limits honest widgets and keeps credentials out of reach; it is not a barrier against your other programs.
  - ow checked the files against the website's rules and the registry's pinned commit. That is not a review of the widget by a person (tier: community).
  - The registry index carries no file hashes, so the files could not be matched to the build that checked them.

It will write the widget files to /tmp/ow-try.Ab3d/data/owd/widgets/io.github.dexterhere.clock and add this to /tmp/ow-try.Ab3d/config/owd/grants.toml:
  widget io.github.dexterhere.clock  topics [system.time]  fields [date, hm, hms, tz]
  file layout.json sha256 00dd840e...
  ...
Type yes to install with this access:
```

How to read it: the line starting `It will be allowed to read` is the whole permission. The clock may read the time, the date and the time zone, nothing else. The "NOTE ... only planned" line is accurate about the website's label, not about the daemon: the daemon already serves clock, battery and system monitor data. Type `yes` and press Enter. You should see:

```
Installed io.github.dexterhere.clock 0.1.0.
The running daemon reloaded the grants.
```

Run `./target/release/ow doctor` again: the data directory, `grants` and the widget line (`OK    widget io.github.dexterhere.clock   Files are intact and valid.`) now appear, and nothing says `FAIL`.

This proves: the real download from the website and GitHub works, the files pass the checks, you were asked before any access was given, and the running daemon picked up the new permission without a restart. Typing anything other than `yes` prints `Not installed.` and changes nothing.

## 5. Install the battery and system monitor

```bash
./target/release/ow install io.github.dexterhere.battery --allow-planned
./target/release/ow install io.github.dexterhere.system-monitor --allow-planned
```

Read each summary and type `yes`. The permissions are:

```
battery:         provider battery: topics battery.status; fields percent, state, timeLeft
system-monitor:  provider system: topics system.cpu, system.memory, system.load, system.temp; fields usage_pct, used_pct, used_mib, total_mib, load1, load5, load15, celsius
```

Now look at what was recorded:

```bash
./target/release/ow list
cat "$S/config/owd/grants.toml"
```

You should see three lines in `ow list` (id, version, commit, status, tier, install time) and three `[[grant]]` blocks. This proves the permissions are plain text you can read and edit, and they sit in the sandbox.

## 6. Check that `ow` refuses what it should

```bash
./target/release/ow install io.github.dexterhere.clock
./target/release/ow install io.github.dexterhere.nope --allow-planned
./target/release/ow install clock --allow-planned
```

You should see, one per command:

```
ow: this widget is only planned (a design concept) and will not show data; pass --allow-planned to install it anyway
ow: this id is not in the registry
ow: this id is not in the registry; ids are the full name, did you mean io.github.dexterhere.clock?
```

Each one exits with an error and installs nothing. This proves `ow` does not install something you did not ask for.

Optional: `./target/release/ow install io.github.dexterhere.docker --allow-planned` installs, but the summary says `The daemon does not serve yet: docker.containers.running, ...`. That is the honest answer: the Docker data provider is not built, so that widget will show "not served".

## 7. Remove and reinstall

```bash
./target/release/ow remove io.github.dexterhere.clock
./target/release/ow list
./target/release/ow install io.github.dexterhere.clock --allow-planned
```

You should see `Removed io.github.dexterhere.clock: files deleted, 1 grant entry.`, a list without the clock, and then the install prompt again (type `yes`). This proves removal deletes both the files and the permission.

## 8. Draw the widgets (offscreen)

```bash
plugin/tests/run-installed.sh "$S" "$S/out.png"
```

This starts a separate, invisible Quickshell, loads the real plugin, reads the installed widgets and the live daemon, and saves a picture. It opens no window and does not touch your running shell. You should see lines like:

```
LINK connected daemon=0.0.0 pushes=10
WIDGET io.github.dexterhere.battery ok topics=battery.status
WIDGET io.github.dexterhere.clock ok topics=system.time
WIDGET io.github.dexterhere.system-monitor ok topics=system.cpu,system.memory,system.load,system.temp
ENTRY io.github.dexterhere.clock system.time live {"date":"Sat 24 Oct","hm":"10:32","hms":"10:32:42","tz":"+0545"}
ENTRY io.github.dexterhere.battery battery.status live {"percent":100,"state":"Full","timeLeft":""}
SAVED true
```

Open `$S/out.png` in any image viewer (for example `xdg-open "$S/out.png"`). You should see one framed tile per widget on a dark background: the clock tile shows your current time and date, the battery tile your battery percent and state, the system monitor tile CPU and memory bars with live percentages.

This proves the full chain: website, `ow install`, files on disk, permission, daemon, plugin, picture. On a desktop PC without a battery the battery tile shows an empty state instead of a percent. That is expected.

Optional, to see what a broken widget looks like: copy an installed widget, change `"type"` in the copy's `layout.json` to `"exec"`, rename the id, run the script again. That tile shows `Widget unavailable` and the others still draw. Delete the copy afterwards (it is inside the sandbox anyway).

## 9. Stop and clean up

```bash
kill "$(cat "$S/owd.pid")"
sleep 1
ls "$S/run/owd"
case "$S" in /tmp/ow-try.*) rm -rf "$S";; esac
```

`kill` ends only the daemon this guide started (its number is in `owd.pid`). `ls` should show only `owd.lock` (the daemon removes its socket `owd.sock` when it stops). The last line deletes the sandbox, but only if the path really starts with `/tmp/ow-try.`. Nothing else was created. The `target/` folder in the repo is the build output and can stay.

If you closed the terminal before this step, run `ls -d /tmp/ow-try.*`, then `kill "$(cat /tmp/ow-try.XXXX/owd.pid)"` with your folder name, then delete that folder.

## 10. Checks that need a person (on the real desktop)

A script cannot see your screen or press your keys. These three checks run a small throwaway test surface (not the plugin) for a few seconds on your live desktop, as a separate Quickshell process with a time limit. They never touch your shell. Read `spikes/README.md` first.

Each run prints the Quickshell log when it ends. Look for the log lines named below.

### 10a. Clicks pass through (S1)

```bash
spikes/run-safe.sh spikes/s1 40
```

For 40 seconds a small translucent blue box (160 by 90) shows on each monitor, under your windows. Click inside the box a few times. Then click on bare wallpaper outside the box on each monitor. At the end, check the log: `S1 CLICK in box on <monitor>` must appear, and `S1 LEAK click outside box` must not.

### 10b. Lock screen hides it (S1)

Run `spikes/run-safe.sh spikes/s1 90`, then lock your session (Omarchy default: `SUPER + CTRL + L`) and look. The blue box must not be visible on the lock screen. Unlock. The run ends by itself.

### 10c. Keyboard focus and Escape (S2)

The shipped test closes itself after 4 seconds, which is too short to click and press a key. Use a longer copy:

```bash
mkdir -p "$S/s2" && cp spikes/s2/*.qml "$S/s2/"
sed -i 's/interval: 4200/interval: 30000/' "$S/s2/shell.qml"
spikes/run-safe.sh "$S/s2" 35
```

A small translucent panel appears near the top left (at 120, 120). Click once inside the panel, then press Escape. The log must contain `S2 ESCAPE -> exit`. Run it again and try the arrow keys after clicking: the box inside should move (the log shows `S2 SAVED` lines).

### Results so far

| Check | What to look for | PASS / FAIL | Measured |
|---|---|---|---|
| Click in box reaches the surface | `S1 CLICK in box on <monitor>` | PASS | 2026-10-24: 2 clicks on eDP-1, 27 on HDMI-A-1 |
| Click outside the box passes through | no `S1 LEAK click outside box` | PASS | 2026-10-24: eDP-1 and HDMI-A-1 |
| Lock screen hides the surface | no blue box on the lock screen | PASS | 2026-10-24: `SUPER + CTRL + L`, 90 s run ended with exit 124 (the time limit) |
| Escape reaches the Overlay surface | `S2 ESCAPE -> exit` | PASS | 2026-10-24: after one click Hyprland sent `wl_keyboard.enter`, three `a` presses arrived (`S2 KEY 65`), then `S2 KEY escape` and `S2 ESCAPE -> exit`. Wait about two seconds after the click before typing. The first two attempts showed no key lines because keys were pressed before the focus event settled. |
| Same, with your result | | ______ | |

Please send your result (PASS or FAIL, the monitor names, and the lines from the log) as an issue. Do not paste anything else from the log.

## 11. Connect GitHub (optional; uses a token you make yourself)

This is the only step that needs an account and a real token, and the only one this guide cannot check for you. `ow connect` itself makes no network call and never opens a browser; the sandbox from step 2 keeps it away from your real config. Skip it if you do not want to create a token.

```bash
./target/release/ow connect github
```

It prints what to create at `https://github.com/settings/personal-access-tokens/new` (fine-grained, read-only: Pull requests: Read-only, Metadata: Read-only, an expiry of 30 to 90 days), offers to create the vault if there is none, asks for the vault passphrase, then for the token twice with nothing shown while you type. It stores the token in the sandbox vault and writes `$XDG_CONFIG_HOME/owd/config.toml` with the binding. It does not test the token and does not restart anything.

The daemon reads `config.toml` only at start, so restart the daemon this guide started (step 9 shows how it was started and stopped), then:

```bash
kill "$(cat "$S/owd.pid")"; sleep 1
./target/release/owd >"$S/owd.log" 2>&1 & echo $! > "$S/owd.pid"; sleep 1
./target/release/ow vault unlock
./target/release/ow connect github --check
```

(On your real desktop the restart is `systemctl --user restart owd.service`; `ow` prints it but never runs it.) `--check` needs an installed widget that holds a `github.prs` grant, because the daemon answers only granted widgets. It prints one line, for example `github.prs: ok open=3 review=1`, or `github.prs: unauthorized` if GitHub rejected the token. `ow connect github --remove` undoes the binding and deletes the secret after you type `yes`.

## Troubleshooting

| What you see | Why | What to do |
|---|---|---|
| `echo "$XDG_DATA_HOME"` is empty or not under `/tmp/ow-try.` | You are in a new terminal, or step 2 was skipped | Run `. /tmp/ow-try.XXXX/env.sh` (your folder name), check again. Do not run `ow install` until it is right. |
| `ow: daemon not reachable: No such file or directory` | The daemon is not running, or you are in a terminal without the sandbox settings | Redo the check above, then step 3. Note `ow install` still works without the daemon; the grants apply when it starts. |
| `ow: installing needs your consent, and consent needs a terminal` | You piped input or ran it from a script | Run it in a normal terminal and type `yes` yourself. To only read the summary, add `--dry-run`. |
| `ow: this widget is only planned ...` | The website still lists it as planned | Add `--allow-planned`. |
| `ow: this id is not in the registry` | Typo. The id is the full name, `io.github.dexterhere.clock`, not `clock` | Copy the id from this guide. |
| `ow: registry index: ...` or a download error | No internet, or the website or GitHub is down | Check `curl -sI https://omarchy-widgets-web.vercel.app/registry.json`, try again later. |
| `ow: --yes is not accepted together with a non-default --registry` | You pointed `--registry` somewhere else and used `--yes` | Leave `--yes` out and read the summary yourself. |
| `owd` exits at once, log says the socket path is too long | The sandbox path is longer than 108 characters | Use the exact `mktemp -d -p /tmp ow-try.XXXX` command; do not nest it. |
| `run-installed.sh: no owd socket` | The daemon is not running | Step 3. |
| `refusing: ... is not a /tmp/ow-try.* sandbox` | The script only draws from a sandbox made in step 2 | Pass `"$S"`. |
| The picture is black or the log says `unavailable` | The widget files did not install, or the daemon is down | Run `ow list`, then step 3, then step 8 again. |
| Tile says `not served` | That data is not built yet (Docker today) | Expected. |
| `quickshell: command not found` | Quickshell is not installed | Step 8 and step 10 need it. Steps 1 to 7 and 9 do not. |
| In 10c the log has no `S2 ESCAPE -> exit` | The keys were pressed before the panel had keyboard focus | Click inside the panel, wait two seconds until the log shows `S2 FOCUS activeFocus=true`, then press Escape. |
| Anything looks wrong on the real desktop during step 10 | The test surface is still showing | It ends by itself at the time limit. Closing the terminal ends it too. |
