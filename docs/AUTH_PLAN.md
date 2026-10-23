# Authentication plan for Omarchy Widgets

Status: design proposal, partly accepted on 2026-10-23 (see the box below). Written 2026-10-23.

Accepted by Dexter on 2026-10-23:
- Default unlock mode is "convenient": the vault key is released at login through the Secret Service keyring, with the passphrase kept as recovery. "Strict" (passphrase every login) stays available.
- First GitHub credential is a guided fine-grained, read-only personal access token (`ow connect github`). Device flow through a project-owned GitHub App waits until the project name is decided; a user-created GitHub App is a possible advanced path.
- All network code lives in one egress module with a host allow-list, https only, no redirects and no proxy variables. TLS is rustls; no OpenSSL or other C TLS stack. No certificate pinning.
- Any new HTTP, TLS or keyring dependency needs a maintenance check, `cargo deny` and `cargo audit`, and a DECISIONS entry before it is added.
- No telemetry. A user-facing `ow auth audit` lists contacted hosts.
Still open: the numbered questions in section 8, and everything marked (verify). Read-only research: nothing in either repo was edited, no network call was made, no real config, keyring or secret was touched.

Honesty conventions used below:
- "Verified in code" means I read the file named. "Docs say" means I did not find it in code.
- Facts about GitHub's API, token types and limits come from my background knowledge, not from a fetch. They are marked **(verify)** and must be checked against current GitHub docs before anything is built on them.
- Nothing here makes an unsandboxed QML code widget safe. A code widget runs as the user and can read anything the user can.

Repos: code = `/home/buggybuck/omarchy-ext/omarchy-widgets/omarchy-widgets/` (called "code repo"), web = `/home/buggybuck/omarchy-ext/omarchy-widgets/omarchy-widgets-web/` ("web repo").

---

## 0. Summary of the recommendation

