# Project brief

Status: planning v0.2, checked against public sources on 17 October 2026. Everything about Omarchy and Quickshell internals is **believed, not verified**; Phase 0 verifies it. Names, UI text and the `ow` command set are placeholders until the naming decision.

## 1. Vision

Give every Omarchy user one place to install, arrange, tweak and create desktop widgets, with their data handled safely.

- A **shortcut key** (and an optional bar icon) opens the **Widget Center**.
- Widgets can be **placed anywhere on screen, resized, removed, tweaked**, and always **follow the active Omarchy theme**.
- Three kinds of user, one system:
  1. Picker: installs from Discover, drags to place.
  2. Tinkerer: edits a spec widget or uses the visual designer, no code.
  3. Developer: scaffolds a code widget (QML), or asks an agent to.
  All three produce the same installable package.
- A **provider daemon** owns credentials, polling and caching. Widgets receive data, never tokens.
- A **website and registry** present the marketplace, previews in every theme, docs, and the contribution flow.
- It is **open source**, built to be contributed to.

Launch widgets: clocks (several designs), calendar, tasks, reminders (integrate with the existing first-party one), system graphs and usage, AI usage with auto-detection of installed agent harnesses, battery, music, Docker, Gmail, GitHub.

## 2. Decisions already made (change only with Dexter's approval)

| Topic | Decision |
|---|---|
| Strategy | Do not rebuild the runtime, marketplace or desktop-widget host. Build what is missing on top: data layer, free-form canvas and external widget package format, manager, maker, website. |
| Target | Omarchy 4+ only (Quickshell shell, plugin system). No Waybar-era support. |
| Host | Contact the author of `anishfn/omarchy-widgets` (MIT, grid-based desktop widget host, the same name we planned) before building a host. Build host-independent parts first (daemon, package spec, CLI, site). Contribute canvas and package loader upstream if welcome, otherwise fork under MIT with credit. Never build a host from scratch unless both fail. |
| Name | "Omarchy Widgets" collides with that repo. Placeholder: "Widget Hub for Omarchy". Also ask the Omarchy maintainers about using "Omarchy" in the name. Keep a fallback name. |
| Safety model | Offer **spec widgets** (declarative JSON, no code) and **code widgets** (QML, full power, reviewed tiers). Research a sandboxed or separate-process desktop host later. |
| Data-layer promise | Vault and daemon ship in v1 with a published threat model. Promise only: secrets never in plain text, least privilege for spec widgets, no leaks to logs. State plainly that a malicious code widget can read whatever the user can. |
| Packaging | `omarchy plugin add` runs no install scripts, so the daemon and CLI ship as an AUR/pacman package; shell parts and widgets ship as plugin repos. Monorepo for source, mirror repos for listing. |
| Languages | Rust for daemon, CLI, core. QML for in-shell parts. TypeScript for site/playground. JSON Schema as the shared contract. |

## 3. What already exists (do not duplicate)

