#!/usr/bin/env bash
# Local installer for people who do not use pacman. Installs owd and ow, the systemd user unit
# and private data/config directories, and records what it did so --uninstall removes exactly that.
#
#   packaging/install-local.sh                  dry run: prints what it would do, changes nothing
#   packaging/install-local.sh --yes            do it
#   packaging/install-local.sh --uninstall      dry run of the removal; add --yes to remove
#
# Options: --prefix DIR (binaries go to DIR/bin; default $PREFIX or ~/.local)
#          --from DIR   (where owd and ow were built; default <repo>/target/release)
#          --force      overwrite or remove files that are not unchanged copies of ours
# Environment: HOME, XDG_CONFIG_HOME, XDG_DATA_HOME are honoured, so tests can use a fake home.
#
# This script never runs systemctl and never uses sudo. It prints the commands for you to run.
# It does not build: run `cargo build --release --locked -p owd -p ow` first.
# Limit: it refuses a symlinked prefix, bin, unit, data or config directory itself, but does not
# resolve symlinks in the parents above them.
set -euo pipefail
umask 077

here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)
repo=$(dirname "$here")
yes=0 force=0 uninstall=0
prefix=${PREFIX:-}
from=$repo/target/release

die() { echo "install-local: $*" >&2; exit 1; }

