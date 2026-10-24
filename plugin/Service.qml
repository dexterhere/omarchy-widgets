import QtQuick
import Quickshell
import Quickshell.Io
import "Layout.js" as L

// Entry point of the service kind. Owns the widget store, the daemon bridge and the desktop
// canvas. keepLoaded is set in the manifest so a plugin rescan does not drop the daemon
// connection (SPIKE-S3). This plugin never calls Qt.quit() and never writes outside its own
// state dir (it does not write at all in this slice).
Item {
  id: root

  // injected by omarchy-shell
  property var shell: null
  property var manifest: null
  property string omarchyPath: ""

  readonly property alias store: widgets
  readonly property alias bridge: link

  // plugin-owned placement state, NOT under ~/.config: $XDG_STATE_HOME/ow-shell/canvas.json
  readonly property string stateHome: Quickshell.env("XDG_STATE_HOME").charAt(0) === "/" ? Quickshell.env("XDG_STATE_HOME") : (Quickshell.env("HOME") + "/.local/state")
  property string placementPath: stateHome + "/ow-shell/canvas.json"
  property var placement: ({})

  Store { id: widgets }
  Bridge { id: link; wanted: widgets.wanted }
  DesktopCanvas { store: widgets; bridge: link; placement: root.placement }

  FileView {
    path: root.placementPath
    watchChanges: true
    printErrors: false
    onFileChanged: reload()
    onLoaded: root.placement = L.parsePlacement(text())
    onLoadFailed: root.placement = ({})        // no file yet: default positions
  }
}
