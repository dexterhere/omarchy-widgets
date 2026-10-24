.pragma library
.import "Json.js" as J

// layout.json v0 and widget file validation, plus the pure helpers the renderer uses.
// A port of the registry's src/lib/registry/layout.ts (same node types, keys, roles,
// limits). The installer (ow) already validated these files; this is the second line:
// the shell never trusts a file because ow wrote it. No function here evaluates,
// loads or builds a path from widget data.

var LIMITS = { depth: 8, nodes: 200, list: 50, string: 120, children: 20, bytes: 64 * 1024,
               fileBytes: 256 * 1024, depthJson: 40 }
var ROLES = ["accent", "foreground", "secondary", "muted"]
var SIZES = ["sm", "md", "lg", "xl"]
var GAPS = ["sm", "md", "lg"]
var ICONS = ["cpu", "memory", "battery", "clock", "list", "git", "container", "disk",
             "network", "bolt", "calendar", "check"]
var TOPIC = /^[a-z][a-z0-9-]*(\.[a-z0-9][a-z0-9-]*)+$/
var FIELD = /^[a-zA-Z][a-zA-Z0-9_.-]{0,39}$/
var WIDGET_ID = /^[a-z0-9][a-z0-9.-]{0,99}$/
var CLASS_NAME = /^[a-z0-9][a-z0-9-]{0,19}$/
var VARIANT_NAME = /^[a-zA-Z0-9][a-zA-Z0-9_-]{0,39}$/
var HTMLISH = /<\s{0,8}\/?\s{0,8}[a-z!?]/i

var KEYS = {
  stack: ["type", "gap", "children"],
  row: ["type", "gap", "children"],
  grid: ["type", "columns", "gap", "children"],
  text: ["type", "text", "bind", "size", "color"],
  badge: ["type", "text", "bind", "size", "color"],
  value: ["type", "bind", "unit", "size", "color"],
  bar: ["type", "bind", "color"],
  ring: ["type", "bind", "color"],
  sparkline: ["type", "bind", "color"],
  list: ["type", "bind", "max", "size", "color"],
  icon: ["type", "name", "size", "color"],
  spacer: ["type"],
  divider: ["type"]
}
var BOUND = { value: 1, text: 1, badge: 1, bar: 1, ring: 1, sparkline: 1, list: 1 }

function isKnownType(t) { return typeof t === "string" && J.has(KEYS, t) }
function oneOf(list, v) { return list.indexOf(v) >= 0 }

// ---------------------------------------------------------------- plain text

// Characters that must never be drawn: controls, bidi controls (RLO, LRE, isolates),
// zero-width and other format characters, private use, line and paragraph
// separators, characters that render blank, variation selectors (except U+FE0F),
// and the astral tag block. Unpaired surrogates are handled by the caller.
function badUnit(c) {
  return c < 0x20 || (c >= 0x600 && c <= 0x605) || c === 0x6dd || c === 0x70f || c === 0x8e2 || c === 0xfffc || (c >= 0x7f && c <= 0x9f) || c === 0xad || c === 0x34f || c === 0x61c
    || c === 0x115f || c === 0x1160 || c === 0x17b4 || c === 0x17b5
    || (c >= 0x180b && c <= 0x180f) || (c >= 0x200b && c <= 0x200f)
    || (c >= 0x2028 && c <= 0x202e) || (c >= 0x2060 && c <= 0x206f)
    || c === 0x2800 || c === 0x3164 || (c >= 0xfe00 && c <= 0xfe0e) || c === 0xfeff
    || c === 0xffa0 || (c >= 0xfff0 && c <= 0xfffb) || (c >= 0xe000 && c <= 0xf8ff)
}
// Format characters outside the BMP (musical, Brahmi, Egyptian and shorthand format controls) and the
// tag block, given as code points; also private-use planes 15 and 16.
function badAstral(cp) {
  return (cp >= 0x1d173 && cp <= 0x1d17a) || cp === 0x110bd || cp === 0x110cd || (cp >= 0x13430 && cp <= 0x1343f)
    || (cp >= 0x1bca0 && cp <= 0x1bca3) || (cp >= 0xe0000 && cp <= 0xe0fff) || cp >= 0xf0000
}
function pairCode(hi, lo) { return (hi - 0xd800) * 0x400 + (lo - 0xdc00) + 0x10000 }

