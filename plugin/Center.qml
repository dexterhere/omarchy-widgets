pragma ComponentBehavior: Bound
import QtQuick
import Quickshell
import Quickshell.Wayland
import "CenterModel.js" as Model

// The Widget Center overlay (SPIKE-S6: one plugin, service plus overlay). READ-ONLY (WIDGET-CENTER-V0
// in docs/DECISIONS.md): it lists what the Store found and what the Bridge knows and prints the
// command to remove each widget. It starts no process, writes no file and sends nothing itself.
//
// How the shell shows and hides an overlay plugin is NOT established in this repo (docs/WIDGET_CENTER.md).
// So this item exposes `visible` and `toggle()` and stops there; whatever the shell uses to summon
// it has to set one of them. Keyboard: OnDemand, so Escape arrives only after one click in the
// panel (SPIKES S2). The Close button works without it.
Item {
  id: root

  // injected by omarchy-shell (OPEN_QUESTIONS 20, 30: not verified in the real shell)
  property var shell: null
  property var manifest: null
  property string omarchyPath: ""
  property var service: null

  visible: false
  function toggle() { visible = !visible }
  function hide() { visible = false }

  readonly property var store: service ? service.store : null
  readonly property var bridge: service ? service.bridge : null
  readonly property var rows: Model.rows(store ? store.list : [], bridge ? bridge.entries : ({}), bridge ? bridge.link : "unavailable")

  readonly property alias window: win     // for the offscreen harness

  PanelWindow {
    id: win
    visible: root.visible
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
      link: root.bridge ? root.bridge.link : "unavailable"
      onCloseRequested: root.hide()
    }
  }
}
