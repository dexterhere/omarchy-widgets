# Widget Center v0 (read-only)

Status: the data, state and panel items are tested offscreen (`plugin/tests/run-all.sh`); the layer-shell window itself (`PanelWindow` in `Center.qml`) is NOT instantiated by any test, because Quickshell has no window backend offscreen. Its layer, keyboard and close wiring are checked only textually by the static check. Never run in the real Omarchy shell. Decision record: WIDGET-CENTER-V0 in `docs/DECISIONS.md`.

## What it shows

One panel, one row group per installed widget:

- id and name, version (from `manifest.json`), status (from `installed.json`: planned, alpha, stable, removed, else `unknown`) and tier (always `spec`; only spec widgets are loaded);
- grants: each granted topic with its fields, and topics the widget declares but was not granted;
- health per topic: `locked (ow vault unlock)`, `no credential`, `unauthorized`, `insufficient permission`, `rate limited`, `expired soon`, `offline`, `unavailable` (the provider states of `docs/DAEMON_SPEC.md`, "States"), otherwise `ok`, `stale`, `no access`, `not served`, `daemon error`, `waiting for data`, or `daemon not running` when the link to `owd` is down;
- the command to remove it, as text: `ow remove io.github.example.clock`.

A widget that failed validation is listed with its id, a fixed reason code and its remove command. A directory whose name is not a valid id is counted by the store and not listed. An id that is not valid never gets a command: the panel prints `id not valid, nothing to run` and the id with every unusual character spelled out as `\uXXXX`.

Limits: the store loads at most 200 widget directories (by name) and the panel shows at most 200 rows, 8 grant lines and 8 health lines per row (`+N more`). When anything is cut it says `N more not shown`. Rows are computed only while the Center is open.

Known gap: the store follows a symlink under `$XDG_DATA_HOME/owd/widgets/` (Quickshell does not tell the plugin which entries are links). Exploiting it needs write access to your data directory. A daemon-provided widget list would remove the gap (OPEN_QUESTIONS 42). Nothing from a widget file is shown except bounded plain text (name, version), and every `Text` is plain text.

## What it does not do

It installs, removes, enables, grants and edits nothing. It starts no process, writes no file and sends only the bridge's existing `hello`, `subscribe` and `unsubscribe`. Removal needs consent typed in a terminal, so the panel prints the command and stops. Spec widgets are JSON and are never executed by this plugin; code widgets are not handled and nothing here makes any widget sandboxed.

Where the list comes from: the daemon has no read-only op that lists installed widgets, so the panel uses the same read-only directory scan as the canvas (`Store.qml`, `$XDG_DATA_HOME/owd/widgets/`).

## Opening it

NOT ESTABLISHED. The repo records that Omarchy 4.0.6 has `omarchy-shell shell summon|toggle|hide <plugin-id>` and that Hyprland bindings are Lua calls to `o.bind` (SPIKES S13, PROJECT_BRIEF). It does not record what the shell does to an overlay plugin's root item when told to toggle it (set `visible`, call a function, or reach an `IpcHandler`). So `Center` exposes `open`, `visible` and `toggle()` and I stopped there (OPEN_QUESTIONS 40). If the shell sets `visible`, it already works; if it needs an `IpcHandler`, that is a small addition once someone reads the shell's plugin host.

Proposed binding, for Dexter to apply by hand, with a backup and a diff shown first (this branch does not touch `bindings.lua`). `SUPER + ALT + W` was free on stock Omarchy 4 (SPIKES S13). The plugin id is `io.github.dexterhere.widget-shell` (`plugin/manifest.json`); the id may change with D-3.

```diff
+-- Widget Center
+o.bind("SUPER + ALT + W", "Widget Center", "omarchy-shell shell toggle io.github.dexterhere.widget-shell")
```

Undo: delete those two lines. This depends on the OPEN_QUESTIONS 40 answer.

## Keyboard

The panel asks for keyboard focus `OnDemand`, the setting measured in SPIKES S2: Escape arrives after one click inside the panel, and the Close button works without it. `Exclusive` was not used.

## Tests

`plugin/tests/tst_center.qml` (list from fixtures, grants text, every health label, remove command including quoted odd ids, Escape, Close, hostile strings), `tst_protocol.qml` (provider words kept from a closed list), the bridge `health` scenario against a fake daemon, the `Center` checks in the store harness, and the static check (per-file rules with planted violations).
