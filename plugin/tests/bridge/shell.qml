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

  property var capsWanted: {
    const out = []
    for (let i = 0; i < 128; i++) out.push({ widget: "io.test.big", topic: "x.t" + i })
    out.push({ widget: "io.test.ok", topic: "x.one" }, { widget: "io.test.ok", topic: "x.two" }, { widget: "io.test.b", topic: "x.one" })
    return out
  }
  function count(state) { let n = 0; for (const k in b.entries) if (b.entries[k].state === state) n++; return n }
  function countFor(widget, notState) { let n = 0; for (const k in b.entries) if (k.indexOf(widget + " ") === 0 && b.entries[k].state !== notState) n++; return n }

  Bridge {
    id: b
    socketPath: h.scenario === "defaultpath" || h.scenario === "nopath" || h.scenario === "relpath" ? b.defaultSocketPath : Quickshell.env("OW_TEST_SOCK")
    wanted: h.scenario === "health" ? h.base.concat([{ widget: "io.test.ok", topic: "x.nocred" }]) : h.scenario === "normal" ? h.normalWanted : (h.scenario === "caps" ? h.capsWanted : h.base)
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
    health: [
      [1500, "live data keeps its value and carries the provider word", function () { const x = e("io.test.ok", "x.one"); return [x && x.state === "live" && x.data.n >= 1 && x.health === "expired_soon", JSON.stringify(x)] }],
      [1500, "a state-only push adds the word and invents no data", function () { const x = e("io.test.ok", "x.two"); return [x && x.health === "unauthorized" && x.data === undefined, JSON.stringify(x)] }],
      [1500, "an error with a provider word is kept", function () { const x = e("io.test.ok", "x.nocred"); return [x && x.state === "error" && x.health === "no_credential", JSON.stringify(x)] }],
      [1500, "no malformed lines", function () { return [b.malformed === 0, "" + b.malformed] }]
    ],
    caps: [
      [1800, "one widget with 128 topics is held to 16", function () { return [h.countFor("io.test.big", "limited") === 16 && h.countFor("io.test.big", "") === 128, h.countFor("io.test.big", "limited") + "/" + h.countFor("io.test.big", "")] }],
      [1800, "the other widgets are not starved", function () { const a = e("io.test.ok", "x.one"), c = e("io.test.b", "x.one"), d = e("io.test.ok", "x.two"); return [a && c && d && a.state !== "limited" && c.state !== "limited" && d.state !== "limited" && a.data !== undefined && c.data !== undefined, JSON.stringify([a, c, d])] }],
      [1800, "at most 30 subscriptions", function () { return [b.link === "connected" && Object.keys(b.keys).length <= 30 && Object.keys(b.keys).length === 19, "keys=" + Object.keys(b.keys).length] }],
      [2000, "widget goes away: its entries, limited ones included, are deleted", function () { b.wanted = [{ widget: "io.test.ok", topic: "x.one" }]; return [true, ""] }],
      [3200, "no orphan entries remain", function () { return [Object.keys(b.entries).join() === "io.test.ok x.one" && Object.keys(b.keys).length === 1, Object.keys(b.entries).length + " entries " + Object.keys(b.keys).length + " keys"] }],
      [3300, "room appears: previously limited topics are admitted by the retry", function () { b.wanted = h.capsWanted.slice(0, 20); return [true, ""] }],
      [3400, "20 wanted for one widget: 16 admitted, 4 limited", function () { return [h.countFor("io.test.big", "limited") === 16 && h.countFor("io.test.big", "") === 20, "" + h.count("limited")] }],
      [3500, "drop 8 of them", function () { b.wanted = h.capsWanted.slice(0, 20).filter(function (x, i) { return i >= 8 }); return [true, ""] }],
      [5200, "limited topics are retried and admitted once there is room", function () { return [h.count("limited") === 0 && h.countFor("io.test.big", "") === 12, "limited=" + h.count("limited")] }]
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
      [4500, "an over-long line is counted and never parsed", function () { return [b.oversizeTotal >= 1, "oversize=" + b.oversizeTotal] }],
      [4500, "the stream continues (same connection, or after a drop and reconnect)", function () { const x = e("io.test.ok", "x.one"); return [b.link === "connected" && x && x.data.n >= 4, b.connects + " " + b.link + " " + JSON.stringify(x)] }]
    ],
    floodpre: [
      [2500, "never-ending line before hello: dropped", function () { return [b.oversizeTotal >= 1 && b.link !== "connected", "oversize=" + b.oversizeTotal + " " + h.links()] }],
      [7500, "never connected, retries are spaced", function () { return [b.connects === 0, "" + b.connects] }]
    ],
    flood: [
      [2500, "never-ending line after hello: dropped", function () { return [b.connects >= 1 && b.oversizeTotal >= 1 && b.link !== "connected", "oversize=" + b.oversizeTotal + " " + h.links()] }],
      [7500, "buffered text was freed", function () { return [b.partial.length === 0, "partial=" + b.partial.length] }]
    ],
    burst: [
      [3500, "endless valid lines hit the byte-rate watchdog", function () { return [b.oversizeTotal >= 1 && h.links().indexOf("connected>unavailable") >= 0, "oversize=" + b.oversize + " " + h.links()] }]
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
    badproto: [
      [1800, "hello answered with another protocol number is refused", function () { return [b.connects === 0 && b.link !== "connected", b.connects + " " + h.links()] }]
    ],
    noproto: [
      [1800, "hello answered without a protocol number is refused", function () { return [b.connects === 0 && b.link !== "connected", b.connects + " " + h.links()] }]
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
    relpath: [
      [1500, "a relative XDG_RUNTIME_DIR is treated as unset", function () { return [b.defaultSocketPath === "" && b.link === "unavailable", b.defaultSocketPath + " " + b.link] }]
    ],
    nopath: [
      [1500, "no runtime dir: unavailable, no crash", function () { return [b.socketPath === "" && b.link === "unavailable", b.socketPath + b.link] }]
    ]
  })
  property var endAt: ({ normal: 4600, fragment: 3000, garbage: 2800, oversize: 4800, caps: 5400, floodpre: 7800, flood: 7800, burst: 3800, drop: 6300, silent: 8300, reject: 7300, refuse: 1800, badproto: 1900, noproto: 1900,
                         unsupported: 1800, locked: 4500, late: 9300, defaultpath: 1800, nopath: 1800, relpath: 1800 })

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
