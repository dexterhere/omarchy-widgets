pragma Singleton
import QtQuick
// Test stand-in for qs.Commons.Style.
QtObject {
  property int cornerRadius: 6
  property int fontBaseSize: 12
  function fontPx(mult) { return Math.max(1, Math.round(fontBaseSize * mult)) }
  function space(px) { return px }
  readonly property QtObject font: QtObject { readonly property string family: "monospace" }
}
