#!/usr/bin/env bash
# Tests for packaging/install-local.sh. Everything happens under a fresh /tmp directory with a
# fake HOME, XDG_* and PREFIX. The real home is never read or written, and a shim in front of
# PATH records any attempt to run systemctl (there must be none).
set -uo pipefail
here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)
script=$here/../install-local.sh
unit_src=$here/../owd.service
T=$(mktemp -d -p /tmp ow-pkg.XXXX)
case $T in /tmp/ow-pkg.*) ;; *) echo "bad temp dir" >&2; exit 2 ;; esac
trap 'rm -rf -- "$T"' EXIT
pass=0 fail=0
ok() { pass=$((pass + 1)); echo "PASS $1"; }
bad() { fail=$((fail + 1)); echo "FAIL $1"; }
check() { # check "name" command...
  local name=$1; shift
  if "$@"; then ok "$name"; else bad "$name"; fi
}

# Fake build output and a systemctl shim that only logs.
mkdir -p "$T/build" "$T/shim"
printf '#!/bin/sh\necho fake owd\n' >"$T/build/owd"; printf '#!/bin/sh\necho fake ow\n' >"$T/build/ow"
chmod 755 "$T/build/owd" "$T/build/ow"
printf '#!/bin/sh\necho "$0 $*" >>"%s/systemctl.log"\n' "$T" >"$T/shim/systemctl"
chmod 755 "$T/shim/systemctl"
export PATH="$T/shim:$PATH"

snap() { # snapshot of a tree: names, types, modes, content hashes
  (cd "$1" && find . -printf '%p %y %m\n' | sort && find . -type f -exec sha256sum {} + | sort)
}
newhome() { # newhome NAME -> sets H and exports the fake environment
  H=$T/$1
  mkdir -p "$H"
  export HOME=$H XDG_CONFIG_HOME=$H/.config XDG_DATA_HOME=$H/.local/share XDG_STATE_HOME=$H/.local/state
  unset PREFIX
}
run() { "$script" --from "$T/build" "$@"; }
mode() { stat -c %a -- "$1"; }

# ---- 1. dry run changes nothing ---------------------------------------------------------------
newhome h1
mkdir -p "$H/keep" && echo x >"$H/keep/file"
before=$(snap "$H")
out=$(run 2>&1); rc=$?
after=$(snap "$H")
check "dry run exits 0" test $rc -eq 0
check "dry run changes nothing" test "$before" = "$after"
check "dry run says what it would do" grep -q "would create $H/.local/bin/owd" <<<"$out"
check "dry run names systemctl commands as text" grep -q "systemctl --user enable --now owd.service" <<<"$out"

# ---- 2. install -------------------------------------------------------------------------------
before_install=$(snap "$H")
out=$(run --yes 2>&1); rc=$?
check "install exits 0" test $rc -eq 0
check "owd installed 755" test "$(mode "$H/.local/bin/owd")" = 755
check "ow installed 755" test "$(mode "$H/.local/bin/ow")" = 755
check "unit installed 644" test "$(mode "$H/.config/systemd/user/owd.service")" = 644
check "data dir is 0700" test "$(mode "$H/.local/share/owd")" = 700
check "config dir is 0700" test "$(mode "$H/.config/owd")" = 700
check "ExecStart points at the prefix" grep -qx "ExecStart=\"$H/.local/bin/owd\"" "$H/.config/systemd/user/owd.service"
check "unit differs from the source only in rewritten lines" test \
  "$(diff <(cat "$unit_src") "$H/.config/systemd/user/owd.service" | grep -c '^>')" -eq 2
check "manifest exists" test -f "$H/.local/share/omarchy-widgets/install-manifest"

# ---- 3. reinstall is idempotent ---------------------------------------------------------------
m1=$(cat "$H/.local/share/omarchy-widgets/install-manifest")
s1=$(snap "$H")
out=$(run --yes 2>&1); rc=$?
check "reinstall exits 0" test $rc -eq 0
check "reinstall reports unchanged" grep -q "unchanged: $H/.local/bin/owd" <<<"$out"
check "reinstall keeps the manifest" test "$m1" = "$(cat "$H/.local/share/omarchy-widgets/install-manifest")"
check "reinstall keeps the tree" test "$s1" = "$(snap "$H")"

# ---- 4. a modified file is refused ------------------------------------------------------------
echo '#!/bin/sh' >"$H/.local/bin/owd"; echo '# local edit' >>"$H/.local/bin/owd"
edited=$(cat "$H/.local/bin/owd")
out=$(run --yes 2>&1); rc=$?
check "modified file: exit nonzero" test $rc -ne 0
check "modified file: message says refusing" grep -q "refusing to overwrite" <<<"$out"
check "modified file: left untouched" test "$edited" = "$(cat "$H/.local/bin/owd")"
out=$(run --yes --force 2>&1); rc=$?
check "--force overwrites" test $rc -eq 0 -a "$(cat "$H/.local/bin/owd")" != "$edited"

