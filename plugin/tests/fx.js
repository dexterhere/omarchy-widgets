.pragma library
// Test support: read fixture files (needs QML_XHR_ALLOW_FILE_READ=1, set by run-qml-tests.sh)
// and build widget file sets.
function read(url) {
  var x = new XMLHttpRequest()
  x.open("GET", url, false)
  x.send()
  return x.responseText
}
var IDS = { clock: "io.github.dexterhere.clock", "system-monitor": "io.github.dexterhere.system-monitor",
            battery: "io.github.dexterhere.battery" }
function texts(base, name) {
  var o = {}
  ;["manifest", "widget", "layout", "installed"].forEach(function (f) { o[f] = read(base + name + "/" + f + ".json") })
  return o
}
function widgetTexts(base, name) { return texts(base, name) }
// A widget file set with a replacement layout object.
function withLayout(t, layout) {
  var c = {}; for (var k in t) c[k] = t[k]
  c.layout = typeof layout === "string" ? layout : JSON.stringify(layout)
  return c
}
function nested(depth, leaf) {
  var n = leaf
  for (var i = 0; i < depth; i++) n = { type: "stack", children: [n] }
  return n
}
function layoutOf(root, variants) {
  var l = { schemaVersion: 0, root: root }
  if (variants) l.variants = variants
  return l
}
