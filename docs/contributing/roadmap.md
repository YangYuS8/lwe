# v1 development roadmap

This page captures the maintained development direction from the current v0.6.1 release line toward LWE v1. Keep it current: when product scope, verified runtime support, or release priorities change, update this page instead of preserving stale planning notes.

## v1 scope statement

LWE v1 should be a trustworthy Linux Wallpaper Engine migration app, not a broad runtime-parity claim.

For v1, LWE targets:

- a Linux desktop app built with Tauri 2 and SvelteKit;
- Rust-owned backend services for Workshop, Library, Desktop, Settings, compatibility, persistence, and runtime integration;
- Wallpaper Engine Workshop content as the primary content source;
- video wallpapers as the only v1 runnable runtime type;
- scene and web wallpapers recognized for metadata and compatibility reporting, but not runnable unless future support is implemented and verified;
- Wayland with `niri` as the verified desktop target;
- maintained English and Simplified Chinese user-facing documentation and copy.

## Milestone 1: support matrix and product truth

Goal: make the product impossible to misunderstand. Video is runnable on the verified path; scene and web are recognized-only unless future verified runtime support exists.

Priorities:

1. Keep one shared support policy authoritative across compatibility, Library, Workshop, Desktop, and docs.
2. Ensure synced video reports runnable support.
3. Ensure synced scene reports recognized/runtime-unsupported support, never ready-to-apply support.
4. Ensure web reports recognized/unsupported runtime status.
5. Keep Library semantics explicit: recognized local items may appear in Library, but Apply must be enabled only for runnable items.
6. Remove or rewrite tests and examples that imply scene runtime support.

Acceptance criteria:

- Synced video is reported as ready for Library and desktop runtime use.
- Synced scene is reported as recognized but runtime-unsupported.
- Web items are not presented as runnable.
- Desktop apply rejects non-video before runtime invocation with a clear reason.
- Library does not enable Apply for non-runnable items.
- English and Simplified Chinese docs describe the same support matrix.
- Tests cover video, scene, web, missing metadata, and missing primary asset behavior.

Main risks:

- Support truth can drift between catalog data, compatibility policy, UI labels, and documentation.
- “Partially supported” can mislead users if it means recognized rather than runnable.
- Library projection behavior affects whether users can inspect recognized-but-not-runnable local content.

## Milestone 2: reliable video runtime on `niri`

Goal: make one runtime path dependable: apply, restore, and clear video wallpapers on Wayland with `niri`.

Priorities:

1. Harden apply-to-monitor flow for supported video items.
2. Harden per-monitor clear behavior.
3. Preserve wallpapers on other monitors when clearing one output.
4. Restore saved assignments on app startup.
5. Surface runtime/backend failures in Desktop with actionable messages.
6. Keep real desktop tests opt-in rather than CI-required.

Acceptance criteria on a verified `niri` machine:

1. LWE discovers at least one active monitor.
2. A user can apply a synced video wallpaper from Library.
3. The desktop visibly changes.
4. Multi-monitor apply works when multiple monitors exist.
5. Clearing one monitor does not stop other monitor wallpapers.
6. Restarting LWE restores saved assignments or shows explicit restore failures.
7. Clearing all assignments prevents later restore.
8. Missing Wayland globals, EGL failure, mpv failure, missing video files, and output mismatch produce visible, non-generic errors.

Main risks:

- CI cannot validate real compositor behavior.
- `niri` output IDs and stable monitor identities can differ across sessions or hardware.
- Runtime status can become stale when the backend dies or compositor state changes.

## Milestone 3: Library-first daily workflow

Goal: make Library the practical daily surface for selecting, inspecting, applying, and managing local content.

Priorities:

1. Show clear compatibility and runnable status on cards and detail panels.
2. Keep Apply enabled only for runnable items with an available monitor.
3. Show monitor assignment state accurately after apply, clear, and restore.
4. Preserve current selection and detail state after refresh/apply/clear where possible.
5. Improve empty, stale, unavailable, and degraded states.
6. Keep filtering and pagination predictable for large local Workshop libraries.

Acceptance criteria:

