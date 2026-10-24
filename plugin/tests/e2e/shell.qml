// End-to-end smoke (see ../run-e2e.sh): real Store + real Bridge + real Tiles, in an ordinary
// offscreen window (a layer-shell PanelWindow cannot exist offscreen), screenshot to a PNG.
import QtQuick
import QtQuick.Window
import Quickshell
import "plugin"
import "plugin/Layout.js" as L

ShellRoot {
  id: h
  property int t: 0
  property var screens: [{ name: "OFF-1", width: 760, height: 720 }]
  Store { id: store }
  Bridge { id: bridge; wanted: store.wanted }
  property var tiles: L.layoutTiles(store.list, ({}), screens)

  Window {
    id: win
    visible: true
    width: 760; height: 720
    color: Theme.background
    Item {
      id: stage
      anchors.fill: parent
      Repeater {
        model: h.tiles.length
        delegate: Item {
          id: cell
          required property int index
          readonly property var d: h.tiles[index]
          x: d.x; y: d.y; width: d.w; height: d.h
          Tile {
            anchors.fill: parent
            record: store.records[cell.d.id]
            entries: bridge.entries
            bridgeState: bridge.link
            variant: cell.d.variant
          }
        }
      }
    }
  }

  function dump() {
    console.log("LINK " + bridge.link + " daemon=" + bridge.daemonVersion + " pushes=" + bridge.pushes)
    for (const r of store.list) console.log("WIDGET " + r.id + " " + (r.ok ? "ok topics=" + r.topics.join(",") : "unavailable:" + r.reason))
    for (const k in bridge.entries) {
      const e = bridge.entries[k]
      console.log("ENTRY " + k + " " + e.state + (e.data ? " " + JSON.stringify(e.data) : ""))
    }
  }
  Timer {
    interval: 100; repeat: true; running: true
    onTriggered: {
      h.t += 100
      if (h.t === 4000) {
        h.dump()
        win.contentItem.grabToImage(function (res) {
          console.log("SAVED " + res.saveToFile(Quickshell.env("OW_E2E_PNG")))
          Qt.quit()
        })
      }
      if (h.t > 9000) { console.log("TIMEOUT"); Qt.quit() }
    }
  }
}
