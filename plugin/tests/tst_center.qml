import QtQuick
import QtTest
import ".."
import "../Layout.js" as L
import "../CenterModel.js" as M
import "fx.js" as Fx

TestCase {
  id: tc
  name: "Center"
  width: 800; height: 700
  visible: true
  when: windowShown
  property url base: Qt.resolvedUrl("fixtures/")

  Component { id: panelC; CenterPanel { width: 720; height: 560 } }

  function rec(name) { return L.loadWidget(Fx.IDS[name], Fx.texts(base, name)) }
  // a widget set with one file replaced by a modified object
  function recWith(name, file, fn) {
    var t = Fx.texts(base, name), o = JSON.parse(t[file]); fn(o); t[file] = JSON.stringify(o)
    return L.loadWidget(Fx.IDS[name], t)
  }
  function entry(id, topic, e) { var o = {}; o[L.entryKey(id, topic)] = e; return o }
  function panelFor(records, entries, link) {
    var p = createTemporaryObject(panelC, tc)
    p.rows = M.rows(records, entries || ({}), link || "connected")
    p.link = link || "connected"
    waitForRendering(p)
    return p
  }
  // every Text in the item tree, by objectName, in order
  function texts(item, objectName, out) {
    out = out || []
    if (item.objectName === objectName && item.text !== undefined) out.push(item.text)
    var kids = item.children || []
    for (var i = 0; i < kids.length; i++) texts(kids[i], objectName, out)
    return out
  }
  function allText(item) {
    var out = []
    ;(function walk(i) { if (i.text !== undefined && i.textFormat !== undefined) out.push(i.text); for (var c = 0; c < i.children.length; c++) walk(i.children[c]) })(item)
    return out.join("\n")
  }

  function test_lists_installed_widgets_from_fixtures() {
    var recs = [rec("battery"), rec("clock"), rec("system-monitor")]
    var p = panelFor(recs)
    compare(p.list.count, 3)
    var titles = texts(p, "title")
    compare(titles.length, 3)
    verify(titles[1].indexOf("io.github.dexterhere.clock") === 0)
    verify(titles[1].indexOf("Clock") > 0)
    var metas = texts(p, "meta")
    verify(metas[1].indexOf("version 0.1.0") >= 0, metas[1])
    verify(metas[1].indexOf("status alpha") >= 0, metas[1])
    verify(metas[1].indexOf("tier spec") >= 0, metas[1])
  }

  function test_empty_list_says_so() {
    var p = panelFor([])
    compare(p.list.count, 0)
    compare(texts(p, "emptyText")[0], "No widgets installed.")
  }

  function test_grants_text() {
    var rows = M.rows([rec("clock")], {}, "connected")
    compare(rows[0].grants[0], "granted system.time (fields: hm, hms, date, tz)")
    // a topic declared but not in installed.json is reported as not granted
    var r = recWith("clock", "installed", function (o) { o.granted = [{ topics: ["system.time"], fields: ["hm"] }] })
    compare(M.rows([r], {}, "connected")[0].grants[0], "granted system.time (fields: hm)")
    var w = L.loadWidget(Fx.IDS.battery, (function () { var t = Fx.texts(base, "battery"); var o = JSON.parse(t.installed); o.granted = []; t.installed = JSON.stringify(o); return t })())
    verify(w.ok)
    verify(M.rows([w], {}, "connected")[0].grants.join("|").indexOf("declared, not granted: battery.status") >= 0)
    // no list at all
    var n = recWith("clock", "installed", function (o) { delete o.granted })
    compare(M.rows([n], {}, "connected")[0].grants[0], "no grant list recorded")
    var p = panelFor([rec("clock")])
    compare(texts(p, "grants")[0], "granted system.time (fields: hm, hms, date, tz)")
  }

  function test_each_health_state_has_its_label() {
    var id = Fx.IDS.clock, r = rec("clock")
    var want = { locked: "locked (ow vault unlock)", no_credential: "no credential", unauthorized: "unauthorized",
                 insufficient: "insufficient permission", rate_limited: "rate limited", expired_soon: "expired soon",
                 offline: "offline", unavailable: "unavailable" }
    for (var s in want) {
      var row = M.rows([r], entry(id, "system.time", { state: "stale", data: { hm: "1" }, health: s }), "connected")[0]
      compare(row.summary, want[s], s)
      compare(row.health[0], "system.time: " + want[s], s)
      var p = panelFor([r], entry(id, "system.time", { state: "pending", health: s }))
      verify(texts(p, "meta")[0].indexOf("health: " + want[s]) > 0, s)
    }
    compare(M.rows([r], entry(id, "system.time", { state: "live", data: { hm: "1" } }), "connected")[0].summary, "ok")
    // expired_soon comes with fresh data and still shows
    compare(M.rows([r], entry(id, "system.time", { state: "live", data: { hm: "1" }, health: "expired_soon" }), "connected")[0].summary, "expired soon")
  }

  function test_bridge_states_without_a_provider_word() {
    var id = Fx.IDS.clock, r = rec("clock")
    var want = { stale: "stale", denied: "no access", locked: "locked (ow vault unlock)", unknown: "not served",
                 limited: "daemon error", error: "daemon error", pending: "waiting for data" }
    for (var s in want) compare(M.rows([r], entry(id, "system.time", { state: s }), "connected")[0].summary, want[s], s)
    compare(M.rows([r], {}, "connected")[0].summary, "waiting for data")
    // no daemon: nothing is claimed about the topic
    compare(M.rows([r], entry(id, "system.time", { state: "live", data: {} }), "unavailable")[0].summary, "daemon not running")
    compare(M.linkLabel("denied"), "daemon refused this client")
    compare(M.linkLabel("<b>"), "daemon not running")
    // a health word that is not on the list is not displayed
    compare(M.rows([r], entry(id, "system.time", { state: "live", health: "<b>pwn</b>" }), "connected")[0].summary, "ok")
  }

  function test_remove_command_text() {
    compare(M.removeCommand("io.github.example.clock"), "ow remove io.github.example.clock")
    var p = panelFor([rec("clock")])
    compare(texts(p, "command")[0], "ow remove io.github.dexterhere.clock")
  }

  function test_odd_ids_are_quoted_and_inert() {
    var odd = ["a; rm -rf ~", "$(touch /tmp/x)", "`id`", "a b", "x'y", "x\ny", "../../etc/passwd", "UPPER", "a\u0000b", "", "-rf", "x\u202ey"]
    for (var i = 0; i < odd.length; i++) {
      var c = M.removeCommand(odd[i])
      verify(c.indexOf("ow remove '") === 0 && c.charAt(c.length - 1) === "'", JSON.stringify(odd[i]) + " -> " + c)
      verify(c.indexOf("\n") < 0 && c.indexOf("\u0000") < 0 && c.indexOf("\u202e") < 0, c)
      // inside the quotes there is no bare quote: each ' is closed, escaped and reopened
      var inner = c.substring("ow remove '".length, c.length - 1)
      verify(inner.replace(/'\\''/g, "").indexOf("'") < 0, c)
    }
    compare(M.removeCommand("x'y"), "ow remove 'x'\\''y'")
    compare(M.removeCommand(42), "ow remove ''")
    compare(M.removeCommand("a".repeat(500)).length, "ow remove ''".length + 100)
  }

  function test_failed_widget_row_shows_only_id_and_a_fixed_reason() {
    var rows = M.rows([{ id: "io.test.bad", ok: false, reason: "layout" }, { id: "io.test.x", ok: false, reason: "<b>boom</b>\n" }], {}, "connected")
    compare(rows[0].summary, "could not be loaded (layout)")
    compare(rows[0].command, "ow remove io.test.bad")
    var p = panelFor([{ id: "io.test.bad", ok: false, reason: "layout" }])
    compare(p.list.count, 1)
    verify(texts(p, "meta")[0].indexOf("could not be loaded") >= 0)
  }

  function test_escape_closes() {
    var p = panelFor([rec("clock")])
    var spy = createTemporaryObject(spyC, tc, { target: p, signalName: "closeRequested" })
    p.forceActiveFocus()
    keyClick(Qt.Key_Escape)
    compare(spy.count, 1)
    keyClick(Qt.Key_A); keyClick(Qt.Key_Return)
    compare(spy.count, 1)                    // other keys do nothing
  }
  Component { id: spyC; SignalSpy { } }

  function test_close_button_closes() {
    var p = panelFor([])
    var spy = createTemporaryObject(spyC, tc, { target: p, signalName: "closeRequested" })
    var b = null
    ;(function walk(i) { if (i.objectName === "closeButton") b = i; for (var c = 0; c < i.children.length; c++) walk(i.children[c]) })(p)
    verify(b !== null)
    mouseClick(b, 2, 2)
    compare(spy.count, 1)
  }

  // Hostile text in widget.json / manifest.json / installed.json: it may change words on screen
  // (plain text, bounded) and nothing else.
  function test_hostile_strings_do_not_break_the_panel() {
    var evil = "<img src=x onerror=1><b>x</b> ${1+1} $(id) `id` ' \" \\ \u202e\u200b\n\t" + "z".repeat(5000)
    var t = Fx.texts(base, "clock")
    var man = JSON.parse(t.manifest); man.name = evil; man.version = evil; man.description = evil; t.manifest = JSON.stringify(man)
    var ins = JSON.parse(t.installed); ins.status = evil; ins.tier = evil; ins.repo = evil; ins.commit = evil
    ins.granted = [{ topics: ["system.time"], fields: ["hm", evil, 5, null, "a b", "x".repeat(60)] }]; t.installed = JSON.stringify(ins)
    var w = JSON.parse(t.widget); w.providers[0].name = evil; t.widget = JSON.stringify(w)
    var r = L.loadWidget(Fx.IDS.clock, t)
    verify(r.ok)
    var rows = M.rows([r], {}, "connected")
    verify(rows[0].name.length <= 60)
    verify(rows[0].version.length <= 32)
    compare(rows[0].status, "unknown")
    compare(rows[0].grants[0], "granted system.time (fields: hm)")
    var p = panelFor([r])
    compare(p.list.count, 1)
    var shown = allText(p)
    verify(shown.indexOf("<img src=x") >= 0)               // markup is shown as characters, never parsed
    verify(shown.indexOf("\u202e") < 0 && shown.indexOf("\u200b") < 0)
    // every Text is plain text
    ;(function walk(i) { if (i.textFormat !== undefined) compare(i.textFormat, Text.PlainText); for (var c = 0; c < i.children.length; c++) walk(i.children[c]) })(p)
    compare(texts(p, "command")[0], "ow remove io.github.dexterhere.clock")
    // hostile data from the other side: row inputs that are not what the store produces
    var junk = M.rows([null, 5, "x", { id: 7 }, { id: "a", ok: true, topics: "no", name: 3, grants: 4 }], null, "connected")
    verify(junk.length >= 1)
    compare(M.rows("nope", {}, "connected").length, 0)
    compare(M.rows(new Array(1000).fill({ id: "io.a", ok: false, reason: "x" }), {}, "connected").length, 200)
  }

  function test_center_item_model_is_inert_data() {
    // nothing in the model is a function or a url; it is strings and arrays of strings
    var rows = M.rows([rec("clock"), rec("battery")], {}, "connected")
    for (var i = 0; i < rows.length; i++)
      for (var k in rows[i]) {
        var v = rows[i][k]
        verify(typeof v === "string" || typeof v === "boolean" || Array.isArray(v), k)
      }
  }
}
