import QtQuick
import QtTest
import ".."

// Run with tests/stubs_missing: every token the adapter reads is absent, as after an
// Omarchy update that renamed them. It must degrade to fallbacks, never throw.
TestCase {
  name: "ThemeMissing"
  function test_falls_back() {
    verify(Theme.foreground !== undefined)
    compare(String(Theme.accent), "#cacccc")
    compare(String(Theme.muted), "#707880")
    verify(Theme.surface.a > 0)
    verify(Theme.border.a > 0)
    compare(Theme.family, "monospace")
    compare(Theme.radius, 0)
    compare(Theme.textPx("sm"), 12)
    compare(Theme.textPx("xl"), 24)
    compare(Theme.role("accent", "foreground"), Theme.accent)
  }
}
