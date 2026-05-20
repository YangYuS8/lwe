# v1 version roadmap

This page captures the maintained development direction from the current v0.6.1 release line toward LWE v1. Keep it current: remove or collapse obsolete version sections after release, and update the page when product scope, verified runtime support, or release priorities change.

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
