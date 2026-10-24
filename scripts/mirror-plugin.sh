#!/usr/bin/env bash
# Export the plugin runtime files of a code-repo ref into DEST, ready to commit to the mirror repo.
# Usage: scripts/mirror-plugin.sh DEST [REF]     (REF defaults to HEAD, normally a v* tag)
# Reads only git objects of REF, never the working tree. Does not commit or push. See docs/PLUGIN_RELEASE.md.
set -euo pipefail
umask 022
export LC_ALL=C

SOURCE_REPO=https://github.com/dexterhere/omarchy-widgets
MIRROR_REPO=https://github.com/dexterhere/omarchy-widget-shell
MAX_BYTES=1048576

die() { echo "mirror-plugin: $*" >&2; exit 1; }

[ $# -ge 1 ] && [ $# -le 2 ] || die "usage: $0 DEST [REF]"
dest_arg=$1
ref=${2:-HEAD}
case "$ref" in -*) die "a ref may not start with a dash: $ref" ;; esac

root=$(git -C "$(dirname "${BASH_SOURCE[0]}")/.." rev-parse --show-toplevel) || die "not inside the code repo"
cd "$root"

# 1. the ref and the tree
[ -z "$(git status --porcelain)" ] || die "code repo working tree is dirty; commit or stash first"
sha=$(git rev-parse --verify --quiet "${ref}^{commit}") || die "unknown ref: $ref"
tag=$(git describe --exact-match --tags "$sha" 2>/dev/null || true)
[ -n "$tag" ] || echo "warning: $sha has no tag; release builds should come from a v* tag" >&2

# 2. the destination: new, empty, or a clean git clone (never the code repo)
mkdir -p -- "$dest_arg"
dest=$(cd -- "$dest_arg" && pwd -P)
[ "$dest" != "/" ] && [ "$dest" != "$HOME" ] || die "refusing destination $dest"
if [ -n "$(ls -A -- "$dest")" ]; then
  top=$(git -C "$dest" rev-parse --show-toplevel 2>/dev/null || true)
  [ "$top" = "$dest" ] || die "destination is not empty and is not the root of a git clone: $dest"
  [ "$dest" != "$(pwd -P)" ] || die "destination is the code repo"
  [ -z "$(git -C "$dest" status --porcelain)" ] || die "destination clone is not clean: $dest"
fi

# 3. select files from the ref's tree: plugin/ minus tests/, scratch and dev docs
want=()
while IFS= read -r -d '' rec; do
  meta=${rec%%$'\t'*}; path=${rec#*$'\t'}
  read -r mode _ _ size <<<"$meta"
  rel=${path#plugin/}
  case "$rel" in tests/*|relative/*|README.md|MIRROR_README.md|__pycache__/*) continue ;; esac
  [[ $path =~ ^[A-Za-z0-9._/-]+$ ]] || die "unexpected file name: $path"
  [[ $rel =~ ^([A-Za-z0-9]+\.(qml|js)|qmldir|manifest\.json)$ ]] || die "unexpected file in plugin/ (add to the exclude list or the allow list): $rel"
  [ "$mode" != 120000 ] || die "symlink refused: $path"
  [ "$mode" = 100644 ] || [ "$mode" = 100755 ] || die "unsupported file mode $mode: $path"
  [ "$size" -le "$MAX_BYTES" ] || die "file over 1 MiB refused: $path ($size bytes)"
  want+=("$rel:$mode")
done < <(git ls-tree -r -l -z "$sha" -- plugin)
[ ${#want[@]} -gt 0 ] || die "no plugin files at $sha"

stage=$(mktemp -d -p "${TMPDIR:-/tmp}" ow-mirror.XXXX)
trap 'rm -rf -- "$stage"' EXIT
mkdir "$stage/out" "$stage/lint"

put() { # put MODE NAME CONTENT-FROM-GIT-SPEC
  git show "$3" >"$stage/out/$2"; chmod "$1" "$stage/out/$2"
}
for e in "${want[@]}"; do
  rel=${e%%:*}; mode=${e##*:}
  put "$([ "$mode" = 100755 ] && echo 0755 || echo 0644)" "$rel" "$sha:plugin/$rel"
done
put 0644 README.md "$sha:plugin/MIRROR_README.md"
put 0644 LICENSE "$sha:LICENSE"
cat >"$stage/out/MIRROR.md" <<EOF
# Mirror record

This repository is generated. Do not edit it by hand and do not open pull requests here.

- Source repository: $SOURCE_REPO
- Source commit: $sha
- Source tag: ${tag:-none}
- Mirror repository: $MIRROR_REPO
- Built by: scripts/mirror-plugin.sh (see docs/PLUGIN_RELEASE.md in the source repository)
EOF
chmod 0644 "$stage/out/MIRROR.md"

while IFS= read -r -d '' f; do
  [ -L "$f" ] && die "symlink in export: $f"
  [ "$(stat -c %s "$f")" -le "$MAX_BYTES" ] || die "file over 1 MiB refused: $f"
done < <(find "$stage/out" -type f -print0 -o -type l -print0)

# 4. validate and lint exactly what will ship (the linter comes from the same ref)
command -v omarchy >/dev/null || die "omarchy not found; cannot validate"
omarchy plugin validate "$stage/out" || die "omarchy plugin validate failed on the export"
git show "$sha:plugin/tests/static_check.py" >"$stage/lint/static_check.py" || die "no static_check.py at $sha"
python3 -I "$stage/lint/static_check.py" --selftest >/dev/null || die "static check self-test failed"
python3 -I "$stage/lint/static_check.py" "$stage/out" || die "static safety check failed on the export"

# 5. replace the destination contents (a clean clone, so git shows every change)
find "$dest" -mindepth 1 -maxdepth 1 ! -name .git -exec rm -rf -- {} +
cp -a -- "$stage/out/." "$dest/"
omarchy plugin validate "$dest" >/dev/null || die "omarchy plugin validate failed on $dest"

# 6. report
echo "== files in $dest ($sha, tag ${tag:-none})"
(cd "$dest" && find . -path ./.git -prune -o -type f -printf '%P\n' | sort | while read -r f; do
  printf '%s  %s  %s\n' "$(stat -c %a "$f")" "$(sha256sum "$f" | cut -d' ' -f1)" "$f"; done)
echo "== next steps (by hand, after reading the diff)"
cat <<EOF
cd '$dest'
git status && git diff
git add -A && git commit -m "release ${tag:-$sha}"
git tag -s ${tag:-vX.Y.Z} -m "${tag:-vX.Y.Z}"      # same tag as the code repo
git push origin main ${tag:-vX.Y.Z}
EOF
