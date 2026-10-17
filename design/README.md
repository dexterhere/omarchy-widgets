# Design reference

These files are the approved concept demo, authored for a design canvas (`.dc.html`). They are **reference for look and behavior**, not production code. Open them in a browser only if the support runtime is available; otherwise read them as markup and logic. Widget data in them is sample data. Name, UI text and `ow` commands are placeholders.

## Artboards

| File | What it shows |
|---|---|
| `Main.dc.html` | Interactive desktop (1440x900): top bar with a widgets button, ten themed widgets, edit mode (drag to move, corner to resize, remove, 20 px snap grid), and the Widget Center bottom sheet with tabs Installed, Discover, Layouts, Create, a theme switcher, and an inspector (size class, layer Background/Bottom/Top, accent role, granted data, remove). |
| `Marketplace.dc.html` | Marketplace website: hero with theme preview, category filter, widget cards that state tier and requested data, "Make your own" levels, "Your data stays yours", contribute steps. |
| `Daemon-what.dc.html` | Diagram: shell, `owd` components, and external sources with the colored flows (requests, live data, secrets, consent). |
| `Daemon-flow.dc.html` | Sequence diagram of one GitHub request, nine steps. |
| `Daemon-build.dc.html` | Crate layout, wire format sample, seven build steps, systemd hardening, honest-limits note. |

## Behaviors to reproduce

- Desktop widgets sit under windows (Bottom layer). Opening the Widget Center enters edit mode: the grid appears, every widget shows a dashed outline; the selected one has a solid outline, a remove button and a resize handle.
- Installing a widget places it automatically at the first free grid slot; "Add to desktop" and "Remove from desktop" are separate from install; removal keeps the widget installed.
- Layout cards show a miniature of widget rectangles; "Save current layout" stores the current arrangement.
- Switching the theme restyles every widget at once; there are no per-widget hex colors, only roles (accent, ok, warn, bad, purple, muted, foreground, panel).
- Each widget adapts to size: the clock shows seconds only when wide enough and scales its numerals; the system widget shows its graph only when tall enough.
- The Create tab presents three doors (tweak, design visually, write code) and a small spec-widget JSON sample.
- Discover cards state the data a widget needs (for example `github.read, net: api.github.com`) before install.

## Theme palettes used in the demo

These approximate the named Omarchy themes for the mockup. In the product, read the live shell theme tokens; never hard-code these.

| Role | Tokyo Night | Gruvbox | Catppuccin | Rose Pine | Latte (light) |
|---|---|---|---|---|---|
| wall | #13141c | #1d2021 | #11111b | #12101c | #ccd0da |
| background | #1a1b26 | #282828 | #1e1e2e | #191724 | #eff1f5 |
| panel | #1f2335 | #32302f | #262637 | #1f1d2e | #e6e9ef |
| panel 2 | #292e42 | #3c3836 | #313244 | #2a273f | #dce0e8 |
| line | #3b4261 | #504945 | #45475a | #403d52 | #acb0be |
| foreground | #c0caf5 | #ebdbb2 | #cdd6f4 | #e0def4 | #4c4f69 |
| muted | #9aa5ce | #bdae93 | #a6adc8 | #a09cbc | #5c5f77 |
| accent | #7aa2f7 | #fabd2f | #89b4fa | #c4a7e7 | #1e66f5 |
| ok | #9ece6a | #b8bb26 | #a6e3a1 | #9ccfd8 | #2f8a1f |
| warn | #e0af68 | #fe8019 | #f9e2af | #f6c177 | #b86e0a |
| bad | #f7768e | #fb4934 | #f38ba8 | #eb6f92 | #d20f39 |
| purple | #bb9af7 | #d3869b | #cba6f7 | #ebbcba | #8839ef |

## Type and style

JetBrains Mono for desktop and headings (matches Omarchy's terminal feel), IBM Plex Sans for body text on the website and diagrams. Small radii (4 to 6 px), 1 px borders, flat panels, no gradients, no emoji. Minimum body text 12 px on the desktop, 14 px on web pages. Interactive controls are real buttons with accessible names.

## Diagram color key

Blue: requests. Green: live data. Amber: secrets. Purple: consent and grants. Tinted boxes: built by this project. Plain boxes: already on the machine or online.
