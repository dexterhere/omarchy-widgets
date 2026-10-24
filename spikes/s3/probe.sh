#!/bin/bash
# Called by run-safe.sh while the throwaway Quickshell runs. Env from run.sh: mode cfg d here sock.
sleep_() { python3 -c "import time;time.sleep($1)"; }
if [ "$mode" = ok ]; then
  sleep_ 2.5; echo "// force hot reload $(date +%s)" >> "$cfg/shell.qml"       # config reload, like rescanPlugins
  sleep_ 6
  python3 "$here/stub_server.py" "$sock" > "$d/stub2.log" 2>&1 & echo $! > "$d/stub2.pid"   # server restart
  sleep_ 3
else sleep_ 3; fi
