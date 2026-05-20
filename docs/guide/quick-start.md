# Quick start

Follow these steps after installing LWE.

## 1. Prepare Steam and Wallpaper Engine

LWE uses Wallpaper Engine content from Steam Workshop. Confirm that:

1. Steam is installed and signed in.
2. Wallpaper Engine exists in your Steam library.
3. Wallpaper Engine is installed locally.

## 2. Configure the Steam Web API key

Workshop browsing and search require a Steam Web API key.

1. Open the official Steam Web API key page: <https://steamcommunity.com/dev/apikey>.
2. Sign in with the Steam account that owns Wallpaper Engine.
3. Create or copy your API key.
4. Open LWE.
5. Go to **Settings**.
6. Paste the key into **Steam Web API Key**.

Without this key, in-app Workshop browsing and search will not work correctly.

## 3. Browse or import wallpaper content

Use the Workshop surface to discover content and hand off acquisition through Steam where required. Imported or synchronized items appear in the Library when LWE can recognize the local content.

When reviewing a wallpaper, check its compatibility status before applying it. Compatibility reporting is a product feature: it exists to explain what LWE can run now, what it can only recognize, and what may require future runtime work.

## 4. Apply a wallpaper

1. Open **Library**.
2. Select a supported wallpaper.
3. Choose the target monitor.
4. Apply the wallpaper.

If the desktop does not reflect the change, check [Troubleshooting](./troubleshooting.md). The currently verified path is Wayland with `niri`.

## 5. Adjust language and theme

Open **Settings** to change language and theme preferences. LWE targets both English and Simplified Chinese for first-release product surfaces.

## What to expect from first-release support

- Video wallpapers are the primary runtime focus.
- Scene and web wallpapers may be identified for compatibility reporting.
- Scene and web items should be treated as recognized-only unless the app explicitly reports runnable support.
- Advanced creator tools, cloud/community systems, and broad web wallpaper parity are not first-release goals.
