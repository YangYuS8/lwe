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

Frontend tests include both server rendering and real component interactions in a simulated DOM. `pnpm test` runs both projects. For a focused run, use `pnpm test --project server` or `pnpm test --project interaction`; interaction fixtures mock IPC while clicking the mounted components. Cover business failures (`ActionOutcome.ok=false`), rejected IPC, and successful actions whose subsequent refresh fails. Keep existing content available for retries and distinguish refreshing data from repeating a successful action.

Tauri commands run synchronous service work through `commands/background.rs`. Desktop-dependent commands wait for background startup restore, and writes stay serialized in the service layer. Session and settings files use private temporary files and atomic replacement; persistence tests must prove that partial-write or replacement failures retain the previous file. Use fake API keys and local HTTP fixtures for network error redaction tests.

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

Runtime changes must be validated on a real supported desktop session before they are described as supported. Video on Wayland with `niri` is the main verified target. Experimental Scene acceptance is limited to the sample and environment recorded below.

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

For the experimental native 2D Scene subset, keep at least one locally validated Scene and one runnable video in Library, then use a separate test config directory:

```bash
lwe_scene_test_config="$(mktemp -d /tmp/lwe-scene-desktop.XXXXXX)" && \
XDG_CONFIG_HOME="$lwe_scene_test_config" LWE_REAL_DESKTOP_TESTS=1 \
  cargo test --locked -p lwe-shell --lib \
  services::desktop_service::tests::desktop_apply_flow_scene_video_switch_clear_and_restore \
  -- --exact --nocapture --test-threads=1
```

This test defaults to three apply/clear cycles and checks Scene → Video → Scene switches in the same backend, rollback after an isolated session-save failure, cancellation after the commit command has been processed, saved Scene assignment restore after backend shutdown/join/restart, and final clear. `LWE_REAL_DESKTOP_SCENE_CYCLES` accepts 1–1,000 cycles; use 100 to repeat the recorded mixed-source stress run, with the same isolated config directory:

```bash
XDG_CONFIG_HOME="$lwe_scene_test_config" LWE_REAL_DESKTOP_TESTS=1 \
LWE_REAL_DESKTOP_SCENE_CYCLES=100 \
  cargo test --locked -p lwe-shell --lib \
  services::desktop_service::tests::desktop_apply_flow_scene_video_switch_clear_and_restore \
  -- --exact --nocapture --test-threads=1
```

Keep `HOME` available for Steam discovery and inspect the isolated session file when checking persistence. Also confirm the wallpaper is visible and its supported effects animate. Increasing the cycle count is a procedure, not a recorded pass. The test covers only the current 2D subset; it does not establish full Scene compatibility. Independent output rendering and clearing require actual multiple-monitor hardware: a single-monitor run cannot cover those cases.

### Scene inventory

The read-only inventory example accepts one project directory or a directory containing up to 64 packaged projects. Replace the paths with the installed Steam library and keep reports local. It does not need the desktop opt-in flag:

```bash
lwe_scene_projects="$HOME/.local/share/Steam/steamapps/workshop/content/431960"
lwe_scene_assets="$HOME/.local/share/Steam/steamapps/common/wallpaper_engine/assets"
lwe_scene_inventory_report="$(mktemp /tmp/lwe-scene-inventory.XXXXXX.json)"
cargo run --locked -p lwe-scene --example inventory -- \
  "$lwe_scene_projects" "$lwe_scene_assets" > "$lwe_scene_inventory_report"
```

`SceneInventory::collect` follows modeled references and scans packaged JSON/TEX/shaders plus project metadata. Loose files are followed by references without recursive directory enumeration. The report distinguishes reachable and packaged-only declarations, `user`/`value` bindings and JS, HDR and bloom, resource origins, framing, and structural unknowns. It omits script/shader/text contents and private values, including asset-looking strings in unknown/text subtrees. It never marks content runnable or supported. Budget exhaustion and invalid/missing resources produce explicit partial coverage; dynamic/script-created references remain unknown.

Default per-traversal limits include 8,192 resources, 32 MiB of JSON, 16 MiB of shader sources, and 16,384 mips plus 16,384 animation frames; document, depth/node, observation, and report-string limits also apply. The framing reader consumes the shared remaining mip/frame budgets before loops and allocations, skips media payloads, and performs no decoding. PKG indexing, range reads, and hashing use the same open descriptor; metadata checks detect ordinary in-place changes and keep a replaced pathname from redirecting the hash. This is a conservative consistency check, not an immutable filesystem snapshot. A complete bounded traversal does not prove complete runtime semantics.

### Scene UI and runtime measurement

Run the runtime modes of these opt-in tools only for desktop acceptance on a suitable Wayland + `niri` session: they apply and clear real wallpapers. Keep `HOME` for Steam discovery and use the locally acquired project/assets paths. The tools do not substitute for original Wallpaper Engine visual A/B or multiple-monitor/endurance acceptance.

`scene_probe` starts its own engine backend, uses a 30 FPS engine limit, and measures animated, paused, and resumed phases under frequent `GetStatus` requests. It emits JSON with draw/swap/callback counters, elapsed time, and logical Scene payload; it does not save settings/session files or test the Tauri UI. Replace `eDP-1` with the actual output:

```bash
lwe_scene_project="$HOME/.local/share/Steam/steamapps/workshop/content/431960/3337606455"
lwe_scene_assets="$HOME/.local/share/Steam/steamapps/common/wallpaper_engine/assets"
lwe_scene_probe_report="$(mktemp /tmp/lwe-scene-probe.XXXXXX.json)"
LWE_REAL_DESKTOP_TESTS=1 cargo run --locked -p lwe-engine --example scene_probe -- \
  "$lwe_scene_project" "$lwe_scene_assets" eDP-1 60 \
  > "$lwe_scene_probe_report"
```

Add `--static /absolute/path/to/an-authored-static-project` to test a static first frame while paused and a separate static idle phase. The static project must pass preparation and contain no animated effects. Without it, the report has no static-phase evidence. The probe checks the frame cap under command traffic, settled paused/static zero activity, clear, and shutdown/join; its deliberate status traffic is not a normal idle CPU/power baseline. The reported logical RGBA texture/FBO bytes exclude actual driver VRAM, EGL buffers, and video resources.

Add `--properties /absolute/path/to/choices.json` to prepare and measure a project with author-property overrides. The JSON must be an object mapping declared names to typed values and is limited to 64 KiB. Preparation uses the same validation as LWE; the probe does not modify the original project or persist those choices to settings/session files.

`scene_resources` separates CPU preparation, Scene rendering, video rendering, mixed-source switching, and repeated backend restart. It requires `LWE_REAL_DESKTOP_TESTS=1` even for its `cpu` mode. Runtime modes require exactly one physical output. Each clear is confirmed by an empty active-session/metrics status before sampling; backend shutdown is joined. It streams one JSON line per snapshot instead of retaining a growing report, and writes no settings/session files:

```bash
lwe_scene_project="$HOME/.local/share/Steam/steamapps/workshop/content/431960/3337606455"
lwe_scene_assets="$HOME/.local/share/Steam/steamapps/common/wallpaper_engine/assets"
lwe_scene_video="/absolute/path/to/local/video.mp4"
lwe_scene_resource_report="$(mktemp /tmp/lwe-scene-resources.XXXXXX.jsonl)"
LWE_REAL_DESKTOP_TESTS=1 cargo run --locked -p lwe-engine --example scene_resources -- \
  "$lwe_scene_project" "$lwe_scene_assets" "$lwe_scene_video" mixed 30 \
  > "$lwe_scene_resource_report"
```

Replace `mixed` with `cpu`, `scene`, `video`, or `restart`; the cycle count is 1–200. `cpu` prepares/drops the Scene without starting a desktop backend; `restart` applies/clears the Scene and shuts down/joins a backend every cycle. The other modes retain one backend across cycles.

The JSON records Rust requested live/peak bytes, glibc used/free/mmap blocks, `/proc/self/smaps` RSS categories, available DRM fdinfo memory fields, FDs, and threads. Rust counts exclude direct native allocations, and its logical realloc peak excludes a native allocator's temporary old/new buffers. glibc free blocks are retained allocator space, not live wallpaper assets; mmap and driver allocations need separate interpretation. Sampling itself allocates temporary `/proc`/JSON buffers and has a fixed warm-up cost. These process snapshots are not total VRAM, frame timing, power, or proof that every native allocation is leak-free. A fresh child process with `MALLOC_ARENA_MAX=2` is a controlled allocator diagnostic only; it is not a production setting or accepted memory fix.

`scripts/scene-ui-smoke.py` uses native Tauri WebDriver DOM actions and real IPC. It selects the Library item and monitor with native pointer clicks, applies, reads Desktop feedback and the isolated session, checks titlebar close-to-hide without stopping the wallpaper, restarts its owned application process, confirms restore, clears, and optionally checks a blocked item. It writes UI/desktop captures and `report.json`. The harness currently requires exactly one output in its target compositor, a built shell, `tauri-driver` and `WebKitWebDriver` available on `PATH`, `grim`, an unnamed empty workspace on the focused output, and acquired Scene content. A nested compositor's output must be reported as nested, not as physical-monitor acceptance. UI monitor labels may differ from backend output names; the report records both. For a debug shell, run `pnpm dev` in a separate terminal and build `cargo build --locked -p lwe-shell` first:

```bash
lwe_scene_project="$HOME/.local/share/Steam/steamapps/workshop/content/431960/3337606455"
lwe_scene_blocked_project="$HOME/.local/share/Steam/steamapps/workshop/content/431960/2454418355"
lwe_scene_driver="$(command -v tauri-driver)"
lwe_scene_ui_report="$(mktemp -d /tmp/lwe-scene-ui-report.XXXXXX)"
LWE_REAL_DESKTOP_TESTS=1 python3 scripts/scene-ui-smoke.py \
  --binary "$PWD/target/debug/lwe-shell" --driver "$lwe_scene_driver" \
  --project "$lwe_scene_project" --blocked-project "$lwe_scene_blocked_project" \
  --blocked-reason unsupported_hdr \
  --output "$lwe_scene_ui_report"
```

The script creates and removes its own isolated `XDG_CONFIG_HOME`, preserves `HOME`, temporarily names its acceptance workspace, and restores the prior focused workspace when it finishes. Wallpaper capture verifies that this workspace is focused and unobstructed. `--output` is a report directory, not a monitor name. Review the screenshots as well as its business-state assertions; a visible Running label alone is insufficient. Missing drivers, a suitable empty workspace, or desktop prerequisites block acceptance. These tools do not establish physical battery transitions, measured VRAM, power, a 24-hour soak, or independent multiple-output behavior.

Add `--playback-settings` for native Settings actions: save 15 FPS with pause-on-battery, change to 30 FPS while the wallpaper runs, and verify saved preferences after the owned-process restart. The example explicitly checks `unsupported_hdr` on `2454418355`; reassess the blocked item/reason before rerunning. `3659880761` now passes original-package preparation and is no longer the current bloom-blocked example. Saved UI values do not measure cadence or physical battery transitions; the separate desktop-service measurement below covers engine cadence.

For the native property/failure/fullscreen acceptance, use the default `3659880761` project and add `--scene-properties --scene-properties-write-failure --long-occlusion` (optionally `--playback-settings`). `--scene-properties` needs authored bool and changeable slider controls; it saves edits, verifies session/control restoration after restart, and resets every author default. The write-failure flag requires a non-root user and changes permissions only inside the harness-owned isolated config, then verifies rollback/draft retention and retries. `--long-occlusion` uses only the owned Tauri fullscreen window for at least twelve seconds; it does not suspend/lock the system or measure callback pause. Review the actual wallpaper, editor, rollback and recovery captures; automated business assertions alone are insufficient.

For a project with many controls, add `--all-scene-properties`; it enables the property flow and edits every declared control. Booleans and sliders use native WebDriver pointer/keyboard actions. Color is changed by dispatching the actual rendered color input's input event, then using the normal Save/IPC flow; this does not test the system color picker. The original `2903458386` run used all twenty controls together with `--scene-properties-write-failure --playback-settings --long-occlusion`.

For a project with exactly one resource-image control, add `--scene-image /absolute/path/to/static.png` together with `--all-scene-properties` and put `wtype` in the test PATH. The harness cancels and selects through the owned native file picker, deletes only its copied picker source, and verifies durable content-token restore and author-image reset. Both config and data directories are isolated; `HOME` remains available for Steam discovery. For a complete effect graph, repeat `--keep-scene-property NAME` for author choices that must remain enabled while other controls are edited. Inverting every boolean can disable all effects and is insufficient render acceptance. These flags do not inject IPC or bypass candidate/save gates.

