use crate::models::DiagnosticsSnapshot;
use crate::services::diagnostics_service::DiagnosticsService;

#[tauri::command]
pub fn load_diagnostics() -> Result<DiagnosticsSnapshot, String> {
    DiagnosticsService::load_snapshot()
}
