#!/bin/sh
# Lodestone installer for macOS and Linux.
#
#   curl -fsSL https://p1e2r.github.io/lodestone-launcher/install.sh | sh
#
# Downloads the latest release for this computer and installs it for the
# current user. No admin rights needed. Your instances and worlds are kept.
set -eu

REPO="p1e2r/lodestone-launcher"
BASE="${LODESTONE_DOWNLOAD_BASE:-https://github.com/$REPO/releases/latest/download}"

say()  { printf '\033[1;32m==>\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33mWarning:\033[0m %s\n' "$*" >&2; }
die()  { printf '\033[1;31mError:\033[0m %s\n' "$*" >&2; exit 1; }

need() { command -v "$1" >/dev/null 2>&1 || die "this installer needs '$1', which isn't installed"; }
need curl
need uname

TMP=$(mktemp -d 2>/dev/null || mktemp -d -t lodestone)
MNT=""
cleanup() {
  if [ -n "$MNT" ]; then hdiutil detach "$MNT" -quiet >/dev/null 2>&1 || true; fi
  rm -rf "$TMP"
}
trap cleanup EXIT INT TERM

download() { # url dest
  curl -fL --progress-bar --retry 3 -o "$2" "$1" || die "couldn't download $1"
}

install_macos() {
  # Detect the real chip, even from a Terminal running under Rosetta.
  if [ "$(sysctl -n hw.optional.arm64 2>/dev/null || echo 0)" = "1" ]; then
    ASSET="Lodestone-macos-arm64.dmg"; CHIP="Apple Silicon"
  else
    ASSET="Lodestone-macos-intel.dmg"; CHIP="Intel"
  fi
  MAJOR=$(sw_vers -productVersion | cut -d. -f1)
  [ "$MAJOR" -ge 12 ] 2>/dev/null || die "Lodestone needs macOS 12 Monterey or newer (you have $(sw_vers -productVersion))"

  say "Downloading Lodestone for macOS ($CHIP)"
  download "$BASE/$ASSET" "$TMP/$ASSET"

  say "Opening the disk image"
  MNT="$TMP/mnt"
  mkdir -p "$MNT"
  hdiutil attach "$TMP/$ASSET" -nobrowse -readonly -noautoopen -mountpoint "$MNT" -quiet || die "couldn't open the disk image"
  [ -d "$MNT/Lodestone.app" ] || die "the disk image doesn't contain Lodestone.app"

  # /Applications if this user can write to it, otherwise ~/Applications.
  if [ -w /Applications ]; then DEST="/Applications"; else DEST="$HOME/Applications"; mkdir -p "$DEST"; fi
  APP="$DEST/Lodestone.app"

  if pgrep -x Lodestone >/dev/null 2>&1; then
    say "Closing the running Lodestone"
    osascript -e 'quit app "Lodestone"' >/dev/null 2>&1 || true
    sleep 2
  fi

  say "Installing to $APP"
  rm -rf "$APP.new"
  ditto "$MNT/Lodestone.app" "$APP.new"
  xattr -dr com.apple.quarantine "$APP.new" 2>/dev/null || true
  rm -rf "$APP"
  mv "$APP.new" "$APP"
  hdiutil detach "$MNT" -quiet >/dev/null 2>&1 || true
  MNT=""

  say "Done! Opening Lodestone"
  open "$APP"
  echo "Find it later in Applications, Launchpad or Spotlight."
}

install_linux() {
  case "$(uname -m)" in
    x86_64|amd64) ;;
    *) die "Lodestone for Linux is only built for x86_64 (this is $(uname -m))" ;;
  esac
  need tar
  ASSET="Lodestone-linux-x64.tar.gz"
  DIR="$HOME/.local/opt/lodestone"
  APPS="${XDG_DATA_HOME:-$HOME/.local/share}/applications"

  say "Downloading Lodestone for Linux (x86_64)"
  download "$BASE/$ASSET" "$TMP/$ASSET"

  say "Installing to $DIR"
  mkdir -p "$DIR" "$TMP/x"
  tar -xzf "$TMP/$ASSET" -C "$TMP/x"
  [ -f "$TMP/x/lodestone" ] || die "the download doesn't contain the lodestone program"
  install -m 755 "$TMP/x/lodestone" "$DIR/lodestone.new"
  mv -f "$DIR/lodestone.new" "$DIR/lodestone"
  [ -f "$TMP/x/lodestone.png" ] && cp -f "$TMP/x/lodestone.png" "$DIR/lodestone.png"

  say "Adding Lodestone to your app menu"
  mkdir -p "$APPS"
  cat > "$APPS/lodestone.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=Lodestone
GenericName=Minecraft Launcher
Comment=Play Minecraft with any version and mod loader
Exec="$DIR/lodestone"
Icon=$DIR/lodestone.png
Terminal=false
Categories=Game;
Keywords=minecraft;launcher;mods;
StartupWMClass=dev.p1e2r.lodestone
EOF
  command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$APPS" >/dev/null 2>&1 || true

  # Check the libraries Lodestone needs to open its window.
  MISSING=""
  if command -v ldconfig >/dev/null 2>&1; then
    LIBS=$(ldconfig -p 2>/dev/null || true)
    for lib in libxkbcommon-x11.so.0 libvulkan.so.1; do
      echo "$LIBS" | grep -q "$lib" || MISSING="$MISSING $lib"
    done
  fi
  if [ -n "$MISSING" ]; then
    warn "missing graphics libraries:$MISSING"
    if command -v apt-get >/dev/null 2>&1; then
      echo "  Install them with: sudo apt install libxkbcommon-x11-0 libvulkan1 mesa-vulkan-drivers"
    elif command -v dnf >/dev/null 2>&1; then
      echo "  Install them with: sudo dnf install libxkbcommon-x11 vulkan-loader mesa-vulkan-drivers"
    elif command -v pacman >/dev/null 2>&1; then
      echo "  Install them with: sudo pacman -S --needed libxkbcommon-x11 vulkan-icd-loader"
    fi
    echo "  Then open Lodestone from your app menu."
    return
  fi

  say "Done! Opening Lodestone"
  if [ -n "${DISPLAY:-}${WAYLAND_DISPLAY:-}" ]; then
    (nohup "$DIR/lodestone" >/dev/null 2>&1 &) || true
  fi
  echo "Find it later in your app menu, or run: $DIR/lodestone"
}

case "$(uname -s)" in
  Darwin) install_macos ;;
  Linux) install_linux ;;
  MINGW*|MSYS*|CYGWIN*)
    die "on Windows, run this in PowerShell instead:  irm https://p1e2r.github.io/lodestone-launcher/install.ps1 | iex" ;;
  *) die "unsupported system: $(uname -s). See https://p1e2r.github.io/lodestone-launcher/install/" ;;
esac
