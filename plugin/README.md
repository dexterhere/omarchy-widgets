# Widget layer plugin (preview, first slice)

An Omarchy shell plugin that draws installed **spec widgets** (JSON only, `layout.json` v0) on the desktop with live data from the `owd` daemon. This is the first slice: bridge, renderer and a minimal canvas. It is not enabled anywhere by default and has not run inside the real shell.

What is here:

| File | Job |
|---|---|
| `manifest.json` | Omarchy manifest: kinds `service` and `overlay`, `keepLoaded`, our metadata under `ow` (never under `omarchy`) |
| `Service.qml` | Entry point: store + bridge + canvas, reads placement state |
| `Bridge.qml`, `Protocol.js` | `Quickshell.Io` socket client for `owd` (NDJSON), reconnect with backoff, link and per-topic states |
| `Store.qml`, `Layout.js`, `Json.js` | Discovers `$XDG_DATA_HOME/owd/widgets/<id>/`, validates, re-reads on change, drops invalid widgets |
| `LayoutNode.qml`, `Kids.qml`, `Tile.qml` | The renderer for the 13 node types, and one widget tile with its status |
| `Theme.qml` | The only file that imports `qs.Commons` or contains a colour literal |
| `DesktopCanvas.qml` | One Bottom-layer layer-shell window per monitor, input mask = tiles |
| `Center.qml`, `CenterState.qml`, `CenterPanel.qml`, `CenterModel.js` | The `overlay` kind: a read-only Widget Center (list, grants, health, the remove command as text). See `docs/WIDGET_CENTER.md` |

## Run it safely (throwaway only)

Never enable it in your real shell from this slice. Nothing below touches `~/.config`, the keyring, or your running shell.

```
plugin/tests/run-all.sh                       # lint, static check, QtTest, bridge, store, manifest validation
plugin/tests/run-qml-tests.sh                 # QtTest only (offscreen)
plugin/tests/run-e2e.sh /tmp/out.png          # real owd + fake installs + offscreen render to a PNG
plugin/tests/run-installed.sh /tmp/ow-try.XXXX /tmp/out.png   # draw what `ow install` put in a sandbox (docs/TRY_IT.md)
OW_E2E_LIVE=1 plugin/tests/run-e2e.sh /tmp/x  # ~6 s of the real Bottom-layer canvas on the live compositor (like spikes/s1)
```

Each script makes fresh `mktemp` trees, points `XDG_*` at them, starts `owd` (`cargo run -p owd -- --config-dir <tmp>` equivalent) and Quickshell with its own config path, records only its own PIDs and signals only those. Use Qt 6 tools from `/usr/lib/qt6/bin`.

## Security properties and how each is tested

