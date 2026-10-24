#!/bin/bash
# S7: what `omarchy plugin update` does to a checkout pinned to an old commit. Pure git on a temp repo; never runs `omarchy plugin add`.
set -eu
d=$(mktemp -d -p /tmp ow-spike.XXXX); trap 'rm -rf "$d"' EXIT
git init -q -b main "$d/up"; git -C "$d/up" config user.email a@b; git -C "$d/up" config user.name t
for i in 1 2 3; do echo $i > "$d/up/f"; git -C "$d/up" add f; git -C "$d/up" commit -qm "c$i"; done
git clone -q -- "file://$d/up" "$d/c"; git -C "$d/c" checkout -q HEAD~2
echo "pinned: $(git -C "$d/c" rev-parse --short HEAD)  upstream: $(git -C "$d/up" rev-parse --short HEAD)"
# the two commands omarchy-plugin-update runs:
git -C "$d/c" fetch --quiet origin HEAD; git -C "$d/c" merge --ff-only FETCH_HEAD >/dev/null
echo "after update logic: $(git -C "$d/c" rev-parse --short HEAD)  (pin lost)"
echo "url#sha form: $(git clone -- "file://$d/up#$(git -C "$d/up" rev-parse HEAD~1)" "$d/x" 2>&1 | tail -1)"
for u in 'ext::sh -c id' '--upload-pack=x' 'file:///tmp/x' 'git@github.com:a/b.git'; do omarchy-git-url-check "$u" >/dev/null 2>&1 && r=accepted || r=refused; echo "git-url-check '$u': $r"; done
