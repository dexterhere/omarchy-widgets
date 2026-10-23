import QtQuick
import QtTest
import "../Protocol.js" as P

TestCase {
  name: "Protocol"

  function line(o) { return JSON.stringify(o) }

  function test_requests_are_single_lines() {
    var h = P.hello(1)
    verify(h.charAt(h.length - 1) === "\n" && h.indexOf("\n") === h.length - 1)
    var o = JSON.parse(h)
    compare(o.v, 0); compare(o.op, "hello"); compare(o.client, "bridge")
    var s = JSON.parse(P.subscribe(7, "io.github.dexterhere.clock", "system.time"))
    compare(s.op, "subscribe"); compare(s.widget, "io.github.dexterhere.clock"); compare(s.topic, "system.time"); compare(s.id, 7)
    compare(JSON.parse(P.unsubscribe(8, 3)).sub, 3)
  }
  function test_requests_refuse_malformed_arguments() {
    compare(P.subscribe(1, "Bad Id", "system.time"), null)
    compare(P.subscribe(1, "../etc", "system.time"), null)
    compare(P.subscribe(1, "a.b", "system.time\n{\"op\":\"vault.unlock\"}"), null)    // no frame injection
    compare(P.subscribe(1, "a.b\n", "system.time"), null)
    compare(P.subscribe(1, "a.b", "vault"), null)
    compare(P.subscribe(1, "a.b", 5), null)
    compare(P.unsubscribe(1, -1), null)
    compare(P.unsubscribe(1, 1.5), null)
    compare(P.unsubscribe(1, "3"), null)
  }
  function test_no_credentials_in_any_request() {
    // the client never builds an admin or credential frame
    var all = P.hello(1) + P.subscribe(2, "a.b", "x.y") + P.unsubscribe(3, 1)
    verify(all.indexOf("vault") < 0 && all.indexOf("passphrase") < 0 && all.indexOf("secret") < 0 && all.indexOf("token") < 0)
  }

  function test_backoff() {
    compare(P.backoffMs(0), 500)
    compare(P.backoffMs(1), 1000)
    compare(P.backoffMs(2), 2000)
    compare(P.backoffMs(4), 8000)
    compare(P.backoffMs(5), 10000)
    compare(P.backoffMs(50), 10000)
    compare(P.backoffMs(-3), 500)
    compare(P.backoffMs(NaN), 500)
    verify(P.backoffMs(5, 0) >= 9000 && P.backoffMs(5, 0.999) <= 11000)
    var prev = 0
    for (var i = 0; i < 12; i++) { var b = P.backoffMs(i); verify(b >= prev); prev = b }
  }

  function test_hello_reply() {
    var e = P.parseLine(line({ v: 0, id: 1, ok: true, daemonVersion: "0.1.0", protocol: 0 }))
    compare(e.kind, "ok"); compare(e.id, 1); compare(e.daemonVersion, "0.1.0"); compare(e.protocol, 0)
    var s = P.parseLine(line({ v: 0, id: 3, ok: true, sub: 7 }))
    compare(s.kind, "ok"); compare(s.sub, 7)
  }
  function test_push() {
    var e = P.parseLine(line({ v: 0, sub: 7, topic: "system.cpu", ts: 1760700000, stale: false, data: { usage_pct: 12.5 } }))
    compare(e.kind, "push"); compare(e.sub, 7); compare(e.topic, "system.cpu"); compare(e.stale, false); compare(e.data.usage_pct, 12.5)
    var st = P.parseLine(line({ v: 0, sub: 7, topic: "system.cpu", ts: 5, stale: true, data: {} }))
    compare(st.stale, true)
    compare(P.parseLine(line({ v: 0, sub: 7, topic: "system.cpu", stale: "true", data: {} })).stale, false)     // only a real true counts
    compare(P.parseLine(line({ v: 0, sub: 7, topic: "system.cpu", ts: -1, data: {} })).ts, 0)
    compare(P.parseLine(line({ v: 0, sub: 7, topic: "system.cpu", ts: "x", data: {} })).ts, 0)
    compare(P.parseLine(line({ v: 0, sub: 7, topic: "system.cpu" })).kind, "push")                           // missing data -> empty
  }
  function test_errors() {
    var e = P.parseLine(line({ v: 0, id: 3, error: { code: "denied", topic: "github.prs" } }))
    compare(e.kind, "error"); compare(e.code, "denied"); compare(e.topic, "github.prs")
    ;["bad_request", "denied", "unknown_topic", "provider_error", "rate_limited", "locked", "unsupported_version", "internal"].forEach(function (c) {
      compare(P.parseLine(line({ v: 0, id: 1, error: { code: c } })).code, c)
    })
    compare(P.parseLine(line({ v: 0, id: 1, error: { code: "totally_new" } })).code, "internal")
    compare(P.parseLine(line({ v: 0, id: 1, error: { code: "<b>x</b>", topic: "<script>" } })).topic, "")
    compare(P.parseLine(line({ v: 0, id: 1, error: "denied" })).kind, "malformed")
  }

  // ---- hostile or broken lines
  function test_malformed_is_ignored_not_thrown() {
    ;["not json", "{", "}", "[]", "[1,2]", "null", "5", "\"str\"", "{}", "{\"v\":1,\"id\":1,\"ok\":true}",
      "{\"v\":0}", "{\"v\":0,\"id\":-1,\"ok\":true}", "{\"v\":0,\"id\":1.5,\"ok\":true}", "{\"v\":0,\"id\":\"1\",\"ok\":true}",
      "{\"v\":0,\"id\":1}", "{\"v\":0,\"id\":1,\"ok\":false}", "{\"v\":0,\"sub\":-1,\"topic\":\"a.b\"}",
      "{\"v\":0,\"sub\":1,\"topic\":\"NOPE\"}", "\u0000", "﻿{\"v\":0}"].forEach(function (l) {
      compare(P.parseLine(l).kind, "malformed", l)
    })
    compare(P.parseLine("").kind, "ignore")
    compare(P.parseLine(undefined).kind, "malformed")
    compare(P.parseLine(5).kind, "malformed")
    compare(P.parseLine(null).kind, "malformed")
  }
  function test_numbers_that_overflow_json_are_rejected() {
    compare(P.parseLine("{\"v\":0,\"sub\":1,\"topic\":\"a.b\",\"data\":{\"x\":1e999}}").kind, "malformed")
    compare(P.parseLine("{\"v\":0,\"sub\":1,\"topic\":\"a.b\",\"data\":{\"x\":NaN}}").kind, "malformed")
    compare(P.parseLine("{\"v\":0,\"sub\":1,\"topic\":\"a.b\",\"data\":{\"x\":Infinity}}").kind, "malformed")
  }
  function test_oversize_line() {
    var big = "{\"v\":0,\"sub\":1,\"topic\":\"a.b\",\"data\":{\"x\":\"" + new Array(70000).join("a") + "\"}}"
    compare(P.parseLine(big).kind, "oversize")
    var just = "{\"v\":0,\"sub\":1,\"topic\":\"a.b\",\"data\":{\"x\":\"" + new Array(65000).join("a") + "\"}}"
    compare(P.parseLine(just).kind, "push")
    compare(P.parseLine(just).data.x.length, 4096)           // a field is cut to 4096 characters
  }
  function test_nesting_bomb() {
    var bomb = "{\"v\":0,\"sub\":1,\"topic\":\"a.b\",\"data\":" + new Array(20000).join("[") + "}"
    compare(P.parseLine(bomb).kind, "malformed")
  }
  function test_clean_data_shapes() {
    var d = P.cleanData({ a: 1, b: "x", c: [1, "y", {}, null, NaN], d: { nested: 1 }, e: true, f: null, g: undefined,
                          h: NaN, i: Infinity, j: -Infinity, "9bad": 1, "bad key": 2, "__proto__": 5 })
    compare(Object.keys(d).sort().join(), "a,b,c")
    compare(d.c.join(), "1,y")
    compare(P.cleanData([1, 2]) instanceof Array, false)
    compare(Object.keys(P.cleanData("str")).length, 0)
    compare(Object.keys(P.cleanData(null)).length, 0)
    var many = {}; for (var i = 0; i < 500; i++) many["f" + i] = i
    compare(Object.keys(P.cleanData(many)).length, 64)
    var arr = P.cleanData({ s: new Array(500).fill(1) })
    compare(arr.s.length, 50)
  }
  function test_prototype_pollution_is_inert() {
    var e = P.parseLine("{\"v\":0,\"sub\":1,\"topic\":\"a.b\",\"data\":{\"__proto__\":{\"polluted\":1},\"constructor\":{\"prototype\":{\"polluted\":1}},\"ok\":2}}")
    compare(e.kind, "push")
    compare(Object.keys(e.data).join(), "ok")
    compare(({}).polluted, undefined)
    var f = P.parseLine("{\"__proto__\":{\"polluted\":2},\"v\":0,\"id\":1,\"ok\":true}")
    compare(({}).polluted, undefined)
  }
  function test_hostile_strings_survive_as_data_only() {
    var e = P.parseLine(line({ v: 0, sub: 1, topic: "a.b", data: { t: "import QtQuick 2.0; Item {}", u: "<img src=x onerror=1>", w: "**md**", r: "‮evil" } }))
    compare(e.data.t, "import QtQuick 2.0; Item {}")        // kept as characters; the renderer draws them as plain text
    compare(e.data.r, "‮evil")                          // bidi stripping happens at display time (Layout.plainText)
  }
  function test_daemon_version_is_sanitised() {
    compare(P.parseLine(line({ v: 0, id: 1, ok: true, daemonVersion: "1.0‮<b>" })).daemonVersion, "1.0<b>")
    compare(P.parseLine(line({ v: 0, id: 1, ok: true, daemonVersion: new Array(500).join("x") })).daemonVersion.length, 32)
  }
}
