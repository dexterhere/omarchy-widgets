# Kickoff prompts for Claude Code

> **Historical, not maintained.** Prompts 1 to 6 have largely been executed; the Phase 0 spikes S1 to S10 have not. This file is kept for reference only. The current status is in [HANDOFF.md](HANDOFF.md) and [BUILD_PLAN.md](BUILD_PLAN.md).

Setup (once)

```bash
mkdir omarchy-widgets && cd omarchy-widgets
git init
# copy this handoff folder's contents in: CLAUDE.md, docs/, design/
git add . && git commit -m "Add project brief and agent context"
claude
```

Use the prompts in order. Each is self-contained; paste one at a time and review the result before the next.

## Prompt 1: orientation and repo bootstrap

```
Read CLAUDE.md, then docs/PROJECT_BRIEF.md, docs/DAEMON_SPEC.md and docs/BUILD_PLAN.md completely. Then:
1. Summarize in under 15 lines what we are building and the three decisions you consider riskiest.
2. List anything in the docs that contradicts itself or that you believe is wrong.
3. Create docs/DECISIONS.md and docs/OPEN_QUESTIONS.md (empty templates: date, decision, reason, what would change it).
4. Bootstrap the Cargo workspace with the crate layout in DAEMON_SPEC section 3, `#![forbid(unsafe_code)]` everywhere, rustfmt/clippy config, deny.toml, and a CI workflow that runs fmt, clippy -D warnings, test, cargo deny and cargo audit. No feature code yet.
Do not touch my real ~/.config. Do not contact anyone or publish anything. Stop and show me the summary before committing.
```

## Prompt 2: Phase 0 environment and spikes S1 to S3

```
We are in Phase 0 (docs/BUILD_PLAN.md). First check what I have on this machine: Omarchy version, Quickshell version, Hyprland version (read-only commands only), and tell me whether it is safe to run plugin experiments here or whether I should use a VM. Do not change anything yet.
After I confirm, work through spikes S1, S2 and S3 one at a time in spikes/<name>/. For each: write the smallest experiment, run it against throwaway XDG dirs where possible, record exact evidence (commands, output, screenshots if available) and the conclusion in docs/DECISIONS.md. If an experiment needs to touch the real shell config, show me the diff and wait for approval. Stop after each spike and summarize.
```

## Prompt 3: spike S4 (vault and keyring) plus S5 to S10

```
Continue Phase 0 with S4. Build a throwaway Rust program that stores and reads a random secret through the Secret Service, then a passphrase-mode (Argon2id) round trip, then evaluate TPM sealing with systemd-creds if a TPM exists. The key question: on this Omarchy install, what can an arbitrary process of my own user read from the keyring without any prompt? Demonstrate it with a second unrelated process and report plainly. Check maintenance status (last release, open advisories) of each crate you use and record it.
Then do S5 to S10 in order, same rules as before. Update docs/OPEN_QUESTIONS.md as questions are answered or new ones appear.
```

## Prompt 4: threat model and spec schema v0

```
Using the spike results, write docs/threat-model.md (plain language, honest about code widgets, with the keyring finding from S4) and draft spec/widget.schema.json and spec/layout.schema.json (JSON Schema 2020-12) matching PROJECT_BRIEF section 7. Add tests that validate example widgets against the schemas, including invalid ones. List every schema field you invented that is not in the docs.
```

## Prompt 5: daemon steps 1 to 3

```
Phase 1, epic 1. Implement DAEMON_SPEC section 12 steps 1 to 3 with tests first: the skeleton and systemd unit, the socket server with correct modes and SO_PEERCRED, the line-delimited JSON codec (with fuzz target), grants (deny by default, field filtering, rate limit) and the redacted audit log. Include the security regression tests from section 11 that apply so far. Keep commits small. At the end, run everything and show me the test output and `systemd-analyze --user security` result.
```

## Prompt 6: vault (step 4)

```
Implement DAEMON_SPEC step 4: the vault file format, keyring and passphrase modes, and the `ow vault` commands, with the tamper tests, wrong-passphrase test, rotation test, and the fake-secret leak test (secret must not appear in logs, audit, argv, environ, or any non-vault file). Secrets enter only via stdin or a no-echo prompt. Before writing code, show me the exact file format you will use and the crate choices with maintenance evidence, and wait for my approval.
```

## Later prompts (write these after Phase 0 results)

- Bridge plugin and kit: "Implement the bridge service plugin and the kit components so a throwaway widget shows live CPU in two themes and follows `omarchy theme set`."
- Canvas: "Implement free-form place/resize/remove in the chosen host, following PROJECT_BRIEF section 6 and the interactions in design/README.md."
- `ow` manager verbs, Widget Center v0, first widgets, packaging, then Phase 2 providers.

## Tips

- Keep `docs/DECISIONS.md` current; it is the agent's memory across sessions.
- If a session ends mid-task, start the next with: "Read CLAUDE.md and docs/BUILD_PLAN.md, check git log and the checkboxes, then continue with the next unchecked item."
- Review anything that touches crypto, the protocol, grants, or the vault yourself or with an independent reviewer. Do not rely on the agent alone for those.
