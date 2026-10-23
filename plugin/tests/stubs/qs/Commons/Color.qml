pragma Singleton
import QtQuick
// Test stand-in for the Omarchy shell's qs.Commons.Color (same property names).
QtObject {
  property color foreground: "#e0e0e0"
  property color background: "#101010"
  property color accent: "#2a9d8f"
  property color urgent: "#c0392b"
  property color muted: "#808080"
  readonly property QtObject popups: QtObject {
    property color background: "#181818"
    property color border: "#2a9d8f"
  }
}
