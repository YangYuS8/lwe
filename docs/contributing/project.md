# Project overview

This page gives contributors the context needed to maintain the project and its current development scope.

## Product direction

LWE is a Linux dynamic wallpaper platform for users migrating practical Wallpaper Engine workflows to Linux.

The product is a desktop application first, not a daemon-first system. Its differentiator is an integrated loop for Workshop discovery, acquisition orchestration, synchronization awareness, import, compatibility visibility, and runtime support.

## First-release scope

First-release priorities:

- Workshop-centered discovery and acquisition orchestration;
- compatibility reporting as a visible user-facing feature;
- library-first daily use;
- video wallpaper runtime support;
- individually validated experimental 2D Scene playback;
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
| `crates/lwe-scene` | Independently implemented Scene package and texture readers |
| `packaging/aur/lwe` | Stable AUR package metadata |
| `packaging/aur/lwe-git` | Development AUR package metadata |
| `.github/workflows/` | Quality, release, and documentation deployment automation |

Retired `wayvid` GUI/CLI paths should be treated as historical context only. Do not reintroduce old product framing unless a spec explicitly justifies it for the LWE direction.

The experimental Scene path is an independent Rust implementation with no Open Wallpaper Engine runtime dependency. It uses the same `PreparedScene::prepare` validation for compatibility assessments and application. Preparation performs filesystem reads and texture decoding off the runtime thread; the engine owns GL initialization, presentation acknowledgements, and cleanup. Cached compatibility assessments retain summaries only and never replace preparation before applying. Unknown features fail closed.

The current renderer accepts 2D image layers with explicit fixed orthographic dimensions, bounded sprite/rope particles and child emitters, supported material/effect passes including Shake, Shine, WaterRipple and Iris, four-pass LDR bloom, and single-entry FLAC loop layers. Particles can retain single-image `TEXS0002` atlas frames, sequence/random-frame selection and sequence-frame interpolation; bounded planar turbulence, alpha/size oscillation and angular motion share finite preparation, simulation and geometry budgets and the SceneClock. Original-engine noise/randomness parity remains unverified. A saved camera whose eye/center have matching X/Y, face toward negative Z, and use default up is normalized by the original program's observed static loading branch; saved X/Y need not be zero. Exact `user`/`value` bindings resolve declared bool/slider/color/combo values after type/range or option-membership checks. Conditional combo equality produces visibility; unknown conditions and invalid defaults/overrides fail closed. Project/output choices are validated before preparation and saved atomically with the assignment in the same session file. Earlier native bool/slider editing, save-failure rollback/retry, new-process property restore and full-default reset passed on the recorded single-output machine; the new Yakuza combo/color edit, save-failure/retry, restart and reset flow has separately passed native acceptance.

The legacy static-card branch accepts `genericimage`, two- and four-texture `flowimage`, `swayimage` and `flag` by adapting the installed or project-owned GLSL at runtime. It uses model `width`/`height` when an object omits dimensions, retains project resource precedence, and preserves top-down author UV math, image/storage padding, supported texture repeat/clamp flags and SceneClock animation. Flag normal decoding derives its format combo from the bound source texture. Declarative `usershadervalues` maps validated bool/slider/color choices from a project property to a material uniform. LWE currently gives that user mapping priority over an explicit material constant; exact native conflict precedence remains unverified. Unknown shaders, shader compile-time combos, runtime dependencies, sampling flags and mapping targets remain blocked.

The `genericimage2` card path accepts the legacy VERSION-absent Brightness/Alpha interface and explicit VERSION 1/2 Color4 interface, retaining VERSION 2 mode 0/30/31 composition after the effect chain. Fullscreen/passthrough, Scroll, Pulse and bounded project-owned passes use the original project-first resources; full-frame FBO snapshots and named targets preserve their inputs. Real desktop-output monitor capture supplies normalized sixteen-bin stereo spectrum uniforms, activating only after durable assignment save and Finalize. Paused/rollback renderers retain their displayed audio input for forced redraws. This does not establish Windows FFT cutoffs or full SceneScript audio APIs.