- **Omarchy 4 "Quattro"** replaced Waybar, Walker, Mako, SwayOSD, hyprlock, hypridle and swaybg with one long-running Quickshell process (`omarchy-shell`). Bar, launcher, menus, notifications, panels, lock screen are plugins.
- **Plugin contract**: a directory with `manifest.json` (`schemaVersion: 1`, `id`, `name`, `version`, `author`, `description`, `kinds`, `entryPoints`) plus QML. Kinds: `bar-widget`, `panel`, `overlay`, `menu`, `service`, `bar`. Bar widgets add a `barWidget` block (display name, category, defaultSection, allowMultiple). Reserved `omarchy.*` id namespace. No symlinks. Exactly one manifest at repo root.
- **CLI**: `omarchy plugin add|enable|disable|remove|validate|clone|update|list`, `omarchy bar move|set|put`, `omarchy-shell shell rescanPlugins|summon|toggle|hide`, `omarchy theme set`. `plugin add` clones into `~/.config/omarchy/plugins/<id>/`, never runs install hooks, warns that plugins are unsandboxed. Saving files under the plugins dir reloads them.
- **Config**: `~/.config/omarchy/shell.json` is canonical (no deep merge) and also written by the shell, so write it only with atomic temp-file-and-rename. Theme tokens live in `shell.toml`, generated from the theme's `colors.toml`; `~/.config/omarchy/shell.toml` overrides live. Widgets that read tokens restyle live on `omarchy theme set`.
- **Menu extension**: `~/.config/omarchy/extensions/omarchy-menu.jsonc`.
- **Keybindings** live in `~/.config/hypr/bindings.lua` (Hyprland 0.55 moved config to Lua), e.g. `o.bind("SUPER + X", "desc", "omarchy-shell shell toggle <id>")`. Manifest-declared shortcuts are only an open upstream proposal (discussion 9631).
- **Marketplaces**: official `omacom/omarchy-plugin-marketplace` (issue-form submission, automated validation, maintainer approval of an exact snapshot, public catalog feed); community `omarchyplugins.com` (independent); **Plugin Depot** (`OmarCodeHub/omarchy-marketplace`, MIT): in-shell panel with pinned-SHA installs, update preview, drag-and-drop bar layout editor, and a detached job runner (needed because a plugin rescan destroys panels).
- **First-party widgets already shipped**: Clock, Monitor, Agents, Weather, Media, Battery, Power, Network, Bluetooth, Audio, Tailscale, Reminders, Clipboard, Emojis and more. About half the launch list overlaps; lead with what is missing (GitHub, Gmail, Calendar, Tasks, polished Docker, AI-usage extension) and ship the rest as **variants**.
- **Desktop widget hosts**: `anishfn/omarchy-widgets` draws cards on a layer-shell surface between wallpaper and windows, grid of 1 to 6 columns, size classes, overlay editor, `widgets.json`, `omarchy-shell widgets` CLI; new widget types are added in the host's own catalog; secrets (calendar URL, crypto addresses) sit in plain text in its config. Other desktop-widget plugins exist (Liquid Widgets and others).
- **Technique proven**: Quickshell `PanelWindow` with `WlrLayershell.layer` (Background/Bottom/Top/Overlay), empty input mask, `exclusiveZone: 0`, edited from a separate overlay surface.

What is missing (our scope): free-form placement and resize with third-party widgets; an external widget package format independent of any host; a vault-backed provider daemon; a maker (scaffolder, spec widgets, visual designer, agent skill); a showcase website with live previews.

Non-goals: new compositor integration, a Waybar port, a second marketplace front end competing with the official one, any sandbox claim we cannot back up.

## 4. Architecture

```
Inside the Omarchy shell (one Quickshell process)          Outside the shell
+------------------------------------------------+        +------------------------+
| Widgets (QML or spec)        Widget Center     |        | ow  CLI (Rust)         |
|      ^                        (manager panel)  |        | installs, packs,       |
|      | live values                 ^           |        | snapshots, doctor      |
| Widgets kit (QML)                  |           |        +------------------------+
|      ^                             |           |        | Registry index.json    |
| Bridge plugin (service) -----------+           |        | (static, built by CI)  |
|      |   Unix socket, line-delimited JSON      |        +------------------------+
+------|-----------------------------------------+
       v
  owd provider daemon (Rust, systemd --user)  --> vault (encrypted) --> keyring/passphrase/TPM
       |                                          cache (RAM only for sensitive data)
       +--> local sources (/proc, docker.sock read-only, agent logs)
       +--> remote APIs (GitHub, Gmail IMAP, ...) via per-provider host allow-list
```

Build order follows the arrows from the bottom: daemon and kit first, then CLI, then widgets, then the manager panel. Secrets never enter QML.

## 5. Widget Center (UX)

