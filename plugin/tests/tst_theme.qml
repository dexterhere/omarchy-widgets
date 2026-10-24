import QtQuick
import QtTest
import ".."

TestCase {
  name: "Theme"
  function test_roles_resolve_to_tokens() {
    compare(Theme.role("accent", "foreground"), Theme.accent)
    compare(Theme.role("foreground", "accent"), Theme.foreground)
    compare(Theme.role("muted", "foreground"), Theme.muted)
    compare(Theme.role("secondary", "foreground"), Theme.secondary)
    verify(Theme.secondary !== Theme.foreground && Theme.secondary !== Theme.background)
  }
  function test_only_roles_become_colours() {
    // anything that is not one of the four roles falls back to the node default
    compare(Theme.role("#ff0000", "foreground"), Theme.foreground)
    compare(Theme.role("red", "accent"), Theme.accent)
    compare(Theme.role("urgent", "muted"), Theme.muted)
    compare(Theme.role(undefined, "accent"), Theme.accent)
    compare(Theme.role({ toString: function () { return "accent" } }, "muted"), Theme.muted)
  }
  function test_sizes_scale_with_base_font() {
    compare(Theme.textPx("sm"), 12)
    verify(Theme.textPx("xl") > Theme.textPx("lg") && Theme.textPx("lg") > Theme.textPx("md") && Theme.textPx("md") >= Theme.textPx("sm"))
    verify(Theme.valuePx("xl") > Theme.valuePx("sm"))
    compare(Theme.textPx("bogus"), Theme.textPx("md"))
    compare(Theme.iconPx("md"), 20)
    compare(Theme.gap("lg"), 14)
    compare(Theme.gap("nonsense"), 4)
  }
  function test_tokens_come_from_commons() {
    compare(String(Theme.accent), "#2a9d8f")        // the stub's accent
    compare(Theme.radius, 6)
    compare(Theme.family, "monospace")
  }
}
