# Development Scripts

Useful development and testing scripts for the active LWE workspace.

## Available Scripts

### Development Checks
- `dev-check.sh` - Check development environment setup
- `quick-check.sh` - Fast workspace checks (format, clippy)
- `pre-push-check.sh` - Full pre-commit validation (tests, build)

## Usage

### Development Environment Check
```bash
# Check that all dependencies and tools are available
./scripts/dev-check.sh
```

### Quick Development Check
```bash
# Fast clippy and format check
./scripts/quick-check.sh
```

### Pre-push Validation
```bash
# Full validation before pushing (all checks + tests)
./scripts/pre-push-check.sh

# Skip tests for faster feedback
SKIP_TESTS=1 ./scripts/pre-push-check.sh
```

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

Retired reference crates:

```bash
# Legacy crates remain in the repository for reference only and are not part of
# the active workspace verification path.
# cargo test --manifest-path crates/wayvid-gui/Cargo.toml
# cargo test --manifest-path crates/wayvid-ctl/Cargo.toml
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

Legacy binaries such as `wayvid-gui` and `wayvid-ctl` are retired reference surfaces and should not be treated as normal active workspace run targets.