Where native chooser keys do not activate GTK buttons, add `--native-dialog-accessibility`. It requires Python GI/AT-SPI and the GTK accessibility bridge. The harness automatically sets `GTK_MODULES=gail:atk-bridge`, `NO_AT_BRIDGE=0` and `LC_ALL=C.UTF-8` only in its owned child environment. `wtype` still enters the copied source path; the alternative mode invokes the real Cancel/Open button's AT-SPI action after checking the owned chooser window, PID and focus. It does not invoke business IPC or replace the native file picker.

Add `--narrow-image-layout` with `--scene-image --playback-settings` to capture the real image controls at a 380-pixel native viewport in English and Simplified Chinese. The harness switches language through Settings, checks the image control and its buttons for overflow, then restores the native window. This checks the image editor's layout, not the whole Library's narrow-window layout.

### Miku extension (2026-10-13) {#miku-extension-2026-10-13}

The shared bounded path now implements empty-default image properties, instance materials, genericimage4/sprite frame selection, the original motionblur accumulation/copy/combine graph and CRT composite alias, required SceneScript frame/audio/property forms and builtin Atami CFF text. Author-hidden layers remain prepared; explicit choices determine visibility. The default graph preserves eleven declarations, ten render layers, five effects and twenty-five passes, including four global bloom passes.

The unchanged original `3420277613` passed complete CPU preparation and a software-EGL full-graph oracle on the final production source. Both eight-frame atlases were selected and visibly rendered under controlled positive audio through the actual original VM. Three independent histories, exact paused output, a same-project candidate, frozen rollback pixels, thaw and final cleanup passed. The retained CPU charge was 255,616,293 bytes and its reservation peak was 525,312,699 bytes (500.98 MiB); the accepted old/candidate GPU union was 525,624,984 bytes (501.28 MiB). Both peaks are below their separate 536,870,912-byte limits. Freezing moved 79,833,600 bytes of private history into sealed CPU snapshots; keeping the old history on GPU would require 605,458,584 bytes and is correctly rejected. CPU charges and shared uploads returned to zero at cleanup. These production allocation/reservation counters exclude test-only readback vectors, allocator overhead, RSS and measured VRAM.

The final-source CPU checks separately passed all five original script modules and the original Atami font, including shared font ownership and bounded text rasterization. The 2.5D Space software-EGL regression also passed all twenty-six pixel comparisons. Those checks use controlled supplied spectra; they do not establish live capture or desktop acceptance.

Separate CPU preparation/worker checks passed the original Glitch OFF (`0`) and POOF (`2`) branches with Framerate enabled and the default effects retained. OFF prepares eleven layers/five effects/twenty-six passes and shows Static50; POOF prepares ten/five/twenty-five and intentionally hides the character. Hidden script-owned layers remain hidden after callbacks. This is CPU option evidence, not native pixels for those branches. The package's scheme-color control has no explicit object or script binding; its UI/persistence checks do not establish an implicit original-engine color effect.

The separate `original-miku-private-capture.json` passed on AMD Radeon/radeonsi with surfaceless EGL. Owned PCM played into a private silent Pulse sink, then entered production default capture and the accepted sixteen/thirty-two/sixty-four-band spectra. Thirteen accepted frame snapshots matched the original sixteen/sixty-four-band VM alpha responses while the complete default graph and three histories rendered. A separate causal probe kept input, geometry and time fixed: captured audio changed 1,468 pixels through the unchanged original SimpleBars shader and its thirty-two-band stereo uniforms. Pause disconnected capture and preserved the accepted pixels exactly; resume reconnected, and cleanup removed capture/shared uploads. This is real private capture plus hardware offscreen rendering, separately scoped from native desktop presentation.

The final native engine-only probe used the unchanged default graph at 1920×1080 in nested niri on AMD 680M. With a 30 FPS cap, thirty animated seconds completed 136 swaps (4.53 FPS); thirty paused seconds completed zero draw/swap/callback operations, and three resumed seconds completed thirteen swaps (4.33 FPS). Clear removed the output metrics and backend shutdown joined. All effects stayed enabled and authored resource resolutions were preserved. **This run did not reach 30 FPS and does not establish daily-use readiness.** Its 500,230,580-byte current GPU plan is a logical union; overlapping diagnostic pool/private views must not be summed. The private final manifest records shell SHA-256 `09857b830cc0d2147c0ffb7ec32aaff0cbac6928d8a094a461d4a40b3de5355d` and probe SHA-256 `42ee6e3d9e04f5f20357107e52ad7e2ff72a42c4ac0a4d84dabf1460701fe29a`.

The actual native layout check passed both languages at a 380-pixel viewport, with a 230-pixel image control and no overflow inside the control or its buttons. Language changed through the real Settings flow; source stayed unchanged during the run, no DOM styling was injected, and owned-window/workspace cleanup passed. The existing catalog-toolbar overflow remains outside this image-editor check.

On this machine's Mesa 26.2.4/llvmpipe LLVM 23.1.1, a separate tiny scalar-array/tint fixture produced incorrect pixels despite correct uniform readback. Independent pure-GL reproduction confirmed the software-driver issue; forty-two equivalent AMD Radeon/radeonsi cases and fresh-program A→B→A checks passed. Keep the failing software comparison explicit; do not weaken its pixel expectation or infer original-engine parity from a source/math check.

The final `native-full-effects-final3/report.json` passed all seventeen native steps in the same nested environment, with no mocked IPC or cleanup errors. It included all fourteen value controls, kept Glitch ON and the full-effect booleans enabled, and changed Framerate, scheme color, all six CRT sliders and the background image. Two real GTK chooser cancel/select rounds used the owned accessibility buttons. The copied picker source was deleted after import; the content token survived atomic save and restart. Permission-denied saves retained the previous session bytes/inode, editable draft and previous Scene Running state; retry succeeded. Settings changed the 15 → 30 FPS limits without replacing the application or assignment. Titlebar close hid the same process, its whole owned group joined before restart, and a new process restored the image and choices. Reset removed overrides and restored every author default; twelve seconds of owned fullscreen coverage recovered, then target clear removed the assignment. The run also passed both 380-pixel image layouts and restored the original named workspace. Fullscreen coverage does not prove callback pausing or system suspend.

Custom image contracts and remaining limits are documented in [Scene images, timed frames and history](./scene-miku-runtime.md). Physical suspend/resume is skipped by explicit user instruction. Physical multi-monitor, installed-package, Windows A/B and Miku endurance acceptance remain unverified.

### Miku performance investigation (2026-10-14) {#miku-performance-2026-10-14}

This iteration starts from merged `00a8ceed5b876139ce649ebf2d321ec218a0d296`. The 2026-10-13 source/binary identities and 4.53 FPS acceptance above remain a separate historical record. New baseline and candidate measurements identify their build profile, source/binary hashes, output and sampling conditions; debug and release measurements must not be compared as though they used the same build.

The installed Wallpaper Engine editor was inspected with inert REA JavaScript analysis and exact reads of shipped declarations, shaders and effect/material resources. No Windows application was executed; the private `editor-rea/editor-rea-findings.md` and source manifest retain the observation boundaries and exact shipped identities. Its animation declarations describe source duration, frame controls, a shared timer through material join, and zero frametime for redraws without animation. Generic image versions 1–4 address sprite regions through UV translation and two basis vectors. These are authoring/schema observations: native phase, hidden/failed-present clock advancement, instance detachment and GPU upload scheduling remain unresolved. The declarations do not expand LWE's admitted host APIs, and a preset's `nointerpolation` flag does not establish temporal interpolation or a precise sampler policy.

The shipped motionblur graph accumulates into buffer2, copies buffer2 into the unique buffer1, then combines from buffer2. Its rate is a fixed shader factor without a frame-time input. Buffer2 therefore remains live through both copy and combine, and persistent history must stay isolated by effect/generation. Static evidence does not establish native allocation or execution cadence, the exact reserved-name resolution, or permission to replace copy with a buffer swap. A zero-dt redraw alone does not prove the original effect is skipped. The editor's cancelled image chooser preserves its value and remove restores the author default/empty choice; LWE's bounded private content-token import remains a separate adaptation, without arbitrary path IPC. Its resource wrappers expose value/user/script/scriptproperties/animation fields, and audio declarations list sixteen/thirty-two/sixty-four-band registration, but neither authoring surface admits unsupported bindings or establishes Windows FFT normalization. The focused effect inventory does not admit additional FBO formats or advanced effect schemas.

LWE already reuses an unchanged validated timed-frame region/view when its source, frame and sampling identity are unchanged. That behavior is inherited, not a new optimization in this iteration, and does not skip new script packets or normal effect/history processing. The scheduling candidate keeps the FPS deadline anchored to the last successful swap and gives incomplete Scene preparation an independent 8 ms retry interval. Video retains its decoder retry policy. A further candidate derives the region key from a new decode-contract tag, the sealed source's actual encoded SHA-256, prepared namespace/provenance fingerprint, frame index and exact ROI/gutter rectangle, avoiding another scan of the decoded RGBA. Source-file validation, BC3 decoding, immutable ownership and 512 MiB CPU/GPU gates remain; stale prepared identity cannot substitute for the actual sealed-source digest. Neither candidate changes original assets, effects or resource resolutions, and the retry interval is not a promised frame rate.

Exploratory measurements remain separate from the final controlled comparison. The initial debug baseline measured 149 completed swaps in thirty seconds (about 4.97 FPS), 741 attempts and an average 5.86 ms for successful render plus swap; sampled worker activity was about 34.58% of one CPU core. An exploratory release baseline measured 176 swaps (5.87 FPS), 704 attempts and 5.44 ms successful render plus swap. The scheduling-only release candidate measured 238 swaps (7.93 FPS), 1,932 attempts and 5.56 ms, about 35.23% more completed swaps; thirty paused seconds had zero draw/swap/callback operations, and resume/clear/join passed. Its higher retry count must remain visible; these runs are not the final combined-candidate comparison.

Six CPU isolation regressions passed for the region-key change, including changed encoded pixels with a stale prepared identity, metadata-only changes, namespace/frame/ROI isolation, distinct physical owners and repeated worker epochs. A warm CPU-only benchmark exercised 32 original regions/cases: per-ROI release averages were 11.12 ms decode, 10.53 ms old RGBA key hashing and 119.75 ns new key derivation; debug averages were 26.02 ms, 10.57 ms and 172.96 ns. Its namespace identity was a surrogate, final CPU charge returned to zero, and it performed no GPU upload or desktop presentation. It neither predicts FPS nor establishes total process memory.

The combined candidate passed the controlled release A–B–B–A comparison. All four runs used the same 1920×1080 nested niri/AMD 680M setup, unchanged original defaults and private PCM, with the parent compositor window fully visible at fixed position `[96, 112]`.

| Run | Build | Completed swaps / measured animated seconds | FPS |
| --- | --- | --- | --- |
| A1 | Baseline release | 174 / 30.000054910 | 5.80 |
| B1 | Combined candidate release | 291 / 30.000036961 | 9.70 |
| B2 | Combined candidate release | 296 / 30.000055844 | 9.87 |
| A2 | Baseline release | 179 / 30.000075875 | 5.97 |

Aggregating swaps and measured time gives 5.8833 → 9.7833 FPS, approximately 66.29% more completed swaps per second. All four thirty-second paused phases had zero attempts/draws/swaps/callbacks; clear, backend shutdown/join and identity checks passed. The current logical GPU plan stayed at 500,230,580 bytes, with the original effects, histories, resource resolutions and 512 MiB gates retained. This is a finite same-profile scheduling/key result; it does not use the isolated microbenchmark or debug measurements to predict FPS.

The combined debug candidate also completed 242 swaps in thirty seconds (8.07 FPS), 1,982 attempts and 5.37 ms successful render plus swap, with zero paused activity and successful resume/clear/join. Its exploratory debug baseline was about 4.97 FPS, but the parent-window position differed; this is supporting evidence rather than the primary controlled comparison.

The final-source `final-miku-gpu-oracle.json` passed on software EGL/llvmpipe with controlled supplied spectra and the unchanged full-default graph: ten layers, five effects and twenty-five passes. Both original eight-frame sources selected frames 0–7 and rendered visibly, three histories remained independent, and pause/resize/candidate/rollback/thaw checks retained the required pixels and state. CPU reservation peak stayed at 525,312,699 bytes and the old/candidate GPU union at 525,624,984 bytes, below their separate 512 MiB limits; cleanup returned CPU/pool charges to zero and original source hashes stayed unchanged. Six authored history tests and two sharing tests separately passed on AMD. The ordinary workspace suite passed 549 tests and left thirty opt-in tests ignored; these nine explicitly executed GPU checks are separate evidence, not a claim that all ignored desktop/GPU tests ran.

