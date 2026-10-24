#!/usr/bin/env python3
"""SPIKE S3 stub: newline-delimited JSON over a Unix socket, SO_PEERCRED uid check.
Speaks a tiny subset of docs/DAEMON_SPEC.md section 5: hello, subscribe (pushes once a second).
Usage: stub_server.py <socket-path> [--expect-uid N] [--die-after SECONDS]
Throwaway: single process, one thread per client, no auth beyond the uid check."""
import json, os, socket, struct, sys, threading, time

path = sys.argv[1]
expect_uid = int(sys.argv[sys.argv.index("--expect-uid") + 1]) if "--expect-uid" in sys.argv else os.getuid()
die_after = float(sys.argv[sys.argv.index("--die-after") + 1]) if "--die-after" in sys.argv else None

def log(*a): print(time.strftime("%H:%M:%S"), "stub:", *a, flush=True)

def peer_uid(conn):
    pid, uid, gid = struct.unpack("3i", conn.getsockopt(socket.SOL_SOCKET, socket.SO_PEERCRED, struct.calcsize("3i")))
    return pid, uid

def serve(conn):
    pid, uid = peer_uid(conn)
    if uid != expect_uid:                      # the check the real daemon does
        log(f"REJECT peer pid={pid} uid={uid} (expected {expect_uid})")
        conn.close(); return
    log(f"accept peer pid={pid} uid={uid}")
    f = conn.makefile("rwb", buffering=0)
    subs = []
    def push():
        n = 0
        while subs:
            n += 1
            try: f.write((json.dumps({"v": 0, "sub": subs[0], "topic": "spike.tick", "ts": int(time.time()), "stale": False, "data": {"n": n}}) + "\n").encode())
            except OSError: return
            time.sleep(1)
    for line in f:
        try: req = json.loads(line)
        except ValueError: f.write(b'{"v":0,"error":{"code":"bad_request"}}\n'); continue
        log("recv", req.get("op"), "id", req.get("id"))
        if req.get("op") == "hello":
            f.write((json.dumps({"v": 0, "id": req["id"], "ok": True, "daemonVersion": "stub", "protocol": 0}) + "\n").encode())
        elif req.get("op") == "subscribe":
            subs.append(7); f.write((json.dumps({"v": 0, "id": req["id"], "ok": True, "sub": 7}) + "\n").encode())
            threading.Thread(target=push, daemon=True).start()
        else:
            f.write((json.dumps({"v": 0, "id": req.get("id"), "error": {"code": "bad_request"}}) + "\n").encode())
    subs.clear(); log("client gone")

try: os.unlink(path)
except FileNotFoundError: pass
srv = socket.socket(socket.AF_UNIX); srv.bind(path); os.chmod(path, 0o600); srv.listen(8)
log("listening", path)
if die_after: threading.Timer(die_after, lambda: (log("dying"), os._exit(0))).start()
while True:
    c, _ = srv.accept(); threading.Thread(target=serve, args=(c,), daemon=True).start()
