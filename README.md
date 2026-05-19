<p align="center">
  <img src="logo.svg" alt="LWE logo" width="180" />
</p>

<p align="center">
  <img alt="AUR lwe-git" src="https://img.shields.io/aur/version/lwe-git?label=AUR%20lwe-git" />
  <img alt="AUR lwe" src="https://img.shields.io/aur/version/lwe?label=AUR%20lwe" />
  <img alt="Quality Check" src="https://github.com/YangYuS8/lwe/actions/workflows/quality-check.yml/badge.svg" />
  <img alt="Prerelease" src="https://github.com/YangYuS8/lwe/actions/workflows/release-prerelease.yml/badge.svg" />
  <img alt="Stable release" src="https://github.com/YangYuS8/lwe/actions/workflows/release-stable.yml/badge.svg" />
</p>

# LWE

English | [简体中文](README_CN.md)

LWE is a Linux desktop app for browsing, managing, and applying Wallpaper Engine content.

It is designed for practical Linux migration workflows:

- Browse Workshop content in-app
- Import supported wallpapers into your local library
- Check compatibility before applying wallpapers
- Assign wallpapers to monitors in a desktop-oriented workflow

## Documentation

- Online documentation: https://yangyus8.top/lwe/
- Local documentation source: `docs/`
- Simplified Chinese documentation: https://yangyus8.top/lwe/zh/

## Tested desktop environment

LWE is currently tested on:

- Wayland session with `niri`

## Prerequisites

LWE relies on Wallpaper Engine content from Steam Workshop. To use it properly, make sure:

- Your Steam library owns Wallpaper Engine
- Steam client is installed on your device
- Wallpaper Engine client is installed on your device

## Workshop setup (Steam Web API key)

LWE Workshop features require a Steam Web API key configured in Settings.

- Open LWE `Settings` and fill in `Steam Web API Key`
- Without this key, in-app Workshop browsing/search will not work correctly

How to get a Steam Web API key:

- Official page: https://steamcommunity.com/dev/apikey
- You must sign in with your Steam account first

## Wallpaper support scope

First-release runtime focus:

- Video wallpapers

Not first-release runtime targets:

- Scene wallpapers (private format reverse engineering cost is currently too high)
- Web wallpapers (recognized for compatibility reporting, not primary runtime target)

## Installation

### Arch Linux (AUR)

- Stable package: `lwe`
- Development package: `lwe-git`

Install with your preferred AUR helper, for example:

```bash
yay -S lwe
```

or

```bash
yay -S lwe-git
```

### GitHub Releases

Pre-release and stable builds publish Linux artifacts:

- `.deb`
- `.rpm`
- `.AppImage`

Download from the repository Releases page.

## Contributor and agent notes

Contributor guidance is documented in `docs/contributing/guide.md` and published in the online documentation.
