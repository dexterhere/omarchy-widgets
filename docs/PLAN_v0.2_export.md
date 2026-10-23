<!-- Auto-exported from the planning document "Omarchy Widgets Plan v0.2" (17 Oct 2026). Tables are flattened to line-per-cell and a few illustrative code blocks were lost in export; PROJECT_BRIEF.md and DAEMON_SPEC.md are the maintained versions. Kept for the sources list and extra detail. -->


> **Historical, not maintained.** Prompts 1 to 6 have largely been executed; the Phase 0 spikes S1 to S10 have not. This export is kept for its sources list and extra detail only. The current status is in [HANDOFF.md](HANDOFF.md) and [BUILD_PLAN.md](BUILD_PLAN.md) in the same folder.

# Omarchy Widgets — Plan v0.2
 · 

## Summary
Don't build a new runtime, marketplace or desktop-widget host from scratch: Omarchy 4 ships the first two, and a community desktop-widget host already exists. Build what is missing on top of them: a secure data layer, free-form placement with installable third-party widget packages, a manager and maker, and a showcase website.
- 
Runtime and marketplaces exist. Omarchy 4 ("Quattro") replaced Waybar and its companions with one Quickshell shell whose bar, panels and overlays are plugins (a folder with manifest.json plus QML, managed with omarchy plugin). An official marketplace, a community one and the Plugin Depot manager panel are live.
- 
Desktop widgets exist too. anishfn/omarchy-widgets (MIT, and the very name we planned) draws a grid of cards on the wallpaper layer with a drag-and-drop editor, a JSON config and a CLI, and other desktop-widget plugins have followed. But new widget types are added by editing that host's own catalog, placement is grid-only, and secrets such as a calendar's private URL sit in plain text in its config file.
- 
What is missing: free-form placement and resize, an external widget-package format so the marketplace can ship widgets independently of any host, a vault-backed data layer, a maker, and a showcase website with live previews.
- 
Recommended path: contact the host's author first; build the host-independent pieces (data layer, package spec, CLI, website) in the early phases; contribute the canvas upstream if welcome, or fork under MIT if not.
The rest of this document turns those gaps into an architecture, a package spec, a contribution flow and a phased roadmap.

## Decisions to settle in the discussion
- 
Host strategy. Contribute to anishfn/omarchy-widgets, fork it, or write a host from scratch. Recommended: contact the author this week and build the host-independent pieces first; contribute the canvas and package loader upstream if welcome, otherwise fork under the MIT license with credit.
- 
Name. "Omarchy Widgets" is already that repository's name and its plugin is anishfn.widgets. Recommended: pick a distinct name (placeholder: "Widget Hub for Omarchy") unless a joint project is agreed, and ask the Omarchy maintainers about using the Omarchy name.
- 
Target. Omarchy 4+ only; Waybar-era installs have no plugin host. Recommended: yes.
- 
Safety model. QML plugins run unsandboxed. Offer spec widgets (declarative, no code) and code widgets (full power, reviewed tiers), and research a sandboxed host for desktop widgets later. Recommended: both.
- 
Data-layer promise. Ship a vault and a provider daemon in v1 with a published threat model. Promise only what can be kept: secrets never in plain text, least privilege for spec widgets, no leaks to logs; and say plainly that a malicious code widget can read whatever the user can.
- 
Packaging. omarchy plugin add runs no install scripts, so the daemon ships as an AUR or pacman package and widgets as plugin repos. Recommended: monorepo for source, mirrors for listing.

## What already exists (October 2026)
Omarchy 4 turned the desktop into a plugin host, so most of the plumbing you planned to build is upstream and documented.
Piece
What exists today
Implication
Shell
One long-running Quickshell process hosts bar, launcher, menus, notifications, OSDs, panels, lock screen and polkit agent as plugins. Waybar, Walker, Mako, SwayOSD, hyprlock, hypridle and swaybg are gone.
Target this; do not run a second shell.
Plugin contract
A plugin is a directory with manifest.json (schemaVersion: 1, id, name, version, kinds, entryPoints) plus QML. Kinds: bar-widget, panel, overlay, menu, service, bar. Bar widgets add a barWidget block (category, default section, allow-multiple).
Our widgets are ordinary plugins.
CLI
omarchy plugin add / enable / disable / remove / validate / clone / list, plus omarchy-shell shell rescanPlugins to reload plugins (saving a file under the plugins directory also reloads it) and summon / toggle / hide per plugin.
The manager wraps this rather than replacing it.
Config
~/.config/omarchy/shell.json is canonical (no deep merge) and stores layout, per-entry settings and enabled plugins. Theme tokens live in shell.toml, generated from the theme's colors.toml; a machine-level ~/.config/omarchy/shell.toml overrides the theme live.
Adaptive color comes for free if widgets use the shell's tokens.
Official marketplace
omacom/omarchy-plugin-marketplace: submit a public GitHub repo (manifest at root, README, license) through an issue form; automated validation, then maintainer approval.
Listing path already exists.
Community marketplace
omarchyplugins.com (independent): same submission shape, anonymous aggregate stats, no accounts.
Another listing path; shows the pattern works.
In-shell manager
Plugin Depot (omarchy-marketplace): a shell panel to browse, install, update, enable and remove, with installs pinned to the reviewed commit, update previews and a drag-and-drop bar layout editor.
Already covers install, update and placement; build on it rather than rebuilding it.
First-party widgets
Clock, Monitor, Agents, Weather, Media, Battery, Power, Network, Bluetooth, Audio, Tailscale, Reminders, Clipboard, Emojis and more ship with the shell.
About half of the launch list already exists.
Third-party widgets
A Docker monitor, agent switchers, an OpenClaw widget, Shibumi Shell (24 plugins in one process) and security scanners such as OmaSafe.
Study before building; partner where possible.
Desktop widget hosts already exist. anishfn/omarchy-widgets (MIT; 17 stars and 8 forks when I looked) draws cards on a layer-shell surface between the wallpaper and your windows. It has a grid of 1 to 6 columns on the left, the right or both sides, per-type sizes such as 1×1, 2×1 and 2×2, a layout editor on an overlay surface with drag, drop, duplicate and per-card settings, a widgets.json config, an omarchy-shell widgets CLI and ten widget types (clock, weather, GitHub graph, repo stats, crypto, calendar, todos, music, photos and a desktop-pet card). A fork adds any installed bar-widget plugin as a card. Others include Liquid Widgets, Nothing Glass and an information-wallpaper overlay.
One blog review reports roughly 3,000 plugins within three weeks of the marketplace opening. I could only confirm that claim secondhand, so treat the size of the ecosystem as unverified until you check the catalog yourself.

