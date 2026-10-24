#!/bin/bash
# S3 driver. Starts only processes it records itself (stub PIDs), kills only those.
#   run.sh ok      -> client, hot reload at ~2s, server dies at ~4s and is restarted at ~6s
#   run.sh reject  -> stub expects a different uid; client must get no replies
set -u
here=$(cd "$(dirname "$0")" && pwd)
mode=${1:-ok}
d=$(mktemp -d -p /tmp ow-spike.XXXX); sock=$d/s.sock; cfg=$d/cfg; mkdir "$cfg"; cp "$here/shell.qml" "$cfg/"
export OW_SOCK=$sock
sleep_() { python3 -c "import time;time.sleep($1)"; }
if [ "$mode" = reject ]; then extra="--expect-uid 0"; else extra="--die-after 7"; fi
python3 "$here/stub_server.py" "$sock" $extra > "$d/stub1.log" 2>&1 & s1=$!
echo "stub1 pid=$s1 sock=$sock"
export mode cfg d here sock
"$here/../run-safe.sh" "$cfg" 13 "$here/probe.sh"
kill -0 "$s1" 2>/dev/null && kill "$s1"
[ -f "$d/stub2.pid" ] && kill "$(cat "$d/stub2.pid")" 2>/dev/null
echo "=== stub1.log"; cat "$d/stub1.log"; echo "=== stub2.log"; cat "$d/stub2.log" 2>/dev/null
