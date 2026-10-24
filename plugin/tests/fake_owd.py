#!/usr/bin/env python3
"""Tiny fake owd for the bridge tests. Speaks the DAEMON_SPEC v0 framing on a Unix socket.

  fake_owd.py <socket-path> <scenario> <log-file>

Scenarios: normal fragment garbage oversize drop silent reject refuse unsupported locked flood floodpre burst
Every request line it receives is appended to the log, so tests can assert what the
client sent. It is a test double; it checks nothing about peers and keeps no state."""
import json, os, socket, sys, threading, time

path, scenario, logf = sys.argv[1], sys.argv[2], sys.argv[3]
lock = threading.Lock()
open(logf, "w").close()


def log(s):
    with lock, open(logf, "a") as f:
        f.write(s + "\n")


def listen():
    try:
        os.unlink(path)
    except FileNotFoundError:
        pass
    s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    s.bind(path)
    os.chmod(path, 0o600)
    s.listen(8)
    return s


def send(c, obj, chunk=0):
    data = (obj if isinstance(obj, bytes) else (json.dumps(obj, ensure_ascii=False) + "\n").encode())
    if chunk:
        for i in range(0, len(data), chunk):
            c.sendall(data[i:i + chunk])
            time.sleep(0.004)
    else:
        c.sendall(data)


def push(c, sub, topic, n, **kw):
    msg = {"v": 0, "sub": sub, "topic": topic, "ts": 1760700000 + n, "stale": kw.get("stale", False),
           "data": kw.get("data", {"n": n, "name": "hello"})}
    send(c, msg, kw.get("chunk", 0))


def flood(c, conn_id):
    """Peers that try to exhaust the client: never a newline (floodpre: before hello is answered;
    flood: after a good hello), or an endless stream of valid short lines (burst)."""
    c.settimeout(2)
    if scenario == "flood":
        try:
            buf = b""
            while b"\n" not in buf:
                buf += c.recv(4096)
            send(c, {"v": 0, "id": 1, "ok": True, "daemonVersion": "0.0-test", "protocol": 0})
        except OSError:
            return
    blob = b"a" * 65536 if scenario != "burst" else (json.dumps({"v": 0, "sub": 1, "topic": "x.one", "ts": 1, "stale": False, "data": {"n": 1}}) + "\n").encode() * 1000
    if scenario == "burst":
        try:
            buf = b""
            while b"\n" not in buf:
                buf += c.recv(4096)
            send(c, {"v": 0, "id": 1, "ok": True, "daemonVersion": "0.0-test", "protocol": 0})
        except OSError:
            return
    end = time.time() + 6
    sent = 0
    try:
        while time.time() < end:
            c.sendall(blob)
            sent += len(blob)
    except OSError:
        pass
    log("SENT_MIB %d" % (sent // 1048576))
    log("FLOOD_END %d" % conn_id)
    c.close()


def serve(c, state):
    buf = b""
    subs = {}
    n = 0
    sent = 0
    c.settimeout(0.25)
    next_push = time.time() + 0.1
    conn_id = state["conns"] = state.get("conns", 0) + 1
    log("CONNECT %d" % conn_id)
    if scenario == "reject":
        c.close()
        return
    if scenario in ("floodpre", "flood", "burst"):
        flood(c, conn_id)
        return
    while True:
        try:
            d = c.recv(65536)
            if not d:
                log("CLOSED %d" % conn_id)
                return
            buf += d
        except socket.timeout:
            pass
        except OSError:
            return
        while b"\n" in buf:
            line, buf = buf.split(b"\n", 1)
            log("REQ " + line.decode(errors="replace"))
            try:
                m = json.loads(line)
            except Exception:
                continue
            op, rid = m.get("op"), m.get("id")
            if op == "hello":
                if scenario == "silent":
                    continue
                if scenario == "refuse":
                    send(c, {"v": 0, "id": rid, "error": {"code": "denied"}})
                elif scenario == "unsupported":
                    send(c, {"v": 0, "id": rid, "error": {"code": "unsupported_version"}})
                else:
                    send(c, {"v": 0, "id": rid, "ok": True, "daemonVersion": "0.0-test", "protocol": 0})
                if scenario == "garbage":
                    for junk in (b"not json\n", b"\n", b"\x00\xff\xfe\n", b"{\"v\":9,\"id\":1,\"ok\":true}\n", b"[1,2,3]\n",
                                 b"{\"v\":0,\"sub\":424242,\"topic\":\"x.one\",\"data\":{\"n\":666}}\n", b"{\"v\":0,\"id\":77,\"ok\":true}\n"):
                        send(c, junk)
            elif op == "subscribe":
                topic = m.get("topic")
                if topic == "x.denied":
                    send(c, {"v": 0, "id": rid, "error": {"code": "denied", "topic": topic}})
                elif topic == "x.locked" or scenario == "locked":
                    send(c, {"v": 0, "id": rid, "error": {"code": "locked", "topic": topic}})
                elif topic == "x.unknown":
                    send(c, {"v": 0, "id": rid, "error": {"code": "unknown_topic", "topic": topic}})
                else:
                    sub = len(subs) + 1 + 100 * conn_id
                    subs[sub] = (m.get("widget"), topic)
                    send(c, {"v": 0, "id": rid, "ok": True, "sub": sub})
            elif op == "unsubscribe":
                subs.pop(m.get("sub"), None)
                send(c, {"v": 0, "id": rid, "ok": True})
        if time.time() >= next_push and subs:
            next_push = time.time() + 0.2
            n += 1
            for sub, (w, t) in list(subs.items()):
                try:
                    if scenario == "fragment":
                        push(c, sub, t, n, chunk=5, data={"n": n, "name": "héllo ✓ \U0001f600 end"})
                        # two messages in one write
                        two = (json.dumps({"v": 0, "sub": sub, "topic": t, "ts": 1, "stale": False, "data": {"n": n + 1000}}) + "\n") * 2
                        send(c, two.encode())
                    elif scenario == "garbage":
                        send(c, b"{broken\n")
                        send(c, {"v": 0, "sub": sub, "topic": "x.other", "ts": 1, "stale": False, "data": {"n": 555}})   # wrong topic for this sub
                        push(c, sub, t, n, data={"n": n, "bad": float("1e308"), "nested": {"a": 1}, "constructor": 5})
                    elif scenario == "oversize":
                        if n == 2 and conn_id == 1:
                            send(c, b"{\"v\":0,\"sub\":%d,\"topic\":\"%s\",\"data\":{\"x\":\"" % (sub, t.encode()) + b"a" * 200000 + b"\"}}\n")
                        push(c, sub, t, n)
                    else:
                        push(c, sub, t, n, stale=(n % 4 == 0))
                    sent += 1
                except OSError:
                    return
            if scenario == "drop" and conn_id == 1 and n >= 3:
                log("DROPPING")
                c.close()
                state["drop"] = True
                return


def main():
    state = {}
    srv = listen()
    while True:
        srv.settimeout(0.5)
        try:
            c, _ = srv.accept()
        except socket.timeout:
            continue
        t = threading.Thread(target=serve, args=(c, state), daemon=True)
        t.start()
        if scenario == "drop":
            t.join()
            if state.get("drop"):
                srv.close()
                try:
                    os.unlink(path)
                except FileNotFoundError:
                    pass
                log("SERVER DOWN")
                time.sleep(1.5)
                srv = listen()
                state["drop"] = False
                log("SERVER UP")


main()
