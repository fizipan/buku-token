#!/bin/sh
# Builds the DMG, publishes it as a GitHub release and updates the Homebrew cask.
# Usage: scripts/release.sh <path to a homebrew-tap checkout>
# Bump the version in src-tauri/tauri.conf.json first, and commit it. Run this on the Mac
# that holds the "Buku Token Local" certificate: releases must keep the same signing identity,
# or macOS asks for Keychain access again after every upgrade.
set -eu

tap=${1:?usage: scripts/release.sh <path to a homebrew-tap checkout>}
cask="$tap/Casks/buku-token.rb"
[ -f "$cask" ] || { echo "No cask at $cask" >&2; exit 1; }
[ -z "$(git status --porcelain)" ] || { echo "Commit your changes first." >&2; exit 1; }

version=$(node -p "require('./src-tauri/tauri.conf.json').version")
tag="v$version"
pnpm tauri build --bundles app,dmg

dmg="src-tauri/target/release/bundle/dmg/Buku Token_${version}_aarch64.dmg"
sha=$(shasum -a 256 "$dmg" | cut -d' ' -f1)
asset="Buku-Token_${version}_aarch64.dmg"
cp "$dmg" "/tmp/$asset"
gh release create "$tag" "/tmp/$asset" --title "$tag" --generate-notes
rm "/tmp/$asset"

sed -i '' -e "s/^  version \".*\"/  version \"$version\"/" -e "s/^  sha256 \".*\"/  sha256 \"$sha\"/" "$cask"
git -C "$tap" add Casks/buku-token.rb
git -C "$tap" commit -m "buku-token $version"
git -C "$tap" push
echo "Released $tag: brew upgrade --cask buku-token"
