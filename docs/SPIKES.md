# Phase 0 spike results

Run on 2026-10-23 on the daily-driver laptop: Omarchy 4.0.6-1, Quickshell 0.3.2 (distributed by Omarchy), Hyprland 0.56.2, Wayland, two monitors (eDP-1, HDMI-A-1). Code and raw evidence are under `spikes/<id>/`; how to run them safely is in `spikes/README.md`.

How facts were obtained, so you can weigh them:
- **Read** means read from the installed Omarchy sources under `/usr/share/omarchy` (`bin/omarchy-plugin-*`, `shell/shell.qml`, `shell/services/PluginRegistry.qml`, `shell/Commons`). That shows what the code does; it was not executed inside the real shell.
- **Run** means executed here, in a throwaway Quickshell process with its own config path and its own XDG state/cache/data dirs, a few seconds each, or as a read-only CLI call.
- Nothing was written under `~/.config`, `~/.local/share/omarchy`, `/usr` or `/etc`. No `omarchy plugin add|enable|disable|remove|update|clone`, no `hyprctl dispatch|keyword|reload`, no `systemctl --user`, no kill by name. The live shell (`quickshell -n -p /usr/share/omarchy/shell`) was never touched.

Skipped on purpose: **S4** (keyring; needs real keyring access, out of scope for this run), **S11** (headless CI), and the Dexter-only tasks (contacting maintainers, naming, publishing).

## Summary

| Spike | Verdict | Measured fact |
|---|---|---|
| S1 Bottom-layer canvas | PARTIAL | Layer 1 (bottom) on both monitors, no focus change, no reserved area, one input rect reaches the compositor. Real click pass-through and lock-screen behavior were not exercised. |
| S2 Overlay edit surface | PASS | Layer 3 (overlay), drag/resize/nudge/Escape logic passes 7 offscreen tests, JSON written atomically. Live keyboard delivery measured by a person on 2026-10-24: with `OnDemand` focus, a click gives the surface the keyboard and Escape reaches it. |
| S3 Socket bridge | PASS | `Quickshell.Io` `Socket` + `SplitParser` talks NDJSON to a `SO_PEERCRED` stub, survives a config hot reload, reconnects after a server restart. |
| S5 Manifest tolerance | PASS | Extra top-level keys, sidecar files, unknown kinds and a data-only plugin all validate. Fold into the manifest. |
| S6 Plugin imports | PARTIAL | Works (one shared singleton), already used on this machine, but Omarchy has no dependency mechanism and its facade deliberately blocks cross-plugin service access. |
| S7 `plugin add` | FAIL (as assumed) | Clones default-branch HEAD of a repo whose root is the plugin. No SHA, no ref, no subdirectory. `plugin update` un-pins. |
| S8 Theme tokens | PARTIAL | `qs.Commons` (`Color`, `Style`, `Util`) and `qs.Ui` are importable by third parties and used by shipped third-party plugins. No stability promise anywhere. The ANSI palette is not exposed. |
| S9 Settings form | PARTIAL | `schema`/`defaults` are declared and registered as metadata, but no renderer ships in 4.0.6, and only bar widgets get per-instance settings. |
| S10 Existing data | PASS | `omarchy.agents` has a file contract; `omarchy.monitor` exposes nothing reusable beyond `hyprctl`. |
| S12 Marketplace | NOT MEASURABLE | No catalog data exists locally. |
| S13 Chord | PASS | `SUPER + ALT + W` is free. The brief's placeholder `SUPER + SHIFT + W` is taken. |

## S1 Canvas surface on `WlrLayer.Bottom`

**Update 2026-10-24, measured by a person (Dexter) on the live desktop (eDP-1 and HDMI-A-1):** with the S1 surface running (`spikes/run-safe.sh /tmp/ow-kb/s1 40`), clicks inside the box were received on both monitors (2 on eDP-1, 27 on HDMI-A-1), and clicks on bare wallpaper outside the box were made on both monitors with no `S1 LEAK click outside box` line in the log, so the surface never received them. Click pass-through of the input mask is therefore **PASS** on both monitors. Lock screen, fullscreen windows and keyboard focus were still unverified at that point.

**Lock screen, measured by Dexter on 2026-10-24:** with the same surface running for 90 s (`spikes/run-safe.sh /tmp/ow-kb/s1 90`), the session was locked with `SUPER + CTRL + L`. No blue box was visible on the lock screen on either monitor, and the run ended normally with the timeout (exit 124). A Bottom-layer surface is therefore hidden by the session lock, as the protocol intends. Result: **PASS**. Fullscreen windows and keyboard focus (S2) remain unverified.

**Question.** Can a plugin draw a Bottom-layer surface that is click-through except one interactive rectangle, without stealing focus or shifting other layers?

