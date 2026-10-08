# Contributor guide

## Development setup

Install the core toolchain:

- Node.js 24, matching CI;
- pnpm 11.1.3, pinned in `package.json`;
- Rust 1.99.0, selected by `rust-toolchain.toml`; the minimum supported Rust version is declared separately in `Cargo.toml`;
- platform dependencies required by Tauri 2 on your distribution.

The Cargo Tauri CLI version is pinned in `.tauri-cli-version` (2.12.1). Use the shared installer to verify or install that exact version rather than an unbounded latest CLI.

Install dependencies:

```bash
pnpm install --frozen-lockfile
pnpm exec svelte-kit sync
./scripts/ensure-tauri-cli.sh
export PATH="$PWD/target/tauri-cli/bin:$PATH"
```

The installer keeps the CLI separate from your existing Cargo tools in `target/tauri-cli` by default. To use another location, set `CARGO_INSTALL_ROOT` when running the installer and add that directory's `bin/` to `PATH` before running `pnpm tauri:dev` or `cargo tauri build`.

Run frontend checks:

```bash
pnpm check
pnpm test
pnpm build
pnpm docs:build
```

Run Rust checks:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo check --workspace
cargo test --workspace
```

Build documentation locally:

```bash
pnpm docs:build
```

Preview documentation locally:

```bash
pnpm docs:preview
```

## Before changing product behavior

Use the maintained documentation in this site as the policy source for product changes.

Every significant change should explain how it supports the Linux dynamic wallpaper platform rather than the retired wayvid product story. It should also state first-release non-goals when scope could be misunderstood.

Design work should identify its area:

- product foundation;
- Workshop loop;
- compatibility;
- runtime;
- application shell.

When content types or import paths are affected, call out user-facing compatibility implications.

## User-facing text and i18n

LWE targets English and Simplified Chinese user-facing surfaces.

When changing user-facing text:

1. update both languages;
2. check layout impact for longer Chinese or English strings;
3. include i18n verification in the task or pull request notes;
4. keep terminology consistent with the documentation.

The documentation site mirrors this rule: every maintained page under the English root must have a Simplified Chinese counterpart under `docs/zh/`.

## Documentation rules

- Keep all maintained documentation under `docs/`.
- Prefer task-oriented pages over archive dumps.
- Delete or merge obsolete planning documents after their useful information is represented in maintained pages.
- Do not reintroduce the deleted `openspec/` tree; project guidance now lives in this documentation site.
- Keep root README files concise and link to the published documentation for details.
- Do not document unsupported runtime behavior as supported.
- If a feature only works on a verified environment, name that environment.

## Testing expectations

Before submitting a change, run the smallest meaningful set of checks. For broad maintenance changes, use:

```bash
pnpm check
pnpm test
pnpm build
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo check --workspace
cargo test --workspace
pnpm docs:build
```

Quality Check also runs the lightweight packaging regressions:

```bash
python3 -m unittest discover -s scripts/tests -p 'test_*.py'
```

For documentation-only changes, `pnpm docs:build` and `git diff --check` are the required validation.

## Packaging validation

Both release workflows read `.tauri-cli-version`, include it in the CLI cache key, and verify `cargo-tauri` after restoring the cache. A cached binary with a different version must be replaced and checked before building.

AppImages are built on Ubuntu 22.04 to keep their glibc requirements compatible with the AppImage catalog test host. Native deb/rpm packages use Ubuntu 24.04 to retain the libmpv.so.2 ABI used by Arch packages. Release CLI and Rust caches are separated by distribution; static metadata validation alone does not prove that an AppImage can launch on the oldest supported system. Quality CI runs checksum-verified actionlint against every workflow.

AppImage validation requires Python 3, `unsquashfs` (from squashfs-tools), `file`, and `desktop-file-validate` (from desktop-file-utils):

```bash
python3 scripts/validate-appimage.py target/release/bundle/appimage/*.AppImage
# Validate an already extracted AppDir instead:
python3 scripts/validate-appimage.py --appdir path/to/LWE.AppDir
```

The validator checks executable `AppRun`, one valid root desktop entry, its icon, and a readable PNG `.DirIcon`. Relevant symbolic links must be relative, resolve inside the AppDir, and have existing targets; ordinary icon files are also accepted. It inspects packages without launching the application or repairing failed packages. Every AppImage selected for upload must pass this gate before release artifacts are published. Check the selected version and remove stale build artifacts from the upload selection.

The regression suite covers valid packages and missing, broken, absolute, or escaping icon links. Passing fixtures does not replace validation of the built release artifact or real desktop acceptance.

## GitHub to CNB mirror

GitHub `main` and its release tags are the authoritative source and build origin. CNB `Nesoriel/lwe` is a one-way code and release mirror. Set the CNB repository's `auto_trigger=false`; `.cnb.yml` keeps only `api_trigger_docs` for EdgeOne documentation deployment. CNB must not repeat Quality Check or Rust/package builds.

The `Sync CNB` workflow mirrors code on pushes to GitHub `main`. Both release workflows call it as a reusable job with `release_tag` and `expected_source_sha` after publishing the GitHub release, rather than depending on a release event generated by `GITHUB_TOKEN`. `workflow_dispatch` can backfill one `release_tag`, or explicitly request the documentation API trigger with `deploy_docs=true`.

CNB writes share a non-canceling GitHub queue (`queue: max`). CI strictly checks that queue declaration and applies an exact exception only for the fixed actionlint version's unsupported queue-field diagnostic; other workflows retain full lint checks. CNB's server SHA-256 and size are verified after upload; legacy assets without SHA-256 are downloaded for verification.

Configure the GitHub repository secret `CNB_TOKEN` with a long-lived CNB personal access token restricted to `Nesoriel/lwe`. Required scopes are:

- `repo-code:rw`;
- `repo-release:rw`;
- `repo-cnb-trigger:rw`;
- `repo-manage:r`.

The local OAuth token lasts eight hours and is unsuitable for persistent CI. An administrator performs the one-time CNB repository setup locally; CI does not receive repository-management write access. Keep token values out of source, logs, and release notes.

Preserve the GitHub tag object and verify its source commit against `expected_source_sha`; never rewrite a tag or force-update CNB `main`, which accepts only fast-forward updates. Copy the public GitHub release assets unchanged, verify the SHA-256 of the `.deb`, `.rpm`, and `.AppImage` on both sides, and retain CNB attachments permanently with `ttl=0`. A rerun fills missing content, skips identical existing assets, and rejects different content under the same name. Any failed sync or verification must remain a failed run.

Source changes alone do not prove that the mirror or EdgeOne deployment is live. After merging and running the workflow, read back the CNB commit, tag object, release assets and hashes; report the documentation trigger/deployment result separately.

## Release smoke checklist

Before publishing a stable tag or merging to `main` for a prerelease, verify the release path that action will exercise:

1. Run the required CI-equivalent checks locally when the change is broad: `pnpm check`, `pnpm test`, `pnpm build`, `pnpm docs:build`, `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo check --workspace`, `cargo test --workspace`, and the packaging regressions above.
2. Confirm the GitHub Actions quality workflow is passing on the commit to be tagged.
3. Confirm release workflows build the expected Linux artifacts (`.deb`, `.rpm`, and `.AppImage`) and pass AppImage validation before upload. After publishing, download the public AppImage, record its SHA-256, and validate it again.
4. Confirm package-channel metadata matches the release channel: AUR stable (`lwe`) for stable tags and AUR git (`lwe-git`) for development/prerelease paths.
5. For runtime-affecting release candidates, run the real desktop runtime acceptance checklist below on a verified Wayland + `niri` session before describing the runtime path as supported.
6. Smoke the fresh install path when possible: install a package artifact, open and move/resize the window, restore it from the tray, open Settings and generate diagnostics, review Workshop/Library state, apply a compatible video wallpaper, clear it, restart LWE, and confirm saved assignment behavior is visible. A Tauri upgrade that changes Wayland startup also needs this check on `niri`.
7. Read back the release and package-channel results. For an AppImage directory repair, request upstream retesting only after the new stable AppImage is publicly available and verified; confirm the upstream log selected the new version.

Package install success only proves the application starts from that package. It does not guarantee runtime support on an unverified compositor, GPU/EGL stack, monitor layout, or wallpaper type.

## Real desktop runtime acceptance

Runtime changes must be validated on a real supported desktop session before they are described as supported. The current verified target is Wayland with `niri` and video wallpapers.

Session restore uses the LWE session file at `$XDG_CONFIG_HOME/lwe/session.toml`, or `$HOME/.config/lwe/session.toml` when `XDG_CONFIG_HOME` is unset. Use an isolated `XDG_CONFIG_HOME` for acceptance runs and inspect that test session file when checking restore behavior. Do not delete existing user settings or assignments as routine test setup.

Use this checklist for runtime changes:

1. discover at least one active monitor in LWE;
2. apply a compatible video wallpaper from Library to one monitor;
3. confirm the desktop visibly changes;
4. if multiple monitors are present, apply a wallpaper to a second monitor and then clear only one monitor;
5. confirm clearing one monitor does not stop wallpapers on other monitors;
6. restart LWE and confirm saved assignments are restored or that restore failures are visible in Desktop;
7. clear all assignments and confirm the saved session no longer restores them;
8. apply, clear, and reapply a video at least three times in the same process to check EGL resource reuse.

If a runtime step fails, keep the terminal log line that names the failing stage: backend start, output discovery, first-frame apply, per-monitor clear, or startup restore. These messages are the supported way to distinguish missing video assets, output mismatches, Wayland layer-shell/EGL failures, and backend timeouts.

Real desktop tests in the Rust test suite are opt-in because they depend on the active compositor, monitor layout, GPU/EGL stack, Steam Workshop content, and local video assets. Run them explicitly on a verified machine:

```bash
lwe_test_config="$(mktemp -d /tmp/lwe-real-desktop.XXXXXX)" && \
XDG_CONFIG_HOME="$lwe_test_config" LWE_REAL_DESKTOP_TESTS=1 \
  cargo test --locked -p lwe-shell --lib \
  services::desktop_service::tests::desktop_apply_flow_reapplies_video_after_clear_on_same_backend \
  -- --exact --nocapture --test-threads=1
```

## Reporting issues

Useful reports include:

- package type (`lwe`, `lwe-git`, `.deb`, `.rpm`, or `.AppImage`);
- distribution and version;
- session type and compositor/desktop environment;
- monitor layout;
- wallpaper type if known (`video`, `scene`, or `web`);
- compatibility status shown by LWE;
- copyable diagnostics from Settings, with the Steam Web API key redacted;
- terminal logs when available.