Throughput remains below the 30 FPS cap and does not establish daily-use readiness or a long soak. This iteration did not repeat the native UI save/restore or installed-package flows from the earlier record, and the finite nested engine runs do not establish physical-output acceptance or Windows parity. Physical suspend/resume continues skipped by explicit user instruction.

### Miku worker notifications and codec (2026-10-14) {#miku-worker-wake-2026-10-14}

This is a separate iteration from merged baseline `9278023`, which already includes the earlier scheduling and sealed region-key work. The preceding 2026-10-13 and 2026-10-14 source identities, measurements and proof scopes remain unchanged. Here A is that baseline, C adds only the CPU codec row-copy change, and D adds renderer-owned worker notifications to C. Overall throughput is evaluated separately through the final A–D–D–A release comparison; percentages from the short stages are not added together.

The BC3 change hoists validated row geometry, iterates fixed sixteen-byte blocks and copies complete interior rows directly, retaining clipped handling at ROI/mip edges. The checked private layout, source/output length checks, endian/rounding equations, full output writes, immutable ownership and fixed 84-byte codec scratch reservation remain. Twenty nonzero-sentinel ROI cases add all start phases, multiple interior rows, one-pixel corners and exact partial right/bottom edges. The final code's original-resource DDS oracle passed all sixteen frames and sixteen neighbor regions. An isolated CPU A–C–C–A comparison averaged 11,172.47 → 8,659.33 µs per decode (−22.49%), with 32 regions per run, unchanged 89,121,153-byte production peak and zero final charge. This measures warm CPU decoding, not GPU time or predicted FPS.

Scene script/texture workers now publish their result or error before sending a coalesced event-loop hint owned by that renderer. Late notifications from another generation cannot mark the selected renderer ready. Handling a hint removes only an outstanding Scene-preparation retry delay, retains the last successful swap as the FPS deadline origin and preserves Video's decoder policy. Paused/idle outputs do not gain forced draws from a hint. The eight-millisecond polling policy remains a fallback; a completion notification does not acknowledge a first frame or bypass complete packet/draw/swap acceptance. Original effects, histories, resource resolutions and 512 MiB CPU/GPU gates are retained.

The final codec-only short stage observed 9.5657 → 10.0000 FPS (about +4.54%), with sampled texture-worker activity 21.5655% → 18.0719% of one core. The baseline ranged from 9.20 to 9.93 FPS, so the small FPS difference is not a stable throughput claim. The separate notification stage observed C 8.9333 FPS (7.93–9.93) versus D 12.2000 FPS (12.13–12.27); variation in C prevents treating its approximately +36.57% as a stable isolated notification effect. CPU samples use one-second phase-start labels that may include settling/resume boundaries, not exact per-phase profiles. All short-stage pause/clear/join and identity checks passed.

The notification stage recorded 682 selected-renderer dispatches and a mean 24.34 µs from first coalesced result publication to event-loop dispatch. This excludes decode, packet consumption, render, swap and GPU work. Older probes without that diagnostic are unmeasured, not zero.

The final same-profile release primary comparison used four thirty-second animated runs in the same 1920×1080 nested niri/AMD 680M setup, with unchanged original defaults, private PCM and verified geometry/input/binary identities:

| Run | Version | Swaps / attempts | FPS |
| --- | --- | --- | --- |
| A1 | Merged baseline | 297 / 1,480 | 9.90 |
| D1 | Codec plus notifications | 361 / 1,627 | 12.03 |
| D2 | Codec plus notifications | 367 / 1,651 | 12.23 |
| A2 | Merged baseline | 301 / 1,494 | 10.03 |

Aggregated swaps/time were A 598/60.000133134 seconds and D 728/60.000109441 seconds: 9.9666 → 12.1333 FPS (about +21.74%). Attempts rose from 49.57 to 54.63 per second. Sampled texture-worker activity fell 21.1669% → 16.8344% of one core (about −20.47%), while engine activity was 6.2985% → 6.4277% and all probe threads 29.7367% → 25.6624%. The CPU sample windows were about 62.08 seconds and include phase boundaries. Successful render-plus-swap CPU wall means were 5.2991 → 4.4352 ms, with the same 6 ms p95 histogram bucket; these are not GPU timer results. D recorded 1,350 selected-renderer dispatches averaging 25.38 µs publication-to-loop latency, with no equivalent measurement for A. All four pause/clear/resume/join, source/binary identity and cross-run consistency gates passed.

The final source also passed the full-default software-EGL oracle with both visible eight-frame sources, three private histories, exact pause and candidate rollback. CPU peak 525,312,699 bytes and old/candidate GPU union 525,624,984 bytes remain below 536,870,912 bytes, with zero final charges and unchanged input hashes. Six AMD history and two sharing tests passed separately, as did the final original-source CPU DDS oracle. The ordinary workspace suite passed 558 tests and kept thirty opt-in tests ignored; the one original CPU plus nine GPU opt-ins actually run here are distinct evidence.

The frozen candidate also passed a nested native engine-only replacement check: original Miku animated, paused for three seconds with zero attempts/draws/swaps/empty frames/callbacks/worker dispatches, then resumed. Replacing the paused source with a private static 16×16 Scene produced exactly one draw/swap; three seconds of static playback remained idle, and clear/shutdown joined without owned child processes. Binary and original project/package hashes remained unchanged, and the paused original-default image was visibly reviewed. This checks engine replacement/idle behavior, without repeating the UI save/restore flow. The iteration still does not establish 30 FPS, daily-use or long-soak readiness, new UI persistence/installed-package acceptance, physical-output behavior or Windows parity. Physical suspend/resume remains skipped by explicit instruction.

### Scene CPU preparation overlap (2026-10-14) {#scene-cpu-ahead-2026-10-14}

This separate iteration starts from merged `b092cbfc955887150bb869922eb8b6ece0553300`. Earlier Miku measurements and source identities above remain historical records. The shared runtime can prepare one next CPU epoch through the existing SceneScript worker and timed-frame/text worker while waiting for compositor callback or FPS readiness. It retains the existing packet slot, workers, immutable resource owners and 512 MiB CPU/GPU gates. CPU preparation does not install GPU views, consume history, grant presentation readiness or acknowledge a first frame; complete packet consumption/upload/render still occurs through the gated presentation path.

Each speculative packet keeps its sequence, scene time and captured PCM spectrum together. Already accepted sequence/time/audio, presented pixels and history remain unchanged during CPU-only preparation. Paused/idle or unconfigured outputs, pending apply/commit/rollback transactions, startup before the first presented frame, frozen or quarantined renderers, and frames awaiting successful swap do not admit ahead preparation. Pausing and replay requiring CPU reconstruction cancel only speculative timed work; its tagged VM packet can remain for later gated consumption. Accepted redraw/resize/rollback uses the accepted epoch before consuming a future packet. The sequence-zero clock rebase now applies only to the true first successful presentation, so resizing an already presented seq0 frame cannot restart its clock.

Timed decode ahead is enabled only when `state.sessions.len() == 1`. With multiple sessions, VM preparation may overlap but timed decode retains the original gated policy, and any speculative timed result is cancelled. A timed result can hold the global producer permit until gated consumption; this guard avoids another session starving when callbacks for the first are withheld. Cancellation retains source/capacity ownership until the existing worker/reaper finishes. This is a conservative multi-session policy, not physical multi-monitor acceptance or a general timed-ahead throughput claim for multiple outputs. Video's decoder and presentation policy remain.

The final A–B–B–A release comparison used the retained #109 baseline A binary (`25e1ca224b59bed394a6a88373bd806cada187189627f8d7be49dfb98de4c172`; its twelve runtime source identities match the merged baseline) and candidate B (`39ed54ca0503401503c350e93ef16aea79922870e9428bf16da3595be97c5615`). Each run used 1920×1080 nested `niri` on AMD680M, the original full defaults/effects/history/resource resolutions, a 30 FPS cap, owned private PCM and 2 ms status-query stress, with private profiling disabled. Each had thirty seconds animated, thirty seconds paused and three seconds resumed. These are two short runs per variant, not a significance or long-soak result; percentages are not added to earlier iterations.

| Variant | Per-run FPS range | Successful swaps / total animated seconds | Aggregate FPS |
| --- | ---: | ---: | ---: |
| A: merged #109 baseline | 11.4333–11.9333 | 701 / 60.000116463 | 11.6833106554 |
| B: CPU preparation overlap | 14.3000–15.1666 | 884 / 60.000078995 | 14.7333139357 |

The directly observed throughput change was **+26.1056%**. Empty render attempts fell from 2,527 to zero; all six paused activity counters stayed zero, and resume/clear/join/source-identity gates passed. Sampled texture-worker CPU increased from 17.1868% to 25.8182% of one core with higher throughput; this is not a CPU-use reduction. The approximately 62.08-second labelled CPU windows can include phase boundaries, so they are not exact thirty-second phase profiles. Successful render+swap CPU wall time was 4.8431 → 4.8057 ms; it excludes ahead-worker waiting and is not GPU elapsed time.

All Rust workspace checks passed, with **559 ordinary tests passed (including two doctests), 36 ignored**, and fourteen separately executed opt-in AMD authored offscreen checks: six new ahead regressions, six history and two sharing checks. The new regressions cover no GPU/accepted-state mutation during ahead work, tagged sequence/time/audio, gated installation, pause/resize/resume replay, frozen rollback, topology cancellation/global permit release, and refusal during startup/unacknowledged swap or shader-only animation. Ignored tests are not counted as passes. Full original Miku was tested separately in software EGL: ten layers, five effects, twenty-five passes, both timed textures selecting all eight frames, three histories, exact paused/frozen-rollback replay and candidate/resize/thaw cleanup. All eight output pixel hashes and all twenty-four history pixel hashes matched the merged #109 software oracle. CPU peak 525,312,699 B and GPU old/candidate union 525,624,984 B stayed below 536,870,912 B (512 MiB), with CPU/pool cleanup zero. This full software result is distinct from the small AMD offscreen fixtures and the native throughput measurement; pixel equality is not a native or Windows comparison.

A separate native engine-only check passed Miku pause/resume, replacing the paused source with a private 16×16 static image using exactly one draw/swap, three seconds of idle static playback, clear/join and owned-child exit. The 1920×1080 original Miku/CRT capture was visually reviewed. Owned `niri`/audio children exited and host audio defaults/focus were restored. This does not revalidate UI persistence or an installed package. No 30 FPS, daily-use, physical-output, Windows parity or long-soak claim is made; physical suspend/resume remains explicitly skipped.

The next step is controlled per-pass GPU attribution from this 14.73 FPS full-default baseline: rotate sparse samples across the twenty-five passes within existing query/memory budgets, associate layer/stage/pass with owner+sequence+time, and identify CRT, motion-blur or composite hotspots before using REA original shader/material/editor evidence to optimize the shared execution path. This sampling and attribution are planned. A prior private `85cc` diagnostic had ten sparse matched-tail samples averaging about 61.32 ms for aggregate draw/composite GPU elapsed and 6.34 ms for upload. Those samples are a lead for further investigation, not GPU timing for candidate `39ed`, an exact steady phase or an equivalent workload comparison. CPU/GPU clocks and these averages must not be combined or used to predict FPS.

### Scene GPU pass investigation (2026-10-14) {#scene-gpu-passes-2026-10-14}

This independent iteration starts from merged `4cb9fe14dcea8534a84f4198f3871af56c0a7139`; earlier measurements and source identities remain historical records. Temporary AMD GPU queries completed a serialized A-off/P-on/P-on/A-off diagnostic at 1920×1080, original defaults, private PCM and a 30 FPS cap. A is the retained production `39ed` release; P is the temporary instrumented `3c135` release. Aggregate throughput was 15.066639 → 14.849973 FPS (observed −1.4381%). Different binaries include hook and sampling overhead, so this is not an optimization result or a same-binary, zero-overhead claim. The diagnostic hooks are private and are not part of the production change.

The catalog contains forty-six sites: twenty-one stage sites (eighteen renders and three history copies), four bloom sites, and separately identified upload/composite/copy/canvas/present intervals. Those twenty-one plus four describe the original twenty-five logical passes; auxiliary intervals do not change that graph. Forty sites had samples in both animated CPU-submission inner windows; missing conditional/hidden sites are not imputed as zero. Each fourth sequence samples one rotated site plus the upload batch, with layer/stage/pass associated with owner, sequence and time. Thirty-two queries were created and deleted, with eight slots and a fixed 512-record limit. Owner identity is process-local and is namespaced across runs.

