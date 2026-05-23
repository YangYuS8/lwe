# Troubleshooting

## Workshop browsing does not work

Check the Steam Web API key first.

- Open **Settings**.
- Confirm that **Steam Web API Key** is filled in.
- If needed, create or copy the key from <https://steamcommunity.com/dev/apikey>.
- Confirm that you are signed in with a Steam account that owns Wallpaper Engine.

Also confirm that Steam and Wallpaper Engine are installed locally.

## A wallpaper appears but cannot be applied

Review the compatibility status shown in LWE.

Possible reasons include:

- the wallpaper type is recognized but not currently supported by the runtime;
- the item is a web or scene wallpaper outside the current support level;
- the local Workshop content is incomplete or has not synchronized yet;
- the current desktop session has not been validated.
- the runtime backend could not initialize Wayland layer-shell, EGL, or the target output.
- the saved assignment in `$XDG_CONFIG_HOME/lwe/session.toml` or `$HOME/.config/lwe/session.toml` points to a missing monitor or Library item.

Video wallpapers are the first-release runtime focus. Do not assume every Wallpaper Engine item is runnable on Linux.

## Applying a wallpaper does not change the desktop

Confirm that you are testing on a supported or known-good session. The currently verified environment is Wayland with `niri`.

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

Common low-level causes include missing `zwlr_layer_shell_v1`, unavailable EGL, a monitor/output name mismatch, a missing local video asset, or an unsupported wallpaper type.

When the error mentions an output-name mismatch, refresh the Desktop page, select the monitor again, and include the requested output plus the listed runtime outputs in any issue report.

If this only fails on a different compositor, document the compositor, session type, and monitor layout when reporting the issue.

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
