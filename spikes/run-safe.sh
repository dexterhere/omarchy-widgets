#!/bin/bash
# Run a throwaway Quickshell config for a few seconds without touching the live shell.
#   run-safe.sh <config-dir> <seconds> [probe-command...]
# Safety properties (all enforced here, not left to the caller):
#  - refuses the real Omarchy shell path and anything under ~/.config
#  - uses its own state/cache/data dirs so nothing lands in the user's XDG dirs
#  - never matches processes by name: it records the PID it started and signals only that PID
#  - hard time limit via timeout(1), which is our own child
set -u
cfg=$(realpath "$1"); secs=${2:-6}; shift 2
case "$cfg" in
  /usr/share/omarchy/*|"$HOME"/.config/*) echo "refusing real config path: $cfg" >&2; exit 2;;
esac
[ -f "$cfg/shell.qml" ] || [ -f "$cfg" ] || { echo "no shell.qml in $cfg" >&2; exit 2; }
run=$(mktemp -d -p /tmp ow-spike.XXXX)
export XDG_STATE_HOME=$run/state XDG_CACHE_HOME=$run/cache XDG_DATA_HOME=$run/data
mkdir -p "$XDG_STATE_HOME" "$XDG_CACHE_HOME" "$XDG_DATA_HOME"
export OW_SPIKE_RUN=$run
log=$run/qs.log
timeout -s TERM "$secs" quickshell -p "$cfg" >"$log" 2>&1 &
pid=$!; echo "$pid" >"$run/pid"
echo "run dir: $run  pid: $pid"
for _ in $(seq 1 50); do grep -q "Configuration Loaded" "$log" && break; python3 -c 'import time;time.sleep(0.1)'; done
if [ $# -gt 0 ]; then "$@"; fi
wait "$pid"; rc=$?
# belt and braces: only our own recorded PID
kill -0 "$pid" 2>/dev/null && kill "$pid"
echo "exit=$rc"; echo "--- qs.log"; cat "$log"
