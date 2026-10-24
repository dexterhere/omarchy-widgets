// SPIKE S2: edit-mode canvas. Pure QtQuick (no Quickshell imports) so the same file runs in
// the shell and under qmltestrunner. The host owns persistence: it connects `committed`.
import QtQuick

Item {
  id: root
  // { id: {x, y, w, h} } in surface pixels. Host sets the initial layout.
  property var layout: ({})
  property string selected: ""
  property int minSize: 40
  signal committed(string json)   // full layout.json text, emitted after every drag/resize/nudge
  signal exitRequested()          // Escape

  focus: true
  Keys.onEscapePressed: root.exitRequested()
  Keys.onPressed: function(ev) {
    if (!root.selected) return
    var step = (ev.modifiers & Qt.ShiftModifier) ? 10 : 1
    var dx = 0, dy = 0
    if (ev.key === Qt.Key_Left) dx = -step
    else if (ev.key === Qt.Key_Right) dx = step
    else if (ev.key === Qt.Key_Up) dy = -step
    else if (ev.key === Qt.Key_Down) dy = step
    else return
    root.move(root.selected, dx, dy)
    ev.accepted = true
  }

  function clamp(v, lo, hi) { return Math.max(lo, Math.min(hi, v)) }
  function rectOf(id) { return layout[id] }
  function set(id, r) {
    var next = {}
    for (var k in layout) next[k] = layout[k]
    next[id] = { x: Math.round(r.x), y: Math.round(r.y), w: Math.round(r.w), h: Math.round(r.h) }
    layout = next   // reassign: QML does not notify on in-place mutation of a var
  }
  function move(id, dx, dy) {
    var r = layout[id]
    set(id, { x: clamp(r.x + dx, 0, width - r.w), y: clamp(r.y + dy, 0, height - r.h), w: r.w, h: r.h })
    commit()
  }
  function resize(id, nw, nh) {
    var r = layout[id]
    set(id, { x: r.x, y: r.y,
              w: clamp(nw, minSize, width - r.x), h: clamp(nh, minSize, height - r.y) })
    commit()
  }
  function commit() { committed(JSON.stringify({ version: 1, widgets: layout }, null, 2)) }

  Repeater {
    model: Object.keys(root.layout)
    delegate: Rectangle {
      id: tile
      required property string modelData
      readonly property var r: root.layout[modelData]
      objectName: "tile-" + modelData
      x: r.x; y: r.y; width: r.w; height: r.h
      color: Qt.rgba(0.3, 0.5, 1, 0.45)
      border.width: root.selected === modelData ? 2 : 1
      border.color: "white"

      MouseArea {
        anchors.fill: parent
        drag.target: tile
        drag.minimumX: 0; drag.minimumY: 0
        drag.maximumX: root.width - tile.width; drag.maximumY: root.height - tile.height
        onPressed: root.selected = tile.modelData
        onReleased: root.set(tile.modelData, { x: tile.x, y: tile.y, w: tile.width, h: tile.height }), root.commit()
      }
      Rectangle {   // resize handle, bottom-right
        objectName: "handle-" + tile.modelData
        width: 16; height: 16; anchors.right: parent.right; anchors.bottom: parent.bottom
        color: "white"
        MouseArea {
          anchors.fill: parent
          property real sx; property real sy; property real sw; property real sh
          onPressed: function(m) { root.selected = tile.modelData
            var p = mapToItem(root, m.x, m.y); sx = p.x; sy = p.y; sw = tile.width; sh = tile.height }
          onPositionChanged: function(m) { if (!pressed) return
            var p = mapToItem(root, m.x, m.y)
            root.resize(tile.modelData, sw + p.x - sx, sh + p.y - sy) }
        }
      }
    }
  }
}
