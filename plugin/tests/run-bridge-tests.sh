#!/bin/bash
# Runs the Bridge against tests/fake_owd.py inside a throwaway Quickshell (own config dir,
# own XDG dirs, own processes). Starts only PIDs it records and signals only those.
# QT_QPA_PLATFORM=offscreen: no display is touched. Usage: run-bridge-tests.sh [scenario ...]
set -u
here=$(cd "$(dirname "$0")" && pwd)
plugin=$(cd "$here/.." && pwd)
scenarios=("$@"); [ ${#scenarios[@]} -eq 0 ] && scenarios=(normal caps fragment garbage oversize flood floodpre burst drop silent reject refuse unsupported badproto noproto locked late defaultpath nopath relpath)
rc=0
cleanup_pids=()
trap 'for p in "${cleanup_pids[@]}"; do kill -0 "$p" 2>/dev/null && kill "$p" 2>/dev/null; done' EXIT
for sc in "${scenarios[@]}"; do
  run=$(mktemp -d -p /tmp ow-bridge.XXXX)
  mkdir -p "$run/cfg" "$run/state" "$run/cache" "$run/data" "$run/rt/owd"
  chmod 700 "$run/rt" "$run/rt/owd"
  cp -r "$plugin" "$run/cfg/plugin"; rm -rf "$run/cfg/plugin/tests"; cp "$here/bridge/shell.qml" "$run/cfg/shell.qml"; cp -r "$here/stubs/qs/Commons" "$run/cfg/Commons"
  cd "$run"      # a relative XDG_RUNTIME_DIR (relpath scenario) must not create directories in the repo
  sock=$run/rt/owd/owd.sock
  log=$run/server.log
  unset WAYLAND_DISPLAY HYPRLAND_INSTANCE_SIGNATURE
  export XDG_STATE_HOME=$run/state XDG_CACHE_HOME=$run/cache XDG_DATA_HOME=$run/data QT_QPA_PLATFORM=offscreen
  export OW_TEST_SCENARIO=$sc OW_TEST_SOCK=$sock XDG_RUNTIME_DIR=$run/rt
  [ "$sc" = defaultpath ] && export OW_TEST_SOCK=""
  [ "$sc" = relpath ] && { export OW_TEST_SOCK=""; export XDG_RUNTIME_DIR=relative/dir; }
  [ "$sc" = nopath ] && { export OW_TEST_SOCK=""; unset XDG_RUNTIME_DIR; }
  srv=""
  srv_scenario=$sc
  case "$sc" in defaultpath|late|nopath|caps|relpath) srv_scenario=normal;; esac
  if [ "$sc" = late ]; then
    ( sleep 3; exec python3 "$here/fake_owd.py" "$sock" normal "$log" ) & srv=$!
  elif [ "$sc" != nopath ] && [ "$sc" != relpath ]; then
    python3 "$here/fake_owd.py" "$sock" "$srv_scenario" "$log" & srv=$!
  fi
  [ -n "$srv" ] && cleanup_pids+=("$srv")
  [ "$sc" != late ] && python3 -c 'import time;time.sleep(0.4)'
  timeout -s TERM 30 quickshell -p "$run/cfg" >"$run/qs.log" 2>&1 &
  qs=$!; cleanup_pids+=("$qs")
  # peak resident memory of the throwaway Quickshell (our own pid; timeout execs it as a child, so look one level down)
  peak=0
  while kill -0 "$qs" 2>/dev/null; do
    for p in "$qs" $(pgrep -P "$qs" 2>/dev/null); do
      r=$(awk '/VmRSS/{print $2}' /proc/$p/status 2>/dev/null); [ -n "${r:-}" ] && [ "$r" -gt "$peak" ] && peak=$r
    done
    python3 -c 'import time;time.sleep(0.1)'
  done
  wait "$qs"
  echo "PEAK_RSS_KB $sc $peak" >"$run/peak.txt"
  case "$sc" in flood|floodpre|burst)
    if [ "$peak" -gt 400000 ]; then echo "FAIL $sc: resident memory peaked at $((peak/1024)) MiB (cap 400 MiB)"; rc=1; else echo "PASS $sc: resident memory peaked at $((peak/1024)) MiB under an unbounded stream"; fi;;
  esac
  # the late server runs in a subshell that exec'd python: $srv is that pid
  [ -n "$srv" ] && kill -0 "$srv" 2>/dev/null && kill "$srv" 2>/dev/null
  sed 's/\x1b\[[0-9;]*m//g' "$run/qs.log" | sed -n 's/^.*qml: \(\(PASS\|FAIL\|RESULT\) .*\)$/\1/p' >"$run/result.txt"
  cat "$run/result.txt"
  grep -q "^RESULT" "$run/result.txt" || { echo "FAIL $sc: harness produced no RESULT"; tail -15 "$run/qs.log"; rc=1; }
  grep -q "^FAIL" "$run/result.txt" && rc=1
  # what the client sent: only hello, subscribe, unsubscribe, with only expected keys, nothing credential-like
  if [ -f "$log" ]; then
    python3 - "$log" "$sc" <<'PY' || rc=1