Entry points: configurable keybinding (the setup flow suggests a free chord and, only after consent, writes one marked block to `bindings.lua` with backup, conflict check and undo; the chord in the demo, SUPER + SHIFT + W, is a placeholder and must be checked against Omarchy defaults, since plain SUPER + W is believed to close a window); optional bar icon with update/error badge; Omarchy menu row; `ow center` / `omarchy-shell shell toggle <plugin-id>`.

Tabs:
- **Installed**: every widget with on/desktop switch, instance count, update badge, health (a provider that lost its token shows here, not as a silent blank card). Selecting one opens settings, variants, grants.
- **Discover**: marketplace with categories, trust badge, the exact capabilities requested ("reads GitHub notifications; talks to api.github.com"), previews in the user's own theme; install is one click and pins a commit.
- **Layouts**: named layouts (Work, Gaming), switchable by shortcut, exportable as packs.
- **Create**: three doors: tweak/design visually from a template, describe it to an agent, or scaffold a folder and open the editor.

Edit mode (desktop dims, every widget shows handles): drag to move; corner/edge resize; arrow keys nudge, shift-arrows resize; snap modes off/grid/edges+neighbors with guides; multi-select to align, distribute, group, lock; duplicate; remove to a tray with undo; automatic snapshot before the first change of a session; inspector (size class, opacity, variant, data source, settings, all typed values, never only drag); fully keyboard reachable.

Widget board (peek): widgets normally sit under windows; a second shortcut lifts them above everything while held or toggled.

## 6. Desktop canvas

- One display surface per monitor on `WlrLayer.Bottom`, click-through by default; an interactive widget declares rectangles that take input and only those become input regions.
- Edit mode uses a second surface on the Overlay layer that dims, takes pointer and keyboard, and hides on Escape.
- Placement: each instance stores monitor, anchor (corner, edge, center), offset, size in logical pixels, size class, z-order. Anchors make layouts survive resolution or scale changes. Layouts are keyed by connected monitor set; unknown monitor falls back to primary; hotplug never loses an instance.
- Resize: a widget declares size classes (compact, wide, tall, large) with min/max and optional aspect lock. Inside a class, content scales continuously (type as fractions of the card). Crossing a class boundary recomposes the layout (show more, not the same drawing bigger). Invalid sizes are clamped, never rejected.
- Code widget contract: inputs are theme tokens, `size`, `sizeClass`, settings, data subscriptions, an `editing` flag. Requests: `requestResize`, `openPanel`, `openLink`. A widget cannot move or close itself. Lifecycle: visible, hidden, suspended (stop animating and sampling when hidden).
- Resilience: pause updates when a fullscreen window covers the monitor or the session is locked; subscribe to providers only while visible; show an error card when a widget fails to load; CI lints every widget because a QML fault can still take the shell down.
- Remove means "off" first: the instance goes to a tray with settings kept.

## 7. Widget package spec

A widget is a standard Omarchy plugin plus a sidecar `widget.json`, so it passes `omarchy plugin validate` and can be listed in the official marketplace unchanged. (Whether the validator tolerates extra manifest keys is a Phase 0 question; if yes, fold the sidecar into the manifest.)

Packaging rules: reverse-domain id outside `omarchy.*` (e.g. `io.github.you.github-activity`); exactly one `manifest.json` at repo root; no symlinks; README with install/removal steps; license; documented external dependencies; `plugin add` runs no scripts so helper binaries must be documented and the widget must degrade without them.

Illustrative `widget.json` (draft v0, to be formalized as JSON Schema in Phase 0):

