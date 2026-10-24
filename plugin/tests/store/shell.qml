// Store test harness (see ../run-store-tests.sh): a hostile widgets tree, then changes on disk.
import QtQuick
import Quickshell
import "plugin"

ShellRoot {
  id: h
  property int passed: 0
  property int failed: 0
  property int t: 0
  property var step: ({})
  Store { id: s; widgetsDir: Quickshell.env("OW_TEST_WIDGETS") }

  // the Widget Center state (the window part cannot be created offscreen) against the same hostile tree; a stub stands in for the injected service
  CenterState { id: c; service: QtObject { property var store: s; property var bridge: null } }

  function check(name, ok, detail) {
    if (ok) { passed++; console.log("PASS " + name) } else { failed++; console.log("FAIL " + name + " :: " + detail) }
  }
  function r(id) { return s.records[id] }
  function why(id) { return r(id) ? (r(id).ok ? "ok" : r(id).reason) : "absent" }

  property var plan: [
    [1600, "center starts hidden", function () { return [c.visible === false, "" + c.visible] }],
    [1600, "center lists every record, bad ones too, with a remove command each", function () {
      const rows = c.rows
      const bad = rows.filter(function (x) { return x.id === "io.test.badnode" })
      return [rows.length === Object.keys(s.records).length && bad.length === 1 && bad[0].ok === false && bad[0].command === "ow remove io.test.badnode"
        && rows.every(function (x) { return x.command.indexOf("ow remove ") === 0 }), rows.length + " rows " + Object.keys(s.records).length + " records"] }],
    [1600, "center shows the daemon as not running when there is no bridge", function () { return [c.rows.filter(function (x) { return x.ok }).every(function (x) { return x.summary === "daemon not running" }), ""] }],
    [1700, "toggle shows it", function () { c.toggle(); return [true, ""] }],
    [1900, "center visible after toggle", function () { return [c.open === true, "" + c.open] }],
    [1950, "toggle hides it", function () { c.toggle(); return [true, ""] }],
    [2100, "center hidden after second toggle", function () { return [c.visible === false, "" + c.visible] }],
    [2150, "show then hide()", function () { c.toggle(); c.hide(); return [true, ""] }],
    [2300, "hide() closes", function () { return [c.visible === false, "" + c.visible] }],
    [1500, "good widget loads", function () { return [why("io.github.dexterhere.clock") === "ok", why("io.github.dexterhere.clock")] }],
    [1500, "all three real widgets load", function () { return [why("io.github.dexterhere.system-monitor") === "ok" && why("io.github.dexterhere.battery") === "ok", Object.keys(s.records).join()] }],
    [1500, "oversize layout is rejected without being read", function () { return [why("io.test.huge") === "layout-type", why("io.test.huge")] }],
    [1500, "unknown node type rejected", function () { return [why("io.test.badnode") === "layout", why("io.test.badnode")] }],
    [1500, "id mismatch rejected", function () { return [why("io.test.mismatch") === "manifest", why("io.test.mismatch")] }],
    [1500, "code widget rejected", function () { return [why("io.test.code") === "tier", why("io.test.code")] }],
    [1500, "missing file rejected", function () { return [why("io.test.missing") === "layout-type", why("io.test.missing")] }],
    [1500, "empty file rejected", function () { return [why("io.test.empty") === "widget-type", why("io.test.empty")] }],
    [1500, "depth bomb rejected", function () { return [why("io.test.bomb") === "layout-size" || why("io.test.bomb") === "layout-depth", why("io.test.bomb")] }],
    [1500, "hidden stage dir and bad names never become records", function () { const k = Object.keys(s.records); return [k.every(function (x) { return x.indexOf("stage") < 0 && x.indexOf(" ") < 0 && x.indexOf("Bad") < 0 }), k.join()] }],
    [1500, "invalid directory names are counted, not used", function () { return [s.skippedDirs >= 1, "" + s.skippedDirs] }],
    [1500, "failure records carry only id, ok and a fixed reason", function () { const x = r("io.test.badnode"); return [Object.keys(x).sort().join() === "id,ok,reason", JSON.stringify(x)] }],
    [1500, "wanted lists only valid widgets, declared topics", function () {
      const w = s.wanted.map(function (x) { return x.widget + " " + x.topic }).sort().join("|")
      return [w.indexOf("io.test") < 0 && w.indexOf("io.github.dexterhere.clock system.time") >= 0 && w.indexOf("battery.status") >= 0, w] }],
    [1600, "break the clock layout on disk", function () { h.mutate("break"); return [true, ""] }],
    [3200, "re-read after change: clock now unavailable", function () { return [why("io.github.dexterhere.clock") === "layout", why("io.github.dexterhere.clock")] }],
    [3200, "clock no longer subscribed", function () { return [s.wanted.every(function (x) { return x.widget !== "io.github.dexterhere.clock" }), ""] }],
    [3300, "fix it again", function () { h.mutate("fix"); return [true, ""] }],
    [5000, "recovers", function () { return [why("io.github.dexterhere.clock") === "ok", why("io.github.dexterhere.clock")] }],
    [5100, "remove battery dir", function () { h.mutate("remove"); return [true, ""] }],
    [6800, "removed widget disappears", function () { return [r("io.github.dexterhere.battery") === undefined, why("io.github.dexterhere.battery")] }],
    [6900, "install a new widget (staged then renamed)", function () { h.mutate("add"); return [true, ""] }],
    [8600, "new widget appears", function () { return [why("io.test.added") === "ok", why("io.test.added")] }],
    [8600, "rescan() keeps records stable", function () { s.rescan(); return [true, ""] }],
    [9800, "still consistent after rescan", function () { return [why("io.github.dexterhere.clock") === "ok" && why("io.test.added") === "ok", Object.keys(s.records).join()] }],
    [10000, "grow the clock layout to 1.5 GB in place", function () { h.mutate("grow"); return [true, ""] }],
    [12500, "a file grown in place is refused, not read", function () { return [why("io.github.dexterhere.clock") === "layout-type", why("io.github.dexterhere.clock")] }],
    [12600, "shrink it back", function () { h.mutate("fix"); return [true, ""] }],
    [15000, "readmitted once the listing shows an acceptable size", function () { return [why("io.github.dexterhere.clock") === "ok", why("io.github.dexterhere.clock")] }]
  ]
  function mutate(what) {
    // the driver script watches this file and performs the change, so the harness stays read-only
    console.log("MUTATE " + what)
  }
  Timer {
    interval: 50; repeat: true; running: true
    onTriggered: {
      h.t += 50
      for (let i = 0; i < h.plan.length; i++) {
        if (h.t >= h.plan[i][0] && h.step["s" + i] !== true) {
          h.step["s" + i] = true
          let res
          try { res = h.plan[i][2]() } catch (err) { res = [false, "threw " + err] }
          h.check(h.plan[i][1], res[0], res[1])
        }
      }
      if (h.t >= 15500) { console.log("RESULT store passed=" + h.passed + " failed=" + h.failed); Qt.quit() }
    }
  }
}
