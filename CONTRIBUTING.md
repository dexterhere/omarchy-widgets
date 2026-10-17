# Contributing

## Branch model

| Branch | Purpose | Merges in from |
|---|---|---|
| `main` | Production. Tagged releases only. | `staging` (release PR), `hotfix/*` |
| `staging` | Review and soak before release. | `develop` (promotion PR) |
| `develop` | Integration. Default target for work. | `feat/*`, `fix/*`, `chore/*`, `docs/*` |

Flow: `feat/x` -> PR to `develop` -> PR `develop` to `staging` -> PR `staging` to `main`. Hotfix: `hotfix/x` from `main` -> PR to `main`, then back-merge to `develop`.

No direct pushes to `main`, `staging` or `develop`. Branches are protected and need a green CI and a review.

## Pull requests

- One concern per PR, small enough to review in one sitting (aim under about 400 changed lines).
- Conventional commit messages (`feat:`, `fix:`, `chore:`, `docs:`, `ci:`, `test:`). Small commits.
- Tests with the code. Security rules in `CLAUDE.md` need a test that fails if broken.
- Run before pushing: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`.
- Changes to the vault, wire protocol, grants or crypto need a second reviewer who did not write them.

## Development safety

Run everything against throwaway XDG directories. Never touch a real `~/.config/omarchy` or keyring. Never commit secrets, tokens or real user data.