function isCombining(c) {
  return (c >= 0x300 && c <= 0x36f) || (c >= 0x1ab0 && c <= 0x1aff) || (c >= 0x1dc0 && c <= 0x1dff)
    || (c >= 0x20d0 && c <= 0x20ff) || (c >= 0xfe20 && c <= 0xfe2f)
}

// Returns true if s contains nothing from the sets above, no HTML-looking markup,
// is NFC, and has no run of 4 or more combining marks.
function isPlain(s) {
  if (typeof s !== "string") return false
  var run = 0
  for (var i = 0; i < s.length; i++) {
    var c = s.charCodeAt(i)
    if (badUnit(c)) return false
    if (c >= 0xd800 && c <= 0xdbff) {
      var d = i + 1 < s.length ? s.charCodeAt(i + 1) : 0
      if (d < 0xdc00 || d > 0xdfff) return false
      if (badAstral(pairCode(c, d))) return false
      i++; run = 0; continue
    }
    if (c >= 0xdc00 && c <= 0xdfff) return false
    if (isCombining(c)) { if (++run >= 4) return false } else run = 0
  }
  if (HTMLISH.test(s)) return false
  try { if (s !== s.normalize("NFC")) return false } catch (e) { /* engine without normalize */ }
  return true
}

function hasVisible(s) { return /[^\s]/.test(s) }

// Display sanitiser for strings that arrive at run time (daemon data, manifest names).
// Never throws, always returns a bounded string: bad code units are dropped, markup is
// kept as inert characters (the Text item is PlainText), the result is cut to max units.
function plainText(v, max) {
  if (typeof v !== "string") return ""
  var lim = max > 0 ? max : LIMITS.string
  var s = v.length > lim * 4 ? v.substring(0, lim * 4) : v
  var out = "", run = 0
  for (var i = 0; i < s.length && out.length < lim; i++) {
    var c = s.charCodeAt(i)
    if (c === 9 || c === 10 || c === 13) { out += " "; run = 0; continue }
    if (badUnit(c)) continue
    if (c >= 0xd800 && c <= 0xdbff) {
      var d = i + 1 < s.length ? s.charCodeAt(i + 1) : 0
      if (d >= 0xdc00 && d <= 0xdfff) { if (!badAstral(pairCode(c, d))) { out += s.charAt(i) + s.charAt(i + 1); run = 0 } i++ }
      continue
    }
    if (c >= 0xdc00 && c <= 0xdfff) continue
    if (isCombining(c)) { if (++run > 3) continue } else run = 0       // at most 3 stacked marks (the registry refuses 4)
    out += s.charAt(i)
  }
  return out
}

// ---------------------------------------------------------------- layout tree

