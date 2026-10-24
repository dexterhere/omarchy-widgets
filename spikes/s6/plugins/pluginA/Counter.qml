pragma Singleton
import QtQuick
// Stand-in for a shared bridge singleton owned by plugin A.
QtObject {
  readonly property real stamp: Math.random()   // fixed at construction: identifies the instance
  property int n: 0
}
