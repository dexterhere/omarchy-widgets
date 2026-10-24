import QtQuick
import QtTest
import "../Layout.js" as L
import "fx.js" as Fx

TestCase {
  name: "Layout"
  property url base: Qt.resolvedUrl("fixtures/")
  property var clock: Fx.texts(base, "clock")
  property var cpu: ({ type: "value", bind: { topic: "system.time", field: "hm" } })

  function ok(layout, variants) { return L.validateLayout(layout, variants || []).ok }
  function lay(root, variants) { return Fx.layoutOf(root, variants) }

  // ---- the three real widgets validate end to end
  function test_real_widgets_load() {
    ["clock", "system-monitor", "battery"].forEach(function (n) {
      var r = L.loadWidget(Fx.IDS[n], Fx.texts(base, n))
      verify(r.ok, n + " " + JSON.stringify(r))
      verify(r.topics.length > 0)
      compare(r.ungranted.length, 0)
      verify(r.sizeClasses.length === 3)
    })
    var sm = L.loadWidget(Fx.IDS["system-monitor"], Fx.texts(base, "system-monitor"))
    compare(sm.variants.length, 8)
    compare(sm.topics.sort().join(), "system.cpu,system.load,system.memory,system.temp")
  }

  // ---- limits
  function test_depth_limit() {
    verify(ok(lay(Fx.nested(7, this.cpu))))        // 8 levels
    verify(!ok(lay(Fx.nested(8, this.cpu))))       // 9 levels
  }
  function test_node_limit() {
    function many(n) { var kids = []; for (var i = 0; i < n; i++) kids.push({ type: "spacer" }); return kids }
    var rows = []
    for (var i = 0; i < 10; i++) rows.push({ type: "row", children: many(19) })   // 10 + 190 = 200 + root
    verify(!ok(lay({ type: "stack", children: rows })))                           // 201 nodes
    rows.pop()
    verify(ok(lay({ type: "stack", children: rows })))
  }
  function test_children_limit() {
    function many(n) { var kids = []; for (var i = 0; i < n; i++) kids.push({ type: "spacer" }); return kids }
    verify(ok(lay({ type: "row", children: many(20) })))
    verify(!ok(lay({ type: "row", children: many(21) })))
    verify(!ok(lay({ type: "row", children: [] })))
  }
  function test_text_limits_and_plainness() {
    function t(s) { return lay({ type: "text", text: s }) }
    verify(ok(t("CPU")))
    verify(ok(t(new Array(121).join("x"))))
    verify(!ok(t(new Array(123).join("x"))))
    verify(!ok(t("")))
    verify(!ok(t("   ")))
    verify(!ok(t("a‮b")))               // right-to-left override
    verify(!ok(t("a​b")))               // zero width space
    verify(!ok(t("a⁦b")))               // isolate
    verify(!ok(t("a\u0000b")))
    verify(!ok(t("<b>x</b>")))
    verify(!ok(t("<!-- x -->")))
    verify(!ok(t("é́́́")))   // stacked marks
    verify(!ok(t("\ud800")))                      // lone surrogate
    verify(!ok(t("a󠀁")))               // tag character
    verify(!ok(t("a")))                     // private use
    verify(ok(t("100% done")))
    verify(ok(t("**bold** [link](http://x) `code`")))   // markdown is just characters, drawn as plain text
    verify(ok(t("import QtQuick; Qt.quit()")))          // looks like QML, is only characters
    verify(!ok(lay({ type: "text", text: "x", bind: { topic: "a.b", field: "c" } })))
    verify(!ok(lay({ type: "text" })))
  }
  function test_unit_limits() {
    function v(u) { return lay({ type: "value", bind: { topic: "a.b", field: "c" }, unit: u }) }
    verify(ok(v("%")))
    verify(ok(v("MiB")))
    verify(!ok(v("12345")))
    verify(!ok(v("‮")))
    verify(!ok(v(5)))
  }
  function test_size_bytes() {
    var big = JSON.stringify(lay({ type: "text", text: "x", pad: new Array(70000).join("a") }))
    var t = Fx.withLayout(clock, big)
    compare(L.loadWidget(Fx.IDS.clock, t).reason, "layout-size")
  }

  // ---- structure
  function test_unknown_type_and_keys() {
    verify(!ok(lay({ type: "script", text: "x" })))
    verify(!ok(lay({ type: "Text" })))
    verify(!ok(lay({ type: "constructor" })))
    verify(!ok(lay({ type: "__proto__" })))
    verify(!ok(lay({ type: "text", text: "x", onClick: "boom" })))
    verify(!ok(lay({ type: "icon", name: "cpu", source: "file:///etc/passwd" })))
    verify(!ok(lay({ type: "spacer", width: 5 })))
    verify(!ok({ schemaVersion: 0, root: { type: "spacer" }, extra: 1 }))
    verify(!ok({ schemaVersion: 1, root: { type: "spacer" } }))
    verify(!ok({ root: { type: "spacer" } }))
    verify(!ok([]))
    verify(!ok(null))
    verify(!ok("x"))
  }
  function test_roles_sizes_gaps() {
    verify(ok(lay({ type: "text", text: "x", color: "muted", size: "xl" })))
    verify(!ok(lay({ type: "text", text: "x", color: "#ff0000" })))
    verify(!ok(lay({ type: "text", text: "x", color: "red" })))
    verify(!ok(lay({ type: "text", text: "x", color: "urgent" })))
    verify(!ok(lay({ type: "text", text: "x", size: "huge" })))
    verify(!ok(lay({ type: "text", text: "x", size: 12 })))
    verify(!ok(lay({ type: "row", gap: "xl", children: [{ type: "spacer" }] })))
    verify(!ok(lay({ type: "grid", columns: 5, children: [{ type: "spacer" }] })))
    verify(ok(lay({ type: "grid", columns: 3, children: [{ type: "spacer" }] })))
  }
  function test_binds() {
    function b(x) { return lay({ type: "value", bind: x }) }
    verify(ok(b({ topic: "system.cpu", field: "usage_pct" })))
    verify(!ok(b({ topic: "system", field: "usage_pct" })))
    verify(!ok(b({ topic: "System.cpu", field: "usage_pct" })))
    verify(!ok(b({ topic: "system.cpu", field: "9x" })))
    verify(!ok(b({ topic: "system.cpu", field: "a/b" })))
    verify(!ok(b({ topic: "system.cpu", field: "x", extra: 1 })))
    verify(!ok(b("system.cpu")))
    verify(!ok(lay({ type: "bar" })))
    verify(!ok(lay({ type: "list", bind: { topic: "a.b", field: "c" }, max: 51 })))
    verify(!ok(lay({ type: "list", bind: { topic: "a.b", field: "c" }, max: 1.5 })))
    verify(ok(lay({ type: "list", bind: { topic: "a.b", field: "c" }, max: 50 })))
    verify(!ok(lay({ type: "icon", name: "skull" })))
  }
  function test_variants_must_be_declared() {
    var root = { type: "spacer" }
    verify(ok(lay(root, { chip: root }), ["chip"]))
    verify(!ok(lay(root, { evil: root }), ["chip"]))
    verify(!ok(lay(root, { chip: Fx.nested(8, root) }), ["chip"]))
    verify(!ok(lay(root, "x"), ["chip"]))
  }

  // ---- whole widget
  function test_widget_rejections() {
    var id = Fx.IDS.clock
    function mut(file, fn) {
      var t = Fx.texts(base, "clock"); var o = JSON.parse(t[file]); fn(o); t[file] = JSON.stringify(o); return L.loadWidget(id, t)
    }
    compare(mut("widget", function (o) { o.tier = "code" }).reason, "tier")
    compare(mut("widget", function (o) { o.id = "other" }).reason, "widget")
    compare(mut("widget", function (o) { o.capabilities.network = ["example.com"] }).reason, "policy")
    compare(mut("widget", function (o) { o.capabilities.spawn = ["sh"] }).reason, "policy")
    compare(mut("manifest", function (o) { o.id = "other" }).reason, "manifest")
    compare(mut("installed", function (o) { o.id = "other" }).reason, "installed")
    compare(mut("widget", function (o) { o.providers[0].topics = ["system.other"] }).reason, "bind")
    compare(mut("widget", function (o) { o.providers[0].fields = ["hm"] }).reason, "bind")   // date/tz undeclared
    compare(mut("widget", function (o) { o.sizeClasses[0].min = [10, "x"] }).reason, "widget")
    compare(mut("widget", function (o) { o.sizeClasses[0].min = [500, 500] }).reason, "widget")
    compare(mut("widget", function (o) { o.variants = ["a b"] }).reason, "widget")
    compare(mut("layout", function (o) { o.root.type = "exec" }).reason, "layout")
    verify(mut("manifest", function (o) { o.extra = { anything: 1 } }).ok)       // keys that do not affect drawing are ignored
  }
  function test_bind_to_ungranted_topic() {
    // layout binds a topic that widget.json does not declare: dropped, not subscribed
    var t = Fx.texts(base, "clock"); var l = JSON.parse(t.layout)
    l.root.children[0].bind.topic = "secret.mail"
    t.layout = JSON.stringify(l)
    compare(L.loadWidget(Fx.IDS.clock, t).reason, "bind")
  }
  function test_installed_granted_filter() {
    var t = Fx.texts(base, "system-monitor"); var i = JSON.parse(t.installed)
    i.granted = [{ topics: ["system.cpu"], fields: ["usage_pct"] }]
    t.installed = JSON.stringify(i)
    var r = L.loadWidget(Fx.IDS["system-monitor"], t)
    verify(r.ok)
    compare(r.topics.join(), "system.cpu")
    compare(r.ungranted.sort().join(), "system.load,system.memory,system.temp")
    delete i.granted; t.installed = JSON.stringify(i)
    compare(L.loadWidget(Fx.IDS["system-monitor"], t).topics.length, 4)
    i.granted = "all"; t.installed = JSON.stringify(i)
    compare(L.loadWidget(Fx.IDS["system-monitor"], t).reason, "installed")
  }
  function test_bad_files() {
    var id = Fx.IDS.clock
    function with_(file, text) { var t = Fx.texts(base, "clock"); t[file] = text; return L.loadWidget(id, t) }
    compare(with_("layout", "").reason, "layout-size")
    compare(with_("layout", "{").reason, "layout-syntax")
    compare(with_("layout", "﻿{}").reason, "layout-syntax")
    compare(with_("widget", null).reason, "widget-type")
    compare(with_("widget", new Array(300000).join("a")).reason, "widget-size")
    compare(with_("layout", new Array(30000).join("[")).reason, "layout-depth")      // depth bomb, never reaches JSON.parse
    compare(with_("layout", new Array(10000).join("{\"a\":")).reason, "layout-depth")
    compare(L.loadWidget(id, null).reason, "manifest-type")
    compare(L.loadWidget("Bad Id", Fx.texts(base, "clock")).reason, "id")
    compare(L.loadWidget("../x", Fx.texts(base, "clock")).reason, "id")
    compare(L.loadWidget("a..b", Fx.texts(base, "clock")).reason, "id")
    compare(L.loadWidget(".hidden", Fx.texts(base, "clock")).reason, "id")
    compare(L.loadWidget("-x", Fx.texts(base, "clock")).reason, "id")
    compare(L.loadWidget("a/b", Fx.texts(base, "clock")).reason, "id")
    compare(L.loadWidget(new Array(200).join("a"), Fx.texts(base, "clock")).reason, "id")
  }
  function test_failure_never_carries_file_text() {
    var t = Fx.texts(base, "clock"); var o = JSON.parse(t.manifest); o.name = "EVIL <script>"; o.id = "x"
    t.manifest = JSON.stringify(o)
    var r = L.loadWidget(Fx.IDS.clock, t)
    verify(!r.ok)
    compare(Object.keys(r).sort().join(), "id,ok,reason")
    verify(JSON.stringify(r).indexOf("EVIL") < 0)
    var o2 = JSON.parse(Fx.texts(base, "clock").manifest); o2.name = "A‮b<i>"
    var t2 = Fx.texts(base, "clock"); t2.manifest = JSON.stringify(o2)
    var good = L.loadWidget(Fx.IDS.clock, t2)
    verify(good.ok)
    verify(good.name.indexOf("‮") < 0)
  }

  // ---- run-time text and numbers
  function test_plain_text_sanitiser() {
    compare(L.plainText("a‮b​c\u0007d", 50), "abcd")
    compare(L.plainText("<b>hi</b> **x** import QtQuick", 50), "<b>hi</b> **x** import QtQuick")   // inert characters
    compare(L.plainText("line1\nline2\tx", 50), "line1 line2 x")
    compare(L.plainText(new Array(100000).join("x"), 120).length, 120)
    compare(L.plainText("😀 ok", 50), "😀 ok")           // a real emoji pair survives
    compare(L.plainText("a\ud800b", 50), "ab")                                // lone surrogate dropped
    compare(L.plainText("a󠀁b", 50), "ab")
    compare(L.plainText(42, 50), "")
    compare(L.plainText(null, 50), "")
    compare(L.plainText({}, 50), "")
  }
  function test_show_numbers() {
    compare(L.show(34), "34")
    compare(L.show(34, "%"), "34%")
    compare(L.show(1.26, ""), "1.3")
    compare(L.show(-0.04), "0")
    compare(L.show(NaN), L.DASH)
    compare(L.show(Infinity), L.DASH)
    compare(L.show(-Infinity, "%"), L.DASH)
    compare(L.show(1e300), L.DASH)
    compare(L.show(1e16), L.DASH)
    compare(L.show(999999999999999), "999999999999999")
    compare(L.show(undefined), L.DASH)
    compare(L.show(null), L.DASH)
    compare(L.show(true), L.DASH)
    compare(L.show({}), L.DASH)
    compare(L.show(""), L.DASH)
    compare(L.show("‮​"), L.DASH)
    compare(L.show("10:09"), "10:09")
    compare(L.show("x", "‮evil"), "xevil")
    compare(L.show(["a", "b", 3, NaN, {}]), "a, b, 3")
    compare(L.show([]), L.DASH)
    verify(L.show(new Array(5000).join("w")).length <= 120)
  }
  function test_fraction_series_spark() {
    compare(L.fraction(50), 0.5)
    compare(L.fraction(150), 1)
    compare(L.fraction(-5), 0)
    compare(L.fraction(NaN), -1)
    compare(L.fraction(Infinity), -1)
    compare(L.fraction("50"), -1)
    compare(L.fraction(undefined), -1)
    compare(L.fraction(0), 0)          // a real zero is a zero, only missing data is a placeholder
    compare(L.series([1, NaN, "x", 2, Infinity, 3]).join(), "1,2,3")
    compare(L.series("abc").length, 0)
    compare(L.series(new Array(500).fill(1)).length, 50)
    compare(L.sparkPoints([1], 100, 24).length, 0)
    var p = L.sparkPoints([0, 10, 5], 100, 24)
    compare(p.length, 3)
    compare(p[0].x, 0); compare(p[2].x, 100)
    var flat = L.sparkPoints([5, 5, 5], 100, 24)       // no divide by zero
    verify(isFinite(flat[1].y))
  }
  function test_rows() {
    compare(L.rows(["a", "b", "c"], 2).join(), "a,b")
    compare(L.rows("nope", 2).length, 0)
    compare(L.rows(["a", {}, null, "b"], 5).join(), "a,b")
    compare(L.rows(new Array(500).fill("x"), 9999).length, 50)
    compare(L.rows(["a"], -3).length, 1)
  }
  function test_lookup_is_own_property_only() {
    var data = { "system.cpu": { usage_pct: 5, constructor: 7 } }
    compare(L.lookup(data, { topic: "system.cpu", field: "usage_pct" }), 5)
    compare(L.lookup(data, { topic: "system.cpu", field: "constructor" }), 7)
    compare(L.lookup(data, { topic: "system.cpu", field: "toString" }), undefined)
    compare(L.lookup(data, { topic: "constructor", field: "name" }), undefined)
    compare(L.lookup(data, { topic: "__proto__", field: "x" }), undefined)
    compare(L.lookup(data, { topic: "system.mem", field: "x" }), undefined)
    compare(L.lookup(null, { topic: "a", field: "b" }), undefined)
    compare(L.lookup(data, null), undefined)
    compare(L.lookup(data, { topic: 5, field: [] }), undefined)
  }

  // ---- choosing what to draw
  function test_size_class_and_variant() {
    var r = L.loadWidget(Fx.IDS.clock, Fx.texts(base, "clock"))
    compare(L.pickSizeClass(r.sizeClasses, 80, 30), "chip")
    compare(L.pickSizeClass(r.sizeClasses, 200, 120), "compact")
    compare(L.pickSizeClass(r.sizeClasses, 400, 150), "wide")
    compare(L.pickSizeClass(r.sizeClasses, 5000, 5000), "wide")        // nearest, never rejected
    compare(L.pickSizeClass([], 10, 10), "")
    var root = r.layout.root
    compare(L.pickNode(r.layout, r.variants, "", "compact"), root)
    compare(L.pickNode(r.layout, r.variants, "minimal", "compact"), r.layout.variants.minimal)
    compare(L.pickNode(r.layout, r.variants, "", "chip"), r.layout.variants.chip)          // class name doubles as variant
    compare(L.pickNode(r.layout, r.variants, "nope", "compact"), root)                      // undeclared name falls back
    compare(L.pickNode(r.layout, r.variants, "constructor", "compact"), root)
    compare(L.pickNode(r.layout, r.variants, "__proto__", "compact"), root)
  }
  function test_placement_parse_and_tiles() {
    var bad = L.parsePlacement("{\"version\":1,\"widgets\":{\"io.a.b\":{\"x\":\"zz\",\"y\":1e308,\"w\":-50,\"h\":null,\"variant\":\"<b>\",\"monitor\":\"../x\"}}}")
    compare(JSON.stringify(bad["io.a.b"]), "{\"w\":0}")
    compare(Object.keys(L.parsePlacement("nonsense")).length, 0)
    compare(Object.keys(L.parsePlacement("{\"version\":2,\"widgets\":{}}")).length, 0)
    compare(Object.keys(L.parsePlacement("{\"version\":1,\"widgets\":{\"../../etc\":{\"x\":1}}}")).length, 0)
    var good = L.parsePlacement("{\"version\":1,\"widgets\":{\"io.a.b\":{\"x\":10.4,\"y\":20,\"variant\":\"minimal\",\"monitor\":\"eDP-1\"}}}")
    compare(good["io.a.b"].x, 10); compare(good["io.a.b"].variant, "minimal"); compare(good["io.a.b"].monitor, "eDP-1")

    var recs = [L.loadWidget(Fx.IDS.clock, Fx.texts(base, "clock")), L.loadWidget(Fx.IDS.battery, Fx.texts(base, "battery")),
                { id: "broken", ok: false, reason: "layout" }]
    var screens = [{ name: "A", width: 1920, height: 1080 }, { name: "B", width: 800, height: 600 }]
    var t = L.layoutTiles(recs, {}, screens)
    compare(t.length, 3)
    compare(t[0].x, 48); verify(t[1].y > t[0].y + t[0].h - 1)               // stacked, not overlapping
    compare(t[0].monitor, "A")
    // saved placement far off screen and oversize is clamped
    var pl = {}; pl[Fx.IDS.clock] = { x: 99999, y: 99999, w: 9999, h: 9999, monitor: "B" }
    var t2 = L.layoutTiles(recs, pl, screens)
    compare(t2[0].monitor, "B")
    verify(t2[0].w <= 480 && t2[0].h <= 220)
    verify(t2[0].x + t2[0].w <= 800 && t2[0].y + t2[0].h <= 600)
    // unknown monitor falls back to the first screen
    pl[Fx.IDS.clock] = { x: 1, y: 1, monitor: "GONE" }
    compare(L.layoutTiles(recs, pl, screens)[0].monitor, "A")
    compare(L.layoutTiles(recs, {}, []).length, 3)
  }

  // ---- tiles can never cover a screen (the input mask is the union of these rectangles)
  function giant() {
    var t = Fx.texts(base, "clock"); var w = JSON.parse(t.widget)
    w.sizeClasses = [{ name: "huge", min: [8192, 8192], max: [8192, 8192] }]
    t.widget = JSON.stringify(w)
    return L.loadWidget(Fx.IDS.clock, t)
  }
  function inside(tile, scr) { return tile.x >= 0 && tile.y >= 0 && tile.x + tile.w <= scr.width && tile.y + tile.h <= scr.height }
  function test_tile_size_is_capped_to_the_screen() {
    var g = giant()
    verify(g.ok)
    var scr = { name: "A", width: 1920, height: 1080 }
    var t = L.layoutTiles([g], {}, [scr])[0]
    verify(t.w <= 960 && t.h <= 540 && t.w <= 1024 && t.h <= 768, t.w + "x" + t.h)
    verify(inside(t, scr))
    var big = { name: "B", width: 7680, height: 4320 }
    var tb = L.layoutTiles([g], {}, [big])[0]
    verify(tb.w <= 1024 && tb.h <= 768, tb.w + "x" + tb.h)       // hard cap, not half of an 8K screen
    var small = { name: "C", width: 300, height: 200 }
    var ts = L.layoutTiles([g], {}, [small])[0]
    verify(ts.w <= 150 && ts.h <= 100 && ts.w >= 48 && ts.h >= 24 && inside(ts, small), ts.w + "x" + ts.h)
    var tiny = { name: "D", width: 10, height: 10 }
    var tt = L.layoutTiles([g], {}, [tiny])[0]
    verify(tt.w >= 48 && tt.h >= 24, tt.w + "x" + tt.h)         // never a zero-size or negative tile
  }
  function test_placement_values_cannot_enlarge_a_tile() {
    var rec = L.loadWidget(Fx.IDS.clock, Fx.texts(base, "clock"))
    var scr = { name: "A", width: 1920, height: 1080 }
    ;[{ w: 99999, h: 99999 }, { w: -5, h: -5 }, { w: NaN, h: Infinity }, { w: "x", h: null }, { w: 0, h: 0 }].forEach(function (pl) {
      var p = {}; p[Fx.IDS.clock] = { x: NaN, y: Infinity, w: pl.w, h: pl.h }
      var t = L.layoutTiles([rec], p, [scr])[0]
      verify(isFinite(t.x) && isFinite(t.y) && isFinite(t.w) && isFinite(t.h), JSON.stringify(t))
      verify(t.w >= 48 && t.h >= 24 && t.w <= 480 && t.h <= 220, JSON.stringify(t))
      verify(inside(t, scr), JSON.stringify(t))
    })
    var p2 = {}; p2[Fx.IDS.clock] = { x: -500, y: -500 }
    var t2 = L.layoutTiles([rec], p2, [scr])[0]
    verify(t2.x >= 0 && t2.y >= 0)
    p2[Fx.IDS.clock] = { x: 1e9, y: 1e9, w: 200, h: 100 }
    verify(inside(L.layoutTiles([rec], p2, [scr])[0], scr))
  }
  function test_screen_values_are_sanitised() {
    var rec = L.loadWidget(Fx.IDS.clock, Fx.texts(base, "clock"))
    ;[{ name: "A", width: NaN, height: -1 }, { name: "A", width: Infinity, height: 0 }, null, { width: "wide" }].forEach(function (s) {
      var t = L.layoutTiles([rec], {}, [s])[0]
      verify(isFinite(t.x + t.y + t.w + t.h) && t.w >= 48 && t.h >= 24, JSON.stringify(t))
    })
  }
  function test_total_mask_area_is_bounded_by_the_clamped_tiles() {
    var g = giant(), recs = []
    for (var i = 0; i < 200; i++) { var c = JSON.parse(JSON.stringify(g)); c.id = "io.test.w" + i; recs.push(c) }
    var scr = { name: "A", width: 1920, height: 1080 }
    var tiles = L.layoutTiles(recs, {}, [scr])
    verify(tiles.length <= 64, "tile count " + tiles.length)
    var sum = 0
    tiles.forEach(function (t) { sum += t.w * t.h; verify(inside(t, scr)) })
    verify(sum <= 64 * 960 * 540)
    // one tile can never be the whole screen
    tiles.forEach(function (t) { verify(t.w * t.h <= 1920 * 1080 / 4, t.w + "x" + t.h) })
  }
  function test_plain_is_conservative() {
    verify(L.isPlain("CPU"))
    verify(L.isPlain("नमस्ते"))       // Devanagari with marks
    verify(!L.isPlain(5))
    verify(!L.isPlain("a<x"))
    verify(!L.isPlain("a < b"))      // same as the registry: "<" followed by a letter looks like markup
  }
}