| Diagnostic interval (zero-based indices) | Samples | Mean GPU interval |
| --- | ---: | ---: |
| Layer 8 material/composite into scene | 4 | 4.983690 ms |
| Layer 2 / stage 1 / pass 0: original Simple Audio Bars | 4 | 4.606180 ms |
| Three history-copy sites | 6 / 6 / 5 | 1.993293 / 1.401640 / 1.219672 ms |
| Bloom combine | 4 | 0.674490 ms |

The Layer 8 interval covers its material/composite, not its separate flat shader. Copies are GPU blit/driver intervals, not steady CPU readback. All four bloom passes and their original dimensions/effects remain; these sparse results do not justify removing passes or reducing resolution. The 189 upload-batch samples mix acquisition, text, cached/no-op and actual uploads, rather than 189 pure atlas transfers. Most sites have only four to six samples from different frames, PCM and atlas states. GL completion intervals include submission/driver gaps, not pure shader busy time; unrelated means are not summed into a frame critical path or an FPS prediction. CPU begin timestamps only classify a guarded inner window, and query-availability polling is not GPU-end time. No exact GPU phase or CPU/GPU clock subtraction is claimed.

Two scalar-material attempts remain rejected research. The broader positive-integer source failed the strict AMD Count=3/frame0 comparison by one pixel LSB; that failure was retained. The final power-of-two source (`0e0b`) passed its recorded correctness checks but measured 15.132597 → 14.833289 FPS (observed −1.977906%), so it was archived and restored for no performance benefit. Neither is delivered, and their evidence is separate from the Blend implementation below.

The implemented shared compiler path specializes only complete canonical LWE-owned image/text shader templates, with exact source and metadata matching. At program compilation it replaces the fixed integer `g_BlendMode` 0/1/2 (Replace/Alpha/Add) with a typed GLSL constant for that renderer generation. Other templates or modes retain the original uniform path. Authored artwork/effect formulas and integer parameters, ColorMode, prepared source/plan, resources/copies, effects/resolutions, runtime inputs, workers/slot and 512 MiB budgets remain unchanged. This does not apply general scalar or artist-shader substitution.

The independent final release A–B–B–A compared retained #110 A (`39ed54ca0503401503c350e93ef16aea79922870e9428bf16da3595be97c5615`; twelve recorded source inventory identities match the merged baseline) with Blend B (`0c893ccc5b9d9b713a14a6b817ee750d9a1ea16720e6834aeaff29c292c059fe`). Both used full original defaults/resources/effects, the saved 1920×1080 nested `niri`/AMD680M environment, private PCM, a 30 FPS cap and 2 ms status-query stress, with private profiling disabled. Each run had thirty seconds animated, thirty paused and three resumed.

| Variant | Per-run FPS range | Successful swaps / total animated seconds | Aggregate FPS |
| --- | ---: | ---: | ---: |
| A: merged #110 baseline | 14.7666–14.8000 | 887 / 60.000109578 | 14.7833063346 |
| B: canonical Blend template | 15.1666–15.5647 | 922 / 60.003809558 | 15.3656910585 |

Observed throughput increased **3.9395%**. Each variant has only two short runs, and private tone/silence epochs were not phase synchronized; this is not a stable, general or statistically significant gain. Percentages are not added to prior iterations. Both variants had zero animated/resumed empty render attempts, thirty-second paused counters/worker dispatch/render statistics stayed zero, and resume/clear/join/source/input-identity and owned probe/player-exit gates passed. Successful render+swap CPU wall mean was 3.956879 → 3.835216 ms, excluding worker waiting and asynchronous GPU time. Labelled CPU samples can cross phase boundaries; neither those means nor the older private GPU-site means are combined into a frame time or FPS prediction.

The final Blend source passed Rust workspace checks: **562 ordinary tests (including two doctests), 37 ignored**, plus sixteen separately executed opt-in GPU tests: one new authored AMD template test with twelve internal cases, six ahead, six history, two sharing and one full-original software test. Ignored tests are not passes. The small template fixture verifies exact RGBA and sampler liveness; it does not replace the full Miku software EGL oracle: ten layers, five effects, twenty-five passes, both eight-frame atlases visible, three histories and exact paused/frozen-rollback replay. All eight output and twenty-four history pixel hashes match #110. CPU peak 525,312,699 B and GPU transition union 525,624,984 B remain below 536,870,912 B (512 MiB), with CPU/pool cleanup zero. Ledger values are not allocator/RSS or driver VRAM, and overlapping native GPU fields are not summed. Full software, small AMD and native throughput results remain separate proof scopes.

A separate native engine-only check passed Miku pause/resume, replacing a paused source with static content (exactly one draw/swap), three seconds of static idle, clear/join/exit and a 1920×1080 capture; all thirty-one functional and fifteen cleanup checks passed. Owned helper processes/windows exited, final raw host-audio-default snapshots matched before/after, and the original focus was restored. These snapshots do not imply continuous per-run observation or UI/session-save acceptance.

Next, assess reusing existing Named/History source FBOs for history copies, retaining the current fallback for Input sources, ownership and accepted replay/rollback behavior. Full-frame blits already use existing FBOs; assess their input liveness separately. Layer 8 ColorMode 28 and original Simple Audio Bars remain investigation targets; the authored formulas and full effects/resolutions remain. This plan is not completed copy elimination or a further gain. This batch does not establish 30 FPS/daily use, physical output, new UI/atomic-save or package acceptance, Windows parity or long-soak behavior. Physical suspend/resume remains skipped.

### Scene history-copy FBO reuse (2026-10-14) {#scene-history-fbo-2026-10-14}

This independent iteration starts from merged #111, `5df7317b9cbe85608cc848b811c74a217d042b1a`. Earlier measurements, source identities and rejected candidates remain historical records. The implementation changes only Named/History copy sources to borrow the existing `RenderTarget.fbo` as the READ framebuffer; construction has already validated its immutable texture attachment. Input, alias-preservation and other texture-only sources retain the temporary create/attach/delete path. Existing full-frame blits already reused their target FBOs and are not a new optimization here.

The copy itself remains `BlitFramebuffer` with the same bounds, `COLOR_BUFFER_BIT`, `NEAREST` and pass ordering. No copy/pass is removed, fused or delayed. Resources, dimensions/resolutions, generation ownership, accepted history/replay/rollback, workers/slot and 512 MiB accounting remain. Feedback or dimension disagreement is rejected before either source path changes GL bindings. Restored history targets must retain their validated attachment and lifetime; this is source-FBO reuse, not texture/history sharing across generations or CPU readback removal.

The final source passed Rust workspace checks: **562 ordinary tests (including two doctests), 40 ignored**, plus nineteen separately executed opt-in GPU tests: three new copy, one owned Blend template, six ahead, six history, two sharing and one full-original software oracle. Ignored tests are not passes. The three new regressions verify exact RGBA agreement between temporary/reused source FBOs and restored snapshots, advancing Named/History copies with paused resize/freeze-thaw/resume, and feedback/dimension rejection on both paths before GL mutation. Full original Miku software EGL separately retains ten layers/five effects/twenty-five passes, both visible eight-frame atlases, three histories and exact paused/frozen-rollback replay. All eight output and twenty-four history pixel hashes strictly match #111/#110. CPU peak 525,312,699 B and GPU transition union 525,624,984 B remain below 536,870,912 B (512 MiB), ending with CPU/pool zero. Full software, small AMD fixtures and native throughput remain separate scopes, not Windows or native pixel-equivalence proof.

The final release A–B–B–A compared retained #111 A (`0c893ccc5b9d9b713a14a6b817ee750d9a1ea16720e6834aeaff29c292c059fe`; fifteen recorded source inventory identities including probe/tests match the merged baseline, not fifteen executed runtime modules or a rebuild) with B (`7114db88cc33f4f409b67d7169328ba4e2ae1d2cb709bf4755a2f482d87e4cac`). Both used the full original defaults/effects/resources, private PCM, the 1920×1080 nested `niri`/AMD680M environment, a 30 FPS cap and 2 ms status-query stress, with private profiling disabled. Each run had thirty seconds animated, thirty paused and three resumed.

| Variant | Per-run FPS range | Successful swaps / total animated seconds | Aggregate FPS |
| --- | ---: | ---: | ---: |
| A: merged #111 baseline | 14.6000–14.8333 | 883 / 60.000098472 | 14.7166425137 |
| B: Named/History READ FBO reuse | 15.1666–15.3000 | 914 / 60.000132644 | 15.2332996566 |

Observed throughput increased **3.5107%**, with both B runs above both A runs. Two short runs per variant and unsynchronized private tone/silence epochs do not establish a stable, general or statistically significant gain; percentages are not added to earlier iterations. Animated/resumed empty render attempts were zero, thirty-second paused render/draw/swap/empty/callback counters, dispatch and CPU-render statistics stayed zero, and resume/clear/join/identity/owned probe-player exit gates passed. Successful render+swap CPU wall mean was 3.966114 → 3.823484 ms, excluding ahead-worker waiting and asynchronous GPU time; it does not prove GPU elapsed decreased. Labelled CPU samples can straddle boundaries and are not combined with older GPU-site means to predict FPS. Each run directly records its 1920×1080 output; host geometry is a mid-B1 snapshot, not a batch-start or continuous per-run visibility/host-audio observation. Ledger values are not allocator/RSS or driver VRAM, and overlapping fields are not summed.

A separate native engine-only check passed original Miku pause/resume, paused static-source replacement (exactly one draw/swap), three seconds of static idle, clear/join/exit: all thirty-one functional and fifteen cleanup checks passed, with a separately reviewed 1920×1080 capture. Three owned helper PIDs/window exited, both supervisor sessions returned zero, raw host-audio-default snapshots matched before/after, and original focus was restored. This does not extend to continuous per-run observation or UI/session-save acceptance.

REA independently inspected the original x86-64 PE `wallpaper64.exe` (SHA-256 `bced836b83eaa078d212eb4781e4e7c4fc89ab04f84d6e85ac3c1ee6081c3e42`) without execution, retaining thirty-one Evidence records. `FUN_1401598b0` distinguishes an absent target handle from an existing target's resize path; `FUN_14017ab60` hashes/compares named targets and returns the saved pointer; the `FUN_1401abd20` image-layer path looks up, tests and skips creation on a hit. `FUN_14013f7b0` also skips unchanged dimensions during resize. These observations support investigating persistent generic render-target reuse, with LWE's implementation and benefit requiring separate validation.

The original PE calls D3D11 device creation in `FUN_14010e2c0`, so this static cache evidence is not proof of OpenGL FBO reuse or original History API/view lifetimes, copy fusion/delay/removal, GPU cost or FPS. Original History dispatch/copy API, Windows execution, device-loss and long-duration behavior remain unknown; a missing History string does not settle its implementation. No original decompiled source is copied into the repository. Next, continue bounded per-pass attribution for Layer 8 ColorMode 28 and original Simple Audio Bars, associating owner/sequence/time and evaluating the shared execution path while preserving authored formulas, full effects/resolutions, resources and budgets. No further gain is assumed. This batch does not establish 30 FPS/daily use, physical output, new UI/session-save or package acceptance, Windows parity or long-soak behavior. Physical suspend/resume remains skipped.

### Scene draw attribution and FullFrame source reuse (2026-10-15) {#scene-material-bind-2026-10-15}

This separate investigation starts from merged #112, `71b69f5497929b70d013694f6315d5020049dd41`. The preceding dated records and rejected candidates remain unchanged. REA retained sixty Evidence records for the original `wallpaper64.exe` PE identity above: thirty-one reused and twenty-nine new, with twenty-three instruction anchors. No Wallpaper Engine/Wine execution occurred. In the observed material path, a dirty-bit test skips the static-value update when clear and clears the bit after updating; the concrete shader vtable separates that update from dynamic binding. The metadata-driven `FUN_1400c57e0` packer copies 128 bytes for each 32-band channel, with separate 64/256-byte cases for 16/64 bands. This is static CPU packing evidence, not observed Windows active ranges, per-frame driver calls, GPU timing or a claim that LWE uploads oversized audio. Coverage of every original mutation path and exact backend API names remains unproven. The shipped ColorMode 28 formula is preserved; no exact arithmetic replacement was established. The REA session was closed; decompiled source is not imported into the repository.

A private two-site probe then recorded actual LWE linked programs and separate CPU/GPU intervals on AMD Radeon 680M (`radeonsi`, Rembrandt), desktop GL 4.6 with 64-bit timestamps. A is the retained #112 release `7114db88cc33f4f409b67d7169328ba4e2ae1d2cb709bf4755a2f482d87e4cac`; P is the temporary instrumented release `8616d101e7b6cca6b210f29ee0a868764fb9403458b1380c20fb47e3887ac8a5`. Its frozen renderer/module/helper source snapshots have SHA prefixes `ea88e30e` / `bcea691f` / `223523b2`; the full identities are retained in the private build manifest. The diagnostic sources were archived/restored and are not committed as a product feature.

