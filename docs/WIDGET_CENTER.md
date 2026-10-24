# Widget Center v0 (read-only)

Status: built and tested offscreen only (`plugin/tests/run-all.sh`). Never run in the real Omarchy shell. Decision record: WIDGET-CENTER-V0 in `docs/DECISIONS.md`.

## What it shows

One panel, one row group per installed widget:

- id and name, version, status and tier as recorded in `installed.json`;
- grants: each granted topic with its fields, and topics the widget declares but was not granted;
- health per topic: `locked (ow vault unlock)`, `no credential`, `unauthorized`, `insufficient permission`, `rate limited`, `expired soon`, `offline`, `unavailable` (the provider states of `docs/DAEMON_SPEC.md`, "States"), otherwise `ok`, `stale`, `no access`, `not served`, `daemon error`, `waiting for data`, or `daemon not running` when the link to `owd` is down;
- the command to remove it, as text: `ow remove io.github.example.clock`.

A widget that failed validation is listed with its id, a fixed reason code and its remove command. Nothing from a widget file is shown except bounded plain text (name, version), and every `Text` is plain text.

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
