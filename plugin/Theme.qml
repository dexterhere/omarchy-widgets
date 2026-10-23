pragma Singleton
import QtQuick
import qs.Commons as Commons

// The ONLY file in this plugin that imports qs.Commons or contains a colour literal.
// Spec widgets name semantic roles (accent, foreground, secondary, muted); this file maps
// them to Omarchy theme tokens (SPIKE-S8). Imported with a qualifier because QtQuick 6.12 has
// its own singleton named Color that otherwise shadows the shell's. qs.Commons is an internal of the Omarchy shell
// with no stability promise, so every read goes through pick(): a renamed or missing token
// degrades to the fallback below instead of throwing, and a rename is a one-file fix.
QtObject {
  id: root

  function pick(v, fallback) { return v === undefined || v === null ? fallback : v }
  function withAlpha(c, a) { return Qt.rgba(c.r, c.g, c.b, a) }
  function mix(a, b, t) { return Qt.rgba(a.r + (b.r - a.r) * t, a.g + (b.g - a.g) * t, a.b + (b.b - a.b) * t, 1) }

  readonly property color foreground: pick(Commons.Color.foreground, "#cacccc")
  readonly property color background: pick(Commons.Color.background, "#101315")
  readonly property color accent: pick(Commons.Color.accent, "#cacccc")
  readonly property color muted: pick(Commons.Color.muted, "#707880")
  readonly property color urgent: pick(Commons.Color.urgent, "#a55555")
  readonly property color secondary: mix(foreground, background, 0.35)
  readonly property color surface: Commons.Color.popups ? pick(Commons.Color.popups.background, withAlpha(background, 0.85)) : withAlpha(background, 0.85)
  readonly property color border: Commons.Color.popups ? pick(Commons.Color.popups.border, withAlpha(foreground, 0.2)) : withAlpha(foreground, 0.2)
  readonly property string family: Commons.Style.font ? pick(Commons.Style.font.family, "monospace") : "monospace"
  readonly property int radius: pick(Commons.Style.cornerRadius, 0)

  // Role name -> colour. Anything that is not one of the four roles gets foreground:
  // a widget can never choose a colour value, only a role.
  function role(name, fallbackRole) {
    var r = (name === "accent" || name === "foreground" || name === "secondary" || name === "muted")
      ? name : fallbackRole
    return r === "accent" ? accent : (r === "secondary" ? secondary : (r === "muted" ? muted : foreground))
  }

  function px(mult, fallbackBase) {
    return Commons.Style.fontPx ? Commons.Style.fontPx(mult) : Math.round(fallbackBase * mult)
  }
  function space(n) { return Commons.Style.space ? Commons.Style.space(n) : n }

  // Sizes as multiples of the theme's base font size, so text-size overrides apply.
  readonly property var textMult: ({ sm: 1.0, md: 1.17, lg: 1.5, xl: 2.0 })
  readonly property var valueMult: ({ sm: 1.0, md: 1.33, lg: 1.83, xl: 2.5 })
  function textPx(size) { return px(textMult[size] !== undefined ? textMult[size] : textMult.md, 12) }
  function valuePx(size) { return px(valueMult[size] !== undefined ? valueMult[size] : valueMult.lg, 12) }
  function iconPx(size) { return space({ sm: 14, md: 20, lg: 26, xl: 32 }[size] || 20) }
  function gap(name) { return space({ sm: 4, md: 8, lg: 14 }[name] || 4) }
}
