#!/usr/bin/env bash
# Builds Phantom in release mode and installs it as ~/Applications/Phantom.app, where Finder,
# Spotlight and Raycast find it. Rerun to update: a running Phantom (and its daemon) is stopped,
# replaced and reopened. Torrents resume where they were; the daemon saves its state.
set -euo pipefail
cd "$(dirname "$0")/.."

APP_DIR="$HOME/Applications"
APP="$APP_DIR/Phantom.app"
BUNDLE_ID="com.evperminov.phantom"
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)

cargo build --release

# Assemble the new bundle beside the old one, so a failed build never leaves no app at all.
stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT
contents="$stage/Phantom.app/Contents"
mkdir -p "$contents/MacOS"
# The GUI finds the daemon next to its own executable.
cp target/release/phantom-gui "$contents/MacOS/Phantom"
cp target/release/phantom-daemon "$contents/MacOS/phantom-daemon"
cat > "$contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>Phantom</string>
  <key>CFBundleDisplayName</key><string>Phantom</string>
  <key>CFBundleIdentifier</key><string>$BUNDLE_ID</string>
  <key>CFBundleExecutable</key><string>Phantom</string>
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
</dict>
</plist>
EOF
# Ad hoc signature: enough to run locally (Apple silicon refuses unsigned code).
codesign --force --sign - "$contents/MacOS/phantom-daemon"
codesign --force --sign - "$stage/Phantom.app"

# Stop the running app and the daemon it started, so the new versions take over. Matched by
# path, so nothing else that happens to be called Phantom is touched.
gui="$APP/Contents/MacOS/Phantom"
daemon="$APP/Contents/MacOS/phantom-daemon"
was_running=false
if pgrep -qf "$gui"; then
  was_running=true
  pkill -TERM -f "$gui" || true
fi
pkill -TERM -f "$daemon" || true
for _ in $(seq 50); do
  pgrep -qf "$gui" || pgrep -qf "$daemon" || break
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
