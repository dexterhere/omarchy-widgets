// SPIKE S1: Bottom-layer canvas surface, click-through except one rectangle.
// Standalone throwaway config (run with `quickshell -p spikes/s1`, see ../README.md).
// The same PanelWindow block is what a service plugin would instantiate; the
// shell hosts plugin roots in the same engine, so nothing here is config-specific.
import QtQuick
import Quickshell
import Quickshell.Wayland

ShellRoot {
  // One canvas surface per monitor (multi-monitor check).
  Variants {
    model: Quickshell.screens
    PanelWindow {
      id: canvas
      required property var modelData
      screen: modelData

      // Full-screen, transparent, never pushes other layers around.
      anchors { top: true; bottom: true; left: true; right: true }
      color: "transparent"
      exclusionMode: ExclusionMode.Ignore

      WlrLayershell.namespace: "ow-spike-canvas"
      WlrLayershell.layer: WlrLayer.Bottom
      WlrLayershell.keyboardFocus: WlrKeyboardFocus.None   // never take keyboard focus

      // Input mask: ONLY the box receives pointer input. Everywhere else the
      // compositor routes the click to whatever is underneath.
      mask: Region { item: box }

      Rectangle {
        id: box
        x: 40; y: 200; width: 160; height: 90
        radius: 12
        color: Qt.rgba(0.4, 0.6, 1, 0.55)
        border.color: "white"; border.width: 1
        Text { anchors.centerIn: parent; color: "white"; text: "ow-spike S1\n" + canvas.screen.name }
        MouseArea { anchors.fill: parent; onClicked: console.log("S1 CLICK in box on " + canvas.screen.name) }
      }
      // Whole-surface MouseArea: if the mask did not work this would log
      // clicks outside the box. It must stay silent.
      MouseArea { anchors.fill: parent; z: -1; onClicked: console.log("S1 LEAK click outside box") }
    }
  }
}
