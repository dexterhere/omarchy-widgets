import QtQuick
import "../../plugins/pluginA"   // same directory, spelled via a different relative route
QtObject {
  Component.onCompleted: { console.log("S6 B2(../../plugins/pluginA) stamp=" + Counter.stamp + " n=" + Counter.n) }
}
