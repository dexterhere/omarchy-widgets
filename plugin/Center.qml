import QtQuick

// Placeholder for the overlay kind named in the manifest (SPIKE-S6: one plugin, service plus
// overlay). The Widget Center and edit mode are a later task; this item draws nothing and takes
// no input. `service` is injected by the shell for plugins that pair an overlay with a service.
Item {
  property var shell: null
  property var manifest: null
  property string omarchyPath: ""
  property var service: null
  visible: false
}
