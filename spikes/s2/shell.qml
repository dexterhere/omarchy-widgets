// SPIKE S2 live part: Overlay-layer edit surface that persists layout.json.
// Deliberately NOT full-screen and OnDemand focus: this is a throwaway run on a live desktop.
// The real edit mode would be full-screen; the layer/keyboard properties are identical.
// Drag and Escape logic are covered by tst_edit.qml (offscreen); here we check layer, file write.
import QtQuick
import Quickshell
import Quickshell.Io
import Quickshell.Wayland

ShellRoot {
  readonly property string out: Quickshell.env("OW_SPIKE_RUN") + "/layout.json"

  FileView {
    id: file
    path: out
    atomicWrites: true
    printErrors: true
    onSaved: console.log("S2 SAVED " + out)
    onSaveFailed: function(e) { console.log("S2 SAVE FAILED " + e) }
  }

  PanelWindow {
    anchors { top: true; left: true }
    margins { top: 120; left: 120 }
    implicitWidth: 520; implicitHeight: 320
    color: Qt.rgba(0, 0, 0, 0.25)
    exclusionMode: ExclusionMode.Ignore
    WlrLayershell.namespace: "ow-spike-edit"
    WlrLayershell.layer: WlrLayer.Overlay
    WlrLayershell.keyboardFocus: WlrKeyboardFocus.OnDemand   // focus only when the user clicks it

    EditCanvas {
      id: canvas
      anchors.fill: parent
      layout: ({ a: { x: 40, y: 40, w: 200, h: 120 } })
      onCommitted: function(json) { file.setText(json) }
      onExitRequested: { console.log("S2 ESCAPE -> exit"); Qt.quit() }
    }
  }

  // Scripted "user": the same code paths the mouse/keyboard handlers call.
  Timer { interval: 1500; running: true; onTriggered: { canvas.selected = "a"; canvas.move("a", 30, 20); canvas.resize("a", 260, 150) } }
  Timer { interval: 4200; running: true; onTriggered: { console.log("S2 TIMEOUT quit"); Qt.quit() } }
}
