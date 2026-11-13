# v1 version roadmap

This page captures the maintained development direction from the released v0.9.9 baseline toward LWE v1. Keep it current: remove or collapse obsolete version sections after release, and update the page when product scope, verified runtime support, or release priorities change.

## v1 target

LWE v1 should be a trustworthy Linux Wallpaper Engine migration app, not a broad runtime-parity claim.

For v1, LWE targets:

- a Linux desktop app built with Tauri 2 and SvelteKit;
- Rust-owned backend services for Workshop, Library, Desktop, Settings, compatibility, persistence, and runtime integration;
- Wallpaper Engine Workshop content as the primary content source;
- video wallpapers as the main runtime target;
- individually validated Scene wallpapers with experimental playback for the supported native 2D subset; unknown features or invalid resources block application;
- web wallpapers recognized for metadata and compatibility reporting, without a playback backend;
- Wayland with `niri` as the verified desktop target;
- maintained English and Simplified Chinese user-facing documentation and copy.

## Version sequence

| Version | Theme | Primary outcome |
| --- | --- | --- |
| v0.7.0 | Support matrix lockdown | Product truth is consistent and tested. |
| v0.8.0 | Runtime hardening | Video apply, clear, and restore work reliably on `niri`. |
| v0.8.5 | Library workflow hardening | Daily Library workflows are safe and clear. |
| v0.9.0 | Workshop clarity | Discovery and Steam sync expectations are honest. |
| v0.9.5 | Diagnostics and release prep | Supportability and release posture are ready. |
| v0.9.6 | Preview performance | Released: Library and Workshop card grids use bounded thumbnail assets instead of raw preview media. |
| v0.9.7 | Workshop browsing polish | Released: Workshop navigation, cached search restoration, and sparse result layouts are smoother. |
| v0.9.8 | Background power profile | Released: runtime and background refresh paths reduce unnecessary work, reuse warm snapshots, and expose lightweight snapshot diagnostics. |
| v0.9.9 | Wayland capability groundwork | Released: Wayland protocol capabilities decide dynamic wallpaper availability, with graceful fallback when required protocols are missing. |
| v0.9.10 | AppImage packaging repair | Reproducible CLI/toolchain selection and validation before publishing. |
| Current development | Experimental native 2D Scene | Local resource validation, a bounded renderer, and explicit per-item playback gates. |
| v1.0.0-rc.1 | First release candidate | Scope is frozen and validated end-to-end. |
| v1.0.0-rc.2 | Optional blocker-fix candidate | Only if rc.1 finds release blockers. |
| v1.0.0 | Stable v1 | Honest, reliable video-first release. |

## Current development: experimental native 2D Scene

The native Scene path reads verified package versions and unpacked project resources, resolves locally installed Wallpaper Engine assets, and prepares textures and shaders before applying. Compatibility reports and playback use the same `PreparedScene::prepare` gate. Cached reports retain summaries, while application validates the current resources again.

The subset includes fixed orthographic image layers and the constrained coplanar perspective path, supported materials/effects, sprite/rope particles and event children, four-pass LDR bloom and single-entry FLAC loop layers. Single-image `TEXS0002` particle atlases now retain sequence/random-frame selection and sequence-frame interpolation; bounded planar turbulence, alpha/size oscillation and angular motion are implemented. Bool/slider/color/combo choices and conditional combo visibility share validation and atomic assignment persistence. Empty-default image properties, bounded timed BC3 frame regions, private history and limited text/font rendering are added by the Miku path described below. Pure scalar scripts retain their CPU path; a separate bounded worker adds the limited dynamic image/audio lifecycle. Fullscreen/passthrough inputs, `genericimage2` modes 0/30/31 and real sixteen-bin stereo uniforms retain the accepted 2.5D Space path; dynamic scripts consume genuine 64-band average audio. Active/tilted cameras, parent bindings, broader scripts/host APIs, unknown wrappers/components, broader text/animation shapes, 3D/puppet layers, multi-image TEXS, video textures and HDR remain blocked. Passing preparation enables an experimental attempt; coplanar perspective mapping, original-engine visual/FFT/noise parity and daily-use readiness remain unverified.

