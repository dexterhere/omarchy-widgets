// Run: QT_QPA_PLATFORM=offscreen qmltestrunner -input tst_edit.qml   (no Wayland, no live desktop)
import QtQuick
import QtTest

Item {
  width: 800; height: 600
  EditCanvas { id: c; anchors.fill: parent
    layout: ({ a: { x: 100, y: 100, w: 200, h: 120 } }) }
  SignalSpy { id: commits; target: c; signalName: "committed" }
  SignalSpy { id: exits; target: c; signalName: "exitRequested" }

  TestCase {
    name: "EditCanvas"; when: windowShown
    function init() { commits.clear(); exits.clear(); c.layout = ({ a: { x: 100, y: 100, w: 200, h: 120 } }); c.selected = "" }

    function test_drag_moves_and_commits() {
      mouseDrag(c, 150, 150, 60, 40, Qt.LeftButton)       // drag the tile body
      // Qt's startDragDistance (~10px) is swallowed before the drag begins, so allow that slack.
      verify(Math.abs(c.layout.a.x - 160) <= 10 && c.layout.a.x > 100, "x=" + c.layout.a.x)
      verify(Math.abs(c.layout.a.y - 140) <= 10 && c.layout.a.y > 100, "y=" + c.layout.a.y)
      verify(commits.count >= 1)
      var j = JSON.parse(commits.signalArguments[commits.count - 1][0])
      compare(j.version, 1); compare(j.widgets.a.x, c.layout.a.x)
    }
    function test_resize_handle() {
      mouseDrag(c, 100 + 200 - 8, 100 + 120 - 8, 50, 30, Qt.LeftButton)   // bottom-right handle
      compare(c.layout.a.w, 250); compare(c.layout.a.h, 150)
      compare(c.layout.a.x, 100)
    }
    function test_resize_respects_min_and_bounds() {
      c.selected = "a"; c.resize("a", 5, 5)
      compare(c.layout.a.w, 40); compare(c.layout.a.h, 40)
      c.resize("a", 5000, 5000)
      compare(c.layout.a.w, 700); compare(c.layout.a.h, 500)   // clamped to the surface
    }
    function test_keyboard_nudge() {
      c.selected = "a"; c.forceActiveFocus()
      keyClick(Qt.Key_Right);                          compare(c.layout.a.x, 101)
      keyClick(Qt.Key_Down, Qt.ShiftModifier);         compare(c.layout.a.y, 110)
    }
    function test_escape_exits() {
      c.forceActiveFocus(); keyClick(Qt.Key_Escape)
      compare(exits.count, 1)
    }
  }
}
