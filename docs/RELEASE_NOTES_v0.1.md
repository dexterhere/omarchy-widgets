# Release notes: v0.1.0 (developer preview)

This is a developer preview for people who run Omarchy 4 and are comfortable reading code before they run it. It is not a stable release, and the package is not yet built from a signed tag.

## What works

- `owd`, the provider daemon, runs as a systemd user unit and serves `system.time`, `system.cpu`, `system.memory`, `system.load`, `system.temp` and `battery.status`. `github.prs` is served only after you connect GitHub with a read-only token (`ow connect github`).
- `ow install`, `ow list` and `ow remove` fetch a widget from the website registry at a pinned commit, show what it will be allowed to read, and write a grant only after you confirm.
- The shell plugin draws installed widgets on the desktop. A widget that cannot be drawn shows `Widget unavailable`.
- `ow doctor` checks the install and says what is missing.

## What is and is not protected

- The daemon holds credentials. Widgets never receive them, and a widget can read only the topics and fields it was granted.
- `ow` checks the downloaded files against the website's rules, the registry's pinned commit and, when the registry index carries them, per-file sha256 hashes. That is not a review of the widget by a person.
- The daemon knows a widget only by the id it claims. Another program running as you could claim the same id, so a grant limits honest widgets and keeps credentials out of reach. It is not a barrier against your other programs.
- QML code widgets are not sandboxed. They run inside your shell and can read anything you can. Install only code you have read or that a person you trust has reviewed.
- Spec widgets contain no code: the plugin draws them from data and never runs their content.
- The vault and the wire protocol have not had an external security review. Do not store credentials you cannot rotate.

## Known limits

- Only the seven first-party widgets are listed. Their website status is still `planned` until the in-shell enable test has been run with a person present.
- The Widget Center is read-only and nothing is bound to open it yet.
- Not tested: machines without a battery or without the CPU temperature sensor, and a real relogin autostart of the user unit.
- The installed plugin must be added from its own repository with `omarchy plugin add`; it runs unsandboxed, so read it first.
