#!/bin/sh
# Per-user installation of an extracted, verified macOS archive.
set -eu
source_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
prefix="$HOME/.local"
apps_dir="$HOME/Applications"
if [ "$#" -gt 0 ]; then
  if [ "$#" -ne 2 ] || [ "$1" != "--prefix" ]; then printf 'Usage: ./install.sh [--prefix directory]\n' >&2; exit 2; fi
  mkdir -p "$2"
  prefix=$(CDPATH= cd -- "$2" && pwd)
  apps_dir="$prefix/Applications"
fi
install_dir="$prefix/lib/hycli"
app_bundle="$apps_dir/Hycli.app"
app_macos="$app_bundle/Contents/MacOS"
app_resources="$app_bundle/Contents/Resources"
mkdir -p "$install_dir" "$prefix/bin" "$app_macos" "$app_resources"
if [ -x "$install_dir/hycli" ]; then "$install_dir/hycli" stop; fi
for binary in hycli hycli-desktop; do
  install -m 755 "$source_dir/$binary" "$install_dir/.$binary.new"
  mv -f "$install_dir/.$binary.new" "$install_dir/$binary"
  ln -sfn "$install_dir/$binary" "$app_macos/$binary"
done
install -m 644 "$source_dir/LICENSE" "$install_dir/LICENSE"
install -m 644 "$source_dir/LICENSE" "$app_resources/LICENSE"
if [ -f "$source_dir/hycli.png" ]; then
  install -m 644 "$source_dir/hycli.png" "$app_resources/hycli.png"
fi
ln -sfn "$install_dir/hycli" "$prefix/bin/hycli"
cat > "$app_bundle/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>io.github.hybirdss.Hycli</string>
<key>CFBundleName</key><string>Hycli</string>
<key>CFBundleDisplayName</key><string>Hycli</string>
<key>CFBundleExecutable</key><string>hycli-desktop</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>LSMinimumSystemVersion</key><string>13.0</string>
<key>LSUIElement</key><true/>
<key>NSHighResolutionCapable</key><true/>
</dict></plist>
EOF
printf 'Installed Hycli (%s). CLI: %s\n' "$app_bundle" "$prefix/bin/hycli"