// -> { ok: true, binds: [{type, bind}] } | { ok: false, errors: [...] }
function validateLayout(input, variants) {
  var errors = [], binds = [], nodes = 0
  var names = variants || []
  function err(m) { if (errors.length < 20) errors.push(m) }

  function bindOk(v, at) {
    if (!J.isObj(v) || Object.keys(v).some(function (k) { return k !== "topic" && k !== "field" })) {
      err(at + ".bind must be {topic, field}"); return false
    }
    var t = J.own(v, "topic"), f = J.own(v, "field")
    if (typeof t !== "string" || !TOPIC.test(t) || typeof f !== "string" || !FIELD.test(f)) {
      err(at + ".bind has an invalid topic or field"); return false
    }
    return true
  }

  function walk(n, depth, at) {
    if (++nodes > LIMITS.nodes) return err("more than " + LIMITS.nodes + " nodes")
    if (depth > LIMITS.depth) return err(at + ": deeper than " + LIMITS.depth)
    if (!J.isObj(n)) return err(at + " must be an object")
    var type = J.own(n, "type")
    if (!isKnownType(type)) return err(at + ".type is not a known node")
    Object.keys(n).forEach(function (k) {
      if (KEYS[type].indexOf(k) < 0) err(at + ": unknown property")
    })
    var c = J.own(n, "color"), s = J.own(n, "size"), g = J.own(n, "gap")
    if (c !== undefined && !oneOf(ROLES, c)) err(at + ".color must be a role")
    if (s !== undefined && !oneOf(SIZES, s)) err(at + ".size is not a known size")
    if (g !== undefined && !oneOf(GAPS, g)) err(at + ".gap is not a known gap")
    if (type === "stack" || type === "row" || type === "grid") {
      var ch = J.own(n, "children")
      if (!Array.isArray(ch) || ch.length < 1 || ch.length > LIMITS.children)
        return err(at + ".children must hold 1 to " + LIMITS.children + " nodes")
      if (type === "grid" && [2, 3, 4].indexOf(J.own(n, "columns")) < 0) err(at + ".columns must be 2, 3 or 4")
      for (var i = 0; i < ch.length; i++) walk(ch[i], depth + 1, at + ".children[" + i + "]")
      return
    }
    var bind = J.own(n, "bind")
    if (J.has(BOUND, type)) {
      if (type === "text" || type === "badge") {
        var text = J.own(n, "text")
        if ((text === undefined) === (bind === undefined)) err(at + " needs exactly one of text or bind")
        if (text !== undefined && (typeof text !== "string" || !hasVisible(text)
            || text.length > LIMITS.string || !isPlain(text)))
          err(at + ".text must be plain text up to " + LIMITS.string + " characters")
      } else if (bind === undefined) err(at + " needs a bind")
      if (bind !== undefined && bindOk(bind, at)) binds.push({ type: type, bind: bind })
    }
    if (type === "value") {
      var u = J.own(n, "unit")
      if (u !== undefined && (typeof u !== "string" || u.length > 4 || !isPlain(u)))
        err(at + ".unit must be plain text up to 4 characters")
    }
    if (type === "list") {
      var m = J.own(n, "max")
      if (m !== undefined && (typeof m !== "number" || Math.floor(m) !== m || m < 1 || m > LIMITS.list))
        err(at + ".max must be 1 to " + LIMITS.list)
    }
    if (type === "icon" && !oneOf(ICONS, J.own(n, "name"))) err(at + ".name is not a known icon")
  }

  if (!J.isObj(input)) return { ok: false, errors: ["layout.json must be an object"] }
  Object.keys(input).forEach(function (k) {
    if (["schemaVersion", "root", "variants"].indexOf(k) < 0) err("unknown property")
  })
  if (J.own(input, "schemaVersion") !== 0) err("schemaVersion must be 0")
  walk(J.own(input, "root"), 1, "root")
  var vs = J.own(input, "variants")
  if (vs !== undefined) {
    if (!J.isObj(vs)) err("variants must be an object")
    else Object.keys(vs).forEach(function (name) {
      if (names.indexOf(name) < 0) err("a variant is not declared in widget.json")
      walk(vs[name], 1, "variants")
    })
  }
  return errors.length ? { ok: false, errors: errors } : { ok: true, binds: binds }
}

// Cheap guard the renderer runs on a tree it is handed directly: within the depth and
// node limits, children capped. Iterative, bounded by the node limit plus one.
function treeFits(root) {
  var stack = [{ n: root, d: 1 }], count = 0
  while (stack.length) {
    var it = stack.pop()
    if (++count > LIMITS.nodes || it.d > LIMITS.depth) return false
    if (!J.isObj(it.n)) continue                  // not a node at all: drawn as a fixed card, no children
    var ch = J.own(it.n, "children")
    if (ch === undefined) continue
    if (!Array.isArray(ch) || ch.length > LIMITS.children) return false
    for (var i = 0; i < ch.length; i++) stack.push({ n: ch[i], d: it.d + 1 })
  }
  return true
}

