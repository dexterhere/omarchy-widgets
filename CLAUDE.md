# CLAUDE.md: Omarchy widget layer (working name: Widget Hub)

You are helping build an open-source widget manager, maker, secure data layer and marketplace for **Omarchy 4+** (the Arch/Hyprland distro by DHH, which runs one Quickshell shell where everything is a plugin). The owner is Dexter (GitHub: dexterhere), an experienced Rust/Go/TypeScript backend developer who runs Omarchy daily. Treat him as the decision-maker on naming, publishing and contacting other maintainers.

Read these before any non-trivial work, in this order:

1. `docs/PROJECT_BRIEF.md`: vision, decisions, architecture, package spec, UX, trust model.
2. `docs/DAEMON_SPEC.md`: the provider daemon (`owd`), the first thing we build.
3. `docs/BUILD_PLAN.md`: phases, Phase 0 spikes, acceptance criteria, working agreements.
4. `design/`: the approved visual demo (desktop canvas, Widget Center, marketplace) and its theme palettes.
5. `docs/PLAN_v0.2_export.md`: the original research plan with its sources list (reference; the files above are the maintained versions).

## What we are building (one paragraph)

A layer on top of Omarchy's existing plugin system, not a replacement. Parts: (1) `owd`, a Rust provider daemon that owns credentials, polling and caching and is the only thing that talks to the network; (2) a thin QML bridge plugin plus a QML kit, so widgets get data and never see tokens; (3) a desktop canvas with free-form place, resize and remove, opened from a Widget Center overlay by a shortcut (or optional bar icon); (4) `ow`, a Rust CLI manager and maker; (5) a widget package format (standard Omarchy plugin plus a `widget.json` sidecar), including no-code **spec widgets**; (6) a static website and registry for the marketplace. User data handling is the core priority.

## Hard rules (do not break these)

**Security**
- Secrets never appear in QML, argv, environment variables, logs, config files, crash dumps or previews. Configs hold references like `secret:github-main`, never values.
- Sensitive provider data (mail, calendar, notifications, agent usage) stays in memory by default. Persisting it is opt-in, encrypted, time-limited.
- The daemon denies by default: a widget gets only topics it has been granted, and a provider may call only its allow-listed hosts.
- `#![forbid(unsafe_code)]` in every crate unless a module is explicitly audited and documented. Prefer safe crates (`rustix`, `nix`) for `prctl`/`mlock`.
- Never claim more than we can deliver. Unsandboxed QML code widgets can read anything the user can. Docs, UI text and code comments must say so plainly. Do not write "sandboxed", "secure widget" or "safe" about code widgets.
- Never invent cryptography. Use vetted crates (`chacha20poly1305`, `argon2`, `zeroize`, `secrecy`). Check maintenance status before adding any crypto or keyring dependency, and record the decision in `docs/DECISIONS.md`.

**Safety of the developer's machine**
- All development and tests run against throwaway XDG directories (`XDG_CONFIG_HOME`, `XDG_DATA_HOME`, `XDG_STATE_HOME`, `XDG_RUNTIME_DIR` pointed at a temp dir). Never read or write the real `~/.config/omarchy`, `~/.config/hypr` or the real keyring unless Dexter explicitly says so in the current conversation.
- Never edit `~/.config/hypr/bindings.lua` or `shell.json` on the real machine without a backup, a diff shown first and explicit approval.
- Do not run `omarchy plugin add` against third-party repos without telling Dexter first; it clones unreviewed code that runs unsandboxed.

**Scope and honesty**
- Do not build a new shell, a new plugin format, or a competing marketplace front end. Reuse `omarchy plugin`, Plugin Depot, and the official marketplace feed.
- Omarchy 4 is moving fast. Every Omarchy or Quickshell contract detail in the docs is "believed, verify in Phase 0". When you verify something, update the docs and `docs/DECISIONS.md` in the same change.
- If a fact is uncertain, say so in the code comment or doc. Do not guess silently.

**Human-only tasks (never do these yourself; remind Dexter)**
- Contacting `anishfn/omarchy-widgets` maintainers, the Omarchy maintainers, or anyone else.
- Choosing the final project name, buying domains, creating GitHub orgs or repos, publishing releases or packages, posting anywhere public.
- Anything that spends money or uses his accounts or tokens.

## Stack and layout

- Rust (daemon, CLI, core libs), QML on Quickshell (host, bridge, kit, widgets), TypeScript + static site generator (website, playground, web renderer), JSON Schema as the single contract across all three.
- Monorepo. Intended layout is in `docs/BUILD_PLAN.md` (section "Repository layout").
- CI: `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test`, `cargo deny check`, `cargo audit`, `qmllint` for QML, schema validation, fuzz targets built (and run on schedule).

## Commands (fill in as they come to exist)

```
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo deny check && cargo audit
cargo run -p owd -- --config-dir "$TMPDIR/owd-dev"     # always a throwaway dir
qmllint path/to/*.qml
```

## How to work

- Small commits, one concern each. Tests first for the vault, the protocol parser and grant checks.
- Every new behavior gets a test; every security rule above gets a test that would fail if broken (for example: grep the log output of a full run for the fake secret and assert it is absent).
- After each work session, update `docs/BUILD_PLAN.md` progress checkboxes and append to `docs/DECISIONS.md` (date, decision, reason, what would change it).
- When blocked on a human decision or an unverified Omarchy fact, stop, write the question into `docs/OPEN_QUESTIONS.md`, and ask Dexter. Do not build on a guess for anything that is expensive to redo.
- Keep user-facing text plain and honest. No emoji. Follow the visual rules in `design/README.md`: widgets read theme tokens, never hex colors.

## Current phase

Phase 0 (spikes and decisions). See `docs/BUILD_PLAN.md`. Update this line when the phase changes.
