// SPIKE S6: cross-plugin singleton import + loading QML from outside the plugin dir.
import QtQuick
import Quickshell
import "plugins/pluginA"
import "plugins/pluginB"

ShellRoot {
  B { }
  B2 { }
  Component.onCompleted: {
    Counter.n += 1
    console.log("S6 host(plugins/pluginA) stamp=" + Counter.stamp + " n=" + Counter.n)
    var url = Qt.resolvedUrl("data/widgets/x/Widget.qml")
    var c = Qt.createComponent(url)
    console.log("S6 createComponent status=" + c.status + " (1=Ready) url=" + url)
    if (c.status === Component.Ready) c.createObject(null)
    // Absolute file URL outside the config root and outside any plugin dir (what $XDG_DATA_HOME/owd/widgets/<id>/ would be).
    var ext = Quickshell.env("OW_EXT")
    if (ext) {
      var c2 = Qt.createComponent("file://" + ext + "/Widget.qml")
      console.log("S6 external createComponent status=" + c2.status + " " + c2.errorString())
      if (c2.status === Component.Ready) c2.createObject(null)
    }
    Qt.callLater(Qt.quit)
  }
}