// ---------------------------------------------------------------- widget files

function validId(id) {
  return typeof id === "string" && WIDGET_ID.test(id) && id.indexOf("..") < 0 && id.charAt(id.length - 1) !== "."
}

function pair(v) {
  if (!Array.isArray(v) || v.length !== 2) return null
  for (var i = 0; i < 2; i++) if (typeof v[i] !== "number" || !isFinite(v[i]) || v[i] < 1 || v[i] > 8192) return null
  return [Math.floor(v[0]), Math.floor(v[1])]
}

// widget.json, only the parts that affect drawing and subscribing.
function checkWidgetJson(w, id) {
  if (!J.isObj(w)) return "widget"
  if (J.own(w, "schemaVersion") !== 0) return "widget"
  if (J.own(w, "id") !== id) return "widget"
  if (J.own(w, "tier") !== "spec") return "tier"            // code widgets are never drawn here
  var caps = J.own(w, "capabilities")
  if (caps !== undefined) {
    if (!J.isObj(caps)) return "widget"
    var bad = ["network", "files", "spawn"].some(function (k) {
      var a = J.own(caps, k); return a !== undefined && !(Array.isArray(a) && a.length === 0)
    })
    if (bad) return "policy"
  }
  var provs = J.own(w, "providers")
  if (!Array.isArray(provs) || provs.length > 8) return "widget"
  var topics = {}, fields = {}
  for (var i = 0; i < provs.length; i++) {
    var p = provs[i]
    if (!J.isObj(p) || !Array.isArray(p.topics) || !Array.isArray(p.fields)
        || p.topics.length > 16 || p.fields.length > 40) return "widget"
    for (var a = 0; a < p.topics.length; a++) {
      if (typeof p.topics[a] !== "string" || !TOPIC.test(p.topics[a])) return "widget"
      topics[p.topics[a]] = 1
    }
    for (var b = 0; b < p.fields.length; b++) {
      if (typeof p.fields[b] !== "string" || !FIELD.test(p.fields[b])) return "widget"
      fields[p.fields[b]] = 1
    }
  }
  var scs = J.own(w, "sizeClasses")
  var classes = []
  if (scs !== undefined) {
    if (!Array.isArray(scs) || scs.length > 8) return "widget"
    for (var k = 0; k < scs.length; k++) {
      var sc = scs[k]
      if (!J.isObj(sc) || typeof sc.name !== "string" || !CLASS_NAME.test(sc.name)) return "widget"
      var mn = pair(sc.min), mx = pair(sc.max)
      if (!mn || !mx || mn[0] > mx[0] || mn[1] > mx[1]) return "widget"
      classes.push({ name: sc.name, min: mn, max: mx })
    }
  }
  var vars = J.own(w, "variants"), variants = []
  if (vars !== undefined) {
    if (!Array.isArray(vars) || vars.length > 32) return "widget"
    for (var v = 0; v < vars.length; v++) {
      if (typeof vars[v] !== "string" || !VARIANT_NAME.test(vars[v])) return "widget"
      variants.push(vars[v])
    }
  }
  return { topics: topics, fields: fields, classes: classes, variants: variants }
}

// installed.json from ow: only used to avoid asking for topics ow never granted.
// The daemon stays authoritative; a missing list means "ask for everything declared".
function checkInstalled(inst, id) {
  if (!J.isObj(inst) || J.own(inst, "id") !== id) return null
  var g = J.own(inst, "granted")
  if (g === undefined) return { granted: null }
  if (!Array.isArray(g) || g.length > 16) return null
  var set = {}
  for (var i = 0; i < g.length; i++) {
    if (!J.isObj(g[i]) || !Array.isArray(g[i].topics) || g[i].topics.length > 16) return null
    for (var t = 0; t < g[i].topics.length; t++) {
      if (typeof g[i].topics[t] !== "string" || !TOPIC.test(g[i].topics[t])) return null
      set[g[i].topics[t]] = 1
    }
  }
  return { granted: set }
}

