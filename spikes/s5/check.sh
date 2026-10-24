#!/bin/bash
# S5: does `omarchy plugin validate` tolerate extra manifest keys and sidecar files? Read-only, runs on a temp copy.
set -u
here=$(cd "$(dirname "$0")" && pwd); t=$(mktemp -d -p /tmp ow-spike.XXXX); cp -r "$here/folded" "$t/p"
echo "extra top-level key 'ow':"; omarchy plugin validate "$t/p"; echo "  exit=$?"
echo '{"id":"x"}' > "$t/p/widget.json"; echo '{}' > "$t/p/layout.json"
echo "plus sidecar widget.json and layout.json files:"; omarchy plugin validate "$t/p"; echo "  exit=$?"
jq '.kinds=["service","widget-canvas"] | .zzz=[1,{"a":null}]' "$t/p/manifest.json" > "$t/m" && mv "$t/m" "$t/p/manifest.json"
echo "plus unknown kind and arbitrary key:"; omarchy plugin validate "$t/p"; echo "  exit=$?"
ln -s /etc/hostname "$t/p/link"; echo "control, a symlink must fail:"; omarchy plugin validate "$t/p"; echo "  exit=$?"
mkdir "$t/d"; echo '{"schemaVersion":1,"id":"io.example.data-only","name":"Data only","version":"1","kinds":["owd-widget"],"entryPoints":{},"ow":{"tier":"spec"}}' > "$t/d/manifest.json"
echo "data-only plugin (unknown kind, empty entryPoints, no QML at all):"; omarchy plugin validate "$t/d"; echo "  exit=$?"
rm -rf "$t"