## Positioning: the widget layer on top of Omarchy
Omarchy Widgets is a curated suite, a toolkit and a manager that sit on Omarchy's plugin system. It does not replace the shell, the plugin format or the official marketplace, and every widget stays installable with plain omarchy plugin add.
Gap
Why it matters
What we build
No way to make a widget without writing QML
The "maker" idea is the largest unserved need and the best contributor on-ramp.
Scaffolder, visual designer, declarative spec widgets, agent skill.
Every widget polls and authenticates on its own
Ten widgets means ten Docker polls, ten GitHub tokens and ten sets of bugs.
One provider daemon; widgets read from it and never see credentials.
One look per widget
Users want several designs of the same data, all matching the theme.
Variants (layouts and skins) per widget, driven by tokens.
No layout presets
Moving widgets one by one does not scale; people share setups.
Packs: exportable slices of shell.json plus the widgets they need.
Desktop widgets are grid-only and closed to third-party packages
Existing desktop hosts snap cards to a grid and add widget types by editing the host's own catalog.
Free-form canvas and an external widget-package format, preferably contributed to the existing host.
Nothing declares what a plugin can touch (the marketplace verifies snapshots, not behavior)
Unsandboxed QML can do anything the user can.
Declared capabilities, a capability diff on update, advisory lint, spec widgets that run no code.
Non-goals: a new compositor integration, a Waybar port, a second marketplace front end that competes with the official one, and any sandbox claim we cannot back up.

## Architecture
Widgets, the kit and the manager panel run inside the shell and reach outside data only through a thin bridge plugin to one provider daemon; the CLI and the registry stay outside the shell, and secrets never enter QML. Build order follows the arrows from the bottom: daemon and kit first, then the CLI, then widgets, then the manager panel.

## User experience: one Widget Center
Everything a user does with widgets starts in one overlay, the Widget Center, opened from a key, the bar or the menu, and it serves three kinds of user: people who only want to pick, people who want to tweak, and people who want to build.
Entry point
How it works
Constraint
Keybinding
Toggles the Widget Center; the chord is the user's choice and the setup flow suggests a free one.
Plugins cannot declare shortcuts yet (an upstream proposal is open), so the first-run step adds one marked block to ~/.config/hypr/bindings.lua, with a backup, a conflict check and an undo, and only after the user agrees.
Bar icon
A small bar widget that opens the center and shows an update or error badge.
Optional: hiding it should keep the plugin's layout entry so the center stays reachable, as Plugin Depot does.
Omarchy menu
A row under Setup, defined in ~/.config/omarchy/extensions/omarchy-menu.jsonc.
Lives outside the plugin folder, so removal needs a documented cleanup step.
Command line
ow center, or omarchy-shell shell toggle <plugin-id>.
For scripts and dotfile managers.
Widget Center tabs:
- 
Installed: every widget with an on/off switch, instance count, update badge and health (a provider that lost its token shows here, not as a silent blank card). Selecting one opens its settings, variants and provider grants.
- 
Discover: the marketplace, with categories, a trust badge, the exact capabilities a widget asks for ("reads GitHub notifications; talks to api.github.com"), and previews drawn in the user's own theme. Install is one click and pins a commit.
- 
Layouts: named layouts such as Work and Gaming, switchable by shortcut, and exportable as packs.
- 
Create: three doors on one screen: start from a template and design visually, describe the widget to an agent, or scaffold a folder and open the editor.
Editing on the canvas (the desktop dims and every widget shows handles):
- 
Drag to move; drag a corner or edge to resize; arrow keys nudge and shift-arrows resize.
- 
Snap modes: off, grid, or edges and neighbors, with visible guides.
- 
Select several to align, distribute, group or lock.
- 
Duplicate with a click; remove by dropping on a tray, with undo and an automatic snapshot before the first change of a session.
- 
An inspector for the selected widget: size class, opacity, variant, data source and its own settings, all editable by typing, never only by dragging.
- 
Everything is reachable by keyboard, since Omarchy users expect that.
Widget board (peek): widgets normally sit under windows, so a second shortcut lifts them above everything for as long as it is held or toggled, which also makes interactive widgets usable when a window covers them.
Three skill levels, one system: a beginner installs from Discover and drags; a tinkerer opens the visual designer and edits a spec; a developer scaffolds a code widget. All three produce the same installable package, so a beginner's design can be handed to a developer later with no conversion.