// Turns the four files of one install directory into a record. `texts` are raw strings
// (or null when the file is missing/too large). -> { id, ok: true, ... } | { id, ok: false, reason }
// The reason is a fixed code, never text from a file.
function loadWidget(id, texts) {
  function fail(reason) { return { id: id, ok: false, reason: reason } }
  if (!validId(id)) return fail("id")
  var names = ["manifest", "widget", "layout", "installed"], parsed = {}
  for (var i = 0; i < names.length; i++) {
    var lim = names[i] === "layout" ? LIMITS.bytes : LIMITS.fileBytes
    var r = J.parse(texts ? texts[names[i]] : null, lim, LIMITS.depthJson)
    if (!r.ok) return fail(names[i] + "-" + r.reason)
    parsed[names[i]] = r.value
  }
  var man = parsed.manifest
  if (!J.isObj(man) || J.own(man, "id") !== id) return fail("manifest")
  var w = checkWidgetJson(parsed.widget, id)
  if (typeof w === "string") return fail(w)
  var inst = checkInstalled(parsed.installed, id)
  if (!inst) return fail("installed")
  var lv = validateLayout(parsed.layout, w.variants)
  if (!lv.ok) return fail("layout")
  var used = {}
  for (var b = 0; b < lv.binds.length; b++) {
    var bd = lv.binds[b].bind
    if (!J.has(w.topics, bd.topic) || !J.has(w.fields, bd.field)) return fail("bind")
    used[bd.topic] = 1
  }
  var wanted = [], ungranted = []
  Object.keys(used).forEach(function (t) {
    if (inst.granted === null || J.has(inst.granted, t)) wanted.push(t); else ungranted.push(t)
  })
  var name = typeof man.name === "string" ? plainText(man.name, 60) : ""
  return { id: id, ok: true, name: name, sizeClasses: w.classes, variants: w.variants,
           layout: parsed.layout, topics: wanted, ungranted: ungranted }
}

// ---------------------------------------------------------------- choosing what to draw

function clamp(v, lo, hi) { return Math.min(hi, Math.max(lo, v)) }

// The class whose [min, max] box holds (w, h); otherwise the nearest by distance to the box.
function pickSizeClass(classes, w, h) {
  if (!classes || !classes.length) return ""
  var best = "", bestD = Infinity
  for (var i = 0; i < classes.length; i++) {
    var c = classes[i]
    var dx = w < c.min[0] ? c.min[0] - w : (w > c.max[0] ? w - c.max[0] : 0)
    var dy = h < c.min[1] ? c.min[1] - h : (h > c.max[1] ? h - c.max[1] : 0)
    var d = dx * dx + dy * dy
    if (d < bestD) { bestD = d; best = c.name }
  }
  return best
}

// Variant by name. An explicit, declared variant wins; otherwise a variant named like the
// size class (the real widgets ship a "chip" variant for the chip class); otherwise root.
function pickNode(layout, variants, requested, sizeClass) {
  var vs = J.isObj(layout) ? J.own(layout, "variants") : undefined
  function named(n) {
    return typeof n === "string" && n.length && Array.isArray(variants) && variants.indexOf(n) >= 0
      && J.isObj(vs) && J.has(vs, n) ? vs[n] : null
  }
  var v = named(requested) || named(sizeClass)
  return v || (J.isObj(layout) ? J.own(layout, "root") : null)
}

function sizeRange(classes) {
  var r = { minW: 40, minH: 20, maxW: 640, maxH: 480 }
  if (classes && classes.length) {
    r = { minW: Infinity, minH: Infinity, maxW: 0, maxH: 0 }
    classes.forEach(function (c) {
      r.minW = Math.min(r.minW, c.min[0]); r.minH = Math.min(r.minH, c.min[1])
      r.maxW = Math.max(r.maxW, c.max[0]); r.maxH = Math.max(r.maxH, c.max[1])
    })
  }
  return r
}

