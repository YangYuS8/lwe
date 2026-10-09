use crate::action_outcome::{ActionOutcome, InvalidatedPage};
use crate::assembly::settings_page::assemble_settings_page;
use crate::commands::background::run_blocking;
use crate::models::{SettingsPageSnapshot, SettingsUpdateInput};
use crate::services::settings_service::SettingsService;
use std::sync::Mutex;

// Keep the persisted language and tray menu ordered together across IPC writes.
static SETTINGS_UPDATE_COMMAND_LOCK: Mutex<()> = Mutex::new(());

fn serialize_settings_update<T>(update: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    let _guard = SETTINGS_UPDATE_COMMAND_LOCK
        .lock()
        .map_err(|_| "Settings update is unavailable".to_string())?;
    update()
}

#[tauri::command]
pub async fn load_settings_page() -> Result<SettingsPageSnapshot, String> {
    run_blocking(|| SettingsService::load_page().map(assemble_settings_page)).await
}

#[tauri::command]
pub async fn update_settings(
    app: tauri::AppHandle,
    input: SettingsUpdateInput,
) -> Result<ActionOutcome<SettingsPageSnapshot>, String> {
    run_blocking(move || {
        serialize_settings_update(|| {
            let requested_language = input.language.clone();
            let snapshot = assemble_settings_page(SettingsService::update_settings(input)?);

            if let Some(language) = requested_language {
                crate::update_tray_menu_language(&app, &language);
            }

            Ok(ActionOutcome {
                ok: true,
                message: Some("Settings updated".to_string()),
                shell_patch: None,
                current_update: Some(snapshot),
                invalidations: vec![InvalidatedPage::Settings],
            })
        })
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, mpsc};
    use std::time::Duration;

    #[test]
    fn settings_save_and_tray_update_do_not_interleave() {
        let changes = Arc::new(Mutex::new(Vec::new()));
        let (saved_tx, saved_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let first_changes = changes.clone();
        let first = std::thread::spawn(move || {
            serialize_settings_update(|| {
                first_changes.lock().unwrap().push("save zh");
                saved_tx.send(()).unwrap();
                release_rx.recv_timeout(Duration::from_secs(2)).unwrap();
                first_changes.lock().unwrap().push("tray zh");
                Ok(())
            })
        });
        saved_rx.recv_timeout(Duration::from_secs(2)).unwrap();

        let (second_done_tx, second_done_rx) = mpsc::channel();
        let second_changes = changes.clone();
        let second = std::thread::spawn(move || {
            let result = serialize_settings_update(|| {
                second_changes.lock().unwrap().push("save en");
                second_changes.lock().unwrap().push("tray en");
                Ok(())
            });
            second_done_tx.send(()).unwrap();
            result
        });

        let finished_early = second_done_rx.recv_timeout(Duration::from_millis(50));
        release_tx.send(()).unwrap();
        first.join().unwrap().unwrap();
        second.join().unwrap().unwrap();

        assert!(matches!(
            finished_early,
            Err(mpsc::RecvTimeoutError::Timeout)
        ));
        assert_eq!(
            *changes.lock().unwrap(),
            ["save zh", "tray zh", "save en", "tray en"]
        );
    }

    #[test]
    fn settings_snapshot_uses_real_settings_fields() {
        let _guard = crate::test_env::env_lock();
        let snapshot = tauri::async_runtime::block_on(load_settings_page()).unwrap();

        assert!(!snapshot.language.is_empty());
        assert!(!snapshot.theme.is_empty());
        assert!(snapshot.steam_status_message.contains("Steam"));
    }
}
