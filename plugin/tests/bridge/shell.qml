// Bridge test harness. Runs inside a throwaway Quickshell (see ../run-bridge-tests.sh), against
// tests/fake_owd.py. Prints PASS/FAIL lines and RESULT, then exits ITS OWN process with Qt.quit();
// plugin code never does that.
import QtQuick
import Quickshell
import "plugin"

ShellRoot {
  id: h
  property string scenario: Quickshell.env("OW_TEST_SCENARIO")
  property int passed: 0
  property int failed: 0
  property int t: 0
  property var history: []
  property var done: ({})

  property var base: [{ widget: "io.test.ok", topic: "x.one" }, { widget: "io.test.ok", topic: "x.two" }]
  property var normalWanted: base.concat([{ widget: "io.test.ok", topic: "x.denied" }, { widget: "io.test.ok", topic: "x.locked" },
    { widget: "io.test.ok", topic: "x.unknown" }, { widget: "Bad Id", topic: "x.one" }, { widget: "io.test.ok", topic: "NOT A TOPIC" },
    { widget: "io.test.ok", topic: "x.one" }, null, "str", { widget: "io.test.ok\n", topic: "x.one" }])

  Bridge {
    id: b
    socketPath: h.scenario === "defaultpath" || h.scenario === "nopath" ? b.defaultSocketPath : Quickshell.env("OW_TEST_SOCK")
    wanted: h.scenario === "normal" ? h.normalWanted : h.base
    retryLockedMs: 1000
    onLinkChanged: h.history = h.history.concat([{ t: h.t, link: link }])
  }

  function key(w, t) { return w + " " + t }
  function e(w, t) { return b.entries[key(w, t)] }
  function check(name, ok, detail) {
    if (ok) { passed++; console.log("PASS " + name) } else { failed++; console.log("FAIL " + name + " :: " + detail) }
  }
  function links() { return history.map(function (x) { return x.link }).join(">") }

  // [time in ms, name, function returning [ok, detail]]
  property var plan: ({
    normal: [
      [1500, "connected", function () { return [b.link === "connected", b.link] }],
      [1500, "daemon version read", function () { return [b.daemonVersion === "0.0-test", b.daemonVersion] }],
      [1600, "live data arrives", function () { const x = e("io.test.ok", "x.one"); return [x && (x.state === "live" || x.state === "stale") && x.data.n >= 1, JSON.stringify(x)] }],
      [1600, "denied topic is marked denied", function () { return [e("io.test.ok", "x.denied").state === "denied", JSON.stringify(e("io.test.ok", "x.denied"))] }],
      [1600, "locked topic is marked locked", function () { return [e("io.test.ok", "x.locked").state === "locked", JSON.stringify(e("io.test.ok", "x.locked"))] }],
      [1600, "unknown topic is marked unknown", function () { return [e("io.test.ok", "x.unknown").state === "unknown", JSON.stringify(e("io.test.ok", "x.unknown"))] }],
      [1600, "invalid wanted entries were never requested", function () { return [Object.keys(b.entries).length === 5, Object.keys(b.entries).join(",")] }],
      [2800, "stale push is seen as stale", function () { return [h.done.sawStale === true, "never saw stale"] }],
      [3000, "wanted changes: drop x.two, add x.three", function () { b.wanted = [{ widget: "io.test.ok", topic: "x.one" }, { widget: "io.test.ok", topic: "x.three" }]; return [true, ""] }],
      [4200, "removed topic is gone, new topic live", function () { return [e("io.test.ok", "x.two") === undefined && e("io.test.ok", "x.three") && e("io.test.ok", "x.three").data !== undefined, JSON.stringify(b.entries)] }],
      [4200, "no malformed counted for a clean stream", function () { return [b.malformed === 0 && b.oversize === 0, b.malformed + "/" + b.oversize] }]
    ],
    fragment: [
      [2800, "partial lines and coalesced lines are reassembled", function () { return [b.pushes >= 8, "pushes=" + b.pushes] }],
      [2800, "no message was lost or corrupted into a malformed line", function () { return [b.malformed === 0, "malformed=" + b.malformed] }],
      [2800, "multi-byte text split across chunks survives", function () { const x = e("io.test.ok", "x.one"); return [x && x.data.name === "héllo ✓ 😀 end" || h.done.sawBigN === true, JSON.stringify(x)] }]
    ],
    garbage: [
      [2500, "still connected after garbage", function () { return [b.link === "connected", b.link] }],
      [2500, "garbage is counted and dropped", function () { return [b.malformed >= 5, "malformed=" + b.malformed] }],
      [2500, "valid pushes still applied", function () { const x = e("io.test.ok", "x.one"); return [x && x.state === "live" && x.data.n >= 2, JSON.stringify(x)] }],
      [2500, "unknown sub and wrong-topic pushes never applied", function () { return [h.done.saw555 !== true && h.done.saw666 !== true, JSON.stringify(h.done)] }],
      [2500, "nested object field dropped, finite number kept", function () { const x = e("io.test.ok", "x.one"); return [x && x.data.nested === undefined && x.data.bad === 1e308, JSON.stringify(x)] }],
      [2500, "prototype untouched", function () { return [({}).n === undefined && ({}).constructor === Object, ""] }]
    ],
    oversize: [
      [3000, "oversize line counted", function () { return [b.oversize >= 1, "oversize=" + b.oversize] }],
      [3000, "stream continues after the oversize line", function () { const x = e("io.test.ok", "x.one"); return [b.link === "connected" && x && x.data.n >= 4, JSON.stringify(x) + " " + b.link] }]
    ],
    drop: [
      [600, "first connection up", function () { return [b.connects === 1 && b.link === "connected", b.connects + " " + b.link] }],
      [1500, "daemon gone: link unavailable, last data kept", function () { const x = e("io.test.ok", "x.one"); return [b.link !== "connected" && x && x.data !== undefined, b.link + " " + JSON.stringify(x)] }],
      [6000, "reconnected by itself", function () { return [b.connects >= 2 && b.link === "connected", b.connects + " " + b.link] }],
      [6000, "subscriptions re-sent and data live again", function () { const x = e("io.test.ok", "x.one"); return [x && (x.state === "live" || x.state === "stale") && x.data.n >= 1, JSON.stringify(x)] }],
      [6000, "state sequence", function () { const l = h.links(); return [l.indexOf("connected>unavailable") >= 0 && l.lastIndexOf("connected") > l.indexOf("unavailable"), l] }],
      [6000, "no tight retry loop while the daemon was down", function () { const n = h.history.filter(function (x) { return x.link === "connecting" }).length; return [n <= 6, "connecting x" + n + " " + h.links()] }]
    ],
    silent: [
      [1500, "waiting for hello", function () { return [b.link === "connecting", b.link] }],
      [4500, "hello timeout gives denied", function () { return [b.link === "denied" || b.link === "connecting", b.link + " " + h.links()] }],
      [8000, "retries with backoff, not in a loop", function () { return [b.link !== "connected", b.link] }]
    ],
    reject: [
      [1500, "closed before hello answers: denied", function () { return [b.link === "denied" || b.link === "connecting", h.links()] }],
      [7000, "never connected", function () { return [b.connects === 0, "" + b.connects] }]
    ],
    refuse: [
      [1500, "hello denied", function () { return [b.link === "denied" || b.link === "connecting", h.links()] }],
      [1500, "no hello success", function () { return [b.connects === 0, "" + b.connects] }]
    ],
    unsupported: [
      [1500, "unsupported version is unavailable, not connected", function () { return [b.link !== "connected" && b.connects === 0, h.links()] }]
    ],
    locked: [
      [1500, "locked state", function () { return [e("io.test.ok", "x.one").state === "locked", JSON.stringify(e("io.test.ok", "x.one"))] }],
      [4200, "locked topics are asked for again", function () { return [b.link === "connected", b.link] }]
    ],
    late: [
      [1500, "no daemon yet: unavailable, topics pending", function () { return [b.link !== "connected" && e("io.test.ok", "x.one").state === "pending", h.links() + JSON.stringify(b.entries)] }],
      [9000, "connects when the daemon appears", function () { return [b.link === "connected" && b.connects === 1, h.links()] }],
      [9000, "data flows", function () { const x = e("io.test.ok", "x.one"); return [x && x.data !== undefined, JSON.stringify(x)] }]
    ],
    defaultpath: [
      [1500, "socket path came from XDG_RUNTIME_DIR", function () { return [b.socketPath === Quickshell.env("XDG_RUNTIME_DIR") + "/owd/owd.sock" && b.socketPath === b.defaultSocketPath && b.link === "connected", b.socketPath + " " + b.link] }]
    ],
    nopath: [
      [1500, "no runtime dir: unavailable, no crash", function () { return [b.socketPath === "" && b.link === "unavailable", b.socketPath + b.link] }]
    ]
  })
  property var endAt: ({ normal: 4600, fragment: 3000, garbage: 2800, oversize: 3300, drop: 6300, silent: 8300, reject: 7300, refuse: 1800,
                         unsupported: 1800, locked: 4500, late: 9300, defaultpath: 1800, nopath: 1800 })

  // sample for facts a single end-of-run check would miss
  function sample() {
    const x = e("io.test.ok", "x.one")
    if (x && x.state === "stale") done.sawStale = true
    if (x && x.data && x.data.n === 555) done.saw555 = true
    if (x && x.data && x.data.n === 666) done.saw666 = true
    if (x && x.data && x.data.n >= 1000) done.sawBigN = true
  }

  Timer {
    interval: 50; repeat: true; running: true
    onTriggered: {
      h.t += 50
      h.sample()
      const list = h.plan[h.scenario]
      if (list === undefined) { console.log("FAIL unknown scenario"); Qt.quit(); return }
      for (let i = 0; i < list.length; i++) {
        if (h.t >= list[i][0] && h.done["s" + i] !== true) {
          h.done["s" + i] = true
          let r
          try { r = list[i][2]() } catch (err) { r = [false, "threw " + err] }
          h.check(h.scenario + ": " + list[i][1], r[0], r[1])
        }
      }
      if (h.t >= h.endAt[h.scenario]) {
        console.log("RESULT " + h.scenario + " passed=" + h.passed + " failed=" + h.failed + " links=" + h.links() + " pushes=" + b.pushes)
        Qt.quit()
      }
    }
  }
}
