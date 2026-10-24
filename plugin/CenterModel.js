.pragma library
.import "Layout.js" as L

// What the Widget Center shows, as plain data (docs/WIDGET_CENTER.md). Pure functions, no IO:
// nothing here runs a command or touches a file. Every string that reaches the screen has been
// through L.plainText, and every record comes from Layout.loadWidget, so a hostile widget.json
// can change the words on screen but never anything else.

var HEALTH_LABEL = {
  locked: "locked (ow vault unlock)", no_credential: "no credential", unauthorized: "unauthorized",
  insufficient: "insufficient permission", rate_limited: "rate limited", expired_soon: "expired soon",
  offline: "offline", unavailable: "unavailable"
}
var LINK_LABEL = { connected: "daemon connected", connecting: "daemon connecting",
                   unavailable: "daemon not running", denied: "daemon refused this client" }

// One topic's health word. A provider state from the daemon wins over the bridge's own state.
function topicHealth(entry, link) {
  if (link !== "connected") return "daemon not running"
  if (!entry || typeof entry !== "object") return "waiting for data"
  if (typeof entry.health === "string" && HEALTH_LABEL[entry.health] !== undefined && entry.health !== "") return HEALTH_LABEL[entry.health]
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

// The command the user types in a terminal. A valid id has only [a-z0-9.-], so it is printed
// as is; anything else is made printable and single-quoted, so what is shown can never be read
// as more than one word. It is text on screen; nothing in the plugin executes it.
function removeCommand(id) {
  if (L.validId(id)) return "ow remove " + id
  var s = L.plainText(typeof id === "string" ? id : "", 100)
  return "ow remove '" + s.replace(/'/g, "'\\''") + "'"
}

function grantLines(rec) {
  var out = []
  if (rec.grants === null || rec.grants === undefined) return ["no grant list recorded"]
  var seen = {}
  for (var i = 0; i < rec.grants.length; i++) {
    var g = rec.grants[i], f = g.fields.length ? " (fields: " + g.fields.join(", ") + ")" : ""
    for (var t = 0; t < g.topics.length; t++) { seen[g.topics[t]] = 1; out.push("granted " + g.topics[t] + f) }
  }
  var declared = rec.declared || []
  for (var d = 0; d < declared.length; d++) if (!seen[declared[d]]) out.push("declared, not granted: " + declared[d])
  if (out.length === 0) out.push("no topics")
  return out
}

// records: Store.list; entries and link: Bridge.entries and Bridge.link.
function rows(records, entries, link) {
  var out = []
  if (!Array.isArray(records)) return out
  for (var i = 0; i < records.length && i < 200; i++) {
    var r = records[i]
    if (!r || typeof r !== "object") continue
    var id = L.plainText(typeof r.id === "string" ? r.id : "", 100)
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
    for (var t = 0; t < topics.length; t++) {
      var h = topicHealth(entries && entries[L.entryKey(r.id, topics[t])], link)
      row.health.push(topics[t] + ": " + h)
      if (h !== "ok" && worst === "") worst = h
    }
    for (var u = 0; u < (r.ungranted || []).length; u++) { row.health.push(r.ungranted[u] + ": no access"); if (worst === "") worst = "no access" }
    row.summary = topics.length === 0 && (r.ungranted || []).length === 0 ? "no data topics" : (worst === "" ? "ok" : worst)
    out.push(row)
  }
  return out
}

function linkLabel(link) { return LINK_LABEL[link] !== undefined ? LINK_LABEL[link] : "daemon not running" }
