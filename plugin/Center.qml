pragma ComponentBehavior: Bound
import QtQuick
import Quickshell
import Quickshell.Wayland

// The Widget Center overlay (SPIKE-S6: one plugin, service plus overlay). READ-ONLY (WIDGET-CENTER-V0
// in docs/DECISIONS.md): it lists what the Store found and what the Bridge knows and prints the
// command to remove each widget. It starts no process, writes no file and sends nothing itself.
//
// How the shell shows and hides an overlay plugin is NOT established in this repo (docs/WIDGET_CENTER.md).
// So this item exposes `open`, `visible` and `toggle()` and stops there; whatever the shell uses to summon
// it has to set one of them (`open` is the one that opens the window). Keyboard: OnDemand, so Escape arrives only after one click in the
// panel (SPIKES S2). The Close button works without it.
CenterState {
  id: root

  readonly property var window: lazy.item     // null while hidden; for tests

  // The window exists only while the Center is shown (LazyLoader is how Quickshell hosts a window under an Item).
  LazyLoader {
    id: lazy
    active: root.open
    component: PanelWindow {
      id: win
      visible: true
      implicitWidth: panel.implicitWidth
      implicitHeight: panel.implicitHeight
      color: "transparent"
      exclusionMode: ExclusionMode.Ignore
      WlrLayershell.namespace: "owd-center"
      WlrLayershell.layer: WlrLayer.Overlay
      WlrLayershell.keyboardFocus: WlrKeyboardFocus.OnDemand

      CenterPanel {
        id: panel
        anchors.fill: parent
        rows: root.rows
        hiddenCount: root.hiddenCount
        link: root.link
        onCloseRequested: root.hide()
      }
    }
  }
}
