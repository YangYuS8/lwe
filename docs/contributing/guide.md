# Contributor guide

## Development setup

Install the core toolchain:

- Node.js 24 recommended; Node.js 20 or newer may work when supported by the frontend toolchain;
- pnpm;
- Rust stable;
- platform dependencies required by Tauri 2 on your distribution.

Install dependencies:

```bash
pnpm install
```

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

For documentation-only changes, `pnpm docs:build` is the required validation.

## Real desktop runtime acceptance

Runtime changes must be validated on a real supported desktop session before they are described as supported. The current verified target is Wayland with `niri` and video wallpapers.

Session restore uses the LWE session file at `$XDG_CONFIG_HOME/lwe/session.toml`, or `$HOME/.config/lwe/session.toml` when `XDG_CONFIG_HOME` is unset. When testing restore behavior, inspect or remove that file if the Desktop page shows stale saved assignments.

Use this checklist for runtime changes:

1. discover at least one active monitor in LWE;
2. apply a compatible video wallpaper from Library to one monitor;
3. confirm the desktop visibly changes;
4. if multiple monitors are present, apply a wallpaper to a second monitor and then clear only one monitor;
5. confirm clearing one monitor does not stop wallpapers on other monitors;
6. restart LWE and confirm saved assignments are restored or that restore failures are visible in Desktop;
7. clear all assignments and confirm the saved session no longer restores them.

If a runtime step fails, keep the terminal log line that names the failing stage: backend start, output discovery, first-frame apply, per-monitor clear, or startup restore. These messages are the supported way to distinguish missing video assets, output mismatches, Wayland layer-shell/EGL failures, and backend timeouts.

Real desktop tests in the Rust test suite are opt-in because they depend on the active compositor, monitor layout, GPU/EGL stack, Steam Workshop content, and local video assets. Run them explicitly on a verified machine:

```bash
LWE_REAL_DESKTOP_TESTS=1 cargo test -p lwe-shell desktop_apply_flow -- --nocapture
```

## Reporting issues

Useful reports include:

- package type (`lwe`, `lwe-git`, `.deb`, `.rpm`, or `.AppImage`);
- distribution and version;
- session type and compositor/desktop environment;
- monitor layout;
- wallpaper type if known (`video`, `scene`, or `web`);
- compatibility status shown by LWE;
- terminal logs when available.
