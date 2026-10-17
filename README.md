<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="assets/logo-dark.svg">
    <img src="assets/logo-light.svg" alt="Omarchy Widgets" width="520">
  </picture>
</p>

# Omarchy Widgets

**Desktop widgets for [Omarchy](https://omarchy.org) that handle your data safely.**

Install a widget, drop it anywhere on your desktop, resize it, and it follows your theme. Your tokens and passwords never touch the widget: a small background service holds them in an encrypted vault and hands each widget only what you allowed.

> **Status: early development.** The secure data layer (daemon, vault, CLI) works and is tested. The desktop canvas, the Widget Center and the widgets themselves are not built yet, so there is nothing to install for end users today. See [Roadmap](#roadmap). The name is a placeholder.

## Why

Omarchy 4 already has a plugin system, a bar and a few desktop widgets. What is missing, and what this project adds on top of it (not instead of it):

- **Free-form placement.** Put any widget anywhere, resize it, snap it, undo it. Layouts survive restarts and monitor changes.
- **A safe place for credentials.** GitHub, Gmail, calendar and similar widgets need secrets. Today those sit in plain-text config files. Here they live in an encrypted vault and widgets only ever receive *data*.
- **One package format.** A widget is a normal Omarchy plugin plus a small `widget.json`, so it installs with the tools you already use.
- **Make your own.** Scaffold a widget, write a no-code JSON "spec widget", or ask an AI agent using our docs.
- **A marketplace page** with previews in every theme, listing exactly what each widget can access.

## How it works

```
widget (QML or JSON spec)  ->  bridge plugin  ->  owd daemon  ->  encrypted vault
        gets data only            Unix socket        checks grants,     holds the secrets
                                                     audits, polls APIs
```

- **`owd`** is the daemon. It owns credentials, polls data sources, caches in memory, and answers widgets over a private local socket. A widget gets a topic and field only if you granted it. Every access is recorded in a local audit log (no values, ever).
- **`ow`** is the command line tool: manage the vault, and later install, place and snapshot widgets.
- **Widgets** never see tokens, never talk to the network themselves.

## What works today

| Part | State |
|---|---|
| Daemon `owd` | Private socket (same user only), deny-by-default grants, rate limits, redacted audit log, live subscriptions with push updates, stale-data handling, `system` provider (CPU, memory, load) |
| Vault | Passphrase mode: Argon2id + XChaCha20-Poly1305, authenticated, atomic writes, rotation. Format in [docs/VAULT_FORMAT.md](docs/VAULT_FORMAT.md) |
| CLI `ow` | `ow status`, `ow vault init / status / list / set / remove / rotate / wipe`. Secrets come from a no-echo prompt or stdin, never command-line arguments |
| Packaging | `PKGBUILD` and a hardened systemd user unit ([packaging/](packaging/)) |
| Quality | Unit, integration and process-level tests, five fuzz targets, `cargo audit` and `cargo deny` on every change, independent security review of every security-relevant PR |

## Roadmap

1. **Phase 0 (now, in parallel):** verify how Omarchy 4 and Quickshell behave (surfaces, plugin contract, keyring), decide the host strategy and the final name.
2. **Phase 1:** QML bridge and kit, desktop canvas with edit mode, `ow` manager commands, Widget Center, first widgets (clocks, system, battery), packaging.
3. **Phase 2 (public beta):** GitHub, Gmail, calendar, tasks, Docker and AI-usage widgets, the Discover tab. Gated on an external review of the vault and protocol.
4. **Phase 3 and 4:** maker tools (scaffolder, spec widgets, visual designer), registry, website, contribution ladder.

Details: [docs/BUILD_PLAN.md](docs/BUILD_PLAN.md), vision: [docs/PROJECT_BRIEF.md](docs/PROJECT_BRIEF.md), daemon design: [docs/DAEMON_SPEC.md](docs/DAEMON_SPEC.md).

## Security, in plain words

User data comes first. The full picture is in the [threat model](docs/threat-model.md); the short version:

- Secrets exist only in the encrypted vault and in process memory. Configs contain references like `secret:github-main`, never values.
- The daemon refuses everything by default; each widget gets only the topics and fields you grant, checked again on every update.
- Logs, audit files, command lines and environment never contain secrets. This is tested by running the real programs and searching what they wrote.
- **Code widgets (QML) are not sandboxed.** They run as you and can read anything you can, so install them like programs. Spec widgets (JSON, no code) get a hard boundary. We never describe code widgets as "safe" or "secure".
- A malicious program already running as your user can still reach an unlocked vault. The vault protects copies of your files, backups and logs, not a compromised session.

Found a vulnerability? **Do not open a public issue.** Follow [SECURITY.md](SECURITY.md).

## Try the developer build

Requires a recent stable Rust toolchain.

```bash
git clone https://github.com/dexterhere/omarchy-widgets.git && cd omarchy-widgets
cargo test --workspace

# Run the daemon and CLI against a throwaway directory (never your real config).
# Keep the path short and absolute: Unix socket paths are limited to 108 bytes.
D=$(mktemp -d -p /tmp owd.XXXX)
cargo run -p owd -- --config-dir "$D" &
cargo run -p ow  -- --config-dir "$D" status
cargo run -p ow  -- --config-dir "$D" vault init
```

## Contributing

Everyone is welcome, and not only with code: bug reports, docs, screenshots, theme testing, reviewing and ideas all count. Start with [CONTRIBUTING.md](CONTRIBUTING.md), which explains the branch model, how to set up, how to open a good pull request and how security-sensitive changes are handled. Please read our [Code of Conduct](CODE_OF_CONDUCT.md).

## Project documents

| Document | What it is |
|---|---|
| [LICENSE](LICENSE) | MIT |
| [SECURITY.md](SECURITY.md) | How to report a vulnerability, scope, our promises |
| [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md) | How we treat each other |
| [CONTRIBUTING.md](CONTRIBUTING.md) | How to contribute |
| [docs/threat-model.md](docs/threat-model.md) | What is and is not protected |
| [docs/DECISIONS.md](docs/DECISIONS.md) | Every design decision with its reason and evidence |
| [docs/OPEN_QUESTIONS.md](docs/OPEN_QUESTIONS.md) | What we have not verified yet |

## Independence

This project is not affiliated with or endorsed by Omarchy, 37signals or the Omarchy maintainers. It builds on Omarchy's public plugin system.

## License

[MIT](LICENSE). Contributions are accepted under the same license.
