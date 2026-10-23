# `ow install`, `ow list`, `ow remove` (spec widgets): design

Status: design written before the code (2026-10-23). The implementation follows it; where the code ends up different, `docs/DECISIONS.md` (INSTALL entry) says so. This is the path by which a third party's files reach a user's machine, so the document states what is checked, what is only trusted, and what is not done.

Scope: **spec widgets only** (`widget.json` + `layout.json` + `manifest.json`, JSON, no code). A widget whose own `widget.json` says `tier: "code"` is refused. Nothing fetched is ever executed, parsed as anything but JSON, or passed to a shell. No `git clone`, no process spawn, no shell.

## 1. Source of truth

1. The **registry index** published by the website: `https://omarchy-widgets-web.vercel.app/registry.json` (the same `registry/dist/index.json` the site builds; `schemaVersion` 1). It carries per widget: `id`, `repo`, `commit`, `status`, `tier`, `installable`, and inline copies of `providers`, `capabilities` and `layout`.
2. The **widget files** `manifest.json`, `widget.json`, `layout.json` are fetched from `raw.githubusercontent.com/<owner>/<repo>/<commit>/` at exactly the index's pinned commit.

Rules shared with the registry build (`scripts/build-registry.ts`): https only, no redirects (any 3xx is an error), 256 KB cap per file, JSON only, no credentials, no cookies, no proxy variables. All of it is enforced by `ow-egress`, the only crate that may touch the network (AUTH-EGRESS). The index itself is capped at 2 MiB (it is about 90 KB for seven widgets and grows with the catalog; the 256 KB cap would break it at about twenty widgets).

The index host is a compiled-in constant (`DEFAULT_REGISTRY`). It can be changed for one run by `--registry https://<host>` and nowhere else: **no environment variable, no config file**. The value must be a bare https origin (lowercase DNS name, no port, path, userinfo or query); `ow-egress` validates the host again. A non-default registry is announced in the output because it moves the trust root. Per call, the egress allow-list is exactly one host: the registry host for the index, `raw.githubusercontent.com` for files.

`repo` must be `https://github.com/<owner>/<repo>` with owner `[A-Za-z0-9-]{1,39}` and repo `[A-Za-z0-9._-]{1,100}` (not `.` or `..`). `commit` must be 40 lowercase hex characters: a short hash, uppercase, or the all-zero placeholder used by concept entries is refused (the placeholder says: not fetchable yet). An id under `io.github.<owner>.` must be listed from a repo owned by `<owner>` (same rule as the registry build).

## 2. Integrity: what is verified, what is trusted

Verified by `ow` before anything is written:

