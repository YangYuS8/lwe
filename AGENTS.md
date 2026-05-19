# AGENTS.md

## What this project is

LWE is a Linux desktop application for practical Wallpaper Engine migration workflows. It helps users discover Workshop content, understand compatibility, manage a local Library, and apply supported wallpapers to Linux desktop monitors.

The current application shape is:

- desktop app first, built with Tauri 2 and SvelteKit;
- Rust-owned backend services for Workshop, Library, Desktop, Settings, compatibility, persistence, and runtime integration;
- video wallpapers as the first runtime focus;
- Wayland with `niri` as the currently verified desktop target;
- English and Simplified Chinese as maintained user-facing languages;
- VitePress documentation under `docs/`.

Do not revive the retired wayvid product framing or the deleted OpenSpec/document-archive workflow. Historical names may still exist in compatibility or migration code paths, but new product work should use the LWE direction and terminology.

## Important paths

- `src/` - SvelteKit frontend application.
- `src-tauri/` - active Tauri desktop shell and Rust service layer.
- `crates/lwe-core/` - shared core types and configuration helpers.
- `crates/lwe-library/` - local library, Workshop scanning, metadata, thumbnails, and persistence-related library logic.
- `crates/lwe-engine/` - Wayland/mpv/EGL wallpaper runtime.
- `docs/` - maintained VitePress documentation, with Simplified Chinese pages under `docs/zh/`.
- `.github/workflows/` - CI, release, and documentation deployment workflows.
- `scripts/` - local development and release helper scripts.

## Development rules

1. Keep user-facing behavior aligned with the documented support scope. Do not imply full scene or web wallpaper runtime support unless it is actually implemented and verified.
2. Keep all maintained documentation under `docs/`. Every maintained English documentation page should have a Simplified Chinese counterpart.
3. Update both English and Simplified Chinese user-facing text when changing product copy.
4. Treat real desktop runtime support as verified only after testing on an actual supported session. The current verified target is Wayland with `niri`.
5. Keep Library, Workshop, Desktop, Settings, and runtime responsibilities separated through the existing frontend IPC -> Tauri command -> service -> assembly/model flow.
6. Prefer small, independently verifiable changes. If a change affects compatibility, import paths, runtime behavior, or persisted state, include tests or a clear manual verification note.
7. Do not commit generated build outputs such as `build/`, `.svelte-kit/`, `target/`, or `docs/.vitepress/dist/`.

## Required checks

For broad maintenance or code changes, run:

```bash
pnpm check
pnpm test
pnpm build
pnpm docs:build
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo check --workspace
cargo test --workspace
```

Real desktop tests are opt-in and should only be run on a machine with the required session, monitor, GPU/EGL, mpv, Steam Workshop, and video wallpaper setup:

```bash
LWE_REAL_DESKTOP_TESTS=1 cargo test -p lwe-shell desktop_apply_flow -- --nocapture
```

For documentation-only changes, `pnpm docs:build` is the required minimum check.