Active camera paths, automatic projection, tilted/reversed cameras, nondefault up, dynamic camera movement, model `parent` bindings and layer hierarchies outside the bounded planar subset, unsupported component/binding shapes, broader SceneScript/FFT APIs, HDR and the other limitations in the [usage guide](../guide/usage.md#experimental-scene-validation) remain blocked. Open Wallpaper Engine source is a behavior reference only; LWE does not execute its C++ runtime. Original-engine noise, child, rope and complete-frame visual A/B remain outstanding. CPU tests and builds do not establish visible Scene playback; real desktop acceptance names each sample, workflow and environment separately.

Pure scalar SceneScript bindings retain Oxc AST validation and bounded QuickJS evaluation on the preparation worker. A separate bounded SceneScript worker supports `init`/`update`/`applyUserProperties`, limited `Vec3` and image-layer state, and initialization-only image-model `createLayer(path)`. All authored objects retain distinct handles before created layers receive new ones. Created resources pass shared preparation; the engine accepts only validated snapshots and waits for one before first-frame ACK. Paused redraw/rollback keeps the accepted state. Cancellation, exceptions, execution timeout, worker failure and unknown capabilities use the existing reversible apply boundary. Source/AST, bindings, total layers, VM memory/stack and callback time have finite limits. Only module-global `registerAudioBuffers(64).average` is supported: it is an updated-in-place `Float32Array` fed by a genuine 64-band transform of desktop-output samples, rather than expanded sixteen-bin values. Windows cutoffs, normalization and visual FFT parity remain unverified. Imports, general dynamic access, clock/random APIs, cursor/media interaction, other channels/resolutions and broader host APIs remain blocked. `3420277613` still needs broader scripts and resources. Yakuza's eight background/bar CPU combinations, five full GPU cases and real native UI transactions passed; real private-output monitor capture through the 64-band FFT, original script and offscreen bar GPU also passed, separately from visible native UI and without certifying the current user music/video route. See the [development record](./guide.md#yakuza-extension-2026-10-12).

Coplanar `perspective:true` image layers are bounded to fixed orthographic projection, origin Z=0, no X/Y rotation and no camera movement/parallax. The equivalent planar transform is a mathematical inference from those constraints. Retained original-binary evidence establishes the canonical camera, but does not establish the per-layer perspective mapping or visual parity.

`SceneInventory::collect` is a bounded static dependency/framing API, separate from `PreparedScene::prepare` eligibility. It follows modeled references, scans packaged JSON/TEX/shaders and project metadata, distinguishes reachable from packaged-only declarations, records structural locations and unknown shapes, and keeps property bindings separate from executable scripts. It does not decode texture/media payloads or execute JS. Unknown/text subtrees cannot export private asset-looking values; script/shader/text contents and private property values are omitted. JSON, shader, resource, node, depth, observation/string, mip, and animation-frame totals have explicit limits; exhausted limits or failed resources produce a partial report. PKG indexing, range reads, and hashing use the same opened descriptor and check metadata consistency, without claiming an immutable filesystem snapshot.

The P1 runtime slice enforces per-output frame deadlines, settles static/paused rendering without a sustained draw/swap/callback loop, and accounts for a 512 MiB logical RGBA texture/FBO and particle-VBO budget across current, candidate, and retained rollback resources. Images share scratch targets by dimensions: one for a single unnamed pass and two for multi-pass graphs, with named effect and bloom targets allocated separately. The estimate excludes driver overhead, EGL back buffers, and video memory; it is not measured VRAM. EGL ownership covers normal and initialization-failure cleanup; initial video loading is issued once. Settings persist FPS/battery preferences and update the existing backend. The isolated desktop-service 15 → 30 FPS test passed at about 14.70 → 29.60 completed swaps/s while retaining backend identity and assignment bytes. The finite native English Settings/save/new-process restore flow and Chinese layout review passed; physical battery/AC transitions, 24-hour endurance, ten actual suspend/resume cycles, power behavior, installed-package and independent multi-monitor acceptance remain unverified.

### Recorded Scene desktop acceptance

The 2026-10-09 development build passed the opt-in Scene flow on Wayland + `niri`, AMD Radeon 680M (`1002:1681`, `amdgpu`), and one `eDP-1` monitor at 2160×1440, scale 1.25. Sample `3337606455` has one image layer, five Shine effect passes, and six total passes; a full-resolution capture confirmed visible rendering. Three Scene ↔ Video/apply/clear cycles, session-save failure rollback, cancellation after commit processing, and restore after backend shutdown/join passed. The original video reapply flow also passed, using isolated config and restoring the prior workspace presentation.

This historical evidence covers one sample on one monitor. At that time `3258032485` was blocked as `unsupported_camera`; its earlier captures were research only. The 2026-10-10 implementation adds the REA-confirmed canonical camera branch, bounded inventory, frame scheduling and resource accounting. The 11-project P0 report, native-pointer UI flows for `3337606455` and `3258032485`, completed-swap/idle probe and 100-cycle lifecycle stress are documented in the [acceptance record](./guide.md#2026-10-10-development-record). These workflows do not establish original-engine visual parity or daily-use readiness.

The subsequent original-package `3659880761` preparation succeeds with seven render layers, five effects and 21 passes. Its authored default is six visible particle roots plus five children; the seventh root, Reactive, is literal `visible:false` and remains hidden. Enabling its audio response remains blocked. Worst-case particles/vertices/work are 3,965/26,790/30,980. The final 60-second engine-only desktop probe on the same single-output `niri`/680M machine passed: a 30 FPS limit yielded about 27.95 completed swaps/s, sixty seconds of pause produced zero draws/swaps/callbacks, and resume yielded about 28.33 swaps/s. Logical GPU payload was 134,862,328 bytes. This probe writes no session. The separate original-default native UI flow then passed with manual English/Chinese capture review: monitor Apply/Running, same-PID close-to-hide, owned-process exit, new-process restore, target clear, and Settings 15 → 30 FPS with unchanged assignment bytes and restored preferences. The separate native property edit/save-failure/retry/restore/reset flow and a twelve-second owned fullscreen cover/reveal also passed. Occlusion callbacks were not measured; suspend/lock and daily-use acceptance remain open. A private Pulse PCM probe separately confirmed silent candidate/pause/mute/drop phases and active output; hardware speakers and full audio rollback/physical-output behavior remains unverified.

The subsequent original `2903458386` (2.5D Space) keeps ten image layers, eleven effects and twenty-four passes. Its fourteen original scalar scripts passed seventy default/boundary/zero/mixed-property CPU cases, and twelve original shader programs linked as GL 130. Real native Library/Desktop application, all twenty controls (sixteen sliders, three booleans and one color), atomic save-failure rollback/retry, new-process restore, full reset, Settings 15 → 30 FPS, twelve-second owned fullscreen recovery and clear passed on the same single-output `niri`/680M machine. Color editing used the rendered input event, not an OS color-picker interaction. The final twelve-second engine probe measured about 27.25 completed swaps/s at a 30 FPS cap, twelve seconds of paused zero draw/swap/callback activity, and about 27.33/s after resume; logical GPU payload was 202,180,100 bytes. Real monitor stereo/pause/resume/cleanup tests are separate from original-engine visual/FFT parity and daily-use endurance. See the [Space record](./guide.md#space-2026-10-10).

A separate CPU gate accepts three of fourteen installed official built-in projects: Deep Space (`deep_space`, two layers/two passes), Beach (`beach`, three/three) and Eagle Flag (`eagleflag`, one/one). The remaining first blockers are six camera, four SceneScript and one model case. On the same single-output `niri`/680M machine, each original project passed a ten-second engine-only run at a 30 FPS limit: about 29.60, 29.60 and 29.70 completed swaps/s respectively. Each ten-second pause produced zero draws/swaps/callbacks, and each three-second resume measured about 29.67 swaps/s; clear removed output metrics and backend shutdown joined. Paired desktop captures confirmed visible movement, with manual picture review. All 66 checked original resource files and user config remained unchanged, and the probes wrote no session. Workspace cleanup required a corrected command; the private report retains the initial failure and verified recovery. These official projects are separate from the Workshop 3/11 corpus and were not added to Library. This establishes finite engine acceptance, not builtin native UI/persistence, original-engine A/B, 24-hour use, independent multi-monitor or physical battery acceptance.

The historical read-only 2026-10-10 CPU gate rerun accepted `3258032485`, `3337606455` and `3659880761` (3/11); eight projects first-blocked in that snapshot: five SceneScript, one camera (`2798622011`), one HDR (`2454418355`) and one layer-count bound (`3485147659`). All eleven package SHA-256 values match the P0 inventory. The preceding 2/11 snapshot and `3659880761` bloom blocker are historical; its default original package now prepares. This reuses the recorded compiled `scene_prepare` artifact and installed builtins, without a build or desktop launch. Local counts are first-blocker snapshots, not exhaustive dependencies or a Workshop-wide rate. The earlier 100-cycle lifecycle passed while process RSS rose. Follow-up CPU/Scene/video/mixed/restart probes show small or fixed released Rust live counts and substantial glibc free-block retention in video/mixed modes. Different cycle counts, memory pressure, swap/file pages and unmeasured native allocations prevent a production memory-stability claim. The [acceptance record](./guide.md#2026-10-10-development-record) preserves counters and limitations; process RSS is not app-only memory or measured VRAM. The new 20-cycle `3659880761` → video → `3659880761` → clear probe passed with fixed clear FDs/Rust live bytes and a joined shutdown; RSS and allocator retention still prevent a memory-stability or native-leak-free claim.

## Release model

| Channel | Outputs |
| --- | --- |
| Stable | GitHub Release artifacts and AUR `lwe` |
| Prerelease | GitHub prerelease artifacts and AUR `lwe-git` |

Linux release artifacts include `.deb`, `.rpm`, and `.AppImage` builds.

The workspace version source is `Cargo.toml`. Prerelease versions are derived by release automation from the base version, GitHub Actions run number, and short commit SHA.

### v0.9.10 packaging repair

The v0.9.10 packaging work repairs AppImage icon links and updates the supported build toolchain. `.tauri-cli-version` pins the CLI for both release channels, `rust-toolchain.toml` pins the Rust build toolchain, and the upload gate checks the actual AppImage before publishing. Check GitHub Releases for version availability. Public artifact validation, installation checks on `niri`, and AppImage directory retesting are separate release steps. That packaging stage retained video wallpapers on Wayland with `niri` as its runtime scope.

## Documentation model

All maintained documentation belongs under `docs/` and is published as a VitePress site. User-facing documentation must exist in both English and Simplified Chinese.

Documentation should prioritize:

1. installation;
2. first-use setup;
3. daily usage;
4. troubleshooting;
5. contributor maintenance rules.

Historical planning documents and obsolete implementation notes should not remain in the published docs tree after their useful content has been merged into maintained pages.
