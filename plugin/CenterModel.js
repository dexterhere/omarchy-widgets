.pragma library
.import "Layout.js" as L

// What the Widget Center shows, as plain data (docs/WIDGET_CENTER.md). Pure functions, no IO:
// nothing here runs a command or touches a file. Every string that reaches the screen has been
// through L.plainText, and every record comes from Layout.loadWidget, so a hostile widget.json
// can change the words on screen but never anything else.

var ROWS_MAX = 200          // rows shown; the rest are counted ("N more not shown")
var LINES_MAX = 8           // grant lines and health lines per row; the rest are "+N more"

var HEALTH_LABEL = {
  locked: "locked (ow vault unlock)", no_credential: "no credential", unauthorized: "unauthorized",
  insufficient: "insufficient permission", rate_limited: "rate limited", expired_soon: "expired soon",
  offline: "offline", unavailable: "unavailable"
}
var LINK_LABEL = { connected: "daemon connected", connecting: "daemon connecting",
                   unavailable: "daemon not running", denied: "daemon refused this client" }

// Own-property lookup only: data-derived keys such as "constructor" or "toString" must not find a function.
function own(table, key) {
  return typeof key === "string" && Object.prototype.hasOwnProperty.call(table, key) ? table[key] : undefined
}

function capLines(lines) {
  if (lines.length <= LINES_MAX) return lines
  var out = lines.slice(0, LINES_MAX - 1)
  out.push("+" + (lines.length - (LINES_MAX - 1)) + " more")
  return out
}

// One topic's health word. A provider state from the daemon wins over the bridge's own state.
function topicHealth(entry, link) {
  if (link !== "connected") return "daemon not running"
  if (!entry || typeof entry !== "object") return "waiting for data"
  var word = own(HEALTH_LABEL, entry.health)
  if (word !== undefined) return word
  switch (entry.state) {
  case "live": return "ok"
  case "stale": return "stale"
  case "denied": return "no access"
  case "locked": return HEALTH_LABEL.locked
  case "unknown": return "not served"
  case "limited": case "error": return "daemon error"
  default: return "waiting for data"
  }
}

// Every character outside [a-z0-9.-] as \uXXXX, so what is printed is plain ASCII and says exactly what it holds.
function escapeId(id) {
  var s = typeof id === "string" ? id.substring(0, 60) : ""
  var out = ""
  for (var i = 0; i < s.length; i++) {
    var ch = s.charAt(i)
    if (/[a-z0-9.-]/.test(ch)) out += ch
    else { var h = s.charCodeAt(i).toString(16); out += "\\u" + "0000".substring(h.length) + h }
  }
  return out + (typeof id === "string" && id.length > 60 ? "..." : "")
}

// The command the user types in a terminal. Only an id that passes the id pattern (so it holds just
// [a-z0-9.-]) gets a command, and it is printed as is. Anything else gets NO command at all, only
// its escaped form: a display-cleaned or truncated id could name a different widget. It is text on
// screen; nothing in the plugin executes it.
function removeCommand(id) {
  if (L.validId(id)) return "ow remove " + id
  return "id not valid, nothing to run: \"" + escapeId(id) + "\""
}

function grantLines(rec) {
  var out = []
  if (rec.grants === null || rec.grants === undefined) return ["no grant list recorded"]
  var seen = {}
  for (var i = 0; i < rec.grants.length && out.length < 4 * LINES_MAX; i++) {
    var g = rec.grants[i], fields = [], fseen = {}
    for (var f = 0; f < g.fields.length; f++) if (!fseen["k" + g.fields[f]]) { fseen["k" + g.fields[f]] = 1; fields.push(g.fields[f]) }
    var suffix = fields.length ? " (fields: " + fields.join(", ") + ")" : ""
    for (var t = 0; t < g.topics.length; t++) {
      if (seen["k" + g.topics[t]]) continue
      seen["k" + g.topics[t]] = 1
      out.push("granted " + g.topics[t] + suffix)
    }
  }
  var declared = rec.declared || []
  for (var d = 0; d < declared.length && out.length < 4 * LINES_MAX; d++) if (!seen["k" + declared[d]]) out.push("declared, not granted: " + declared[d])
  if (out.length === 0) out.push("no topics")
  return capLines(out)
}

// records: Store.list; entries and link: Bridge.entries and Bridge.link.
function rows(records, entries, link) {
  var out = []
  if (!Array.isArray(records)) return out
  for (var i = 0; i < records.length && out.length < ROWS_MAX; i++) {
    var r = records[i]
    if (!r || typeof r !== "object") continue
    var id = L.validId(r.id) ? r.id : escapeId(r.id)
    var row = { id: id, command: removeCommand(r.id), ok: r.ok === true, name: "", version: "", status: "", tier: "",
                grants: [], health: [], summary: "" }
    if (r.ok !== true) {
      row.summary = "could not be loaded (" + L.plainText(typeof r.reason === "string" ? r.reason : "", 24) + ")"
      row.grants = ["unknown"]
      out.push(row); continue
    }
    row.name = r.name; row.version = r.version; row.status = r.status; row.tier = "spec"
    row.grants = grantLines(r)
    var worst = "", topics = r.topics || []
    for (var t = 0; t < topics.length && t < 4 * LINES_MAX; t++) {
      var h = topicHealth(entries && entries[L.entryKey(r.id, topics[t])], link)
      row.health.push(topics[t] + ": " + h)
      if (h !== "ok" && worst === "") worst = h
    }
    for (var u = 0; u < (r.ungranted || []).length && u < 4 * LINES_MAX; u++) { row.health.push(r.ungranted[u] + ": no access"); if (worst === "") worst = "no access" }
    row.health = capLines(row.health)
    row.summary = topics.length === 0 && (r.ungranted || []).length === 0 ? "no data topics" : (worst === "" ? "ok" : worst)
    out.push(row)
  }
  return out
}

function linkLabel(link) { var w = own(LINK_LABEL, link); return w !== undefined ? w : "daemon not running" }

// How many installed widgets the panel is not showing: rows cut at ROWS_MAX plus directories the store did not load.
function hiddenCount(recordCount, rowCount, storeOverflow) {
  return Math.max(0, (recordCount | 0) - (rowCount | 0)) + Math.max(0, storeOverflow | 0)
}
