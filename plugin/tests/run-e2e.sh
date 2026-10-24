#!/bin/bash
# End-to-end smoke: the real owd (from this repo) on a throwaway tree, fake installs of
# widgets, the real plugin files in an offscreen Quickshell, a PNG of what it draws.
#   run-e2e.sh <output.png>
# Safety: every directory is a fresh mktemp tree; XDG_* point there; only PIDs this script
# started are signalled; WAYLAND_DISPLAY is unset and the Qt platform is offscreen.
set -u
here=$(cd "$(dirname "$0")" && pwd); plugin=$(cd "$here/.." && pwd); repo=$(cd "$plugin/.." && pwd)
png=${1:?output png path}
D=$(mktemp -d -p /tmp owd.XXXX)
run=$(mktemp -d -p /tmp ow-e2e.XXXX)
pids=()
trap 'for p in "${pids[@]}"; do kill -0 "$p" 2>/dev/null && kill "$p" 2>/dev/null; done; rm -rf "$D" "$run"' EXIT
(cd "$repo" && cargo build -q -p owd --locked) || exit 1
# paths as ow-core maps them: D/{config,data,state,run}/owd
mkdir -p "$D"/{config,data,state,run}/owd "$D"/data/owd/widgets
chmod 700 "$D"/{config,data,state,run}/owd
fx=$here/fixtures
inst() { cp -r "$fx/$1" "$D/data/owd/widgets/$2"; }
python3 - "$fx" "$D" <<'PY'
import json, os, shutil, sys
fx, D = sys.argv[1:3]
w = f"{D}/data/owd/widgets"; grants = []
for name, topics in (("clock", ["system.time"]), ("system-monitor", ["system.cpu", "system.memory", "system.load"]), ("battery", ["battery.status"])):
    wid = json.load(open(f"{fx}/{name}/widget.json"))
    shutil.copytree(f"{fx}/{name}", f"{w}/{wid['id']}")
    fields = [f for p in wid["providers"] for f in p["fields"]]
    grants.append(f'[[grant]]\nwidget = "{wid["id"]}"\ntopics = {json.dumps(topics)}\nfields = {json.dumps(fields)}\napproved_at = "2026-10-23T12:00:00Z"\n')
# a hostile install: valid id, layout with an unknown node type
bad = f"{w}/io.test.hostile"; shutil.copytree(f"{fx}/clock", bad)
for f in ("manifest", "widget", "installed"):
    o = json.load(open(f"{bad}/{f}.json")); o["id"] = "io.test.hostile"; json.dump(o, open(f"{bad}/{f}.json", "w"))
l = json.load(open(f"{bad}/layout.json")); l["root"]["children"][0]["type"] = "exec"; json.dump(l, open(f"{bad}/layout.json", "w"))
open(f"{D}/config/owd/grants.toml", "w").write("\n".join(grants))
os.chmod(f"{D}/config/owd/grants.toml", 0o600)
PY
(cd "$repo" && exec ./target/debug/owd --config-dir "$D") >"$run/owd.log" 2>&1 &
pids+=($!)
python3 -c 'import time;time.sleep(1)'
ls -l "$D/run/owd/" | tail -2
mkdir -p "$run/cfg" "$run/state" "$run/cache"
cp -r "$plugin" "$run/cfg/plugin"; rm -rf "$run/cfg/plugin/tests"
if [ "${OW_E2E_LIVE:-0}" = 1 ]; then cp "$here/e2e/live.qml" "$run/cfg/shell.qml"; else cp "$here/e2e/shell.qml" "$run/cfg/shell.qml"; fi
# real Omarchy tokens if present (read-only symlink), else the test stub
if [ -d /usr/share/omarchy/shell/Commons ] && [ "${OW_E2E_STUB:-0}" != 1 ]; then ln -s /usr/share/omarchy/shell/Commons "$run/cfg/Commons"; tokens=real
else cp -r "$here/stubs/qs/Commons" "$run/cfg/Commons"; tokens=stub; fi
echo "theme tokens: $tokens"
export XDG_RUNTIME_DIR=$D/run XDG_DATA_HOME=$D/data XDG_STATE_HOME=$run/state XDG_CACHE_HOME=$run/cache OW_E2E_PNG=$png
if [ "${OW_E2E_LIVE:-0}" = 1 ]; then
  # a few seconds of a real Bottom-layer surface on the live compositor (like spikes/s1); read-only hyprctl probes
  realrt=${XDG_RUNTIME_DIR_REAL:-/run/user/$(id -u)}
  case "$WAYLAND_DISPLAY" in /*) ;; *) export WAYLAND_DISPLAY=$realrt/$WAYLAND_DISPLAY;; esac   # our XDG_RUNTIME_DIR is the throwaway one
  timeout -s TERM 12 quickshell -p "$run/cfg" >"$run/qs.log" 2>&1 &
  qs=$!; pids+=("$qs")
  python3 -c 'import time;time.sleep(3)'
  echo "--- hyprctl layers (owd-canvas only)"; XDG_RUNTIME_DIR=$realrt hyprctl layers -j 2>/dev/null | python3 -c '
import json,sys
d=json.load(sys.stdin)
for mon,lv in d.items():
    for level,obj in lv["levels"].items():
        for l in obj:
            if l["namespace"]=="owd-canvas": print(mon,"level",level,l["namespace"],l["w"],"x",l["h"])'
  wait "$qs"
else
  unset WAYLAND_DISPLAY HYPRLAND_INSTANCE_SIGNATURE
  export QT_QPA_PLATFORM=offscreen QT_QUICK_BACKEND=software
  timeout -s TERM 20 quickshell -p "$run/cfg" >"$run/qs.log" 2>&1 &
  qs=$!; pids+=("$qs"); wait "$qs"
fi
sed 's/\x1b\[[0-9;]*m//g' "$run/qs.log" | sed -n 's/^.*qml: \(.*\)$/\1/p'
grep -E "ERROR|WARN" "$run/qs.log" | sed 's/\x1b\[[0-9;]*m//g' | grep -v "WAYLAND_DISPLAY\|QT_QPA_PLATFORM\|If you are" | head
echo "--- owd log"; cat "$run/owd.log" | head -5
echo "--- audit lines (no payloads)"; head -3 "$D/state/owd/audit.jsonl" 2>/dev/null
