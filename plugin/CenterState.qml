import QtQuick
import "CenterModel.js" as Model

// Everything the Widget Center does that needs no window: whether it is shown, and the rows it
// would show. Center.qml adds the layer-shell window (which cannot be created offscreen, so this
// part is what the tests load). Read-only, WIDGET-CENTER-V0 in docs/DECISIONS.md.
Item {
  id: root

  // injected by omarchy-shell (OPEN_QUESTIONS 20, 30: not verified in the real shell)
  property var shell: null
  property var manifest: null
  property string omarchyPath: ""
  property var service: null

  // `open` is the truth (an Item's own `visible` reads false when the parent is not a visible item,
  // which is the case under ShellRoot); `visible` follows it.
  property bool open: false
  visible: open
  function toggle() { open = !open }
  function hide() { open = false }

  readonly property var store: service ? service.store : null
  readonly property var bridge: service ? service.bridge : null
  readonly property string link: bridge ? bridge.link : "unavailable"
  readonly property var rows: Model.rows(store ? store.list : [], bridge ? bridge.entries : ({}), link)
}