The serialized A-off/P-on/P-on/A-off runs retained full original defaults, private PCM, native 1920×1080 output, a 30 FPS cap and 2 ms status-query stress. P rotates one site every fourth normal sequence between Layer 2/stage 1/pass 0 (Simple Audio Bars) and Layer 8's composite; both actual offscreen targets are **3840×2160**, not the native output dimensions. Each context allocated/deleted all thirty-two queries with eight slots, a 512-record cap and zero final pending queries. The image ledger retained its 512 MiB limit; query objects and private diagnostic bookkeeping are separate from measured image bytes.

P1/P2 emitted 260 presented records. A guarded animated CPU-submission window retained 210: 104 Audio Bars and 106 composite samples; forty-three startup/settle/boundary records and seven resumed records are separate. Selection uses CPU begin/clear/draw anchors in `[observed begin + 1 s, min(observed end − 1 s, begin + actual probe duration − 1 s)]`. Asynchronous query-result polling may happen after pause/resume, so result-retrieval time does not classify the submitted work or denote GPU completion. Owner/sequence/time bits and rotation are checked within each process/context; identically named owners are not merged across processes. Replay and sequence-zero exclusion follow the frozen producer contract, with one warmup exclusion per P run.

| Animated submission-interior interval | Samples | Mean | Median | Range |
| --- | ---: | ---: | ---: | ---: |
| Audio Bars DrawArrays-only completion | 104 | 4.579791 ms | 4.613920 ms | 4.301520–5.352960 ms |
| Layer 8 DrawArrays-only completion | 106 | 5.159573 ms | 5.162860 ms | 4.895080–5.395960 ms |
| Audio Bars Clear-only completion | 104 | 13.096538 µs | 5.100000 µs | 3.480000–807.920000 µs |
| Audio Bars CPU spectrum loop | 104 | 0.706683 µs | 0.666000 µs | 0.521000–1.343000 µs |
| Audio Bars other CPU binding spans combined | 104 | 3.756567 µs | 3.410500 µs | 2.463000–20.210000 µs |
| Layer 8 other CPU binding spans combined | 106 | 1.242396 µs | 1.252500 µs | 0.431000–2.334000 µs |

The linked Audio Bars program has two active 32-float spectrum arrays, left/right: two active API calls, 64 floats, **256 bytes** per sampled binding. The loop still attempts six calls/224 floats; the 16/64-band locations are −1 and those calls are ignored, not uploads. The composite has active matrix, two texture and color uniforms but no spectrum-binding loop. Clear and spectrum timings are inapplicable there, not zero-valued samples. Active API payload is not PCIe traffic or measured GPU consumption. Clear's 807.92 µs outlier remains; no trimming creates a steady-state estimate. GPU intervals delimit completion around the individual GL calls, not pure shader busy time. Other-binding CPU spans are not a whole-site critical path, and alternating sites belong to different frames: their means cannot be added or used to predict FPS. CPU/GPU clocks are never subtracted.

The instrumentation control measured 14.799973 → 14.949973 aggregate FPS (observed +1.0135%). It compares different baseline/private binaries including hooks and sampling, not a same-binary off/on test, a production optimization or proof of negligible overhead. There are only two thirty-second runs per variant. A1 recorded two animated empty attempts (444 attempts = 442 draws + 2 empty), while presentation/callback/bucket accounting remained consistent; this is not an all-runs-zero-empty claim. P2 discarded one uncollected sample at cleanup (133 selected, 132 emitted), with its missing duration not imputed and all thirty-two queries deleted. Ring-full, QBO, clock, incomplete/unpresented and overwritten-record counts were zero; this is not an all-runs-no-drop claim. Pause counters/dispatch, resume, clear/join, original-input identities and recorded owned probe/player exits were checked separately.

A separate three-second baseline shader diagnostic first passed runtime with `AMD_DEBUG=ps` but captured no NIR/ISA. A corrected child-only request used `AMD_DEBUG=ps,nir,asm,stats` and `MESA_SHADER_CACHE_DISABLE=true`, without changing host driver configuration. The retained request manifest records those flags; the harness's environment subset does not independently repeat them. The subsequent raw capture directly contains 213,119 bytes of NIR/ACO ISA (SHA-256 `0da29f0215c89c14394ec4a0518ecb64a2659180023eeef1d8069310ca5e76d5`), from the unchanged A binary. The flag investigation records independent stage/output gates and the reworked flags noted in [Mesa 23.1 release notes](https://docs.mesa3d.org/relnotes/23.1.0.html); the inspected upstream source is a structural reference, not an exact local-package source identity. Compilation/cache perturbation excludes this capture from throughput measurements.

Dump names `GLSL15`/`GLSL60`, matching program numbers, unique linked interfaces and Audio32/Color data flow corroborate the two profiled sites. Capture and P1/P2 are different processes/binaries, however, and no same-process expanded-source-SHA/link-attachment marker was emitted: this is not cryptographic GLSL-to-NIR identity. Exact `source_blake3` matching joins NIR to ISA **within the dump only**, not to the other process, and no attribution relies on compilation order.

Audio NIR already uses four lane-varying `load_ubo` operations in compact byte ranges 0–127/128–255; ISA has four `buffer_load_dword ... offen` operations with the right channel at offset 128. There is no 32/64-element comparison/select chain for the array lookup; unrelated bar-shape selects remain. This is the driver's internal default UBO, not an application-declared std140 block. Color NIR already eliminates background hue/saturation: background lightness uses min/max/average, while source hue/saturation, final HSL-to-RGB, opacity interpolation and background alpha remain. Two texture samples are still live, so this does not prove the background sample or its required contents can be removed.

| Final ISA resource field | Audio | Color composite |
| --- | ---: | ---: |
| VGPR / SGPR | 32 / 108 | 24 / 108 |
| Code bytes | 856 | 948 |
| Scratch bytes/wave / SGPR spill / VGPR spill | 0 / 0 / 0 | 0 / 0 / 0 |
| Max Waves | 16 | 16 |

These are final ISA fields; earlier `Shader Stats` code sizes 836/928 B have a different dump label. Max Waves is compiler capacity, not measured active occupancy. Neither static loads nor zero spills establish cache misses, per-instruction cost or a bottleneck fraction. The evidence does not justify migrating uniforms to an explicit UBO or removing an already-eliminated background HSL calculation; original formulas remain unchanged.

A separate production-path material candidate was tested and **rejected for no demonstrated throughput benefit**. It attempted to upload non-`g_` material constants once at each owning pass/program's first successful bind, keeping `g_` overrides in their original per-bind position and leaving time/audio/matrix inputs dynamic. The archived renderer snapshot is `fc1c45ee9c3c7882d241d75f7fb0ffdef9966fdab1ba5d1dae42d5684e30d875`; candidate release `439fb7ce7dcff24a064851d493b42b7b9eeb51e0aa6cd1dc1d0f98d3dbcfd8af` has no private profiler. Its seventeen-file frozen inventory includes probe/tests, not seventeen executed runtime modules. The renderer and two new tests were archived/restored; this material-upload optimization is not delivered.

Recorded correctness checks belong to that rejected candidate: **562 ordinary tests including two doctests, 42 ignored**, plus twenty-one separately executed GPU tests (two new material tests, three copy, one owned template, six ahead, six history, two sharing and one full-original software oracle). The new tests covered hidden first binding, overrides, frozen/thaw pixels and retry after a failed initial upload before marking constants ready. Eight output/twenty-four history hashes and whole-frame metadata matched the baseline exactly; original 10/5/25, both eight-frame atlases, three histories and paused/frozen replay remained. CPU peak 525,312,699 B, GPU transition union 525,624,984 B and zero final CPU/pool accounting were unchanged. These software/small-AMD checks do not turn the rejected source into a shipped or native-pixel-parity result.

The separate, profiling-disabled release A–B–B–A comparison retained full original defaults/effects/resources, native 1920×1080, private PCM, a 30 FPS cap and 2 ms status traffic, with thirty seconds animated, thirty paused and three resumed per run. Its recorded aggregates are:

| Material candidate comparison | Per-run FPS range | Aggregate FPS |
| --- | ---: | ---: |
| A: retained #112 | 15.1333–15.4000 | 15.2666388412 |
| B: rejected first-bind material constants | 15.2333–15.3666 | 15.2999669087 |

The observed **+0.2183%** lies within overlapping run ranges and a time trend. It does not demonstrate a useful or stable gain, is not added to previous percentages, and did not justify retaining the change. These `material-runs` are separate from the private attribution A/P control above; correctness passes and fewer uniform calls alone are not performance acceptance.

A separate FullFrame source-reuse candidate changes only the pre-layer snapshot decision. A staged, non-direct layer without an authored composite may sample the current composition texture when `work_offset >= 2` keeps every scratch/named target away from both full-frame slots. All such reads finish before the layer's final copy compositor writes the composition. Other cases retain the snapshot path, including direct/partial-work and authored-composite cases. This is a generic liveness condition, not a Miku/layer-ID exception; shader arithmetic, prepared graph, source resolution, resource allocation and budget limits are unchanged.

The retained candidate release is `73aa35ef86820048c0029b5c060a5b4aedc7d713ca73f3dc3fde353fa4688a30`, with renderer `50fef0b3edcd8229992ef01dabbe823cab9b93216ca3385cb60212fc055af6b1` and final test source `a3e2e283b49cc0b236a9a0e1619651c82ab0c5d506972effef98c45530a91db5`; its seventeen-file inventory includes probe/tests. The initial source passed 562 ordinary/42 ignored and twenty-one explicit GPU tests. The final focused two-test rerun additionally poisons the snapshot slot and proves it stays untouched on the fast path, alongside strict RGBA, history/discard/alpha/thaw, replay and fallback coverage. Those two reruns are not two additional unique GPU tests. Final engine clippy/fmt passed. The full-original software-EGL oracle JSON SHA-256 `917825fff4d4c0536a4c53c5929838a15893bc45a2b6cc0d8ae15a62e271ceb1` is identical to #112, including eight outputs, twenty-four histories, metadata/graph, CPU peak 525,312,699 B, GPU union 525,624,984 B and zero final accounting.

A separate full-original AMD offscreen comparison rebuilt the #112 baseline and then restored/tested the frozen candidate source. Both used the same fixed test spectrum, 2160×1440 output and original 4K composition/history targets. All eight output hashes, twenty-four history hashes, frame metadata, graph, replay and resource/cleanup checks match; both report files have SHA-256 `3c1c4fb2a5af42ae4e2dbdbc82fe43334e300b6c363559b61e2a5975b352cb04`. The production source was restored and verified. These are two executions of the same original-scene test, not two additional unique regression tests; they establish hardware offscreen equality with #112, separately from live-PCM native presentation and Windows parity.

Its separate profiling-disabled native release A–B–B–A comparison retained the full original default scene, private PCM, 1920×1080 output, 30 FPS cap and 2 ms status traffic. Each run measured thirty seconds animated, thirty paused and three resumed; the duration-weighted animated results are:

| FullFrame comparison | Swaps / measured seconds | Per-run FPS range | Aggregate FPS |
| --- | ---: | ---: | ---: |
| A: retained #112 | 883 / 60.000115738 | 14.566637–14.866640 | 14.7166382788 |
| B: FullFrame source reuse | 982 / 60.000110696 | 15.933304–16.799968 | 16.3666364713 |

The observed **+11.2118%** and both B runs exceeding both A runs justify retaining this change for this batch. It removes the pre-layer full-frame helper snapshot for the two eligible original Miku layers (2/9), preserving all twenty-five logical passes; it does not alter Audio/Color formulas. Successful render+swap **CPU wall** mean changed from 3.969689 to 3.525623 ms, not GPU elapsed time. Animated/resumed empty counts were zero; every paused attempt/draw/swap/empty/callback counter, worker dispatch and render CPU total remained zero. Resume, clear/join, owned probe/player exits, original-input and binary identities, private environment and 512 MiB gates passed. Logical image accounting is not RSS/VRAM, and overlapping ledger fields are not summed.

Independent native engine checks used the same frozen B binary and all seventeen recorded source identities: original Miku animated for three seconds, paused for three with zero counters, then resumed for three. Replacing it while paused with a private static image caused exactly one draw/one swap; three seconds of static playing then kept all frame counters/dispatch zero. Clear, backend join and owned probe/player exits passed, with original inputs and binary unchanged. The reviewer inspected the 1920×1080 paused capture with default Miku/CRT intact; this is one visible native frame, not Windows pixel parity or continuous visibility proof.

