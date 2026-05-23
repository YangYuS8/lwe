use crate::action_outcome::ActionOutcome;
use crate::assembly::action_outcome::assemble_desktop_apply_outcome;
use crate::assembly::desktop_page::assemble_desktop_page;
use crate::models::DesktopPageSnapshot;
use crate::services::desktop_service::DesktopService;

#[tauri::command]
pub fn load_desktop_page() -> Result<DesktopPageSnapshot, String> {
    DesktopService::load_page().map(assemble_desktop_page)
}

#[tauri::command]
pub fn apply_library_item_to_monitor(
    monitor_id: String,
    item_id: String,
) -> Result<ActionOutcome<()>, String> {
    Ok(assemble_desktop_apply_outcome(
        DesktopService::apply_to_monitor(&monitor_id, &item_id)?,
    ))
}

#[tauri::command]
pub fn clear_library_item_from_monitor(monitor_id: String) -> Result<ActionOutcome<()>, String> {
    Ok(assemble_desktop_apply_outcome(
        DesktopService::clear_monitor(&monitor_id)?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::results::desktop::DesktopApplyResult;

    #[test]
    fn desktop_apply_flow_command_assembly_returns_failure_outcome_without_real_desktop() {
        let outcome =
            assemble_desktop_apply_outcome(DesktopApplyResult::MonitorDiscoveryUnavailable {
                reason: "niri is unavailable".to_string(),
            });

        assert!(matches!(
            outcome,
            ActionOutcome {
                ok: false,
                message: Some(_),
                ..
            }
        ));
    }
}
