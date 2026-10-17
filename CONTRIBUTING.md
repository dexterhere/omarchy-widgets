# Contributing

Thanks for helping. You do not need to write Rust to contribute. Please read the [Code of Conduct](CODE_OF_CONDUCT.md) first.

## Ways to contribute

| You are good at | You can |
|---|---|
| Using Omarchy | Test on your setup, report bugs with your Omarchy, Quickshell and Hyprland versions, try themes |
| Writing | Improve docs, README, error messages, the threat model's wording |
| Design | Review the target look in `design/`, propose empty/loading/error states, accessibility fixes |
| Rust | Pick an item from [docs/BUILD_PLAN.md](docs/BUILD_PLAN.md) or an issue labelled `good first issue` |
| QML / Quickshell | Phase 0 spikes (see BUILD_PLAN) and, later, the kit, canvas and widgets |
| Security | Review the vault, protocol and grants, write adversarial tests, fuzz, report privately |
| Anything | Triage issues, reproduce bugs, answer questions |

Not sure where to start? Open a discussion or an issue and say what you are interested in.

## Before you start

1. Search existing issues and pull requests.
2. For anything bigger than a small fix, open an issue first so we can agree on the approach. This is mandatory for changes to the **vault, wire protocol, grants, audit log or anything cryptographic**.
3. Never put a vulnerability in a public issue or PR. See [SECURITY.md](SECURITY.md).

## Set up

Requirements: a recent stable Rust toolchain (`rustup`), and for fuzzing a nightly toolchain plus `cargo install cargo-fuzz`.

```bash
git clone https://github.com/<you>/omarchy-widgets.git && cd omarchy-widgets   # your fork
git remote add upstream https://github.com/dexterhere/omarchy-widgets.git
git switch -c feat/my-change upstream/develop
```

**Safety rule:** run everything against throwaway directories. Never point development at your real `~/.config/omarchy`, `~/.config/hypr` or your real keyring.

```bash
D=$(mktemp -d -p /tmp owd.XXXX)          # short, absolute path (socket paths max 108 bytes)
cargo run -p owd -- --config-dir "$D"
cargo run -p ow  -- --config-dir "$D" status
```

## Checks every change must pass

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny check && cargo audit          # dependency policy and advisories
# when you touch a parser (proto, grants, vault):
cd fuzz && cargo +nightly fuzz run <target> -- -max_total_time=60
```

CI runs the same checks on every pull request. A red CI cannot be merged.

## Branch model

| Branch | Purpose | Merges in from |
|---|---|---|
| `main` | Production. Tagged releases only. | `staging` (release PR), `hotfix/*` |
| `staging` | Review and soak before a release. | `develop` (promotion PR) |
| `develop` | Integration. **Open your PR against this branch.** | `feat/*`, `fix/*`, `chore/*`, `docs/*`, `test/*` |

Flow: `feat/x` -> PR to `develop` -> promotion PR `develop` to `staging` -> release PR `staging` to `main`. A hotfix branches from `main`, goes to `main`, then is merged back to `develop`. Direct pushes to these three branches are blocked.

## Pull requests

- **One concern per PR**, small enough to review in one sitting (aim for under about 400 changed lines). Split big work into a chain of PRs.
- **Commits**: small, conventional (`feat:`, `fix:`, `docs:`, `test:`, `chore:`, `ci:`), imperative subject. No co-author trailers or generated sign-offs are needed.
- **Tests come with the code.** Every new behaviour gets a test, and every security rule gets a test that fails if the rule is broken (for example: run with a fake secret and assert it appears in no log, audit file, argument list, environment or non-vault file).
- **Docs**: update the relevant document. If you decide something, append to [docs/DECISIONS.md](docs/DECISIONS.md) (date, decision, reason, evidence, what would change it). If something is unverified, add it to [docs/OPEN_QUESTIONS.md](docs/OPEN_QUESTIONS.md) instead of guessing.
- Fill in the PR template. Explain *why*, not just what.
- Be ready for review comments. Reviews are about the change, never about you.

## Security-sensitive changes

The vault, protocol, grants, audit, process hardening and anything touching secrets get extra scrutiny:

- A second reviewer who did not write the code is required.
- Expect an independent review pass and adversarial questions (races, hostile input, leaks, fail-open paths).
- Do not add a cryptography dependency without checking its maintenance and recording the decision in DECISIONS.md. Never invent cryptography.
- No `unsafe` code: every crate has `#![forbid(unsafe_code)]`.
- Error messages and logs carry categories, never values, names of secrets, paths of secrets or upstream response bodies.

## Honest wording

Code widgets (QML) are not sandboxed. Do not write "sandboxed", "secure widget" or "safe" about them anywhere in code, docs or UI text. Do not claim a protection the code does not provide; say what is verified and what is not.

## Reporting bugs

Open an issue with: what you did, what you expected, what happened, versions (Omarchy, Quickshell, Hyprland, this project), and logs with **secrets removed** (`journalctl --user -u owd`). Feature ideas: describe the problem first, then your proposal.

## Good first contributions

- Fixture tests for a parser or provider.
- A new `/proc`-based `system` topic (disk, network) following the existing provider.
- Docs and examples.
- Checking a documented Omarchy or Quickshell fact and recording the evidence in DECISIONS.md.

## License

By contributing you agree that your contribution is licensed under the [MIT License](LICENSE).