Cleanup verified exact owned commands before stopping only this batch's nested compositor/private audio. Both supervisors exited zero; the three owned helper PIDs and window were gone, host audio default sink/source raw before/after values matched, and the original app focus was restored. These before/after readbacks and the single inter-batch host snapshot do not establish continuous visibility or per-run host-audio observations.

Two thirty-second runs per variant with unsynchronized twelve-second PCM epochs do not establish statistical significance, stable daily-use or general performance gains. This percentage is not added to earlier batches; rotating diagnostic site means are not summed to explain it. The next investigation should first verify available hardware-counter capabilities and controlled instrumentation overhead, then distinguish sampling/writeback traffic from arithmetic cost before choosing another production change. Existing evidence does not reopen the presumed array-select/UBO or already-eliminated background-HSL explanations.

This batch does not establish Windows runtime behavior, 30 FPS/daily use, physical multi-output, new UI/atomic-save or package acceptance, or endurance. Physical suspend/resume remains skipped.

### Recorded experimental Scene acceptance

Development acceptance on 2026-10-09 used Wayland with `niri`, AMD Radeon 680M (`1002:1681`, `amdgpu`), and one `eDP-1` output at 2160×1440 with scale 1.25. Scene sample `3337606455` uses the default zero-X/Y camera, one image layer, and Shine: five effect passes, six total passes. A full-resolution capture confirmed visible Shine rendering.

The final compatibility gate admitted only that Scene sample from the local catalog. `desktop_apply_flow_scene_video_switch_clear_and_restore` passed three Scene ↔ Video/apply/clear cycles, isolated session-save failure rollback, cancellation after commit processing, and saved Scene restore after the backend was shut down and joined. The existing three-cycle video reapply test also passed. Tests used isolated `XDG_CONFIG_HOME`; the original workspace presentation was restored and the user's original config was not changed.

At this 2026-10-09 acceptance point, `3258032485` was blocked as `unsupported_camera` because its non-zero camera X/Y semantics were unconfirmed. Earlier captures were research and did not establish support. This historical record covers one accepted Scene sample on one monitor, not complete Scene compatibility; independent multi-monitor behavior was not tested. The Scene implementation is independent Rust code without an Open Wallpaper Engine runtime dependency.

### 2026-10-10 development record

The new limited camera branch is based on REA evidence from the installed original `wallpaper64.exe`: its orthographic loader resets saved eye/center/up when the active camera-path vector is empty, and the runtime camera update consumes those reset values. LWE admits only explicit fixed dimensions, parallel forward-facing saved vectors, default up, disabled automatic projection, and absent/empty paths; other camera modes remain blocked. This supports the restricted implementation used by `3258032485`, not a claim of original-engine visual equivalence. Original visual A/B has not been executed.

| Area | Implemented evidence/tooling | Acceptance status for this build |
| --- | --- | --- |
| P0 inventory | Bounded declarations/framing API, same-descriptor PKG fingerprint, privacy and pre-loop aggregate budget regressions | Final `scene-inventory-p0.json`: 11 projects, 2,695,798 bytes, all 11 PKG SHA-256 values match the baseline. Reachable declarations include 31 JS locations in five projects, 20 puppet locations in five, and hierarchy in three. This does not assert runtime eligibility. |
| P1 runtime | Per-output deadlines, settled static/paused idle behavior, counters, 512 MiB logical RGBA/FBO accounting including retained rollback, one/two scratch targets; EGL lifetime cleanup and single initial video load | `scene-probe-shine.json` passed the 30 FPS completed-swap cap under frequent status requests and separate 60-second paused/static idle windows. Resource follow-up is recorded below. Settings FPS/battery persistence/live integration, the separate 15 → 30 FPS desktop-service measurement, and the finite native Settings/restart flow passed. Display timestamps, GPU p95, physical battery/AC transitions, VRAM/power/endurance remain unverified. |
| P0 real UI | Native pointer monitor selection → Apply → Desktop → titlebar close/hide → owned-process restart/restore → target Clear; isolated session and unobstructed captures | `3337606455`, `3258032485` and the original-default `3659880761` passed native-pointer apply/hide/new-process restore/clear and capture review. Earlier reports retained the then-blocked 365 explanation. The default 365 run also passed Settings 15 → 30 FPS/restart preferences; the separate property/failure/reset/fullscreen flow passed as recorded below. Cleanup and original-workspace restoration passed. |
| P2 camera | Authored positive/blocked fixtures and original-binary control-flow/constants evidence | Original-engine visual A/B and daily-use parity are unverified. |
| P2 properties | Exact bool/slider/color binding defaults and validated per-project/output edits; atomic assignment/value persistence with rollback | Unknown expressions/types/shapes or invalid declarations remain blocked. Native bool/slider edit, real write-failure rollback/retry, atomic inode replacement, new-process control restore and full-default reset passed. CPU/persistence tests remain distinct from UI evidence. |

The preceding development snapshot passed 18 resource-crate tests and clippy; the independent privacy/budget/fingerprint review had no remaining blockers. Rust fmt, workspace clippy, check, and tests passed. Engine tests passed 49 with one GPU fixture ignored in the ordinary run. That fixture was then explicitly run under surfaceless EGL and passed; the ordinary ignored result is not its GPU evidence. The final inventory confirms shared pre-loop budgets of 16,384 mips and 16,384 animation frames, and checks the indexed descriptor stamp after hashing. After the CSS fix, frontend type checking reported zero errors/warnings, all 113 tests across 25 files passed, and the production build passed. The Python suite passed 88 tests, including five UI-harness safety regressions. These counts belong to that snapshot; the subsequent settings/property/resource changes need their own final checks. Documentation build results are recorded separately. The 100-cycle result is recorded below.

The 2026-10-10 `3337606455` UI run passed on the recorded Wayland + `niri`, AMD 680M single `eDP-1` machine (2160×1440, scale 1.25). Native pointer selection verified Library identity `d6d309c547f3c764` and monitor identity `niri:BOE:0x0893:eDP-1` (backend `eDP-1`, UI label `BOE 0x0893`). Apply persisted the assignment; the matching Desktop card reported Running. Normal titlebar Close hid the window while retaining the same application PID, wallpaper, and session. The test then terminated its owned process group, launched a new application process, confirmed saved restore, and cleared the target. `3659880761` remained browsable with Apply disabled and its property-binding explanation visible. Applied/restored/cleared background captures were reviewed as focused and unobstructed.

Native pointer testing exposed a CSS scope defect: `body > div` applied application-root sizing to the monitor menu's portal wrapper and placed the menu outside the viewport. The application root now has ID `lwe-app`, and those styles target `body > #lwe-app`; native pointer selection passed after the fix. The acceptance uses native WebDriver clicks, without synthetic DOM input.

Final timed captures of both Shine and Shake showed pixel changes in the background region outside static desktop components (ROI x=100–1700, y=400–1400). Both UI reports completed visual review, with separate visual-review and animation-difference records. This shows changing pixels, not original-engine visual parity. Normal tray Show/Quit was not tested; owned-process termination is the test's restart step. The final `3258032485` UI rerun also passed: Library identity `18bab41e6ab0c721`, the same native monitor selection, same-PID close-to-hide, owned-process exit before new-process restore, target clear, and the then-current blocked-property explanation for `3659880761`. Its report has no cleanup errors and restores original workspace ID 1. The completed-swap/idle probe and 100-cycle lifecycle stress passed as recorded below; the follow-up resource diagnostics do not yet establish daily-use RSS stability.

The final `scene-probe-shine.json` records an engine-only probe on `eDP-1` with a 30 FPS limit; it writes no session/settings and does not test UI. The animated window processed about 475.93 status requests per second. Its stable logical Scene RGBA/FBO payload was 51,871,748 bytes (49.47 MiB), not measured VRAM.

| Phase | Window | Completed swaps | Other observations |
| --- | --- | --- | --- |
| Animated Shine | 60.000055 seconds | 1,773; about 29.54997/s | 1,773 draws under frequent command wakeups. |
| Paused, after settling | 60.000080 seconds | 0 | Render attempts, draws, empty frames, and callback requests/receipts all zero. |
| Resumed | 3 seconds | 89; about 29.66595/s | Animation resumed within the configured cap. |
| Authored static candidate while paused | First frame | 1 | Exactly one draw and swap to accept the candidate. |
| Static playing, after settling | 60 seconds | 0 | Render attempts, draws, swaps, and callback requests/receipts zero; logical payload 1,032 bytes. |
| Clear and shutdown | End of probe | — | Output metrics removed; backend shutdown joined successfully. |

These are engine completed-swap/callback counters and bounded idle observations. They establish deadline enforcement under command traffic and the absence of a sustained paused/static render loop during these windows. They do not measure compositor display timestamps, p95 GPU render time, normal idle CPU/power, VRAM, 24-hour endurance, or multiple outputs. The completed 100-cycle lifecycle result below does not establish flat memory or daily-use readiness.

The final `scene-stress-100.json` and log record 100 alternating Shake/Shine cycles on the same backend, each Scene → Video → Scene → Clear. The Rust test passed in 424.37 seconds; the wrapper took 430.392 seconds. Isolated save-failure rollback, late commit acknowledgement with deadline cancellation, backend shutdown/join, saved Scene restore, and final clear all passed. The user's original config remained unchanged; owned application processes were removed and workspace 1 restored.

Whole-test-process RSS, including prepare/cache/engine/libmpv, peaked at 832,548,864 bytes (793.98 MiB) and ended at 714,219,520 bytes (681.13 MiB). Its four time-window medians rose from 562.84 → 620.44 → 641.48 → 664.35 MiB. This is neither application-only memory nor VRAM; attribution was unresolved at that acceptance point. P1 daily-use resource stability has not passed. FD maxima were 22 in each window and 14 at the end; the initial count of 4 is not a steady-state baseline. No FD growth was observed across those windows, which does not establish long-term stability.

The resource follow-up added explicit context/display destruction and idempotent surface cleanup, including initialization failures; the display/library owner remains alive until its last context/surface is released. Video construction now loads its initial source once instead of issuing two consecutive `loadfile replace` commands. These fixes address concrete lifetime/redundant-work paths without claiming they explain all of the preceding RSS rise.

Private `resources-{cpu,scene,video,mixed,restart}.jsonl` and `resources-summary.json` record separate engine-only subprocesses on the same single-output 680M machine; all exited successfully and preserved the user's original config. The following values compare the first and last release/clear snapshots, not cold startup:

| Mode | Cycles | Rust requested live bytes after release/clear | Additional observation |
| --- | --- | --- | --- |
| CPU prepare/drop | 30 | 2,177 → 2,177 | No desktop backend; decoded Scene data was released each cycle. |
| Scene apply/clear | 30 | 49,259 → 51,539 | Backend remains running; this small change is not a proof of a steady state. |
| Video apply/clear | 30 | 49,259 → 49,259 | RSS still rose despite fixed Rust live bytes. |
| Scene → Video → Scene → Clear | 50 | 49,637 → 63,029 | glibc free blocks: 166,546,944 → 654,284,608 bytes; RSS: 290,889,728 → 614,535,168 bytes. glibc used blocks changed from 15,622,656 to 16,472,256 bytes. |
| Backend restart | 20 | 2,229 after every shutdown/join | Every stopped snapshot has four FDs and one thread. |
| Mixed, fresh child with `MALLOC_ARENA_MAX=2` | 100 | 49,260 → 51,846 | glibc free blocks: 134,691,232 → 198,131,456 bytes; clear snapshots retain 12 FDs. Final clear has 19 threads. |

These counters support substantial allocator-retained free space as a contributor to the video/mixed process memory trend; they do not establish that every native allocation is correct or that RSS reaches a stable bound. System-allocator Rust counts and glibc block counts measure different scopes, and the probe omits shell catalog/cache/UI work. P1 remains open.

The controlled arena comparison in private `resources-mixed-arena2.jsonl`/log passed 100 cycles in 174.26 seconds. After final shutdown/join, Rust live bytes were 2,228, with four FDs, one thread, and no DRM clients. Its final clear RSS was 226,689,024 bytes with another 29,519,872 bytes swapped out; file-page residency also changed. The observations are consistent with the arena policy influencing retained free blocks, but the default run used 50 cycles and the controlled run 100 under different memory pressure. They are not a production performance comparison or proof of an RSS benefit. Only the diagnostic child received this environment variable; no product allocator tuning or forced trimming is adopted.

The follow-up also implements persisted FPS/battery controls and live engine policy updates. Saved settings remain authoritative if a live update fails, and the UI exposes the runtime issue. The FPS option caps paced rendering/swaps; battery pause preserves the user's manual pause intent. The opt-in `desktop_settings_live_updates_preserve_assignments_and_backend` test passed in 22.05 seconds with isolated config, recording the following in private `settings-live.log`:

