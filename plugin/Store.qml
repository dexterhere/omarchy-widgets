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

  // a relative XDG path is ignored, as the XDG spec says
  readonly property string dataHome: Quickshell.env("XDG_DATA_HOME").charAt(0) === "/" ? Quickshell.env("XDG_DATA_HOME") : (Quickshell.env("HOME") + "/.local/share")
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
  // Each loaded directory costs memory (about 0.35 MiB measured: four FileViews and a listing), so only the
  // first maxDirs directories (by name) are loaded. The rest are counted here and never opened.
  readonly property int maxDirs: 200
  readonly property int overflowDirs: Math.max(0, top.count - maxDirs)

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
      required property int index
      readonly property bool validName: index < store.maxDirs && L.validId(fileName)
      readonly property string dir: store.widgetsDir + "/" + fileName
      readonly property var names: ["manifest", "widget", "layout", "installed"]
      // Files are only opened while `fresh`: the size in the listing was read AFTER the last change we
      // saw. A change (or a rescan) clears it, which unloads the FileViews (empty path), and it comes
      // back only once the directory has been listed again. FileView itself has no size limit.
      // Residual: a file that grows in the milliseconds between the listing and the read is read.
      property bool fresh: false

      Component.onDestruction: { if (validName) store.forget(fileName) }

      FolderListModel {
        id: files
        folder: src.validName ? "file://" + encodeURI(src.dir) : ""
        showDirs: false
        showHidden: false
        nameFilters: ["manifest.json", "widget.json", "layout.json", "installed.json"]
        onCountChanged: debounce.restart()
        onStatusChanged: { if (status === FolderListModel.Ready && src.relisting && files.folder.toString() !== "") { src.relisting = false; src.fresh = true } debounce.restart() }
      }
      property bool relisting: false
      property bool lastOk: false
      // a widget that failed is looked at again every 2 s: files we refused to open have no watcher, so
      // this is how a shrunk or completed file is noticed
      Timer { interval: 2000; repeat: true; running: src.validName && src.fresh && !src.lastOk; onTriggered: src.invalidate() }
      // a file the listing shows changing (also files we refused to open, which have no watcher of their own)
      Connections { target: files; function onDataChanged() { if (src.fresh) src.invalidate() } }
      // forget what we knew about the files and list the directory again
      function invalidate() {
        fresh = false
        relisting = true
        files.folder = ""
        relist.restart()
      }
      Timer { id: relist; interval: 20; onTriggered: files.folder = src.validName ? "file://" + encodeURI(src.dir) : "" }
      Component.onCompleted: { if (!validName) { if (index < store.maxDirs) store.skippedDirs++ } else invalidate() }
      Connections { target: store; function onGenerationChanged() { src.invalidate() } }
      Timer { id: debounce; interval: 120; onTriggered: src.evaluate() }

      // one FileView per file; path stays empty until the listing says the size is acceptable
      FileView { id: fvManifest; path: src.pathFor("manifest.json"); blockLoading: true; printErrors: false; watchChanges: true; onFileChanged: src.invalidate()
        onLoaded: debounce.restart() }
      FileView { id: fvWidget; path: src.pathFor("widget.json"); blockLoading: true; printErrors: false; watchChanges: true; onFileChanged: src.invalidate()
        onLoaded: debounce.restart() }
      FileView { id: fvLayout; path: src.pathFor("layout.json"); blockLoading: true; printErrors: false; watchChanges: true; onFileChanged: src.invalidate()
        onLoaded: debounce.restart() }
      FileView { id: fvInstalled; path: src.pathFor("installed.json"); blockLoading: true; printErrors: false; watchChanges: true; onFileChanged: src.invalidate()
        onLoaded: debounce.restart() }

      // size of a listed file, or -1 if it is not there
      function sizeOf(name) {
        for (let i = 0; i < files.count; i++) if (files.get(i, "fileName") === name) return files.get(i, "fileSize")
        return -1
      }
      function limitFor(name) { return name === "layout.json" ? L.LIMITS.bytes : L.LIMITS.fileBytes }
      function sizeOk(name) { const s = sizeOf(name); return s > 0 && s <= limitFor(name) }
      function pathFor(name) { return validName && fresh && files.count >= 0 && sizeOk(name) ? dir + "/" + name : "" }

      function evaluate() {
        if (!validName || !fresh) return
        const views = { manifest: fvManifest, widget: fvWidget, layout: fvLayout, installed: fvInstalled }
        const texts = {}
        for (const n of names) {
          const file = n + ".json"
          if (!sizeOk(file)) { texts[n] = null; continue }       // missing, empty or too large: never read
          texts[n] = views[n].text()
        }
        const rec = L.loadWidget(fileName, texts)
        lastOk = rec.ok === true
        store.report(fileName, rec)
      }
    }
  }
}
