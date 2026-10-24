pragma ComponentBehavior: Bound
import QtQuick
import "Layout.js" as L

// One widget on the desktop: a surface, the rendered layout, and a one-word status in the
// corner when the data is not fully live. It only ever reads `record` (validated by
// Layout.loadWidget), and `entries`/`bridgeState` from the bridge.
Item {
  id: tile

  property var record: null          // loadWidget result, ok or not
  property var entries: ({})         // bridge.entries
  property string bridgeState: "connecting"
  property string variant: ""        // requested variant name from placement state

  readonly property bool unavailable: record === null || record.ok !== true
  readonly property var view: L.tileState(record, entries, bridgeState)
  readonly property string statusLabel: view.label
  readonly property string sizeClass: unavailable ? "" : L.pickSizeClass(record.sizeClasses, width, height)
  readonly property var rootNode: unavailable ? null : L.pickNode(record.layout, record.variants, variant, sizeClass)
  readonly property alias content: content
  readonly property real pad: Theme.space(12)

  implicitWidth: 200
  implicitHeight: 120
  clip: true

  Rectangle {
    anchors.fill: parent
    color: Theme.surface
    border.color: Theme.border
    border.width: 1
    radius: Theme.radius
  }

  LayoutNode {
    id: content
    visible: !tile.unavailable
    x: tile.pad
    width: tile.width - 2 * tile.pad
    y: Math.max(tile.pad, (tile.height - implicitHeight) / 2)
    opacity: tile.view.dim ? 0.5 : 1
    node: tile.rootNode
    values: tile.view.values
  }

  // A widget that failed validation: the id (charset-checked, so inert) and a fixed sentence.
  // Nothing from the widget's files is shown.
  Column {
    visible: tile.unavailable
    anchors.centerIn: parent
    width: parent.width - 2 * tile.pad
    spacing: Theme.space(4)
    Text {
      width: parent.width
      horizontalAlignment: Text.AlignHCenter
      text: "Widget unavailable"
      textFormat: Text.PlainText
      elide: Text.ElideRight
      font.family: Theme.family
      font.pixelSize: Theme.textPx("md")
      color: Theme.foreground
    }
    Text {
      width: parent.width
      horizontalAlignment: Text.AlignHCenter
      text: tile.record !== null && L.validId(tile.record.id) ? tile.record.id : ""
      textFormat: Text.PlainText
      elide: Text.ElideMiddle
      font.family: Theme.family
      font.pixelSize: Theme.textPx("sm")
      color: Theme.muted
    }
  }

  Text {
    visible: !tile.unavailable && tile.statusLabel.length > 0
    anchors.right: parent.right
    anchors.bottom: parent.bottom
    anchors.margins: Theme.space(4)
    width: Math.min(implicitWidth, parent.width - 2 * Theme.space(4))
    text: tile.statusLabel
    textFormat: Text.PlainText
    elide: Text.ElideLeft
    font.family: Theme.family
    font.pixelSize: Theme.textPx("sm") - 1
    color: Theme.muted
  }
}
