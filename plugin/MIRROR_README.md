# Omarchy widget layer (plugin)

An Omarchy shell plugin that draws no-code **spec widgets** (JSON files, nothing executed) on the desktop, using live data from the `owd` daemon.

**This repository is a generated mirror. Do not open pull requests here.** The source, the tests and the issue tracker are at <https://github.com/dexterhere/omarchy-widgets>; pull requests go there. `MIRROR.md` records the exact source commit and tag of this copy.

## Status

Pre-release. Be honest with yourself before installing:

- Spec widgets only. No code widgets.
- Not yet verified inside the real Omarchy shell. It has only run offscreen and in throwaway Quickshell instances. Click pass-through, lock screen, fullscreen windows and theme switching are untested live.
- It never calls `Qt.quit()`.
- The desktop tiles take no keyboard focus. The Widget Center overlay takes keyboard focus on demand (after a click) and uses it for Escape only.
- The Widget Center overlay is read-only: it lists installed widgets, what each may read and how each is doing, and prints the command to remove one. It cannot install or remove anything, and nothing is bound to open it yet.

## Install

```
omarchy plugin add https://github.com/dexterhere/omarchy-widget-shell
```

Not yet tested by the maintainers: nobody has run this command against this repository. It clones the code and runs it inside your desktop shell with your permissions, unsandboxed, so read it first (it is about a dozen small files). Without the `owd` daemon from the main repository it shows a "daemon offline" tile.

`omarchy plugin update` re-clones the default branch of this repository. It only ever holds tagged releases.

## What it reads and sends

- Reads the installed widget files in `$XDG_DATA_HOME/owd/widgets/` (size checked before reading).
- Connects to the daemon socket in `$XDG_RUNTIME_DIR` and sends only `hello`, `subscribe` and `unsubscribe`.
- Opens no network connection and starts no program. It holds no passwords or tokens; the daemon does.

## Security properties, in plain words

- Widget files are data. They are never run, evaluated, loaded as code or drawn as rich text, and unknown parts show an error tile.
- The daemon decides what each widget may see. This plugin only asks for data a widget declared and was granted.
- A static check in the main repository fails the build on `eval`, dynamic component loading, URL loading, `Qt.quit` and colour literals outside `Theme.qml`.
- Limits: the plugin cannot verify that the socket belongs to the real daemon, and any program running as you can edit your widget files. Plugins in general run unsandboxed.

## Reporting a problem

- Bug or request: <https://github.com/dexterhere/omarchy-widgets/issues/new/choose>
- Security problem (private, please do not use a public issue): <https://github.com/dexterhere/omarchy-widgets/security/advisories/new>

MIT licensed, see `LICENSE`.
