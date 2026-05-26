use crate::assembly::card_cover::card_cover_path;
use crate::assembly::compatibility::compatibility_summary;
use crate::models::{ItemType, WorkshopItemSummary, WorkshopPageSnapshot, WorkshopSyncStatus};
use crate::results::workshop::{AssessedWorkshopCatalogEntry, WorkshopRefreshResult};
use lwe_library::{WorkshopCatalogEntry, WorkshopProjectType, WorkshopSyncState};

fn item_type_from_project_type(project_type: WorkshopProjectType) -> ItemType {
    match project_type {
        WorkshopProjectType::Video => ItemType::Video,
        WorkshopProjectType::Scene => ItemType::Scene,
        WorkshopProjectType::Web => ItemType::Web,
        WorkshopProjectType::Other => ItemType::Application,
    }
}

fn sync_status(entry: &WorkshopCatalogEntry) -> WorkshopSyncStatus {
    match entry.sync_state {
        WorkshopSyncState::Synced => WorkshopSyncStatus::Synced,
        WorkshopSyncState::MissingProjectFile => WorkshopSyncStatus::MissingProject,
        WorkshopSyncState::MissingPrimaryAsset => WorkshopSyncStatus::MissingAsset,
        WorkshopSyncState::UnsupportedType => WorkshopSyncStatus::UnsupportedType,
    }
}

fn workshop_summary_from_entry(
    assessed_entry: AssessedWorkshopCatalogEntry,
) -> WorkshopItemSummary {
    let workshop_id = assessed_entry.entry.workshop_id.to_string();
    let title = assessed_entry.entry.title.clone();
    let item_type = item_type_from_project_type(assessed_entry.entry.project_type);
    let cover_path = card_cover_path(&assessed_entry.entry);
    let sync_status = sync_status(&assessed_entry.entry);
    let compatibility = compatibility_summary(&assessed_entry.compatibility);

    WorkshopItemSummary {
        id: workshop_id,
        title,
        item_type,
        cover_path,
        sync_status,
        compatibility,
    }
}

pub fn assemble_workshop_page(result: &WorkshopRefreshResult) -> WorkshopPageSnapshot {
    WorkshopPageSnapshot {
        items: result
            .catalog_entries
            .clone()
            .into_iter()
            .map(workshop_summary_from_entry)
            .collect(),
        selected_item_id: None,
        stale: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::results::workshop::WorkshopRefreshResult;

    #[test]
    fn assembler_turns_app_result_into_page_snapshot() {
        let result = WorkshopRefreshResult {
            catalog_entries: Vec::new(),
            library_refresh_required: true,
        };

        let snapshot = assemble_workshop_page(&result);
        assert!(snapshot.items.is_empty());
    }
}