```json
{
  "schemaVersion": 0,
  "id": "io.github.you.github-activity",
  "tier": "spec",
  "category": "dev-git",
  "surfaces": ["desktop", "bar"],
  "sizeClasses": [
    { "name": "compact", "min": [160, 100], "max": [240, 160] },
    { "name": "wide", "min": [280, 120], "max": [480, 200], "aspectLock": null }
  ],
  "variants": ["list", "counts"],
  "providers": [
    { "name": "github", "topics": ["github.prs"], "fields": ["open", "review"] }
  ],
  "capabilities": { "network": [], "files": [], "spawn": [] },
  "interactiveRegions": [],
  "settings": [
    { "key": "interval", "type": "int", "default": 60, "min": 30, "max": 600 }
  ],
  "compat": { "omarchy": ">=4.0", "widgetsKit": "^1" }
}
```

A widget never stores or declares a secret; it names the provider and topics it needs and the daemon supplies fields.

Illustrative spec-widget layout (blocks: value, label, ring, bar, sparkline, list, icon; colors are semantic roles only):

```json
{
  "layout": [
    { "block": "value", "bind": "time.hm", "size": "xl", "color": "accent" },
    { "block": "label", "bind": "time.date", "color": "muted" }
  ],
  "providers": ["system.time"]
}
```

## 8. Theming and customization

Widgets never pick colors. They read semantic roles from the shell's theme singletons; a lint rule fails any widget with color literals. Four layers: (1) theme tokens, automatic; (2) schema-driven settings stored in `shell.json`; (3) variants (layouts and skins of one data model, never hard-coded color); (4) eject: `ow eject <id>` copies the widget into the user's plugin dir under their own id, keeps position and settings, shows an "ejected" badge and offers a diff against upstream.

Design rules: sizes scale with the shell's text-size setting (no fixed pixel fonts); honor font/spacing/bar-height overrides in machine-level `shell.toml`; render useful empty, loading and error states; a failed provider never blanks the bar; CI renders a preview on at least three themes (light, dark, high contrast).

## 9. Data providers

| Provider | Source | Auth | Notes |
|---|---|---|---|
| system | /proc, /sys, GPU tools | none | `omarchy.monitor` exists; reuse before duplicating |
| battery, media | UPower, MPRIS | none | first-party widgets exist; add variants |
| docker | Docker Engine API, local socket | socket access | expose only an allow-listed read-only subset; actions need per-widget opt-in |
| github | REST/GraphQL | device flow or fine-grained token | notifications, PRs, issues, contribution graph |
| gmail | IMAP | app password, or user's own OAuth client (loopback PKCE) | never a shared hosted OAuth client (restricted scopes; verify Google's current policy) |
| calendar, tasks | ICS/CalDAV, local task file | none or basic | local-first; secret iCal URLs are secrets |
| ai-usage | local session logs of installed agent harnesses | none | counts and timings only, never prompt content; one adapter per harness, fixture-tested, versioned; per-harness opt-in |

Transport: Unix socket in `$XDG_RUNTIME_DIR`, mode 0600, line-delimited JSON, subscribe and snapshot calls. One service-kind plugin bridges the socket into a QML singleton imported from the kit. Stale data shows its age instead of disappearing. No telemetry, no cloud relay.

## 10. Secure data layer (core priority)

Threat model summary (full detail and wire protocol in `DAEMON_SPEC.md`):

| Threat | Protection | Residual risk |
|---|---|---|
| Secrets in plain-text configs, dotfile repos, backups | Secrets only in the vault; configs hold `secret:<name>` references | none if the reference rule holds |
| Stolen laptop/disk | Omarchy enables LUKS by default (skippable); vault encrypts on its own | memory after login |
| Copies of home dir (snapshots, rsync, cloud backup) | Vault file AEAD-encrypted, master key never stored beside it | protection of the master key (see modes) |
| Other users/sessions | Socket 0600 in `$XDG_RUNTIME_DIR`, `SO_PEERCRED` uid check | other processes of the same user |
| Over-broad tokens | Least-scope tokens, shown with the widgets using them | user choice |
| Leaks via logs, argv, env, crash dumps, previews | No secrets in argv/env, log redaction, core dumps off, `PR_SET_DUMPABLE=0`, locked and zeroized key memory, masked previews | user screen capture |
| Spec widget or compromised data source | Spec widgets run no code and see only granted fields | granted data |
| **Malicious or buggy code widget** | **Cannot be prevented**: unsandboxed, runs as the user, can read anything including an unlocked keyring. Reduced by tiers, review, declared capabilities, lint, spec-first design | Real. Say so in UI and docs |
| Supply chain | Locked deps, cargo-deny, cargo-audit, reproducible builds, signed releases, SBOM | upstream compromise |