## Desktop canvas: place, resize, remove
The technique is proven: existing plugins draw widget cards on a layer-shell surface on the Bottom layer with an empty input region and no reserved space, and edit them from a separate overlay surface. What none of them offer yet is free-form placement and resize with third-party widgets, which is the canvas to build.
Surface model
- 
One display surface per monitor on WlrLayer.Bottom, click-through by default. An interactive widget declares the rectangles that take input, and only those become input regions.
- 
Edit mode uses a second surface on the Overlay layer that dims the desktop, takes the pointer and keyboard, and hides again on Escape. This separation means a card never has to take focus while it is being arranged.
- 
The widget board lifts the display surface above windows temporarily.
Placement model
- 
Each instance stores its monitor, an anchor (corner, edge or center), an offset and a size in logical pixels, plus its size class and z-order. Anchors let a layout survive a resolution or scale change.
- 
Layouts are keyed by the connected monitor set; an unknown monitor falls back to the primary one, and hotplugging never loses an instance.
- 
Snap to grid or to neighbors is a user setting, off by default in free mode.
Resize model
- 
A widget declares size classes (for example compact, wide, tall and large), each with minimum and maximum sizes and an optional aspect lock.
- 
Inside a class, content scales continuously, with type sized as fractions of the card. Crossing a class boundary recomposes the layout, so a larger widget shows more rather than the same drawing blown up. The existing host already follows this rule per size.
- 
Invalid sizes are clamped, never rejected.
Widget component contract (code widgets)
- 
Inputs: theme tokens, size and sizeClass, settings, data subscriptions from the data layer, and an editing flag.
- 
Requests: requestResize, openPanel, openLink. A widget cannot move or close itself.
- 
Lifecycle: visible, hidden and suspended. A widget must stop animating and sampling when hidden.
Performance and resilience
- 
Pause updates when a fullscreen window covers the monitor or the session is locked, and subscribe to providers only while visible.
- 
Show an error card when a widget fails to load instead of dropping the surface. Reports show that a QML fault can still take the shell down, so CI lints every widget and a later phase explores a separate-process host.
- 
Remove means "off" first: the instance goes to a tray with its settings kept, and deleting the last instance of a type is only a switch-off, as in the existing host.
Building on the existing host. The same canvas can be reached three ways:
Option
Pros
Cons
Verdict
A. Contribute to anishfn/omarchy-widgets
One community, working editor and tests, MIT license.
Grid-first design, widget types live in the host's catalog, depends on a small project's maintainer.
Propose first.
B. Fork it
Full control of architecture; keeps their editor and CLI ideas.
A second host splits users; you carry its maintenance.
Fall back to this if A stalls.
C. New host from scratch
Cleanest design for free-form canvas.
Rebuilds an editor, config, CLI and tests that exist.
Not recommended.
Whichever wins, build the host-independent parts first (data layer, widget package spec, CLI, website), because they are needed in every case.

## Widget package spec
A widget is a standard Omarchy plugin plus one sidecar file, so it passes omarchy plugin validate and can be listed in the official marketplace unchanged.
The sidecar keeps our fields out of Omarchy's manifest, because I have not confirmed that the validator tolerates unknown keys (see open questions). Illustrative widget.json:=4.0", "widgetsKit": "^1" }
}]]>
Packaging rules (taken from the official submission and plugin add behavior):
- 
Reverse-domain id outside the reserved omarchy.* namespace, for example io.github.you.github-activity.
- 
Exactly one manifest.json, at the repository root. A manifest in a subfolder makes omarchy plugin add refuse the repo.
- 
No symlinks inside the plugin folder; validate rejects them, which affects dotfile-repo workflows.
- 
README with install and removal steps, a license file, and documented external dependencies.
- 
plugin add does not run install scripts, so a widget that needs a helper binary must say so and degrade cleanly without it.
Settings should use the manifest's own settings-form support where it covers the need, so users edit them in the shell's UI and values land in shell.json.
Desktop widgets add a few fields to the sidecar: the surfaces a widget supports (bar, desktop, panel), its sizeClasses with minimum and maximum sizes and an optional aspect lock, and any interactive regions that need input. A widget never stores or declares a secret; it names the provider it needs and the data layer supplies fields.

