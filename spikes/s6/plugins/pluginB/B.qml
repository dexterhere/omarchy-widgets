import QtQuick
import "../pluginA"          // plain relative directory import of ANOTHER plugin's folder
QtObject {
  Component.onCompleted: { Counter.n += 10; console.log("S6 B(relative ../pluginA) stamp=" + Counter.stamp + " n=" + Counter.n) }
}