function defaultSize(classes) {
  var c = null
  if (classes) for (var i = 0; i < classes.length; i++) if (classes[i].name === "compact") c = classes[i]
  if (!c && classes && classes.length) c = classes[0]
  if (!c) return { w: 200, h: 120 }
  return { w: Math.round((c.min[0] + c.max[0]) / 2), h: Math.round((c.min[1] + c.max[1]) / 2) }
}

// ---------------------------------------------------------------- bound values

// Own-property lookup only: a topic or field called "constructor" or "toString" never
// reaches the prototype chain. `data` is { topic: { field: value } }.
function lookup(data, bind) {
  if (!J.isObj(data) || !J.isObj(bind)) return undefined
  var t = J.own(bind, "topic"), f = J.own(bind, "field")
  if (typeof t !== "string" || typeof f !== "string" || !J.has(data, t)) return undefined
  var fields = data[t]
  return J.isObj(fields) ? J.own(fields, f) : undefined
}

var DASH = "—"
var MAX_ABS = 1e15

function finiteNum(v) { return typeof v === "number" && isFinite(v) && Math.abs(v) <= MAX_ABS }

// Text for a bound value; the em dash is the explicit "no data" placeholder.
function show(v, unit) {
  var u = typeof unit === "string" ? plainText(unit, 4) : ""
  if (finiteNum(v)) return (Math.floor(v) === v ? String(v) : String(Math.round(v * 10) / 10)) + u
  if (typeof v === "string") { var s = plainText(v, LIMITS.string); return s.length ? s + u : DASH }
  if (Array.isArray(v)) {
    var parts = []
    for (var i = 0; i < v.length && i < LIMITS.list; i++) {
      if (finiteNum(v[i])) parts.push(String(v[i])); else if (typeof v[i] === "string") parts.push(plainText(v[i], 40))
    }
    var j = plainText(parts.join(", "), LIMITS.string)
    return j.length ? j : DASH
  }
  return DASH
}

// 0..1 for bars and rings, or -1 when there is no usable number (placeholder, never 0).
function fraction(v) { return finiteNum(v) ? clamp(v, 0, 100) / 100 : -1 }

function series(v) {
  if (!Array.isArray(v)) return []
  var out = []
  for (var i = 0; i < v.length && out.length < LIMITS.list; i++) if (finiteNum(v[i])) out.push(v[i])
  return out
}

// Points for a w by h box from a series of at least two numbers; [] otherwise.
function sparkPoints(s, w, h) {
  if (s.length < 2) return []
  var lo = Math.min.apply(null, s), hi = Math.max.apply(null, s), pts = []
  for (var i = 0; i < s.length; i++)
    pts.push({ x: (i / (s.length - 1)) * w, y: h - 1 - ((s[i] - lo) / ((hi - lo) || 1)) * (h - 2) })
  return pts
}

function rows(v, max) {
  if (!Array.isArray(v)) return []
  var m = Math.min(typeof max === "number" && max >= 1 ? Math.floor(max) : 5, LIMITS.list), out = []
  for (var i = 0; i < v.length && out.length < m; i++) {
    if (typeof v[i] === "string") out.push(plainText(v[i], 60)); else if (finiteNum(v[i])) out.push(String(v[i]))
  }
  return out
}

// ---------------------------------------------------------------- placement state

