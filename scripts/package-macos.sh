#!/bin/sh
set -eu

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

version="$(sed -n '/^\[workspace.package\]/,/^\[/s/^version = "\(.*\)"/\1/p' Cargo.toml)"
out="$root/target/dist"
app="$out/Bobba.app"
dmg="$out/Bobba-$version-macos.dmg"

for target in aarch64-apple-darwin x86_64-apple-darwin; do
  cargo build --release --locked -p bobba --target "$target"
done

rm -rf "$out"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
lipo -create \
  target/aarch64-apple-darwin/release/bobba \
  target/x86_64-apple-darwin/release/bobba \
  -output "$app/Contents/MacOS/bobba"
cp crates/app/assets/Bobba.icns "$app/Contents/Resources/Bobba.icns"

cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>Bobba</string>
  <key>CFBundleDisplayName</key><string>Bobba</string>
  <key>CFBundleIdentifier</key><string>com.ruangustavo.bobba</string>
  <key>CFBundleExecutable</key><string>bobba</string>
  <key>CFBundleIconFile</key><string>Bobba</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$version</string>
  <key>CFBundleVersion</key><string>$version</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST

codesign --force --deep --sign - "$app"

staging="$out/dmg"
mkdir -p "$staging"
cp -R "$app" "$staging/"
ln -s /Applications "$staging/Applications"
hdiutil create -quiet -volname "Bobba" -srcfolder "$staging" -ov -format UDZO "$dmg"
rm -rf "$staging"

echo "$dmg"
