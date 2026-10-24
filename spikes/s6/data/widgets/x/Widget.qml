import QtQuick
// Lives OUTSIDE the plugin dirs (stand-in for $XDG_DATA_HOME/owd/widgets/<id>/).
QtObject { Component.onCompleted: console.log("S6 data-dir widget loaded via Qt.createComponent(file url)") }
