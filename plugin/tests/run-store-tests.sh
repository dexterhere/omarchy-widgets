#!/bin/bash
# Store against a hostile throwaway widgets tree, in a throwaway offscreen Quickshell.
# The harness is read-only; this script performs the on-disk changes it asks for.
set -u
here=$(cd "$(dirname "$0")" && pwd); plugin=$(cd "$here/.." && pwd)
run=$(mktemp -d -p /tmp ow-store.XXXX)
mkdir -p "$run/cfg" "$run/state" "$run/cache" "$run/data" "$run/w"
cp -r "$plugin" "$run/cfg/plugin"; rm -rf "$run/cfg/plugin/tests"
cp "$here/store/shell.qml" "$run/cfg/shell.qml"; cp -r "$here/stubs/qs/Commons" "$run/cfg/Commons"
python3 - "$here/fixtures" "$run/w" <<'PY'
import json, os, shutil, sys
fx, w = sys.argv[1:3]
for n in ("clock", "system-monitor", "battery"):
    shutil.copytree(f"{fx}/{n}", f"{w}/" + json.load(open(f"{fx}/{n}/widget.json"))["id"])
def make(id, **over):
    d = f"{w}/{id}"; shutil.copytree(f"{fx}/clock", d)
    for f in ("manifest", "widget", "layout", "installed"):
        o = json.load(open(f"{d}/{f}.json"))
        if "id" in o: o["id"] = id
        json.dump(o, open(f"{d}/{f}.json", "w"))
    return d
d = make("io.test.huge"); open(f"{d}/layout.json", "w").write(json.dumps({"schemaVersion": 0, "root": {"type": "text", "text": "x"}, "pad": "a" * 100000}))
d = make("io.test.badnode"); l = json.load(open(f"{d}/layout.json")); l["root"]["children"][0]["type"] = "exec"; json.dump(l, open(f"{d}/layout.json", "w"))
d = make("io.test.mismatch"); m = json.load(open(f"{d}/manifest.json")); m["id"] = "io.test.other"; json.dump(m, open(f"{d}/manifest.json", "w"))
d = make("io.test.code"); x = json.load(open(f"{d}/widget.json")); x["tier"] = "code"; json.dump(x, open(f"{d}/widget.json", "w"))
d = make("io.test.missing"); os.unlink(f"{d}/layout.json")
d = make("io.test.empty"); open(f"{d}/widget.json", "w").close()
d = make("io.test.bomb"); open(f"{d}/layout.json", "w").write("[" * 60000)
make(".stage-abc"); os.makedirs(f"{w}/Bad Dir"); os.makedirs(f"{w}/UPPER")
open(f"{w}/.lock", "w").close()
PY
unset WAYLAND_DISPLAY HYPRLAND_INSTANCE_SIGNATURE
export XDG_STATE_HOME=$run/state XDG_CACHE_HOME=$run/cache XDG_DATA_HOME=$run/data XDG_RUNTIME_DIR=$run/data QT_QPA_PLATFORM=offscreen OW_TEST_WIDGETS=$run/w
fx=$here/fixtures; clk=io.github.dexterhere.clock
timeout -s TERM 30 quickshell -p "$run/cfg" >"$run/qs.log" 2>&1 &
qs=$!
trap 'kill -0 "$qs" 2>/dev/null && kill "$qs" 2>/dev/null' EXIT
seen=0; peak=0
while kill -0 "$qs" 2>/dev/null; do
  for p in "$qs" $(pgrep -P "$qs" 2>/dev/null); do r=$(awk '/VmRSS/{print $2}' /proc/$p/status 2>/dev/null); [ -n "${r:-}" ] && [ "$r" -gt "$peak" ] && peak=$r; done
  n=$(grep -c "MUTATE" "$run/qs.log" 2>/dev/null); n=${n:-0}
  while [ "$seen" -lt "$n" ]; do
    seen=$((seen+1))
    what=$(grep "MUTATE" "$run/qs.log" | sed -n "${seen}p" | sed 's/^.*MUTATE //')
    case "$what" in
      break) python3 -c "import json,sys;p='$run/w/$clk/layout.json';l=json.load(open(p));l['root']['type']='exec';json.dump(l,open(p,'w'))";;
      grow) truncate -s 1500M "$run/w/$clk/layout.json";;
      fix) cp "$fx/clock/layout.json" "$run/w/$clk/layout.json";;
      remove) rm -rf "$run/w/io.github.dexterhere.battery";;
      add) mkdir "$run/w/.stage-new"; for f in manifest widget layout installed; do python3 -c "import json;o=json.load(open('$fx/clock/$f.json'));o.update({'id':'io.test.added'} if 'id' in o else {});json.dump(o,open('$run/w/.stage-new/$f.json','w'))"; done; mv "$run/w/.stage-new" "$run/w/io.test.added";;
    esac
  done
  python3 -c 'import time;time.sleep(0.1)'
done
sed 's/\x1b\[[0-9;]*m//g' "$run/qs.log" | sed -n 's/^.*qml: \(\(PASS\|FAIL\|RESULT\) .*\)$/\1/p' | tee "$run/result.txt"
rc=0
if [ "$peak" -gt 400000 ]; then echo "FAIL store: peak resident memory $((peak/1024)) MiB after a 1.5 GB in-place growth"; rc=1; else echo "PASS store: peak resident memory $((peak/1024)) MiB with a 1.5 GB file grown in place"; fi
grep -q '^RESULT' "$run/result.txt" || { echo "FAIL store: no RESULT"; tail -20 "$run/qs.log"; rc=1; }
grep -q '^FAIL' "$run/result.txt" && rc=1
rm -rf "$run"; exit $rc
