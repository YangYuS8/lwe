# Near-term roadmap

This page captures the maintained development direction for upcoming LWE work. It is intentionally short-lived guidance: update it when product scope, verified runtime support, or release priorities change.

## Guiding principle

Stabilize the current video wallpaper path before broadening runtime scope. LWE should earn one dependable workflow first: find Wallpaper Engine content, understand compatibility, import it into Library, apply a supported video wallpaper on the verified desktop target, restore it after restart, and clear it predictably.

Do not document scene, web, or broad compositor runtime behavior as supported until it is implemented and verified on real machines.

## Now

### Align the support matrix

Compatibility is a core product promise. Keep support rules consistent across documentation, shared policies, Library/Workshop badges, and desktop apply checks.

Current authoritative runtime position:

| Wallpaper type | Current direction |
| --- | --- |
| Video | Primary first-release runtime target on the verified environment. |
| Scene | Recognized for metadata and compatibility reporting, but not a first-release runtime target unless real support is implemented and verified. |
| Web | Recognized for reporting; not a first-release runtime target. |
| Other/application | Unsupported for LWE runtime workflows. |

Near-term work should remove ambiguity where code or UI implies stronger scene support than the runtime can provide.

### Stabilize video runtime on `niri`

The verified desktop target remains Wayland with `niri`. Runtime work should focus on:

1. applying one video wallpaper to one monitor;
2. applying wallpapers across multiple monitors;
3. clearing one monitor without stopping other outputs;
4. restoring saved assignments after app restart;
5. surfacing actionable restore/runtime failures in Desktop;
6. preserving the opt-in real desktop acceptance flow documented in the contributor guide.

### Make Library apply behavior unambiguous

Library is the daily-use center. It should clearly separate items that are runnable now from items that are only recognized or degraded.

Useful next work:

- disable or explain apply actions for non-runnable items;
- show runtime prerequisites near apply controls;
- keep stale Desktop state actionable instead of merely informational;
- populate current monitor details, such as cover art, when reliable data exists.

## Next

### Improve Workshop acquisition and sync messaging

Workshop remains discovery and acquisition orchestration, not a replacement for Steam downloads. Improve the user loop around:

- opening the Steam source page;
- subscribing or acquiring content through Steam;
- waiting for local Steam synchronization;
- explaining missing `project.json`, missing primary assets, and unsupported types.

### Add diagnostics for user reports

Before broad compositor expansion, add a diagnostics surface that users can copy into issue reports. It should prefer observable facts over guesses, such as package/version, session/compositor hints, monitor discovery result, runtime backend initialization result, EGL/mpv availability when known, Steam integration status, and Library content counts.

### Strengthen release smoke validation

CI already covers frontend, docs, Rust checks, and package builds. Add lightweight release confidence where possible:

- package artifact name checks;
- AppImage executable-bit or launch-smoke checks where practical;
- manual runtime acceptance notes for runtime-affecting releases;
- no CI requirement for real desktop tests unless a verified runner exists.

## Later

### Expand compositor support only after verification

Do not turn `niri` success into a general Wayland support claim. Expansion should be capability-driven and one compositor at a time.

Possible order:

1. `niri` hardening;
2. wlroots-style targets such as `sway`, after monitor identity and layer-shell behavior are verified;
3. Hyprland after explicit testing;
4. GNOME/KDE only after a separate integration assessment.

Each new target needs real desktop validation before documentation claims support.

### Defer large runtime scope increases

Keep these out of the near-term roadmap unless a focused design and verified implementation exists:

- full scene wallpaper runtime;
- web wallpaper parity;
- creator tooling;
- cloud/community systems;
- advanced automation rules;
- daemon-first architecture.

## Maintenance reminders

- Keep user-facing behavior aligned with documented support scope.
- Keep English and Simplified Chinese documentation in sync.
- Treat compatibility, import paths, runtime behavior, and persisted state changes as test-worthy.
- Remove obsolete planning notes after their useful content is reflected in maintained docs.
