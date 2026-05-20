# Usage guide

This page describes the main LWE workflows.

## Library

The Library is the daily-use home for local wallpaper content. Use it to review recognized wallpapers, inspect compatibility information, and apply supported items to monitors.

Typical Library flow:

1. Open **Library**.
2. Select a wallpaper.
3. Review metadata and compatibility details.
4. Apply it to a monitor, or clear the current assignment.

## Workshop

Workshop features are designed around discovery and acquisition orchestration. LWE helps users find Wallpaper Engine content, understand whether it is useful on Linux, and move recognized content into a local workflow.

Workshop browsing requires a Steam Web API key configured in **Settings**. Steam may still be responsible for subscription, download, and account-owned content synchronization.

## Compatibility levels

Treat compatibility information as the source of truth for whether a wallpaper should work in the current app.

LWE's product model recognizes common Wallpaper Engine content categories:

- `video`: primary first-release runtime focus and the only current runnable type on the verified path;
- `scene`: recognized where metadata is available, but not a first-release runtime target until real support is implemented and verified;
- `web`: recognized for reporting, not a first-release runtime target.

If the app reports that an item is unsupported or degraded, do not assume it can be fixed by reinstalling the package. It may be outside the current runtime scope.

## Monitor assignment

LWE is desktop-app first. The intended workflow is:

1. choose content from Library;
2. select a monitor;
3. apply the wallpaper;
4. clear or replace the assignment when needed.

Monitor discovery and restore behavior depend on the Linux session. The verified path is Wayland with `niri`; other environments need explicit validation before they are documented as supported.

## Settings

Use Settings for user preferences and integration state:

- language;
- theme;
- Steam Web API key;
- launch-on-login behavior where supported;
- visible Steam integration state.

Settings are part of the user-facing product and must remain available in both English and Simplified Chinese.

## Data and local state

LWE stores application state under the `lwe` configuration root used by the app. Contributors should avoid documenting internal paths as permanent user-facing API unless the path is intentionally stabilized.

## Current limitations

The first release does not aim to provide:

- full web wallpaper runtime parity;
- creator tools;
- cloud or community systems;
- advanced automation rules;
- guaranteed behavior on untested compositors or desktop environments.
