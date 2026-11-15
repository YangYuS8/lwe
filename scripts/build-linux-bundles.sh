#!/usr/bin/env bash
set -euo pipefail

case "${1:-}" in
  appimage)
    bundles=appimage
    extensions=(AppImage)
    ;;
  native)
    bundles=deb,rpm
    extensions=(deb rpm)
    ;;
  *)
    echo "Usage: $0 <appimage|native>" >&2
    exit 2
    ;;
esac

cd "$(dirname "${BASH_SOURCE[0]}")/.."
rustc -Vv
cargo -V
node --version
pnpm --version
bash scripts/ensure-tauri-cli.sh
pnpm install --frozen-lockfile

# Rust caches may contain bundles from another release.
rm -rf target/release/bundle src-tauri/target/release/bundle dist
cargo tauri build -b "$bundles" -- --locked

shopt -s nullglob
mkdir -p dist
for extension in "${extensions[@]}"; do
  bundle_dir="${extension,,}"
  files=(target/release/bundle/"$bundle_dir"/*."$extension" src-tauri/target/release/bundle/"$bundle_dir"/*."$extension")
  if [ "${#files[@]}" -ne 1 ]; then
    echo "Expected exactly one $extension bundle, found ${#files[@]}" >&2
    exit 1
  fi
  cp -v "${files[@]}" dist/
done

if [ "$bundles" = appimage ]; then
  python3 scripts/validate-appimage.py dist/*.AppImage
fi
sha256sum dist/*