// canvas.json: { "version": 1, "widgets": { "<id>": { x, y, w, h, variant, monitor } } }.
// Everything is clamped or dropped; unknown keys are ignored.
function parsePlacement(text) {
  var out = {}
  var r = J.parse(text, 64 * 1024, 10)
  if (!r.ok || !J.isObj(r.value) || J.own(r.value, "version") !== 1) return out
  var ws = J.own(r.value, "widgets")
  if (!J.isObj(ws)) return out
  var ids = Object.keys(ws)
  for (var i = 0; i < ids.length && i < 64; i++) {
    var p = ws[ids[i]]
    if (!validId(ids[i]) || !J.isObj(p)) continue
    var e = {}
    ;["x", "y", "w", "h"].forEach(function (k) {
      var n = J.own(p, k)
      if (finiteNum(n)) e[k] = clamp(Math.round(n), 0, 16384)
    })
    if (typeof p.variant === "string" && VARIANT_NAME.test(p.variant)) e.variant = p.variant
    if (typeof p.monitor === "string" && /^[A-Za-z0-9._:-]{1,40}$/.test(p.monitor)) e.monitor = p.monitor
    out[ids[i]] = e
  }
  return out
}

// Hard limits on what a tile may cover, whatever a widget's size classes or the state file say:
// at most half the screen each way and never more than 1024 by 768, never smaller than 48 by 24.
var TILE_MIN = { w: 48, h: 24 }
var TILE_CAP = { w: 1024, h: 768 }
var MAX_TILES = 64

function num(v, fallback) { return typeof v === "number" && isFinite(v) ? v : fallback }

function cleanScreen(s) {
  var w = num(s && s.width, 1920), h = num(s && s.height, 1080)
  return { name: s && typeof s.name === "string" ? s.name : "", width: w >= 1 && w <= 65536 ? Math.floor(w) : 1920,
           height: h >= 1 && h <= 65536 ? Math.floor(h) : 1080 }
}

// Where each record goes: saved placement where present, else stacked down the left edge.
// -> [{ id, monitor, x, y, w, h, variant }]. Sizes are clamped to the widget's size classes AND to
// the screen cap above; positions to the screen they land on (screens: [{name, width, height}]).
// The canvas input mask is the union of these rectangles, so this is also its upper bound.
function layoutTiles(records, placement, screens) {
  var list = [], nextY = 64
  var scr0 = cleanScreen(screens && screens.length ? screens[0] : null)
  var recs = records.slice(0, MAX_TILES)
  for (var i = 0; i < recs.length; i++) {
    var rec = recs[i], pl = J.has(placement, rec.id) ? placement[rec.id] : {}
    var scr = scr0
    for (var s = 0; s < (screens ? screens.length : 0); s++) {
      if (screens[s] && screens[s].name === pl.monitor) scr = cleanScreen(screens[s])
    }
    var capW = clamp(Math.floor(scr.width / 2), TILE_MIN.w, TILE_CAP.w)
    var capH = clamp(Math.floor(scr.height / 2), TILE_MIN.h, TILE_CAP.h)
    var classes = rec.ok ? rec.sizeClasses : []
    var rng = sizeRange(classes), def = defaultSize(classes)
    var w = clamp(Math.round(num(pl.w, def.w)), Math.min(Math.max(rng.minW, TILE_MIN.w), capW), Math.min(rng.maxW, capW))
    var h = clamp(Math.round(num(pl.h, def.h)), Math.min(Math.max(rng.minH, TILE_MIN.h), capH), Math.min(rng.maxH, capH))
    w = clamp(w, TILE_MIN.w, capW); h = clamp(h, TILE_MIN.h, capH)
    var x = pl.x, y = pl.y
    if (!(typeof x === "number" && isFinite(x)) || !(typeof y === "number" && isFinite(y))) { x = 48; y = nextY; nextY += h + 16 }
    list.push({ id: rec.id, monitor: scr.name,
                x: clamp(Math.round(x), 0, Math.max(0, scr.width - w)), y: clamp(Math.round(y), 0, Math.max(0, scr.height - h)),
                w: w, h: h, variant: pl.variant || "" })
  }
  return list
}

// ---------------------------------------------------------------- helpers for the renderer

