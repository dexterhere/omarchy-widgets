#!/bin/bash
# Draw the widgets that `ow install` put in a SANDBOX, with the plugin, offscreen, to a PNG.
#   run-installed.sh <sandbox-dir> <output.png>
# <sandbox-dir> is the mktemp tree of docs/TRY_IT.md (env.sh inside it points the XDG_* variables
# at it and `owd` is already running there). Nothing is started here except one offscreen
# Quickshell that this script owns; no layer-shell surface, no Wayland, your shell is not touched.
set -u
here=$(cd "$(dirname "$0")" && pwd); plugin=$(cd "$here/.." && pwd)
S=$(realpath "${1:?sandbox dir}"); png=${2:?output png path}
case "$S" in /tmp/ow-try.*) ;; *) echo "refusing: $S is not a /tmp/ow-try.* sandbox" >&2; exit 2;; esac
[ -S "$S/run/owd/owd.sock" ] || { echo "no owd socket in $S/run/owd: start owd first (docs/TRY_IT.md step 3)" >&2; exit 2; }
run=$(mktemp -d -p /tmp ow-qs.XXXX)
trap 'rm -rf "$run"' EXIT
mkdir -p "$run/cfg" "$run/state" "$run/cache"
cp -r "$plugin" "$run/cfg/plugin"; rm -rf "$run/cfg/plugin/tests"
cp "$here/e2e/shell.qml" "$run/cfg/shell.qml"
if [ -d /usr/share/omarchy/shell/Commons ]; then ln -s /usr/share/omarchy/shell/Commons "$run/cfg/Commons"
else cp -r "$here/stubs/qs/Commons" "$run/cfg/Commons"; fi
unset WAYLAND_DISPLAY HYPRLAND_INSTANCE_SIGNATURE
export XDG_RUNTIME_DIR=$S/run XDG_DATA_HOME=$S/data XDG_STATE_HOME=$run/state XDG_CACHE_HOME=$run/cache
export OW_E2E_PNG=$png QT_QPA_PLATFORM=offscreen QT_QUICK_BACKEND=software
timeout -s TERM 20 quickshell -p "$run/cfg" >"$run/qs.log" 2>&1 &
qs=$!; wait "$qs"
sed 's/\x1b\[[0-9;]*m//g' "$run/qs.log" | sed -n 's/^.*qml: \(.*\)$/\1/p'
grep -E "ERROR|WARN" "$run/qs.log" | sed 's/\x1b\[[0-9;]*m//g' | grep -v "WAYLAND_DISPLAY\|QT_QPA_PLATFORM\|If you are" | head
