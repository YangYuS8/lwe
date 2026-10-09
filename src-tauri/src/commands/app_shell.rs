use crate::assembly::app_shell::assemble_app_shell;
use crate::commands::background::run_blocking;
use crate::models::AppShellSnapshot;
use crate::services::app_shell_service::AppShellService;

#[tauri::command]
pub async fn load_app_shell() -> Result<AppShellSnapshot, String> {
    run_blocking(|| Ok(assemble_app_shell(AppShellService::load_summary()?))).await
}