# ---- 5. a file we did not create is refused ---------------------------------------------------
newhome h5
mkdir -p "$H/.local/bin" && echo mine >"$H/.local/bin/ow"
out=$(run --yes 2>&1); rc=$?
check "foreign file: exit nonzero" test $rc -ne 0
check "foreign file: untouched" test "$(cat "$H/.local/bin/ow")" = mine
check "foreign file: nothing else was installed" test ! -e "$H/.local/bin/owd" -a ! -e "$H/.config/systemd"
ln -s /nonexistent "$H/.local/bin/owd" 2>/dev/null; rm -f "$H/.local/bin/ow"
out=$(run --yes 2>&1); rc=$?
check "symlink at a target is refused even with a clean slate" test $rc -ne 0
rm -f "$H/.local/bin/owd"

# ---- 6. uninstall removes exactly its files ---------------------------------------------------
newhome h6
mkdir -p "$H/.local/bin" && echo other >"$H/.local/bin/unrelated"
before=$(snap "$H")
run --yes >/dev/null 2>&1
echo "FAKE-vault-bytes" >"$H/.local/share/owd/vault.owv"   # user data must survive
s_dry=$(snap "$H")
out=$(run --uninstall 2>&1); rc=$?
check "uninstall dry run exits 0" test $rc -eq 0
check "uninstall dry run changes nothing" test "$s_dry" = "$(snap "$H")"
out=$(run --uninstall --yes 2>&1); rc=$?
check "uninstall exits 0" test $rc -eq 0
check "owd removed" test ! -e "$H/.local/bin/owd"
check "ow removed" test ! -e "$H/.local/bin/ow"
check "unit removed" test ! -e "$H/.config/systemd/user/owd.service"
check "manifest removed" test ! -e "$H/.local/share/omarchy-widgets/install-manifest"
check "unrelated file kept" test "$(cat "$H/.local/bin/unrelated")" = other
check "vault kept" test "$(cat "$H/.local/share/owd/vault.owv")" = FAKE-vault-bytes

# A clean cycle returns the tree to exactly where it started (dirs we created are removed too).
newhome h6c
mkdir -p "$H/.local/bin" && echo other >"$H/.local/bin/unrelated"
before=$(snap "$H")
run --yes >/dev/null 2>&1; run --uninstall --yes >/dev/null 2>&1
check "install then uninstall restores the tree exactly" test "$before" = "$(snap "$H")"

# uninstall keeps a file the user modified
newhome h6b
run --yes >/dev/null 2>&1
echo changed >>"$H/.local/bin/ow"
out=$(run --uninstall --yes 2>&1)
check "uninstall keeps a modified file" test -f "$H/.local/bin/ow"
check "uninstall says why" grep -q "modified since install" <<<"$out"
check "uninstall without manifest refuses" bash -c "! HOME=$T/nohome XDG_DATA_HOME=$T/nohome/d XDG_CONFIG_HOME=$T/nohome/c '$script' --uninstall --yes >/dev/null 2>&1"

# ---- 7. paths with spaces ---------------------------------------------------------------------
H="$T/h 7"
mkdir -p "$H"
export HOME=$H XDG_CONFIG_HOME="$H/my config" XDG_DATA_HOME="$H/my data"
out=$(run --yes --prefix "$H/pre fix" 2>&1); rc=$?
check "spaces: install exits 0" test $rc -eq 0
check "spaces: binaries installed" test -x "$H/pre fix/bin/owd"
check "spaces: ExecStart is quoted" grep -qx "ExecStart=\"$H/pre fix/bin/owd\"" "$H/my config/systemd/user/owd.service"
check "spaces: ReadWritePaths rewritten and quoted" grep -qx "ReadWritePaths=\"$H/my data/owd\"" "$H/my config/systemd/user/owd.service"
if command -v systemd-analyze >/dev/null; then
  v=$(systemd-analyze --user verify "$H/my config/systemd/user/owd.service" 2>&1)
  check "spaces: systemd-analyze verify is clean" test -z "$v"
fi
out=$(run --uninstall --yes --prefix "$H/pre fix" 2>&1)
check "spaces: uninstall removes the binary" test ! -e "$H/pre fix/bin/owd"

# ---- 8. a symlinked prefix is refused ---------------------------------------------------------
newhome h8
mkdir -p "$H/real" && ln -s "$H/real" "$H/link"
before=$(snap "$H")
out=$(run --yes --prefix "$H/link" 2>&1); rc=$?
check "symlinked prefix: refused" test $rc -ne 0
check "symlinked prefix: nothing written" test "$before" = "$(snap "$H")"
mkdir -p "$H/p2" && ln -s "$H/real" "$H/p2/bin"
out=$(run --yes --prefix "$H/p2" 2>&1); rc=$?
check "symlinked bin dir: refused" test $rc -ne 0

# ---- 9. bad input -----------------------------------------------------------------------------
newhome h9
check "relative prefix refused" bash -c "! '$script' --from '$T/build' --prefix rel >/dev/null 2>&1"
check "missing binaries refused" bash -c "! '$script' --from '$T/nothing' >/dev/null 2>&1"
check "unknown option refused" bash -c "! '$script' --bogus >/dev/null 2>&1"

# ---- never systemctl --------------------------------------------------------------------------
check "systemctl was never called" test ! -e "$T/systemctl.log"

echo "passed $pass, failed $fail"
test $fail -eq 0
