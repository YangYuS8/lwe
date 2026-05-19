# Project overview

This page gives contributors the context needed to maintain the current released project.

## Product direction

LWE is a Linux dynamic wallpaper platform for users migrating practical Wallpaper Engine workflows to Linux.

The product is a desktop application first, not a daemon-first system. Its differentiator is an integrated loop for Workshop discovery, acquisition orchestration, synchronization awareness, import, compatibility visibility, and runtime support.

## First-release scope

First-release priorities:

- Workshop-centered discovery and acquisition orchestration;
- compatibility reporting as a visible user-facing feature;
- library-first daily use;
- video wallpaper runtime support;
- English and Simplified Chinese product surfaces;
- Linux desktop integration for applying and clearing wallpapers.

First-release non-goals:

- full web wallpaper runtime parity;
- creator tools;
- cloud or community systems;
- advanced automation rules;
- promising support for unverified desktop environments.

## Active architecture

Key paths:

| Path | Purpose |
| --- | --- |
| `src/` | Svelte/SvelteKit frontend application |
| `src-tauri/` | Active Tauri desktop shell |
| `crates/lwe-core` | Shared models, configuration, and cross-cutting Rust types |
| `crates/lwe-library` | Library indexing, metadata, and local asset management |
| `crates/lwe-engine` | Runtime/playback and Linux wallpaper integration knowledge |
| `packaging/aur/lwe` | Stable AUR package metadata |
| `packaging/aur/lwe-git` | Development AUR package metadata |
| `.github/workflows/` | Quality, release, and documentation deployment automation |

Retired `wayvid` GUI/CLI paths should be treated as historical context only. Do not reintroduce old product framing unless a spec explicitly justifies it for the LWE direction.

## Release model

| Channel | Outputs |
| --- | --- |
| Stable | GitHub Release artifacts and AUR `lwe` |
| Prerelease | GitHub prerelease artifacts and AUR `lwe-git` |

Linux release artifacts include `.deb`, `.rpm`, and `.AppImage` builds.

The workspace version source is `Cargo.toml`. Prerelease versions are derived by release automation from the base version, GitHub Actions run number, and short commit SHA.

## Documentation model

All maintained documentation belongs under `docs/` and is published as a VitePress site. User-facing documentation must exist in both English and Simplified Chinese.

Documentation should prioritize:

1. installation;
2. first-use setup;
3. daily usage;
4. troubleshooting;
5. contributor maintenance rules.

Historical planning documents and obsolete implementation notes should not remain in the published docs tree after their useful content has been merged into maintained pages.
