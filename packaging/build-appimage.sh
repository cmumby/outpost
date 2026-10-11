#!/usr/bin/env bash
# Builds Outpost as an AppImage, bundling GTK 4 and libadwaita so it runs on
# distros that don't ship them. Output: target/appimage/Outpost-<arch>.AppImage
#
# The AppImage only runs on distros at least as new as the one it's built on,
# so release builds happen on Ubuntu 24.04 (see .github/workflows/release.yml).
set -euo pipefail
cd "$(dirname "$0")/.."

arch=$(uname -m)
out=target/appimage
tools=$out/tools
appdir=$out/AppDir
mkdir -p "$tools"

# The packaging tools, downloaded once.
fetch() {
    if [ ! -x "$tools/$1" ]; then
        curl -fsSL -o "$tools/$1" "$2"
        chmod +x "$tools/$1"
    fi
}
fetch linuxdeploy "https://github.com/linuxdeploy/linuxdeploy/releases/download/continuous/linuxdeploy-$arch.AppImage"
fetch linuxdeploy-plugin-gtk.sh "https://raw.githubusercontent.com/linuxdeploy/linuxdeploy-plugin-gtk/master/linuxdeploy-plugin-gtk.sh"

cargo build --release

rm -rf "$appdir"
mkdir -p "$appdir/usr/share/icons/hicolor/symbolic/apps"
cp data/icons/io.github.cmumby.Outpost-symbolic.svg "$appdir/usr/share/icons/hicolor/symbolic/apps/"

export PATH="$PWD/$tools:$PATH"
export DEPLOY_GTK_VERSION=4
# Run the tools without FUSE, which build machines often don't have.
export APPIMAGE_EXTRACT_AND_RUN=1
export OUTPUT="$out/Outpost-$arch.AppImage"
export LINUXDEPLOY_OUTPUT_VERSION
LINUXDEPLOY_OUTPUT_VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)

linuxdeploy \
    --appdir "$appdir" \
    --executable target/release/outpost \
    --desktop-file packaging/io.github.cmumby.Outpost.desktop \
    --icon-file data/icons/io.github.cmumby.Outpost.svg \
    --plugin gtk

# The GTK plugin's startup script forces two settings that break libadwaita apps:
# GTK_THEME replaces libadwaita's styling with plain GTK's (rows lose their spacing),
# and GDK_BACKEND=x11 runs through XWayland, where windows can stop redrawing.
sed -i -e '/^export GTK_THEME=/d' -e '/^export GDK_BACKEND=/d' "$appdir/apprun-hooks/linuxdeploy-plugin-gtk.sh"

linuxdeploy --appdir "$appdir" --output appimage

echo "Built $OUTPUT"