**Method.** `spikes/s1/shell.qml`: a `Variants` over `Quickshell.screens`, one full-screen transparent `PanelWindow` per monitor, `WlrLayershell.layer: Bottom`, `keyboardFocus: None`, `exclusionMode: Ignore`, `mask: Region { item: box }`. Run for 6 s with `WAYLAND_DEBUG=client`, probed with read-only `hyprctl`.

**Commands.** `WAYLAND_DEBUG=client spikes/run-safe.sh spikes/s1 6 <probe>`; probe = `hyprctl activewindow -j`, `hyprctl layers -j`, `hyprctl monitors -j`.

**Result** (`spikes/s1/evidence.txt`).
- `hyprctl layers -j`: `ow-spike-canvas` at level 1 (bottom) on eDP-1 (next to the user's own `omarchy-desk`, also level 1) and on HDMI-A-1, geometry 1920x1080 each. Levels in Hyprland: 0 background, 1 bottom, 2 top (the bar), 3 overlay.
- Active window identical before and after (no focus steal). `reserved` stayed `[0,0,0,0]` on both monitors.
- The client sent `set_layer(1)`, `set_exclusive_zone(-1)`, `set_keyboard_interactivity(0)` and exactly one input region `wl_region.add(40, 200, 160, 90)` followed by `set_input_region`. So the mask reaches the compositor as one rectangle; the rest of the surface has an empty input region by protocol definition.
- Corroboration, not part of the run: the user's own plugin `buggybuck.desk` (an overlay-kind plugin, `keepLoaded: true`) draws a Bottom-layer `PanelWindow` today, and the first-party `omarchy.notifications` service draws one `PanelWindow` per screen from a `Variants`. Drawing layer surfaces from a plugin is an established pattern in 4.0.6.

**Not measured.** Actual pointer click-through: injecting a click into a live session would disturb the user's desktop, so it was not done. Lock screen behavior: not exercised (the lock uses `WlSessionLock`, which by protocol covers all layers, but nothing here verified that). Multi-monitor was measured only as "a surface per output exists". A 10-second manual check is the way to close it: run the spike and click on empty wallpaper outside the box.

**Verdict.** PARTIAL.

**Consequence.** The canvas design (one Bottom surface per monitor, input mask limited to interactive rects) is feasible with stock Quickshell and is how the user's own desk widget already works. Plan one `PanelWindow` per screen from `Variants`. Re-measure click pass-through with a human at the keyboard before building on `interactiveRegions`. Note that `mask` takes `Item` rectangles, so an interactive region must be a real item whose geometry tracks the widget.

## S2 Overlay edit surface

**Question.** Can an Overlay-layer surface offer free-form drag and resize, write the result to JSON, and exit on Escape?

**Method.** `spikes/s2/EditCanvas.qml` is pure QtQuick (drag via `MouseArea.drag`, bottom-right resize handle, arrow-key nudge, Shift for 10 px, clamp to the surface, Escape emits `exitRequested`, every change emits `committed(json)`). `tst_edit.qml` tests it with QtTest under `QT_QPA_PLATFORM=offscreen`, so no display is touched. `spikes/s2/shell.qml` hosts it in an Overlay `PanelWindow` with `FileView { atomicWrites: true }` and a timer that calls the same move/resize functions.

**Commands.**
- `cd spikes/s2 && QT_QPA_PLATFORM=offscreen /usr/lib/qt6/bin/qmltestrunner -input tst_edit.qml`
- `spikes/run-safe.sh spikes/s2 7 <probe>` (probe: `hyprctl layers -j`, `cat layout.json`)

**Result** (`spikes/s2/evidence.txt`).
- 7 of 7 offscreen tests pass: drag moves and commits, resize handle, resize clamps to min size and surface, Shift-arrow nudge, Escape emits exit.
- Live: `hyprctl layers -j` shows `ow-spike-edit` at level 3 (overlay), geometry as requested. The client sent `set_layer(3)` and `set_keyboard_interactivity(2)` (on demand). `FileView.setText` with `atomicWrites` produced valid `layout.json` (`{"version":1,"widgets":{"a":{"x":70,"y":60,"w":260,"h":150}}}`) and fired `onSaved`.
- Qt's drag threshold (about 10 px) is swallowed before a drag begins; the test allows that slack. A real editor will see the same small lag.
- The spike surface was deliberately small and used on-demand focus so a live run could not grab the user's keyboard.

**Update 2026-10-24.** Measured by Dexter on 2026-10-24 on the live desktop with a logging copy of the S2 spike and `WAYLAND_DEBUG=client`: after one click inside the panel Hyprland sent `wl_keyboard.enter` and the QML logged `S2 FOCUS activeFocus=true`; three presses of `a` arrived as `wl_keyboard.key` events and as `S2 KEY 65`; Escape arrived as `S2 KEY escape` followed by `S2 ESCAPE -> exit`, and the surface released the keyboard. The surface gives the keyboard back when the person clicks elsewhere and takes it again on the next click, as `OnDemand` should. Earlier runs showed no `S2 KEY` lines because keys were pressed before the focus event settled; wait about two seconds after the click. `Exclusive` was not needed and was not run.

**Not measured.** Keyboard handling on the dimmed full-screen surface, and the "pause when a fullscreen window covers the monitor" behavior, are untested.

**Verdict.** PARTIAL (logic and persistence pass; live key delivery unmeasured).

**Consequence.** Keep the editor logic as a pure-QtQuick component with a `committed` signal and let the host own the write (testable offscreen, and the daemon or a file can be the sink). Use an atomic write. Two rules for the real plugin: never call `Qt.quit()` (the spike does, because it is its own process; in the shell it would end the user's desktop), and exit by hiding the window or setting a property. Decide `Exclusive` versus `OnDemand` for edit mode with a human present.

## S3 Socket bridge

**Question.** Can a QML plugin hold a Unix-socket connection to a daemon speaking newline-delimited JSON, survive a reload, and reconnect after the daemon restarts? Which Quickshell types are needed?

**Method.** `spikes/s3/stub_server.py` (about 50 lines of Python) listens on a `0600` socket in a short `mktemp` dir, reads `SO_PEERCRED`, refuses a peer whose uid is not the expected one, answers `hello` and `subscribe` in the DAEMON_SPEC shape and pushes once a second. `spikes/s3/shell.qml` uses `Quickshell.Io.Socket` with `parser: SplitParser { splitMarker: "\n" }`. The driver starts only its own stub PIDs.

**Commands.** `spikes/s3/run.sh ok`; `spikes/s3/run.sh reject`.

**Result** (`spikes/s3/evidence.txt`).
- Connect, `hello` reply, `subscribe` reply and pushes work. Stub log: `accept peer pid=... uid=1000`.
- Hot reload (appending a line to the config; Quickshell logs `Reloading configuration...`, same process): the old connection closed (`client gone`), the new instance connected and re-sent `hello` and `subscribe` immediately, pushes resumed.
- Server killed: `QLocalSocket::PeerClosedError`, `connected` turned false. While the server was down each retry logged `ConnectionRefusedError`. After the stub was restarted the client connected, re-sent `hello`/`subscribe` and pushes resumed, with no manual action.
- Reject run: stub expecting a different uid logged `REJECT peer ... uid=1000 (expected 0)` and closed; the client saw no reply, got `PeerClosedError` and, with a naive 1 s timer, retried in a loop.
- Types: `Socket` (properties `path`, `connected`, `parser`; methods `write(string)`, `flush()`; signals `connectedChanged`, `error`) and `SocketServer` (`path`, `active`, `handler`) from `Quickshell.Io`. `write` takes a string, so the channel is text-only; NDJSON fits. `Socket` does not reconnect by itself: toggle `connected` from a timer. The QML client cannot read the server's credentials, so only the daemon can authenticate the peer.

**Not measured.** The real `rescanPlugins` path inside `omarchy-shell` (a standalone config reload was used instead). Read from source: services without `keepLoaded` are destroyed and recreated on a rescan, and `keepLoaded: true` services survive it unchanged ("code changes to a `keepLoaded` service itself only take effect on a shell restart"). The DAEMON_SPEC item "different-uid refusal" is still only tested with a stub's configured uid, not a second real uid.

**Verdict.** PASS.

**Consequence.** The bridge can be a QML service using `Socket` + `SplitParser`; no native plugin is needed. It must (a) re-send `hello` and its subscriptions on every `connected`, (b) reconnect with backoff (a fixed 1 s retry loops forever against a refusing peer), (c) treat the server as authoritative for uid checks. Mark the bridge service `keepLoaded: true` so a rescan does not drop the connection, and accept that its own code then updates only on a shell restart.

## S5 Manifest tolerance

**Question.** Does `omarchy plugin validate` accept extra top-level manifest keys such as a `widget.json` sidecar? Can `widget.json` fold into the manifest?

**Method.** Read `bin/omarchy-plugin-validate` and `PluginRegistry.validateManifest`, then ran the validator on temp folders (`spikes/s5/check.sh`).

**Result** (`spikes/s5/evidence.txt`).
- The validator checks only: valid JSON, `schemaVersion == 1`, required `id name version kinds entryPoints`, id charset and not `omarchy.*`, non-empty `kinds`, `entryPoints` an object of safe existing relative paths, the entry point each of six known kinds requires, `barWidget.defaultSection`, and no symlinks. Unknown keys are never inspected.
- Exit 0 for: an extra top-level `ow` block, an arbitrary `zzz` key, extra files `widget.json` and `layout.json` beside the manifest, an unknown kind, and a data-only plugin (`kinds: ["owd-widget"]`, `entryPoints: {}`, no QML). Exit 1 (control) for a symlink and for an `omarchy.*` id.
- The shell side also keeps unknown keys: `publicPluginManifest` hands a third-party plugin a deep copy of its own manifest without only `__isFirstParty` and `__hostCapabilities`, so a plugin can read its folded block at run time. First-party manifests already carry non-schema keys (`keepLoaded`, `activation`, `license`, `homepage`, `omarchy.clonedFrom`, `omarchy.capabilities`).
- Caution: the `omarchy` key is used by the host (`omarchy.capabilities`, `omarchy.clonedFrom`). Do not put our data under it.

**Not measured.** Whether the official marketplace's automated validation (`omacom/omarchy-plugin-marketplace`) is as tolerant as the local validator. Its code is not installed. Recorded as an open question.

**Verdict.** PASS.

**Consequence.** Decision D-4 resolves to: fold `widget.json` into `manifest.json` under a namespaced `ow` key, keeping a standalone `widget.json` only as an optional generated copy for tools that want it. A spec widget needs no QML at all: it can be a data-only plugin. See SPIKE-S5 in `docs/DECISIONS.md`.

## S6 Can one plugin import another's singleton

**Question.** Can plugin B import a singleton owned by plugin A? Decides one shared bridge singleton versus a client per widget.

**Method.** `spikes/s6`: plugin A has a `qmldir` with `singleton Counter` that records a random stamp at construction; the host and plugin B import it by `import "../pluginA"`, and B2 by `import "../../plugins/pluginA"`. Also `Qt.createComponent` of a QML file outside any plugin folder and outside the config root. Read `PluginShellApi.qml` and `shell.qml` for the sanctioned API.

**Result** (`spikes/s6/evidence.txt`).
- All three importers print the same stamp, and the counter is shared (`n` goes 1, 11): one singleton instance, even when the folder is spelled by two different relative routes.
- `Qt.createComponent("file:///abs/path/Widget.qml")` for a file outside the config and plugin dirs loads and instantiates (`status=1`). Quickshell reports in-config URLs as `qs:@/qs/...`.
- On this machine today: `~/.config/omarchy/plugins/buggybuck.desk/Desk.qml` does `import "../buggybuck.tasks/TaskModel.js" as T`. A cross-plugin relative import works in 4.0.6.
- Against it, read from source: there is no `dependencies` or `requires` manifest field and no validator or `plugin remove` check for dependents (grep found none); if A is missing, B fails with a QML load error. `PluginShellApi.serviceFor(id)` returns a service only when `id` resolves to the caller itself (`pluginOwnsTarget`), a deliberate capability limit. The host's own comment in `BarWidgetRegistry.qml` says relative-path singleton imports created per-importer instances for it (not reproduced here, but a warning that identity depends on how the import is spelled). Visual plugins share the host scene graph anyway ("Facades do not isolate visual widgets").
- Read from source: an overlay plugin that also declares a `service` entry receives `item.service = shell.serviceFor(ownId)` on load, which is a sanctioned way for a plugin's own UI and service to share state.

**Not measured.** Singleton lifetime across a real `rescanPlugins` (the spike does not run inside the shell). Whether the marketplace allows plugins that import siblings.

**Verdict.** PARTIAL: possible, unsupported, unguarded.

**Consequence.** Do not make widgets depend on importing the bridge singleton from another plugin. Ship the bridge and the renderer as one plugin (service + overlay in one folder, state shared through the injected `service` property), and have code widgets receive their data from the renderer (properties/signals) rather than importing it. Per-widget sockets are an option only for ejected code widgets. See SPIKE-S6.

## S7 What `omarchy plugin add` accepts

**Question.** Does it accept a commit SHA or a subdirectory, or always track HEAD?

**Method.** Read `bin/omarchy-plugin-add`, `omarchy-git-url-check`, `omarchy-plugin-update`, `omarchy-plugin-validate`. Did not run `add`. `spikes/s7/pin-demo.sh` shows the update behavior on a temp git repo.

**Result** (`spikes/s7/evidence.txt`).
- `add [git-url] [--enable] [--yes]` has no ref, branch or path option; unknown options fail. It runs `git clone -- "$url"` into `~/.config/omarchy/plugins/.add.tmp.$$`, validates `manifest.json` at the clone root, then moves the clone to `~/.config/omarchy/plugins/<manifest id>/`. So the repo root is the plugin; a monorepo subdirectory cannot be installed, and the folder name comes from the manifest id.
- The URL check refuses options and `helper::` forms, and allows `ssh git git+ssh ssh+git http https ftp ftps file` URLs and scp-style paths (`file://` was accepted here). A `url#sha` form is passed to git as part of the path and the clone fails ("repository ... does not exist"), so a SHA cannot ride on the URL.
- It tracks the remote default-branch HEAD. `update` does `git fetch origin HEAD`, shows a diff, then `git merge --ff-only FETCH_HEAD`, and rolls back on failed validation. The demo shows a checkout pinned to an old commit is moved to upstream by that logic (pin lost). The README says "pinning a ref, switching branches" is "ordinary git in the plugin directory".
- Pinned-SHA installs are not an `omarchy plugin` feature. The brief attributes them to Plugin Depot; that is not installed and not verified here.

**Verdict.** FAIL against the brief's assumption of "install pins a commit".

**Consequence.** The widget manager (`ow install`) cannot delegate pinned installs to `omarchy plugin add`. Either `ow` does its own pinned clone/extract (monorepo subdirectories and exact SHAs are then possible) and places files itself, or widgets are one-repo-per-widget installed by `omarchy plugin add` at HEAD with `ow` recording and checking the commit. Either way `ow` needs a verify step after `omarchy plugin update`. See SPIKE-S7 and D-6.

## S8 Theme tokens and singletons for third parties

**Question.** Which theme singletons and components may third-party QML import, and are they stable?

**Method.** Read `shell/Commons/{qmldir,Color.qml,Style.qml}`, `shell/Ui/qmldir`, `default/themed/shell.toml.tpl`, `themes/*/colors.toml`; counted colour keys with a script; grepped the user's third-party plugins for imports.

**Result.**
- `import qs.Commons` exposes `Color`, `Style`, `Util`, `Border` (all `singleton`); `import qs.Ui` exposes 33 components (`Panel`, `Button`, `Toggle`, `TextField`, `Dropdown`, ...). `qs` resolves to the running shell's config root. All 8 third-party plugins installed here have QML files that `import qs.Commons` or `qs.Ui`, including the three written by other authors (`dexterhere.omallama`, `kayooliveira.omadocker`, `org.jsongerber.vikunja`), so it works for third parties in 4.0.6.
- `Color` exposes: `foreground`, `background`, `accent`, `urgent` (from `red`), `muted`; and per-surface role objects `bar`, `popups`, `tooltip`, `notifications`, `menu`, `polkit`, `lock`, `imagePicker` (e.g. `Color.menu.text`, `Color.popups.border`). The ANSI palette (green, yellow, blue, magenta, cyan, orange, brown, the `bright_*` set, `dark_*`/`light_*`) is **not** exposed by `Color`.
- `Style` exposes spacing (`Style.spacing.*`, `Style.space(px)`), fonts (`Style.font.family`, `caption..displayLarge`, `Style.fontPx(mult)`), corner radius, gaps, control state fills and borders. All derive from `[font] base-size` and `[spacing] scale`, so respecting them gives the "scale with text size" rule for free.
- Colour counts: `colors.toml` has 23 hex keys in all 22 shipped themes, `orange` and `brown` in 19 of them, and 2 theme-specific extras (`active_border_color`, `active_tab_background`) in 3 themes (27 distinct keys, `mode` is a non-hex string). So the earlier "22 vs 24" claims are both off: 23 universal plus up to 4 optional.
- Live update: `Color` is bound to the theme; a theme switch pushes new values through shell IPC and every binding to `Color.*` restyles (the user's desk widget relies on this). The theme dir `~/.local/state/omarchy/current/theme/colors.toml` is read by `Color` at startup only.
- Stability: no document in `/usr/share/omarchy` promises that `qs.Commons` or `qs.Ui` is a public API. The shell README speaks of capability-scoped facades for services, not of a visual toolkit contract. These are internals that third parties happen to import.

**Not measured.** Behavior of a plugin when a property it uses is renamed in an Omarchy update (no previous version available to diff).

**Verdict.** PARTIAL.

**Consequence.** Import only `qs.Commons` (`Color`, `Style`) from the renderer, behind one adapter file (`Theme.qml`) that maps our semantic roles to those tokens and supplies fallbacks, so an Omarchy rename is a one-file fix and a missing property degrades instead of throwing. Provide `success`/`warning`/ANSI-like roles by parsing `colors.toml` in the adapter (with documented fallbacks to `accent`/`urgent` for the optional keys), or have the daemon expose them. Do not depend on `qs.Ui` widgets for desktop cards. Add a CI smoke test that loads the renderer against the installed Omarchy and fails on a missing token. See SPIKE-S8.

## S9 How far the settings form goes

**Question.** Types, validation and per-instance values of the manifest settings form.

**Method.** Read `shell.qml` (`syncPluginWidgets`, `updateEntryInline`), `PluginRegistry.qml`, `shell/plugins/bar/Bar.qml`, the shell README, and every first-party and user manifest that declares `schema`.

**Result.**
- Declared in the wild: `barWidget.defaults` (object) and `barWidget.schema` (array of `{key, type, label, description, defaultValue, min, max, step, options}`). Types seen: `string`, `integer`, `boolean`, `enum`, `path`. Also `barWidget.settingsForm` (a string naming a built-in form such as `weatherSettings`/`spacerSettings`).
- The host copies `defaults`, `schema` and `settingsForm` into the bar-widget registry metadata and does nothing else with them. I found no code in the installed 4.0.6 tree that renders `schema` into a form or validates values against it (grep for `schema`, `settingsForm`, `defaultValue` outside manifests hits only `shell.qml`'s metadata copy). A settings-panel plugin is mentioned in the README history but is not shipped.
- Values: stored inline on the plugin's entry in `shell.json` ("No `config:` sub-object"), written by the shell, and injected as a `settings` object into bar widgets. Multiple instances exist only when `allowMultiple: true`, and only for bar widgets. Every other kind has one entry per plugin id (`findEntryLocation` returns the first match) and no `settings` injection; a non-bar plugin gets `manifest`, `shell`, `omarchyPath`, and for a panel/overlay its own `service`.
- Everything under `barWidget` is meaningful only to `bar-widget` kind plugins.

**Verdict.** PARTIAL: a declarative vocabulary exists, with no shipped UI and no per-instance settings for desktop-style plugins.

**Consequence.** Own the whole settings pipeline: our `ow.settings` schema in the manifest (we may borrow the key names `key/type/label/min/max/step/options/defaultValue` for familiarity), a form rendered by the Widget Center, validation in the daemon or CLI, and values stored in our own `layout.json`/state per instance, not in `shell.json`. This also avoids sharing a file the shell rewrites. If Omarchy later ships a renderer for `barWidget.schema`, a bar-widget variant could reuse it. See SPIKE-S9.

## S10 What `omarchy.monitor` and `omarchy.agents` expose

**Method.** Read both plugins, their README and manifests, and the collector scripts; listed the key names (not values) of the live usage records.

**Result.**
- `omarchy.agents`: a stable file contract. `omarchy-agent-usage-update` runs one `omarchy-agent-usage-<agent>` collector each and writes one JSON record per agent to `~/.local/state/omarchy/agents/usage/<agent>.json` (mode 0600; here `claude`, `codex`, `fireworks`). Record keys: `schemaVersion, id, name, tierLabel, ready, limits[{label,percent,resetsAt}], todayTotalTokens, todayTokensByModel, todayPrompts, todaySessions, totalPrompts, totalSessions, modelUsage, recentDays, activeDays, activeDates, hasLocalStats, authHelpText, usageStatusText, updatedAt`. "Adding an agent therefore never touches this plugin." The widget itself exposes nothing (no singleton, no IPC) beyond its own panel and `IpcHandler`.
- `omarchy.monitor`: a bar-widget panel, no data API. It shells out to `omarchy-monitor-state`, which prints brightness, monitor names, scale and a JSON array on separate lines (not one JSON document). Reusable data comes from `hyprctl monitors -j` or `Quickshell.Hyprland`.
- Neither plugin exports a QML singleton or a documented IPC for data.

**Verdict.** PASS (the questions are answered).

**Consequence.** The `ai-usage` provider in `owd` should read `~/.local/state/omarchy/agents/usage/*.json` instead of re-collecting (and may call `omarchy agent usage-update` to refresh); this is local data, same sensitivity class as the DAEMON_SPEC "agent usage stays in memory". Keep `schemaVersion` handling defensive. The `system` and display providers should use `/proc` and `hyprctl`, not the monitor plugin.

## S12 Marketplace reality

**Question.** What does the live catalog contain by category and where are the gaps, from installed catalog data only.

**Method.** Searched the machine for catalog or depot data (`find` for `*depot*`, `*marketplace*`, `catalog*.json`; grep in `/usr/share/omarchy`). `omarchy-plugin-catalog` is a hidden helper that lists local manifests only, not a remote feed.

**Result.** No local catalog or marketplace data exists, so category counts and gaps cannot be measured offline. What is installed locally is the user's own set: 8 third-party plugins (`buggybuck.clock/lock/network/tasks/desk`, `dexterhere.omallama`, `kayooliveira.omadocker`, `org.jsongerber.vikunja`) in the categories Time, Network, System, Productivity, of which 3 are clones of first-party plugins (clock, lock, network). That is a sample of one user, not a catalog.

**Verdict.** NOT MEASURABLE offline. Needs network and was out of scope for this run.

**Consequence.** None for the design yet. Recorded in `docs/OPEN_QUESTIONS.md`. Note for the human step: the Plugin Depot, `omarchyplugins.com` and the official `omacom` catalog feed should be read by Dexter or an agent with network consent.

## S13 Free chord for the Widget Center

**Method.** Read `default/hypr/bindings/*.lua` and `apps/*.lua`, the user-visible `~/.config/hypr/bindings.lua` (read only), and `hyprctl binds -j` (read only). Modifier masks: SUPER 64, CTRL 4, ALT 8, SHIFT 1.

**Result.**
- `SUPER + W` closes the window (`bindings/tiling.lua:1`). `SUPER + SHIFT + W` is bound to Omawrite (`bindings/applications.lua:19`), so the brief's placeholder is taken on stock Omarchy 4. `SUPER + CTRL + W` is Network, `SUPER + CTRL + ALT + W` is Weather.
- `hyprctl binds -j` shows 225 live binds. With `W`, the modmasks present are 64, 65, 68 and 76; mask 72 (SUPER + ALT) has no `W`. The SUPER + ALT keys in use are F, G, K, S, comma, Home, arrows, Tab, Slash, Space, Return and the workspace code keys. The user's own `bindings.lua` binds `SUPER + H`, `SUPER + PERIOD`, `SUPER + SHIFT + B/R/S` and `SUPER + SPACE`.
- Bindings are Lua (`o.bind(keys, description, dispatcher)`); shell toggles use `omarchy-shell shell toggle <plugin-id>`.

**Proposed diff (not applied; to `~/.config/hypr/bindings.lua`, only after Dexter consents and a backup exists):**

```diff
+-- Widget Center
+o.bind("SUPER + ALT + W", "Widget Center", "omarchy-shell shell toggle <center-plugin-id>")
```

`<center-plugin-id>` is the final plugin id (placeholder until D-3). Undo is deleting those two lines. A second binding for a "widget board" peek can use `SUPER + ALT + SHIFT + W`, also unused (`hyprctl binds -j` shows no `W` at mask 73).

**Verdict.** PASS.

**Consequence.** Replace the brief's `SUPER + SHIFT + W` with `SUPER + ALT + W`. See SPIKE-S13. The consented write remains a human-approved step.

## What the desktop layer needs

A design for the spec-widget renderer plugin, built only from the facts above. Anything marked *untested* was read from source or inferred and must be confirmed when the plugin is first run inside the real shell.

**Shape: one Omarchy plugin, folder at the repo root.**

```
manifest.json            kinds: ["service", "overlay"], keepLoaded: true
                         entryPoints: { service: "Service.qml", overlay: "Center.qml" }
                         ow: { ... renderer metadata, see below }
Service.qml              root Item; daemon client, widget registry, per-monitor canvas windows
Center.qml               overlay: Widget Center and edit mode, summoned by shortcut
Theme.qml                singleton adapter: semantic roles -> qs.Commons tokens (the only file importing qs.Commons)
Client.qml               Socket + SplitParser NDJSON client with backoff (S3)
Registry.qml             discovers widgets, watches their folders
Canvas.qml               one PanelWindow per screen (S1), input mask from interactive rects
nodes/*.qml              one small component per layout node type
EditCanvas.qml           the S2 editor (pure QtQuick)
qmldir                   singleton Theme
```

Why one plugin: S6 shows cross-plugin imports are unsupported and the facade blocks cross-plugin services, while S7 means install is one `omarchy plugin add <repo>` at HEAD (the repo root is the plugin). A service plus an overlay in one folder shares state through the injected `service` property (read from `shell.qml`, *untested*).

**Manifest.** `schemaVersion: 1`, an id outside `omarchy.*`, `kinds: ["service","overlay"]`, `keepLoaded: true` (the connection and canvas survive rescans; the cost is code updates need a shell restart). Declare `property var shell`, `property string omarchyPath` and `property var manifest` on the roots so the host injects them (S5: the manifest copy includes our `ow` block). Do not use the `omarchy` key. No `barWidget` block. A bar icon, if wanted, would be a separate `bar-widget` kind entry in the same manifest.

**Where widgets live and how they are found.** Installed widgets sit in `$XDG_DATA_HOME/owd/widgets/<id>/{manifest,widget,layout}.json`. The renderer does not rely on Omarchy's plugin watcher (it watches only `~/.config/omarchy/plugins`, S5/S6 source). `Registry.qml` lists that directory with `Qt.labs.folderlistmodel` (module present on this machine, *untested*; fallback is a `Process` running `inotifywait`, which is how Omarchy itself watches plugins) and reads each JSON with `FileView { watchChanges: true; onFileChanged: reload() }`. Validation of the three files is done in the renderer with a defensive loader (unknown node types render a visible error card, never throw). `ow install` is the writer; the renderer is read-only on that tree. Because `Qt.createComponent` loads QML from any absolute file URL (S6), code widgets could be loaded from the same tree, but that bypasses Omarchy's add/enable review and trust prompts and is a product decision for Dexter (OPEN_QUESTIONS), not something to do by default. Spec widgets need no code, so this is not needed for them.

**Connecting to owd.** `Client.qml` is the S3 client: `Socket { path: runtimeDir + "/owd/owd.sock" }` with `SplitParser`, `hello` then `subscribe` per visible `(widget, topic)` on every `connected`, exponential backoff capped at ~10 s (and a visible "daemon offline" state), `unsubscribe` when a widget is hidden or off. The renderer sends the widget's claimed id with each request; grants stay authoritative in `owd` (DAEMON_SPEC). The renderer never holds or receives secrets, so nothing sensitive reaches QML properties. A same-uid code widget can still open the socket itself; the docs must keep saying so.

**Mapping `layout.json` to QML.** A `Loader` per node chooses a component from a node-type table (`value`, `label`, `ring`, `bar`, `sparkline`, `list`, `icon`, `row`/`column`/`stack` containers). Each component takes `node` (the JSON), `data` (the resolved binding from the subscription, a plain JS object), `theme` (`Theme` singleton) and `box` (size in logical pixels). Bindings (`"bind": "time.hm"`) are resolved by a small path lookup against the pushed `data`; a missing path shows the node's empty state; a `stale: true` push dims the node and shows its age. Size classes come from `widget.json`: the renderer picks the class for the current box and re-evaluates the layout, clamping invalid sizes rather than rejecting them. Interactive nodes (only those that declare it) are Items collected into a list that feeds the canvas input mask.

**Mapping theme roles.** Spec files name semantic roles only. `Theme.qml` maps them in one place:

| Role | Source (S8) |
|---|---|
| `text` | `Color.foreground` |
| `muted` | `Color.muted` |
| `accent` | `Color.accent` |
| `urgent` | `Color.urgent` |
| `surface` | `Color.popups.background` (carries its own alpha) |
| `border` | `Color.popups.border` |
| `success`, `warning`, `info` | parsed from the current theme's `colors.toml` (`green`, `yellow`, `blue`), falling back to `accent`/`urgent`; re-read when `Color.accent` or `Color.background` changes (*untested*; the shell only loads that file at startup) |
| fonts and sizes | `Style.font.family`, `Style.fontPx(mult)`, `Style.space(px)`, `Style.cornerRadius` |

Roles that are not in the table are an error shown in the widget, not a fallback to a hex colour. Sizes are expressed as fractions of the box and multiples of `Style.fontPx`, so text-size and spacing overrides apply.

**Canvas.** `Canvas.qml` is the S1 surface: `Variants` over `Quickshell.screens`, `WlrLayer.Bottom`, `keyboardFocus: None`, `exclusionMode: Ignore`, transparent, `mask` limited to interactive rectangles, namespace `owd-canvas` so a user layer rule can target it. Placement comes from `layout.json` keyed by monitor name set, with anchor, offset and size; unknown monitors fall back to the primary; hotplug rebuilds the `Variants` model without losing instances. Pause rendering and unsubscribe when the widget is hidden. Lock screen and fullscreen-window pausing are *untested* (S1).

**Edit mode.** `Center.qml` is an `overlay` kind plugin, summoned with `omarchy-shell shell toggle <id>` from the `SUPER + ALT + W` binding (S13). It hosts the S2 `EditCanvas` on an Overlay layer window and the Widget Center UI. Edits are written as `layout.json` by an atomic write (S2) or, better, through `owd`/`ow` so there is one writer. Escape and the close button hide the window; they never call `Qt.quit()`. Whether edit mode takes `Exclusive` or `OnDemand` keyboard focus must be decided with a person at the keyboard (S2).

**Settings.** Per-instance settings live in our own state, rendered from `ow.settings` in the manifest by the Center (S9). Nothing is written into `shell.json`, which the shell itself rewrites. Enable/disable of the renderer itself uses plain `omarchy plugin enable`.

**Install and update.** `omarchy plugin add <repo> --enable` installs the renderer (HEAD only, S7); `ow` installs widgets into the data tree and pins their commits itself, because `omarchy plugin add` cannot (S7). After `omarchy plugin update`, `ow doctor` re-validates the renderer manifest and the installed widgets against it.

**What must be verified once the plugin runs in the real shell** (each is a concrete test, not a guess): `service`/`overlay` property injection and the shared `service`; that `keepLoaded` keeps the socket across `omarchy-shell shell rescanPlugins`; click pass-through on the Bottom surface; Escape and focus on the Overlay surface; behavior on lock; theme re-read on `omarchy theme set`; the `FolderListModel` watcher.
