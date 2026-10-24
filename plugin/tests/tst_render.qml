import QtQuick
import QtQuick.Layouts
import QtTest
import ".."
import "../Layout.js" as L
import "fx.js" as Fx

TestCase {
  id: tc
  name: "Render"
  width: 400; height: 300
  visible: true
  when: windowShown
  property url base: Qt.resolvedUrl("fixtures/")

  Component {
    id: nodeC
    Item {
      property alias ln: ln
      property alias frame: frame
      width: 220; height: 160
      id: frame
      LayoutNode { id: ln; width: parent.width }
    }
  }

  function make(node, values, w) {
    var o = createTemporaryObject(nodeC, tc)
    if (w) o.width = w
    o.ln.values = values || ({})
    o.ln.node = node
    waitForRendering(o)
    return o
  }

  // every LayoutNode in the tree, depth first
  function all(item, out) {
    out = out || []
    if (item.nodeType !== undefined && item.fits !== undefined) out.push(item)
    for (var i = 0; i < item.children.length; i++) all(item.children[i], out)
    return out
  }
  function texts(item, out) {
    out = out || []
    if (item.textFormat !== undefined) out.push(item)
    for (var i = 0; i < item.children.length; i++) texts(item.children[i], out)
    return out
  }
  function countNodes(n) {
    var c = 1, ch = n.children || []
    for (var i = 0; i < ch.length; i++) c += countNodes(ch[i])
    return c
  }
  // position of an inner node in the coordinates of the outermost one
  function pos(item, rootItem) { return item.mapToItem(rootItem, 0, 0) }
  function sameColor(a, b) { return Math.abs(a.r - b.r) < 0.01 && Math.abs(a.g - b.g) < 0.01 && Math.abs(a.b - b.b) < 0.01 && Math.abs(a.a - b.a) < 0.01 }
  function sample(name) { return JSON.parse(Fx.texts(base, name).widget).sampleData }
  function layoutOf(name) { return JSON.parse(Fx.texts(base, name).layout) }

  // ---------------------------------------------------------------- the three real widgets
  function test_real_widgets_all_variants_render_with_data() {
    ["clock", "system-monitor", "battery"].forEach(function (name) {
      var lay = layoutOf(name), data = sample(name)
      var nodes = [lay.root]
      Object.keys(lay.variants).forEach(function (v) { nodes.push(lay.variants[v]) })
      nodes.forEach(function (root, i) {
        var o = make(root, data, 260)
        var drawn = all(o.ln)
        compare(drawn.length, countNodes(root), name + " #" + i + " node count")
        drawn.forEach(function (d) {
          verify(d.nodeType !== "unsupported" && d.nodeType !== "toolarge", name + " drew " + d.nodeType)
          compare(d.status, Loader.Ready)
          verify(!d.placeholder, name + " " + d.nodeType + " is a placeholder although data is present")
        })
        texts(o.ln).forEach(function (t) {
          compare(t.textFormat, Text.PlainText)
          verify(t.text.length > 0)
        })
        verify(o.ln.implicitHeight > 0 && o.ln.height > 0, name + " has a height")
        o.destroy()
      })
    })
  }
  function test_real_widgets_show_the_values() {
    var o = make(layoutOf("clock").root, sample("clock"), 240)
    var shown = all(o.ln).map(function (d) { return d.shownText })
    verify(shown.indexOf("10:09") >= 0, shown.join("|"))
    verify(shown.indexOf("Mon 19 Oct") >= 0)
    var s = make(layoutOf("system-monitor").root, sample("system-monitor"), 240)
    shown = all(s.ln).map(function (d) { return d.shownText })
    verify(shown.indexOf("34%") >= 0, shown.join("|"))
    verify(shown.indexOf("58%") >= 0)
    var b = make(layoutOf("battery").root, sample("battery"), 240)
    shown = all(b.ln).map(function (d) { return d.shownText })
    verify(shown.indexOf("72%") >= 0, shown.join("|"))
    verify(shown.indexOf("Discharging") >= 0)
    var v = make(layoutOf("system-monitor").variants["memory-ring"], sample("system-monitor"), 240)
    shown = all(v.ln).map(function (d) { return d.shownText })
    verify(shown.indexOf("9200MiB") >= 0, shown.join("|"))
  }
  function test_real_widgets_without_data_show_placeholders_not_zero() {
    ["clock", "system-monitor", "battery"].forEach(function (name) {
      var lay = layoutOf(name)
      var roots = [lay.root]
      Object.keys(lay.variants).forEach(function (v) { roots.push(lay.variants[v]) })
      roots.forEach(function (root) {
        var o = make(root, {}, 260)
        all(o.ln).forEach(function (d) {
          if (["value", "bar", "ring", "sparkline", "list"].indexOf(d.nodeType) >= 0 || (d.nodeType === "text" && d.node.bind))
            verify(d.placeholder, name + " " + d.nodeType + " should be a placeholder")
          if (d.nodeType === "value") compare(d.shownText, L.DASH)
          verify(d.shownText.indexOf("0%") < 0, name + " must not show 0% for missing data")
        })
        o.destroy()
      })
    })
  }

  // ---------------------------------------------------------------- each node type
  function test_text_and_badge() {
    var o = make({ type: "text", text: "CPU", size: "lg", color: "muted" })
    compare(o.ln.shownText, "CPU")
    var t = texts(o.ln)[0]
    verify(sameColor(t.color, Theme.muted))
    compare(t.font.pixelSize, Theme.textPx("lg"))
    compare(t.textFormat, Text.PlainText)
    var b = make({ type: "badge", bind: { topic: "system.time", field: "tz" }, color: "secondary" }, { "system.time": { tz: "NPT" } })
    compare(b.ln.shownText, "NPT")
    verify(b.ln.implicitWidth > 0)
    verify(sameColor(texts(b.ln)[0].color, Theme.secondary))
    var bm = make({ type: "badge", bind: { topic: "system.time", field: "tz" } }, {})
    verify(bm.ln.placeholder)
  }
  function test_value_formats() {
    var vals = { "t.a": { n: 34, f: 1.26, neg: -3, big: 999999999999999, s: "10:09", arr: ["a", "b"] } }
    function show(field, unit) { return make({ type: "value", bind: { topic: "t.a", field: field }, unit: unit }, vals).ln.shownText }
    compare(show("n", "%"), "34%")
    compare(show("f", ""), "1.3")
    compare(show("neg", "C"), "-3C")
    compare(show("big", ""), "999999999999999")
    compare(show("s", ""), "10:09")
    compare(show("arr", ""), "a, b")
    compare(show("missing", "%"), L.DASH)
  }
  function test_bar_and_ring() {
    var vals = { "t.a": { p: 40, z: 0, over: 250, under: -10 } }
    function fr(type, field) {
      var o = make({ type: type, bind: { topic: "t.a", field: field } }, vals)
      return { f: o.ln.item.fraction, ph: o.ln.placeholder, shown: o.ln.shownText }
    }
    ;["bar", "ring"].forEach(function (t) {
      compare(fr(t, "p").f, 0.4); verify(!fr(t, "p").ph)
      compare(fr(t, "z").f, 0); verify(!fr(t, "z").ph)                 // a real zero is drawn as zero
      compare(fr(t, "over").f, 1)
      compare(fr(t, "under").f, 0)
      verify(fr(t, "nope").ph)                                          // missing is not zero
      compare(fr(t, "nope").f, -1)
    })
    var r = make({ type: "ring", bind: { topic: "t.a", field: "p" }, color: "accent" }, vals)
    verify(r.ln.implicitWidth >= Theme.space(44))
  }
  function test_sparkline() {
    var v = { "t.a": { s: [1, 3, 2, 5], one: [1], bad: [NaN, "x", {}] } }
    var ok = make({ type: "sparkline", bind: { topic: "t.a", field: "s" } }, v)
    verify(!ok.ln.placeholder); compare(ok.ln.item.pts.length, 4)
    verify(make({ type: "sparkline", bind: { topic: "t.a", field: "one" } }, v).ln.placeholder)
    verify(make({ type: "sparkline", bind: { topic: "t.a", field: "bad" } }, v).ln.placeholder)
    verify(make({ type: "sparkline", bind: { topic: "t.a", field: "nope" } }, v).ln.placeholder)
  }
  function test_list() {
    var v = { "t.a": { rows: ["one", "two", "three", "four", "five", "six", "seven"], none: [] } }
    var o = make({ type: "list", bind: { topic: "t.a", field: "rows" }, max: 3 }, v)
    compare(o.ln.item.items.length, 3)
    compare(texts(o.ln).length, 3)
    compare(make({ type: "list", bind: { topic: "t.a", field: "rows" } }, v).ln.item.items.length, 5)   // default max
    verify(make({ type: "list", bind: { topic: "t.a", field: "none" } }, v).ln.placeholder)
    verify(make({ type: "list", bind: { topic: "t.a", field: "nope" } }, v).ln.placeholder)
    compare(texts(make({ type: "list", bind: { topic: "t.a", field: "none" } }, v).ln)[0].text, L.DASH)
  }
  function test_icons_all_twelve() {
    L.ICONS.forEach(function (n) {
      var o = make({ type: "icon", name: n, size: "lg", color: "secondary" })
      compare(o.ln.nodeType, "icon")
      compare(o.ln.status, Loader.Ready)
      compare(o.ln.item.px, Theme.iconPx("lg"))
      verify(L.iconPath(n).length > 5)
    })
    compare(L.iconPath("skull"), "")
    compare(L.iconPath("constructor"), "")
    compare(L.iconPath("file:///etc/passwd"), "")
  }
  function test_layout_containers() {
    var kids = [{ type: "text", text: "a" }, { type: "text", text: "b" }, { type: "text", text: "c" }, { type: "text", text: "d" }]
    var st = make({ type: "stack", gap: "lg", children: kids })
    compare(all(st.ln).length, 5)
    var a = all(st.ln)
    verify(pos(a[2], st.ln).y > pos(a[1], st.ln).y, "stack lays out downward")
    var rw = make({ type: "row", children: kids })
    var r = all(rw.ln)
    verify(pos(r[2], rw.ln).x > pos(r[1], rw.ln).x, "row lays out to the right")
    var gd = make({ type: "grid", columns: 2, children: kids })
    var g = all(gd.ln)
    verify(pos(g[2], gd.ln).x > pos(g[1], gd.ln).x); verify(pos(g[3], gd.ln).y > pos(g[1], gd.ln).y)
    compare(make({ type: "grid", columns: 99, children: kids }).ln.item.columns, 2)   // bad column count -> 2
  }
  function test_text_that_fits_is_not_elided() {
    // regression: a layout rounds widths down, which used to elide "Memory" and "66.3%" by a fraction of a pixel
    var o = make({ type: "row", children: [{ type: "text", text: "Memory" }, { type: "spacer" },
                                            { type: "value", bind: { topic: "a.b", field: "x" }, unit: "%", size: "md" }] },
                 { "a.b": { x: 66.3 } }, 190)
    texts(o.ln).forEach(function (t) { verify(!t.truncated, "'" + t.text + "' was elided at width " + t.width + " of " + t.implicitWidth) })
    var sm = make(layoutOf("system-monitor").root, sample("system-monitor"), 190)
    texts(sm.ln).forEach(function (t) { verify(!t.truncated, "'" + t.text + "' was elided in the real system-monitor layout") })
  }
  function test_spacer_and_divider_in_a_row() {
    var o = make({ type: "row", children: [{ type: "text", text: "L" }, { type: "spacer" }, { type: "text", text: "R" }] }, {}, 200)
    var a = all(o.ln)
    verify(pos(a[3], o.ln).x + a[3].width > 150, "spacer pushes the right item to the right edge")
    var d = make({ type: "stack", children: [{ type: "divider" }, { type: "text", text: "x" }] }, {}, 200)
    compare(all(d.ln)[1].nodeType, "divider")
    verify(all(d.ln)[1].width > 150)
  }

  // ---------------------------------------------------------------- hostile layouts
  function test_unknown_and_malformed_nodes_show_a_fixed_card() {
    ;[{ type: "script", text: "x" }, { type: "constructor" }, { type: "__proto__" }, { type: "Text" }, {}, [], "text", 5, null, undefined,
      { type: ["text"] }, { type: { toString: function () { return "text" } } }].forEach(function (n) {
      var o = make(n)
      compare(o.ln.nodeType, "unsupported")
      verify(o.ln.placeholder)
      compare(o.ln.shownText, "node unavailable")             // never the node's own text
    })
    var o2 = make({ type: "<b>evil</b>", text: "hello" })
    compare(o2.ln.shownText, "node unavailable")
  }
  function test_depth_and_node_budgets() {
    var deep = Fx.nested(8, { type: "text", text: "leaf" })        // 9 levels
    var o = make(deep)
    compare(o.ln.nodeType, "toolarge")
    compare(o.ln.shownText, "layout too large")
    compare(all(o.ln).length, 1)                                    // nothing below the root was instantiated
    var ok = make(Fx.nested(7, { type: "text", text: "leaf" }))
    compare(ok.ln.nodeType, "stack")
    var kids = []; for (var i = 0; i < 20; i++) kids.push({ type: "stack", children: [{ type: "spacer" }, { type: "spacer" }, { type: "spacer" }, { type: "spacer" }, { type: "spacer" }, { type: "spacer" }, { type: "spacer" }, { type: "spacer" }, { type: "spacer" }, { type: "spacer" }, { type: "spacer" }] })
    compare(make({ type: "stack", children: kids }).ln.nodeType, "toolarge")       // 1 + 20 + 220 nodes
    var wide = []; for (var j = 0; j < 40; j++) wide.push({ type: "spacer" })
    compare(make({ type: "row", children: wide }).ln.nodeType, "toolarge")          // 41 children > 20
    // a child handed straight to the renderer at an excessive level is cut as well
    var lone = createTemporaryObject(nodeC, tc)
    lone.ln.level = 9
    lone.ln.node = { type: "text", text: "x" }
    compare(lone.ln.nodeType, "toolarge")
  }
  function test_bind_to_ungranted_topic_shows_placeholder() {
    // the tile only passes declared and granted topics; anything else is simply absent
    var values = { "system.cpu": { usage_pct: 12 } }
    var o = make({ type: "value", bind: { topic: "secret.mail", field: "subject" } }, values)
    verify(o.ln.placeholder); compare(o.ln.shownText, L.DASH)
    var o2 = make({ type: "value", bind: { topic: "system.cpu", field: "other" } }, values)
    verify(o2.ln.placeholder)
    var o3 = make({ type: "value", bind: { topic: "constructor", field: "name" } }, values)
    verify(o3.ln.placeholder)
    var o4 = make({ type: "value", bind: { topic: "system.cpu", field: "toString" } }, values)
    verify(o4.ln.placeholder)
    var o5 = make({ type: "value", bind: "system.cpu" }, values)
    verify(o5.ln.placeholder)
  }
  function test_long_strings_are_elided_not_overflowing() {
    var long = new Array(5000).join("w")
    var o = make({ type: "stack", children: [{ type: "text", bind: { topic: "t.a", field: "s" } }, { type: "value", bind: { topic: "t.a", field: "s" } },
                                              { type: "badge", bind: { topic: "t.a", field: "s" } }, { type: "list", bind: { topic: "t.a", field: "l" } }] },
                 { "t.a": { s: long, l: [long, long] } }, 180)
    texts(o.ln).forEach(function (t) {
      verify(t.text.length <= 120, "text capped at 120 characters, got " + t.text.length)
      verify(t.width <= 180.5, "text wider than its tile: " + t.width)
      verify(t.width > 20, "text has a real width, got " + t.width)
      verify(t.truncated || t.implicitWidth <= 180.5, "text is elided")
    })
    var lit = make({ type: "text", text: long })              // a literal over the limit (validator would refuse it first)
    verify(texts(lit.ln)[0].text.length <= 120)
  }
  function test_bidi_and_zero_width_are_removed_from_data() {
    var o = make({ type: "stack", children: [{ type: "text", bind: { topic: "t.a", field: "s" } }, { type: "text", bind: { topic: "t.a", field: "z" } }] },
                 { "t.a": { s: "‮evil.exe‬", z: "a​b⁦c\u0000d\u0007" } })
    var t = texts(o.ln)
    compare(t[0].text, "evil.exe")
    compare(t[1].text, "abcd")
    var only = make({ type: "text", bind: { topic: "t.a", field: "s" } }, { "t.a": { s: "‮​" } })
    verify(only.ln.placeholder)
  }
  function test_qml_html_markdown_strings_are_just_text() {
    var evil = ["import QtQuick; Item { Component.onCompleted: Qt.quit() }", "<b>bold</b><img src='file:///etc/passwd'>", "<script>alert(1)</script>",
                "**bold** [x](http://evil) `code` # head", "&lt;b&gt; &amp;", "file:///etc/passwd", "http://evil.example/x.png", "${1+1} #{1+1} {{1+1}}", "javascript:alert(1)"]
    evil.forEach(function (s) {
      var o = make({ type: "stack", children: [{ type: "text", bind: { topic: "t.a", field: "s" } }, { type: "value", bind: { topic: "t.a", field: "s" } },
                                                { type: "list", bind: { topic: "t.a", field: "l" } }] }, { "t.a": { s: s, l: [s] } }, 400)
      texts(o.ln).forEach(function (t) {
        compare(t.textFormat, Text.PlainText)
        compare(t.text, L.plainText(s, 120).length ? L.plainText(s, t.parent === o.ln.item ? 120 : 60) : L.DASH)
        verify(t.contentWidth > 0)
      })
      // nothing was created from the string: exactly the three nodes plus root
      compare(all(o.ln).length, 4)
    })
  }
  function test_numbers_nan_infinity_huge() {
    var vals = { "t.a": { nan: NaN, inf: Infinity, ninf: -Infinity, huge: 1e300, huge2: 1.7976931348623157e308, tiny: 5e-324, ok: 12 } }
    ;["nan", "inf", "ninf", "huge", "huge2"].forEach(function (f) {
      ;["value", "bar", "ring", "text", "badge"].forEach(function (t) {
        var o = make({ type: t, bind: { topic: "t.a", field: f } }, vals)
        verify(o.ln.placeholder, t + " " + f)
      })
      verify(make({ type: "sparkline", bind: { topic: "t.a", field: f } }, vals).ln.placeholder)
    })
    compare(make({ type: "value", bind: { topic: "t.a", field: "tiny" } }, vals).ln.shownText, "0")
    compare(make({ type: "value", bind: { topic: "t.a", field: "ok" } }, vals).ln.shownText, "12")
  }
  function test_missing_and_odd_value_shapes() {
    var vals = { "t.a": { o: { x: 1 }, b: true, n: null, f: function () { return 1 } } }
    ;["o", "b", "n", "f", "absent"].forEach(function (f) {
      ;["value", "bar", "ring", "text"].forEach(function (t) {
        verify(make({ type: t, bind: { topic: "t.a", field: f } }, vals).ln.placeholder, t + " " + f)
      })
    })
    verify(make({ type: "value", bind: { topic: "t.a", field: "o" } }, null).ln.placeholder)
    verify(make({ type: "value", bind: { topic: "t.a", field: "o" } }, "str").ln.placeholder)
    verify(make({ type: "value", bind: { topic: "t.a", field: "o" } }, [1, 2]).ln.placeholder)
    verify(make({ type: "value" }, vals).ln.placeholder)
  }
  function test_colours_only_from_roles() {
    var o = make({ type: "stack", children: [{ type: "text", text: "a", color: "#ff0000" }, { type: "text", text: "b", color: "red" },
                                              { type: "text", text: "c", color: "urgent" }, { type: "text", text: "d", color: { r: 1 } },
                                              { type: "text", text: "e", color: "accent" }] })
    var t = texts(o.ln)
    verify(sameColor(t[0].color, Theme.foreground))
    verify(sameColor(t[1].color, Theme.foreground))
    verify(sameColor(t[2].color, Theme.foreground))
    verify(sameColor(t[3].color, Theme.foreground))
    verify(sameColor(t[4].color, Theme.accent))
  }
  function test_sizes_only_from_the_table() {
    var o = make({ type: "stack", children: [{ type: "text", text: "a", size: "xl" }, { type: "text", text: "b", size: 9999 },
                                              { type: "text", text: "c", size: "huge" }, { type: "value", bind: { topic: "t.a", field: "n" }, size: "-1" }] }, { "t.a": { n: 1 } })
    var t = texts(o.ln)
    compare(t[0].font.pixelSize, Theme.textPx("xl"))
    compare(t[1].font.pixelSize, Theme.textPx("md"))
    compare(t[2].font.pixelSize, Theme.textPx("md"))
    compare(t[3].font.pixelSize, Theme.valuePx("xl"))
  }
  function test_node_is_never_mutated_or_executed() {
    var n = { type: "text", text: "x", onCompleted: "boom", source: "file:///etc/passwd", url: "http://x", children: [{ type: "text", text: "ignored" }] }
    var before = JSON.stringify(n)
    var o = make(n)
    compare(JSON.stringify(n), before)
    compare(all(o.ln).length, 1)           // a text node does not render children
    compare(o.ln.shownText, "x")
  }
}
