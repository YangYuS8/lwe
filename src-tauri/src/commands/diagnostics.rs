use crate::commands::background::run_desktop_blocking;
use crate::models::DiagnosticsSnapshot;
use crate::services::diagnostics_service::DiagnosticsService;

#[tauri::command]
pub async fn load_diagnostics() -> Result<DiagnosticsSnapshot, String> {
    run_desktop_blocking(DiagnosticsService::load_snapshot).await
}
