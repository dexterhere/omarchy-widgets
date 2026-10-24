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
| `Center.qml` | Inert placeholder for the `overlay` kind (Widget Center is a later task) |

## Run it safely (throwaway only)

Never enable it in your real shell from this slice. Nothing below touches `~/.config`, the keyring, or your running shell.

```
plugin/tests/run-all.sh                       # lint, static check, QtTest, bridge, store, manifest validation
plugin/tests/run-qml-tests.sh                 # QtTest only (offscreen)
plugin/tests/run-e2e.sh /tmp/out.png          # real owd + fake installs + offscreen render to a PNG
OW_E2E_LIVE=1 plugin/tests/run-e2e.sh /tmp/x  # ~6 s of the real Bottom-layer canvas on the live compositor (like spikes/s1)
```

Each script makes fresh `mktemp` trees, points `XDG_*` at them, starts `owd` (`cargo run -p owd -- --config-dir <tmp>` equivalent) and Quickshell with its own config path, records only its own PIDs and signals only those. Use Qt 6 tools from `/usr/lib/qt6/bin`.

## Security properties and how each is tested

| Property | Where | Test |
|---|---|---|
| Nothing from a widget file is executed, evaluated, used as a QML source, URL or file path | no `createQmlObject`, `createComponent`, `Qt.include`, `eval`, `Function`, `setSource`, no `Loader.source` except one constant file name; no `Image`, `Process`, `XMLHttpRequest` | `tests/static_check.py` (with `--selftest` planting a violation per rule) |
| Text is never rich text | every `Text` sets `Text.PlainText`, elided, one line; strings are sanitised first | static check; `tst_render` (markup, QML, markdown strings, RLO, zero-width, huge strings) |
| Colours are theme roles only | widget names `accent/foreground/secondary/muted`; `Theme.qml` maps them; hex literals and `Qt.rgba` only in `Theme.qml` | static check; `tst_render` (`#ff0000`, `red`, `urgent` fall back to the node default); `tst_theme*` |
| Limits: depth 8, 200 nodes, 20 children, 120-character text, 64 KB layout | `Layout.validateLayout`, `treeFits` | `tst_layout`, `tst_render` |
| Invalid widgets are dropped to a tile that names only the id | `Layout.loadWidget`, `Tile` | `tst_layout`, `tst_tile`, `run-store-tests.sh`, e2e PNG |
| File size is checked from the directory listing before a file is read; JSON nesting is bounded before `JSON.parse` | `Store.qml`, `Json.depthOk` | store test (200 KB layout, 60000 `[`), `tst_layout` |
| A bind can only read a topic the widget declared and was recorded as granted; the daemon stays authoritative | `loadWidget` (declared binds), `tileState` (only `record.topics`), daemon grants | `tst_layout`, `tst_tile`, e2e (`system.temp` shows "no access") |
| Missing, stale or non-finite data is an explicit placeholder, never 0 | `Layout.show/fraction`, `LayoutNode` | `tst_render` (NaN, Infinity, 1e300, null, objects), `tst_tile` |
| Socket path comes from `$XDG_RUNTIME_DIR` only; no credential ever passes through | `Bridge.defaultSocketPath`; only hello, subscribe, unsubscribe are built | bridge tests assert every request line the fake saw |
| Only plain numbers and bounded strings are kept from the daemon; bad lines are dropped | `Protocol.parseLine/cleanData` | `tst_protocol`; bridge tests: partial lines, coalesced lines, split UTF-8, oversize line, garbage, wrong sub or topic |
| No keyboard, no exit | `keyboardFocus: None`, no `Qt.quit()` outside test harnesses | static check; e2e live probe |
| Placement state is plugin-owned and read-only here | `$XDG_STATE_HOME/ow-shell/canvas.json`, never under `~/.config` | `tst_layout` (hostile placement values) |

Code widgets are not handled here and are not sandboxed anywhere in this project: they run as the user.

## What is NOT done

- Edit mode, the Widget Center, settings forms, drag/resize (the overlay entry is an inert placeholder).
- Code widgets (only `tier: "spec"` is drawn; anything else is dropped).
- Real click pass-through on the canvas. The mask is the union of tile rectangles and one live run confirmed a level-1 `owd-canvas` surface, but nobody has clicked through it (OPEN_QUESTIONS 19). Clicks on a tile are swallowed (no interaction exists yet). Lock screen and fullscreen windows untested.
- Running inside the real shell: `service`/`overlay` property injection, `keepLoaded` across `rescanPlugins`, theme re-read on `omarchy theme set` (OPEN_QUESTIONS 20, 24).
- The daemon in this tree serves only `system.cpu|memory|load`. Clock (`system.time`), battery and `system.temp` show "not served" or "no access" placeholders until those providers exist.
- Hidden or paused widgets still hold their subscription; there is no visibility reporting yet.
- Same-uid limits: a process running as you can edit installed files, `installed.json`, the state file and the socket. `SplitParser` has no line cap, so one endless line without a newline from the socket peer is buffered until it ends (the daemon enforces its own 64 KiB limit). A symlinked widget file is read as given.

## Notes for maintainers

- `import qs.Commons as Commons`: QtQuick 6.12 has its own `Color` singleton that shadows the shell's when imported unqualified.
- QML refuses a type that instantiates itself, and refuses a two-file cycle. Recursion goes through `Loader { source: "LayoutNode.qml" }` in `Kids.qml`. Pass counts, not objects, through `Repeater` models: objects become variant wrappers whose arrays are no longer arrays.
- `qmllint` 6.12 reports `Type Theme not declared as singleton in qmldir` for any singleton in a plain directory (reproduced on a three-line file); `run-all.sh` filters exactly that line.
