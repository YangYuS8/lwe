use crate::assembly::library_detail::assemble_library_detail;
use crate::assembly::library_page::assemble_library_page;
use crate::models::{LibraryItemDetail, LibraryPageSnapshot};
use crate::services::desktop_service::DesktopService;
use crate::services::library_service::LibraryService;

#[tauri::command]
pub fn load_library_page() -> Result<LibraryPageSnapshot, String> {
    let projection = LibraryService::load_projection()?;
    let desktop = DesktopService::load_page_with_projection(Ok(projection.clone()))?;

    Ok(assemble_library_page(projection, &desktop))
}

#[tauri::command]
pub fn load_library_item_detail(item_id: String) -> Result<LibraryItemDetail, String> {
    let projection = LibraryService::load_projection()?;
    let desktop = DesktopService::load_page_with_projection(Ok(projection.clone()))?;

    Ok(assemble_library_detail(
        LibraryService::inspect_item_in_projection(&projection, &item_id)?,
        &desktop,
    ))
}

#[cfg(test)]
mod tests {
    use crate::assembly::library_page::assemble_library_page;
    use crate::results::desktop::DesktopPageResult;
    use crate::results::library::LibraryProjection;
    use std::collections::BTreeMap;

    #[test]
    fn desktop_apply_flow_library_page_reuses_desktop_state_in_snapshot() {
        let projection = LibraryProjection {
            entries: Vec::new(),
            source_catalog_count: 1,
        };
        let desktop = DesktopPageResult {
            monitors: Vec::new(),
            assignments: BTreeMap::new(),
            resolved_assignments: BTreeMap::new(),
            library_item_assignments: BTreeMap::new(),
            running_outputs: Default::default(),
            restore_issues: Vec::new(),
            runtime_issue: None,
            monitors_available: true,
            monitor_discovery_issue: None,
            persistence_issue: None,
            assignments_available: true,
            stale: false,
        };

        let snapshot = assemble_library_page(projection, &desktop);

        assert!(snapshot.desktop_assignment_issue.is_none());
        assert!(snapshot.desktop_assignments_available);

        if snapshot.monitors_available {
            assert!(snapshot.monitor_discovery_issue.is_none());
        } else {
            assert!(snapshot.monitor_discovery_issue.is_some());
        }
    }
}
