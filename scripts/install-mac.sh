#!/usr/bin/env bash
# Builds Phantom in release mode and installs it as ~/Applications/Phantom.app, where Finder,
# Spotlight and Raycast find it. Rerun to update: a running Phantom (and its service) is stopped,
# replaced and reopened. Torrents resume where they were; the service saves its state.
set -euo pipefail
cd "$(dirname "$0")/.."

APP_DIR="$HOME/Applications"
APP="$APP_DIR/Phantom.app"
BUNDLE_ID="io.github.hugeletters.phantom"
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)

cargo build --release

# Assemble the new bundle beside the old one, so a failed build never leaves no app at all.
stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT
contents="$stage/Phantom.app/Contents"
mkdir -p "$contents/MacOS" "$contents/Resources"
# The GUI finds the service next to its own executable.
cp target/release/phantom-gui "$contents/MacOS/Phantom"
cp target/release/phantom-service "$contents/MacOS/phantom-service"
# The App icons the user can choose and their manifest (design/icons/README.md, "App icons").
# The default, phantom.icns, is also the bundle's own icon.
cp design/icons/app/*.icns design/icons/app/app-icons.json "$contents/Resources/"
cat > "$contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>Phantom</string>
  <key>CFBundleDisplayName</key><string>Phantom</string>
  <key>CFBundleIdentifier</key><string>$BUNDLE_ID</string>
  <key>CFBundleExecutable</key><string>Phantom</string>
  <key>CFBundleIconFile</key><string>phantom</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$VERSION</string>
  <key>CFBundleVersion</key><string>$VERSION</string>
  <key>LSMinimumSystemVersion</key><string>13.0</string>
  <key>LSApplicationCategoryType</key><string>public.app-category.utilities</string>
  <key>NSHighResolutionCapable</key><true/>
  <!-- Magnet links clicked in a browser or anywhere else open in Phantom. -->
  <key>CFBundleURLTypes</key>
  <array>
    <dict>
      <key>CFBundleURLName</key><string>Magnet link</string>
      <key>CFBundleURLSchemes</key><array><string>magnet</string></array>
    </dict>
  </array>
  <!-- .torrent files opened from Finder or dropped on the Dock icon open in Phantom. -->
  <key>CFBundleDocumentTypes</key>
  <array>
    <dict>
      <key>CFBundleTypeName</key><string>Torrent file</string>
      <key>CFBundleTypeRole</key><string>Viewer</string>
      <key>LSHandlerRank</key><string>Default</string>
      <key>LSItemContentTypes</key><array><string>org.bittorrent.torrent</string></array>
    </dict>
  </array>
  <!-- macOS only knows the type when another torrent app declared it; an app that exports it wins. -->
  <key>UTImportedTypeDeclarations</key>
  <array>
    <dict>
      <key>UTTypeIdentifier</key><string>org.bittorrent.torrent</string>
      <key>UTTypeDescription</key><string>Torrent file</string>
      <key>UTTypeConformsTo</key><array><string>public.data</string></array>
      <key>UTTypeTagSpecification</key>
      <dict>
        <key>public.filename-extension</key><array><string>torrent</string></array>
        <key>public.mime-type</key><array><string>application/x-bittorrent</string></array>
      </dict>
    </dict>
  </array>
</dict>
</plist>
EOF
# Icon detritus (Finder info, resource forks) fails the signature's strict check. The new bundle
# has no custom icon of its own (the app sets it again at launch, see
# gui/src/platform/app_icon/appkit.rs), but `cp` carries over the files' extended attributes.
xattr -cr "$stage/Phantom.app"
# Ad hoc signature: enough to run locally (Apple silicon refuses unsigned code).
codesign --force --sign - "$contents/MacOS/phantom-service"
codesign --force --sign - "$stage/Phantom.app"

# Stop the running app and the service it started, so the new versions take over. Matched by
# path, so nothing else that happens to be called Phantom is touched. Older installs named the
# service phantom-daemon and kept its socket and lock under other names, so the new app would
# never find it and it would run on beside the new service: stop it too.
gui="$APP/Contents/MacOS/Phantom"
service="$APP/Contents/MacOS/phantom-service"
old_service="$APP/Contents/MacOS/phantom-daemon"
was_running=false
if pgrep -qf "$gui"; then
  was_running=true
  pkill -TERM -f "$gui" || true
fi
pkill -TERM -f "$service" || true
pkill -TERM -f "$old_service" || true
for _ in $(seq 50); do
  pgrep -qf "$gui" || pgrep -qf "$service" || pgrep -qf "$old_service" || break
  sleep 0.1
done

mkdir -p "$APP_DIR"
rm -rf "$APP"
mv "$stage/Phantom.app" "$APP"
# Tell Launch Services now rather than whenever it next scans, so launchers see it at once.
/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister -f "$APP"

echo "Installed $APP ($VERSION)"
if $was_running; then
  open "$APP"
fi
