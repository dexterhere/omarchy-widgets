# Releasing the plugin mirror

`omarchy plugin add <git-url>` clones the default branch of a repo whose root is the plugin (SPIKES S7). The plugin lives in `plugin/` of this repo, so a generated mirror repo, `dexterhere/omarchy-widget-shell`, holds the runtime files only. Everything here is done by hand by the maintainer; nothing is automated and no key or secret exists for it (DECISIONS PLUGIN-MIRROR).

What the mirror holds: `plugin/*.qml`, `*.js`, `qmldir`, `manifest.json`, plus `README.md` (from `plugin/MIRROR_README.md`), `LICENSE` and `MIRROR.md` (source commit and tag). Not `plugin/tests/`, `plugin/README.md` or scratch. The script refuses symlinks, files over 1 MiB, unlisted file names, a dirty code tree, an unknown ref, and a destination that is not empty-or-a-clean-clone.

## One-time setup (human only; do not run from an agent)

```
gh repo create dexterhere/omarchy-widget-shell --public --description "Generated mirror of the Omarchy widget layer plugin. Pull requests go to dexterhere/omarchy-widgets." --disable-issues --disable-wiki
git clone git@github.com:dexterhere/omarchy-widget-shell.git ~/src/omarchy-widget-shell
cd ~/src/omarchy-widget-shell && git checkout -b main && git commit --allow-empty -m "init" && git push -u origin main
gh repo edit dexterhere/omarchy-widget-shell --default-branch main
```

Branch protection on `main` (applies to admins too, so a slip needs a deliberate unprotect). A solo maintainer pushes released commits directly, so no pull request is required; force pushes and deletion are blocked:

```
gh api -X PUT repos/dexterhere/omarchy-widget-shell/branches/main/protection --input - <<'JSON'
{"required_status_checks":null,"enforce_admins":true,"required_pull_request_reviews":null,
 "restrictions":null,"allow_force_pushes":false,"allow_deletions":false,"required_linear_history":true}
JSON
```

Tag ruleset: `v*` tags cannot be moved or deleted once pushed (a retraction is a new tag, see below):

```
gh api -X POST repos/dexterhere/omarchy-widget-shell/rulesets --input - <<'JSON'
{"name":"release tags","target":"tag","enforcement":"active",
 "conditions":{"ref_name":{"include":["refs/tags/v*"],"exclude":[]}},
 "rules":[{"type":"deletion"},{"type":"non_fast_forward"},{"type":"update"}]}
JSON
```

Enable private vulnerability reporting there too, or leave issues off and rely on the pointers in the mirror README. Check the result with `gh api repos/dexterhere/omarchy-widget-shell/branches/main/protection`.

## Release steps

1. In the code repo on `main`: everything for the release merged, `plugin/tests/run-all.sh` green, `docs/RELEASE_CHECKLIST.md` updated, `git status` clean.
2. Tag the code repo: `git tag -s vX.Y.Z -m vX.Y.Z` and push the tag (the maintainer decides when; see the other release steps in `RELEASE_CHECKLIST.md` G).
3. Run the export from that tag into the mirror clone (it reads git objects of the tag, not your working tree):
   `scripts/mirror-plugin.sh ~/src/omarchy-widget-shell vX.Y.Z`
   It validates the export with `omarchy plugin validate`, lints it with the plugin static check from the same tag, prints the file list with sha256 sums and the commands for step 5.
4. Review in the mirror clone: `git status && git diff`. Check that `MIRROR.md` shows the tag and the commit you expect, that no file appears that should not ship, and read the changed QML. This is the only review users get before the code runs unsandboxed in their shell.
5. Commit and tag the mirror with the same tag, then push the branch and the tag:
   `git add -A && git commit -m "release vX.Y.Z"` then `git tag -s vX.Y.Z -m vX.Y.Z` then `git push origin main vX.Y.Z`
6. Verify: `git clone https://github.com/dexterhere/omarchy-widget-shell /tmp/x` and `omarchy plugin validate /tmp/x` (read-only). Install it once on a spare account or with the maintainer present (`RELEASE_CHECKLIST.md` E) before telling anyone to use it.

## How users update

`omarchy plugin update` runs `git fetch origin HEAD` and fast-forwards (SPIKES S7). So the mirror's default branch is what every user gets next. It must only ever hold released, reviewed commits: never push to it between releases, and never from a branch other than a release tag. After an update the shell must be restarted to load new code (`keepLoaded`).

## A bad release shipped

The history cannot be rewritten (protection) and the tags cannot be moved (ruleset), by design: users who fast-forwarded would otherwise diverge. Retract by moving forward:

1. Fix or revert in the code repo, merge, tag `vX.Y.(Z+1)`, and run the release steps. The next `omarchy plugin update` repairs everyone.
2. Same day: say so in the code repo (a security advisory if it is a security problem, a release note otherwise) and edit the bad release's notes to point at the fix.
3. If the bad release must stop spreading before a fix exists, push a revert commit to the mirror's `main` by hand (the code repo's previous tag's export, committed with message `revert vX.Y.Z`), so new installs and updates get the last good code. This is the only case where the mirror gets a commit that is not a fresh tag.
4. Only if the content is dangerous (leaked secret, malicious code): temporarily lift the protection (`enforce_admins` off), rewrite or delete, restore the protection, and rotate whatever leaked. Tell users to run `omarchy plugin remove` and add again, since a force-pushed history will not fast-forward.
