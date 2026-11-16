# Usage guide

This page describes the main LWE workflows.

## Library

The Library is the daily-use home for local wallpaper content. Use it to review recognized wallpapers, inspect compatibility information, and apply supported items to monitors.

Library cards expose content type, compatibility level, and playback eligibility. Video items use the verified runtime path. Scene items are assessed individually: assets that pass the experimental renderer's validation receive an **Experimental Scene** label; blocked items remain visible with playback disabled. Open the detail panel to inspect the blocking resource or feature.

Typical Library flow:

1. Open **Library**.
2. Select a wallpaper.
3. Review metadata and compatibility details.
4. Apply it to a monitor, or clear the current assignment.

After applying, clearing, or restoring desktop assignments, LWE refreshes the selected Library detail where possible so assigned-monitor labels stay aligned with the Desktop page. If refresh fails, the populated detail view remains visible and the error is shown inline.

## Workshop

Workshop features are designed around discovery and acquisition orchestration. LWE helps users find Wallpaper Engine content, understand whether it is useful on Linux, and move recognized content into a local workflow.

Workshop browsing requires a Steam Web API key configured in **Settings**. Steam may still be responsible for subscription, download, and account-owned content synchronization.

Online Workshop results are discovery-only. A result being visible in LWE does not mean the item is already synchronized locally, and LWE does not directly download Workshop content. Use **Open in Steam** as the acquisition and subscription handoff, then refresh local Workshop content after Steam finishes synchronizing.

Search result cards label inferred runtime expectations. Video playback requires local sync and a working desktop runtime. Scene results need local resource validation before playback eligibility can be determined; an online result is not a compatibility test. Web and application results remain outside the current runtime scope.

Workshop search cards keep a bounded card width even when a filter returns only one or two results, so sparse result sets keep the same visual scale as normal browsing pages.

When the Steam Web API key is missing, Workshop search points you back to **Settings**. Settings also distinguishes the common Steam integration states where possible: Steam not detected, Steam detected but Wallpaper Engine Workshop content missing, Wallpaper Engine present but no local Workshop items synchronized yet, and local Workshop content available.

## Compatibility levels

Treat compatibility information as the source of truth for whether a wallpaper should work in the current app.

LWE's product model recognizes common Wallpaper Engine content categories:

- `video`: the main runtime target, verified on Wayland with `niri`;
- `scene`: recognized locally and validated against the experimental 2D renderer; desktop acceptance covers the specific sample and environment recorded below, rather than complete Scene support;
- `web`: recognized for reporting, not a first-release runtime target.

If the app reports that an item is unsupported or degraded, do not assume it can be fixed by reinstalling the package. It may be outside the current runtime scope.

### Experimental Scene validation

The Scene reader accepts packaged resources and unpacked project folders. It uses locally installed Wallpaper Engine assets when a project references built-in materials or effects. LWE does not redistribute those original assets.

The renderer validates 2D image layers with explicit fixed orthographic dimensions, static transforms and colors, supported texture formats, and a bounded effect pipeline. The saved eye and center must have matching X/Y, face forward toward negative Z with the default up direction, have no active `camera.paths` (missing or an empty array), and leave automatic projection disabled. Saved X/Y may be nonzero: in this limited branch, LWE uses the canonical camera reset found in the original program's static loading path, rather than translating layers by the saved coordinates. Tilted or reversed cameras, a nondefault up direction, camera paths, automatic projection, and movement/parallax remain blocked. Planar image layers can inherit transforms from an image or transform-only parent. Hiding an ancestor suppresses the branch. Supported SceneScript can change local transforms and visibility on fixed authored links. Named attachments, script-driven reparenting and model `parent` bindings remain blocked.

The legacy image-material subset includes `genericimage`, the two- and four-texture `flowimage` variants, `swayimage` and `flag`. Layer dimensions can come from the image model's `width`/`height`; smaller layers keep their authored scale and position. These materials use the project's original shader resources, supported repeat/clamp texture flags, padded image bounds and time-based animation. Declarative `usershadervalues` mappings connect validated project properties to material uniforms, including color choices. This does not establish exact original-engine precedence when a user mapping and a material constant target the same value.

The effect subset includes Shake, Shine, WaterRipple, Iris, Scroll, Pulse and bounded project-owned effect passes, with sprite/rope particles, event children and four-pass LDR bloom. Particles can use validated single-image `TEXS0002` sprite atlases with sequence/random-frame selection and sequence-frame interpolation, plus bounded planar turbulence, alpha/size oscillation and angular motion. Their noise and randomness have not been matched to the original engine. Embedded audio supports one FLAC entry per loop layer with the author's gain. Exact declarative `user`/`value` bindings use validated bool, slider, color and combo choices; conditional combo equality can control visibility. Project/monitor choices are validated and saved with the assignment. Unknown binding/component shapes, missing or invalid declarations, text, 3D models, puppet animation, timed or multi-image TEXS, video textures and HDR remain blocked.