| Property | Where | Test |
|---|---|---|
| Nothing from a widget file is executed, evaluated, used as a QML source, URL or file path | no `createQmlObject`, `createComponent`, `Qt.include`, `eval`, `Function`, `setSource`, no `Loader.source` except one constant file name; no `Image`, `Process`, `XMLHttpRequest` | `tests/static_check.py` (with `--selftest` planting a violation per rule) |
| Text is never rich text | every `Text` sets `Text.PlainText`, elided, one line; strings are sanitised first | static check; `tst_render` (markup, QML, markdown strings, RLO, zero-width, huge strings) |
| Colours are theme roles only | widget names `accent/foreground/secondary/muted`; `Theme.qml` maps them; hex literals and `Qt.rgba` only in `Theme.qml` | static check; `tst_render` (`#ff0000`, `red`, `urgent` fall back to the node default); `tst_theme*` |
| Tiles and the input mask: at most half the screen each way and 1024 by 768, at least 48 by 24, 64 per canvas, on their own screen; the live run compares the `wl_region` rectangles sent to the compositor with the tile list | `Layout.layoutTiles` | `tst_layout` (8192 by 8192 size classes, NaN, negative, huge placement values), live probe |
| Subscriptions: 16 per widget, 30 per connection, round-robin so one widget cannot starve the rest, limited ones retried, orphans deleted | `Bridge.reconcile` | bridge `caps` scenario (a widget declaring 128 topics) |
| Limits: depth 8, 200 nodes, 20 children, 120-character text, 64 KB layout | `Layout.validateLayout`, `treeFits` | `tst_layout`, `tst_render` |
| Invalid widgets are dropped to a tile that names only the id | `Layout.loadWidget`, `Tile` | `tst_layout`, `tst_tile`, `run-store-tests.sh`, e2e PNG |
| File size is checked from a fresh directory listing before a file is read, again after every change (a file grown in place to 1.5 GB is not read); JSON nesting is bounded before `JSON.parse` | `Store.qml`, `Json.depthOk` | store test (200 KB layout, 60000 `[`, 1.5 GB grown in place with the shell's memory sampled), `tst_layout` |
| A bind can only read a topic the widget declared and was recorded as granted; the daemon stays authoritative | `loadWidget` (declared binds), `tileState` (only `record.topics`), daemon grants | `tst_layout`, `tst_tile`, e2e (`system.temp` shows "no access") |
| Missing, stale or non-finite data is an explicit placeholder, never 0 | `Layout.show/fraction`, `LayoutNode` | `tst_render` (NaN, Infinity, 1e300, null, objects), `tst_tile` |
| Socket path comes from an absolute `$XDG_RUNTIME_DIR` only (relative XDG paths are treated as unset); no credential ever passes through | `Bridge.defaultSocketPath`; only hello, subscribe, unsubscribe are built | bridge tests assert every request line the fake saw |
| Only plain numbers and bounded strings are kept from the daemon; bad lines are dropped; a hello answer must say protocol 0; the read buffer is bounded | `Protocol.parseLine/cleanData`, `Bridge.onChunk` | `tst_protocol`; bridge tests: partial lines, coalesced lines, split UTF-8, oversize line, never-ending line before and after hello with memory sampled, endless valid lines, garbage, wrong sub or topic, wrong protocol |
| No keyboard, no exit (canvas) | `keyboardFocus: None`, no `Qt.quit()` outside test harnesses | static check; e2e live probe |
| The Widget Center is read-only: no process, no file write, no request except hello, subscribe, unsubscribe; the remove command is text; hostile `widget.json` text is plain, bounded, never parsed | `Center*.qml`, `CenterModel.js` (no `Process`, no `FileView`; the only keyboard code is the `Keys.onEscapePressed` handler in `CenterPanel.qml`; the only overlay layer and `OnDemand` focus are in `Center.qml`; the static check requires each surface line to be present exactly once and rejects writes, sends and rescans in the Center files) | static check (per-file rules, planted violations); `tst_center` (list, grants, every health label, invalid ids get no command, caps and dedupe, closed Center does no row work, Escape, hostile strings). The `PanelWindow` itself is not instantiated by any test (no window backend offscreen); store harness (`Center` against the hostile tree); bridge `health` scenario |
| Placement state is plugin-owned and read-only here | `$XDG_STATE_HOME/ow-shell/canvas.json`, never under `~/.config` | `tst_layout` (hostile placement values) |

Code widgets are not handled here and are not sandboxed anywhere in this project: they run as the user.

## What is NOT done

- Edit mode, settings forms, drag/resize. The Widget Center exists but is read-only v0: it lists installed widgets, their grants and their health and prints `ow remove <id>` as text. It has no install, no remove, no enable, no Discover, Layouts or Create tabs, no audit viewer (WIDGET-CENTER-V0 in `docs/DECISIONS.md`).
- A symlink under the widgets directory (a symlinked widget directory or a symlinked `manifest.json`) is followed by the Store, and can load content from outside the widgets directory. Quickshell's `FolderListModel` does not report links. Exploiting it needs write access to your data directory. A daemon-provided list of widgets would fix it (OPEN_QUESTIONS 42).
- The Store loads at most 200 widget directories (by name) and counts the rest (`overflowDirs`); the Widget Center shows at most 200 rows and says how many were not shown.
- How the real shell summons the overlay is not established from the repo (`docs/WIDGET_CENTER.md`). `Center` exposes `open`, `visible` and `toggle()`; nothing calls them yet and no key is bound. Escape needs one click in the panel first (`OnDemand` focus, SPIKES S2); the Close button works without it. Never run in the real shell.
- Code widgets (only `tier: "spec"` is drawn; anything else is dropped).
- Real click pass-through on the canvas. The mask is the union of tile rectangles and one live run confirmed a level-1 `owd-canvas` surface, but nobody has clicked through it (OPEN_QUESTIONS 19). Clicks on a tile are swallowed (no interaction exists yet). Lock screen and fullscreen windows untested.
- Running inside the real shell: `service`/`overlay` property injection, `keepLoaded` across `rescanPlugins`, theme re-read on `omarchy theme set` (OPEN_QUESTIONS 20, 24).
- The daemon in this tree serves only `system.cpu|memory|load`. Clock (`system.time`), battery and `system.temp` show "not served" or "no access" placeholders until those providers exist.
- Hidden or paused widgets still hold their subscription; there is no visibility reporting yet.
- Same-uid limits: a process running as you can edit installed files, `installed.json`, the state file and the socket. A symlinked widget file is read as given. A file that grows in the milliseconds between the directory listing and the read is read (the store re-lists on every change, so the window is small but not zero).
- No peer check on the socket: QML cannot read `SO_PEERCRED`, so the plugin cannot tell that it is talking to the real `owd`; only the daemon checks the peer's uid. What limits the damage is that the plugin sends only hello, subscribe and unsubscribe (no credential, nothing from a widget file), keeps only numbers and short strings, and drops the connection on a malformed flood.
- Read buffer: `SplitParser` with a `"\n"` marker buffers without limit (a peer that never sent a newline grew the shell to 6 GB in 9 s; OOM would have ended the desktop). The bridge uses an EMPTY marker, which hands over each chunk as it arrives (about 230 KB per read), and frames lines itself: an unfinished line over 64 KiB, or more than 4 MiB in a second, drops the connection with the normal backoff. Measured peak resident memory against a never-ending stream: about 100 MiB, against 2 GB or more with the newline marker (the test fails if the marker is reverted). One residual: Quickshell decodes each chunk as UTF-8 on its own, so a multi-byte character split exactly at a chunk boundary would show as a replacement character; the daemon's data is ASCII in practice (the split-character test passes at small chunk sizes).

## Notes for maintainers

- `import qs.Commons as Commons`: QtQuick 6.12 has its own `Color` singleton that shadows the shell's when imported unqualified.
- QML refuses a type that instantiates itself, and refuses a two-file cycle. Recursion goes through `Loader { source: "LayoutNode.qml" }` in `Kids.qml`. Pass counts, not objects, through `Repeater` models: objects become variant wrappers whose arrays are no longer arrays.
- `qmllint` 6.12 reports `Type Theme not declared as singleton in qmldir` for any singleton in a plain directory (reproduced on a three-line file); `run-all.sh` filters exactly that line.
