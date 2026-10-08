# AGENTS.md

## Product constraints

LWE is a Linux desktop app for Wallpaper Engine migration: Workshop discovery and acquisition, compatibility reporting, a local Library, and per-monitor wallpaper assignments. It uses Tauri 2, SvelteKit/Svelte 5, and Rust backend services.

- Video wallpapers on Wayland with `niri` are the documented verified runtime target. Recognizing Scene/Web content or detecting Wayland protocols does not establish runtime support.
- Keep product copy in English and Simplified Chinese (`src/lib/i18n.ts`). Update both languages and check layout when changing user-facing text.
- Keep maintained product/contributor documentation in `docs/`, with matching pages under `docs/zh/`. Root README and agent instructions are concise entry points.
- Use LWE terminology. Do not revive the retired wayvid GUI/CLI or deleted OpenSpec/archive workflow. Preserve historical names when they are still required by compatibility paths.

## Find the relevant code

Read the entry points and callers needed for the task; use these references when their subject is affected:

| Area | Entry points |
| --- | --- |
| UI and IPC | `src/routes/`, `src/lib/components/`, `src/lib/ipc.ts`, `src/lib/types.ts`, `src/lib/stores/ui.ts` |
| Rust application | `src-tauri/src/`: `commands/`, `services/`, `results/`, `assembly/`, `models.rs`, `action_outcome.rs` |
| Shared business rules | `src-tauri/src/policies/shared/` (support, compatibility, cover, invalidation) |
| Retained crates | `crates/lwe-core/` (types/config), `crates/lwe-library/` (Workshop scanning/metadata/assets), `crates/lwe-engine/` (Wayland/mpv/EGL runtime) |
| Product scope and contributor policy | `docs/contributing/project.md`, `docs/contributing/guide.md`; roadmap in `docs/contributing/roadmap.md` |
| Checks, packaging, releases | `package.json`, `.github/workflows/`, `scripts/`, `packaging/aur/`; release smoke checklist in the contributor guide |

## Run locally

Use Node.js 24 (matching CI), pnpm pinned by `package.json`, the Rust toolchain pinned by `rust-toolchain.toml`, and the Linux Tauri/mpv development libraries listed in `.github/workflows/quality-check.yml`. `Cargo.toml` records the minimum supported Rust version separately. `.tauri-cli-version` is the sole Cargo CLI version source; run `./scripts/ensure-tauri-cli.sh` to install or verify that exact version in `target/tauri-cli` by default. Set `CARGO_INSTALL_ROOT` to customize the location and add its `bin/` directory to `PATH`.

From the repository root:

```bash
pnpm install --frozen-lockfile
pnpm exec svelte-kit sync
./scripts/ensure-tauri-cli.sh
export PATH="$PWD/target/tauri-cli/bin:$PATH"
pnpm tauri:dev
```

`pnpm dev` runs the frontend at `127.0.0.1:1420`; it does not exercise the Rust services or wallpaper runtime. `cargo run -p lwe-shell` requires that frontend server to be started separately. Use `pnpm docs:dev` for the documentation site.

## Preserve application contracts

- Keep commands thin: frontend IPC -> Tauri command -> service/application result -> assembly/frontend model. Business decisions belong in services and shared policies, with runtime integration in the existing backend/engine paths.
- For IPC changes, keep Rust serialization and `src/lib/ipc.ts`/`src/lib/types.ts` aligned; register new commands in `src-tauri/src/lib.rs`. Preserve `ActionOutcome` business success/failure, messages, current updates, shell patches, and page invalidations; a resolved IPC call alone does not mean the action succeeded.
- Preserve snapshot/cache invalidation and unavailable/stale/error states across Workshop, Library, Desktop, and Settings. Reuse existing policies rather than duplicating compatibility or support rules in the UI.
- Missing Steam or downloaded Workshop content is normal first-launch setup, and an unsupported desktop session is a playback requirement. Keep Library/Workshop browsing available in those cases; preserve real filesystem, command, and parsing failures. Include a fresh-user startup check for packaging changes.
- Trace the active call path before changing retained crate code. The shell's Library is currently a projection of assessed Workshop entries; the SQLite code in `lwe-library` is not the shell's active Library persistence path.
- Settings and assignments use `$XDG_CONFIG_HOME/lwe/settings.toml` and `lwe/session.toml` (default config root: `~/.config`). Preserve atomic writes and the distinction between missing state and failed reads. Use isolated fixtures for persistence tests; do not reset user state as routine test setup. Keep Steam API keys redacted in diagnostics and reports.

