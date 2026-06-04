use crate::action_outcome::ActionOutcome;
use crate::assembly::action_outcome::assemble_workshop_refresh_outcome;
use crate::assembly::workshop_detail::assemble_workshop_detail;
use crate::assembly::workshop_page::assemble_workshop_page;
use crate::models::{
    WorkshopItemDetail, WorkshopOnlineSearchInput, WorkshopOnlineSearchResult, WorkshopPageSnapshot,
};
use crate::services::workshop_service::WorkshopService;

fn workshop_item_url(workshop_id: &str) -> String {
    format!("https://steamcommunity.com/sharedfiles/filedetails/?id={workshop_id}")
}

fn steam_openurl(workshop_id: &str) -> String {
    format!("steam://openurl/{}", workshop_item_url(workshop_id))
}

#[tauri::command]
pub fn load_workshop_page() -> Result<WorkshopPageSnapshot, String> {
    let result =
        WorkshopService::load_catalog_snapshot().or_else(|_| WorkshopService::refresh_catalog())?;
    Ok(assemble_workshop_page(&result))
}

#[tauri::command]
pub fn load_workshop_item_detail(workshop_id: String) -> Result<WorkshopItemDetail, String> {
    Ok(assemble_workshop_detail(WorkshopService::inspect_item(
        &workshop_id,
    )?))
}

#[tauri::command]
pub fn refresh_workshop_catalog() -> Result<ActionOutcome<WorkshopPageSnapshot>, String> {
    Ok(assemble_workshop_refresh_outcome(
        &WorkshopService::refresh_catalog()?,
    ))
}

#[tauri::command]
pub fn search_workshop_online(
    input: WorkshopOnlineSearchInput,
) -> Result<WorkshopOnlineSearchResult, String> {
    WorkshopService::search_online(input)
}

#[tauri::command]
pub fn open_workshop_in_steam(workshop_id: String) -> Result<ActionOutcome<()>, String> {
    open::that_detached(steam_openurl(&workshop_id)).map_err(|error| error.to_string())?;

    Ok(ActionOutcome {
        ok: true,
        message: Some("Opened item in Steam".to_string()),
        shell_patch: None,
        current_update: None,
        invalidations: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steam_url_uses_official_workshop_page() {
        assert_eq!(
            workshop_item_url("12345"),
            "https://steamcommunity.com/sharedfiles/filedetails/?id=12345"
        );
    }

    #[test]
    fn steam_openurl_wraps_official_workshop_page() {
        assert_eq!(
            steam_openurl("12345"),
            "steam://openurl/https://steamcommunity.com/sharedfiles/filedetails/?id=12345"
        );
    }
}
