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

# Effective uid. A test can pretend to be another user, but only when BOTH variables below are set
# (the second is a sentinel no real environment has); otherwise they are ignored.
euid=$EUID
if [[ ${OW_TEST_HARNESS:-} == 1 && ${OW_TEST_SENTINEL:-} == install-local-test-2c1f && -n ${OW_TEST_EUID:-} ]]; then
  euid=$OW_TEST_EUID
fi
((euid != 0)) || die "refusing to run as root: this installs into your own home, run it as your normal user"

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
  # Characters that cannot be written safely into the manifest, a unit line or a terminal.
  [[ $p != *[[:cntrl:]]* && $p != *'"'* && $p != *'\'* ]] \
    || die "paths with control characters, a double quote or a backslash are not supported"
  [[ /$p/ != */../* ]] || die "paths must not contain '..': $(printf '%q' "$p")"
done

bindir=$prefix/bin
unitdir=$cfg/systemd/user
unit=$unitdir/owd.service
datadir=$dat/owd
cfgdir=$cfg/owd
mandir=$dat/omarchy-widgets
manifest=$mandir/install-manifest

for d in "$prefix" "$bindir" "$cfg" "$cfg/systemd" "$unitdir" "$dat" "$datadir" "$cfgdir" "$mandir"; do
  [[ ! -L $d ]] || die "refusing a symlinked directory: $(printf '%q' "$d")"
done
# An existing prefix or bin dir must be yours and not writable by group or others.
for d in "$prefix" "$bindir"; do
  [[ -d $d ]] || continue
  read -r owner perm < <(stat -c '%u %a' -- "$d")
  [[ $owner == "$euid" ]] || die "refusing $(printf '%q' "$d"): it is not owned by you"
  (( (8#$perm & 8#022) == 0 )) || die "refusing $(printf '%q' "$d"): it is writable by group or others (mode $perm)"
done
tmps=()
cleanup() { local t; for t in "${tmps[@]}"; do rm -f -- "$t"; done; }

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
  [[ -f $manifest && ! -L $manifest && $(stat -c %u -- "$manifest") == "$euid" ]] \
    || die "manifest is not a regular file of yours: $(printf '%q' "$manifest")"
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

declare -A new_hash=()  # path -> sha256; seeded from the old manifest so nothing is forgotten
new_dirs=()
manifest_ready=0        # set only after pre-flight: a refused run must not rewrite the manifest
write_manifest() {
  ((manifest_ready)) || return 0
  ((yes)) || return 0
  [[ -d $mandir ]] || return 0
  local tmp
  tmp=$(mktemp -- "$mandir/.manifest.XXXXXX")
  {
    echo "# omarchy-widgets install manifest; --uninstall removes only what is listed here"
    local d f
    for d in "${new_dirs[@]}"; do printf 'D\t%s\n' "$d"; done
    for f in "${!new_hash[@]}"; do printf 'F\t%s\t%s\n' "${new_hash[$f]}" "$f"; done
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
    # XDG base directories and everything we create below them are private.
    local md=755
    [[ $m == "$cfg"* || $m == "$dat"* ]] && md=700
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
    unchanged) say "  unchanged: $dest"; ((yes)) && new_hash[$dest]=$want; return 0 ;;
    replace) act "replace $dest (older copy installed by this script)" ;;
    overwrite) act "OVERWRITE $dest (--force; it was not created by this script, or was modified)" ;;
    *) act "create $dest (mode $mode)" ;;
  esac
  if ((yes)); then
    local tmp
    tmp=$(mktemp -- "$(dirname -- "$dest")/.owtmp.XXXXXX")
    tmps+=("$tmp")
    install -m "$mode" -- "$src" "$tmp"
    mv -f -- "$tmp" "$dest"
    new_hash[$dest]=$want
  fi
}

# Escape a path for a systemd unit line: %% is literal everywhere, $$ only in ExecStart= (systemd
# does not expand $ in path settings), and spaces need quotes. unit_quote PATH [exec]
unit_quote() {
  local s=$1
  s=${s//%/%%}
  [[ ${2:-} == exec ]] && s=${s//\$/\$\$}
  printf '"%s"' "$s"
}

render_unit() { # stdout: packaging/owd.service with paths rewritten to the chosen locations
  local line
  while IFS= read -r line || [[ -n $line ]]; do
    case $line in
      ExecStart=/usr/bin/owd) printf 'ExecStart=%s\n' "$(unit_quote "$bindir/owd" exec)" ;;
      ReadOnlyPaths=-%h/.local/share/owd/widgets) printf 'ReadOnlyPaths=%s\n' "$(unit_quote "-$datadir/widgets")" ;;
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
for f in "${!old_hash[@]}"; do new_hash[$f]=${old_hash[$f]}; done
trap 'cleanup; write_manifest' EXIT

unit_tmp=$(mktemp -- "${TMPDIR:-/tmp}/owd.service.XXXXXX")
tmps+=("$unit_tmp")
render_unit >"$unit_tmp"
# Check every target before changing anything, so a refusal never leaves half an install.
check_file "$from/owd" "$bindir/owd"
check_file "$from/ow" "$bindir/ow"
check_file "$unit_tmp" "$unit"
manifest_ready=1   # pre-flight passed; from here a partial run still records what it did

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
echo
echo "Next, run these yourself (this script never runs systemctl):"
echo "  systemctl --user daemon-reload"
echo "  systemctl --user enable --now owd.service"
printf '  %q doctor\n' "$bindir/ow"
[[ ":$PATH:" == *":$bindir:"* ]] || printf 'Note: %q is not in your PATH; use the full path above or add it.\n' "$bindir"
[[ $dat == "$HOME/.local/share" ]] || printf 'Note: XDG_DATA_HOME is not the default; the unit was rewritten for %q. The systemd user manager must see the same XDG_DATA_HOME, or owd will use another directory.\n' "$datadir"
