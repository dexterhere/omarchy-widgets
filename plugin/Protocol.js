.pragma library
.import "Json.js" as J
.import "Layout.js" as L

// owd wire protocol v0 (docs/DAEMON_SPEC.md section 5), client side. Pure functions, no
// IO, so they are tested offscreen. Everything that arrives is treated as untrusted
// text: the daemon is the same user's process, but a bug or a swapped socket must not
// be able to inject anything but plain numbers and plain strings into a widget.

var MAX_LINE = 64 * 1024            // same cap as the daemon's MAX_LINE
var CLIENT_VERSION = "0.1.0"
var MAX_FIELDS = 64
var MAX_ITEMS = 50
var MAX_STRING = 4096
var CODES = ["bad_request", "denied", "unknown_topic", "provider_error", "rate_limited",
             "locked", "unsupported_version", "internal"]

// Provider health words (docs/DAEMON_SPEC.md "States"). A closed list: anything else is dropped,
// so nothing the daemon (or a swapped socket) sends can become display text through this field.
var HEALTH = ["locked", "no_credential", "unauthorized", "insufficient", "rate_limited",
              "expired_soon", "offline", "unavailable"]
function health(v) { return typeof v === "string" && HEALTH.indexOf(v) >= 0 ? v : "" }

function frame(obj) { return JSON.stringify(obj) + "\n" }

function hello(id) {
  return frame({ v: 0, id: id, op: "hello", client: "bridge", clientVersion: CLIENT_VERSION })
}

// Returns null instead of a frame when the widget id or topic is not well-formed, so a
// bad record can never put arbitrary text on the socket.
function subscribe(id, widget, topic) {
  if (!L.validId(widget) || typeof topic !== "string" || !L.TOPIC.test(topic)) return null
  return frame({ v: 0, id: id, op: "subscribe", widget: widget, topic: topic })
}

function unsubscribe(id, sub) {
  if (typeof sub !== "number" || Math.floor(sub) !== sub || sub < 0) return null
  return frame({ v: 0, id: id, op: "unsubscribe", sub: sub })
}

// Reconnect delay: 0.5 s doubling to a 10 s cap. A fixed 1 s retry loops forever against a
// peer that keeps refusing (SPIKE-S3). `rand` in [0,1) adds up to 20% jitter; pass 0.5 for none.
function backoffMs(attempt, rand) {
  var n = Math.max(0, Math.min(10, Math.floor(attempt) || 0))
  var base = Math.min(10000, 500 * Math.pow(2, n))
  var r = typeof rand === "number" ? rand : 0.5
  return Math.round(base * (0.9 + 0.2 * r))
}

// Keep only what a spec widget can draw: finite numbers, bounded plain strings, short
// arrays of those. Keys must look like layout fields. Nested objects, booleans, null and
// non-finite numbers are dropped (the node then shows its placeholder).
function cleanData(d) {
  var out = {}
  if (!J.isObj(d)) return out
  var keys = Object.keys(d), kept = 0
  for (var i = 0; i < keys.length && kept < MAX_FIELDS; i++) {
    var k = keys[i], v = d[k]
    if (!L.FIELD.test(k)) continue
    if (typeof v === "number") { if (isFinite(v)) { out[k] = v; kept++ } }
    else if (typeof v === "string") { out[k] = v.length > MAX_STRING ? v.substring(0, MAX_STRING) : v; kept++ }
    else if (Array.isArray(v)) {
      var arr = []
      for (var j = 0; j < v.length && arr.length < MAX_ITEMS; j++) {
        if (typeof v[j] === "number" && isFinite(v[j])) arr.push(v[j])
        else if (typeof v[j] === "string") arr.push(v[j].length > MAX_STRING ? v[j].substring(0, MAX_STRING) : v[j])
      }
      out[k] = arr; kept++
    }
  }
  return out
}

function isUint(n) { return typeof n === "number" && isFinite(n) && Math.floor(n) === n && n >= 0 && n <= 9007199254740991 }

// One line from the daemon -> an event. Never throws.
//   { kind: "ignore" }                              empty line
//   { kind: "oversize" }  { kind: "malformed" }     counted, otherwise ignored
//   { kind: "ok", id, sub?, daemonVersion?, protocol? }
//   { kind: "error", id, code, topic }
//   { kind: "push", sub, topic, ts, stale, data }
//   { kind: "state", sub, topic, state }            push with a state and no data: not an update
function parseLine(line) {
  if (typeof line !== "string") return { kind: "malformed" }
  if (line.length === 0) return { kind: "ignore" }
  if (line.length > MAX_LINE) return { kind: "oversize" }
  var r = J.parse(line, MAX_LINE, 8)
  if (!r.ok || !J.isObj(r.value)) return { kind: "malformed" }
  var m = r.value
  if (J.own(m, "v") !== 0) return { kind: "malformed" }
  var id = J.own(m, "id"), sub = J.own(m, "sub"), topic = J.own(m, "topic")
  if (id === undefined && isUint(sub) && typeof topic === "string") {
    if (!L.TOPIC.test(topic)) return { kind: "malformed" }
    var ts = J.own(m, "ts")
    // A push with a state and no data key says why there is no value (DAEMON_SPEC "States"). It
    // is not an empty update: the bridge must keep what it has.
    if (J.own(m, "data") === undefined && typeof J.own(m, "state") === "string")
      return { kind: "state", sub: sub, topic: topic, state: health(J.own(m, "state")) }
    return { kind: "push", sub: sub, topic: topic, ts: isUint(ts) ? ts : 0,
             stale: J.own(m, "stale") === true, data: cleanData(J.own(m, "data")),
             health: health(J.own(m, "state")) }
  }
  if (!isUint(id)) return { kind: "malformed" }
  var e = J.own(m, "error")
  if (e !== undefined) {
    if (!J.isObj(e)) return { kind: "malformed" }
    var code = J.own(e, "code"), t = J.own(e, "topic")
    return { kind: "error", id: id, code: CODES.indexOf(code) >= 0 ? code : "internal",
             topic: typeof t === "string" && L.TOPIC.test(t) ? t : "", health: health(J.own(e, "state")) }
  }
  if (J.own(m, "ok") !== true) return { kind: "malformed" }
  var out = { kind: "ok", id: id }
  if (isUint(sub)) out.sub = sub
  var dv = J.own(m, "daemonVersion")
  if (typeof dv === "string") out.daemonVersion = L.plainText(dv, 32)
  var p = J.own(m, "protocol")
  if (isUint(p)) out.protocol = p
  return out
}