Vault modes (chosen at `ow vault init`, changeable): **Keyring** (default; Secret Service via D-Bus; caveat: Omarchy logs in automatically after disk unlock and an older-release user report found gnome-keyring effectively unprotected; verify on Omarchy 4), **Passphrase** (Argon2id, key held in the kernel keyring with a timeout), **TPM-bound** (systemd-creds, where a TPM exists; spike).

Cache policy: sensitive data in memory only by default; persisting is opt-in, encrypted under the data key, time-limited; public low-sensitivity data (weather) may be cached in plain files. Removing a token purges everything derived from it; `ow vault wipe` removes all.

Provider authentication: GitHub device flow (no client secret shipped) or fine-grained token; Gmail IMAP app password or BYO-OAuth with loopback PKCE; Docker read-only subset; AI usage counts only; network egress per-provider hostname allow-list, system TLS roots only, no cross-host redirects.

Grants and audit: grants are approved by the user in the Widget Center; bridge and daemon both filter. For spec widgets this is a hard boundary (only the renderer talks to the daemon). For code widgets it is a guardrail for honest code, not a barrier. Every provider access goes to a local redacted audit log readable in the Center, with per-widget rate limits.

Process hardening: systemd user service with NoNewPrivileges, ProtectSystem=strict, restricted ProtectHome with one writable state dir, PrivateTmp, restricted address families, empty capability set, syscall filter; check with `systemd-analyze security`.

Assurance before 1.0: published threat model, fuzzing of socket and spec parsers, CI audit gates, independent review of vault and protocol, security policy with private disclosure, signed releases. The vault is the one component that must be boring, small and reviewed.

## 11. Manager (`ow` CLI first, then a thin QML panel)

Does what `omarchy plugin` and Plugin Depot do not: per-widget settings and variants, provider grants, packs, snapshots/rollback, health checks (doctor), capability diff on update.

