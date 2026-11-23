#!/usr/bin/env bash
# Builds the Linux app: "Campaign Keeper <version>.AppImage" in ../installers.
# Run it from a terminal on any x86_64 Linux machine:  ./build-linux.sh
# A Steam Deck works too, in Desktop Mode, once the packages below are installed.
#
# Needs Node 20+, Rust, and Tauri's Linux prerequisites. On Arch and SteamOS:
#   sudo steamos-readonly disable            # SteamOS only; it resets on the next update
#   sudo pacman -S --needed base-devel curl wget file openssl nodejs npm rust \
#     webkit2gtk-4.1 gtk3 libappindicator-gtk3 librsvg
# On Debian and Ubuntu:
#   sudo apt install libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev \
#     patchelf libgtk-3-dev build-essential curl wget file
#
# GitHub's own runners do the same thing in .github/workflows/linux.yml; use that when there is no
# Linux machine at hand.
set -euo pipefail
cd "$(dirname "$0")"

version=$(node -p "require('./package.json').version")

npm ci
npm run tauri build -- --bundles appimage

appimage=$(ls -t src-tauri/target/release/bundle/appimage/*.AppImage | head -1)
mkdir -p ../installers
cp "$appimage" "../installers/Campaign Keeper $version (Linux & Steam Deck).AppImage"
echo "Built: installers/Campaign Keeper $version (Linux & Steam Deck).AppImage"
