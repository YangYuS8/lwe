# Contributor guide

## Development setup

Install the core toolchain:

- Node.js 20 or newer;
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
```

Run Rust checks:

```bash
cargo check --workspace
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
cargo check --workspace
pnpm docs:build
```

For documentation-only changes, `pnpm docs:build` is the required validation.

## Reporting issues

Useful reports include:

- package type (`lwe`, `lwe-git`, `.deb`, `.rpm`, or `.AppImage`);
- distribution and version;
- session type and compositor/desktop environment;
- monitor layout;
- wallpaper type if known (`video`, `scene`, or `web`);
- compatibility status shown by LWE;
- terminal logs when available.
