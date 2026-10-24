// SPIKE S3 client: Quickshell.Io Socket -> newline-delimited JSON, reconnect, survives hot reload.
// Socket path comes from $OW_SOCK (the real bridge would use $XDG_RUNTIME_DIR/owd/owd.sock).
import QtQuick
import Quickshell
import Quickshell.Io

ShellRoot {
  id: root
  property int ticks: 0

  Socket {
    id: sock
    path: Quickshell.env("OW_SOCK")
    connected: true                       // asking to connect
    onConnectedChanged: {
      console.log("S3 connected=" + connected)
      if (connected) {
        write(JSON.stringify({ v: 0, id: 1, op: "hello", client: "qml-spike", clientVersion: "0" }) + "\n")
        write(JSON.stringify({ v: 0, id: 2, op: "subscribe", widget: "spike", topic: "spike.tick" }) + "\n")
        flush()
      }
    }
    onError: function(e) { console.log("S3 socket error " + e) }
    parser: SplitParser {                 // one JSON object per line
      splitMarker: "\n"
      onRead: function(line) {
        var m = JSON.parse(line)
        if (m.topic) { root.ticks++; console.log("S3 push n=" + m.data.n + " total=" + root.ticks) }
        else console.log("S3 reply " + line)
      }
    }
  }

  // Reconnect: Socket does not retry by itself. Toggle `connected` to re-attempt.
  Timer {
    interval: 1000; running: true; repeat: true
    onTriggered: if (!sock.connected) { sock.connected = false; sock.connected = true }
  }
  Component.onCompleted: console.log("S3 started, path=" + sock.path)
}