- The commit pin: files are requested at the 40-character SHA from the index, nothing else. The URL is built from validated parts only.
- Strict JSON: duplicate object keys are an error (a plain parse would silently keep the last one), nesting deeper than 32 is an error, invalid UTF-8, a BOM, or trailing bytes are errors, sizes are capped.
- Id equality: the requested id, the index entry, `manifest.json` `id` and `widget.json` `id` must be identical. The id must also match the schema pattern and the storage pattern `^[a-z0-9.-]{1,100}$` with no leading dot or dash and no `..`.
- `manifest.json`: required `id`, `name`, `version`, `description`, `license`, with the registry's bounds; `name` and `version` are plain text (no control, bidi or zero-width characters) because they are printed to the terminal. If the index carries `name` or `version`, they must equal the manifest's.
- `widget.json` against a Rust re-implementation of `registry/schema/widget.schema.json` (draft-0): required fields, `additionalProperties: false` at every level, patterns, enums, array and string limits, `schemaVersion` 0. Re-implemented by hand (no schema engine), so it can drift from the schema; the tests use the real widget files and the unit tests pin the limits. The schema is itself marked draft in the web repo.
- `layout.json` against a Rust re-implementation of `src/lib/registry/layout.ts` (node types, allowed keys per type, color roles, sizes, gaps, icon names, depth 8, 200 nodes, 20 children, text 120 characters plain, 64 KB). Every `bind` must name a topic and field the widget declares under `providers`. `sampleData` is only bounded (object, at most 16 properties), not shape-checked: it is for the web emulator and `ow` never uses it.
- Policy (`scripts/gate/check-policy.ts`, `check-providers.ts`): a spec widget declares **no** network, files or spawn (all three arrays empty). Every provider must exist in the **catalog snapshot** compiled into the binary (`crates/ow/data/providers.json`, a copy of the web repo's `registry/providers.json`); every topic must belong to that provider and every field to one of its topics; a declared `auth` must equal the catalog's; a topic may be declared once; mail providers are refused.
- Cross-check against the index (what the person saw on the website is what installs): if the index entry carries `providers`, `capabilities` or `layout`, the fetched file must be equal to it. A difference is refused, not merged.
- Per-file hashes when the index has them (see 2.2): sha256 of each fetched file must match.

Trusted, not verified:

- **The website's index.** It decides which repo and commit a name maps to. Someone who can change what `registry.json` says (the site's deployment, its DNS, or the registry repo's maintainers) can point an id at any commit. The maintainers' review (`lock.json`, `review/`) and CI gates happen on the website side; `ow` cannot see them.
- **GitHub** (`raw.githubusercontent.com`) to serve the bytes of a commit, and the git SHA-1 commit id to identify content. `ow` does not recompute git object hashes.
- **TLS roots.** `ow-egress` trusts the compiled-in Mozilla root bundle; there is no pinning. Any CA in it can issue for these hosts.
- **Reachability.** raw.githubusercontent.com serves a commit from any repo in the fork network. The registry build checks that the commit is on the default branch's history (via `api.github.com`). `ow` does **not** repeat that check (it would need a second host and the API rate limit); it relies on the index having been built with it.
- **No signatures.** Nothing is signed by anyone: not the index, not the widget, not the commit as far as `ow` checks.
- **Same-uid code.** Any process running as the user can change the installed files, `installed.json`, and `grants.toml` after the fact. The recorded hashes detect accidents and let `ow list` flag changed files; they are not tamper-proofing.

### 2.1 What the check does not mean

Passing these checks means: the files are well-formed, declare no network, files or program access, use only first-party providers and fields, and are the files at a pinned commit that the website's index named. It does not mean a person reviewed them. The tier from the index says how much review happened (`concept`, `community`, `spec`, `verified`, `core`), and `ow` prints it.

### 2.2 Proposed web-repo change: per-file hashes in the index

The index has no per-file hashes today. Proposal (web repo; listed in `docs/HANDOFF.md`, not edited here): `scripts/build-registry.ts` already holds the exact bytes it validated for each non-fixture listing; add to each `dist/index.json` widget entry

```json
"sha256": { "manifest.json": "<64 hex>", "widget.json": "<64 hex>", "layout.json": "<64 hex>" }
```

over the raw fetched bytes, and extend `listing`/`index` types. `ow` already verifies them **when present**: all three must be present for a spec widget, hex lowercase, and each must match. When absent, `ow` prints "file hashes: not provided by the index (not verified)". The hashes bind what `ow` downloads to what the build validated; they do not defend against a compromised index (which could publish its own hashes).

## 3. Status and tier gate

Index `status`: `planned`, `alpha`, `stable`, `removed`. Index `tier`: `concept`, `community`, `spec`, `verified`, `core`. Any other value is refused ("unknown", with the value shown sanitized and truncated).

- `alpha`, `stable`: installable.
- `planned`: refused unless `--allow-planned`. With the flag the output says: *this widget is a design concept; it will not show data until the needed provider exists*, and lists the topic fields the daemon does not serve yet (from the catalog's `serving` lists).
- `removed`: **always refused**, even with `--allow-planned`. The brief says `--allow-planned` lifts `planned` and `removed`; a takedown is a safety signal and the registry build never publishes `removed` entries, so a `removed` entry reaching the CLI is a stale or hostile index. This is a deliberate tightening (see DECISIONS INSTALL).
- Whatever the index says, a `widget.json` with `tier` other than `spec` is refused.

## 4. Consent

Before anything is written, `ow` prints a plain-language access summary generated from the validated `widget.json` plus the catalog snapshot (not from anything the widget wrote as prose):

- name, version, id, licence, repo, commit, status, tier; "this widget is JSON only, it contains no code";
- every topic it will read with the fields it may read, and which of them the daemon serves today;
- whether a vault credential is involved (catalog `auth: vault`), which hosts the **daemon** will contact for it, and: the widget never receives the credential, and the widget itself has no network, file or program access;
- a plain note that the daemon identifies widgets by the id they claim (D-1), so the grant is a guardrail against honest over-reading and not a barrier against another program of the same user;
- the exact grants file lines that will be added, the files that will be written, and whether this is an update (old commit to new commit, topics added/removed).

Then it requires the person to type `yes` (exactly, case-sensitive; anything else, EOF, or no terminal cancels). Stdin and stderr must be a terminal; with no terminal the command refuses before any network access.

`--yes` is not a bypass. It is accepted only together with `--accept-access <hash>`, where `<hash>` is the sha256 printed as "access hash" by a previous run (`--dry-run` or the consent screen). The hash covers the id, the commit and the exact grant entries (topics and fields). If the computed hash differs, nothing is installed. This allows a reviewed, scripted install of exactly the access that was read, and nothing broader. It is small (about 30 lines), so it is included.

`--dry-run`: do everything up to and including the summary and the hash, write nothing, ask nothing.

## 5. Storage

Layout (all under the daemon's data dir, `$XDG_DATA_HOME/owd`):

```
widgets/                      0700, checked with check_private_dir
widgets/.lock                 flock, one install/remove at a time
widgets/<id>/                 0700
widgets/<id>/manifest.json    0600, exact bytes fetched
widgets/<id>/widget.json      0600
widgets/<id>/layout.json      0600
widgets/<id>/installed.json   0600 {id, repo, commit, installedAt, status, tier, files:{name:sha256}, granted:[{topics,fields}]}
```

`<id>` is validated on every use (install, list, remove) against `^[a-z0-9.-]{1,100}$` with no leading dot or dash and no `..`. It is joined to the widgets dir only after validation, and no path is ever built from a fetched string except the id, repo and commit that were validated for their charset. Removal refuses a non-directory or symlink as the widget dir (it unlinks a symlink itself, never follows it) and deletes only below `widgets/<id>/`.

Install is staged: a `.stage-<random>` directory is created inside `widgets/` (same filesystem), files are written 0600 with `create_new`, fsynced, then the directory is renamed into place. An existing install of the same id is first renamed to `.old-<random>` and deleted after the whole operation succeeds; on any later failure it is renamed back. Leftover `.stage-*` and `.old-*` directories from a crash are removed at the start of the next install or remove.

**Grants.** An entry per provider of the widget is appended to the daemon's `grants.toml` (`$XDG_CONFIG_HOME/owd/grants.toml`, the format in DAEMON_SPEC section 6):

```toml
[[grant]]
widget = "io.github.dexterhere.clock"
topics = ["system.time"]
fields = ["hm", "hms", "date", "tz"]
approved_at = "2026-10-23T12:00:00Z"
```

The existing file is read with the daemon's own loader rules (no symlink, regular file, mode 0600, owned by us, 1 MiB cap); if it is invalid or insecure, `ow` stops and changes nothing. The edit is textual so other entries and comments survive: entries for the same widget id are removed (with the comment lines directly above them) and new ones appended. The result is then re-parsed with the daemon's parser and compared with the intended entry list; if the comparison fails, the file is rewritten from the parsed entries instead (comments are lost, nothing else). The file is replaced atomically (temp file 0600 in the same directory, fsync, rename, fsync of the directory) and stays 0600. Key: the widget id. Removing a widget removes every entry whose `widget` equals its id.

Order of commits: widget directory first, then grants. If the grants write fails, the directory change is undone. A crash between the two leaves files without grants (deny by default), and `ow list` shows "no grant".

**Reload.** A new admin op `grants.reload` (no payload) in `ow-proto` and `owd` makes the daemon re-read `grants.toml` (same function as SIGHUP; a failed reload fails closed). It follows the vault admin op rules: parsed before `widget`-claim checks, refused with `denied` and audited when the frame carries a `widget` key (null included), same-uid peers only, a small shared rate limit, an audit line (`admin`/`grants`/`reload`) with no payload. Parser tests and the `admin_parse` fuzz target are extended. If the daemon is not running (no socket, connection refused), `ow` says the grants apply at the next daemon start. If it answers with an error, `ow` says the daemon denies everything until the grants file is fixed. Signals are not used (the CLI would have to find the pid).

## 6. `ow list` and `ow remove`

- `ow list`: for each `widgets/<id>/installed.json` (read with a size cap and strict JSON): id, version, commit (12 characters), status, tier, installed time; and flags: `files changed since install` (sha256 mismatch), `no grant` (the grants file has no entry for it), `unreadable record`. Output is sanitized.
- `ow remove <id>`: remove grants for the id first (revocation matters most), reload the daemon, then rename the directory to `.old-*` and delete it. Idempotent: a missing widget and missing grants prints "not installed" and succeeds. Nothing outside `widgets/<id>/` and the one grants file is touched.

## 7. Failure and rollback

Nothing is written before consent. After consent: stage, then swap, then grants; each failure path undoes the earlier steps (see section 5). A failed network step, a validation failure, a hash mismatch, a declined prompt or a refused flag leaves the data and config directories as they were (only the empty `widgets/` directory and its lock file may be created).

## 8. Testing approach

The network is behind a small `Source` trait with two methods (fetch the index, fetch a file of a commit). Production code has one implementation, on `ow-egress`. Tests implement it in the test code with a directory and a map; **no test-only reader is compiled into the binary and no cargo feature exists**. Everything between the bytes and the disk is a pure function (parse index, select listing, validate manifest/widget/layout, build summary and hash, plan the writes, edit grants), unit-tested with hostile inputs. The orchestration (`install::run`) is exercised in-process from `crates/ow/tests` against a throwaway tree, and the real `ow` binary is exercised for the paths that do not need the network (usage errors, refusal without a terminal, `list`, idempotent `remove`, hostile ids). Not covered: a real TLS fetch of the index or of raw.githubusercontent.com in CI (one manual `--dry-run` against the live site is recorded in DECISIONS), a real terminal `yes` against the real binary with a live fetch.

## 9. Decisions this rests on

- INSTALL-1 index host is a compiled-in constant plus `--registry`, never an environment variable.
- INSTALL-2 raw.githubusercontent.com at the pinned commit, through `ow-egress` only; `ow` depends on `ow-egress` and no other network crate.
- INSTALL-3 spec widgets only; `removed` always refused; `planned` needs `--allow-planned`; unknown status/tier refused.
- INSTALL-4 consent by typing `yes` on a terminal; `--yes` only with `--accept-access <hash>`.
- INSTALL-5 `grants.reload` admin op instead of signals.
- INSTALL-6 sha256 via the `sha2` crate (RustCrypto, same family as the existing AEAD crates) for file hashes and the access hash; no hand-written hash.
- INSTALL-7 catalog snapshot shipped in the binary; drift from the website's `providers.json` is a release-time check (listed as an open question).

## 10. Residual risks (also in DECISIONS)

Trust in the website index and in GitHub; no signatures; no per-file hashes until the web change lands; the schema and layout checks are a re-implementation that can drift; same-uid processes can edit installed files and grants; the grant key is a self-claimed widget id (D-1); no default-branch reachability check in the CLI; a stale catalog snapshot in an old binary rejects newer providers (fails closed).
