// Live canvas check (see ../run-e2e.sh with OW_E2E_LIVE=1): the real plugin Service, which opens
// its Bottom-layer window on the current compositor for a few seconds. Throwaway process only.
import QtQuick
import Quickshell
import "plugin"

ShellRoot {
  Service { id: svc }
  Timer { interval: 5500; running: true; onTriggered: { console.log("TILES " + JSON.stringify(svc.tiles)); console.log("LIVE widgets=" + svc.store.list.length + " link=" + svc.bridge.link + " pushes=" + svc.bridge.pushes); Qt.quit() } }
}
