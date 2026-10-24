import QtQuick
import QtTest
import ".."
import "../Layout.js" as L
import "fx.js" as Fx

TestCase {
  id: tc
  name: "Tile"
  width: 400; height: 300
  visible: true
  when: windowShown
  property url base: Qt.resolvedUrl("fixtures/")

  Component { id: tileC; Tile { width: 220; height: 140 } }

  // properties are assigned after creation: initial-property maps would turn the JS objects
  // into variant wrappers (not what Store hands over in the plugin)
  function mk(props) {
    var t = createTemporaryObject(tileC, tc)
    for (var k in props) t[k] = props[k]
    return t
  }
  function rec(name) { return L.loadWidget(Fx.IDS[name], Fx.texts(base, name)) }
  function entries(name, state, over) {
    var r = rec(name), data = JSON.parse(Fx.texts(base, name).widget).sampleData, e = {}
    r.topics.forEach(function (t) { e[L.entryKey(r.id, t)] = { state: state, data: data[t] } })
    for (var k in over) e[k] = over[k]
    return e
  }
  function tile(name, ents, bridge, variant, w, h) {
    var t = mk({ record: rec(name), entries: ents, bridgeState: bridge || "connected", variant: variant || "" })
    if (w) t.width = w
    if (h) t.height = h
    waitForRendering(t)
    return t
  }
  function shownTexts(t) {
    var out = []
    ;(function walk(i) { if (i.nodeType !== undefined && i.fits !== undefined && i.shownText !== "") out.push(i.shownText); for (var c = 0; c < i.children.length; c++) walk(i.children[c]) })(t.content)
    return out
  }

  function test_live() {
    var t = tile("system-monitor", entries("system-monitor", "live"))
    compare(t.statusLabel, "")
    verify(shownTexts(t).indexOf("34%") >= 0)
    compare(t.content.opacity, 1)
  }
  function test_waiting_has_placeholders_not_zero() {
    var t = tile("system-monitor", {})
    compare(t.statusLabel, "waiting for data")
    verify(shownTexts(t).indexOf("0%") < 0)
    verify(shownTexts(t).indexOf(L.DASH) >= 0)
  }
  function test_stale_dims_and_keeps_last_values() {
    var t = tile("battery", entries("battery", "stale"))
    compare(t.statusLabel, "stale")
    compare(t.content.opacity, 0.5)
    verify(shownTexts(t).indexOf("72%") >= 0)
  }
  function test_offline_keeps_last_values_dimmed() {
    var t = tile("battery", entries("battery", "live"), "unavailable")
    compare(t.statusLabel, "daemon offline")
    compare(t.content.opacity, 0.5)
    verify(shownTexts(t).indexOf("72%") >= 0)
    var none = tile("battery", {}, "unavailable")
    compare(none.statusLabel, "daemon offline")
    verify(shownTexts(none).indexOf("72%") < 0)
    compare(tile("battery", {}, "connecting").statusLabel, "connecting")
    compare(tile("battery", {}, "denied").statusLabel, "daemon refused")
  }
  function test_per_topic_errors() {
    var key = L.entryKey(Fx.IDS.battery, "battery.status")
    function st(s) { var e = {}; e[key] = { state: s }; return tile("battery", e).statusLabel }
    compare(st("locked"), "locked")
    compare(st("denied"), "no access")
    compare(st("unknown"), "not served")
    compare(st("limited"), "daemon error")
    compare(st("error"), "daemon error")
    compare(st("pending"), "waiting for data")
    var t = tile("battery", (function () { var e = {}; e[key] = { state: "denied" }; return e })())
    verify(t.content.visible)
    verify(shownTexts(t).indexOf(L.DASH) >= 0)            // the denied topic shows placeholders, not stale numbers
  }
  function test_only_declared_and_granted_topics_reach_the_renderer() {
    // an entry for a topic the widget does not use, or for another widget, never shows up
    var e = entries("battery", "live")
    e[L.entryKey(Fx.IDS.battery, "secret.mail")] = { state: "live", data: { subject: "TOP SECRET" } }
    e[L.entryKey("io.other.widget", "battery.status")] = { state: "live", data: { percent: 1 } }
    var t = tile("battery", e)
    compare(Object.keys(t.view.values).join(), "battery.status")
    verify(shownTexts(t).join("|").indexOf("TOP SECRET") < 0)
    verify(shownTexts(t).indexOf("1%") < 0)
  }
  function test_ungranted_topics_are_not_requested_and_say_so() {
    var r = rec("system-monitor")
    var t = Fx.texts(base, "system-monitor"), inst = JSON.parse(t.installed)
    inst.granted = [{ topics: ["system.cpu"], fields: ["usage_pct"] }]
    t.installed = JSON.stringify(inst)
    var limited = L.loadWidget(Fx.IDS["system-monitor"], t)
    var tl = mk({ record: limited, entries: {}, bridgeState: "connected" })
    compare(tl.statusLabel, "no access")
    compare(limited.topics.join(), "system.cpu")
  }
  function test_unavailable_tile_names_only_the_id() {
    var bad = { id: "io.github.dexterhere.clock", ok: false, reason: "layout" }
    var t = mk({ record: bad })
    waitForRendering(t)
    verify(t.unavailable)
    var seen = []
    ;(function walk(i) { if (i.textFormat !== undefined && i.visible) seen.push(i.text); for (var c = 0; c < i.children.length; c++) walk(i.children[c]) })(t)
    compare(seen.join("|"), "Widget unavailable|io.github.dexterhere.clock")
    // an id that is not an id is not shown at all
    var evil = mk({ record: { id: "<b>x</b>", ok: false, reason: "id" } })
    seen = []
    ;(function walk(i) { if (i.textFormat !== undefined && i.visible) seen.push(i.text); for (var c = 0; c < i.children.length; c++) walk(i.children[c]) })(evil)
    compare(seen.join("|"), "Widget unavailable|")
    verify(!mk({ record: null }).content.visible)
  }
  function test_size_class_and_variant_selection() {
    var chip = tile("clock", entries("clock", "live"), "connected", "", 90, 30)
    compare(chip.sizeClass, "chip")
    compare(chip.rootNode, rec("clock").layout.variants.chip)
    var compact = tile("clock", entries("clock", "live"), "connected", "", 200, 120)
    compare(compact.sizeClass, "compact")
    compare(compact.rootNode, rec("clock").layout.root)
    var named = tile("clock", entries("clock", "live"), "connected", "date-focus", 200, 120)
    compare(named.rootNode, rec("clock").layout.variants["date-focus"])
    compare(tile("clock", entries("clock", "live"), "connected", "not-declared", 200, 120).rootNode, rec("clock").layout.root)
  }
  function test_content_stays_inside_the_tile() {
    var lay = Fx.nested(1, { type: "text", text: "x" })
    var r = rec("system-monitor")
    var t = tile("system-monitor", entries("system-monitor", "live"), "connected", "full", 150, 100)
    verify(t.clip)
    verify(t.content.width <= t.width)
  }
}
