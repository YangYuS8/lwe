# Troubleshooting

## Workshop browsing does not work

Check the Steam Web API key first.

- Open **Settings**.
- Confirm that **Steam Web API Key** is filled in.
- If Workshop search reports that the key is missing, use its Settings link, save the key, then return to Workshop search.
- If needed, create or copy the key from <https://steamcommunity.com/dev/apikey>.
- Confirm that you are signed in with a Steam account that owns Wallpaper Engine.

Also confirm that Steam and Wallpaper Engine are installed locally.

Search results are discovery-only. Seeing an item online does not mean it is downloaded or synchronized locally; use **Open in Steam** to subscribe or inspect the item in Steam.

## A wallpaper appears but cannot be applied

Review the compatibility status shown in LWE.

In Library, first check the card badges: runnable video items can be applied, while recognized-only scene/web items are intentionally blocked by the current runtime. Opening the detail panel shows the same policy with more context.

Workshop online result cards use the same distinction: videos are the only current runnable target after local sync, while scene/web results are recognized-only and application results are unsupported for runtime.

Possible reasons include:

- the wallpaper type is recognized but not currently supported by the runtime;
- the item is a web or scene wallpaper outside the current support level;
- the local Workshop content is incomplete or has not synchronized yet;
- the current desktop session has not been validated.
- the runtime backend could not initialize Wayland layer-shell, EGL, or the target output.
- the saved assignment in `$XDG_CONFIG_HOME/lwe/session.toml` or `$HOME/.config/lwe/session.toml` points to a missing monitor or Library item.

Video wallpapers are the first-release runtime focus. Do not assume every Wallpaper Engine item is runnable on Linux.

## Applying a wallpaper does not change the desktop

Confirm that you are testing on a supported or known-good session. The currently verified environment is Wayland with `niri`. LWE's dynamic wallpaper runtime is Wayland-only and requires protocol capabilities such as `wl_compositor`, `wl_output`, and `zwlr_layer_shell_v1`; it does not fall back to an X11 runtime.

Then try:

1. clear the current assignment in LWE;
2. reselect the monitor;
3. apply the wallpaper again;
4. restart LWE if the session state appears stale;
5. if stale assignments keep returning, inspect or remove the LWE session file at `$XDG_CONFIG_HOME/lwe/session.toml` or `$HOME/.config/lwe/session.toml`.

When launching from a terminal, runtime errors are most useful when they name one of these stages:

- backend start;
- output discovery;
- first-frame apply;
- per-monitor clear;
- restore on startup.

Common low-level causes include missing `zwlr_layer_shell_v1`, unavailable EGL, a monitor/output name mismatch, a missing local video asset, or an unsupported wallpaper type. The Settings diagnostics output includes a Wayland capability report that can show whether the compositor exposes the required protocols before the runtime starts.

When the error mentions an output-name mismatch, refresh the Desktop page, select the monitor again, and include the requested output plus the listed runtime outputs in any issue report.

If this only fails on a different compositor, document the compositor, session type, and monitor layout when reporting the issue.

## The app shell crashes on GNOME/Mutter Wayland

Some GNOME/Mutter + WebKitGTK combinations can fail during app shell startup with a Wayland explicit-sync error similar to `Explicit Sync only supported on dmabuf buffers`. As an app-shell workaround, try launching LWE with:

```bash
WEBKIT_DISABLE_DMABUF_RENDERER=1 lwe-shell
```

This workaround only affects the Tauri/WebKit app window. It does not mean GNOME/Mutter supports LWE's dynamic wallpaper runtime. The wallpaper runtime still depends on Wayland protocol capabilities, especially `zwlr_layer_shell_v1`, and will degrade gracefully when required protocols are unavailable.

## AppImage does not launch

Make sure the AppImage is executable:

```bash
chmod +x LWE*.AppImage
```

Then launch it from a terminal to capture logs:

```bash
./LWE*.AppImage
```

## AUR package build fails

Try updating the package database and rebuilding in a clean environment. If the failure is package-specific, mention whether you used `lwe` or `lwe-git` when reporting the issue.

## Language or theme does not persist

Change the setting again and restart the app. If the problem remains, include your distribution, desktop session, package type, and whether the app can write to its configuration directory.