| Saved limit | Measurement window | Completed swaps/rate | Status requests |
| --- | --- | --- | --- |
| 15 FPS | 10.0003 seconds | 147; 14.6996/s | 4,589 |
| 30 FPS, updated live | 10.0004 seconds | 296; 29.5987/s | 4,758 |

Both samples confirmed the same backend, unchanged assignment bytes, and saved preference readback. Pause-on-battery was saved and active in the engine policy; the machine reported `on_battery=false` and `power_paused=false`. No physical battery/AC transition was tested. These are completed-swap counts from the real service path, not compositor presentation timestamps or Settings UI actions.

The separate native UI run in private `ui-settings-verified/report.json` passed, with `visual-review.json` recording completed review. Native English Settings actions saved 15 FPS and pause-on-battery, changed the limit to 30 FPS while the wallpaper ran, and verified 30 FPS plus the battery preference after a new owned process restored the wallpaper. Chinese Settings layout was also reviewed. Apply, same-PID close-to-hide, complete owned-process-group exit before restart, saved restore, target clear, and the then-visible `3659880761` bloom blocker all passed. Cleanup errors were empty, the isolated config was removed, and original workspace ID 1 was restored. The preceding `ui-settings-final` automated result had poorly framed 15/30 FPS captures and is not the new visual acceptance record. This finite workflow does not establish physical battery transitions, screen lock/suspend recovery, power, 24-hour endurance, or full P1 readiness. These are development-checkout results; installed-package acceptance remains outstanding.

### Mixed Scene and per-output properties: 2026-10-10

The independent Rust CPU/GPU path now implements the original `3659880761` package's active particle tree, WaterRipple/Shake/Iris effects, LDR bloom and authored single-entry FLAC loop layers. Preparation succeeds with seven render layers, five effects and 21 passes. Its default is six visible particle roots plus all five children, with worst-case particles/vertices/work of 3,965/26,790/30,980. The seventh root, Reactive, is literal `visible:false`; preserving that author-hidden layer is correct default playback. Enabling its audio analysis remains blocked. No authored active particle/effect/bloom layer was disabled to pass preparation.

Particles execute bounded emitters, initializers, movement/drag/gravity, alpha/size oscillation, angular movement, turbulence, sprite/rope rendering and spawn/follow children against SceneClock. Unknown behaviors fail explicitly. OWE source is only a behavior reference; the Rust runtime has independent state structures, RNG and gradient/curl field. Native noise, child lifecycle/transforms, rope sampling and complete-frame visual equivalence remain unverified.

REA confirms the LDR four-pass input chain, RGBA8 color targets, scaled dimensions rounded down with a two-pixel minimum, and texel size from the full output dimensions. It also confirms that a single-entry loop has no random min/max interval and uses squared layer gain. These are static native findings, separate from visual or audio acceptance.

The final finite engine-only probe, private `runtime/365-engine-final-60s.json`, ran on the recorded Wayland + `niri`, AMD 680M, single `eDP-1` machine. It passed with shutdown joined and output metrics removed after clear, writes no session/settings, and performs no Tauri UI actions:

| Phase | Observation |
| --- | --- |
| Default mixed Scene, 30 FPS cap | 1,677 completed swaps in 60 seconds, about 27.95/s; about 466 status requests/s and zero empty frames. |
| Settled pause | 60 seconds and 28,000 status requests with zero render attempts, draws, swaps, empty frames and callbacks. |
| Resume | 85 completed swaps in three seconds, about 28.33/s. |
| Logical GPU payload | 134,862,328 bytes for RGBA textures/FBOs and particle VBO; driver/EGL/video overhead is excluded. |

A separate private Pulse sink/PCM probe observed zero PCM in candidate, paused, muted and dropped phases, and activated RMS 2,894.53. Its owned sink was cleaned up and pre-existing sinks/default sink/source remained unchanged. This does not prove physical-speaker output, every loop boundary or the complete property/audio rollback transaction.

The separate native default UI flow in private `runtime/ui-365-default-final/report.json` passed with `mockIpc=false`, isolated config and manual capture review. Native Library/monitor Apply reached Desktop Running; titlebar close hid the same PID while wallpaper/session remained; the entire owned process group exited before a new Tauri process restored the Scene; target Clear restored the baseline. Native Settings saved 15 FPS and battery pause, changed to 30 FPS without changing PID or assignment bytes, then read back 30 FPS and the battery preference after the new-process restore. English/Chinese captures were reviewed for full composition, particle change across two frames, restored visibility, clear baseline and label layout. Cleanup errors were empty and workspace 1 was restored. No original-default active feature was disabled. This passes the default UI workflow; the separate property and finite fullscreen run is recorded below. Daily-use acceptance remains outstanding.

The finite `runtime/365-resources-mixed-20.jsonl` and summary record 20 cycles of `3659880761` → video `2848041076` → `3659880761` → clear. Every clear had empty active status/metrics; shutdown joined. Clear FDs stayed 12 and Rust live bytes stayed 48,811. First/last clear process RSS was 257,609,728 → 287,825,920 bytes, with glibc used blocks 14,897,040 → 15,433,312 bytes. After shutdown: four FDs, one thread, 2,227 Rust live bytes, RSS 191,705,088 bytes and 250,144,480 glibc free bytes. This supports finite resource return and no FD accumulation in this run; substantial allocator/driver RSS retention prevents a memory-stability or all-native-allocations-leak-free claim. Power and daily-use endurance remain unverified.

The final implementation snapshot passed workspace fmt/clippy/check/tests and 98 Python tests. After the error-state Reset follow-up, frontend type checking reported zero errors/warnings, 25 component regressions and all 148 tests across 26 files passed, and the production build passed. The new error-state recovery remains component evidence, distinct from the recorded native normal Save/Reset flow. Three ignored GPU fixtures explicitly passed in a separate surfaceless EGL run: authored image/GL state, bloom halo/state, and installed original-asset bloom/WaterRipple/Iris/generic-particle/padded-orientation/cutout pixels. This is separate from CPU preparation and native desktop/UI acceptance; it does not establish an original-engine frame A/B. Documentation build is the final integration check after these records are complete.

Validated bool/slider/color edits are scoped to project and output and saved in `session.toml` alongside assignments. The Save and apply transaction is first-frame/commit acceptance → one atomic assignment/value save → Finalize and audio activation. Preparation, candidate acceptance or save failure before atomic publication retains the prior renderer and choices. Once saved, the durable assignment remains authoritative; deferred finalization cleanup cannot undo it. Ordinary save/clear retains other monitor assignments and every unrelated choice; clearing the final assignment does not delete saved properties. Missing files mean empty state; invalid/oversized state prevents overwrite and diagnostics omit property values. The separate native property run below passed. If an author-schema change invalidates shape-valid saved choices, Reset defaults is also available after property-load failure with no snapshot; it retries `{}` through default preparation and removes only that monitor/item selection on successful application/save. Normal Save requires validated definitions/values. Unsupported defaults or unreadable/corrupt session state remain explicit failures. This new error-state recovery has component-regression validation; the recorded native run covers normal Save/Reset, not a separate author-schema-change scenario.

The native property/failure/fullscreen run, private `runtime/ui-365-properties-final/report.json`, passed with real checkbox/number input and manual capture review. The author property `hdr` changed true → false to disable **LDR bloom**, not enable HDR rendering; `newproperty2` changed 0 → 0.1 for the authored audio-layer gain. Real Save failed with PermissionDenied after only the isolated `config/lwe` directory was made 0500: session bytes/inode stayed unchanged, the draft remained editable and the previous Scene stayed Running and visible. Restoring 0700 and retrying the same choices succeeded: the complete TOML was atomically replaced with a new inode, while an already-open descriptor retained the prior values. A new Tauri process restored the saved choices in real controls. Reset defaults removed overrides, restored every author value including bloom=true and the original session byte hash, and retained the assignment.

The owned Tauri window then covered the whole logical 1728×1152 output for 12.000 seconds. Reveal retained Running and two captures showed evolving particles. Callback/occlusion-pause was not directly measured; this does not establish system suspend or lock recovery. All owned process groups stopped, isolated config was removed, workspace 1 was restored and cleanup errors were empty. The passed finite property/fullscreen flow does not establish original-engine visual parity, 24-hour endurance, actual suspend/lock, physical battery transitions, power/VRAM stability or installed-package acceptance.

An original-engine reference attempt used an isolated engine copy and Wine prefix. The reference UI failed with `c0000005` before any usable capture; owned cleanup had no errors and source hashes stayed unchanged. This is not an original-engine A/B result. The read-only 11-project CPU rerun is saved privately as `scene-prepare-corpus-2026-10-10-mixed.json` (binary/inventory SHA-256, package hashes, timings, exit codes and raw first-blocker results). It passes `3258032485`, `3337606455` and `3659880761`; the eight first blockers are five SceneScript, `2798622011` camera, `2454418355` HDR and `3485147659` layer-count bound. The latter reports a null feature code plus the exact layer-count validation error. All eleven packages match P0. The existing compiled example was reused without a build, backend/GL launch or settings/session write. This 3/11 CPU result is not a Workshop compatibility rate or native UI acceptance. Keep finite CPU/GPU/PCM evidence and the passed default native UI workflow separate from outstanding original-engine visual comparison, 24-hour soak, ten actual suspend/resume cycles, physical battery/AC transitions, power/VRAM measurement, independent multi-monitor and installed-package acceptance. Daily-use readiness is not established.

Keep the 2026-10-09 acceptance and earlier property/bloom-blocked UI captures as historical evidence. The new original-package mixed-Scene implementation and finite probes do not supersede their workflow boundaries or establish daily-use support.

The three separate GPU pixel/GL-state fixtures can be repeated without applying a desktop wallpaper. Original-asset coverage requires the local default 365 project and installed assets:

```bash
lwe_scene_project="$HOME/.local/share/Steam/steamapps/workshop/content/431960/3659880761"
lwe_scene_assets="$HOME/.local/share/Steam/steamapps/common/wallpaper_engine/assets"
for lwe_scene_gpu_test in \
  scene::renderer::tests::offscreen_pixels_and_gl_state_are_preserved \
  scene::postprocess::tests::offscreen_bloom_halo_threshold_tint_and_state \
  scene::postprocess::tests::offscreen_original_bloom_and_effect_shader_assets
do
  EGL_PLATFORM=surfaceless LWE_SCENE_TEST_PROJECT="$lwe_scene_project" \
    LWE_SCENE_TEST_ASSETS="$lwe_scene_assets" \
    cargo test --locked -p lwe-engine --lib "$lwe_scene_gpu_test" \
    -- --ignored --exact --test-threads=1 --nocapture
done
```

These fixtures validate their pixel cases and GL-state restoration on the available EGL/GPU context. They do not compare captured original-engine frames or establish Wayland/multiple-output lifecycle behavior.

### Space 2026-10-10