1. First credential: a **fine-grained personal access token (PAT), read-only**, pasted once into `ow vault set github-main`. Permissions: Metadata read (automatic) and Pull requests read. Short expiry (30 to 90 days). One resource owner (the user's own account).
2. Second, after the daemon can write to the vault: **device flow through a GitHub App** (public client_id, no client secret), whose user tokens are expiring and have per-permission scopes. Not an OAuth App: its `repo` scope is read/write on all private repos and cannot express "read pull requests only".
3. Never first: classic PAT (only coarse `repo`), `gh auth token` reuse (borrows a broad token the user did not create for us). Offer neither as the documented path. Classic PAT stays accepted by the daemon because the vault stores any string, but the docs recommend against it.
4. The widget never sees a credential. It names a provider and topic, declares `auth: vault`, and receives granted fields only. The token exists in `owd` memory and in the encrypted vault, and is attached to the HTTP request by an egress wrapper, not by provider code.
5. The biggest gap is not GitHub. **Today `owd` cannot open the vault, has no HTTP client and no egress allow-list.** The auth plan therefore starts with "how does the daemon get the key" (section 5, items 1 to 3).

This differs from the docs, which say "GitHub device flow first" (`docs/HANDOFF.md`, `docs/BUILD_PLAN.md` line 102, `docs/DAEMON_SPEC.md` section 12 step 6). Reasons are in section 1.

---

## 1. Credential types for GitHub

### 1.1 What the job needs

The first topic is `github.prs` with fields `open` and `review` (web repo `registry/providers.json`; the concept fixture binds exactly these). Meaning: count of my open PRs, and count of PRs where my review is requested. Both are searches:

- open PRs: `is:pr is:open author:@me`
- review requests: `is:pr is:open review-requested:@me`

Either a GraphQL `search(type: ISSUE)` query asking only for `issueCount` (one request for both counts, a tiny response) or REST `GET /search/issues` (two requests). Prefer GraphQL for the counts. **(verify)** that a fine-grained PAT can run GraphQL `search` and return counts across all repos it covers; limits differ for private repos outside the token's repository selection (those are invisible, so the counts would be lower than the user expects; this must be a stated limitation, see 1.4).

### 1.2 Options compared

| Option | Least privilege for PR counts | Needs Dexter-owned infrastructure | Daemon must write the vault | Expiry and revoke | Verdict |
|---|---|---|---|---|---|
| Fine-grained PAT | Good: Pull requests read, Metadata read, chosen repos or all repos of one owner. Read-only is expressible. | None | No | Expiry set by user (max set by GitHub/org policy **(verify)**); user can revoke in GitHub settings | **Recommend first** |
| Classic PAT | Poor: private PRs need `repo` (read and write all repos). `public_repo` or no scope sees only public data. | None | No | Optional expiry; revoke in settings | Accept silently, do not recommend |
| OAuth App, device flow | Poor: scopes are coarse (`repo`); cannot ask "pull requests read" **(verify)**. No scope = public data only. | Yes: an OAuth App owned by Dexter, device flow switched on | Yes, if tokens expire; classic OAuth tokens do not expire by default **(verify)**, which is its own risk | User can revoke the app in settings; Dexter can revoke app-wide | Do not build |
| GitHub App, device flow (user-to-server token) | Best: fine-grained permissions on the App; token is the intersection of the App's permissions and the user's access; must be installed on the repos or accounts it should see **(verify)** | Yes: a GitHub App, device flow enabled, client_id public | Yes: access tokens expire (about 8 h) and need a refresh token; refresh tokens rotate **(verify)**, so the daemon must persist the new pair | Short-lived access token limits damage; user can uninstall or revoke | **Recommend second** |
| `gh auth token` reuse | Poor: the token `gh` holds usually carries broad scopes (`repo`, `read:org`, `gist`) **(verify)** | None | No | The user's own gh session; we cannot rotate it | Do not recommend; see below |

### 1.3 Why not `gh auth token` reuse

- Over-scoped: we would hold a token that can write to every private repo to display two numbers.
- It must be fetched by spawning `gh` (the daemon should not spawn user programs) or by reading `~/.config/gh/hosts.yml` (the web repo's own `FILES_DENY` list in `scripts/gate/check-policy.ts` lists `~/.config/gh` as a protected location for widgets, for exactly this reason).
- Revoking it breaks the user's `gh` too, so the user will not revoke it casually.
- Allowed as a one-time manual import only: the user may run `gh auth token`, look at it, and decide. We do not automate it. If an import helper is ever added, it must read the value from a pipe into the prompt path used by `ow vault set` and print the scopes it found, then warn.

### 1.4 Fine-grained PAT, concrete recipe the CLI should print

1. GitHub settings, Developer settings, Fine-grained tokens, Generate new token.
2. Resource owner: your own account. (An organisation needs its own token and, often, owner approval **(verify)**; the first release supports one token per account only, see open question Q6.)
3. Expiration: 30 to 90 days. A token with no expiry is refused by our guidance (the daemon cannot enforce it; it can only warn when it sees the expiry header, see 3.6).
4. Repository access: "All repositories" for full counts, or a selection (counts then cover only those repos; the widget must say so).
5. Permissions: Repository, Pull requests: Read-only. Metadata: Read-only is added automatically. Nothing else. No account permissions.
6. Run `ow vault set github-main` and paste it. It is shown once by GitHub; do not store it elsewhere.

Honest limit: a fine-grained PAT with "Pull requests: read" can still read the contents of every PR in scope (titles, diffs, comments). That is more than two counts. The least-privilege floor on GitHub is permission-level, not field-level; our daemon narrows to two integers, but the token itself is broader than the widget's need. Say this in the UI.

### 1.5 Device flow and the public client_id

Assessment:
- Device flow is the OAuth 2.0 device authorization grant. The client holds only a `client_id`. There is no client secret, so there is nothing to embed or leak. A client_id is an identifier, not a credential; publishing it in source is fine. **I will not propose embedding a client secret anywhere, and the GitHub App/OAuth flow chosen must work without one.**
- Cost: Dexter must create the App (a human-only task per `CLAUDE.md`), turn on device flow, pick its name and logo, and own its lifecycle. That identity is shown to every user on the GitHub authorise page ("Omarchy Widgets wants access"). It is tied to the project name, which is undecided (D-3).
- Phishing: anyone can start a device flow using our client_id and trick a victim into entering the attacker's code on `github.com/login/device`; the victim then authorises an app named after us for the attacker's session. Mitigation is on the user's side and in our UI: the code is displayed only by `ow` in the user's own terminal, started by the user, and the instructions tell the user to confirm the code in the terminal matches the one on github.com and that they started the flow. A fork using our client_id is not a secret leak but does lend our name to a fork; accept, document.
- Refresh: with a GitHub App the daemon must keep a refresh token and rewrite the vault. That needs the unlocked key in the daemon and a vault write path from the daemon (not built, see section 5). It also needs the refresh-token reuse behaviour **(verify)**: refresh tokens are single-use, so a crash between receiving and saving the new pair can lock the user out and force a new login. The write must be atomic and happen before the old token is discarded (the existing `write_atomic` in `crates/ow-vault/src/store.rs` already does temp file, fsync, rename).
- Rate/poll: during device flow the CLI must respect the `interval` and `slow_down` responses and stop at code expiry.

Conclusion: device flow is a good second step, not the first. It adds infrastructure Dexter must own and a daemon-side vault write path before it adds anything a fine-grained PAT cannot already do.

### 1.6 What is deferred

| Item | Deferred until |
|---|---|
| GitHub App device flow with refresh | Daemon can unlock and write the vault; App created by Dexter; project name decided |
| Notifications, issues, contribution graph topics | After `github.prs` works; each topic adds permissions and a new threat review |
| Multiple GitHub accounts or organisations | Question Q6 answered |
| Fine-grained PAT for org resources | Same |
| GitHub Enterprise Server hosts | Needs a per-user egress host; conflicts with a static host allow-list. Not in v1. |

---

## 2. User flow, end to end

### 2.1 CLI first (what exists vs what is new)

Verified in code (`crates/ow/src/vault_cmd.rs`, `crates/ow/src/input.rs`):
- `ow vault init|status|list|set <name>|remove <name>|rotate|wipe --yes` exist and are tested by `crates/ow/tests/vault_cli.rs`.
- Secrets come from a no-echo terminal prompt (ECHO, ICANON, ISIG cleared; escape sequences dropped; value confirmed twice) or from stdin; extra argv is rejected (`secret_in_argv_is_rejected_not_stored` test). There is deliberately no `get`.
- `ow` calls `harden_process()` (umask 0077, non-dumpable, no core dumps).
- `ow vault rotate` rotates the **vault passphrase and data key**, not a token. To rotate a token you run `ow vault set github-main` again (it overwrites; `Vault::set` inserts over the existing record, verified in `store.rs`). The wording in the docs and the UI must not call `rotate` "token rotation".

Gap found in code: the passphrase and the value both come from `stdin` (`secret()` reads one line each). When stdin is a pipe, line 1 is the passphrase and line 2 is the value. So `some-command | ow vault set github-main` cannot work: the pipe would be taken as the passphrase. A user-friendly "pipe the token in" flow needs the passphrase to come from the terminal (`/dev/tty`) while the value comes from stdin. This is a small, testable change and avoids the temptation to put a token in a shell variable or argv.

Proposed flow (CLI):

```
$ ow vault init                      # once: passphrase, prompted twice, no echo (exists)
$ ow auth github                     # NEW: prints the permission recipe from 1.4, links, then...
$ ow vault set github-main           # exists: passphrase prompt, then token prompt (no echo, twice)
$ ow vault unlock                    # NEW: lets the daemon use the vault until timeout / logout
$ ow auth github test                # NEW: asks the daemon to call GET /rate_limit or a one-shot count;
                                     #      prints "ok, token expires in 41 days" or a category error.
```

`ow auth github` is a help/diagnostic wrapper and must never accept the token itself. It only prints text and runs `test` through the daemon. The token is always entered via `ow vault set`.

### 2.2 How a widget declares it needs auth

Verified in the web repo:
- Catalog entry `github`: `class: personal`, `auth: vault`, `egress: ["api.github.com"]` in `registry/providers.json`.
- Widget side: `registry/schema/widget.schema.json` has `providers[].auth` enum `none | vault`, described as a "Draft addition". `scripts/gate/check-providers.ts` rejects a widget whose declared `auth` differs from the catalog; the widget cannot lie its way to a different auth, but it also cannot say anything more specific (which credential kind, which permissions).
- The concept fixture `registry/fixtures/io.github.dexterhere.concept-github-activity/widget.json` declares `providers[0].auth: "vault"` and `capabilities.network: ["api.github.com"]`.

Problem I found by reading `scripts/gate/check-policy.ts`: that fixture would fail the policy. A `spec` widget must declare no network (`spec widgets declare no network, files or spawn`) and no widget may combine a sensitive provider (`personal` is sensitive) with its own network (`no widget combines a sensitive provider with its own network, files or spawn`). The fixture is exempt only because gates run for non-fixture listings (`scripts/build-registry.ts` treats `source: "fixture"` separately). The code repo's `docs/PROJECT_BRIEF.md` example has `network: []` for the same widget. So the fixture teaches the wrong thing. Egress for a vault-backed provider belongs to the provider (catalog `egress`), and the widget's own `capabilities.network` must be `[]`. See section 7.

Widget declares (target shape): provider name, topics, fields, `auth: vault`. It never names a secret, a scope or a host.

### 2.3 How the user grants it

Two different decisions, kept separate:

1. **Credential binding (provider level)**: "the `github` provider uses vault secret `github-main`". Set once by the user. Lives in the daemon config (`config.toml` is design only, not implemented). A widget cannot change it.
2. **Widget grant (widget to topic to fields)**: "widget `io.github.you.github-activity` may read `github.prs` fields `open`, `review`". This is the existing `grants.toml` mechanism, verified in `crates/ow-core/src/grants.rs`: deny by default, field filter (top-level keys), file must be 0600 and ours, loaded without following symlinks, SIGHUP reload that fails closed, duplicate grants rejected.

Install-time prompt (CLI now, Widget Center later), generated from the widget's declaration and the provider catalog, in plain words:

```
io.github.you.github-activity (spec widget) asks to:
  read  github.prs: open, review      (two numbers)
  using your GitHub token  secret:github-main  (read-only PAT expected)
  The daemon, not the widget, talks to api.github.com.
  The widget never receives the token.
Grant? [y/N]
```

`ow grants` and `ow install` do not exist (verified: `ow` handles only `status` and `vault`). Until they do, the user edits `grants.toml` by hand. That is acceptable for a first slice if the docs say so.

Honest limit: `widget` in a request is a self-claimed string (D-1, `docs/DECISIONS.md`). Any same-uid process can claim any widget id and receive the granted fields. The grant protects the credential (never exposed) and stops honest widgets over-reading; it does not stop a hostile local process from reading the two counts.

### 2.4 Revoke and rotate

| Action | Command | What it does | What it does not do |
|---|---|---|---|
| Stop one widget reading | edit/remove grant, then SIGHUP (later `ow grants revoke`) | Grants re-checked on every push (verified in `hub.rs`); data stops at the next push | Does not revoke the token |
| Stop the provider using the token | `ow vault remove github-main` (exists) | Removes it from the vault file | Must also purge the daemon's in-memory copy and every cache entry derived from it. Docs promise this ("Removing a secret purges all cache entries derived from it"); not implemented because the daemon never opens the vault. Needs a build item. |
| Rotate the token | create new PAT on GitHub, `ow vault set github-main` (overwrite), `ow auth github test`, then delete the old PAT on GitHub | New value used on the next fetch after a reload | We cannot revoke at GitHub for the user; the UI must say "also delete the old token on GitHub" with the settings link. |
| Rotate vault passphrase | `ow vault rotate` (exists) | New salt, new data key, re-sealed | Does not change any token |
| Panic | `ow vault wipe --yes` + revoke the token on GitHub | Removes the local file | Backups and snapshots stay decryptable with the passphrase (the CLI says so) |

Token expiry (fine-grained PATs expire): the daemon should read GitHub's token-expiry response header **(verify its name)** and report "expires in N days" in `ow status` and the Installed tab, so the user is not surprised by a blank card. When it expires, the provider enters the `expired` state (3.6).

### 2.5 Widget Center UI later

A future password dialog is the weakest link. A QML code widget is same-uid and unsandboxed, so it can draw a convincing fake "enter your GitHub token" overlay or fake the Center's own dialog. Rules for the later UI:
- Credential entry happens in a separate process owned by the project (not in a widget's QML tree), launched by the Center, never by a widget.
- The Center never shows a secret back. It shows name, class, last used, expiry, and which widgets are granted.
- The dialog tells the user the exact origin and the exact permissions expected, and refuses obviously over-broad classic tokens (prefix `ghp_`) unless the user clicks through a warning.
- Until the project can offer a separate, trustworthy prompt, keep entry in a terminal and say plainly that a fake prompt in any program running as you is possible.

---

## 3. Data flow

### 3.1 Picture

```
 widget (QML/spec)            bridge           owd (one process, non-dumpable)                     internet
 ------------------           ------           -------------------------------------------         --------
  asks: github.prs   ---->   unix socket --->  SO_PEERCRED uid check
  (names no secret)                           grant check (widget, topic, fields)  [exists]
                                              rate limit (widget, topic)           [exists]
                                              hub cache / scheduler                [exists]
                                                 |
                                                 v  (cache miss or poll due)
                                              provider "github" (our compiled code)  [NOT BUILT]
                                                 | asks wrapper: POST /graphql, auth = secret "github-main"
                                                 v
                                              egress wrapper                          [NOT BUILT]
                                                 - host must be in provider's egress list
                                                 - https only, system TLS roots
                                                 - redirects: none followed
                                                 - adds Authorization header itself
                                                 - timeouts, size cap, no body logging
                                                 |  vault (unlocked key in RAM)      [NOT WIRED]
                                                 +----------------------------------------> api.github.com
                                                 <-------------- JSON counts -------------
                                              provider output trimmed to declared fields   [exists]
                                              cache in RAM, sensitivity high
  {open:4, review:2} <---- push ----------    granted fields only                    [exists]
```

### 3.2 Who holds the token

Only `owd`. Inside `owd`, narrower: the vault handle and the egress wrapper. Provider code should receive a **handle**, not a string: `fetch` asks the wrapper to perform a request "authenticated as secret X", and the wrapper adds the header. Then a bug in provider parsing code cannot print the token because it never has it. This also makes the "malicious or compromised provider" row (section 4) smaller. Today's trait (`crates/ow-core/src/provider.rs`) has `secrets()` returning names and `egress_hosts()`, both defaulting to empty and unused; `fetch(topic, params)` has no context argument. A context parameter carrying the wrapper is a trait change that must land with the first network provider.

### 3.3 Unlocking: how the daemon gets the key (the missing piece)

Verified: `owd` does not depend on `ow-vault` (`crates/owd/Cargo.toml`), and `ErrorCode::Locked` exists but is never emitted. The vault is passphrase mode only. Options:

| Option | How | Pros | Cons |
|---|---|---|---|
| A. Passphrase to daemon over the socket | `ow vault unlock` prompts, sends once on the 0600 socket; `owd` derives the key, keeps it in zeroizing memory, auto-locks after idle/timeout | Smallest build; no new dependency; key never touches disk or a keyring | Passphrase crosses a same-uid socket (any same-uid process can also connect, but needs the passphrase to unlock anything; but it could send a wrong passphrase repeatedly: needs attempt limiting); frame must be exempt from any logging; D-1 still open |
| B. Kernel keyring (as the spec sketches) | `ow` derives the key, stores it with a timeout via keyctl; `owd` reads it | Survives daemon restarts within the timeout | Same-uid processes in the same session may read it depending on key permissions **(verify)**; extra syscalls; interacts with unit `KeyringMode=private` in `packaging/owd.service` (the unit sets `KeyringMode=private`, so the daemon would not see the user's session keyring without changes) |
| C. Secret Service keyring | Master key stored in gnome-keyring | Unlocks at login without a prompt | Docs themselves say it is weak under autologin (spike S4 unrun); not implemented |
| D. TPM via systemd-creds | Sealed key | Convenient, resists disk theft | Spike only, not implemented; same-uid process on a logged-in machine may still be able to unseal via the daemon |

Recommendation: A for the first network provider, with a bounded idle timeout and attempt limit, and say plainly that after unlock any process running as you could ask the daemon for what grants allow. B/C/D remain follow-ups for spike S4. Note that unlock is a user-present action each boot (or each timeout). That is a UX cost; the alternative is a weaker mode. Dexter decides (open question Q1).

### 3.4 Calling api.github.com

The wrapper rules (none exist yet; there is no HTTP client in the workspace, verified by grep of all `Cargo.toml`):

- Allow-list: the request host must equal one of the provider's `egress_hosts()` exactly (`api.github.com`). Compare the parsed host, not a substring of the URL. No IP literals, no ports, no userinfo in the URL.
- Scheme https only; system TLS roots; no custom CA option; certificate errors are errors.
- Redirects: do not follow any redirect. GitHub answers 301/307 for renamed or moved endpoints; our two fixed endpoints do not need redirects, and a redirect that carried the `Authorization` header to another host is the classic token leak. Treat any 3xx as `unavailable` and log the category only.
- Token placement: `Authorization: Bearer <token>` header only, attached by the wrapper, never in URL, query string, body or any other header; never copied into an error value. The type holding it must not implement `Debug`/`Display` (already the approach in `ow-vault`: `Zeroizing`, "never `Debug`-printed").
- Required request headers GitHub wants (User-Agent, API version) **(verify)** are fixed constants in the provider.
- Limits: connect and total timeout (suggest 10 s and 20 s), response size cap (suggest 256 KiB; counts need far less), no decompression bombs, JSON depth limit, parse into a typed struct with only the fields we use.
- Logging: the wrapper logs category and HTTP status class at most. It never logs URLs with query strings, headers, request or response bodies. `ow_core::harden_process` already withholds panic messages (verified in `harden.rs`), which matters because a `panic!` or `expect` formatting a request would otherwise write it to the journal.
- Proxy environment variables (`HTTPS_PROXY`) must not be honoured silently: a proxy sees the token in TLS-terminating setups. Either ignore them or require an explicit config; decide in Q4.
- DNS: rely on TLS name verification; additionally refuse hosts resolving to loopback/private ranges as defence in depth (cheap, optional).
- Library choice: the `Provider` trait is synchronous on purpose ("blocking HTTP with timeouts fits"). A small blocking client with rustls fits. Candidate crates must pass `cargo deny`/`cargo audit` and have their maintenance status checked and recorded in `docs/DECISIONS.md` before adoption, exactly as the repo rules require. Not decided here.

### 3.5 What the widget receives

Only the granted fields of the topic, as plain numbers, plus `ts` and `stale`. `TopicSpec::sanitize` drops anything undeclared and `Grants` filter top-level keys (both verified). Provider output is capped at 48 KiB serialized (verified in hub notes). For `github.prs` that means `{open: n, review: n}` and nothing about repos, titles or authors. If a later topic exposes PR titles, that is a new `high`-sensitivity topic with its own grant and a new review; titles can be private business information.

Because field filtering is top-level only, never nest mixed-sensitivity data under one key (existing rule).

### 3.6 Caching, polling, rate limits, error states

- Cache: RAM only, sensitivity `High`, no disk persistence. Verified: `Sensitivity::High` means memory only; the hub cache is RAM.
- Polling happens only while a granted widget is subscribed (verified: "Polling happens only for keys with live subscriptions").
- Interval: the widget manifest's `interval` setting (fixture: default 60, min 30, max 600) is a request. The provider's `TopicSpec.interval` is the real authority, and the daemon should clamp up: default 120 s, floor 60 s, backoff to 900 s on errors (hub already backs off exponentially up to 16 times the interval).
- Limits to respect **(verify all numbers)**: authenticated REST about 5,000 requests/hour; GraphQL has its own point budget; the search endpoints have a much smaller per-minute limit (about 30/min). A 120 s poll for one GraphQL request is about 30 requests/hour. Multiple widgets share one cache key because subscriptions are keyed by `(topic, params)` (verified), so ten widgets still produce one request.
- Conditional requests: use `ETag`/`If-None-Match` for REST calls where GitHub supports it; 304 responses do not count against the primary limit **(verify)**. GraphQL POST does not support conditional requests; that is a reason to keep the interval at 120 s or more.
- Rate-limit handling: read `x-ratelimit-remaining` and `x-ratelimit-reset` and `retry-after`; when remaining is low or a 403/429 arrives, stop polling until reset, serve the last good value with `stale: true`.
- Errors shown to the user are categories only. The existing contract: error messages never contain tokens or upstream bodies (`ProviderError` carries a category only; `docs/DAEMON_SPEC.md` section 5).

States the user must be able to tell apart (today the protocol only has `stale: true` and eight error codes, so a `state` field is a protocol change, see section 5 item 9):

| State | Cause | Message shape (no secret) | Action offered |
|---|---|---|---|
| locked | vault not unlocked | "Credentials locked" | `ow vault unlock` |
| no_credential | no secret bound or name missing | "GitHub not set up" | show recipe |
| unauthorized | 401: revoked, wrong, or expired | "GitHub rejected the token" | replace token |
| expired_soon | expiry header within N days | "Token expires in 5 days" | create new token |
| insufficient | 403 with missing permission (not rate-limit) | "Token lacks Pull requests: read" | fix permissions |
| rate_limited | limit reached | "Paused until 14:32" (reset time is not secret) | none, automatic |
| offline | DNS/connect/TLS failure | "Offline, showing data from 14:10" | none |
| unavailable/format | 5xx or unexpected JSON | "GitHub unavailable" | none |

Distinguishing 403 rate-limit from 403 permission needs the headers **(verify)**; if unsure, say "forbidden" and do not guess.

---

## 4. Threat model for credentials

Baseline honesty: same-uid malware wins against everything here except credentials that are not present on the machine. The vault protects copies of files and keeps secrets out of logs, argv, env and widgets. It does not defend a live logged-in session.

| Threat | What it can do | Mitigations | Residual risk |
|---|---|---|---|
| Malicious spec widget | Ask for topics/fields. Cannot run code. | Deny by default; field filtering; widget declares no network/files/spawn; listing gates (`check-policy.ts`); only the trusted renderer talks to the bridge. | It gets exactly what was granted. Widget id is self-claimed until D-1 is solved, but a spec widget has no way to call the socket itself. |
| Malicious code widget (QML) | Runs as the user, unsandboxed. Can read `~/.config/gh`, browser profiles, the vault file, the socket, env of the shell. | Listing policy forbids code widgets from using personal providers at all and forbids combining a sensitive provider with network/files/spawn (`check-policy.ts`, "listing policy ... not a sandbox"). `FILES_DENY` includes `~/.config/gh`, `~/.config/owd`, `~/.local/share/owd`, keyrings. | The listing check is a speed bump on the web registry only. A code widget installed from a raw git URL, or one that evades the lint, can connect to the owd socket, claim any widget id, read granted data, read other credentials on disk, and show fake prompts. It cannot read our token from `owd` memory (non-dumpable) or from the vault without the passphrase, but it can read the passphrase if it can observe keystrokes in the terminal or replace `ow`. |
| Malicious or compromised provider | Providers are compiled into `owd`: our code. A compromise is a code compromise in our repo or in a dependency. | Provider gets a request handle, not the token; egress wrapper enforces hosts and no redirects; provider output trimmed to declared fields; `secrets()` name allow-list per provider; panic messages withheld. Third-party providers do not exist and should not until there is a process boundary. | Rust in-process code can still read daemon memory. Supply chain below. An out-of-process provider with a seccomp/ network namespace is the real fix and is out of scope for v1. |
| Local malware as the same user | Everything the user can do. | Vault is encrypted at rest; `PR_SET_DUMPABLE=0` blocks ptrace/`/proc/pid/mem` for non-root same-uid processes; socket 0700/0600; no secrets in env/argv/files. | After unlock, it can use the daemon for granted data; it can keylog or shim the passphrase prompt; it can steal the token **if it exists elsewhere** (browser session at GitHub, `gh`, the clipboard when the user pasted it). The token we store is as strong as the user's GitHub settings: scoping and expiry limit damage, nothing else does. State this in the UI. |
| Log or crash leakage | Token appears in journal, audit log, core dump, panic text, error string, cached error body. | Never logged by construction (wrapper, typed handle, no `Debug`); audit entries have no payload field (verified); core dumps disabled; panic payloads withheld; error types carry categories; required test: run a full session with a fake token and grep all output, files, `/proc/<pid>/cmdline` and `environ` for it. | Terminal scrollback of the paste prompt is not visible (no echo) but screen recording is. Library debug logging in a dependency (TLS/HTTP crates) must be checked. A heap residue in freed memory is not zeroed unless the type is `Zeroizing`; HTTP library internals copy headers (known gap, see section 5 item 6). |
| Token over-scoping | A broad token turns any leak into a large breach. | Recommend fine-grained, read-only, Pull requests only, expiring; refuse to promote classic/`gh` reuse; later GitHub App short-lived tokens; show scopes and expiry in status; warn on `ghp_` prefix. | We cannot read a PAT's permissions without calling the API; a classic token cannot be narrowed. User choice. |
| Phishing prompts | Fake `ow` prompt, fake Center dialog, fake device-flow page, or a malicious widget's overlay asking for the token or passphrase. | Entry only in the terminal in v1; documented exact flow; device flow code is shown by `ow` and the user confirms; Center dialog is a separate project-owned process; never ask for the token inside a widget; all prompts show origin and permissions. | A same-uid program can mimic any terminal or window. We cannot prove to the user that a prompt is genuine. Mitigation is habit and distinct process. |
| Supply chain | Malicious crate, build, release or registry listing. | `cargo audit`, `cargo deny`, locked deps, small crypto surface, fuzz targets, pinned commits for widgets, secret scanner (G7), maintenance checks recorded for new deps. | Signed releases and SBOM are planned, not done (`CLAUDE.md`: "no SBOM or signing yet"; PKGBUILD blocked on a signing key). Adding an HTTP+TLS stack roughly doubles the dependency count; this needs its own review. |
| Rollback of vault file | Attacker restores an older valid vault, bringing back a revoked token. | Documented non-goal (`VAULT_FORMAT.md`). | Mitigated only by the user revoking tokens at GitHub when removing them. |
| Redirect or host confusion | Token sent to an attacker host. | Allow-list on parsed host; no redirects; no proxies; https only; no user-supplied URLs reach the wrapper. | A bug in URL parsing; needs egress tests (section 5 item 5). |
| Denial of daemon | Flooding, slow responses, huge responses. | Existing caps and rate limits; timeouts and response size cap in the wrapper. | A same-user flooder uses its share. |

Direct statement for users and docs: an unsandboxed QML code widget can read anything you can, including files holding other credentials. That is why the policy refuses to let a code widget use a personal provider, and why no widget may have both a personal provider and its own network/files/spawn. The rule exists in `scripts/gate/check-policy.ts` (verified); it is a listing rule enforced when a widget is listed in the web registry, not a runtime barrier.

---

## 5. What exists today, and the ordered build list

### 5.1 Audit of the current code (checked by reading and grepping, not from docs)

| Capability | Status | Evidence |
|---|---|---|
| Passphrase-mode vault, set/remove/list/rotate/wipe | Implemented | `crates/ow-vault/src/store.rs`, `crates/ow/src/vault_cmd.rs` |
| No-echo prompt, secrets never via argv | Implemented | `crates/ow/src/input.rs`; test `secret_in_argv_is_rejected_not_stored` |
| Fake-secret leak test across CLI output and all written files | Implemented for `ow` only | `crates/ow/tests/vault_cli.rs::full_session_never_prints_or_stores_secrets_outside_the_vault` |
| Process hardening (umask, non-dumpable, no core, quiet panics) | Implemented | `crates/ow-core/src/harden.rs`, tests in `vault_cli.rs` and `owd/tests/hardening.rs` |
| Grants (deny by default, fields, SIGHUP reload fail closed) | Implemented | `crates/ow-core/src/grants.rs`, `owd/src/hub.rs` |
| Audit log without payloads, rotation | Implemented | `crates/ow-core/src/audit.rs` |
| Rate limit per (widget, topic) | Implemented | `crates/ow-core/src/ratelimit.rs` |
| Provider hub: RAM cache, subscriptions, stale + backoff | Implemented | `crates/owd/src/hub.rs` |
| `Provider` trait has `egress_hosts()` and `secrets()` | Present but unused (defaults empty, no caller, no context argument to `fetch`) | `crates/ow-core/src/provider.rs` |
| `ErrorCode::Locked` | Defined, never emitted | `crates/ow-proto/src/msg.rs` |
| `owd` opens or reads the vault | **Not implemented** | `owd/Cargo.toml` has no `ow-vault`; grep finds no `ow_vault` outside `ow` |
| HTTP client, TLS, egress allow-list | **Not implemented** | no HTTP crate in any `Cargo.toml` |
| GitHub provider, any network provider | **Not implemented** | only `system` exists |
| `ow vault unlock`, `ow auth`, `ow grants`, `ow audit`, `ow install` | **Not implemented** | `crates/ow/src/main.rs` handles only `status` and `vault` |
| Admin ops on the socket (`grants.*`, `vault.*`, `audit.tail`) | **Not implemented** | `ow-proto::Op` has only hello/snapshot/subscribe/unsubscribe/status |
| `config.toml` (provider to secret binding) | **Not implemented** | docs say so; no reader in `paths.rs` or `owd` |
| Purge cache on secret removal | **Not implemented** | daemon never sees the vault |
| Keyring and TPM vault modes, kernel-keyring caching | **Not implemented** | VAULT_FORMAT, `ow vault status` shows passphrase mode only |
| Widget authentication to the daemon (D-1) | **Not implemented**, undecided | `docs/DECISIONS.md` pending list |
| Egress tests, different-uid refusal test | **Not implemented** | `DAEMON_SPEC.md` section 11 status; open question 13 |
| Provider auth schema beyond `none|vault` | **Not implemented** | `registry/providers.json`, `check-providers.ts` types |
| Catalog `serving` list for github | Absent (only `system` has it) | `registry/providers.json` |

### 5.2 Ordered build list with acceptance tests

Rule for every item: a test that fails if the security rule is broken. Fake values only; use a throwaway directory (`XDG_*` or `--config-dir`), never the real config or keyring.

| # | Item | Status today | Acceptance test |
|---|---|---|---|
| 1 | Decide D-1 minimum for `vault.unlock` and the config binding (Q1, Q2); record in `DECISIONS.md` | Not decided | Decision entry exists with reason and what would change it |
| 2 | Daemon unlock: add `ow-vault` to `owd`; `vault.unlock` admin op (passphrase in frame, never logged or audited); idle auto-lock; attempt limit; `locked` error emitted by provider path when locked; `ow vault unlock` / `lock` | Not implemented | (a) request for a secret-backed topic while locked returns `locked`; (b) wrong passphrase leaves it locked and is rate-limited; (c) after unlock a fake provider can resolve a fake secret; (d) frame containing the fake passphrase never appears in audit file, stderr, stdout; (e) idle timeout locks and clears the key (observe via test hook, not by dumping memory) |
| 3 | Provider context: `fetch` gets a context exposing a request handle bound to the provider's allow-listed secret names, not strings; provider-to-secret binding from `config.toml` (0600, ours, unknown keys rejected, fail closed like `grants.toml`) with a name pattern per provider | Not implemented | (a) provider asking for a secret outside `secrets()` gets an error; (b) config pointing `github` at `gmail-app-pw` is refused; (c) world-writable config aborts startup |
| 4 | Fake-secret leak test for the daemon: start `owd` against a temp tree and fake provider, unlock with fake passphrase, run a full session, then **grep the full run logs, audit log, stdout, stderr, every file in the tree, `/proc/<pid>/cmdline` and `/proc/<pid>/environ` for the fake secret and the fake passphrase and assert absent** (vault file included: ciphertext only) | Present for `ow`; **not present for `owd`** | Test passes; temporarily making the fake provider `eprintln!` the secret makes it fail (mutation check, run once and record) |
| 5 | Egress wrapper with a local fake HTTPS server in tests: host allow-list, https only, no redirects, size cap, timeouts, header injection, no proxy env, no IP literals | Not implemented | (a) request to non-listed host fails; (b) 302 to a listed or unlisted host is not followed and the second server never sees the Authorization header; (c) response over cap is rejected; (d) timeout; (e) fake server records headers: token appears only in `Authorization`, never in URL, body or other headers; (f) with `HTTPS_PROXY` set the proxy receives nothing |
| 6 | Dependency decision for HTTP+TLS client: maintenance check, `cargo deny`, `cargo audit`, record in `DECISIONS.md`; check whether the library logs or `Debug`-prints headers; zeroize header copies where the API allows | Not started | Decision entry; `cargo deny check` and `cargo audit` pass; a test sets library debug logging and asserts the fake token is absent from output; document the unzeroizable residue honestly |
| 7 | `github` provider: GraphQL counts for `open` and `review`, typed parse, declared fields only, interval floor and backoff, rate-limit header handling, error categories | Not implemented | Fixture-driven tests against the local fake server: normal response gives `{open,review}`; 401 gives `unauthorized`; 403 with rate-limit headers gives `rate_limited` with reset; 403 permission gives `insufficient`; 5xx gives stale value; malformed JSON gives `format`; the 48 KiB cap holds; extra fields in upstream JSON never reach the widget |
| 8 | Purge on removal/rotation: `ow vault set|remove` followed by daemon reload (SIGHUP or `vault.reload`) drops the old secret and every cache entry derived from it | Not implemented | After remove, `snapshot` of `github.prs` returns `no_credential`, not the cached counts; after overwrite, the next fetch uses the new token (observed at the fake server) |
| 9 | Protocol: add a `state` category (locked, no_credential, unauthorized, expired_soon, insufficient, rate_limited, offline, unavailable) plus optional `until` epoch to snapshot/push; bump or keep `PROTOCOL` per policy | Not implemented | Parser tests and fuzz target for the new field; schema/doc updated; no state ever contains a string from upstream |
| 10 | `ow` input: passphrase from `/dev/tty`, value from stdin when stdin is a pipe (so a token can be piped without a shell variable) | Not implemented (both from stdin today) | pty or file-descriptor test: with no tty the command fails clearly; with a tty passphrase and piped value it stores; extra argv still rejected; the pipe content is never echoed |
| 11 | `ow auth github` (help/test), `ow auth github test` through the daemon, expiry display in `ow status` | Not implemented | Output contains the recipe text and never prompts for the token; `test` prints category and days to expiry; output grep for fake token empty |
| 12 | `ow grants` list/add/revoke and the install-time prompt text generated from `widget.json` + catalog; SIGHUP | Not implemented (`grants.toml` by hand today) | Revoke removes data at next push; prompt text lists topic, fields and "never receives the token"; granting a topic the widget did not declare is refused |
| 13 | Widget identity (D-1): bridge-authenticated widget id (e.g. per-widget handle minted by the bridge) so the grant check is not self-claimed | Not designed | Second client with a different handle cannot read another widget's grant; documented limit for code widgets |
| 14 | Different-uid peer refusal integration test | Not implemented (open question 13) | Test under `unshare`/container refuses a foreign uid |
| 15 | Web repo: auth schema in `providers.json`, `widget.schema.json`, `check-providers.ts`, fixture fix (see 6 and 7) | Not implemented | `bun test` / gates: fixture with `capabilities.network: []` passes policy; widget claiming `auth` kind not in the catalog fails; widget with a personal provider plus any network/files/spawn fails; code tier + personal provider fails |
| 16 | Installed-unit verification of `owd.service` with the network allowance (the unit already permits inet/inet6; `IPAddressDeny` is intentionally unset); consider `IPAddressAllow` for GitHub is not practical (CDN addresses), so rely on the wrapper | Not verified as an installed unit | `systemd-analyze --user security owd`; run as installed unit against throwaway dirs |
| 17 | GitHub App device flow in `ow auth github login` plus daemon refresh/rotation with atomic vault write | Not designed | Fake device-flow server: respects `interval` and `slow_down`; stops at expiry; refresh pair persisted atomically (kill between steps and recover); no client secret anywhere in tree (secret scanner and a grep for `client_secret`) |
| 18 | Keyring/TPM unlock (spike S4) | Spike unrun | Per spike plan |
| 19 | External review of vault + protocol + egress wrapper | Not done | Release gate before public beta (already in HANDOFF) |

Items 2 to 8 are the minimum for a working PAT-backed GitHub widget. Items 9 to 12 make it usable. 13 and 14 harden it. 17 onward is the later work.

---

## 6. Generalisation: provider auth schema

### 6.1 Today

`registry/providers.json` has `auth: "none" | "vault"` per provider; widgets declare `providers[].auth` as `none | vault` and the build checks equality with the catalog. This says only "a vault credential is involved". It cannot express kind (API key, bearer, OAuth), what the user must create, which permissions are expected, how many secrets, or where the credential is sent.

### 6.2 Proposed catalog shape (draft v1; additive, keeps `auth` readable by old code)

```jsonc
"github": {
  "class": "personal",
  "auth": "vault",                         // unchanged: coarse flag for existing gates
  "credentials": [                          // new, ordered by preference
    {
      "kind": "bearer",                     // bearer | api-key | basic | oauth-device
      "label": "Fine-grained personal access token",
      "secretName": "^github-[a-z0-9-]{1,40}$",   // pattern the user's vault name must match
      "send": { "header": "Authorization", "scheme": "Bearer", "host": "api.github.com" },
      "permissions": ["Pull requests: read", "Metadata: read"],   // plain text shown to the user
      "recommendedExpiryDays": 90,
      "setupUrl": "https://github.com/settings/personal-access-tokens/new",
      "warnOnPrefix": ["ghp_"],             // classic token prefix: show an over-scope warning
      "expiryHeader": null                   // name only if verified
    },
    {
      "kind": "oauth-device",
      "label": "GitHub App device flow",
      "clientId": "<public id, owned by Dexter, not a secret>",
      "deviceAuthUrl": "https://github.com/login/device/code",
      "tokenUrl": "https://github.com/login/oauth/access_token",
      "verificationUrlHost": "github.com",
      "refresh": true,
      "status": "planned"                    // planned | supported
    }
  ],
  "egress": ["api.github.com"]               // also the only hosts the credential may be sent to
}
```

Additional kinds, same pattern:
- `api-key`: header name or query param name (prefer header; a key in a URL leaks into logs and proxies; refuse query-param credentials unless the provider has no alternative and then mark it high-risk), host, pattern for the secret name. Example: an AI-usage billing API.
- `basic`: user/password pair as two vault records or one `user:pass` record (decide Q7); used only for local CalDAV style hosts, which conflicts with the "no private hostnames" listing rule for widget network and needs a separate non-widget path because these are provider egress, not widget capabilities.
- `oauth-device`: client_id, endpoints, scopes as plain text, polling rules, refresh flag. Never a `clientSecret` field. The schema should list `clientSecret` as a forbidden property (`additionalProperties: false` plus a gate test) so nobody adds one.
- `oauth-pkce-loopback` (Gmail "BYO client", calendar): user supplies their own client_id/secret as vault records; the project never ships one (matches `PROJECT_BRIEF.md`); needs a loopback listener, which is a new local-network capability for `owd`. Defer; mail is "not available in v1" per `check-policy.ts`.

The catalog is first-party only (`check-providers.ts`: "only first-party providers exist"), so catalog entries are reviewed code, not user input. The daemon should load the same facts from compiled provider metadata and a test should assert the web catalog and the Rust provider agree (the same drift problem the `serving` list exists to catch).

### 6.3 How a widget manifest and listing state it

Widget (`widget.json`), target shape:

```json
"providers": [
  { "name": "github", "topics": ["github.prs"], "fields": ["open", "review"], "auth": "vault" }
],
"capabilities": { "network": [], "files": [], "spawn": [] }
```

- `auth` stays a coarse echo that must equal the catalog; the widget does not choose the credential kind. This keeps the widget unable to ask for a more privileged credential than the provider defines.
- `capabilities.network` must be empty for a widget using a personal provider (already enforced for non-fixtures).
- A widget never lists a secret name, scope or URL. (`PROJECT_BRIEF.md` already states this.)

Listing / site (access summary, `registry/README.md` step 7 "Generate the plain-language access summary ... secrets"): derive rows from the catalog, not from the widget:

```
Data       github.prs: open, review (two numbers)
Credential your GitHub token in the vault (fine-grained, read-only; Pull requests: read)
Network    the daemon contacts api.github.com for you; this widget has no network of its own
Files      none        Programs  none
Never      this widget never receives your token
```

Trust badge rules unchanged: spec tier only for personal providers.

### 6.4 Open questions for Dexter (design level)

See section 8 for the top five; the full list is in 8.2.

---

## 7. Documentation and schema consistency (list only; no edits made)

Code repo (`omarchy-widgets/`):

| File | Change needed |
|---|---|
| `docs/HANDOFF.md` ("Left" item 4: "GitHub device flow first") | Reorder: fine-grained PAT first, device flow via GitHub App second |
| `docs/BUILD_PLAN.md` line 102 ("GitHub (device flow)") | Same |
| `docs/DAEMON_SPEC.md` section 8 provider list ("`github` (device flow ...)") and section 12 step 6 | Same; add "provider receives a request handle, not the token" and the context argument to the trait sketch; reflect the unlock design chosen |
| `docs/DAEMON_SPEC.md` sections 5 and 7 | Add `vault.unlock`/`vault.reload` (or the chosen mechanism) to admin ops; note `secret value from stdin only` also applies to unlock; state D-1 outcome |
| `docs/DAEMON_SPEC.md` section 9 | Document the `state` category (section 3.6) and what `ow status` will show |
| `docs/PROJECT_BRIEF.md` section 9 table row github ("device flow or fine-grained token") and provider authentication paragraph | Name the order and the "no OAuth App" reasoning; add the over-scoping caveat |
| `docs/PROJECT_BRIEF.md` lines 6, 188 (keyring default) | Already has a reality note; make the unlock requirement explicit |
| `docs/THREAT_MODEL` (`docs/threat-model.md`) | Add rows: over-scoped token, phishing prompts, egress/redirect leak, in-process provider compromise; state that unlock makes the daemon usable by same-uid processes within grant limits |
| `docs/VAULT_FORMAT.md` | Add daemon-side use: key lifetime in `owd`, write path from the daemon (needed for refresh), reload semantics; open question about an `open_with_key` API |
| `docs/DECISIONS.md` | Append: auth order decision, unlock mechanism, HTTP/TLS dependency decision, D-1 resolution |
| `docs/OPEN_QUESTIONS.md` | Add Q1 to Q8 from section 8.2 |
| `README.md` / `SECURITY.md` | Check that no sentence implies the daemon already talks to the network |
| `CLAUDE.md` "Current phase" | Update once item 2 or 7 starts |
| `docs/DECISIONS.md` pending D-1 | Its resolution now blocks the first credential feature, not only admin ops |

Web repo (`omarchy-widgets-web/`):

| File | Change needed |
|---|---|
| `registry/providers.json` | Add `credentials` (6.2); add `serving` for github when the daemon serves it; keep `auth` for compatibility |
| `registry/schema/widget.schema.json` | The `auth` property still says "Draft addition"; either keep as echo or extend enum if a kind is ever widget-selectable (recommendation: keep `none|vault`); remove the "draft" comment when decided |
| `registry/fixtures/io.github.dexterhere.concept-github-activity/widget.json` | `capabilities.network` should be `[]`; today it contradicts `check-policy.ts` rules and `PROJECT_BRIEF.md` |
| `scripts/gate/check-providers.ts` | Extend the `Catalog` type with `credentials`; keep the equality check on `auth`; add a check that no catalog credential contains a `clientSecret` property |
| `scripts/gate/check-policy.ts` | No logic change needed for this plan. Consider adding a test case for a personal provider with and without network so the fixture drift is caught (fixtures bypass the gates today) |
| `scripts/build-registry.ts` (access summary) | Derive network row from the provider's `egress` for vault-backed providers; say "the daemon contacts ... for you" |
| `registry/README.md` | Document credential kinds, the "no client secret" rule, and that fixtures are not gated (so fixtures must still follow policy) |
| Emulator `src/lib/emulator/terminal.ts` | Its help text lists `ow vault status`; keep in sync when `ow auth`/`vault unlock` land; make sure no emulator path ever suggests typing a token on the website (the site must never collect a token) |
| `docs/HANDOFF.md` (web) | Mention the auth schema work item |

---

## 8. Decisions for Dexter

### 8.1 Top five

1. **First credential type.** Accept fine-grained PAT first, GitHub App device flow second, no OAuth App, no `gh` reuse? (This overrules "device flow first" in three docs.)
2. **How the daemon gets the key.** Passphrase sent to `owd` once per session via `ow vault unlock` with idle timeout (recommended, smallest), versus waiting for kernel keyring/Secret Service/TPM (spike S4). Accept that the user unlocks after each login.
3. **D-1 for the first network feature.** Is a self-claimed widget id acceptable for the first PAT-backed release if the docs say it plainly, or must widget authentication (item 13) land before any personal provider ships?
4. **Where the provider-to-secret binding lives and who can change it.** `config.toml` 0600 with per-provider name patterns (recommended) versus a fixed secret name per provider. This decides whether multiple accounts are possible.
5. **GitHub App ownership.** Are you willing to own a GitHub App (name, logo, device flow on, client_id in source, takedown responsibility)? It is tied to the unresolved project name (D-3), so it should wait for that.

### 8.2 Full list

- Q1 Unlock mechanism (above).
- Q2 Minimum D-1 outcome (above).
- Q3 HTTP+TLS client dependency: acceptable to add a blocking client with rustls, with the extra audit surface? Or build on a smaller primitive? (Needs the maintenance check from item 6.)
- Q4 Honour proxy environment variables? Recommended: no by default, opt-in in `config.toml`.
- Q5 Daemon-enforced minimum poll interval for GitHub (recommended: 120 s default, 60 s floor) regardless of the widget's `interval` setting.
- Q6 One token per provider (v1) versus multiple named accounts via a `params.account`; organisation tokens and SSO approval.
- Q7 Secrets with more than one part (basic auth, client id + secret for BYO OAuth): one record with a delimiter, or two records?
- Q8 Should the daemon ever write the vault (needed for refresh tokens)? It raises the stakes of a daemon bug and requires the daemon to hold the key long enough to rewrite.
- Q9 Does the Widget Center credential dialog ship in v1 or stay terminal-only until a separate trusted prompt process exists?
- Q10 Is a classic PAT accepted at all (silently, or refused by prefix `ghp_`)? Refusing is stricter but annoying.
- Q11 Telemetry-free expiry reporting: reading the expiry header is a local read; any "your token is expiring" notification must be local too. Confirm.
- Q12 GitHub Enterprise Server: confirm it is out of scope for v1.

---

## Appendix A. Verification performed

- Read: `CLAUDE.md`, `docs/PROJECT_BRIEF.md`, `docs/DAEMON_SPEC.md`, `docs/VAULT_FORMAT.md`, `docs/DECISIONS.md`, `docs/OPEN_QUESTIONS.md`, `docs/threat-model.md`, `docs/HANDOFF.md`.
- Read in code: `crates/ow-core/src/provider.rs`, `crates/ow-core/src/harden.rs`, `crates/ow/src/main.rs`, `crates/ow/src/vault_cmd.rs`, `crates/ow/src/input.rs` (structure), `crates/ow-vault/src/store.rs` (`set`, `remove`, `names`, `get`), `crates/ow/tests/vault_cli.rs` (test names and the leak test), the `Cargo.toml` of `owd`, `ow`, `ow-providers`.
- Grepped: no HTTP/TLS crate and no `ow_vault` usage in `owd`; `egress_hosts`/`secrets` have no callers; `ErrorCode::Locked` is defined only.
- Read in the web repo: `registry/providers.json`, `registry/schema/widget.schema.json`, `scripts/gate/check-policy.ts`, `scripts/gate/check-providers.ts`, `scripts/gate/scan-secrets.ts` (head), the concept-github fixture files, `registry/README.md` gate paragraphs.
- Not done: no tests were run, no network access, no GitHub documentation was fetched (all GitHub-specific facts marked **(verify)**), `hub.rs`/`grants.rs`/`audit.rs` were relied on through the decision log and the spec status notes plus the test and function names found by grep, not read line by line.