Acceptance records must identify the desktop, GPU, wallpaper features, and actual monitor count. Validate repeated apply/clear, Scene ↔ Video switches, and saved-assignment restore. Multi-monitor isolation requires actual multi-monitor hardware. Video on Wayland with `niri` remains the main runtime target. The [2026-10-09 Scene record](./guide.md#recorded-experimental-scene-acceptance) covers `3337606455` on one monitor and records the then-current camera blocker for `3258032485`; the [2026-10-10 development record](./guide.md#2026-10-10-development-record) tracks the new build separately. Original-engine visual A/B has not been performed.

The current P0 API, `SceneInventory::collect`, observes modeled static references, packaged JSON/TEX/shaders and project metadata, resource origins, reachable versus packaged-only declarations, texture framing, particle/puppet/parent declarations, and structural unknowns. It separates `user`/`value` bindings from JS and HDR from bloom. Resource/document/shader, node/depth, report, mip, and animation-frame budgets apply across each traversal; framing loops consume their remaining budgets before allocation. Exhaustion or failed resources produces a partial report, not a supported result. Script/shader/text payloads and private scalar values are omitted, including asset-looking strings inside unknown or text subtrees. PKG range reads and fingerprinting retain the indexed descriptor and verify metadata consistency; this is not an immutable filesystem snapshot.

The final `scene-inventory-p0.json` covers 11 projects (2,695,798 bytes); all 11 PKG SHA-256 values match the baseline. Reachable declarations include 31 JS locations across five projects, 20 puppet locations across five, and hierarchy in three. Resource validation passed 18 tests and clippy, and independent review found no remaining privacy/budget/fingerprint blockers. These declaration counts do not count migrated projects.

The implemented P1 slice uses per-output deadlines, stops settled static/paused draw/swap/frame-callback loops, and counts current/candidate/rollback Scene resources against a 512 MiB engine-wide logical RGBA texture/FBO/particle-VBO budget. Scratch, named effect and bloom targets are counted; driver overhead, EGL buffers and video resources are excluded. EGL lifetime cleanup and single initial video loading are implemented. Persisted/live FPS and battery controls passed the isolated 15 → 30 FPS desktop-service test at about 14.70 → 29.60 completed swaps/s without replacing backend or assignment. Finite native Settings/save/restart and Chinese layout checks passed. Physical battery/AC transitions, VRAM/power, 24-hour endurance, ten actual suspend/resume cycles, installed-package and independent multi-monitor acceptance remain open.

### Next goal: Scene wallpapers that work in daily use

Progress is measured by complete wallpapers and ordinary desktop workflows. A decoded texture, compiled shader, successful IPC response, or first rendered frame is insufficient. A qualified wallpaper must render its intended layers and motion, expose its supported properties, remain usable across normal desktop lifecycle events, and fail without losing the last saved assignment.

The first migration corpus is the 11 locally inspected Scene projects. The 2026-10-09 gate accepted one project and reported ten **first blockers**: six script/binding, three camera, and one HDR. Those historical first blockers were not complete dependency lists and are not the post-expansion compatibility count. Texture decoding in that investigation succeeded for 286 of 291 TEX resources; the remaining five involved TEXS/video. The bounded P0 inventory now reports additional independent declarations and unknowns without qualifying playback. Neither historical ratio estimates Workshop-wide compatibility.

The historical read-only 2026-10-10 CPU gate rerun passed `3258032485`, `3337606455` and `3659880761` (3/11), with eight first-blocked projects: SceneScript in `3232289987`, `3241857956`, `3420277613`, `3578699777` and `3718622170`; camera in `2798622011`; HDR in `2454418355`; and layer-count bound in `3485147659`. All eleven package hashes match P0. The recorded existing `scene_prepare` binary was reused without rebuilding or launching a desktop. The original `3659880761` package prepares seven render layers, five effects and 21 passes; its default engine-only desktop probe is separate evidence. The preceding 2/11 result and bloom blocker are historical. This 3/11 local CPU result is not a Workshop-wide compatibility rate, complete dependency list, native UI or daily-use acceptance.

| Initial projects | Known dependencies that determine the work sequence |
| --- | --- |
| `3337606455` | Shine baseline; complete native-pointer Tauri UI/close-hide/restart-restore/clear acceptance passed on 2026-10-10. Original-engine comparison and daily-use acceptance remain outstanding. |
| `3258032485` | Shake; original-binary evidence established the limited static orthographic camera reset. The complete native-pointer UI/close-hide/restart-restore/clear flow passed on 2026-10-10. Original-engine visual A/B and daily-use acceptance remain outstanding. |
| `3659880761` | No JavaScript or puppet. Default playback uses six visible particle roots plus five children, WaterRipple/Shake/Iris, LDR bloom and two authored single-FLAC loop layers. The seventh Reactive root is authored literal `visible:false`; it stays hidden, and enabling audio analysis still gates. Original-package preparation, a finite engine-only probe and the default native UI Apply/Running/hide/restart/restore/clear plus Settings 15 → 30 FPS flow passed, with manual English/Chinese capture review. Native property transactions and twelve-second owned fullscreen recovery passed. Occlusion callback/pause was not measured; original-engine A/B, actual suspend/lock and daily-use acceptance remain outstanding. |
| `3241857956`, `3420277613`, `3718622170` | Script/text and other project-specific dependencies; `3718622170` additionally requires video/animated resources. Inspect the complete dependency graph before choosing the first script project. |
| `2454418355`, `2798622011`, `3232289987`, `3485147659`, `3578699777` | Model JSON files contain actual puppet references, alongside their other camera/effect/particle/audio/script dependencies. These projects need the puppet path; the references alone do not establish true 3D content. |

### Workshop acceptance target

The implementation target is original Scene projects from Steam Workshop through the real LWE Library and Desktop flow. Built-in projects provide focused material and renderer checks; their successful playback does not replace Workshop acceptance. New support must operate through the shared package/preparation/runtime path rather than wallpaper-ID branches. A complete sample must preserve its required layers, animation, audio and interaction; omitting unsupported behavior or replacing the Scene with a recorded video does not count as compatibility.

The next discovery cohort contains four public Scene items with different advanced dependencies. All four were acquired through the existing licensed Steam session on 2026-10-10, using download-completion callbacks followed by actual file checks. Twelve original files totaling 144,299,366 bytes match their private copies by SHA-256 and remained stable during verification; the subscription set was unchanged. Downloaded packages and shader/script bodies stay outside Git. The original eleven-project corpus remains a separate fixed baseline. 2.5D Space was the first to gain finite real-engine and native LWE UI acceptance (1/4 at that snapshot), retaining its original layers and effects. Yakuza, Predator and Disbelief Y first-blocked on broader SceneScript in that historical snapshot. The current Yakuza extension has passed finite original-project CPU/GPU and native UI checks, plus a separate real private-output capture/FFT/script/offscreen-GPU audio check; Predator and Disbelief Y remain outside the bounded subset. These local results do not estimate Workshop-wide compatibility.

| Workshop project | Selection purpose |
| --- | --- |
| [Yakuza Audio Responsive (`2228594093`)](https://steamcommunity.com/sharedfiles/filedetails/?id=2228594093) | Author-described SceneScript audio bars and configurable controls. |
| [2.5D Space (`2903458386`)](https://steamcommunity.com/sharedfiles/filedetails/?id=2903458386) | Six scrolling layers, audio pulse/chromatic effects and bloom. The description does not prove true 3D models. |
| [Predator (`3143988185`)](https://steamcommunity.com/sharedfiles/filedetails/?id=3143988185) | Puppet Warp, audio response and 4K resource pressure. |
| [Disbelief Y (`3407827570`)](https://steamcommunity.com/sharedfiles/filedetails/?id=3407827570) | Sprite-sheet artwork, text/clock, audio bars and draggable media controls. Sprite-sheet artwork does not establish TEXS encoding. |

Record download completion, original file hashes, bounded dependency inventory and preparation blockers separately from playback. Each project ultimately needs visible rendering, author controls, pause/resume, replacement/clear and persisted restart restoration through LWE; required audio and interaction need their own checks. Passing builds, static inventories or a single first frame does not complete this target.

The first acquired item, Yakuza, exposed an older `PKGV0009` package. Its entire 30-entry directory and payload ranges match the existing bounded directory layout; all 25 JSON entries and five texture headers were inspected without changing the original bytes. The reader now admits this verified version under the same path, range, overlap and size checks, with malformed-input regressions; other unverified versions remain rejected. In that snapshot, its preparation blocker moved from package framing to SceneScript; the current extension is recorded below. Static inspection found 64-bin audio registration and dynamically created image layers, so its eleven authored objects do not establish the runtime layer count or cost. This is reader and dependency evidence, not playback support.

Space verified the `PKGV0018` directory layout with 49 entries, 28 JSON documents and nine texture headers; this exact version shares the reader's guards and corruption regressions. It now also passes the shared preparation/runtime path. Package framing alone remains insufficient for playback eligibility.

**2.5D Space is the first complete item in this new Workshop cohort:** the unchanged original retains ten image layers, eleven effects and twenty-four passes, including eight Scroll instances, audio Pulse/Chromatic and the four-pass custom bloom. General bloom, HDR and camera movement are off by default; the project's custom bloom is active. Its fourteen original scripts pass seventy numeric property cases, twelve original shader programs link as GL 130, and real stereo monitor analysis passes pause/resume and cleanup checks. Real native editing of all twenty controls, save-failure rollback/retry, new-process restore, reset, Settings and finite fullscreen recovery also passed. The final finite engine probe measured about 27.25 swaps/s at a 30 FPS cap, paused with zero draw/swap/callback activity and resumed. The passed twenty-six GPU pixel comparisons and the separately checked audio-to-picture flow are recorded in the [Space record](./guide.md#space-2026-10-10); Windows visual/FFT parity and P1 endurance remain open. The Yakuza dynamic image/audio subset now has finite CPU/GPU/native UI acceptance, plus a separate real private-output capture/FFT/script/offscreen-GPU audio check; the current Miku frame/history work is recorded below.

Predator is the later deformation/resource target: 81 objects, 29 puppet model references, 67 effect instances, reachable animated texture declarations totaling 152 frames, MP3 playlists and audio scripts. Disbelief Y exercises text/fonts, hierarchy, cursor dragging, media events and 38 scripts; its inspected texture framing does not show TEXS, so an animation-texture implementation alone cannot make it work. Static discovery is bounded: unknown builtin texture framing, opaque dynamic behavior and uninterpreted fields remain explicit in private reports and are not proof of exhaustive semantics.

Keep four milestones distinct:

- **Development progress:** a complete real project gains correct playback and a regression fixture.
- **First local daily-use milestone:** `3337606455`, `3258032485`, and `3659880761` pass visual, real UI, endurance, power, and recovery acceptance on the current single-output `niri`/680M machine. This can be reached after P3 without waiting for puppet support or other hardware.
- **Expanded 2D candidate:** a declared capability set and larger fixed corpus pass the acceptance below. Broader monitor/GPU claims require the corresponding hardware evidence.
- **Initial migration complete:** all 11 initial projects play correctly, including their advanced dependencies. A blocked project or a project with silently omitted layers does not count as migrated. A daily-use milestone may precede this milestone and must list its exact scope.

### Yakuza extension (2026-10-12)

The shared path now implements bounded `init`/`update`/`applyUserProperties`, limited `Vec3` and image-layer state, initialization-only image-model `createLayer(path)`, a genuine 64-band `Float32Array` average buffer, validated combo choices and conditional visibility. Single-image particle atlas interpolation and additional planar particle components close separate resource/particle gaps. The shared `genericimage2` card adapter also accepts legacy VERSION-absent Brightness/Alpha and explicit VERSION 1/2 Color4, retaining the existing geometry and gated combo interfaces. Resource and execution budgets remain enforced; missing APIs or unsupported shapes fail closed. Original scripts/assets remain outside Git, and no wallpaper-ID branch substitutes for authored behavior.

The unchanged original Yakuza passed CPU preparation for all four backgrounds (`Majima_1`, `Kiryu_1`, `YakuzaDragon`, `Majima_2`) with both `av`/`0` bar settings. Five full GPU cases retain 64 bar handles/63 created layers, 66–67 render layers and 67–68 passes. Twenty-five pixel comparisons and fifteen worker destroy/recreate cycles passed, including original-card color/alpha/visibility; enabled bars changed 8,070 isolated pixels under injected audio, disabled bars changed none, and paused forced redraw/resize-back retained pixels. The real native single-output `niri`/AMD 680M flow passed three-property edit/save, real PermissionDenied rollback/retry, new-process restore, full reset, Settings 15 → 30 FPS, twelve-second fullscreen recovery and clear, with reviewed final-build captures and clean cleanup. The final Space offscreen GPU regression also passed all twenty-six comparisons while retaining its original ten-layer/eleven-effect/twenty-four-pass structure. An independent owned silent-output monitor capture → 64-band FFT average → original SceneScript in LWE's worker → offscreen bar GPU check also passed, retaining pixels during pause and returning to silence after playback stopped; current user music/video routing and Windows parity are unverified. Coplanar perspective remains limited to fixed orthographic projection, origin Z=0 and no X/Y rotation or camera movement/parallax; planar equivalence is mathematical inference, with original-engine per-layer and visual parity unverified. See the [current record](./guide.md#yakuza-extension-2026-10-12).

The user explicitly deferred physical suspend/resume because it interrupts Codex. This batch does not invoke sleep and does not require it for completion; it remains an unverified future P1 target. Physical multi-monitor, installed-package, hardware battery/power, Windows A/B, advanced scripts, bones and HDR retain their own outstanding requirements.

### Miku full graph (2026-10-13)

The shared image/frame/history/script/text path now prepares the unchanged Hatsune Miku | GLITCHED HOLOGRAM package (`3420277613`), retaining eleven declarations, ten render layers, five effects and twenty-five default passes including global bloom. Safe image properties keep the author's empty selection and instance-material fallback; groups remain non-value metadata. Detailed ownership and failure contracts are in [Scene images, timed frames and history](./scene-miku-runtime.md), alongside [texture sharing](./scene-texture-sharing.md) and [timed atlas resources](./scene-timed-atlas.md).

The earlier whole-atlas estimates exceeded both resource gates. The current working set decodes bounded regions, shares actual immutable source owners, reuses targets only after their last reader, and seals frozen rollback histories in bounded CPU snapshots. The complete final-source software-EGL oracle passed both visible eight-frame atlases, three private histories, pause, same-project candidate, exact rollback/thaw and zero-charge cleanup. CPU reservation peak 525,312,699 bytes and old/candidate GPU union 525,624,984 bytes (501.28 MiB) remain below the separate 512 MiB gates. These are production logical charges, not test-only readback allocations, total RSS or measured VRAM. Freezing moves 79,833,600 bytes of history to sealed CPU snapshots; keeping it on GPU would require 605,458,584 bytes and is rejected. Sharing alone is insufficient. All five original scripts and the Atami font passed separate final-source CPU checks; twenty-six Space software-EGL comparisons also passed.

Separate AMD hardware offscreen acceptance now passed real private Pulse capture through the original sixteen/sixty-four-band VM and original shader pixels, including exact paused output, capture reconnect and cleanup. CPU options also passed Glitch OFF/POOF with the author's hidden-layer behavior retained; those branches do not yet have native-pixel acceptance. English/Chinese image controls fit the actual 380-pixel native viewport, with the existing narrow catalog-toolbar overflow outside this check.

The finite 1920×1080 nested-niri/AMD 680M engine probe retained all default effects and authored resource resolutions. Its 30 FPS cap produced only 4.53 FPS over thirty seconds, then zero draw/swap/callback operations during thirty paused seconds; resume and clear/shutdown passed. Functional completeness does not establish 30 FPS or daily-use readiness. The final native flow passed all seventeen steps: full-effect choices and CRT edits, real image-picker cancel/select, copied-source deletion, atomic save failure/rollback/retry, live Settings limits, close/hide, new-process image/choice restore, full default reset, owned fullscreen recovery and target clear. English/Chinese image-control checks and owned-process/workspace cleanup also passed. See the [Miku record](./guide.md#miku-extension-2026-10-13) for separate proof levels and the independently reproduced llvmpipe scalar-array uniform issue; AMD equivalents passed. Physical suspend/resume continues skipped by explicit instruction. Windows A/B, physical multi-monitor, installed-package and Miku endurance proof retain their separate requirements.

### Miku performance iteration (2026-10-14)

The merged Miku implementation is now the baseline for a separate scheduling investigation. Inert REA/editor resource reads support source-timed sprite addressing, zero-dt redraw declarations and ordered history/copy/combine liveness. They do not establish original Windows clock/upload/FBO scheduling or permit new host APIs, effect schemas or copy-to-swap shortcuts. Reuse of an unchanged validated timed-frame region/view already exists and is not a new optimization in this batch.

The scheduling change anchors the FPS deadline to the last successful swap and retries incomplete Scene preparation independently every 8 ms, retaining Video's decoder policy. Timed-region keys now use sealed encoded content, the prepared namespace/provenance and frame/ROI/decode contract instead of rescanning RGBA; six CPU isolation regressions passed. All original effects, assets, resource resolutions and 512 MiB CPU/GPU gates remain. The final same-profile release A–B–B–A comparison measured 5.80–5.97 → 9.70–9.87 FPS; aggregating swaps/time gives 5.8833 → 9.7833 FPS (about +66.29%). Thirty-second pause, clear/shutdown and identity checks passed; the logical GPU plan remains 500,230,580 bytes. The final-source full-default software-EGL oracle passed both visible eight-frame sources, three independent histories, pause/resize/rollback/thaw and zero-charge cleanup, retaining the earlier CPU/GPU bounds; six AMD history and two sharing checks also passed. The warm CPU key benchmark is not an FPS prediction. Finite nested results do not establish 30 FPS, long-run stability, physical-output or Windows parity; native UI save/restore and installed-package flows were not rerun. Keep this iteration separate from the 2026-10-13 acceptance identities above. The [performance record](./guide.md#miku-performance-2026-10-14) lists evidence boundaries and measurement conditions. Physical suspend/resume remains skipped.

### Miku worker notification iteration (2026-10-14)

A separate iteration starts from merged `9278023`. It adds renderer-owned coalesced result-ready hints that remove only Scene's preparation retry delay while preserving swap-based FPS deadlines, paused/idle behavior, generation isolation and Video's decoder policy. The CPU codec also hoists checked row geometry and copies full interior BC3 rows; clipping, endian/rounding, full output writes and the 84-byte scratch reservation remain. Original resources/effects/history and 512 MiB gates are unchanged.

The final CPU-only micro comparison measured about 22.49% less decode time and passed all sixteen original frame/sixteen neighbor DDS comparisons with unchanged allocation limits. Short codec-only and notification stages remain diagnostics because their baselines fluctuate; they are not stable isolated FPS effects and their percentages cannot be added. The primary thirty-second release A–D–D–A comparison measured 9.9666 → 12.1333 FPS (about +21.74%) while sampled texture-worker CPU fell about 20.47%; attempts/s increased. Pause/clear/resume/join and identity gates passed, as did the final full-default software-EGL frame/history/rollback oracle and eight AMD history/sharing checks. A separate nested engine-only check passed paused Miku → one-draw static replacement → idle/clear/join, with reviewed original-default pixels and unchanged identities. CPU/GPU bounds and cleanup remain unchanged. Publication-to-loop dispatch latency excludes decoding/presentation, and absent old-probe fields are unmeasured. See the separate [worker notification record](./guide.md#miku-worker-wake-2026-10-14) for phase/profile and sampling limits. Earlier dated evidence remains unchanged. Physical suspend/resume continues skipped, with no 30 FPS/daily-use or new UI/physical-output/package/Windows/long-soak claim.

The next step starts from this measured 12.13 FPS baseline and attributes the remaining frame time across worker-packet preparation/consumption, texture upload and GPU execution. Continue through the shared runtime with the complete original default graph, resource resolutions and budgets retained; completion-to-dispatch and CPU wall counters alone do not identify GPU cost.

### Scene CPU preparation overlap (2026-10-14)

A new iteration from merged `b092cbfc955887150bb869922eb8b6ece0553300` overlaps one next CPU packet with callback/FPS waiting, using the existing VM/timed workers and resource limits. GPU view installation and packet/history consumption remain gated. Active configured outputs only enter after a presented frame and outside pause/idle, apply/commit/rollback, frozen/quarantined or unacknowledged-swap states; sequence/time/PCM remain tied to their epoch, and seq0 resize does not rebase an already presented clock.

Timed ahead is restricted to one engine session. Multiple sessions use VM ahead plus the original gated timed decoder, cancelling speculative timed work so a withheld callback cannot hold the shared producer permit indefinitely across sessions. The final nested AMD680M release A–B–B–A comparison at 1920×1080/full defaults/30 FPS cap/private PCM/2 ms status stress observed **11.6833 → 14.7333 FPS (+26.1056%)**, with per-run ranges 11.4333–11.9333 → 14.3000–15.1666. Empty render attempts fell from 2,527 to zero; texture-worker sampled CPU increased with throughput, so this is not a CPU-use reduction. Each variant had two thirty-second animated runs, with paused/resume/cleanup/identity gates passed; no stable or long-term gain is established.

Workspace checks passed: 559 ordinary tests (including two doctests), 36 ignored, plus fourteen explicit AMD authored GPU checks (six ahead, six history, two sharing). Full original Miku software EGL separately passed eight-frame/three-history/pause/rollback/cleanup checks, including eight output and twenty-four history pixel hashes matching the merged #109 oracle; CPU/GPU bounds remain below 512 MiB. Native engine-only paused source replacement required exactly one draw/swap, then static playback was idle and all owned children exited. Earlier records remain unchanged, and original effects/resolutions, accepted replay/history and resource gates remain. This is not physical multi-monitor, 30 FPS/daily-use, new UI/package/Windows or long-soak acceptance. Physical suspend/resume continues skipped. See the separate [CPU overlap record](./guide.md#scene-cpu-ahead-2026-10-14) for identities and proof scopes.

Next, rotate sparse GPU samples across the twenty-five passes from the current 14.73 FPS full-default baseline within existing query/memory budgets, associating layer/stage/pass with owner+sequence+time. After identifying CRT, motion-blur or composite hotspots, use REA original shader/material/editor evidence to optimize the shared execution path while retaining all effects/resolutions/budgets. This attribution is planned. Prior sparse private GPU diagnostics suggest roughly 61 ms of aggregate draw/composite work, but they are not measurements of this production candidate or an exact/equivalent phase; use them only to select controlled follow-up measurements.

### Scene GPU pass investigation (2026-10-14)

From merged `4cb9fe14dcea8534a84f4198f3871af56c0a7139`, bounded private AMD query attribution completed with forty-six catalog sites, forty commonly sampled sites and the original twenty-one stage plus four bloom logical passes preserved. Layer 8 material/composite and original Simple Audio Bars had sparse means around 4.98 and 4.61 ms; three history-copy sites were around 1–2 ms and are GPU copy intervals, not CPU readback. Thirty-two queries/eight slots/512 records stayed bounded. Different-frame means are not summed, and CPU begin classification is not an exact GPU phase. The A/P comparison (15.0666 → 14.8500 FPS, −1.4381%) includes temporary hooks and sampling in different binaries, not an optimization gain.

The scalar material attempt was rejected for no benefit (15.132597 → 14.833289 FPS, observed −1.977906%), independently of the earlier broader integer Count=3 one-LSB precision rejection. Both were restored/archived and are not delivered or reused as Blend acceptance evidence.

The implemented shared compiler path matches only complete canonical LWE-owned image/text source and metadata, compiling fixed integer Blend 0/1/2 as a typed constant. Other templates/modes retain uniforms; authored formulas/parameters, ColorMode, prepared source/plan, resources/copies, effects/resolutions, runtime inputs, workers/slot and 512 MiB limits remain. Independent release A–B–B–A observed **14.783306 → 15.365691 FPS (+3.9395%)**, with ranges 14.7666–14.8000 → 15.1666–15.5647. Two thirty-second runs per variant and unsynchronized private tone/silence epochs do not establish stable/general/significant gain. Paused/resume/clear/join/identity gates passed; CPU-wall means and old GPU-site means are not combined or added to prior percentages.

Final source checks passed: 562 ordinary tests (including two doctests), 37 ignored, plus sixteen explicit GPU tests (one new AMD template test with twelve internal cases, six ahead, six history, two sharing, one full-original software oracle). Eight output/twenty-four history pixel hashes match #110; CPU/GPU bounds and zero cleanup remain. Full software, small AMD fixtures and nested native throughput are separate scopes. Separate native engine-only pause/resume, paused static replacement with one draw/swap, idle/clear/join, 1920×1080 capture and owned helper/audio-default/focus restoration checks also passed. See the independent [GPU pass record](./guide.md#scene-gpu-passes-2026-10-14). Previous evidence remains unchanged; no 30 FPS/daily-use, physical/UI/package/Windows or long-soak acceptance is claimed, and physical suspend/resume stays skipped.

Next, prioritize existing Named/History source FBO reuse for history copies while retaining Input fallback, ownership and replay/rollback behavior. Full-frame blits already use existing FBOs; assess their input liveness separately before changing execution. Layer 8 ColorMode 28 and original Simple Audio Bars remain targets while preserving artwork formulas, full effects/resolutions and budgets. This is planned work, not a completed copy optimization.

### Scene history-copy FBO reuse (2026-10-14)

From merged #111 `5df7317b9cbe85608cc848b811c74a217d042b1a`, the implementation borrows validated existing READ FBOs only for Named/History copy sources. Input/alias-preservation and other sources retain temporary attachments; full-frame blits already used existing FBOs. Every blit, `NEAREST`, order, dimension, resource/generation/history owner and 512 MiB limit remains. Three new opt-in regressions cover exact temporary-vs-reuse/restored pixels, advancing history with pause/resize/freeze-thaw/resume, and pre-GL feedback/dimension rejection. Final source checks passed: 562 ordinary tests including two doctests, 40 ignored, plus nineteen explicit GPU tests (three copy, one owned template, six ahead, six history, two sharing, one full-original software oracle). Ignored tests are not passes. Full-original software retains 10/5/25, eight output/twenty-four history hashes strictly matching #111/#110, CPU/GPU bounds below 512 MiB and zero final cleanup.

Independent native release A–B–B–A observed **14.716643 → 15.233300 FPS (+3.5107%)**, with ranges 14.6000–14.8333 → 15.1666–15.3000 and both B runs above both A runs. Two thirty-second runs per variant and unsynchronized private tone/silence epochs do not establish stable/general/significant gain; percentages are not added to earlier iterations. Pause/resume/clear/join/identity gates passed. CPU wall is not GPU elapsed and unrelated older site means are not summed; native throughput, small AMD and full software proof remain separate, and B1 geometry is only a mid-batch snapshot. Every pixel blit still executes; only eligible source-FBO setup is reduced.

A separate native engine-only check passed pause/resume, paused static replacement with one draw/swap, idle/clear/join, 1920×1080 capture review and owned helper/audio-default/focus restoration through thirty-one functional and fifteen cleanup checks; this does not establish UI/session-save or continuous per-run observation.

New original-PE REA evidence retains thirty-one static records for generic target-handle caching, named lookup-before-create and dimension-dependent resize. It does not establish original History/view/copy semantics, OpenGL behavior or GPU/FPS cost; LWE validation is separate, and original decompiled source is not imported. See the independent [history FBO record](./guide.md#scene-history-fbo-2026-10-14). Next, continue bounded per-pass attribution for Layer 8 ColorMode 28 and original Simple Audio Bars while preserving authored formulas, full effects/resolutions, resources and budgets; no further gain is assumed. Earlier evidence remains unchanged; no 30 FPS/daily-use, physical/UI/package/Windows or long-soak acceptance is claimed, and physical suspend/resume stays skipped.

### Scene draw attribution and FullFrame source reuse (2026-10-15)

Merged #112 `71b69f5497929b70d013694f6315d5020049dd41` is the new baseline; earlier dated records remain unchanged. Original-PE REA retained sixty static Evidence records (twenty-nine new, twenty-three instruction anchors), observing dirty-gated material updates, separate dynamic records and 128-byte packing per 32-band channel. It did not execute Wallpaper Engine/Wine or establish Windows upload cadence, API names, active ranges or GPU/FPS costs. No original source or private diagnostic hook is imported into production.

The separate, archived/restored two-site AMD/GL4.6 diagnostic completed with thirty-two queries/eight slots/512 records and unchanged 512 MiB image gates. Both measured targets are 3840×2160 inside a native 1920×1080 output. From 210 guarded animated records, Audio Bars/Layer 8 DrawArrays-only completion means are 4.579791/5.159573 ms; Audio's CPU spectrum loop mean is 0.706683 µs, with two linked 32-float arrays active (256 API payload bytes). Location−1 attempts are not uploads. Clear retains its 807.92 µs outlier; different-frame GPU/CPU means are not a summed critical path or an FPS prediction, and CPU/GPU clocks are not subtracted.

The different-binary A-off/P-on/P-on/A-off control observed 14.799973 → 14.949973 FPS (+1.0135%), not a production optimization or proof of zero instrumentation overhead. A1's two animated empty attempts and P2's one cleanup-discarded sample remain explicit; query deletion, paused/resume/clear/join and identity checks passed without claiming all-zero empty/drop counts. Two short runs per variant do not establish stable or significant effects. See the independent [draw-attribution record](./guide.md#scene-material-bind-2026-10-15). Authored formulas, full effects/resolutions and budgets remain required; broader runtime/Windows/UI/package/endurance acceptance is not implied, and physical suspend/resume remains skipped.

A separate baseline shader capture obtained actual NIR/ACO ISA after the initial `ps`-only capture failed; child-only `ps,nir,asm,stats` and cache-disable requests are retained separately from the harness environment subset. Program numbers plus unique interfaces/data flow corroborate Audio/Color attribution across different processes, without a same-process source-SHA/link marker; `source_blake3` joins NIR to ISA only within the dump. Audio already has four compact dynamic buffer loads, not a 32/64-element array select chain, and background Color hue/saturation is already eliminated. Final ISA reports Audio/Color VGPR 32/24, code 856/948 B, zero scratch/spills and Max Waves 16; these are not occupancy, cache-miss or instruction-cost measurements. Explicit UBO migration or redundant HSL simplification has no demonstrated benefit, and the debug capture is excluded from throughput comparisons.

A separate first-bind non-`g_` material-constant candidate (`439fb7…`) passed its recorded 562 ordinary/42 ignored and twenty-one explicit GPU checks, with exact eight-output/twenty-four-history hashes/metadata and unchanged 512 MiB bounds/zero cleanup. Its independent release A–B–B–A observed 15.2666388412 → 15.2999669087 FPS (+0.2183%), with overlapping ranges and a time trend. No useful gain was established, so the renderer/test changes were archived/restored and this candidate is rejected, not a delivered material optimization. These checks are distinct from the retained #112 baseline and the private A/P diagnostic.

The retained FullFrame source-reuse candidate (`73aa35…`, renderer `50fef0…`) borrows current composition only for staged/non-direct layers without an authored composite and with `work_offset >= 2`; reads finish before the final copy compositor writes. Other cases keep snapshots. It changes no shader math, graph, dimensions, allocations or 512 MiB gates and uses no layer-ID exception. The recorded 562 ordinary/42 ignored and twenty-one explicit GPU checks passed; a final two-test sentinel rerun proves the snapshot stays untouched and preserves strict RGBA/history/replay/fallbacks, without adding unique-test counts. The original oracle's entire JSON matches #112, including eight outputs/twenty-four histories/metadata and unchanged CPU/GPU peaks/zero accounting.

Its separate profiling-disabled native A–B–B–A measured A 883 swaps/60.000115738 s = 14.7166382788 FPS (range 14.566637–14.866640), B 982/60.000110696 = 16.3666364713 (15.933304–16.799968), observed **+11.2118%**. Both B runs exceeded both A runs; the change is retained for this batch, removing only two original Miku pre-layer helper snapshots (layers 2/9), with all twenty-five logical passes preserved. Successful render+swap CPU wall mean was 3.969689 → 3.525623 ms, not GPU time. Paused counters/dispatch/render CPU were zero; animated/resumed empty counts, resume/clear/join, binary/input/private-environment and budget gates passed. This is two thirty-second runs per variant with unsynchronized twelve-second PCM epochs, not stable/general/significant-gain proof or a percentage to add to previous batches.

Separate same-binary native checks passed original animation/pause/resume, paused static replacement with exactly one draw/swap, zero static-playing counters/dispatch, clear/join and owned child exits. A reviewed 1920×1080 default Miku/CRT capture establishes one visible frame, not Windows parity. Exact owned compositor/audio cleanup, supervisor exit status 0, removed PIDs/window, matching host-default before/after raw readbacks and restored focus passed; the inter-batch host snapshot is one observation, not continuous/per-run visibility or audio proof. Next, verify hardware-counter capabilities and perturbation control before separating sampling/writeback traffic from arithmetic cost; avoid repeating the unsupported UBO or already-eliminated background-HSL proposals. Physical suspend/resume remains skipped.

The additional full-original fixed-spectrum AMD offscreen baseline/candidate comparison also matched all eight outputs/twenty-four histories, metadata, replay and unchanged resource/cleanup records at 2160×1440 output with original 4K composition targets. These two executions of the existing test are separate from software and live-PCM native evidence, and do not add unique test cases or establish Windows parity.

### Work sequence and exit criteria

The table retains full future acceptance targets; its physical suspend/resume target is explicitly deferred from the current batch. Bounded P0 inventory, the first P1 runtime/settings slices, restricted P2 camera and editable-property persistence, and the default mixed-project CPU/GPU path are implemented. Default and property-editor native UI flows, including real save failure/retry and a twelve-second owned fullscreen recovery, passed; broader lifecycle acceptance remains open. No complete stage or daily-use milestone is marked finished. Unsupported behaviors remain gated, and implemented paths still require their stated acceptance.

| Stage | Deliverables | Exit criteria |
| --- | --- | --- |
| P0: establish correctness | Use the bounded inventory for independent declarations/blockers with structural locations; distinguish declarative user properties from JS and reachable from unused packaged resources. Create authored fixtures and original Wallpaper Engine reference captures. Validate the real Tauri Library → monitor selection → Apply → Desktop feedback → close/reopen flow. | All 11 projects have a dependency report with explicit coverage, exhausted budgets, and remaining unknown/dynamic references. Record reference conditions and assess camera, coordinate/UV orientation, alpha, masks, layer order, blending, and timed effects. Verify visible application and saved restore through the real UI for Shine, preserving ordinary desktop focus/clicks/workspace behavior; document the unavailable original reference environment. A static inventory does not certify exhaustive runtime dependencies. |
| P1: make the runtime dependable | Implement actual frame deadlines, stop unnecessary static/paused callbacks, connect playback/power settings, and define pause/lock/suspend policies. Budget resources across outputs and current/candidate/rollback renderers. Add observable failure states, bounded recovery, and measurements. | Pass 100 Scene ↔ Video switches, a 24-hour Scene soak, and 10 suspend/resume cycles without state loss, stale frames, duplicate audio, or accumulating resources. Record frame times, wakeups, CPU/GPU load, and memory trends. A configured 30 FPS limit must control presentation; use p95 render time ≤33.3 ms as the initial 30 FPS budget on the recorded 680M machine. A 60 FPS claim requires its own ≤16.7 ms budget and presentation measurement. Paused clocks must stop and resume without a catch-up jump. |
| P2: complete ordinary 2D semantics | Retain the restricted canonical-camera branch established by the original static loader; expand coordinate transforms, hierarchy, size/texture flags, depth/order, and materials only with evidence. Add declarative property binding and common water, opacity, scrolling, and sway effects needed by selected projects. | Compare `3258032485` and the Shine baseline with original-engine reference frames before claiming visual parity or daily-use readiness. Static native evidence supports the limited camera gate change; it does not replace the visual comparison. Property changes update the intended object/effect and persist where applicable. Each added feature needs positive rendering and unsupported/error fixtures; video/Shine behavior must remain correct. |
| P3: complete real mixed projects | Complete and validate `3659880761` properties, particle/material/timing, effects, LDR bloom and audio. Add text/fonts, animated TEXS/video, HDR and other resources when the next whole project requires them. Optimize FBO lifetimes and reuse before admitting larger projects. | Preserve the authored default: six visible roots plus all five children, every active effect, default bloom and both authored loop tracks/gains. Reactive remains author-hidden; enabling its FFT behavior requires a real audio-analysis implementation. Bindings and audio survive edit, pause, switch and restore. Record lifetime, alpha/size/motion, materials and limits. Turning off an authored active feature does not qualify playback; forcing an author-hidden layer on is not required. Original-engine noise/child/rope and complete-frame A/B, actual suspend/lock and daily-use checks remain outstanding. |
| P4: SceneScript and interactive behavior | Pure scalar CPU preparation and the new bounded dynamic image/audio worker use QuickJS and Oxc AST validation; retain explicit blockers for broader ECMAScript/Wallpaper Engine APIs and Rust ownership of application/rendering. Implement script lifecycle, property changes, scene/layer/effect access, required input/audio/time APIs, and scoped storage. Define execution, memory, cancellation, and failure boundaries. | The actual scripts in selected projects pass behavior fixtures and original-engine comparisons. An infinite loop, exception, excess allocation, or worker death cannot freeze the UI or corrupt saved state. Required platform inputs have a verified source; missing APIs or inputs remain explicit blockers. Test a complete script-bearing project before expanding the API surface. |
| P5: advanced migration and release | Implement remaining model/puppet and advanced render dependencies needed by the initial corpus; use a separate authored 3D fixture before claiming 3D support. Exercise real multi-monitor lifecycle and an installable candidate, then expand the corpus across independent project styles and GPU families. | Reach 11/11 complete initial migrations before declaring that migration milestone. An expanded 2D candidate requires at least 10 complete real 2D projects from at least three distinct content/feature profiles, plus at least 20 positive/negative corpus cases. Record exact supported projects and capability combinations; run full CI, package validation, fresh-user startup, and the release smoke checklist. |

The initial numerical targets above are provisional engineering gates. Calibrate resource budgets and frame targets from baseline measurements without weakening correctness or retaining an unmeasured “supported” claim. Define the sample list before acceptance; do not replace failing projects with easier ones to improve a percentage. Unsupported corpus cases exercise rejection and do not count toward the ten complete projects.

### Runtime and platform decisions

- Preserve the apply transaction: prepare → first frame → commit → atomic assignment save → finalize. Candidate failure must restore the prior source; stale cancellation must not undo a finalized assignment. Report desired assignment, active source, and recovery failure separately.
- Continue attribution of the observed RSS rise after the resource diagnostics. CPU prepare/drop and backend-stop Rust live counts return to small fixed baselines, while video/mixed RSS rises alongside retained glibc free blocks. The controlled arena child supports investigating allocator retention, not a production memory fix or a claim that native memory is leak-free. Keep shell caches, driver allocations, swap/file pages, and longer-term bounds in scope. P1 daily-use resource stability remains unaccepted; the logical RGBA/FBO budget is not an RSS or VRAM cap.
- Extend the existing global Scene budget to subsequent resource types as well as per-project limits. Count two outputs and temporary old/new renderers together. Bound decoded assets, textures, render targets, particles, and scripts. Avoid allocating full-resolution scratch targets for unused passes; raise existing layer/pass limits only after representative measurements.
- Separate immutable resources from per-output property state. Validate defaults, types, ranges, and bindings before evaluating them; keep compatibility/cache identities aligned with values that affect required capabilities. Persist user choices by project/output through the existing serialized atomic-write path, preserving unrelated assignments. Treat embedded audio playback and desktop-audio capture/FFT as separate capabilities.
- Validate static, paused, battery, fullscreen/occlusion, screen lock, output disconnect, and suspend/resume behavior separately. Compositor-dependent policies need a supported capability source; document unavailable policies. Check long-running shader time precision against intended phase behavior rather than applying an arbitrary time modulo to every shader.
- Use a prototype to choose process ownership before broad custom scripting/shader support. A script/simulation worker can send bounded updates to a Rust renderer but cannot isolate GPU/context faults. A renderer process needs explicit output/surface ownership, request generations, first-frame readiness, cancellation, resource transfer, and restart contracts. Preserve the apply transaction and avoid two active audio sources. Driver-wide GPU failure is not guaranteed recoverable through process isolation.
- Wallpaper Engine [SceneScript](https://docs.wallpaperengine.io/en/scene/scenescript/reference.html) uses ECMAScript 2018 and engine APIs. Implementing Rust rendering independently does not require writing a JavaScript interpreter. Evaluate engine compatibility, interruptibility, host API behavior, and memory limits in a prototype before choosing a dependency. [User properties](https://docs.wallpaperengine.io/en/scene/userproperties/overview.html) include declarations that can be supported without enabling all SceneScript behavior.
- Wayland [pointer events](https://wayland.freedesktop.org/docs/html/apa.html#protocol-spec-wl_pointer) concern the focused surface. Receiving those events on a wallpaper is not proof of a global cursor source while other windows have focus. Establish a compositor-specific capability path for cursor parallax/interaction; preserve normal desktop input and do not silently fake required interaction.

After static or paused rendering settles, observe a 60-second interval without a self-sustaining draw/swap/frame-callback loop; resize and explicit restoration remain valid redraw triggers. For faults, inject shader compilation, allocation, swap/context, worker exit, and save failures at their implemented boundaries. The UI must remain responsive, retain the last saved assignment, show the actual active/recovery state, and permit a subsequent successful apply. A saved wallpaper is not guaranteed to remain visible after an unrecoverable GPU failure, so recovery claims need their own evidence.

### Earlier acceptance evidence and implementation batches

Use self-authored, redistributable fixtures for automated resource/rendering tests; keep Workshop projects and installed engine assets local. Record hashes, feature inventories, and comparison results without committing their copyrighted payloads. Use original Wallpaper Engine as the behavior reference; OWE and reverse-engineering observations can inform implementation but cannot replace that reference. Fixed-time captures need controlled settings, audio/input, camera, and random seeds where possible; compare visible structure and motion as well as image metrics.

The **P0 plus first P1/P2 batch** has added bounded inventory, restricted camera normalization from REA observations of the original `wallpaper64.exe`, validated direct property defaults, per-output scheduling, logical payload accounting, counters, and opt-in `scene_probe`/`scene_resources`/`scripts/scene-ui-smoke.py` tools. The 11-project report and complete native-pointer UI flows for `3337606455` and `3258032485` are recorded; the 30 FPS completed-swap/60-second paused/static idle probe passed under frequent status requests. The preceding 100 mixed-source lifecycle cycles passed, while their process RSS rose.

The [contributor follow-up](./guide.md#2026-10-10-development-record) records the EGL cleanup and single initial video load, persisted/live FPS and battery implementation, and separate resource diagnostics: 30 CPU, 30 Scene, 30 video, 50 mixed, and 20 backend-restart cycles passed. CPU release returns to 2,177 Rust live bytes; every stopped backend in the restart run returns to 2,229 bytes, four FDs, and one thread. Video clear Rust live bytes remain 49,259, while the mixed run's retained glibc free blocks rise from 166,546,944 to 654,284,608 bytes. A fresh diagnostic child with `MALLOC_ARENA_MAX=2` passed 100 mixed cycles with a smaller free-block increase; different cycle counts, memory pressure, swap, and file-page residency prevent treating that comparison as a production RSS benefit. The 2026-10-12 matched 30-cycle diagnostic identifies freed-page retention: final cleared RSS was about 592.5 MiB without reclaim and 240.8 MiB with explicit reclaim (2.4–4.9 ms per boundary). GNU builds now reclaim large freed native heaps only after actual output/candidate/rollback release, with a 64 MiB free-block threshold; frame rendering and allocator arena policy are unchanged. This controls retention and does not prove absence of native/driver leaks. The new 20-cycle `3659880761` → video → `3659880761` → clear probe passed with fixed clear FDs/Rust live bytes and a joined shutdown; RSS and allocator retention still prevent a memory-stability or native-leak-free claim.

The real desktop-service settings test passed at about 14.70/29.60 completed swaps per second for saved 15/30 FPS limits, retaining the backend and assignment. Its battery preference check ran on AC, without a hardware transition. The separate native English Settings 15 → 30 FPS/save/restart flow and Chinese layout review passed in `ui-settings-verified`; the full apply/hide/new-process restore/clear flow and the then-current bloom blocker also passed with clean cleanup.

The mixed-project follow-up implements bounded particles/children, WaterRipple/Iris, four-pass LDR bloom and single-FLAC looping audio, using independent Rust data structures and Open Wallpaper Engine only as a behavior reference. Native evidence establishes RGBA8 bloom targets with floor/max-two dimensions, full-resolution texel size and four-pass ordering, plus single-entry loop without random interval and squared layer gain. `3659880761` worst-case particles/vertices/work are 3,965/26,790/30,980. Its 30 FPS engine-only probe achieved about 27.95 swaps/s, paused for sixty seconds at zero draws/swaps/callbacks, and resumed at about 28.33 swaps/s; logical GPU payload was 134,862,328 bytes. Private Pulse PCM checks are separate from physical speakers or full service transactions.

Project/output properties use the same declaration checks and preparation gate, then first-frame/commit acceptance → atomic assignment plus choices → finalization/audio activation; before atomic save, failed preparation, candidate acceptance or save retains the previous renderer and choices; a successful durable save remains authoritative. The separate default `3659880761` native UI Apply/Running/hide/restart/restore/clear and Settings flow passed, preserving PID/assignment bytes for live updates and preferences across new-process restore; English/Chinese captures were reviewed. Native property bool/slider editing, real PermissionDenied rollback, retry, new-process choices/controls restore and full-default reset passed. The twelve-second owned fullscreen cover/reveal retained Running and evolving particles; callbacks were not measured, so it does not replace actual suspend/lock acceptance. The isolated copied-engine/Wine-prefix attempt failed before reference capture (`c0000005`); cleanup succeeded and source hashes were unchanged. Original-engine visual A/B remains unavailable. These finite checks do not pass P1 or daily-use readiness: 24-hour soak, ten actual suspend/resume cycles, hardware battery transitions, measured power/VRAM, multi-monitor and installed-package acceptance remain open. That earlier batch did not add broad SceneScript APIs; the current bounded Yakuza extension is recorded separately above.

The first local daily-use milestone is explicitly single-monitor. Before claiming multi-monitor support, validate two physical outputs with different wallpapers, independent clear, mixed scale/resolution, hotplug, restart restore, and one-output failure isolation. Before extending GPU claims, repeat representative rendering and recovery on another GPU family. The present machine proves neither multi-monitor behavior nor cross-vendor compatibility. If hardware or an original-engine reference is unavailable, retain that acceptance item as unverified instead of substituting a mocked test. A distributed daily-use candidate also needs installed-package acceptance; a service test or development build alone is insufficient.

Only freeze a candidate scope after these gates pass. Source checks, package creation, publication, and installed-runtime acceptance are separate outcomes; publish only through the authorized release workflow.

The version sections from v0.7.0 through v0.9.10 below record scope and acceptance requirements at those stages. Their video-only restrictions are historical and do not override the current experimental Scene scope. Release availability remains determined by published artifacts.

## v0.7.0: support matrix lockdown

Theme: make product truth consistent everywhere.

Deliverables:

- Authoritative support matrix:
  - video is the runnable first-release target;
  - scene is recognized for metadata and compatibility reporting only;
  - web is recognized for reporting only;
  - other/application content is unsupported for runtime workflows.
- Compatibility policy, Library projection, Workshop detail, Desktop apply, and docs aligned.
- Scene no longer appears as fully supported or ready to apply.
- Apply is enabled only for runnable video Library items, or disabled with an explanation for recognized-only items.
- English and Simplified Chinese docs updated together.

Acceptance criteria:

- Synced video reports runnable support.
- Synced scene reports recognized/runtime-unsupported support.
- Web reports unsupported runtime status.
- Desktop apply rejects non-video with a clear reason before backend invocation.
- Tests cover video, scene, web, missing metadata, and missing primary asset behavior.
- CI tests do not require Steam or a real desktop session.

Must not claim:

- scene runtime support;
- web runtime support;
- general Wayland support;
- compositor support beyond the verified Wayland + `niri` path.

## v0.8.0: reliable video runtime on `niri`

Theme: make the current runtime path dependable.

Deliverables:

- Harden video apply to a selected monitor.
- Harden per-monitor clear.
- Preserve wallpapers on other outputs when clearing one monitor.
- Restore saved assignments on startup.
- Improve Desktop runtime status for active outputs, stale state, restore failures, and backend initialization/runtime errors.
- Keep a documented manual real-desktop validation checklist for runtime changes.

Acceptance criteria on verified Wayland + `niri`:

- LWE discovers active monitors.
- A synced video wallpaper applies visibly to one monitor.
- Multi-monitor apply works when hardware is available.
- Clearing one monitor does not stop wallpapers on other monitors.
- Restart restores saved assignments or reports explicit restore failure.
- Missing video file, output mismatch, Wayland layer-shell failure, EGL failure, and backend timeout produce actionable errors.

Must not claim:

- runtime reliability outside `niri`;
- runtime support for scene or web;
- CI-verified desktop runtime behavior unless a verified real-desktop runner exists.

## v0.8.5: Library workflow hardening

Theme: make Library trustworthy for daily use.

Deliverables:

- Library cards and detail panels clearly show runtime status.
- Apply controls are gated by runnable state and monitor availability.
- Assigned monitor labels stay accurate after apply, clear, and restore.
- Refresh/apply/clear flows preserve user context where possible.
- Empty, stale, unavailable, and partial states are understandable.
- Large-library pagination and filtering remain predictable.

Acceptance criteria:

- Runnable video items are easy to identify and apply.
- Recognized-only scene/web items, if shown in Library, cannot be applied and explain why.
- Failed apply does not replace populated detail UI with a dead state.
- Library command/unit tests do not depend on Steam being installed.
- Library state updates correctly after Desktop invalidations.

Must not claim:

- all Library items are runnable unless the UI enforces that;
- recognized scene/web items can be made runnable merely by resyncing.

## v0.9.0: Workshop and sync clarity

Theme: make discovery and acquisition expectations honest.

Deliverables:

- Workshop search and filtering polished around Steam Web API key setup.
- Local Workshop refresh explains synced, missing `project.json`, missing primary asset, and unsupported runtime type states.
- “Open in Steam” clearly acts as the acquisition/subscription handoff.
- Settings clearly shows Steam integration state.
- Online search results do not imply local sync or runtime support.

Acceptance criteria:

- Missing Steam API key points users to Settings.
- Steam not installed, Wallpaper Engine missing, and no Workshop content produce distinct messages where possible.
- Malformed project metadata does not fail the whole catalog scan.
- Scene/web search results are labeled as recognized/unsupported for runtime.
- Network and parsing tests avoid real Steam API calls.

Must not claim:

- LWE directly downloads Workshop content;
- search result availability means local content is synchronized;
- Steam metadata inference is authoritative.

## v0.9.5: diagnostics, persistence, and release candidate prep

Theme: prepare for supportable v1 release candidates.

Deliverables:

- User-copyable diagnostics surface.
- Settings persistence verified for language, theme, Steam API key, launch-on-login preference, and Workshop filters.
- Sensitive values masked.
- Release smoke checklist added to docs.
- Package and release artifact expectations documented.

Diagnostics should include:

- LWE version and package type when available;
- OS, session, and compositor hints where available;
- monitor discovery result;
- runtime backend status or last backend initialization error;
- Steam discovery result;
- Wallpaper Engine Workshop content availability;
- Library counts by compatibility/runtime state;
- current support-scope reminder.

Implementation note: package type is reported as unknown/local build unless provided by the package manager or reporter; diagnostics should not claim perfect environment detection.

Acceptance criteria:

- Settings persist across restart.
- Diagnostics do not expose the Steam API key.
- Launch-on-login unavailable state is non-fatal and visible.
- `.deb`, `.rpm`, `.AppImage`, AUR stable, and AUR git release paths are documented.
- Required CI checks are documented and passing.
- Manual runtime validation is required for runtime-affecting release candidates.

Must not claim:

- diagnostics are perfect environment detection;
- launch-on-login works on every desktop/session/package format;
- package install success guarantees runtime support.

## v0.9.6: preview performance and thumbnail cache

Theme: make Library and Workshop browsing smooth when many preview assets are present.

Problem statement:

- Current card grids can receive Workshop-bundled `preview.gif` and large `preview.jpg` files directly.
- Tauri converts local paths into asset URLs, but the frontend still asks WebView to decode and render the original media.
- Animated GIF previews are especially expensive when many cards are visible because each file can contain many frames.
- Wallpaper Engine's public creator guidance treats preview assets as small, bounded UI media. LWE should follow the same product principle without claiming undocumented Wallpaper Engine client internals.

Deliverables:

- Introduce an LWE-owned thumbnail cache for card-grid cover art.
- Use static, bounded-size thumbnails for Library and local Workshop card grids.
- Convert animated GIF previews to static first-frame thumbnails for card grids.
- Downscale oversized bundled previews before exposing them to card grids.
- Keep original preview media available only for detail views or explicit future preview actions, not for default grid rendering.
- Move thumbnail cache naming and storage away from retired `wayvid` terminology into the LWE cache namespace.
- Update Tauri asset protocol scope so the frontend can read only the required Steam Workshop and LWE thumbnail cache paths.
- Generate thumbnails before exposing card data where practical; when cold-cache work is unavoidable, preserve a known usable visual fallback and avoid indefinite placeholders for items that have preview media.
- Avoid full-list reprocessing on every item selection; reuse the current Library/Workshop projection where possible.
- Add lightweight frontend safeguards such as `decoding="async"` and stable image dimensions. Treat virtualization or hover-play previews as follow-up work unless static thumbnails are insufficient.

Implementation notes:

- Prefer reusing `crates/lwe-library` thumbnail code, but route it through the active Library/Workshop service and assembly flow instead of calling it from Svelte.
- Cache keys should account for the source path and enough file identity, such as modified time or size, so changed previews regenerate.
- Thumbnail failures should degrade to the existing placeholder rather than failing the whole Library or Workshop page.
- Generated thumbnails should be small enough for card grids; target dimensions around 256-320 px square are acceptable for the current UI.

Acceptance criteria:

- Library card grids do not render raw local `preview.gif` files.
- Library card grids do not render oversized local preview images when a generated thumbnail is available.
- GIF-heavy local Workshop content remains responsive when showing the largest supported page size.
- Warm-cache navigation back to Library does not regenerate unchanged thumbnails.
- Cold-cache thumbnail generation does not leave items with known preview media as permanent placeholders.
- Detail panels still show a cover or placeholder consistently.
- Unit tests cover thumbnail source selection, GIF first-frame behavior, cache-path naming, stale-cache regeneration, and placeholder fallback.
- Frontend tests cover rendering thumbnail cover paths without regressing missing-cover placeholders.
- Manual verification records a GIF-heavy Library before/after comparison on a real desktop session.

Must not claim:

- Wallpaper Engine's private client implementation details unless backed by official documentation;
- animated previews are fully supported in card grids;
- performance is guaranteed on every GPU, disk, or desktop environment;
- scene or web wallpaper runtime support;
- LWE-managed Workshop downloading independent of Steam.

## v0.9.7: Workshop browsing polish

Theme: keep the Workshop page responsive during navigation and filtering edge cases.

Deliverables:

- Restore cached online Workshop search results without skipping local Workshop and Library marker refresh work.
- Clean up pending debounced Workshop search timers when leaving the page.
- Bound online Workshop search result card widths so sparse result sets do not stretch one card across the whole content area.
- Keep loading skeleton grid dimensions aligned with the final result grid.

Acceptance criteria:

- Navigating to Workshop keeps the page shell and search controls responsive while cached or asynchronous data is restored.
- Returning to a cached Workshop search still schedules required local marker refreshes when those caches are stale.
- A single online search result keeps normal card proportions instead of becoming a full-width oversized card.
- `pnpm check` passes.

Must not claim:

- Workshop search results are locally synchronized;
- LWE directly downloads Workshop content;
- all compositor or WebView performance issues are fixed.

## v0.9.8: background runtime power profile

Status: completed for the v0.9.8 release. The release includes lower idle render work, cached monitor discovery, warm Workshop/Library snapshot reads for normal page load and restore paths, Rustls-backed Workshop HTTP, and lightweight snapshot source diagnostics. Real desktop power measurements remain environment-specific and should not be generalized beyond the verified Wayland + `niri` setup.

Theme: reduce CPU wakeups and unnecessary work while LWE is running in the background, especially with active video wallpapers.

Problem statement:

- The current engine loop wakes on the frame interval, scans active surfaces, and re-arms frame callbacks for rendered outputs.
- The mpv render path checks for new frames and drains mpv events in the hot path.
- Monitor discovery can shell out to `niri msg -j outputs` for each request.
- Library and Workshop projections can trigger full catalog refresh and compatibility assessment when a cheaper cached view would be enough.
- Security scanning currently reports a `glib` advisory through the Tauri Linux GTK/WebKit stack. This should be tracked with upstream Tauri/GTK movement; do not paper over it with unsafe dependency overrides.

Deliverables:

- Measure baseline idle/background CPU wakeups and CPU usage for:
  - app open with no wallpaper running;
  - one active video wallpaper;
  - minimized/background app with an active wallpaper;
  - Library/Workshop refresh after warm cache.
- Add lightweight runtime diagnostics or logs that expose frame pacing, skipped frames, active surface count, and backend event-loop state without increasing normal overhead.
- Reduce per-frame engine work where possible:
  - avoid rendering when mpv has no new frame and no surface resize/state change is pending;
  - avoid redundant EGL make-current/resize/swap work for unchanged surfaces;
  - keep frame callbacks tied to visible/active wallpaper surfaces only.
- Make monitor discovery cheaper by adding a short-lived cache or explicit invalidation path before calling `niri msg -j outputs` repeatedly.
- Separate cheap snapshot reads from expensive Workshop/Library full refreshes so page loads and restore flows can reuse warm projections.
- Keep security dependency hygiene current:
  - use Rustls-backed Workshop HTTP requests instead of OpenSSL-backed native TLS;
  - monitor Tauri/Linux GTK dependency updates for a supported path away from vulnerable `glib` 0.18.x.

Implementation notes:

- Treat power work as runtime-sensitive: prefer small, measurable changes and keep the current `niri` video path reliable.
- Use real desktop observation for power claims. CI can cover invariants and regressions, but it cannot prove compositor/GPU power behavior.
- Avoid changing supported runtime scope while optimizing; scene and web wallpapers remain recognized-only unless separately implemented and verified.
- Avoid unsafe Cargo patching of Tauri's GTK stack just to silence scanners. Upgrade Tauri or remove affected features only when behavior and packaging remain validated.

Acceptance criteria:

- Baseline and after-change measurements are recorded for the verified Wayland + `niri` setup.
- An active video wallpaper does not regress apply, clear, restore, or multi-monitor behavior on the verified path.
- Idle/background app paths show fewer unnecessary wakeups or lower CPU usage in manual measurement.
- Monitor discovery and Library/Workshop load paths avoid avoidable repeated full refreshes during normal navigation.
- OpenSSL/native-tls no longer appears in `cargo tree` for Workshop HTTP requests.

Must not claim:

- battery-life improvements on every device;
- compositor support beyond the verified Wayland + `niri` path;
- scene/web runtime support;
- that upstream GTK/WebKit/Tauri vulnerabilities are fixed locally unless the dependency graph actually moves to fixed versions.

## v0.9.9: Wayland capability groundwork

Status: completed for the v0.9.9 release. The release adds a lightweight Wayland capability report, runtime gating before dynamic wallpaper startup, an explicit output-discovery source boundary that keeps `niri` as the augmented verified path, diagnostics for protocol support and output discovery source, and documentation for app-shell-only WebKitGTK startup workarounds.

Theme: make future desktop compatibility Wayland capability-driven instead of desktop-environment-specific.

Problem statement:

- LWE's current verified runtime path is Wayland + `niri`, but the underlying video runtime is better described as a Wayland layer-shell + EGL/mpv backend.
- Monitor discovery currently uses `niri msg -j outputs` as the only concrete backend, which makes output availability look desktop-name-driven instead of capability-driven.
- Other Wayland compositors may expose enough protocols to run the current lightweight runtime, but support should be decided by protocol capability and real validation, not by broad compositor claims.
- Some environments, such as GNOME/Mutter Wayland, can run the app shell with WebKitGTK workarounds but may not expose the layer-shell protocols required for dynamic wallpapers.
- LWE should not add X11 runtime support or heavyweight desktop-environment-specific dependencies while preparing broader Wayland compatibility.

Deliverables:

- Add a lightweight Wayland capability report that checks:
  - whether a Wayland session and display connection are available;
  - whether required globals such as `wl_compositor`, `wl_output`, and `zwlr_layer_shell_v1` are exposed;
  - whether dynamic wallpaper runtime support is available, degraded, or unsupported;
  - a user-readable unsupported reason when a required protocol is missing.
- Reframe monitor discovery as output discovery:
  - keep `niri` as an augmented output information source for the verified path;
  - prepare the service boundary for generic Wayland output discovery where stable output identity is sufficient;
  - avoid treating desktop environment names as primary runtime backends.
- Gate Desktop apply/restore paths with the capability report before starting the runtime backend, returning graceful unsupported results when required Wayland protocols are unavailable.
- Extend diagnostics with Wayland capability data:
  - session hints;
  - protocol availability;
  - output discovery source;
  - dynamic wallpaper runtime support status and unsupported reason.
- Document the GNOME/Mutter WebKitGTK startup workaround `WEBKIT_DISABLE_DMABUF_RENDERER=1` as app-shell-only mitigation, not as wallpaper runtime support.
- Keep support scope honest in English and Simplified Chinese docs.

Implementation notes:

- Keep the core lightweight: do not add desktop-environment-specific DBus/plugin integrations unless a future plan explicitly justifies them.
- Do not plan or implement X11 wallpaper runtime support for v1.
- Prefer a shared Wayland capability probe in `lwe-engine`, because that crate already owns the Wayland client dependencies and registry code.
- Use compositor names only for diagnostics, known-issue hints, and manual verification records.
- A compositor that lacks `zwlr_layer_shell_v1` should fail gracefully before runtime startup instead of attempting unreliable fallback windows.
- The verified path remains Wayland + `niri` until other Wayland capability combinations are manually tested.

Acceptance criteria:

- Diagnostics show a Wayland capability report on the current device.
- On the current verified `niri` setup, video apply, clear, restore, and monitor discovery still work.
- When required Wayland protocols are unavailable, Desktop apply/restore returns a clear unsupported reason before starting the wallpaper runtime.
- Documentation distinguishes app shell startup workarounds from dynamic wallpaper runtime support.
- CI covers capability-report formatting and unsupported gating without requiring a real compositor.

Must not claim:

- X11 runtime support;
- GNOME/KDE/Hyprland/sway runtime support by name without explicit verification;
- that app shell startup compatibility implies dynamic wallpaper runtime compatibility;
- scene/web runtime support;
- that every Wayland compositor exposing layer-shell is automatically verified.

## v0.9.10: AppImage packaging repair

Theme: repair AppImage packaging and make release validation reproducible. Check GitHub Releases for availability; record desktop and public artifact acceptance separately for each release.

Deliverables:

- Upgrade the Tauri and frontend maintenance dependencies without changing the product scope; pin the Rust build toolchain and use `.tauri-cli-version` as the sole Cargo CLI version source.
- Verify the exact CLI after cache restore in stable and prerelease workflows.
- Reject invalid AppImage `.DirIcon`, desktop entries, and icon links before publishing artifacts; cover the validator and CLI selection with lightweight regressions.
- Publish a stable patch release only after the required checks, validate its public AppImage, and request AppImage directory retesting against that version.

Acceptance criteria:

- Full Quality Check and packaging regressions pass on the release commit; `.deb`, `.rpm`, and `.AppImage` build successfully.
- The built and downloaded public AppImage both pass package validation without links to the build machine.
- Installation, window/tray behavior, Workshop/Library IPC, and video apply/clear/restore are checked separately on `niri` with isolated configuration.
- Upstream AppImage directory tests select the new stable artifact and pass; directory inclusion still depends on upstream review.
- Video on Wayland with `niri` remains the documented verified target; the toolchain update does not imply new Scene/Web or compositor support.

## v1.0.0-rc.1: first release candidate

Theme: freeze scope and validate end-to-end.

Deliverables:

- Feature freeze except blockers.
- Full docs review in English and Simplified Chinese.
- Release notes draft with explicit support scope.
- Manual `niri` runtime acceptance pass.
- Known limitations page or section updated.

Acceptance criteria:

- Full CI suite passes:
  - `pnpm check`
  - `pnpm test`
  - `pnpm build`
  - `pnpm docs:build`
  - `cargo fmt --all -- --check`
  - `cargo clippy --workspace --all-targets -- -D warnings`
  - `cargo check --workspace`
  - `cargo test --workspace`
- Fresh install → Settings → Workshop/Library → Apply video → Clear → Restart restore path manually validated.
- Eligible Scene apply/clear, Scene ↔ Video switching, and restore are checked separately; blocked Scene and web items stay non-runnable with an explanation.
- No user-facing copy claims unsupported runtime behavior.
- Release artifacts build successfully.

Must not claim:

- v1 final stability;
- broad compositor support;
- full Scene parity, Scene features beyond the validated 2D subset, or web runtime support.

## v1.0.0-rc.2: optional blocker-fix candidate

Theme: only fix release blockers found in rc.1.

Allowed work:

- data loss fixes;
- broken install or launch fixes;
- broken video or accepted experimental Scene apply/clear/restore fixes on verified `niri`;
- incorrect support-scope claim fixes;
- package or release artifact blocker fixes;
- severe documentation mismatch fixes.

Acceptance criteria:

- rc.1 blockers are resolved.
- No new feature scope is added.
- Manual validation is rerun for affected areas.

Must not claim new capabilities beyond rc.1 scope.

## v1.0.0: stable v1

Theme: reliable, honest first stable release.

v1 deliverables:

- Video Wallpaper Engine wallpapers runnable on the verified Wayland + `niri` path.
- Scene compatibility reporting with experimental playback for individually validated 2D items; other Scene features and web playback remain unavailable.
- Library-first daily workflow.
- Workshop discovery/acquisition handoff through Steam.
- Desktop monitor apply/clear/restore flow.
- Settings persistence and diagnostics.
- English and Simplified Chinese maintained docs.
- Release artifacts: `.deb`, `.rpm`, `.AppImage`, AUR `lwe`, and AUR `lwe-git`.

v1 acceptance criteria:

- Full CI passes.
- Docs build passes.
- Fresh install path is documented and verified.
- Manual real-desktop `niri` acceptance passes.
- Eligible 2D Scene behavior is accepted separately, and unsupported Scene/web behavior is clear and non-runnable.
- Known limitations are documented.
- Release notes accurately describe scope.

v1 must not claim:

- full Wallpaper Engine compatibility;
- complete Scene runtime parity or features beyond the documented experimental subset;
- web wallpaper runtime;
- creator/editor tools;
- cloud/community features;
- advanced automation;
- general Wayland support;
- GNOME/KDE/Hyprland/sway support without explicit verified testing;
- LWE-managed Workshop downloading independent of Steam.

### Daily-use reliability run (2026-10-12)

The original reliability plan uses an isolated production `DesktopService` test and the durable `scripts/scene-reliability.py` recorder. The fixed switch cohort is `2903458386`, `3659880761`, `3337606455`, and `3258032485`; the continuous primary is 2.5D Space. Its future full targets are 100 complete Scene → Video → Scene round trips in one backend, 24 hours of observed active Scene presentation, and ten actual kernel suspend/resume cycles with unchanged assignments and subsequent frames. The user has explicitly deferred physical suspend/resume from this batch because it interrupts Codex; it is neither run nor counted as passed, and current non-suspend runs use `--suspend-cycles 0`. Clock gaps, locks, preparation, mock tests, and interrupted segments earn no active-time credit. Binary/source/input identities, frame histograms, RSS/PSS, native allocator statistics, FD/thread counts, DRM diagnostics and owned audio streams are recorded locally. A 64 MiB post-warm-up RSS/PSS growth budget and 30 FPS render budget are explicit test limits, not general hardware support claims.

The separate privileged `scripts/scene-suspend-cycles.py` helper remains available for a future task that includes physical recovery acceptance. It arms an owned RTC wake alarm before normal logind suspend, preserves inhibitors/locking, verifies kernel counters and logs, and waits for an unlocked healthy Scene; it refuses an existing RTC alarm and AC disconnection. It is not invoked in this batch. A completed recorder can establish only the targets actually recorded: zero required suspend cycles cannot prove physical recovery, and short smoke tests or starting a 24-hour job are not completed endurance acceptance.