The original Workshop [2.5D Space (`2903458386`)](https://steamcommunity.com/sharedfiles/filedetails/?id=2903458386) passes shared CPU preparation and real LWE application without rewriting its package or removing layers/effects: ten image layers, eleven effects and twenty-four passes. Eight Scroll instances, the audio Pulse/Chromatic passes and the four-pass project bloom remain in authored order. General bloom is disabled in this project; that does not disable its custom bloom. Project resources retain priority over installed builtins.

Fourteen unchanged scalar scripts passed seventy independent numeric cases across default, minimum, maximum, zero and mixed property snapshots. Oxc checks the module AST before bounded QuickJS evaluation on the CPU worker; no script callback runs on the renderer. Twelve original shader programs linked as GL 130. Real desktop-output monitor analysis separately passed stereo bass/treble, pause/resume, silence and cleanup checks. Its sixteen normalized bins per channel do not establish Windows frequency-cutoff or visual equivalence.

Private `space-runtime/gl-acceptance.json` passed all twenty-six GPU pixel comparisons: setting each of eight image layers to zero opacity changed more than a million pixels; six opacity groups, Chromatic/Pulse/Bloom switches, layer order and background color changed the complete scene as expected. Original-source composition modes 0/30/31 passed their pixel checks. With a nonzero spectrum, paused forced redraw and resize back to the original size retained identical pixels, and resume accepted a new spectrum. These EGL checks establish the authored paths and retained input; they do not compare a Windows engine screenshot. The separate production monitor-to-desktop flow also passed: with Global Speed set to zero, quiet frames and the return to silence were identical; an owned low-volume stereo tone changed the wallpaper, pause retained that audible frame through tone changes, and resume responded again. Pixel comparisons used the 2160×1090 wallpaper region below existing desktop widgets. The user's audio routes, configuration, original assets and workspace were preserved.

The real native Tauri flow in private `runtime/space-ui-delivery/report.json` passed on the recorded single-output Wayland + `niri`/AMD 680M machine: Library/monitor Apply reached Running, all twenty author controls (sixteen sliders, three booleans and one color) changed and saved, isolated PermissionDenied preserved the prior assignment/choices and draft, and retry atomically replaced the complete session. A new owned application process restored saved controls and wallpaper; Reset restored every author default. Settings 15 → 30 FPS, twelve-second owned fullscreen cover/reveal, target clear and owned cleanup also passed. Color used the rendered input event rather than the system picker. This finite UI flow does not measure physical suspend/lock or occlusion callback policy.

The final engine-only probe used a 30 FPS cap and frequent status requests: twelve seconds measured about 27.25 completed swaps/s, twelve seconds of settled pause had zero render attempts/draws/swaps/empty frames/callbacks, and resume measured about 27.33/s. Shared scratch targets by layer dimensions plus named/full-frame targets accounted for 202,180,100 logical GPU bytes. The configured cap is an upper bound, not guaranteed 30 FPS. Driver/EGL/video overhead and measured VRAM are excluded. Windows A/B, 24-hour endurance, ten real suspend/resume cycles, hardware battery transitions, measured power, independent multiple monitors and installed-package acceptance remain open.

### Yakuza extension 2026-10-12

This batch adds a bounded dynamic SceneScript worker alongside the existing pure scalar path. The accepted lifecycle is module initialization, `init`, initial declared-property delivery and repeated `update`; callbacks can use limited `Vec3`/`copy`, declared controls and layer `origin`/`scale`/`angles`/`color`/`alpha`/`visible`. An `init` return can update its bound property. `createLayer(path)` is limited to safe image-model JSON resources during initialization, with normal shared preparation/decoding; the rendering thread does not run a JavaScript VM. Authored object handles are reserved before assigning created handles. Validated worker state precedes first-frame ACK, paused forced redraws keep the accepted state, and the reversible apply/save/finalize boundary still owns failures and rollback. This subset does not implement config/handle overloads, arbitrary clones, destruction, scene queries, Vec2, engine time, cursor/media events or general interaction. Official [init](https://docs.wallpaperengine.io/en/scene/scenescript/reference/event/init.html), [update](https://docs.wallpaperengine.io/en/scene/scenescript/reference/event/update.html) and [scene APIs](https://docs.wallpaperengine.io/en/scene/scenescript/reference/class/IScene.html) describe a wider engine contract than LWE currently accepts.

Oxc validates source and AST before QuickJS, with at most 32 bindings, 64 KiB per source/256 KiB total, AST depth 48, 128 total authored/created handles, 8 MiB VM memory, 256 KiB stack and 100 ms initialization/10 ms frame execution. Prototype/global escapes, unvalidated computed writes, imports, unknown APIs and unsupported outputs fail closed. Runtime exceptions, deadline exhaustion, worker exit and cancellation must preserve the last durable assignment. CPU tests cover these finite rules; they do not replace GPU or native application acceptance.

`registerAudioBuffers(64)` is accepted only at module global scope; registration inside `init`, `update` or property callbacks is rejected. Its `average` is an actual fixed-length `Float32Array` refreshed in place from a genuine 64-band transform of the captured desktop output. The sixteen-bin stereo shader uniforms remain separate; expanding them would not produce 64 independent bins. LWE currently accepts only the average channel at this resolution, with silence on unavailable input and retained accepted input/state during pause or rollback. The official [audio buffers](https://docs.wallpaperengine.io/en/scene/scenescript/reference/class/AudioBuffers.html) expose additional channels and resolutions; Windows cutoff, level normalization and picture parity are unverified.

Combo declarations now preserve author labels and nonempty unique string option values, require the default and every override to be declared members, expose an accessible labelled select and persist strings in the same per-project/per-output choice snapshot. Exact `{name, condition}` combo equality bindings produce bool visibility. A mixed script/user wrapper extracts the script and validates its remaining user condition. Scalar shader mappings do not consume combo strings. The original Yakuza choices are `av`/`0` for `av`, four `pic` variants (`Majima_1`, `Kiryu_1`, `YakuzaDragon`, `Majima_2`) and `schemecolor`; invalid declarations/selections cannot silently substitute defaults.

Single-image `TEXS0002` particle atlases retain validated rectangular UV frames and sequence/random-frame selection, with supported adjacent-frame interpolation in sequence mode. Timed `TEXS0003`, multi-image atlases and use as ordinary image/effect textures remain gated. Added planar particle components include turbulent velocity, random alpha/angular velocity, angular movement, alpha/size oscillation and turbulence; finite simulation/geometry/work budgets still apply. Native retained framing/component evidence supports specific fields, while noise/random behavior and complete original-engine particle A/B remain unverified.

Yakuza's authored `perspective:true` bar is admitted only in the coplanar fixed-orthographic subset: origin Z=0, no X/Y rotation and no camera movement/parallax. Retained original-binary evidence supports canonical camera/projection behavior, but does not establish the per-layer perspective branch's extra mapping. Treat planar equivalence as a mathematical inference and retain the original-engine visual parity gap.

The shared `genericimage2` card adapter now preserves the legacy VERSION-absent Brightness/Alpha interface as well as explicit VERSION 1/2 Color4, without adding a renderer API or relaxing unknown-input/combo guards. The final GPU cases below verify original-card color/alpha/visibility; original-engine visual parity remains unverified. The final adapter also preserves the author's object color/alpha when VERSION is absent; this fixes the original owner bar's black tint without removing its handle. The earlier Space VERSION 2 record remains unchanged.

**Recorded finite acceptance:** private `yakuza/cpu-prepare-report.json` passes all eight original-package combinations: four `pic` backgrounds × `av`/`0`. Private `yakuza/gpu-report-final.json` passes five full-scene native offscreen OpenGL cases: each background with bars enabled, plus default background with bars disabled. These retain 64 bar handles, 63 initialization-created layers, original 4×4 bar material/texture, 66–67 render layers and 67–68 passes. All twenty-five pixel comparisons and fifteen worker destruction/recreation cycles passed, including original-card color, zero-alpha and hidden-layer assertions. Injected 64-band audio changed 8,070 isolated bar pixels in each enabled case and zero in the disabled case; twenty paused forced redraws and resize-back preserved the accepted frame. Injected-input GPU evidence does not establish the actual capture path.

Private `yakuza/ui-run-2/report.json` passes the final frozen build through the real `mockIpc=false` Tauri Library/Desktop flow on single `eDP-1`, Wayland + `niri`/AMD 680M. Private `yakuza/visual-review-final.json` records review of the full default and Kiryu scenes, restored controls and Chinese Settings layout. The author color label remains its original localization token; this does not establish complete translation of author labels. Native controls saved `av=0`, `pic=Kiryu_1` and color, while a real isolated PermissionDenied preserved session bytes/inode, editable draft and the prior Running renderer. Retry atomically saved the complete choices; a new process restored them, Reset removed overrides and restored all defaults while retaining the assignment. Settings 15 → 30 FPS, same-process close/hide, twelve-second owned fullscreen recovery and target clear passed; cleanup errors were empty and workspace 1 was restored. Color used the rendered input event, not the system picker. Private `yakuza/native-temporal-pixels-final.json` compares exact RGB8 pixels below desktop widgets (y≥300): default paired frames changed 1,297 pixels and paired frames after fullscreen recovery changed 1,848. This confirms temporal Scene-area changes, without assigning every changed pixel to a particular component. Fullscreen recovery does not measure callback occlusion pause or physical suspend.

Private `yakuza/gpu-report-final-live-audio.json` separately passes real PulseAudio/PipeWire PCM capture from an owned silent null-sink monitor → genuine 64-band FFT average → unchanged original SceneScript in LWE's worker → native offscreen bar GPU pixels. The renderer is AMD Radeon Graphics (`radeonsi`, `rembrandt`, ACO), OpenGL 4.6/Mesa 26.2.4. Playback changed 1,285 bar pixels; after pause/disconnect and resume/reconnect, responsive bars changed 690 pixels. Pause retained the displayed pixels exactly, and stopping playback returned exactly to the silent bar frame; initial and released 64-element buffers were zero. The captured source was verified as the owned sink monitor, capture and script workers released, the owned sink unloaded, and default sink/source remained unchanged. This proves the real capture/FFT/script/GPU chain under isolated routing. It uses offscreen pixels, excludes speaker output and default-device capture, and is separate from the passed visible desktop UI flow; it does not certify the user's current music/video route or Windows FFT/visual parity.

The final source/binary identity is retained privately in `yakuza/final-source-identity.json`; all 125 recorded Rust source/manifest/lock files match the frozen tree. Final workspace fmt/clippy/check, 466 ordinary Rust tests and the shell build passed; earlier frontend 154 tests and Python 144 regressions also passed. These source checks remain separate from runtime acceptance.

The final `yakuza/space-regression-gpu-report-final.json` additionally passes all twenty-six 2.5D Space GPU comparisons after this extension. The unchanged project retains ten layers, eleven effects, twenty-four passes, twenty controls and 202,180,100 logical GPU bytes. Original opacity/effect/order/composition, injected stereo spectrum and paused redraw/resize retention remain covered. This is a new deterministic offscreen GPU regression, preserving the older Space acceptance record; it does not add a new desktop/audio-capture run or Windows parity.

**Remaining acceptance:** Preserve original file hashes and private evidence, and keep earlier blockers as dated snapshots. Physical suspend/resume was explicitly deferred by the user because it interrupts Codex; this batch neither invokes it nor requires it for completion. The old 24-hour job was gracefully ended using verified owned PIDs to release the monitor, with its evidence retained; it is an interrupted run, not passed endurance. Physical multi-monitor, installed-package, Windows A/B, hardware battery/power, advanced scripts, bones and HDR remain separately unverified or unsupported. These finite results do not establish daily-use readiness.

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

### Durable Scene reliability acceptance

Physical suspend/resume is explicitly deferred from the current batch at the user's request because sleep interrupts Codex. The following non-suspend recorder example uses zero required sleep cycles and cannot certify suspend recovery. The full future recovery target and helper remain documented separately.

Use `scripts/scene-reliability.py` with an immutable copy of the compiled `lwe-shell` library-test executable and its exact ignored test `services::desktop_service::reliability_tests::desktop_scene_daily_use_reliability`. Build the executable with `cargo test --locked -p lwe-shell --lib --no-run --message-format=json`; select the `compiler-artifact.executable` from that output. Preserve the full dirty-source fingerprint and original input files during the run. The recorder isolates `XDG_CONFIG_HOME`; retain `HOME` for Steam discovery.

Run with `LWE_REAL_DESKTOP_TESTS=1`, `--hours 24 --cycles 100 --suspend-cycles 0 --sample-seconds 5 --memory-growth-mib 64`, the actual `--binary`, `--project`, `--assets`, `--video`, and a new private `--output` directory outside Git. Set `--scene-ids 2903458386,3659880761,3337606455,3258032485` for the fixed cohort, and append `-- services::desktop_service::reliability_tests::desktop_scene_daily_use_reliability --ignored --exact --nocapture --test-threads=1`. The first project is the continuous primary; the others participate in finalized round trips. The recorder never requests sleep.

For a future task that includes physical recovery acceptance, after `suspend-readiness.json` reports a fresh healthy primary Scene, use `scripts/scene-suspend-cycles.py --check --readiness PATH` for read-only preflight. Privileged RTC cycles require an authorized local invocation with `--readiness PATH --output-dir NEW_PRIVATE_HELPER_DIRECTORY --cycles 10`. The helper preserves normal logind inhibitors and locking and waits for normal authenticated unlock. A root invocation must not overwrite an existing RTC alarm. Credentials must never appear in argv, environment, files, reports, or automation prompts.

A completed recorder's `passed=true` establishes only its recorded targets; a run with `--suspend-cycles 0` cannot establish physical recovery. Future recovery acceptance also needs real helper evidence. `report.json`, `manifest.json`, and append-only `evidence.jsonl` distinguish active presentation, complete switches, actual kernel sleep, resource growth, timing and cleanup. A running job, mock/unit test, wall-clock day, restart after failure, or source rebuild is not a passed 24-hour run.

`--finish-after-soak` optionally requests normal clear/shutdown/join once effective Scene presentation and finalized round trips reach their targets, even if a separately requested suspend target is incomplete. Such a partial result remains `passed=false` with recorder exit code 1, `status=completed`, a successful child exit, explicit completed/incomplete components and `stop_reason=soak_and_switch_targets_reached_suspend_incomplete`; it is not a runtime failure requiring a restart. This policy is part of the immutable manifest and cannot be changed with `--resume`. Interrupted reports record the actual signal. Fresh complete acceptance can pass; prior continuity segments remain separate and cannot be appended into an uninterrupted pass.
