import QtQuick
import Quickshell
import Quickshell.Io
import "Protocol.js" as P
import "Layout.js" as L

// Client for the owd daemon (docs/DAEMON_SPEC.md section 5): newline-delimited JSON over the
// user's Unix socket. The socket path is built here from $XDG_RUNTIME_DIR and nothing else.
// No credential of any kind passes through this file: it sends hello, subscribe and
// unsubscribe, and keeps only what the daemon pushes (numbers and short strings, see
// Protocol.cleanData). The daemon, not this client, decides who may read what.
//
// link: "connecting" | "connected" | "unavailable" (no daemon / lost it) | "denied" (the
// daemon closed or refused before answering hello). Per topic (entries): "pending" "live"
// "stale" "denied" "locked" "unknown" "limited" "error".
Item {
  id: root

  // The only place the path is built: the user's runtime dir, as in docs/DAEMON_SPEC.md section 4.
  // Never taken from a widget file. (`socketPath` is overridable only so tests can point at a fake.)
  readonly property string defaultSocketPath: {
    const dir = Quickshell.env("XDG_RUNTIME_DIR")
    return dir && dir.length > 0 ? dir + "/owd/owd.sock" : ""
  }
  property string socketPath: defaultSocketPath
  property var wanted: []                       // [{widget, topic}] chosen by the store, from validated records
  property bool enabled: true

  property string link: "connecting"
  property var entries: ({})                    // "<widget> <topic>" -> { state, data, ts }
  property string daemonVersion: ""
  property int malformed: 0                     // lines dropped on this connection
  property int oversize: 0                      // over-long lines on this connection
  property int oversizeTotal: 0                 // ... since the plugin started
  property int connects: 0                      // successful hellos, for tests and diagnostics
  property int pushes: 0                        // accepted pushes

  readonly property int maxSubs: 30             // the daemon allows 32 per connection
  property int helloTimeoutMs: 3000
  property int retryLockedMs: 10000

  // ---- internals (plain JS state, not bound to)
  property int attempt: 0
  property int nextId: 2
  property bool helloOk: false
  property var pending: ({})                    // request id -> { kind, key }
  property var subs: ({})                       // sub id -> key
  property var keys: ({})                       // key -> { widget, topic, sub, asked }

  function read(widget, topic) {
    const e = entries[L.entryKey(widget, topic)]
    return e === undefined ? null : e
  }

  // ---------------------------------------------------------------- connection
  Socket {
    id: sock
    path: root.socketPath
    connected: false
    onConnectedChanged: root.onConnection(connected)
    onError: function (e) { root.onSocketError() }
    // An EMPTY marker makes Quickshell hand over every chunk as it arrives (measured: about 230 KB per
    // read, resident memory flat under a never-ending stream). With the marker "\n" it buffers until a
    // newline, with no limit: a peer that never sends one grew the shell to 6 GB in 9 s. So framing is
    // done in onChunk() below, with a hard cap.
    parser: SplitParser {
      splitMarker: ""
      onRead: function (chunk) { root.onChunk(chunk) }
    }
  }

  Timer {
    id: reconnectTimer
    repeat: false
    onTriggered: root.connectNow()
  }
  Timer {
    id: helloTimer
    interval: root.helloTimeoutMs
    repeat: false
    onTriggered: { if (!root.helloOk) root.drop("denied") }
  }
  Timer {          // a connection that lasted this long counts as healthy: reset the backoff
    id: stableTimer
    interval: 10000
    repeat: false
    onTriggered: root.attempt = 0
  }
  Timer {          // locked and rate-limited topics are asked for again from time to time
    id: retryTimer
    interval: root.retryLockedMs
    repeat: true
    running: root.helloOk
    onTriggered: root.retryTopics()
  }

  function connectNow() {
    if (!enabled || socketPath === "") { link = "unavailable"; schedule(); return }
    link = "connecting"
    sock.connected = false          // Socket does not retry by itself: toggle to attempt again (SPIKE-S3)
    sock.connected = true
  }

  function schedule() {
    if (reconnectTimer.running) return
    reconnectTimer.interval = P.backoffMs(attempt, Math.random())
    attempt++
    reconnectTimer.start()
  }

  property bool wasUp: false

  function onConnection(up) {
    if (up) {
      wasUp = true
      partial = ""
      bytesThisSecond = 0
      helloOk = false
      malformed = 0
      oversize = 0
      pending = ({})
      subs = ({})
      for (const k in keys) { keys[k].sub = -1; keys[k].asked = false }
      send(P.hello(1))
      helloTimer.restart()
      return
    }
    if (!wasUp) return               // an attempt that never connected: onSocketError handles it
    wasUp = false
    partial = ""
    helloTimer.stop()
    stableTimer.stop()
    const hadHello = helloOk
    helloOk = false
    // the connection's subscriptions are gone; the last data stays so tiles can show it dimmed
    for (const k in keys) { keys[k].sub = -1; keys[k].asked = false }
    if (hadHello) link = "unavailable"
    else if (link === "connecting") link = "denied"    // the daemon closed before answering hello
    schedule()
  }

  function onSocketError() {
    if (!wasUp) {                    // could not connect at all: no daemon, or no socket yet
      link = "unavailable"
      schedule()
    }
  }

  // Close this connection on purpose (hello refused, flood) and back off.
  function drop(newLink) {
    link = newLink
    helloTimer.stop()
    if (sock.connected) sock.connected = false     // onConnection(false) schedules the retry
    else schedule()
  }

  function send(frame) {
    if (frame === null || !sock.connected) return false
    sock.write(frame)
    sock.flush()
    return true
  }

  // ---------------------------------------------------------------- incoming
  property string partial: ""                   // bytes of an unfinished line, never longer than P.MAX_LINE
  property int bytesThisSecond: 0
  readonly property int maxBytesPerSecond: 4 * 1024 * 1024    // a daemon pushes a few KB per second

  Timer {
    id: rateTimer
    interval: 1000
    repeat: true
    running: root.wasUp
    onTriggered: root.bytesThisSecond = 0
  }

  // A chunk may hold part of a line, several lines, or both. Memory is bounded: a chunk is at most a
  // few hundred KB and what is kept between chunks is cut off at MAX_LINE, which drops the connection.
  // (A multi-byte character split exactly at a chunk boundary is decoded per chunk by Quickshell and
  // shows as a replacement character; the daemon's data is ASCII in practice.)
  function onChunk(chunk) {
    if (!wasUp || typeof chunk !== "string") return
    bytesThisSecond += chunk.length
    if (bytesThisSecond > maxBytesPerSecond) { oversize++; oversizeTotal++; partial = ""; drop("unavailable"); return }
    let buf = partial + chunk
    partial = ""
    let nl = buf.indexOf("\n")
    while (nl >= 0) {
      const line = buf.substring(0, nl)
      buf = buf.substring(nl + 1)
      onLine(line)
      if (!wasUp) return                        // onLine may have dropped the connection
      nl = buf.indexOf("\n")
    }
    if (buf.length > P.MAX_LINE) { oversize++; oversizeTotal++; drop("unavailable"); return }   // never-ending line
    partial = buf
  }

  function onLine(line) {
    const ev = P.parseLine(line)
    switch (ev.kind) {
    case "ignore": return
    case "oversize":
      oversize++
      oversizeTotal++
      if (oversize > 5) drop("unavailable")
      return
    case "malformed":
      malformed++
      if (malformed > 50) drop("unavailable")
      return
    case "ok": return onOk(ev)
    case "error": return onError(ev)
    case "push": return onPush(ev)
    }
  }

  function onOk(ev) {
    const p = pending[ev.id]
    if (ev.id === 1 && !helloOk) {
      helloOk = true
      daemonVersion = ev.daemonVersion !== undefined ? ev.daemonVersion : ""
      helloTimer.stop()
      link = "connected"
      connects++
      stableTimer.restart()
      reconcile()
      return
    }
    if (p === undefined) return
    delete pending[ev.id]
    if (p.kind === "sub" && ev.sub !== undefined) {
      const k = keys[p.key]
      if (k === undefined) { send(P.unsubscribe(nextId++, ev.sub)); return }   // no longer wanted
      k.sub = ev.sub
      subs[ev.sub] = p.key
    }
  }

  function onError(ev) {
    if (ev.id === 1 && !helloOk) { drop(ev.code === "denied" ? "denied" : "unavailable"); return }
    const p = pending[ev.id]
    if (p === undefined) return
    delete pending[ev.id]
    if (p.kind !== "sub") return
    const state = ev.code === "denied" ? "denied" : ev.code === "locked" ? "locked"
      : ev.code === "unknown_topic" ? "unknown" : ev.code === "rate_limited" ? "limited" : "error"
    setEntry(p.key, { state: state })
    // denied, unknown and plain errors are final until the next connection; locked and
    // rate-limited topics are asked again by retryTopics()
    const k = keys[p.key]
    if (k !== undefined) k.asked = !(state === "locked" || state === "limited")
  }

  function onPush(ev) {
    const key = subs[ev.sub]
    if (key === undefined) return
    const k = keys[key]
    if (k === undefined || k.topic !== ev.topic) return        // a push must match what we asked for
    pushes++
    setEntry(key, { state: ev.stale ? "stale" : "live", data: ev.data, ts: ev.ts })
  }

  function setEntry(key, value) {
    const copy = Object.assign({}, entries)      // new object: bindings that read `entries` re-evaluate
    copy[key] = value
    entries = copy
  }

  // ---------------------------------------------------------------- subscriptions
  onWantedChanged: reconcile()
  onEnabledChanged: { if (enabled) connectNow(); else drop("unavailable") }

  function reconcile() {
    const want = {}
    let count = 0
    for (let i = 0; i < wanted.length; i++) {
      const w = wanted[i]
      if (typeof w !== "object" || w === null) continue
      if (!L.validId(w.widget) || typeof w.topic !== "string" || !L.TOPIC.test(w.topic)) continue
      const key = L.entryKey(w.widget, w.topic)
      if (want[key] !== undefined) continue
      want[key] = { widget: w.widget, topic: w.topic }
      count++
    }
    // drop what is no longer wanted
    const copy = Object.assign({}, entries)
    for (const key in keys) {
      if (want[key] === undefined) {
        if (keys[key].sub >= 0) send(P.unsubscribe(nextId++, keys[key].sub))
        delete subs[keys[key].sub]
        delete keys[key]
        delete copy[key]
      }
    }
    let n = Object.keys(keys).length
    for (const key in want) {
      if (keys[key] !== undefined) continue
      if (n >= maxSubs) { copy[key] = { state: "limited" }; continue }
      keys[key] = { widget: want[key].widget, topic: want[key].topic, sub: -1, asked: false }
      copy[key] = { state: "pending" }
      n++
    }
    entries = copy
    if (helloOk) askAll()
  }

  function askAll() {
    for (const key in keys) {
      const k = keys[key]
      if (k.sub >= 0 || k.asked) continue
      const id = nextId++
      if (send(P.subscribe(id, k.widget, k.topic))) { k.asked = true; pending[id] = { kind: "sub", key: key } }
    }
  }

  function retryTopics() {
    for (const key in keys) {
      const e = entries[key]
      if (e !== undefined && (e.state === "locked" || e.state === "limited")) keys[key].asked = false
    }
    askAll()
  }

  Component.onCompleted: connectNow()
  Component.onDestruction: { if (sock.connected) sock.connected = false }
}