## Theming and 100% customization
Color adaptation costs nothing if widgets never choose colors themselves: the shell already regenerates its theme tokens on every omarchy theme set and watches a machine-level override file, so a widget that reads those tokens restyles live.
Customization works in four layers, from least to most effort for the user:
- 
Theme tokens (automatic). Widgets read semantic roles from the shell's theme singletons, never hex values. A lint rule fails any widget with color literals.
- 
Settings (schema-driven). Each widget declares typed options (interval, units, accounts, which fields to show). Users edit them in the shell's settings UI; values persist in shell.json.
- 
Variants (layouts and skins). One data model, several designs: compact text, ring gauge, sparkline, card grid. Variants differ in layout, density and shape, never in hard-coded color. Users switch per instance, and the same widget can appear twice with different variants.
- 
Eject (full control). ow eject <id> wraps the existing clone command: it copies the widget into the user's plugin directory under their own id, keeps its bar position and settings, and reloads on every save. The clone stops receiving upstream updates, so the manager shows an "ejected" badge and offers a diff against upstream.
Design rules every widget follows:
- 
Sizes scale with the shell's text-size setting (the 9 to 20 px knob), so no fixed pixel fonts.
- 
Honor the user's font, spacing and bar-height overrides from the machine-level shell.toml.
- 
Render a useful empty, loading and error state; a failed provider must never blank the bar.
- 
Ship a preview screenshot rendered against at least three themes (one light, one dark, one high-contrast) as part of CI.