while (($#)); do
  case $1 in
    --yes) yes=1 ;;
    --force) force=1 ;;
    --uninstall) uninstall=1 ;;
    --prefix) (($# >= 2)) || die "--prefix needs a directory"; prefix=$2; shift ;;
    --from) (($# >= 2)) || die "--from needs a directory"; from=$2; shift ;;
    -h|--help) sed -n '2,17p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) die "unknown option: $1 (try --help)" ;;
  esac
  shift
done

[[ -n ${HOME:-} && $HOME == /* ]] || die "HOME must be an absolute path"
[[ -n $prefix ]] || prefix=$HOME/.local
cfg=${XDG_CONFIG_HOME:-$HOME/.config}
dat=${XDG_DATA_HOME:-$HOME/.local/share}
for p in "$prefix" "$cfg" "$dat" "$from"; do
  [[ $p == /* ]] || die "paths must be absolute: $p"
  # Characters that cannot be written safely into the manifest or a systemd unit line.
  [[ $p != *$'\n'* && $p != *$'\t'* && $p != *'"'* && $p != *'\'* ]] \
    || die "paths with a newline, tab, double quote or backslash are not supported: $p"
  [[ /$p/ != */../* ]] || die "paths must not contain '..': $p"
done

bindir=$prefix/bin
unitdir=$cfg/systemd/user
unit=$unitdir/owd.service
datadir=$dat/owd
cfgdir=$cfg/owd
mandir=$dat/omarchy-widgets
manifest=$mandir/install-manifest

for d in "$prefix" "$bindir" "$unitdir" "$datadir" "$cfgdir" "$mandir"; do
  [[ ! -L $d ]] || die "refusing a symlinked directory: $d"
done

say() { echo "$@"; }
act() { # act "text": print what is (or would be) done
  if ((yes)); then say "  $*"; else say "  would $*"; fi
}
sha() { sha256sum -- "$1" | cut -d' ' -f1; }

# ---- manifest ---------------------------------------------------------------------------------
declare -A old_hash=()   # path -> sha256 of the file we installed
old_dirs=()              # directories we created, in creation order
load_manifest() {
  [[ -e $manifest || -L $manifest ]] || return 0
  [[ -f $manifest && ! -L $manifest && -O $manifest ]] || die "manifest is not a regular file of yours: $manifest"
  local kind a b
  while IFS=$'\t' read -r kind a b; do
    case $kind in
      F) [[ $b == /* && /$b/ != */../* ]] || die "bad path in manifest"; old_hash[$b]=$a ;;
      D) [[ $a == /* && /$a/ != */../* ]] || die "bad path in manifest"; old_dirs+=("$a") ;;
      '#'*|'') ;;
      *) die "unreadable manifest line" ;;
    esac
  done <"$manifest"
}

new_files=()  # "hash<TAB>path"
new_dirs=()
write_manifest() {
  ((yes)) || return 0
  [[ -d $mandir ]] || return 0
  local tmp
  tmp=$(mktemp -- "$mandir/.manifest.XXXXXX")
  {
    echo "# omarchy-widgets install manifest; --uninstall removes only what is listed here"
    local d f
    for d in "${new_dirs[@]}"; do printf 'D\t%s\n' "$d"; done
    for f in "${new_files[@]}"; do printf 'F\t%s\n' "$f"; done
  } >"$tmp"
  mv -f -- "$tmp" "$manifest"
}

# ---- helpers ----------------------------------------------------------------------------------
# Create DIR (and missing parents, 0755) with MODE for DIR itself; record every directory created.
mk_dir() {
  local dir=$1 mode=$2 missing=() p=$1
  while [[ ! -d $p ]]; do
    [[ ! -e $p && ! -L $p ]] || die "not a directory: $p"
    missing=("$p" "${missing[@]}")
    p=$(dirname -- "$p")
  done
  if ((${#missing[@]} == 0)); then
    local have
    have=$(stat -c '%a' -- "$dir")
    if [[ $mode == 700 && $have != 700 ]]; then
      say "  WARNING: $dir exists with mode $have, not 700. Not changed. Fix: chmod 700 '$dir'"
    fi
    return 0
  fi
  local m
  for m in "${missing[@]}"; do
    local md=755
    [[ $m == "$dir" ]] && md=$mode
    act "create directory $m (mode $md)"
    if ((yes)); then
      install -d -m "$md" -- "$m"
      new_dirs+=("$m")
    fi
  done
}

# Decide, without changing anything, whether DEST may be written: dies if it is not ours.
# Prints nothing; sets $verdict to unchanged | create | replace | overwrite.
check_file() {
  local src=$1 dest=$2 want have
  want=$(sha "$src")
  if [[ -L $dest ]]; then die "refusing to replace a symlink: $dest"; fi
  verdict=create
  [[ -e $dest ]] || return 0
  [[ -f $dest ]] || die "not a regular file: $dest"
  have=$(sha "$dest")
  if [[ $have == "$want" ]]; then verdict=unchanged
  elif [[ ${old_hash[$dest]:-} == "$have" ]]; then verdict=replace
  elif ((force)); then verdict=overwrite
  else die "refusing to overwrite $dest: it was not created by this script or has been modified (use --force)"
  fi
}

# Install SRC as DEST with MODE (call check_file for every target first).
put_file() {
  local src=$1 dest=$2 mode=$3 want
  want=$(sha "$src")
  check_file "$src" "$dest"
  case $verdict in
    unchanged) say "  unchanged: $dest"; ((yes)) && new_files+=("$want"$'\t'"$dest"); return 0 ;;
    replace) act "replace $dest (older copy installed by this script)" ;;
    overwrite) act "OVERWRITE $dest (--force; it was not created by this script, or was modified)" ;;
    *) act "create $dest (mode $mode)" ;;
  esac
  if ((yes)); then
    local tmp
    tmp=$(mktemp -- "$(dirname -- "$dest")/.owtmp.XXXXXX")
    install -m "$mode" -- "$src" "$tmp"
    mv -f -- "$tmp" "$dest"
    new_files+=("$want"$'\t'"$dest")
  fi
}

# Escape a path for a systemd unit line: %% and $$ are literal, spaces need quotes.
unit_quote() {
  local s=$1
  s=${s//%/%%}
  s=${s//\$/\$\$}
  printf '"%s"' "$s"
}

render_unit() { # stdout: packaging/owd.service with paths rewritten to the chosen locations
  local line
  while IFS= read -r line || [[ -n $line ]]; do
    case $line in
      ExecStart=/usr/bin/owd) printf 'ExecStart=%s\n' "$(unit_quote "$bindir/owd")" ;;
      ReadWritePaths=%h/.local/share/owd) printf 'ReadWritePaths=%s\n' "$(unit_quote "$datadir")" ;;
      *) printf '%s\n' "$line" ;;
    esac
  done <"$here/owd.service"
}

# ---- uninstall --------------------------------------------------------------------------------
if ((uninstall)); then
  [[ -f $manifest ]] || die "no manifest at $manifest: nothing to uninstall (this script did not install here)"
  load_manifest
  say "Before removing, stop the service yourself (this script never runs systemctl):"
  say "  systemctl --user disable --now owd.service"
  say
  for f in "${!old_hash[@]}"; do
    if [[ -L $f ]]; then say "  keep $f (now a symlink)"; continue; fi
    if [[ ! -e $f ]]; then say "  already gone: $f"; continue; fi
    if [[ $(sha "$f") != "${old_hash[$f]}" ]] && ((!force)); then
      say "  keep $f (modified since install; use --force to remove it)"; continue
    fi
    act "remove $f"
    ((yes)) && rm -f -- "$f"
  done
  act "remove $manifest"
  ((yes)) && rm -f -- "$manifest"
  for ((i = ${#old_dirs[@]} - 1; i >= 0; i--)); do
    d=${old_dirs[i]}
    [[ -d $d && ! -L $d ]] || continue
    if [[ -n $(ls -A -- "$d") ]]; then say "  keep $d (not empty; your data stays)"; continue; fi
    act "remove empty directory $d"
    ((yes)) && rmdir -- "$d"
  done
  ((yes)) || say $'\nDry run: nothing was changed. Add --yes to apply.'
  exit 0
fi

# ---- install ----------------------------------------------------------------------------------
for b in owd ow; do
  [[ -f $from/$b && -x $from/$b ]] || die "$from/$b not found. Build first: cargo build --release --locked -p owd -p ow"
done
load_manifest
new_dirs=("${old_dirs[@]}")
trap write_manifest EXIT

unit_tmp=$(mktemp -- "${TMPDIR:-/tmp}/owd.service.XXXXXX")
trap 'rm -f -- "$unit_tmp"; write_manifest' EXIT
render_unit >"$unit_tmp"
# Check every target before changing anything, so a refusal never leaves half an install.
check_file "$from/owd" "$bindir/owd"
check_file "$from/ow" "$bindir/ow"
check_file "$unit_tmp" "$unit"

say "Install plan (prefix $prefix):"
mk_dir "$bindir" 755
mk_dir "$unitdir" 755
mk_dir "$datadir" 700
mk_dir "$cfgdir" 700
mk_dir "$mandir" 700
put_file "$from/owd" "$bindir/owd" 755
put_file "$from/ow" "$bindir/ow" 755
put_file "$unit_tmp" "$unit" 644
act "write manifest $manifest"

if ((yes)); then say $'\nDone.'; else say $'\nDry run: nothing was changed. Add --yes to apply.'; fi
cat <<MSG

Next, run these yourself (this script never runs systemctl):
  systemctl --user daemon-reload
  systemctl --user enable --now owd.service
  $bindir/ow doctor
$([[ ":$PATH:" == *":$bindir:"* ]] || echo "Note: $bindir is not in your PATH; use the full path above or add it.")
$([[ $dat == "$HOME/.local/share" ]] || echo "Note: XDG_DATA_HOME is not the default; the unit was rewritten for $datadir. The systemd user manager must see the same XDG_DATA_HOME, or owd will use another directory.")
MSG
