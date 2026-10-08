# Installation

This guide explains how to install LWE and what you need before the first launch.

## Prerequisites

LWE works with Wallpaper Engine content from Steam Workshop. Before using Workshop-related features, make sure that:

- your Steam account owns Wallpaper Engine;
- the Steam client is installed and signed in on the Linux desktop;
- Wallpaper Engine is installed in Steam;
- the desktop session supports the runtime path you want to test.

The currently verified desktop environment is a Wayland session with `niri`. Other compositors or desktop environments may work, but they should be treated as unverified until tested.

You can open LWE before installing Steam or downloading wallpapers. An empty Library is a normal first-launch state: subscribe to Wallpaper Engine items in Steam, wait for Steam to download them, then refresh the Library. On X11 or without `niri`, Library and Workshop browsing remain available while wallpaper playback requires the supported desktop session.

## Arch Linux AUR

Two AUR packages are published:

| Package | Channel | Use when |
| --- | --- | --- |
| `lwe` | Stable | You want the latest stable release. |
| `lwe-git` | Prerelease/development | You want newer builds from the active development branch. |

Install the stable package with an AUR helper such as `yay`:

```bash
yay -S lwe
```

Install the development package when you intentionally want prerelease changes:

```bash
yay -S lwe-git
```

## GitHub Releases

Stable and prerelease builds publish Linux artifacts from GitHub Actions. Download the package that matches your distribution from the repository Releases page.

Published artifact types include:

- `.deb`
- `.rpm`
- `.AppImage`

Use your distribution's normal package tooling for `.deb` or `.rpm` files. For AppImage builds, mark the file executable before launching it.

```bash
chmod +x lwe_*.AppImage
./lwe_*.AppImage
```

## Build from source

Use this path when contributing or when you need to validate a local change.

Required tooling:

- Node.js 24, matching CI
- pnpm 11.1.3, pinned in `package.json`
- Rust 1.99.0, selected by `rust-toolchain.toml`
- Tauri 2 build dependencies for your distribution

The minimum supported Rust version is declared separately in `Cargo.toml`. Desktop builds use the exact Cargo Tauri CLI from `.tauri-cli-version`; the shared installer verifies or installs it.

Install JavaScript dependencies:

```bash
pnpm install --frozen-lockfile
pnpm exec svelte-kit sync
./scripts/ensure-tauri-cli.sh
export PATH="$PWD/target/tauri-cli/bin:$PATH"
```

Run frontend checks:

```bash
pnpm check
pnpm test
```

Run Rust checks:

```bash
cargo check --workspace
```

Start local desktop development with `pnpm tauri:dev`. The active desktop app path is `src-tauri` with the frontend under `src`; `pnpm dev` alone runs only the frontend. For package builds and acceptance checks, follow the [contributor guide](../contributing/guide.md).

## After installation

Continue with [Quick start](./quick-start.md) to configure the Steam Web API key and apply your first wallpaper.