Rules: prefer the official CLI for every change it supports; edit `shell.json` directly only for what it cannot express, atomically; idempotent and scriptable (`ow <verb> --json --yes`); never block the shell (long work in CLI/daemon; panels are destroyed on plugin rescan, so mutations run as detached jobs that re-summon the panel when done, as Plugin Depot's job runner does).

First-cut verbs: `ow new`, `check`, `install`, `enable`, `place`, `pack export|import`, `snapshot`, `rollback`, `eject`, `doctor`, `search`, plus `ow vault ...`, `ow grants ...`, `ow audit ...`. Consider contributing packs and snapshots to Plugin Depot instead of shipping a second panel.

## 12. Maker (four levels)

0. Scaffolder (`ow new`): templates (text item, item plus popup, chart, list, service); writes manifest, QML, README, license, preview; runs validate. 1. Spec widgets (JSON bound to provider fields, drawn by a generic renderer with theme tokens). 2. Visual designer (shell panel, live preview across themes, exports a spec). 3. Agent-assisted (a skill and `llms.txt` teach an agent the kit, spec and `ow check` loop).

Ship levels 0 and 3 first (need only kit + docs); start 1 and 2 after the kit has settled across the v0.1 set. The spec renderer is a framework in its own right: keep the schema small.

Kit (shared QML): theme-aware cards, rings, bars, sparklines, list rows, provider client, settings helpers, formatters, lint rules; own semver, pinned by `compat.widgetsKit`.

## 13. Registry, trust and contribution

Static `index.json` built by CI from listing files in a Git repo, served from GitHub Pages: no servers, accounts or databases. Categories: System, Time and Calendar, Productivity, Dev and Git, AI and Agents, Media, Communication, Network and Containers, Ambient and Fun, Utilities.

Trust tiers: **Core** (this project; reviewed, tested on every Omarchy channel, signed tags), **Verified** (community, a maintainer read the pinned commit, capabilities match the code, builds and previews pass; pinned to reviewed SHA), **Spec** (schema-valid, runs no custom code), **Community** (passes validate and lint; shows an "unreviewed code" warning). Lint (process spawns, undeclared network hosts, file access outside declared paths) is advisory; tiers describe review, not isolation, and the UI must not imply otherwise. Keep Verified small.

Contributor flow: fork, `ow new`, develop with hot reload against a throwaway shell config, `ow check` (validate, kit lint, capability cross-check, screenshots on three themes), PR adding a listing file (repo + commit SHA, or the widget folder), CI repeats checks and posts previews, maintainer reviews, merge publishes in the next index build.

Governance: MIT license, CODEOWNERS per widget, short maintainer team, code of conduct, RFC process for kit/spec/sidecar changes, semver with deprecation window, security policy with private disclosure and fast takedown path, a contribution ladder that is not only code (screenshots, variants, translations, provider adapters, docs, triage), labeled good-first-widget issues.

## 14. Website

Static, no accounts, no cookies. Pages: Home (hero demo switching the same desktop across Omarchy themes, install command), Gallery (category, search, filters by tier/provider/size/surface), Widget page (preview with theme switcher and size slider, variants, requested capabilities, changelog, install command, source), Packs, Playground (browser designer for spec widgets, exports the spec), Docs, Security (threat model, tiers, reporting).

Data pipeline: GitHub Actions merges our listings with the official marketplace's public catalog feed (credited, linked out, cached politely, subject to its terms), validates every listing, builds `index.json` and static pages, deploys to static hosting. Previews: CI renders each widget from real QML per size class and theme on a headless/nested compositor (Phase 0 spike); the site swaps pre-rendered images. Spec widgets also render live in the browser via a TypeScript renderer, with golden-screenshot tests against the QML renderer; build it only after the spec schema is stable. Footer states the site is independent of Omarchy and 37signals unless that changes.

## 15. v0.1 launch set

New: GitHub, Gmail, Calendar, Tasks, polished Docker (partner with the existing community Docker monitor), AI-usage extension (coordinate with `omarchy.agents` owners). Variant packs: Clock (digital, analog, binary, world, countdown), Monitor (history graphs), Media (cover art, mini, visualizer), Battery. Skip: Weather, Reminders (integrate with Tasks). Six widgets plus four variant packs prove the daemon, kit and manager. Re-check the live first-party list before committing to any row.

## 16. Risks (short list; full table in the plan)

Upstream contract changes (pin `schemaVersion: 1`, isolate Omarchy calls in one adapter module, CI against stable/RC/edge); duplicating upstream or Plugin Depot (open discussions first); malicious/buggy widgets (spec widgets, pinned installs, capability diff, honest labels, takedown path); a QML error taking down the shell (logic in the daemon, qmllint, headless test, guard every provider read); provider format drift (adapters, fixtures, "unsupported version" state); Gmail/OAuth policy; maintainer overload; naming friction; scope creep (ship phases in order); host collision (decide the fork fallback in Phase 0); overstated security; keyring weakness under autologin; Docker socket exposure; two renderers drifting.

## 17. Design reference (approved demo)

The approved visual demo lives in `design/`. See `design/README.md` for the artboards, interactions, theme palettes (Tokyo Night, Gruvbox, Catppuccin, Rose Pine, Catppuccin Latte) and the exact widget set shown. Treat it as the target look and behavior, not as production code.