## Validate the change

Choose checks by the affected behavior and its callers. Add or update regression coverage for compatibility, import, runtime, and persisted-state changes, or explain the necessary manual verification. Broaden checks when shared dependencies or multiple layers are affected.

| Change | Required validation |
| --- | --- |
| Documentation or agent instructions only | `pnpm docs:build` and `git diff --check` |
| Frontend only | `pnpm check`, `pnpm test`, `pnpm build` |
| A single Rust crate | `cargo fmt --all -- --check`, `cargo clippy -p <crate> --all-targets -- -D warnings`, `cargo test -p <crate>`; check affected dependent crates too |
| Broad maintenance, cross-layer contracts, shared dependencies or build/workspace changes | Full CI set below |
| Packaging/release automation | Run the packaging regressions and validate the actual AppImage with `scripts/validate-appimage.py`; validate affected workflows/package metadata and use the contributor guide's release smoke checklist |

Build docs whenever maintained documentation changes. After a fresh install, run `pnpm exec svelte-kit sync` before frontend type checks, as CI does.

Full CI set (`.github/workflows/quality-check.yml`):

```bash
pnpm check
pnpm test
pnpm build
pnpm docs:build
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo check --workspace
cargo test --workspace
python3 -m unittest discover -s scripts/tests -p 'test_*.py'
```

`scripts/quick-check.sh` covers frontend types plus Rust fmt/clippy/check. `scripts/pre-push-check.sh` runs the eight frontend/docs/Rust checks above; run packaging regressions separately. `SKIP_TESTS=1` skips only Rust tests and is not full validation.

Real desktop tests are opt-in: they apply/clear wallpapers and write session state. Run them only when the task calls for desktop acceptance and a suitable Wayland + `niri` session, monitors, GPU/EGL, mpv, Steam Workshop content, and video assets are available:

Use the contributor guide's isolated test command and real-desktop checklist to verify repeated application in the same backend, visible application, independent per-monitor clear, restart/restore, and clear-all. The opt-in flag is `LWE_REAL_DESKTOP_TESTS=1`; keep `HOME` available for Workshop discovery while isolating `XDG_CONFIG_HOME`. Ordinary tests or a package/frontend build do not prove desktop runtime support; report desktop acceptance separately.

## Finish and maintain

- Review the diff, fix failures introduced by the change, and report what changed, checks run, and any unverified behavior or pre-existing blockers. Complete the authorized delivery steps.
- Do not commit generated outputs (`build/`, `.svelte-kit/`, `target/`, `src-tauri/gen/`, `docs/.vitepress/dist/`) or local credentials. Keep application lockfiles tracked.
- `Cargo.toml` is the workspace version source. Stable X.Y.Z bumps use `scripts/sync-version.sh` (requires `makepkg`); prerelease versions are derived by release workflows. Verify AUR `PKGBUILD`/`.SRCINFO` against the selected channel.
- Stable and prerelease workflows verify the exact Cargo CLI after cache restore and validate every AppImage before uploading release artifacts. Keep `.DirIcon` and the desktop icon usable without paths from the build machine; do not bypass the gate by repairing a built package in place.
- Build AppImages on Ubuntu 22.04 and native deb/rpm packages on Ubuntu 24.04 to retain the libmpv.so.2 ABI used by Arch packages. Keep caches separated by distribution. Keep workflow syntax validation in Quality CI; valid YAML alone does not validate GitHub Actions expressions.
- Successful Quality Check runs for pushes to `main` trigger prereleases; `v*` tags trigger stable releases. Treat these pushes and release dispatches as publishing actions, not local validation.
- Keep these instructions accurate when commands, contracts, or support boundaries change. Add directory-specific instructions only when that area needs distinct rules; keep detailed procedures in the relevant docs.

Guidance structure follows OpenAI's [AGENTS.md guide](https://learn.chatgpt.com/docs/agent-configuration/agents-md) and [Codex best practices](https://learn.chatgpt.com/guides/best-practices): concise repository rules, task-relevant references, runnable commands, and explicit verification/completion criteria.
