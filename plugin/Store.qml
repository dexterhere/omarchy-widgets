pragma ComponentBehavior: Bound
import QtQuick
import Qt.labs.folderlistmodel
import Quickshell
import Quickshell.Io
import "Layout.js" as L

// Discovers installed spec widgets under $XDG_DATA_HOME/owd/widgets/<id>/ and keeps one
// validated record per directory (docs/INSTALL_DESIGN.md section 5). Read-only: this file
// never writes there. `ow install` is the writer.
//
// Nothing is trusted because ow wrote it. A directory name must pass the id pattern before any
// path is built from it; each of the four files must exist, be non-empty and within a size cap
// (checked from the directory listing BEFORE the file is read); contents go through
// Layout.loadWidget (strict JSON, depth guard, the registry's limits). A failure produces a
// record { id, ok: false, reason } with a fixed reason code and nothing from the files.
Item {
  id: store

  readonly property string dataHome: Quickshell.env("XDG_DATA_HOME") || (Quickshell.env("HOME") + "/.local/share")
  property string widgetsDir: dataHome + "/owd/widgets"

  property var records: ({})                 // id -> record
  readonly property var list: Object.keys(records).sort().map(function (id) { return records[id] })
  // [{widget, topic}] for the bridge: valid widgets, declared and recorded-as-granted topics only
  readonly property var wanted: {
    const out = []
    for (const r of list) if (r.ok) for (const t of r.topics) out.push({ widget: r.id, topic: t })
    return out
  }
  property int generation: 0                 // bump to force every directory to be re-read
  property int skippedDirs: 0                // directory names that were not valid ids

  function rescan() { generation++ }

  function report(id, record) {
    const copy = Object.assign({}, records)
    copy[id] = record
    records = copy
  }
  function forget(id) {
    if (records[id] === undefined) return
    const copy = Object.assign({}, records)
    delete copy[id]
    records = copy
  }

  FolderListModel {
    id: top
    folder: "file://" + encodeURI(store.widgetsDir)
    showFiles: false
    showDirs: true
    showHidden: false                        // .stage-* and .old-* from an install in progress
    showDotAndDotDot: false
    sortField: FolderListModel.Name
  }

  Repeater {
    model: top
    delegate: Item {
      id: src
      required property string fileName
      readonly property bool validName: L.validId(fileName)
      readonly property string dir: store.widgetsDir + "/" + fileName
      readonly property var names: ["manifest", "widget", "layout", "installed"]

      Component.onCompleted: { if (!validName) store.skippedDirs++ }
      Component.onDestruction: { if (validName) store.forget(fileName) }

      FolderListModel {
        id: files
        folder: src.validName ? "file://" + encodeURI(src.dir) : ""
        showDirs: false
        showHidden: false
        nameFilters: ["manifest.json", "widget.json", "layout.json", "installed.json"]
        onCountChanged: debounce.restart()
        onStatusChanged: debounce.restart()
      }
      Connections { target: store; function onGenerationChanged() { src.reloadAll() } }
      Timer { id: debounce; interval: 120; onTriggered: src.evaluate() }

      // one FileView per file; path stays empty until the listing says the size is acceptable
      FileView { id: fvManifest; path: src.pathFor("manifest.json"); blockLoading: true; printErrors: false; watchChanges: true; onFileChanged: { reload(); debounce.restart() }
        onLoaded: debounce.restart() }
      FileView { id: fvWidget; path: src.pathFor("widget.json"); blockLoading: true; printErrors: false; watchChanges: true; onFileChanged: { reload(); debounce.restart() }
        onLoaded: debounce.restart() }
      FileView { id: fvLayout; path: src.pathFor("layout.json"); blockLoading: true; printErrors: false; watchChanges: true; onFileChanged: { reload(); debounce.restart() }
        onLoaded: debounce.restart() }
      FileView { id: fvInstalled; path: src.pathFor("installed.json"); blockLoading: true; printErrors: false; watchChanges: true; onFileChanged: { reload(); debounce.restart() }
        onLoaded: debounce.restart() }

      // size of a listed file, or -1 if it is not there
      function sizeOf(name) {
        for (let i = 0; i < files.count; i++) if (files.get(i, "fileName") === name) return files.get(i, "fileSize")
        return -1
      }
      function limitFor(name) { return name === "layout.json" ? L.LIMITS.bytes : L.LIMITS.fileBytes }
      function sizeOk(name) { const s = sizeOf(name); return s > 0 && s <= limitFor(name) }
      function pathFor(name) { return validName && files.count >= 0 && sizeOk(name) ? dir + "/" + name : "" }

      function reloadAll() {
        for (const v of [fvManifest, fvWidget, fvLayout, fvInstalled]) if (v.path !== "") v.reload()
        debounce.restart()
      }

      function evaluate() {
        if (!validName) return
        const views = { manifest: fvManifest, widget: fvWidget, layout: fvLayout, installed: fvInstalled }
        const texts = {}
        for (const n of names) {
          const file = n + ".json"
          if (!sizeOk(file)) { texts[n] = null; continue }       // missing, empty or too large: never read
          texts[n] = views[n].text()
        }
        store.report(fileName, L.loadWidget(fileName, texts))
      }
    }
  }
}
