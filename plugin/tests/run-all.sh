#!/bin/bash
# Everything for the plugin, in order. Offscreen and throwaway only; see plugin/README.md.
set -u
here=$(cd "$(dirname "$0")" && pwd); plugin=$(cd "$here/.." && pwd)
rc=0
echo "== qmllint"
out=$(cd "$plugin" && /usr/lib/qt6/bin/qmllint -I "$here/stubs" *.qml 2>&1 | grep -v "not declared as singleton in qmldir")
# the filtered line is a qmllint 6.12 false positive: it also fires on a three-line singleton in an empty directory
[ -n "$out" ] && { echo "$out"; rc=1; } || echo "clean"
echo "== static safety check"; python3 "$here/static_check.py" --selftest && python3 "$here/static_check.py" || rc=1
echo "== QtTest (offscreen)"; "$here/run-qml-tests.sh" || rc=1
echo "== bridge against fake owd"; "$here/run-bridge-tests.sh" | grep -E "^(FAIL|RESULT)" ; [ "${PIPESTATUS[0]}" -ne 0 ] && rc=1
echo "== store against hostile tree"; "$here/run-store-tests.sh" | grep -E "^(FAIL|RESULT)"; [ "${PIPESTATUS[0]}" -ne 0 ] && rc=1
echo "== omarchy plugin validate (read-only)"; omarchy plugin validate "$plugin" && echo valid || rc=1
echo "overall rc=$rc"; exit $rc
