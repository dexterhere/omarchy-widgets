pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Layouts

// The children of a container node.
//
// QML refuses a type that instantiates itself, and a LayoutNode -> Kids -> LayoutNode
// cycle between two files is refused as a cyclic dependency too. The supported way to
// recurse is a Loader. Its source is the ONE constant file name below: nothing a widget
// file says is ever used as a source, URL or path. The recursion is bounded by
// LayoutNode (treeFits at level 1, an error card beyond depth 8).
Repeater {
  id: kids
  property var nodes: []
  property var values: ({})
  property int level: 1
  // Only the COUNT goes through the model: objects passed through a Repeater model are converted to
  // opaque variant wrappers whose lists are no longer arrays. Each delegate reads kids.nodes[index].
  model: nodes.length
  delegate: Loader {
    id: cell
    required property int index
    source: "LayoutNode.qml"
    // the inner node is the layout child, but it sits inside this Loader: pass its sizing hints on
    // qmllint disable missing-property
    Layout.fillWidth: item !== null && item.fill === true
    Layout.maximumWidth: item !== null ? item.maxW : Number.POSITIVE_INFINITY
    Layout.minimumWidth: 0
    onLoaded: {
      item.level = kids.level
      item.node = kids.nodes[cell.index]
    }
    // qmllint enable missing-property
    Binding { target: cell.item; property: "values"; value: kids.values; when: cell.item !== null }
  }
}
