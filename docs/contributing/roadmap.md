# v1 version roadmap

This page captures the maintained development direction from the released v0.9.8 baseline toward LWE v1. Keep it current: remove or collapse obsolete version sections after release, and update the page when product scope, verified runtime support, or release priorities change.

## v1 target

LWE v1 should be a trustworthy Linux Wallpaper Engine migration app, not a broad runtime-parity claim.

For v1, LWE targets:

- a Linux desktop app built with Tauri 2 and SvelteKit;
- Rust-owned backend services for Workshop, Library, Desktop, Settings, compatibility, persistence, and runtime integration;
- Wallpaper Engine Workshop content as the primary content source;
- video wallpapers as the only v1 runnable runtime type;
- scene and web wallpapers recognized for metadata and compatibility reporting, but not runnable unless future support is implemented and verified;
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
| v0.9.9 | Wayland capability groundwork | Wayland protocol capabilities decide dynamic wallpaper availability, with graceful fallback when required protocols are missing. |
| v1.0.0-rc.1 | First release candidate | Scope is frozen and validated end-to-end. |
| v1.0.0-rc.2 | Optional blocker-fix candidate | Only if rc.1 finds release blockers. |
| v1.0.0 | Stable v1 | Honest, reliable video-first release. |

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
- Scene/web recognized-only behavior manually checked.
- No user-facing copy claims unsupported runtime behavior.
- Release artifacts build successfully.

Must not claim:

- v1 final stability;
- broad compositor support;
- scene/web runtime support.

## v1.0.0-rc.2: optional blocker-fix candidate

Theme: only fix release blockers found in rc.1.

Allowed work:

- data loss fixes;
- broken install or launch fixes;
- broken video apply/clear/restore fixes on verified `niri`;
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
- Scene/web recognized for compatibility reporting but runtime-unsupported.
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
- Unsupported scene/web behavior is clear and non-runnable.
- Known limitations are documented.
- Release notes accurately describe scope.

v1 must not claim:

- full Wallpaper Engine compatibility;
- scene wallpaper runtime;
- web wallpaper runtime;
- creator/editor tools;
- cloud/community features;
- advanced automation;
- general Wayland support;
- GNOME/KDE/Hyprland/sway support without explicit verified testing;
- LWE-managed Workshop downloading independent of Steam.