## Data providers
One small background daemon owns polling, caching and credentials, and widgets subscribe to it. This keeps tokens out of QML, keeps a crashing integration from taking down the shell (a QML error can crash the whole desktop), and stops ten widgets from hitting the same API.
Provider
Source
Auth
Notes
system
/proc, /sys, GPU tools
none
omarchy.monitor already exists; reuse before duplicating.
battery, media
UPower, MPRIS (Quickshell services)
none
First-party widgets exist; we add variants only.
docker
Docker Engine API on the local socket
socket access
A community Docker widget exists; adopt or partner.
github
REST and GraphQL
fine-grained token or gh token
Notifications, PRs, issues, contribution graph.
gmail
IMAP
app password, or the user's own OAuth client
Do not ship a hosted OAuth app: Gmail read scopes are restricted and distributed apps typically need a security assessment (from memory; verify against Google's current policy).
calendar, tasks
ICS and CalDAV, local task file
none or basic
Local-first; hosted task apps later.
ai-usage
local session logs of installed agent harnesses
none
See below.
AI usage with auto-detection. A detector scans for installed agent harnesses and loads one adapter per harness. As far as I know, Claude Code and Codex keep local JSONL session logs that community tools already parse; each adapter is small, versioned and tested against fixture logs, because these formats change without notice. First-party omarchy.agents and third-party agent widgets already exist, so start by checking what data they expose and extend or upstream rather than fork.
Transport. The daemon listens on a Unix socket in $XDG_RUNTIME_DIR (mode 0600) and speaks line-delimited JSON with subscribe and snapshot calls. One service-kind plugin bridges the socket into a QML singleton that widgets import from the kit.
Security rules:
- 
Secrets live in the system keyring and only the daemon reads them; widgets get data, never tokens.
- 
A widget can query a provider only if its widget.json declares it, and the user approves that grant in the manager.
- 
No telemetry, no cloud relay; all traffic goes from the user's machine to the service they connected.
- 
Stale data shows its age instead of disappearing when a provider fails.
Implementation fits Rust (single static binary, easy to package for pacman) with the QML bridge kept thin.

## Secure data layer: threat model and design
The data layer protects what can be protected on a desktop where plugins run as the user, and says plainly what it cannot. Encryption at rest defends against theft and leaked copies; it does not defend against code that runs as the user after login, and the product must say so in user-facing words.
Threat
Protection
Residual risk
Secrets in plain text in configs, dotfile repos and backups (the existing host keeps a calendar's private URL and crypto addresses in widgets.json)
Secrets live only in the vault; configs hold references such as secret:github-main.
None if the reference rule holds.
Stolen laptop or disk
Omarchy's installer enables LUKS by default (it can be skipped), and the vault encrypts on its own, so a no-LUKS install does not expose tokens.
Anything in memory after login.
Copies of the home directory: snapshots, rsync, cloud backup
The vault file is AEAD-encrypted with a key never stored beside it.
The key's own protection (see modes).
Other local users and other sessions
Socket in $XDG_RUNTIME_DIR with mode 0600, peer UID checked with SO_PEERCRED.
Other processes of the same user.
Over-broad tokens
Least-scope tokens, shown to the user with the widgets that use them.
User choice.
Leaks through logs, argv, environment, crash dumps and previews
Secrets never in argv or env, log redaction, core dumps off, PR_SET_DUMPABLE=0, locked and zeroized key memory, masked sensitive fields in previews.
Screen capture by the user.
A spec widget or a compromised data source
Spec widgets run no code and see only fields granted to them.
Data the user granted.
A malicious or buggy code widget
Cannot be prevented: it runs unsandboxed as the user and can read anything the user can, including an unlocked keyring. Reduced by tiers, review, declared capabilities, lint and spec-first design.
Real. State it in the UI and the docs.
Supply chain for the daemon and its dependencies
Locked dependencies, cargo-deny and cargo-audit in CI, reproducible builds, signed releases and an SBOM.
Upstream compromise.
Vault protection modes, chosen at ow vault init and changeable later:
Mode
Key protected by
Strength
Weakness
Keyring (default)
The desktop Secret Service (gnome-keyring or compatible) via D-Bus
No prompts; widely supported.
Omarchy logs in automatically after disk unlock, and a user report on an older release found the keyring effectively unprotected as a result; any app can read it once unlocked. Verify on Omarchy 4.
Passphrase
Argon2id-derived key from a passphrase typed once per session; the unlocked key is held in the kernel keyring with a timeout
Strongest software-only option; independent of the keyring.
A prompt each login.
TPM-bound
A key sealed to the machine's TPM2 through systemd-creds, where present
No prompt; bound to the device.
Needs a TPM; evaluate in a spike.
Secrets are stored as individual XChaCha20-Poly1305 records under a data key wrapped by the master key, in a versioned file with 0600 permissions, atomic writes and key rotation. Candidate Rust crates are oo7 or keyring for Secret Service, age or chacha20poly1305 with argon2, and zeroize and secrecy for memory handling; check maintenance status before committing.
Cache policy. Sensitive provider data (mail subjects, calendar events, notifications, agent usage) stays in memory by default. Persisting it for offline display is opt-in and encrypted under the vault's data key, with a time limit. Public, low-sensitivity data such as weather may be cached in plain files. Removing a token purges everything derived from it, and ow vault wipe removes it all.
Provider authentication and scope
- 
GitHub: the device flow (no client secret to ship) or a fine-grained token with the least scopes.
- 
Google Mail and Calendar: IMAP with an app password, or OAuth with the user's own client and a loopback PKCE flow; never a shared hosted client. Secret iCal URLs are stored as secrets, not as settings.
- 
Docker: socket access is close to root-equivalent, so the daemon exposes only an allow-listed, read-only subset of the API, and start, stop or exec actions need a separate explicit opt-in per widget.
- 
AI usage: parse local session logs for counts and timings only, never forward or render prompt content, and enable each harness individually.
- 
Network egress: a per-provider hostname allow-list inside the daemon, system TLS roots only, and no redirects to other hosts.
Protocol, grants and audit. The bridge plugin enforces per-widget grants that the user approves in the Widget Center, and the daemon filters by grant as well. For spec widgets this is a hard boundary, because only the renderer talks to the daemon. For code widgets it is a guardrail for honest code, not a barrier, for the reason in the threat table. Every provider access is written to a local, redacted audit log the user can read in the center, with rate limits per widget.
Process hardening. The daemon runs as a systemd user service with NoNewPrivileges, ProtectSystem=strict, a restricted ProtectHome with one writable state directory, PrivateTmp, restricted address families, an empty capability set and a system-call filter. Check the result with systemd-analyze security, and write it in Rust with unsafe forbidden outside audited modules.
Assurance before 1.0: a published threat model, fuzzing of the socket and spec parsers, CI audit gates, an independent review of the vault and protocol, a security policy with private disclosure, and signed releases. Treat the vault as the one component that must be boring, small and reviewed.

## Manager
The manager does what omarchy plugin and Plugin Depot do not: per-widget settings and variants, provider grants, packs, health checks and rollback. It has two faces over one engine: an ow command-line tool and a panel plugin inside the shell.
Capability
Behavior
Built on
Browse and filter
Catalog by category, tier, compatibility and required providers; previews on the user's own theme.
Registry index.json plus the official marketplace feed.
Install
Delegated to Plugin Depot or the official CLI. Our own path exists only for monorepo widgets: fetch one folder at a pinned commit, validate, copy in (no symlinks).
omarchy plugin add for standalone repos.
Place
Plugin Depot already has a drag-and-drop bar layout editor; reuse it and add only what it lacks (per-monitor layouts later).
omarchy plugin enable, omarchy bar move, shell.json.
Configure
Per-instance settings, variant picker and provider grants in one screen.
Manifest settings form plus widget.json.
Packs
Export or import a named layout (widgets, order, settings, variants) as one file; shareable by URL or repo.
A slice of shell.json plus a lockfile of pinned widgets.
Snapshots
Automatic snapshot before any change; one-click rollback.
Copy of shell.json and plugin directory state.
Update
Upstream already previews incoming commits; we add a diff of newly requested capabilities from the sidecar file.
Pinned SHAs and widget.json capability diff.
Doctor
Validate manifests, flag missing providers, broken tokens, unsafe patterns and version drift.
omarchy plugin validate, kit lint.
Rules for the engine:
- 
Prefer the official CLI for every change it supports; edit shell.json directly only for what it cannot express, using atomic temp-file-and-rename writes, because the shell treats that file as canonical and also writes it.
- 
Be idempotent and scriptable: every action works as ow <verb> --json --yes so agents and dotfile managers can drive it.
- 
Never block the shell: long work runs in the CLI or daemon, not in QML. A panel is destroyed whenever plugins rescan, so mutations must run as detached jobs that outlive the panel and re-summon it when done; Plugin Depot's job runner is a working example.
CLI surface (first cut): ow new, ow check, ow install, ow enable, ow place, ow pack export|import, ow snapshot, ow rollback, ow eject, ow doctor, ow search.
Build the CLI first. The panel is a thin QML client of the same engine, written after the CLI proves the model. Consider contributing packs and snapshots to Plugin Depot (MIT licensed, takes pull requests) instead of shipping a second panel.

## Maker
The maker is the main differentiator and the main contributor on-ramp. It has four levels, each usable on its own, so people can start with no QML and graduate when they want more control.
Level
Who it is for
How it works
Output
0. Scaffolder
Developers
ow new asks for a template (text item, item plus popup, chart, list, service) and writes manifest, QML, README, license and preview, then runs validate. Files reload on save; force a reload with rescanPlugins.
Code widget
1. Spec widgets
Tinkerers, no QML
A JSON spec binds provider fields to blocks (value, label, ring, bar, sparkline, list, icon) and picks a variant. A generic renderer plugin draws it with theme tokens.
Spec widget (no code)
2. Visual designer
Anyone
A shell panel with live preview across themes: pick a provider field, drag blocks, tune density and variant, export the spec.
Spec widget
3. Agent-assisted
Agent users
A skill and llms.txt teach an agent the kit, spec and ow check loop, so "a widget showing my Docker containers with a ring gauge" produces a validated plugin.
Either
Why spec widgets matter. They run no code of their own, so they can be installed from strangers with far less risk, they can be shared as a single JSON file or URL, and they survive kit upgrades because the renderer owns the QML. Code widgets stay available for anything the spec cannot express.
Kit (shared QML library). Theme-aware components (cards, rings, bars, sparklines, list rows), the provider client, the settings helpers, formatting for time, bytes and units, and lint rules. Widgets that use the kit stay small and look consistent. The kit has its own semantic version, and compat.widgetsKit in widget.json pins the major.
Scope warning. The spec renderer is a small framework in its own right. Ship levels 0 and 3 first, since they need only the kit and docs, and start levels 1 and 2 after the kit has settled across the v0.1 set.

## Registry, trust and contribution
The registry is a static index.json built by CI from listing files in a Git repository and served from GitHub Pages. There are no servers, accounts or databases to run, which suits an open-source project with a small team. The same index feeds the manager, a browsable website and the official marketplace submission helper.
Categories: System, Time and Calendar, Productivity, Dev and Git, AI and Agents, Media, Communication, Network and Containers, Ambient and Fun, Utilities.
Trust tiers (shown in the manager before install):
Tier
Who maintains it
What we check
Install behavior
Core
This project
Reviewed, tested on every Omarchy release channel, signed tags.
Default catalog.
Verified
Community, reviewed by us
A maintainer read the pinned commit; declared capabilities match the code; builds and previews pass CI.
Pinned to the reviewed SHA; updates need re-review.
Spec
Anyone
Spec validates against the schema; renders no custom code.
Installs without a code warning.
Community
Anyone
Passes omarchy plugin validate and lint only.
Shows a clear "unreviewed code" warning.
The official marketplace already approves an exact snapshot of each listing, and Plugin Depot installs that reviewed commit. Our Verified tier therefore adds one thing on top: a check that the code's behavior matches the capabilities the widget declares.
Lint (suspicious process spawns, network hosts not in widget.json, file access outside declared paths) is advisory. QML here is unsandboxed, so the tiers describe review, not isolation, and the UI must not imply otherwise.
Contributor flow:
- 
Fork the repo and run ow new (or start from the agent skill).
- 
Develop with hot reload against a throwaway shell config.
- 
Run ow check: validate, kit lint, capability cross-check, screenshots on three themes.
- 
Open a PR that adds a listing file pointing at the widget repo and a commit SHA, or, for a monorepo widget, adds the widget folder.
- 
CI repeats the checks and posts the previews on the PR.
- 
A maintainer reviews; merge publishes it in the next index build, and a mirror repo is created if the widget needs standalone plugin add.
Governance and community:
- 
MIT license, CODEOWNERS per widget, a short maintainer team, and a code of conduct.
- 
An RFC process for changes to the kit, the spec and the sidecar format; semantic versioning with a deprecation window.
- 
A security policy with a private disclosure address, and a fast takedown path for listings.
- 
Contribution ladder that is not only code: screenshots and variants, translations, provider adapters (for example a new AI harness), docs, and triage.
- 
Labeled good-first-widget issues, and a showcase page of user packs.

## Website and marketplace
The website shows what the product is, lets people browse and install widgets, and teaches contributors. It stays a static site with no accounts, so there is nothing to breach and little to operate.
Page
Purpose
Data source
Home
A hero demo that switches the same desktop between several Omarchy themes, plus the install command.
Pre-rendered previews.
Gallery
Widgets by category with search and filters: trust tier, required providers, size classes, desktop or bar.
index.json built by CI.
Widget page
Preview with a theme switcher and size slider, variants, requested capabilities, changelog, copy-to-clipboard install command, source link, contributors.
Listing file, the widget's repo, CI renders.
Packs
Whole layouts with screenshots and an import command.
Pack files in the registry repo.
Playground
A browser designer for spec widgets that exports the spec, with a live preview.
Web renderer.
Docs
Getting started, building at three levels, spec and provider references, contributing.
The docs folder in the monorepo.
Security
Threat model, trust tiers, how to report a problem.
The same source as the repo's policy.
Data pipeline. A GitHub Actions job merges our registry listings with the official marketplace's public catalog feed (credited, linked out, cached politely, and subject to its terms), validates every listing, builds index.json and the static pages, and deploys to static hosting such as GitHub Pages or Cloudflare Pages.
Previews. CI renders each widget from its real QML in every size class and in several themes, using Quickshell on a headless or nested compositor, and publishes PNGs or short videos. The theme switcher swaps those pre-rendered assets, so the site needs no QML runtime. Whether headless rendering works reliably is one of the Phase 0 spikes.
Spec widgets render in the browser too. The spec is portable, so a TypeScript renderer can draw it live on the site and in the Playground, and a golden-screenshot test compares it with the QML renderer to catch drift. Two renderers cost real maintenance, which is why this follows a stable spec schema.
Privacy and trust. No accounts or cookies; at most anonymous aggregate counts like the community marketplace keeps. Reports go to GitHub issues, with a private address for security reports. The footer states that the site is independent of Omarchy and 37signals unless that changes.

## Launch catalog and overlap
About half of your launch list duplicates first-party widgets, so the first release should lead with what Omarchy lacks and ship the rest as variants. This comparison uses the first-party list on the quattro branch; recheck the live catalog before committing to any row.
Widget
Provider
Already exists
Plan
GitHub: notifications, PRs, CI status, contribution graph
github
None seen
New, launch.
Gmail: unread count and inbox list
gmail (IMAP)
None seen
New, launch.
Calendar and agenda
calendar
None seen
New, launch.
Task manager
tasks
None seen
New, launch.
Docker and Compose
docker
Community Docker monitor
Partner or adopt, add variants.
AI usage across harnesses
ai-usage
omarchy.agents and third-party agent widgets
Add adapters and history views; coordinate with the owners.
Clocks: digital, analog, binary, world, countdown
none
omarchy.clock
Variants; world and countdown are new.
System graphs and usage: CPU, memory, disk, network, GPU, temperatures
system
omarchy.monitor
Variants with history graphs.
Music player
MPRIS
omarchy.media
Variants: cover art, mini, visualizer.
Battery
UPower
omarchy.battery, omarchy.power
Variants only.
Reminders
local
omarchy.reminders
Skip; integrate with Tasks.
Weather
web
omarchy.weather
Skip.
Desktop widgets on the wallpaper layer
various
Hosts exist (anishfn/omarchy-widgets, Liquid Widgets, others)
Collaborate first; add free-form placement.
Proposed v0.1 set: GitHub, Gmail, Calendar, Tasks, a polished Docker widget, the AI-usage extension, and variant packs for Clock, Monitor, Media and Battery. That is six widgets and four variant packs, enough to prove the provider daemon, the kit and the manager without chasing parity with everything.

## Build plan: repository, stack and first sprint
One monorepo holds everything so the spec, the daemon, the host and the site change together. Names below are placeholders until the name decision is made.
Area
Technology
Reason
Daemon, CLI, core library
Rust
Memory safety for the vault, one static binary.
Host, bridge, kit, widgets
QML on Quickshell
Required by the Omarchy plugin system.
Site, playground, web renderer
TypeScript with a static-site generator
Cheap hosting, shares the spec schema.
Spec
JSON Schema
One contract validated by Rust, QML and TypeScript.
CI
GitHub Actions: qmllint, cargo audit, cargo deny, fuzzing, headless preview renders
Gates every change.
Distribution
AUR or pacman package for the daemon and CLI; omarchy plugin add for the shell parts and widgets
plugin add runs no install scripts, so binaries cannot ride along.
Testing strategy: unit tests for the spec, grid and snapping math; golden screenshots for widgets and for the two renderers; fuzzing of the socket and spec parsers; and integration tests in a disposable Omarchy 4 virtual machine that installs, enables, restarts the shell and checks the result.
Phase 0 sprint (about two weeks):
- 
Open a discussion with the author of anishfn/omarchy-widgets proposing a free-form canvas, an external package loader and a provider API.
- 
Stand up an Omarchy 4 test machine or virtual machine with a snapshot to roll back to.
- 
Spike: a service plugin draws a Bottom-layer surface that is click-through except for one interactive rectangle.
- 
Spike: free-form drag and resize on an Overlay surface that writes anchors and sizes to JSON.
- 
Spike: bridge-to-daemon over a Unix socket with a peer UID check.
- 
Spike: vault round trip in keyring, passphrase and TPM modes; record how the keyring behaves under autologin on Omarchy 4.
- 
Spike: does the validator accept extra manifest keys, and can one plugin depend on another.
- 
Spike: render a widget screenshot headlessly in CI.
- 
Draft the threat model and spec schema v0.
- 
Settle the name and ask the Omarchy maintainers about using it.

## Roadmap
Each phase ends in something a stranger can install, so progress is visible and any phase can become a stopping point. Durations are rough guesses and should be rebased after Phase 0, whose answers (especially the host decision and how the keyring behaves under autologin) can reshape Phases 1 and 3. Keep the Verified tier small until the maintainer team can review what it lists.

## Risks and mitigations
Risk
Likelihood and impact
Mitigation
Upstream contract changes (open proposals include manifest shortcuts and panel extension points)
High likelihood, medium impact
Pin to schemaVersion: 1, isolate Omarchy calls in one adapter module, run CI against the stable, RC and edge channels.
Duplicating upstream or Plugin Depot and splitting the community
Medium, high
Open a discussion upstream before building; reuse their install path; lead with gaps, not parity.
A malicious or buggy widget (plugins are unsandboxed)
Medium, high
Spec widgets, pinned installs, capability diff on update, advisory lint, honest tier labels, takedown path.
One QML error takes the shell down
Medium, high
Keep logic in the daemon, test with qmllint and a headless shell in CI, guard every provider read, fail to an error state.
Provider formats drift (agent logs, APIs)
High, medium
One adapter per source, fixture-based tests, versioned adapters, "unsupported version" state instead of wrong numbers.
Gmail and OAuth policy
Medium, medium
IMAP with app passwords, or bring-your-own OAuth client; never a shared hosted client.
Maintainer overload as listings grow
Medium, high
Static registry, CODEOWNERS, automated checks, clear tier definitions, cap the Verified tier to what you can review.
Naming and trademark friction
Low, medium
Ask early; keep a fallback name.
Scope creep: maker, manager, daemon, kit and 20 widgets at once
High, high
Ship the roadmap's phases in order; each ends in something installable.
Added by the build plan:
- 
Host collision. If the existing host's author is unresponsive or disagrees on direction, the fallback is a fork, which costs maintenance and splits users. Decide the fallback in Phase 0.
- 
Overstated security. Users may assume the vault protects them from a malicious widget. Say in the UI, docs and tier labels that it does not.
- 
Keyring behavior. If the default keyring is effectively unprotected under autologin, the default mode is weak; the passphrase and TPM modes exist for that reason.
- 
Docker socket exposure. Treat any write access as root-equivalent: read-only by default, per-widget opt-in for actions.
- 
Two renderers drifting apart. Golden screenshots in CI, and delay the web renderer until the spec schema is stable.

## Open questions to settle with spikes
I could not verify these from documentation. Each is a short experiment on a real Omarchy 4 machine, and the answers change the design above, so they form Phase 0 of the roadmap.
- 
Does omarchy plugin validate accept extra top-level manifest keys? The documented checks (schema version, required fields, reserved id, safe entry paths, no symlinks) do not mention it. If yes, widget.json can fold into manifest.json.
- 
Can one plugin depend on or import another? This decides whether widgets use a shared service singleton or each open the provider socket through the kit.
- 
Can a panel or service plugin create a window on the Wayland background layer without stealing focus or affecting lock-screen behavior? Existing plugins already draw on the Bottom layer, so the open part is free-form drag and resize with input limited to each widget's own rectangle.
- 
Which theme singletons and shared components may third-party QML import from the shell, and are they stable? (Sources disagree on whether colors.toml has 22 or 24 colors; read the shipped schema.)
- 
How far does the manifest's settings form go: field types, validation, per-instance values?
- 
What data do omarchy.monitor and omarchy.agents already expose, and can we read it instead of re-collecting it?
- 
Does omarchy plugin add accept a commit SHA or a subdirectory? The official manual says it clones the repo and moves it into place, and Plugin Depot gets pinned installs by fetching one commit itself, so plain add appears to track HEAD.
- 
Can the shell run headless (a nested compositor in CI) to render preview screenshots?
- 
What does the live marketplace actually contain by category, and where are the real gaps?
- 
Is the name acceptable to the Omarchy maintainers?

## Sources
Checked on 17 October 2026. Omarchy 4 is moving quickly, so recheck contract details before building.
Opened in full:
- 
Omarchy manual: Shell Plugins: kinds, CLI, enable rules, validation, clone.
- 
Plugin Depot README: pinned installs, bar layout editor, panel lifecycle constraints.
- 
Publish a plugin: manifest fields and submission steps.
- 
daily.dev summary of "Omarchy Quattro: One Year In, Part 3": the 3,000-plugin claim (secondhand; the original post was not opened).
Read as search excerpts only:
- 
Omarchy Quattro release notes and PR 6231
- 
omarchy-shell reference and first-party plugin list
- 
Official marketplace submission guide and community marketplace
- 
Develop a plugin
- 
Upstream proposals: manifest shortcuts, discussion 9631 and panel extension points, discussion 7033
- 
Existing widgets: Docker monitor, Shibumi Shell, OmaSafe, OpenClaw on Omarchy
- 
Omarchy theming architecture (third-party write-up) and a manifest-at-root install failure
- 
Hyprland 0.55 announcement
Added for the build plan. Opened in full: anishfn/omarchy-widgets README. Read as search excerpts only:
- 
Quickshell WlrLayershell source and docs
- 
Other desktop-widget plugins: Liquid Widgets, Information Wallpaper Overlay, Desktop Icons and the plugins-as-cards fork
- 
Keybinding pattern: OmaIpsum, Fathom and OmaCoach
- 
Disk encryption and autologin: Omarchy manual installation page; keyring behavior: Omarchy discussion 3331 (a user report on an older release) and the Arch Wiki on GNOME Keyring