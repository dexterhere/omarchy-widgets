.pragma library

// Strict JSON entry point for everything that comes from outside this plugin
// (widget files, the state file, daemon lines). Never throws.
//
// JSON.parse in the QML engine is recursive, so a file made of tens of thousands
// of '[' could exhaust the native stack of the whole shell. depthOk() is a flat
// loop that refuses such input before it reaches JSON.parse.

function depthOk(text, max) {
  var depth = 0, inStr = false, esc = false
  for (var i = 0; i < text.length; i++) {
    var c = text.charCodeAt(i)
    if (inStr) {
      if (esc) esc = false
      else if (c === 92) esc = true
      else if (c === 34) inStr = false
      continue
    }
    if (c === 34) inStr = true
    else if (c === 123 || c === 91) { if (++depth > max) return false }
    else if (c === 125 || c === 93) depth--
  }
  return true
}

// -> { ok: true, value } | { ok: false, reason: "type"|"size"|"depth"|"syntax" }
function parse(text, maxLen, maxDepth) {
  if (typeof text !== "string") return { ok: false, reason: "type" }
  if (text.length === 0 || text.length > maxLen) return { ok: false, reason: "size" }
  if (text.charCodeAt(0) === 0xfeff) return { ok: false, reason: "syntax" }
  if (!depthOk(text, maxDepth)) return { ok: false, reason: "depth" }
  try { return { ok: true, value: JSON.parse(text) } }
  catch (e) { return { ok: false, reason: "syntax" } }
}

function isObj(v) { return typeof v === "object" && v !== null && !Array.isArray(v) }
function has(o, k) { return Object.prototype.hasOwnProperty.call(o, k) }
function own(o, k) { return has(o, k) ? o[k] : undefined }
