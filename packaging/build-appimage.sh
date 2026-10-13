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

# GIO launches apps through a small helper, gio-launch-desktop, at a path baked in
# for the build machine's distro (Debian's layout here). Other distros keep it
# elsewhere, so ship it and point GIO at it, or opening apps fails on, say, Arch.
# Copy the very file the bundled GIO would have used, so it matches its architecture.
helper=$(strings "$appdir/usr/lib/libgio-2.0.so.0" | grep -m1 '/gio-launch-desktop$' || true)
if [ ! -x "$helper" ]; then
    echo "gio-launch-desktop not found at '$helper'; install GLib's tools (libglib2.0-0t64 on Ubuntu)" >&2
    exit 1
fi
install -m 755 "$helper" "$appdir/usr/bin/gio-launch-desktop"
echo 'export GIO_LAUNCH_DESKTOP="$APPDIR/usr/bin/gio-launch-desktop"' >> "$appdir/apprun-hooks/linuxdeploy-plugin-gtk.sh"

# The image loader list names each loader by bare file name ("libpixbufloader-svg.so"),
# leaving the system's library loader to find it. On some distros (CachyOS, for one)
# it doesn't, and every SVG icon fails to load. At startup, write a copy of the list
# with full paths into the AppImage and use that instead.
cat >> "$appdir/apprun-hooks/linuxdeploy-plugin-gtk.sh" <<'HOOK'
loaders="$APPDIR/usr/lib/gdk-pixbuf-2.0/2.10.0"
cache="${XDG_RUNTIME_DIR:-/tmp}/outpost-pixbuf-loaders.cache"
if sed "s|^\"libpixbufloader|\"$loaders/loaders/libpixbufloader|" "$loaders/loaders.cache" > "$cache"; then
    export GDK_PIXBUF_MODULE_FILE="$cache"
fi
# Don't load the system's GIO plugins (such as gvfs); they're built for a newer GLib
# than the one bundled here.
export GIO_MODULE_DIR="$APPDIR/usr/lib/gio/modules"
HOOK

# GTK's video plugin pulls in the system's GStreamer, which doesn't match the bundled
# GLib. Outpost doesn't play video, so leave it out.
rm -rf "$appdir/usr/lib/gtk-4.0/4.0.0/media"

linuxdeploy --appdir "$appdir" --output appimage

echo "Built $OUTPUT"
