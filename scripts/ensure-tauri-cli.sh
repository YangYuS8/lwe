#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
expected_version="$(cat "$repo_root/.tauri-cli-version")"
if ! [[ "$expected_version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "Invalid version in .tauri-cli-version: expected X.Y.Z" >&2
  exit 1
fi

if [ "${1:-}" = "--version" ] && [ "$#" -eq 1 ]; then
  printf '%s\n' "$expected_version"
  exit 0
fi
if [ "$#" -ne 0 ]; then
  echo "Usage: $0 [--version]" >&2
  exit 1
fi

# Use a separate installation by default so local checks do not replace the
# developer's global CLI. CI supplies its own cacheable CARGO_INSTALL_ROOT.
install_root="${CARGO_INSTALL_ROOT:-$repo_root/target/tauri-cli}"
mkdir -p "$install_root/bin"
install_root="$(cd "$install_root" && pwd)"
export PATH="$install_root/bin:$PATH"

cli_version() {
  [ -x "$install_root/bin/cargo-tauri" ] || return 1
  cargo tauri --version
}

actual_version="$(cli_version 2>/dev/null || true)"
if [ "$actual_version" != "tauri-cli $expected_version" ]; then
  echo "Installing tauri-cli $expected_version into $install_root"
  cargo install tauri-cli --version "=$expected_version" --locked --force --root "$install_root"
fi

actual_version="$(cli_version 2>/dev/null || true)"
if [ "$actual_version" != "tauri-cli $expected_version" ]; then
  echo "Tauri CLI version mismatch: expected tauri-cli $expected_version, got ${actual_version:-unavailable}" >&2
  exit 1
fi
printf 'Tauri CLI verified: %s (%s/bin/cargo-tauri)\n' "$actual_version" "$install_root"