import json, sys
log, sc = sys.argv[1], sys.argv[2]
ops, conns, hellos, subs = [], 0, 0, 0
per_topic = {}
bad = []
for l in open(log):
    if l.startswith("CONNECT"): conns += 1
    if not l.startswith("REQ "): continue
    raw = l[4:]
    try: m = json.loads(raw)
    except Exception: bad.append("unparseable request: " + raw[:80]); continue
    if m.get("op") not in ("hello", "subscribe", "unsubscribe"): bad.append("unexpected op " + str(m.get("op")))
    if set(m) - {"v", "id", "op", "client", "clientVersion", "widget", "topic", "sub"}: bad.append("unexpected keys " + str(sorted(m)))
    if m.get("v") != 0: bad.append("bad version")
    low = raw.lower()
    for w in ("passphrase", "token", "secret", "password", "vault"):
        if w in low: bad.append("credential-like word " + w)
    hellos += m.get("op") == "hello"; subs += m.get("op") == "subscribe"
    if m.get("op") == "subscribe": per_topic[m.get("topic")] = per_topic.get(m.get("topic"), 0) + 1
msgs = []
if bad: msgs.append("FAIL %s: bad requests: %s" % (sc, bad[:3]))
else: msgs.append("PASS %s: every request is hello/subscribe/unsubscribe with expected keys only (%d hellos, %d subscribes, %d connections)" % (sc, hellos, subs, conns))
if sc == "normal" and not any('"unsubscribe"' in l for l in open(log)): msgs.append("FAIL normal: no unsubscribe was sent when a topic was dropped")
if sc == "normal":
    if sorted(per_topic) != ["x.denied", "x.locked", "x.one", "x.three", "x.two", "x.unknown"]: msgs.append("FAIL normal: subscribed topics %s" % sorted(per_topic))
    if per_topic.get("x.denied") != 1 or per_topic.get("x.unknown") != 1: msgs.append("FAIL normal: denied/unknown topics must be asked once, got %s" % per_topic)
    if per_topic.get("x.locked", 0) < 2: msgs.append("FAIL normal: locked topic was not retried: %s" % per_topic)
    if per_topic.get("x.one") != 1: msgs.append("FAIL normal: x.one asked %s times (duplicates in wanted must collapse)" % per_topic.get("x.one"))
if sc == "drop" and hellos < 2: msgs.append("FAIL drop: expected a second hello after reconnect, got %d" % hellos)
if sc == "drop" and subs < 4: msgs.append("FAIL drop: expected subscriptions re-sent after reconnect, got %d" % subs)
if sc in ("flood", "floodpre") and not (2 <= conns <= 5): msgs.append("FAIL %s: %d connections in 8s (backoff expected 2 to 5)" % (sc, conns))
if sc in ("flood", "floodpre", "burst"):
    sent = [int(l.split()[1]) for l in open(log) if l.startswith("SENT_MIB")]
    msgs.append("PASS %s: peer managed to send only %s MiB before being dropped" % (sc, sent)) if sent and max(sent) < 3000 else msgs.append("FAIL %s: peer sent %s MiB" % (sc, sent))
if sc == "caps":
    big = [t for t in per_topic if t[3:].isdigit() and t.startswith("x.t")]
    if len(big) > 20: msgs.append("FAIL caps: %d distinct big-widget topics were ever subscribed (16 at once, then the retry admitted up to 4 more)" % len(big))
    if per_topic.get("x.one", 0) < 2: msgs.append("FAIL caps: the other widgets' topics were never subscribed: %s" % per_topic)
if sc == "silent" and not (2 <= conns <= 6): msgs.append("FAIL silent: %d connections in 8s (backoff expected 2 to 6)" % conns)
if sc == "reject" and not (3 <= conns <= 7): msgs.append("FAIL reject: %d connections in 7s (backoff expected 3 to 7)" % conns)
if sc == "locked" and subs < 6: msgs.append("FAIL locked: locked topics should be re-asked, subscribes=%d" % subs)
for m in msgs: print(m)
sys.exit(1 if any(m.startswith("FAIL") for m in msgs) else 0)
PY
  fi
  rm -rf "$run"
done
exit $rc
