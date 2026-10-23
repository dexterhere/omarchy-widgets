pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Layouts
import QtQuick.Shapes
import "Layout.js" as L

// Draws one node of a layout.json v0 tree. The tree is data from a stranger, so:
//  - the node only SELECTS one of the fixed drawings below by its type name; no
//    component, URL, path, expression or colour value is ever taken from it;
//  - every string reaches a Text item with textFormat PlainText, elided, after
//    Layout.plainText (control, bidi and zero-width characters removed);
//  - colours are the four theme ROLES, resolved by Theme.qml;
//  - numbers must be finite, otherwise the node shows an explicit placeholder (an em
//    dash, a dashed track), never a zero;
//  - bound values come only from `values` ({topic: {field: value}}), which the Tile
//    fills with the topics this widget declared and was granted. A bind to anything
//    else finds nothing and shows the placeholder.
Loader {
  id: root

  property var node: null
  property var values: ({})
  property int level: 1

  // what was drawn, for tests and for the tile's status line
  readonly property string nodeType: !fits ? "toolarge" : (L.typeOf(node) || "unsupported")
  readonly property bool fits: level <= L.LIMITS.depth && (level > 1 || L.treeFits(node))
  // Loader.item is typed QObject; every drawing below defines these two properties.
  // qmllint disable missing-property
  readonly property bool placeholder: item !== null && item.placeholder === true
  readonly property string shownText: item !== null && item.shown !== undefined ? item.shown : ""
  // qmllint enable missing-property

  readonly property bool textLike: nodeType === "text" || nodeType === "badge" || nodeType === "value" || nodeType === "list"
  readonly property bool stretches: nodeType === "bar" || nodeType === "sparkline" || nodeType === "divider"
    || nodeType === "spacer" || nodeType === "stack" || nodeType === "row" || nodeType === "grid"
  // Sizing hints. Kids.qml copies them onto the Loader that holds this node inside a container.
  // Text may shrink (and elide) but not grow, so it sits next to its neighbours like inline content.
  readonly property bool fill: textLike || stretches
  readonly property real maxW: textLike && item !== null ? (item as Item).implicitWidth : Number.POSITIVE_INFINITY
  Layout.fillWidth: fill
  Layout.maximumWidth: maxW
  Layout.minimumWidth: 0

  readonly property var bound: L.lookup(values, L.prop(node, "bind"))

  sourceComponent: {
    switch (nodeType) {
    case "stack": return stackC
    case "row": return rowC
    case "grid": return gridC
    case "text": return textC
    case "badge": return badgeC
    case "value": return valueC
    case "bar": return barC
    case "ring": return ringC
    case "sparkline": return sparkC
    case "list": return listC
    case "icon": return iconC
    case "spacer": return spacerC
    case "divider": return dividerC
    default: return errorC
    }
  }

  // ---- containers
  // The recursion (through Kids.qml) is bounded: treeFits() caps nodes at 200 and depth at 8 before
  // level 1 draws, and any node deeper than 8 becomes an error card.
  Component {
    id: stackC
    ColumnLayout {
      spacing: Theme.gap(L.gapOf(root.node))
      Kids { nodes: L.kids(root.node); values: root.values; level: root.level + 1 }
    }
  }
  Component {
    id: rowC
    RowLayout {
      spacing: Theme.gap(L.gapOf(root.node))
      Kids { nodes: L.kids(root.node); values: root.values; level: root.level + 1 }
    }
  }
  Component {
    id: gridC
    GridLayout {
      columns: L.columnsOf(root.node)
      columnSpacing: Theme.gap(L.gapOf(root.node))
      rowSpacing: Theme.gap(L.gapOf(root.node))
      Kids { nodes: L.kids(root.node); values: root.values; level: root.level + 1 }
    }
  }

  // ---- text
  Component {
    id: textC
    Text {
      readonly property string shown: {
        let t = L.prop(root.node, "text")
        if (typeof t === "string") { let s = L.plainText(t, L.LIMITS.string); return s.length ? s : L.DASH }
        return L.show(root.bound, "")
      }
      readonly property bool placeholder: shown === L.DASH
      text: shown
      textFormat: Text.PlainText
      elide: Text.ElideRight
      wrapMode: Text.NoWrap
      maximumLineCount: 1
      font.family: Theme.family
      font.pixelSize: Theme.textPx(L.sizeOr(root.node, "md"))
      color: Theme.role(L.roleOr(root.node, "foreground"), "foreground")
    }
  }
  Component {
    id: badgeC
    Rectangle {
      id: badge
      readonly property string shown: {
        let t = L.prop(root.node, "text")
        if (typeof t === "string") { let s = L.plainText(t, L.LIMITS.string); return s.length ? s : L.DASH }
        return L.show(root.bound, "")
      }
      readonly property bool placeholder: shown === L.DASH
      readonly property color tone: Theme.role(L.roleOr(root.node, "foreground"), "foreground")
      color: "transparent"
      border.color: tone
      border.width: 1
      radius: Theme.radius / 2
      implicitWidth: label.implicitWidth + 2 * Theme.space(6)
      implicitHeight: label.implicitHeight + 2 * Theme.space(1)
      Text {
        id: label
        anchors.fill: parent
        anchors.leftMargin: Theme.space(6)
        anchors.rightMargin: Theme.space(6)
        text: badge.shown
        textFormat: Text.PlainText
        elide: Text.ElideRight
        wrapMode: Text.NoWrap
        maximumLineCount: 1
        verticalAlignment: Text.AlignVCenter
        font.family: Theme.family
        font.pixelSize: Theme.textPx(L.sizeOr(root.node, "sm"))
        color: badge.tone
      }
    }
  }
  Component {
    id: valueC
    Text {
      readonly property string shown: L.show(root.bound, L.prop(root.node, "unit"))
      readonly property bool placeholder: shown === L.DASH
      text: shown
      textFormat: Text.PlainText
      elide: Text.ElideRight
      wrapMode: Text.NoWrap
      maximumLineCount: 1
      font.family: Theme.family
      font.bold: true
      font.pixelSize: Theme.valuePx(L.sizeOr(root.node, "xl"))
      color: Theme.role(L.roleOr(root.node, "foreground"), "foreground")
    }
  }

  // ---- bar, ring, sparkline
  Component {
    id: barC
    Item {
      id: bar
      readonly property real fraction: L.fraction(root.bound)
      readonly property bool placeholder: fraction < 0
      readonly property string shown: placeholder ? L.DASH : String(Math.round(fraction * 100))
      implicitWidth: Theme.space(60)
      implicitHeight: Theme.space(6)
      Rectangle {
        id: track
        anchors.fill: parent
        visible: !bar.placeholder
        radius: Theme.radius / 2
        color: Theme.withAlpha(Theme.foreground, 0.12)
        Rectangle {
          width: track.width * Math.max(0, bar.fraction)
          height: track.height
          radius: track.radius
          color: Theme.role(L.roleOr(root.node, "accent"), "accent")
        }
      }
      Row {   // no data: a dashed track, visibly different from an empty (0%) bar
        visible: bar.placeholder
        spacing: Theme.space(3)
        Repeater {
          model: 10
          Rectangle {
            width: Math.max(1, (bar.width - 9 * Theme.space(3)) / 10)
            height: bar.height
            color: Theme.withAlpha(Theme.muted, 0.6)
          }
        }
      }
    }
  }
  Component {
    id: ringC
    Item {
      id: ring
      readonly property real fraction: L.fraction(root.bound)
      readonly property bool placeholder: fraction < 0
      readonly property string shown: placeholder ? L.DASH : String(Math.round(fraction * 100))
      readonly property real stroke: Math.max(2, Theme.space(5))
      readonly property real radius: (Math.min(width, height) - stroke) / 2
      implicitWidth: Theme.space(44)
      implicitHeight: Theme.space(44)
      Shape {
        anchors.fill: parent
        ShapePath {
          strokeColor: Theme.withAlpha(Theme.foreground, 0.12)
          strokeWidth: ring.stroke
          fillColor: "transparent"
          PathAngleArc {
            centerX: ring.width / 2; centerY: ring.height / 2
            radiusX: ring.radius; radiusY: ring.radius
            startAngle: -90; sweepAngle: 360
          }
        }
        ShapePath {
          strokeColor: Theme.role(L.roleOr(root.node, "accent"), "accent")
          strokeWidth: ring.stroke
          fillColor: "transparent"
          capStyle: ShapePath.FlatCap
          PathAngleArc {
            centerX: ring.width / 2; centerY: ring.height / 2
            radiusX: ring.radius; radiusY: ring.radius
            startAngle: -90; sweepAngle: 359.9 * Math.max(0, ring.fraction)
          }
        }
      }
      Text {   // no data: a dash in the middle of the bare track
        anchors.centerIn: parent
        visible: ring.placeholder
        text: L.DASH
        textFormat: Text.PlainText
        font.family: Theme.family
        font.pixelSize: Theme.textPx("md")
        color: Theme.muted
      }
    }
  }
  Component {
    id: sparkC
    Item {
      id: spark
      readonly property var pts: L.sparkPoints(L.series(root.bound), width, height)
      readonly property bool placeholder: pts.length < 2
      readonly property string shown: placeholder ? L.DASH : String(pts.length)
      implicitWidth: Theme.space(80)
      implicitHeight: Theme.space(24)
      Shape {
        anchors.fill: parent
        visible: !spark.placeholder
        ShapePath {
          strokeColor: Theme.role(L.roleOr(root.node, "accent"), "accent")
          strokeWidth: 1.5
          fillColor: "transparent"
          PathPolyline { path: spark.pts.map(function (p) { return Qt.point(p.x, p.y) }) }
        }
      }
      Row {   // no data: a dashed flat line
        visible: spark.placeholder
        y: spark.height / 2
        spacing: Theme.space(3)
        Repeater {
          model: 10
          Rectangle { width: Math.max(1, (spark.width - 9 * Theme.space(3)) / 10); height: 1; color: Theme.withAlpha(Theme.muted, 0.6) }
        }
      }
    }
  }

  // ---- list
  Component {
    id: listC
    ColumnLayout {
      id: list
      readonly property var items: L.rows(root.bound, L.prop(root.node, "max"))
      readonly property bool placeholder: items.length === 0
      readonly property string shown: placeholder ? L.DASH : items.join("\n")
      spacing: Theme.space(2)
      Repeater {
        model: list.placeholder ? [L.DASH] : list.items
        Text {
          required property string modelData
          Layout.fillWidth: true
          text: modelData
          textFormat: Text.PlainText
          elide: Text.ElideRight
          wrapMode: Text.NoWrap
          maximumLineCount: 1
          font.family: Theme.family
          font.pixelSize: Theme.textPx(L.sizeOr(root.node, "sm"))
          color: Theme.role(L.roleOr(root.node, "foreground"), "foreground")
        }
      }
    }
  }

  // ---- icon, spacer, divider
  Component {
    id: iconC
    Item {
      id: icon
      readonly property real px: Theme.iconPx(L.sizeOr(root.node, "md"))
      readonly property bool placeholder: false
      readonly property string shown: String(L.prop(root.node, "name"))
      implicitWidth: px
      implicitHeight: px
      Shape {
        width: 16; height: 16
        scale: icon.px / 16
        transformOrigin: Item.TopLeft
        ShapePath {
          strokeColor: Theme.role(L.roleOr(root.node, "accent"), "accent")
          strokeWidth: 1.5
          fillColor: "transparent"
          capStyle: ShapePath.RoundCap
          joinStyle: ShapePath.RoundJoin
          PathSvg { path: L.iconPath(L.prop(root.node, "name")) }
        }
      }
    }
  }
  Component { id: spacerC; Item { readonly property bool placeholder: false; implicitWidth: 0; implicitHeight: 0 } }
  Component {
    id: dividerC
    Rectangle {
      readonly property bool placeholder: false
      implicitWidth: Theme.space(20)
      implicitHeight: 1
      Layout.topMargin: Theme.space(4)
      Layout.bottomMargin: Theme.space(4)
      color: Theme.withAlpha(Theme.foreground, 0.2)
    }
  }

  // ---- anything else: a fixed, harmless card. The node's own text is never shown.
  Component {
    id: errorC
    Rectangle {
      id: card
      readonly property bool placeholder: true
      readonly property string shown: root.nodeType === "toolarge" ? "layout too large" : "node unavailable"
      color: "transparent"
      border.color: Theme.muted
      border.width: 1
      implicitWidth: msg.implicitWidth + 2 * Theme.space(4)
      implicitHeight: msg.implicitHeight + 2 * Theme.space(2)
      Text {
        id: msg
        anchors.centerIn: parent
        width: Math.min(implicitWidth, card.width - 2 * Theme.space(4))
        text: card.shown
        textFormat: Text.PlainText
        elide: Text.ElideRight
        font.family: Theme.family
        font.pixelSize: Theme.textPx("sm")
        color: Theme.muted
      }
    }
  }
}