A restricted pure scalar SceneScript module computes declared-control values during CPU preparation. A separate bounded worker now implements `init`, `update`, `applyUserProperties`, limited `Vec3`/layer access and image-model `createLayer(path)` during initialization. It can drive bars from a real 64-band `average` audio buffer. Unknown APIs, imports, general dynamic access, wall-clock/time APIs, randomness and broader interaction remain blocked. The original Yakuza project's four backgrounds × bars on/off pass CPU preparation. Five complete offscreen GPU cases and the real Library/Desktop control/save/failure/restart/reset workflow passed on the recorded single-output `niri`/AMD 680M desktop; independent real monitor capture → 64-band FFT → original script → offscreen bar pixels also passed using an owned silent audio output. This does not establish the user's current music/video route or Windows parity. See the [Yakuza development record](../contributing/guide.md#yakuza-extension-2026-10-12).

A `perspective:true` image can enter the coplanar subset only with fixed orthographic projection, zero Z origin, no X/Y rotation and no camera movement/parallax. Equivalence to the planar transform is a mathematical inference; original-engine per-layer mapping and visual parity remain unverified.

The static-card path also accepts `genericimage2` with its legacy VERSION-absent Brightness/Alpha interface or explicit VERSION 1/2 Color4 interface; post-effect composition uses VERSION 2 modes 0/30/31. It preserves project-owned shaders, effect ordering and the full-frame input used by fullscreen/passthrough layers. Pulse and compatible project effects can read real desktop-output audio as sixteen normalized bass-to-treble bins per stereo channel. This captures the current output monitor, not the microphone; unavailable output capture temporarily supplies silence. The normalization and frequency cutoffs have not been matched to Windows Wallpaper Engine.

Mixed sample `3659880761` now passes original-package preparation and a finite engine-only desktop probe with its authored default: six visible particle roots and five children, all active effects and LDR bloom. Its seventh Reactive root is authored `visible:false` and stays hidden; enabling its audio response remains blocked. This is default playback support for the bounded path, not full particle/FFT compatibility or original-engine visual equivalence.

The original Workshop [2.5D Space (`2903458386`)](https://steamcommunity.com/sharedfiles/filedetails/?id=2903458386) now preserves all ten image layers, eleven effects and twenty-four passes, including Scroll, audio Pulse/Chromatic and the project's custom bloom chain. On the recorded single-output `niri`/680M machine, real LWE Library/Desktop application, editing all twenty author controls, save-failure rollback/retry, new-process restore, default reset, Settings updates and finite fullscreen recovery passed. The initial engine probe separately passed pause/resume and clear. This was the first accepted item in the four-item discovery cohort. The other three were blocked in that snapshot; Yakuza now has its own finite CPU/GPU/native UI record and a separate real private-output audio-to-GPU check. See the [Space acceptance record](../contributing/guide.md#space-2026-10-10) for measurements and remaining validation boundaries.

The official built-in projects Deep Space, Beach and Eagle Flag separately passed original-resource CPU preparation and finite engine-only desktop playback, pause/resume and clear checks; reviewed captures confirmed visible animation. They account for three of fourteen installed official projects and are separate from the downloaded Workshop sample count below. This work does not add those builtins to Library or establish their native UI, assignment persistence, original-engine visual A/B or daily-use acceptance.

The compatibility panel reports the first blocking feature or resource error. Passing CPU asset validation enables an experimental apply attempt; shader compilation, GPU initialization and visible playback still require runtime confirmation. Video remains the main runtime target. Samples `3337606455` (Shine), `3258032485` (Shake) and the original-default `3659880761` have real Tauri UI apply/close-hide/restart-restore/clear acceptance on the recorded single-output machine. The default 365 flow also passed Settings 15 → 30 FPS and new-process preference restore, with English/Chinese captures reviewed. Its native property edit, real save-failure rollback/retry, new-process control restore and full reset also passed, as did twelve-second owned fullscreen recovery. Occlusion callback/pause was not measured, and actual suspend/lock remain unverified; engine-only probes are recorded separately. See the [acceptance record](../contributing/guide.md#2026-10-10-development-record). Original-engine visual A/B and daily-use acceptance remain outstanding.

The engine schedules frames against a deadline per output, stops sustained drawing/swapping/frame callbacks for settled static or paused Scenes, and bounds logical RGBA texture/framebuffer and particle-VBO payload to 512 MiB across outputs, candidates and rollback resources. This is not a measured VRAM limit. Settings saves frame-rate and battery-pause preferences and updates the running engine. The isolated 15 → 30 FPS service test measured about 14.70 → 29.60 completed swaps/s without replacing backend or assignment. Finite native Settings/save/restart and Chinese layout checks passed. The newer mixed-Scene probe measured about 27.95 swaps/s at a 30 FPS limit, zero draws/swaps/callbacks during sixty seconds of pause, and about 28.33 swaps/s after resume. Physical battery transitions, 24-hour operation, actual suspend/resume, installed-package and independent multi-monitor acceptance remain unverified.

The historical 2026-10-10 read-only CPU gate rerun accepted three of eleven original packages: `3258032485`, `3337606455` and `3659880761`. Eight first-blocked in that snapshot on SceneScript (five), camera, HDR or layer count. All eleven package hashes match the earlier inventory. This is preparation evidence from the recorded existing binary, not native UI or a Workshop compatibility rate; the earlier two-of-eleven result is historical. The earlier 100-cycle Scene/video lifecycle test passed while process RSS rose. Resource diagnostics point to allocator-retained free space as one contributor, without proving all native memory is leak-free or RSS reaches a stable bound. The newer 20 mixed 365/video cycles passed with no FD accumulation and fixed clear Rust live bytes, while process RSS and allocator retention remained significant. Daily-use resource stability remains unaccepted; see the [resource follow-up](../contributing/guide.md#2026-10-10-development-record).

## Monitor assignment

LWE is desktop-app first. The intended workflow is:

1. choose content from Library;
2. select a monitor;
3. apply the wallpaper;
4. clear or replace the assignment when needed.

Monitor discovery and restore behavior depend on the Linux session. The verified path is Wayland with `niri`; other environments need explicit validation before they are documented as supported. LWE does not plan X11 wallpaper runtime support for v1. Future runtime expansion is based on Wayland protocol capabilities such as `wl_output` and `zwlr_layer_shell_v1`, not desktop-environment-specific integrations.

## Settings

Use Settings for user preferences and integration state:

- language;
- theme;
- Steam Web API key;
- launch-on-login behavior where supported;
- wallpaper frame-rate limit and pause-on-battery preference;
- visible Steam integration state.

Settings are part of the user-facing product and must remain available in both English and Simplified Chinese.

The frame-rate selection is an upper limit, not a guaranteed presentation rate. Playback settings are saved for the next start and sent to a running backend; a reported live-update issue means the saved preference may differ from the current runtime. Battery pause preserves manual pause intent. Actual battery/AC transitions remain unverified on the recorded desktop.

## Data and local state

For a Scene that passes the resource gate, select a monitor in its Library detail panel and open **Scene properties**. Supported author controls include checkboxes, slider values edited in numeric fields, colors, combo selections and text input bound to a supported text layer. Text accepts up to 256 UTF-8 bytes and eight lines. Saving an empty string clears the text, and resetting restores the author's default. Slider limits and steps come from the project, and combo values must remain among the author's declared options. Changing a background choice can select layers through supported bool/combo visibility bindings.

Author groups collect following controls until the next group. Their sections can be expanded or collapsed with the mouse, Enter or Space; controls before the first group remain outside sections. Folding affects only the panel layout. Group headers are never submitted as wallpaper values, sent to scripts or stored in the session file, and fold state resets when the wallpaper or monitor selection changes.

Author display text appears as plain, read-only text. Edits remain a draft until **Save and apply** succeeds. The panel is available only for a locally assessed, playback-eligible Scene and a selected monitor. Group layout does not make an unsupported Scene playable. Option display-condition expressions, broader resource replacement and user shortcuts still have separate runtime requirements.

LWE stores application state under the `lwe` configuration root used by the app. Scene choices are scoped to both wallpaper project and monitor in the same session file as assignments. Ordinary wallpaper replacement or clear retains those choices; resetting choices restores the project's defaults.

If saved choices no longer match an updated author declaration, Reset defaults is available from the property-load error state. It removes that project/monitor's invalid selection by applying supported defaults through the same resource gate; normal Save still requires a validated property snapshot. The development Save and apply path keeps new values only after preparation, first-frame acceptance and an atomic save succeed. Preparation, candidate acceptance or save failure before durable publication retains the previous wallpaper, saved choices and editable draft; candidate audio stays silent until finalization. Native Save and apply, real write-failure retry, new-process property restore and Reset defaults passed in isolated config. Reset restores every author default and retains the assignment; ordinary clear retains saved choices. Original-engine A/B, hardware power/lock/suspend and daily-use acceptance remain open.

## Current limitations

The first release does not aim to provide:

- full web wallpaper runtime parity;
- creator tools;
- cloud or community systems;
- advanced automation rules;
- guaranteed behavior on untested compositors or desktop environments;
- X11 wallpaper runtime support.
