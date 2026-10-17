# Omarchy widget layer (working name: Widget Hub)

Early development. Nothing here is installable yet.

A widget manager, maker and secure data layer for [Omarchy](https://omarchy.org) 4+. It sits on top of Omarchy's own plugin system instead of replacing it.

## What exists today

| Part | State |
|---|---|
| `owd`, the provider daemon | Socket server (private modes, same-user only), grants (deny by default), redacted audit log, rate limits, provider hub with live pushes, `system` provider (CPU, memory, load) |
| `ow`, the CLI | `ow status`, `ow vault init/status/list/set/remove/rotate/wipe` |
| Vault | Passphrase mode (Argon2id + XChaCha20-Poly1305), format in [docs/VAULT_FORMAT.md](docs/VAULT_FORMAT.md). Keyring and TPM modes are not built yet |
| QML bridge, kit, desktop canvas, Widget Center, marketplace site | Not started (Phase 0 spikes come first, see [docs/BUILD_PLAN.md](docs/BUILD_PLAN.md)) |

## Security stance

User data comes first. Read the [threat model](docs/threat-model.md) for what is and is not protected. In short:

- Secrets live only in the encrypted vault. Configs hold references like `secret:github-main`, never values.
- The daemon denies by default: a widget sees only the topics and fields it was granted.
- Spec widgets (declarative, no code) get a hard boundary. **Code widgets (QML) are not sandboxed**: they run as you and can read anything you can. We say so everywhere and never call them safe.
- Report vulnerabilities privately: see [SECURITY.md](SECURITY.md).

## Develop

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
# run the daemon against a throwaway directory (keep the path short: Unix socket limit)
D=$(mktemp -d -p /tmp owd.XXXX); cargo run -p owd -- --config-dir "$D"
```

Never point development runs at your real `~/.config` or keyring. See [CONTRIBUTING.md](CONTRIBUTING.md) for the branch model (`main` production, `staging` review, `develop` integration) and PR rules.

## Layout

`crates/` Rust workspace (`ow-proto`, `ow-core`, `ow-vault`, `ow-providers`, `owd`, `ow`), `fuzz/` fuzz targets, `docs/` design and decisions, `design/` approved visual demo (reference only), `packaging/` systemd unit.

## License

MIT, see [LICENSE](LICENSE).
