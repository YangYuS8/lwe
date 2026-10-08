# Development Scripts

Useful development and testing scripts for the active LWE workspace.

## Available Scripts

### Development Checks

- `dev-check.sh` - Check development environment setup
- `quick-check.sh` - Frontend types plus workspace Rust format, clippy, and compilation checks
- `pre-push-check.sh` - The eight frontend, documentation, and Rust quality checks

### Packaging

- `ensure-tauri-cli.sh` - Install or verify the exact Cargo Tauri CLI from `.tauri-cli-version`
- `validate-appimage.py` - Validate AppImages or an extracted AppDir before publishing
- `sync-version.sh` - Sync stable workspace and AUR versions (requires `makepkg`)
- `sync-cnb.py` - Mirror GitHub main and verified release packages to CNB without rebuilding

## Usage

### Development Environment Check
```bash
# Check that all dependencies and tools are available
./scripts/dev-check.sh
```

### Quick Development Check
```bash
# Frontend types and Rust format/clippy/compile checks
./scripts/quick-check.sh
```

### Pre-push Validation
```bash
# Frontend/docs/Rust validation before pushing
./scripts/pre-push-check.sh

# Skip only Rust tests; frontend tests still run
SKIP_TESTS=1 ./scripts/pre-push-check.sh
```

`SKIP_TESTS=1` is partial validation. Packaging regressions are a separate Quality Check step:

```bash
python3 -m unittest discover -s scripts/tests -p 'test_*.py'
```

### Tauri CLI and AppImage Validation

Use Node.js 24, pnpm pinned by `package.json`, and Rust pinned by `rust-toolchain.toml`. From the repository root:

```bash
./scripts/ensure-tauri-cli.sh
export PATH="$PWD/target/tauri-cli/bin:$PATH"
cargo tauri --version
python3 scripts/validate-appimage.py target/release/bundle/appimage/*.AppImage
python3 scripts/validate-appimage.py --appdir path/to/LWE.AppDir
```

`ensure-tauri-cli.sh` defaults to an isolated installation in `target/tauri-cli` and accepts `CARGO_INSTALL_ROOT` for another location. Add that root's `bin/` directory to `PATH` when using the installed CLI. Both release workflows share `.tauri-cli-version` for installation, cache keys, and version verification.

AppImage validation requires Python 3, `unsquashfs` (squashfs-tools), `file`, and `desktop-file-validate` (desktop-file-utils). It inspects the final package without running or repairing it. A package that fails validation must not be uploaded. See the maintained [contributor guide](../docs/contributing/guide.md) for the release and real-desktop acceptance checklists.

### CNB Mirror

GitHub Actions owns code, package builds, and publishing. `Sync CNB` mirrors the current `main`; stable and prerelease workflows call it with their published tag and exact source SHA. CNB keeps its EdgeOne documentation deployment under `api_trigger_docs` and has automatic Git-triggered builds disabled.

Use a repository-scoped `CNB_TOKEN` Actions secret. The maintained [contributor guide](../docs/contributing/guide.md#github-to-cnb-mirror) describes permissions, migration, and manual replay. For a local replay with `CNB_TOKEN` supplied in the environment and authenticated `gh`:

```bash
python3 scripts/sync-cnb.py --release-tag v0.9.11
# Retry documentation deployment independently:
python3 scripts/sync-cnb.py --deploy-docs
```

Every selected package is downloaded from GitHub, verified against its SHA-256 digest, and read back from CNB after upload. Releases remain drafts until verification completes. Conflicting tags or attachment contents fail; the script never replaces them.

## Testing

The active product path uses Rust's built-in test framework across the LWE shell and the retained crates `lwe-core`, `lwe-library`, and `lwe-engine`:

```bash
# Run all workspace tests
cargo test --workspace

# Run tests for a specific crate
cargo test -p lwe-shell
cargo test -p lwe-core
cargo test -p lwe-library
cargo test -p lwe-engine
```

## Building

```bash
# Debug build
cargo build --workspace

# Release build
cargo build --release --workspace

# Run the active shell from the workspace root with the frontend dev server
pnpm tauri:dev
```

`cargo run -p lwe-shell` starts only the Rust shell. During development the
application window expects the Vite dev server configured in
`src-tauri/tauri.conf.json`, so use `pnpm tauri:dev` or start `pnpm dev`
separately before running the shell binary.

The retired wayvid GUI/CLI crates have been removed; their history is available in Git, not as workspace build targets.
