use std::sync::atomic::{AtomicBool, Ordering};

use tauri::{
    Manager,
    menu::{MenuBuilder, MenuItemBuilder},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder},
};

// Layered application core for the Tauri shell.
pub mod action_outcome;
pub mod assembly;
pub mod commands;
pub mod models;
pub mod policies;
pub mod results;
pub mod services;

pub const APP_CODE_NAME: &str = "lwe";

struct QuitRequested(AtomicBool);
const TRAY_ID: &str = "main-tray";

fn tray_menu_labels(language: &str) -> (&'static str, &'static str) {
    if language.eq_ignore_ascii_case("en") {
        ("Show Main Window", "Quit")
    } else {
        ("显示主界面", "退出")
    }
}

pub fn update_tray_menu_language(app: &tauri::AppHandle, language: &str) {
    let (show_main_label, quit_label) = tray_menu_labels(language);

    let show_main_item = match MenuItemBuilder::with_id("show-main", show_main_label).build(app) {
        Ok(item) => item,
        Err(reason) => {
            eprintln!("failed to build tray show-main item: {reason}");
            return;
        }
    };
    let quit_item = match MenuItemBuilder::with_id("quit-app", quit_label).build(app) {
        Ok(item) => item,
        Err(reason) => {
            eprintln!("failed to build tray quit item: {reason}");
            return;
        }
    };
    let menu = match MenuBuilder::new(app)
        .items(&[&show_main_item, &quit_item])
        .build()
    {
        Ok(menu) => menu,
        Err(reason) => {
            eprintln!("failed to build tray menu: {reason}");
            return;
        }
    };

    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        eprintln!("tray icon not found while updating menu language");
        return;
    };

    if let Err(reason) = tray.set_menu(Some(menu)) {
        eprintln!("failed to update tray menu language: {reason}");
    }
}

fn show_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn is_start_hidden() -> bool {
    std::env::args_os().any(|arg| arg == "--start-hidden")
}

pub fn register_commands(builder: tauri::Builder<tauri::Wry>) -> tauri::Builder<tauri::Wry> {
    builder.invoke_handler(tauri::generate_handler![
        commands::app_shell::load_app_shell,
        commands::workshop::load_workshop_page,
        commands::workshop::load_workshop_item_detail,
        commands::workshop::refresh_workshop_catalog,
        commands::workshop::search_workshop_online,
        commands::workshop::open_workshop_in_steam,
        commands::library::load_library_page,
        commands::library::load_library_item_detail,
        commands::desktop::load_desktop_page,
        commands::desktop::apply_library_item_to_monitor,
        commands::desktop::clear_library_item_from_monitor,
        commands::settings::load_settings_page,
        commands::settings::update_settings,
    ])
}

pub fn builder() -> tauri::Builder<tauri::Wry> {
    register_commands(tauri::Builder::default())
        .setup(|app| {
            let app = app.app_handle();
            app.manage(QuitRequested(AtomicBool::new(false)));

            let language = crate::services::settings_service::SettingsService::load_page()
                .map(|page| page.language)
                .unwrap_or_else(|_| "system".to_string());
            let (show_main_label, quit_label) = tray_menu_labels(&language);

            let show_main_item =
                MenuItemBuilder::with_id("show-main", show_main_label).build(app)?;
            let quit_item = MenuItemBuilder::with_id("quit-app", quit_label).build(app)?;
            let tray_menu = MenuBuilder::new(app)
                .items(&[&show_main_item, &quit_item])
                .build()?;

            let _tray =
                TrayIconBuilder::with_id(TRAY_ID)
                    .icon(app.default_window_icon().cloned().ok_or_else(|| {
                        "default app icon is unavailable for tray icon".to_string()
                    })?)
                    .menu(&tray_menu)
                    .show_menu_on_left_click(false)
                    .on_menu_event(|app, event| match event.id.as_ref() {
                        "show-main" => show_main_window(app),
                        "quit-app" => {
                            app.state::<QuitRequested>()
                                .0
                                .store(true, Ordering::Relaxed);
                            app.exit(0);
                        }
                        _ => {}
                    })
                    .on_tray_icon_event(|tray, event| {
                        if let tauri::tray::TrayIconEvent::Click {
                            button: MouseButton::Left,
                            button_state: MouseButtonState::Up,
                            ..
                        } = event
                        {
                            show_main_window(tray.app_handle());
                        }
                    })
                    .build(app)?;

            if is_start_hidden() {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.hide();
                }
            }

            if let Err(reason) =
                crate::services::desktop_service::DesktopService::restore_saved_assignments()
            {
                eprintln!("desktop restore failed during startup: {reason}");
            }

            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window.state::<QuitRequested>().0.load(Ordering::Relaxed) {
                    return;
                }
                api.prevent_close();
                if let Err(reason) = window.hide() {
                    eprintln!("failed to hide window on close request: {reason}");
                }
            }
        })
}

#[cfg(test)]
mod tests {
    #[test]
    fn app_name_uses_lwe_code_name() {
        assert_eq!(super::APP_CODE_NAME, "lwe");
    }
}
