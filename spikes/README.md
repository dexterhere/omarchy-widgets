# Phase 0 spikes

Throwaway prototypes that turn "believed, verify" claims into measured facts. Results and verdicts are in `docs/SPIKES.md`; this file is only about running the code safely.

## Rules these scripts follow

- They never touch the running Omarchy shell. A spike is a separate Quickshell process started with its own config path (`quickshell -p <dir>`), so it gets a different instance id from `quickshell -n -p /usr/share/omarchy/shell`.
- `run-safe.sh` refuses `/usr/share/omarchy/*` and anything under `~/.config`, points `XDG_STATE_HOME`, `XDG_CACHE_HOME` and `XDG_DATA_HOME` at a temp dir, records the PID it started and signals only that PID, and ends the run with `timeout`.
- Nothing here calls `pkill`, `killall`, `quickshell kill`, `omarchy plugin add|enable|...`, `hyprctl dispatch|keyword|reload` or `systemctl --user`. `hyprctl layers|clients|monitors|activewindow -j` are read-only and are used for the checks.
- A spike surface sits on a live desktop for a few seconds. S1 draws a small translucent box under your windows on each monitor. S2 shows a small translucent overlay at 120,120 for about 4 s and uses on-demand keyboard focus (it takes the keyboard only if you click it). If anything looks wrong, close the terminal; `timeout` ends the process by itself.
- Plugin code is unsandboxed. A real plugin must never call `Qt.quit()` (S2 does, because it is a standalone process); inside the shell that would end the whole desktop shell.

## Run

```
spikes/run-safe.sh spikes/s1 6            # S1: Bottom-layer surface; check `hyprctl layers -j` meanwhile
WAYLAND_DEBUG=client spikes/run-safe.sh spikes/s1 6 2>&1 | grep set_input_region   # see the input region sent to the compositor
(cd spikes/s2 && QT_QPA_PLATFORM=offscreen /usr/lib/qt6/bin/qmltestrunner -input tst_edit.qml)   # S2 logic, no display
spikes/run-safe.sh spikes/s2 7            # S2 live part: Overlay layer + layout.json write
spikes/s3/run.sh ok                       # S3: socket client, hot reload, server restart
spikes/s3/run.sh reject                   # S3: wrong-uid peer is refused
spikes/run-safe.sh spikes/s6 5            # S6: shared singleton across plugin folders
```

Use the Qt 6 tools in `/usr/lib/qt6/bin`. On this machine `/usr/bin/qml`, `qmltestrunner` and `qmllint` are Qt 5.15 and fail silently on Qt 6 files.

Each `evidence.txt` is the trimmed output of the run recorded on 2026-10-23 (Omarchy 4.0.6, Quickshell 0.3.2, Hyprland 0.56.2).
