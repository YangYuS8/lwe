use crate::assembly::compatibility::compatibility_explanation;
use crate::models::{ItemType, WorkshopItemDetail, WorkshopSyncStatus};
use crate::policies::shared::cover_policy::{CoverArtSource, cover_art_source};
use crate::results::workshop::WorkshopInspection;
use lwe_library::{WorkshopCatalogEntry, WorkshopProjectType, WorkshopSyncState};

fn item_type_from_project_type(project_type: WorkshopProjectType) -> ItemType {
    match project_type {
        WorkshopProjectType::Video => ItemType::Video,
        WorkshopProjectType::Scene => ItemType::Scene,
        WorkshopProjectType::Web => ItemType::Web,
        WorkshopProjectType::Other => ItemType::Application,
    }
}

fn cover_path(entry: &WorkshopCatalogEntry) -> Option<String> {
    let bundled_cover_path = entry
        .cover_path
        .as_ref()
        .map(|path| path.to_string_lossy().into_owned());

    match cover_art_source(bundled_cover_path) {
        CoverArtSource::Bundled(path) => Some(path),
        CoverArtSource::Placeholder => None,
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

pub fn assemble_workshop_detail(result: WorkshopInspection) -> WorkshopItemDetail {
    let entry = result.entry;
    let id = entry.entry.workshop_id.to_string();
    let title = entry.entry.title.clone();
    let item_type = item_type_from_project_type(entry.entry.project_type);
    let cover_path = cover_path(&entry.entry);
    let sync_status = sync_status(&entry.entry);
    let compatibility = compatibility_explanation(&entry.compatibility);
    let description = entry.project_metadata.description.clone();
    let tags = entry.project_metadata.tags.clone();

    WorkshopItemDetail {
        id,
        title,
        item_type,
        cover_path,
        sync_status,
        compatibility,
        tags,
        description,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::CompatibilityBadge;
    use crate::policies::shared::compatibility_policy::{
        CompatibilityDecision, CompatibilityLevel, CompatibilityNextStep, CompatibilityReason,
    };
    use crate::results::workshop::{AssessedWorkshopCatalogEntry, WorkshopProjectMetadata};
    use std::path::PathBuf;

    fn inspection(
        project_type: WorkshopProjectType,
        sync_state: WorkshopSyncState,
        compatibility: CompatibilityDecision,
    ) -> WorkshopInspection {
        WorkshopInspection {
            requested_workshop_id: "77".to_string(),
            entry: AssessedWorkshopCatalogEntry {
                entry: WorkshopCatalogEntry {
                    workshop_id: 77,
                    title: "Unsupported Web".to_string(),
                    project_type,
                    project_dir: PathBuf::from("/tmp/77"),
                    cover_path: None,
                    sync_state,
                    supported_first_release: false,
                    library_item_id: None,
                },
                compatibility,
                project_metadata: WorkshopProjectMetadata {
                    description: Some("A web wallpaper".to_string()),
                    tags: vec!["web".to_string()],
                    inferred_age_rating: None,
                },
            },
        }
    }

    #[test]
    fn support_matrix_workshop_detail_reports_web_as_unsupported_without_runtime_claim() {
        let detail = assemble_workshop_detail(inspection(
            WorkshopProjectType::Web,
            WorkshopSyncState::UnsupportedType,
            CompatibilityDecision {
                level: CompatibilityLevel::Unsupported,
                reason: CompatibilityReason::UnsupportedWebItem,
                next_step: CompatibilityNextStep::WaitForFutureSupport,
            },
        ));

        assert_eq!(detail.id, "77");
        assert_eq!(detail.item_type, ItemType::Web);
        assert_eq!(detail.sync_status, WorkshopSyncStatus::UnsupportedType);
        assert_eq!(detail.compatibility.badge, CompatibilityBadge::Unsupported);
        assert_eq!(detail.compatibility.reason_code, "unsupported_web_item");
        assert!(detail.compatibility.detail.contains("Web Workshop items"));
        assert!(
            detail
                .compatibility
                .detail
                .contains("limited to video wallpapers")
        );
    }
}
