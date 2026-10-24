#!/bin/bash
# Offscreen QtTest runs. Touches no display, no config dir, no shell process.
#   plugin/tests/run-qml-tests.sh [tst_name.qml ...]
set -u
here=$(cd "$(dirname "$0")" && pwd)
export QT_QPA_PLATFORM=offscreen QML_XHR_ALLOW_FILE_READ=1 QT_QUICK_BACKEND=software
export XDG_CACHE_HOME=$(mktemp -d -p /tmp ow-qmltest.XXXX)
runner=/usr/lib/qt6/bin/qmltestrunner
rc=0
files=("$@"); [ ${#files[@]} -eq 0 ] && files=($(cd "$here" && ls tst_*.qml))
for f in "${files[@]}"; do
  case "$f" in
    *_missing.qml) stub=$here/stubs_missing;;     # Omarchy tokens renamed: separate process, separate module path
    *) stub=$here/stubs;;
  esac
  out=$(cd "$here" && "$runner" -import "$stub" -input "$f" 2>&1); code=$?
  printf '%s\n' "$out" | grep -E "^(FAIL|XFAIL|XPASS|Totals|QFATAL)|Error|error:|   Loc:|   Actual|   Expected" | sed "s|^|$f: |"
  [ "$code" -ne 0 ] && rc=1
done
rm -rf "$XDG_CACHE_HOME"
exit $rc
