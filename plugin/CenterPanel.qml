pragma ComponentBehavior: Bound
import QtQuick
import "CenterModel.js" as Model

// The Widget Center v0 contents: a READ-ONLY list of installed widgets, what each may read, how
// each is doing, and the command to remove it as text. It runs nothing, writes nothing and sends
// nothing (WIDGET-CENTER-V0 in docs/DECISIONS.md): removing or installing needs consent typed in a
// terminal. `rows` come from CenterModel.rows, so every string is already plain text.
// Escape emits closeRequested; the hosting window decides what closing means.
Item {
  id: panel

  property var rows: []
  property string link: "connecting"
  signal closeRequested()

  readonly property alias list: list
  focus: true
  Keys.onEscapePressed: panel.closeRequested()

  implicitWidth: 720
  implicitHeight: 560

  Rectangle {
    anchors.fill: parent
    color: Theme.background
    border.color: Theme.border
    border.width: 1
    radius: Theme.radius
  }

  Column {
    anchors.fill: parent
    anchors.margins: Theme.space(16)
    spacing: Theme.space(8)

    Item {
      width: parent.width
      height: titleText.implicitHeight
      Text {
        id: titleText
        text: "Widget Center"
        textFormat: Text.PlainText
        font.family: Theme.family
        font.pixelSize: Theme.textPx("lg")
        color: Theme.foreground
      }
      Text {
        objectName: "linkText"
        anchors.centerIn: parent
        text: panel.linkLabel
        textFormat: Text.PlainText
        font.family: Theme.family
        font.pixelSize: Theme.textPx("sm")
        color: Theme.muted
      }
      Text {
        objectName: "closeButton"
        anchors.right: parent.right
        text: "Close (Esc)"
        textFormat: Text.PlainText
        font.family: Theme.family
        font.pixelSize: Theme.textPx("sm")
        color: Theme.accent
        MouseArea { anchors.fill: parent; onClicked: panel.closeRequested() }
      }
    }

    Text {
      width: parent.width
      text: "This panel only shows information. It cannot install, remove or change anything: run the command shown in a terminal."
      textFormat: Text.PlainText
      wrapMode: Text.WordWrap
      maximumLineCount: 2
      elide: Text.ElideRight
      font.family: Theme.family
      font.pixelSize: Theme.textPx("sm")
      color: Theme.secondary
    }

    Text {
      objectName: "emptyText"
      visible: panel.rows.length === 0
      width: parent.width
      text: "No widgets installed."
      textFormat: Text.PlainText
      font.family: Theme.family
      font.pixelSize: Theme.textPx("md")
      color: Theme.muted
    }

    ListView {
      id: list
      width: parent.width
      height: parent.height - y
      clip: true
      spacing: Theme.space(10)
      model: panel.rows
      delegate: Column {
        id: card
        required property var modelData
        width: list.width
        spacing: Theme.space(2)
        Text {
          objectName: "title"
          width: parent.width
          text: card.modelData.id + (card.modelData.name.length ? "  " + card.modelData.name : "")
          textFormat: Text.PlainText
          elide: Text.ElideRight
          font.family: Theme.family
          font.pixelSize: Theme.textPx("md")
          color: Theme.foreground
        }
        Text {
          objectName: "meta"
          width: parent.width
          text: card.modelData.ok ? "version " + (card.modelData.version.length ? card.modelData.version : "unknown")
                                    + "   status " + card.modelData.status + "   tier " + card.modelData.tier
                                    + "   health: " + card.modelData.summary
                                  : "health: " + card.modelData.summary
          textFormat: Text.PlainText
          elide: Text.ElideRight
          font.family: Theme.family
          font.pixelSize: Theme.textPx("sm")
          color: Theme.secondary
        }
        Text {
          objectName: "grants"
          width: parent.width
          text: card.modelData.grants.join("\n")
          textFormat: Text.PlainText
          elide: Text.ElideRight
          maximumLineCount: 8
          wrapMode: Text.WordWrap
          font.family: Theme.family
          font.pixelSize: Theme.textPx("sm")
          color: Theme.muted
        }
        Text {
          objectName: "topicHealth"
          visible: card.modelData.health.length > 0
          width: parent.width
          text: card.modelData.health.join("\n")
          textFormat: Text.PlainText
          elide: Text.ElideRight
          maximumLineCount: 8
          wrapMode: Text.WordWrap
          font.family: Theme.family
          font.pixelSize: Theme.textPx("sm")
          color: Theme.muted
        }
        Text {
          objectName: "command"
          width: parent.width
          text: card.modelData.command
          textFormat: Text.PlainText
          elide: Text.ElideRight
          font.family: Theme.family
          font.pixelSize: Theme.textPx("sm")
          color: Theme.accent
        }
      }
    }
  }

  readonly property string linkLabel: Model.linkLabel(link)
}