- Library clearly distinguishes runnable video, recognized runtime-unsupported scene/web, missing metadata, and missing primary asset states.
- Failed apply does not destroy current Library context.
- Assigned monitor labels stay consistent after apply, clear, and restore.
- Library tests do not depend on local Steam installation in CI.

Main risks:

- Including recognized scene/web in Library requires strong Apply gating and clear copy.
- Excluding scene/web from Library would make recognized local content harder to inspect.
- Large Workshop folders may expose scan or rendering performance issues.

## Milestone 4: Workshop discovery and sync clarity

Goal: keep Workshop as discovery and acquisition orchestration, not a replacement for Steam.

Priorities:

1. Make Steam Web API key setup clear.
2. Keep online search and filtering usable.
3. Explain that Steam handles subscription, download, and synchronization.
4. Improve local sync status explanations: synced, missing project metadata, missing primary asset, unsupported type.
5. Keep “Open in Steam” reliable and documented.

Acceptance criteria:

- Missing API key produces clear Settings guidance.
- Online search does not imply local availability.
- Workshop detail explains the next step for each compatibility reason.
- Malformed `project.json` files do not fail the whole local scan.
- Online parsing and type-inference tests do not call Steam network APIs.

Main risks:

- Steam metadata is heuristic and can misclassify type or age.
- Users may expect LWE to download Workshop items directly.
- Steam install paths and package variants can differ across distributions.

## Milestone 5: Settings, persistence, and diagnostics

Goal: give users enough visibility to configure LWE and report useful issues.

Priorities:

1. Persist language, theme, Steam API key, launch-on-login preference, and Workshop filters.
2. Report persistence failures without losing the current app context.
3. Add a diagnostics surface for issue reports.
4. Mask sensitive values such as API keys.
5. Keep English and Simplified Chinese copy in sync.

Diagnostics should prefer observable facts over guesses, including:

- LWE version and package type when available;
- OS, session, and compositor hints when available;
- monitor discovery result;
- runtime backend status or last initialization error;
- Steam discovery result;
- Wallpaper Engine Workshop path availability;
- Library item counts by compatibility/runtime status;
- support-scope reminder: video runtime on the verified `niri` path.

Acceptance criteria:

- Settings survive restart.
- Steam API keys are not exposed in diagnostics or copyable logs.
- Launch-on-login unavailable state is visible and non-fatal.
- Diagnostics can be copied into issue reports.
- Settings tests remain deterministic and do not require desktop/session services.

Main risks:

- Diagnostics can accidentally expose local paths or secrets.
- Launch-on-login behavior differs by desktop/session/package format.
- Compositor detection can be unreliable; do not overstate guesses.

## Milestone 6: release hardening and v1 candidate

Goal: make v1 releasable and supportable.

Priorities:

1. Keep CI green for frontend, docs, Rust formatting, linting, checking, and tests.
2. Verify `.deb`, `.rpm`, `.AppImage`, AUR `lwe`, and AUR `lwe-git` outputs.
3. Add release smoke checks where practical.
4. Maintain release notes that state support scope accurately.
5. Require manual real-desktop acceptance for runtime-affecting v1 candidates.

v1 release acceptance criteria:

- Documentation builds in English and Simplified Chinese.
- No documentation implies scene or web runtime support.
- Fresh install and quick-start paths are documented.
- Quick start works on a verified `niri` machine.
- Video apply, clear, and restore are manually verified.
- Unsupported scene/web behavior is clear and non-runnable.
- Release artifacts are produced and named as expected.
- Known limitations are documented.

## v1 non-goals

Do not include these in v1 unless separately implemented, tested, and documented:

- full scene wallpaper runtime;
- web wallpaper runtime parity;
- creator/editor tools;
- cloud or community features;
- advanced automation rules;
- daemon-first architecture;
- general Wayland support claims;
- GNOME, KDE, Hyprland, sway, or other compositor support claims without real validation;
- direct replacement for Steam Workshop download/subscription behavior.

## Maintenance rules

- Keep user-facing behavior aligned with the documented support scope.
- Keep English and Simplified Chinese documentation in sync.
- Treat compatibility, import paths, runtime behavior, and persisted state changes as test-worthy.
- Do not archive obsolete planning notes in published docs; update or remove stale roadmap content.
