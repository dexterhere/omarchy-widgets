#!/usr/bin/env bash
# Tests for scripts/mirror-plugin.sh against a throwaway git repo in /tmp. No framework.
set -euo pipefail
here=$(cd "$(dirname "$0")/../.." && pwd)
T=$(mktemp -d -p /tmp ow-mirror.XXXX)
trap 'rm -rf -- "$T"' EXIT
pass=0; fail=0
ok()  { pass=$((pass+1)); echo "ok   $1"; }
bad() { fail=$((fail+1)); echo "FAIL $1"; }
check() { local n=$1; shift; if "$@"; then ok "$n"; else bad "$n"; fi; }
refuses() { local n=$1 re=$2; shift 2; local out; if out=$("$@" 2>&1); then bad "$n (succeeded)"; elif grep -q -- "$re" <<<"$out"; then ok "$n"; else bad "$n (wrong error: $out)"; fi; }

# stub omarchy: validate = manifest.json must exist
mkdir "$T/bin"
printf '#!/bin/sh\n[ "$1 $2" = "plugin validate" ] && [ -f "$3/manifest.json" ]\n' >"$T/bin/omarchy"; chmod +x "$T/bin/omarchy"
export PATH="$T/bin:$PATH"

R=$T/code; mkdir -p "$R/scripts" "$R/plugin/tests"
cp "$here/scripts/mirror-plugin.sh" "$R/scripts/"
echo '{"id":"x"}' >"$R/plugin/manifest.json"
echo 'Item {}' >"$R/plugin/Service.qml"; echo 'singleton Theme 1.0 Theme.qml' >"$R/plugin/qmldir"
echo dev notes >"$R/plugin/README.md"; echo mirror readme >"$R/plugin/MIRROR_README.md"
echo secret-test >"$R/plugin/tests/t.sh"; echo MIT >"$R/LICENSE"
# fixture linter: fails when a file contains BAD
cat >"$R/plugin/tests/static_check.py" <<'P'
import sys, os
d = sys.argv[-1] if not sys.argv[-1].startswith("--") else "."
if "--selftest" in sys.argv: sys.exit(0)
sys.exit(1 if any("BAD" in open(os.path.join(d, f)).read() for f in os.listdir(d) if f.endswith(".qml")) else 0)
P
git -C "$R" init -q -b main; git -C "$R" config user.name t; git -C "$R" config user.email t@example.invalid
git -C "$R" add -A; git -C "$R" commit -qm init; git -C "$R" tag v9.9.9
M=$R/scripts/mirror-plugin.sh
manifest() { (cd "$1" && find . -path ./.git -prune -o -type f -printf '%P %m\n' | sort | while read -r f m; do echo "$m $(sha256sum "$f")"; done); }

"$M" "$T/d1" v9.9.9 >"$T/o1" 2>&1 || { cat "$T/o1"; bad "first run"; }
check "tests/ excluded" test ! -e "$T/d1/tests"
check "dev README replaced, MIRROR_README not copied" bash -c "[ \"\$(cat $T/d1/README.md)\" = 'mirror readme' ] && [ ! -e $T/d1/MIRROR_README.md ]"
check "LICENSE present" test -f "$T/d1/LICENSE"
check "MIRROR.md has sha and tag" bash -c "grep -q $(git -C "$R" rev-parse HEAD) $T/d1/MIRROR.md && grep -q 'tag: v9.9.9' $T/d1/MIRROR.md"
check "runtime files present" test -f "$T/d1/manifest.json" -a -f "$T/d1/Service.qml" -a -f "$T/d1/qmldir"
"$M" "$T/d2" v9.9.9 >/dev/null 2>&1
check "deterministic" test "$(manifest "$T/d1")" = "$(manifest "$T/d2")"
# idempotent on a clean clone
git -C "$T/d1" init -q -b main; git -C "$T/d1" add -A; git -C "$T/d1" -c user.name=t -c user.email=t@e.invalid commit -qm one
"$M" "$T/d1" v9.9.9 >/dev/null 2>&1
check "idempotent: clone stays clean" test -z "$(git -C "$T/d1" status --porcelain)"
# dirty destination clone, non-clone non-empty destination
echo x >"$T/d1/junk"; refuses "dirty destination clone refused" "not clean" "$M" "$T/d1" v9.9.9; rm "$T/d1/junk"
mkdir "$T/plain"; echo x >"$T/plain/f"; refuses "non-empty non-clone refused" "not the root of a git clone" "$M" "$T/plain" v9.9.9
refuses "unknown ref refused" "unknown ref" "$M" "$T/d3" v0.0.0
# linter and validator gate
git -C "$R" checkout -q -b bad; echo BAD >>"$R/plugin/Service.qml"; git -C "$R" commit -qam bad
refuses "static check failure refused" "static safety check failed" "$M" "$T/d4" HEAD
git -C "$R" checkout -q main
# dirty tree
echo x >>"$R/plugin/Service.qml"; refuses "dirty tree refused" "dirty" "$M" "$T/d5" v9.9.9; git -C "$R" checkout -q -- plugin
echo x >"$R/untracked"; refuses "untracked file refused" "dirty" "$M" "$T/d5" v9.9.9; rm "$R/untracked"
# symlink
git -C "$R" checkout -q -b link; ln -s /etc/passwd "$R/plugin/Evil.qml"; git -C "$R" add -A; git -C "$R" commit -qm link
refuses "symlink refused" "symlink refused" "$M" "$T/d6" HEAD
git -C "$R" checkout -q main
# oversize
git -C "$R" checkout -q -b big; head -c 1048577 /dev/zero | tr '\0' a >"$R/plugin/Big.js"; git -C "$R" add -A; git -C "$R" commit -qm big
refuses "oversize refused" "over 1 MiB" "$M" "$T/d7" HEAD
git -C "$R" checkout -q main
# unexpected file
git -C "$R" checkout -q -b odd; echo x >"$R/plugin/notes.txt"; git -C "$R" add -A; git -C "$R" commit -qm odd
refuses "unlisted file refused" "unexpected file" "$M" "$T/d8" HEAD
git -C "$R" checkout -q main
check "refusals left no destination files" bash -c "[ -z \"\$(find $T/d3 $T/d4 $T/d6 $T/d7 $T/d8 -type f 2>/dev/null)\" ]"

echo "RESULT: $pass passed, $fail failed"; [ "$fail" -eq 0 ]
