pragma ComponentBehavior: Bound
import QtQuick
import Quickshell
import Quickshell.Wayland
import "Layout.js" as L

// The desktop surface: one Bottom-layer layer-shell window per monitor (SPIKE-S1), transparent,
// no keyboard focus, no exclusive zone, namespace "owd-canvas" so a user layer rule can
// target it. Only the tiles accept pointer input (mask = union of the tile rectangles); the
// rest of the surface lets clicks through to the desktop. A monitor with no tile gets no
// visible window at all.
//
// Not verified on a live session: actual click pass-through, lock screen, fullscreen
// windows (docs/SPIKES.md S1, OPEN_QUESTIONS 19). No edit mode in this slice.
Item {
  id: canvas
  property var store: null
  property var bridge: null
  property var placement: ({})

  readonly property var screensInfo: Quickshell.screens.map(function (s) { return { name: s.name, width: s.width, height: s.height } })
  readonly property var tiles: L.layoutTiles(store ? store.list : [], placement, screensInfo)

  Variants {
    model: Quickshell.screens
    delegate: PanelWindow {
      id: win
      required property var modelData
      screen: modelData

      readonly property var mine: canvas.tiles.filter(function (t) { return t.monitor === win.modelData.name })
      property var regionList: []
      // qmllint disable missing-property
      function rebuildRegions() {
        const out = []
        for (let i = 0; i < reps.count; i++) { const it = reps.itemAt(i); if (it) out.push(it.region) }
        regionList = out
      }
      // qmllint enable missing-property

      visible: mine.length > 0
      anchors { top: true; bottom: true; left: true; right: true }
      color: "transparent"
      exclusionMode: ExclusionMode.Ignore
      WlrLayershell.namespace: "owd-canvas"
      WlrLayershell.layer: WlrLayer.Bottom
      WlrLayershell.keyboardFocus: WlrKeyboardFocus.None

      mask: Region { regions: win.regionList }

      Repeater {
        id: reps
        model: win.mine.length
        onCountChanged: win.rebuildRegions()
        delegate: Item {
          id: cell
          required property int index
          readonly property var desc: win.mine[index]
          readonly property alias region: hit
          x: desc.x; y: desc.y; width: desc.w; height: desc.h
          Component.onCompleted: win.rebuildRegions()
          Region { id: hit; item: tile }
          Tile {
            id: tile
            anchors.fill: parent
            record: canvas.store ? canvas.store.records[cell.desc.id] : null
            entries: canvas.bridge ? canvas.bridge.entries : ({})
            bridgeState: canvas.bridge ? canvas.bridge.link : "unavailable"
            variant: cell.desc.variant
          }
        }
      }
    }
  }
}
