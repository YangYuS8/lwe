# LWE Documentation

LWE is a Linux desktop application for browsing, managing, checking, importing, and applying Wallpaper Engine content.

It is built for practical migration workflows on Linux: keep Workshop discovery close to the desktop app, make compatibility visible before users apply a wallpaper, and provide a library-first daily workflow.

## Start here

- [Installation](./guide/installation.md): install from AUR or GitHub Releases.
- [Quick start](./guide/quick-start.md): configure Steam, import content, and apply a wallpaper.
- [Usage guide](./guide/usage.md): understand Library, Workshop, compatibility, settings, and desktop behavior.
- [Troubleshooting](./guide/troubleshooting.md): fix common setup and runtime issues.

## Contributor documentation

- [Project overview](./contributing/project.md): product scope, architecture, release model, and active paths.
- [Near-term roadmap](./contributing/roadmap.md): maintained priorities for upcoming development.
- [Contributor guide](./contributing/guide.md): development setup, checks, documentation policy, and product-change workflow.

## Current support scope

| Area | Status |
| --- | --- |
| Platform | Linux desktop app |
| Tested session | Wayland with `niri` |
| Primary content source | Wallpaper Engine content from Steam Workshop |
| First-release runtime focus | Video wallpapers |
| Compatibility reporting | Video, scene, and web wallpaper metadata where available |
| Languages | English and Simplified Chinese |

Scene and web wallpapers can appear in compatibility reporting, but they should not be assumed to have the same runtime support level as video wallpapers unless the app reports that they are supported.