// Reads that never throw on a node that is not an object.
function prop(n, k) { return J.isObj(n) ? J.own(n, k) : undefined }
function typeOf(n) { var t = prop(n, "type"); return isKnownType(t) ? t : "" }
function kids(n) {
  var c = prop(n, "children")
  return Array.isArray(c) ? c.slice(0, LIMITS.children) : []
}
function sizeOr(n, fallback) { var s = prop(n, "size"); return oneOf(SIZES, s) ? s : fallback }
function roleOr(n, fallback) { var c = prop(n, "color"); return oneOf(ROLES, c) ? c : fallback }
function columnsOf(n) { var c = prop(n, "columns"); return c === 2 || c === 3 || c === 4 ? c : 2 }
function gapOf(n) { var g = prop(n, "gap"); return oneOf(GAPS, g) ? g : "sm" }

// Icon outlines on a 16 by 16 grid, the same drawings the website uses. A widget picks a
// NAME from ICONS; it never supplies a path, a file or a URL.
var ICON_PATHS = {
  cpu: "M3 3h10v10H3zM6 1v2M10 1v2M6 13v2M10 13v2",
  memory: "M2 5h12v6H2zM5 5v6M8 5v6M11 5v6",
  battery: "M1.5 4.5h11v7h-11zM14 7v2",
  clock: "M8 2a6 6 0 100 12A6 6 0 008 2zM8 4v4l3 2",
  list: "M2 3h12M2 8h12M2 13h12",
  git: "M5 3v10M11 5v2a3 3 0 01-3 3H5M5 3a1 1 0 100 .01M11 5a1 1 0 100 .01M5 13a1 1 0 100 .01",
  container: "M2 5l6-3 6 3v6l-6 3-6-3zM2 5l6 3 6-3M8 8v6",
  disk: "M2 5h12v6H2zM11 8h1",
  network: "M8 2v4M3 14V10h10v4M8 6v4M2 14h4M10 14h4",
  bolt: "M9 1L3 9h4l-1 6 6-8H8z",
  calendar: "M2 4h12v10H2zM2 7h12M5 2v3M11 2v3",
  check: "M3 8l3 3 7-7"
}
function iconPath(name) { return oneOf(ICONS, name) ? ICON_PATHS[name] : "" }

// ---------------------------------------------------------------- what a tile shows

function entryKey(widget, topic) { return widget + " " + topic }

// Combines the loaded record, the bridge's per-topic entries and the bridge state into
// { values, label, dim }. `values` holds ONLY the topics this widget declared and was
// granted (record.topics); `label` is a fixed string ("" when everything is live) that
// the tile prints in its corner; `dim` greys the content when it is stale or offline.
// entries: { "<widget> <topic>": { state: "pending"|"live"|"stale"|"denied"|"locked"|"unknown"|"limited"|"error", data } }
function tileState(record, entries, bridgeState) {
  var out = { values: {}, label: "", dim: false }
  if (!record || record.ok !== true) return { values: {}, label: "widget unavailable", dim: false }
  var topics = record.topics || [], have = 0, locked = false, denied = (record.ungranted && record.ungranted.length > 0)
  var unknown = false, limited = false, stale = false
  for (var i = 0; i < topics.length; i++) {
    var e = J.isObj(entries) ? J.own(entries, entryKey(record.id, topics[i])) : undefined
    if (!J.isObj(e)) continue
    if (J.isObj(e.data) && (e.state === "live" || e.state === "stale")) { out.values[topics[i]] = e.data; have++ }
    if (e.state === "stale") stale = true
    else if (e.state === "locked") locked = true
    else if (e.state === "denied") denied = true
    else if (e.state === "unknown") unknown = true
    else if (e.state === "limited" || e.state === "error") limited = true
  }
  var online = bridgeState === "connected"
  if (!online) {
    out.dim = have > 0
    out.label = bridgeState === "denied" ? "daemon refused" : (bridgeState === "connecting" ? "connecting" : "daemon offline")
  } else if (locked) out.label = "locked"
  else if (denied) out.label = "no access"
  else if (unknown) out.label = "not served"
  else if (limited) out.label = "daemon error"
  else if (stale) { out.label = "stale"; out.dim = true }
  else if (topics.length > 0 && have < topics.length) out.label = "waiting for data"
  return out
}
