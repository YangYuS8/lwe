use crate::policies::shared::compatibility_policy::{
    CompatibilityLevel, CompatibilityReason, compatibility_decision,
};
use crate::results::workshop::{AssessedWorkshopCatalogEntry, WorkshopProjectMetadata};
use lwe_library::{WeProject, WorkshopCatalogEntry};

pub struct CompatibilityService;

impl CompatibilityService {
    fn project_metadata(entry: &WorkshopCatalogEntry) -> WorkshopProjectMetadata {
        WeProject::load(&entry.project_dir)
            .map(|project| WorkshopProjectMetadata {
                description: project.description,
                tags: project.tags,
                inferred_age_rating: None,
            })
            .unwrap_or_default()
    }

    pub fn assess_catalog_entry(entry: WorkshopCatalogEntry) -> AssessedWorkshopCatalogEntry {
        let compatibility = compatibility_decision(&entry);
        let project_metadata = Self::project_metadata(&entry);

        AssessedWorkshopCatalogEntry {
            entry,
            compatibility,
            project_metadata,
        }
    }

    pub fn assess_catalog_entries(
        entries: Vec<WorkshopCatalogEntry>,
    ) -> Vec<AssessedWorkshopCatalogEntry> {
        entries
            .into_iter()
            .map(Self::assess_catalog_entry)
            .collect()
    }

    pub fn supports_library_projection(entry: &AssessedWorkshopCatalogEntry) -> bool {
        entry.entry.library_item_id.is_some()
            && (entry.compatibility.level == CompatibilityLevel::FullySupported
                || entry.compatibility.reason
                    == CompatibilityReason::RecognizedButRuntimeUnsupported)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policies::shared::compatibility_policy::CompatibilityNextStep;
    use lwe_library::{WorkshopProjectType, WorkshopSyncState};
    use std::path::PathBuf;

    fn synced_video_entry() -> WorkshopCatalogEntry {
        WorkshopCatalogEntry {
            workshop_id: 42,
            title: "Forest Video".to_string(),
            project_type: WorkshopProjectType::Video,
            project_dir: PathBuf::from("/tmp/42"),
            cover_path: None,
            sync_state: WorkshopSyncState::Synced,
            supported_first_release: true,
            library_item_id: Some("video-42".to_string()),
        }
    }

    fn catalog_entry(
        workshop_id: u64,
        project_type: WorkshopProjectType,
        sync_state: WorkshopSyncState,
        library_item_id: Option<&str>,
    ) -> WorkshopCatalogEntry {
        WorkshopCatalogEntry {
            workshop_id,
            title: format!("Item {workshop_id}"),
            project_type,
            project_dir: PathBuf::from(format!("/tmp/{workshop_id}")),
            cover_path: None,
            sync_state,
            supported_first_release: matches!(project_type, WorkshopProjectType::Video)
                && matches!(sync_state, WorkshopSyncState::Synced),
            library_item_id: library_item_id.map(str::to_string),
        }
    }

    #[test]
    fn compatibility_service_assesses_catalog_entries_once_for_service_consumers() {
        let assessed = CompatibilityService::assess_catalog_entry(synced_video_entry());

        assert_eq!(
            assessed.compatibility.level,
            CompatibilityLevel::FullySupported
        );
        assert_eq!(
            assessed.compatibility.reason,
            CompatibilityReason::ReadyForLibrary
        );
    }

    #[test]
    fn compatibility_service_uses_assessment_for_library_projection_gate() {
        let assessed = CompatibilityService::assess_catalog_entry(synced_video_entry());

        assert!(CompatibilityService::supports_library_projection(&assessed));
    }

    #[test]
    fn compatibility_service_includes_recognized_scene_in_library_projection() {
        let assessed = CompatibilityService::assess_catalog_entry(WorkshopCatalogEntry {
            workshop_id: 43,
            title: "Forest Scene".to_string(),
            project_type: WorkshopProjectType::Scene,
            project_dir: PathBuf::from("/tmp/43"),
            cover_path: None,
            sync_state: WorkshopSyncState::Synced,
            supported_first_release: false,
            library_item_id: Some("scene-43".to_string()),
        });

        assert_eq!(
            assessed.compatibility.reason,
            CompatibilityReason::RecognizedButRuntimeUnsupported
        );
        assert!(CompatibilityService::supports_library_projection(&assessed));
    }

    #[test]
    fn compatibility_service_excludes_degraded_video_without_runtime_asset_from_library_projection()
    {
        let assessed = AssessedWorkshopCatalogEntry {
            entry: WorkshopCatalogEntry {
                workshop_id: 44,
                title: "Broken Video".to_string(),
                project_type: WorkshopProjectType::Video,
                project_dir: PathBuf::from("/tmp/44"),
                cover_path: None,
                sync_state: WorkshopSyncState::MissingPrimaryAsset,
                supported_first_release: false,
                library_item_id: Some("video-44".to_string()),
            },
            compatibility: crate::policies::shared::compatibility_policy::CompatibilityDecision {
                level: CompatibilityLevel::PartiallySupported,
                reason: CompatibilityReason::MissingPrimaryAsset,
                next_step: CompatibilityNextStep::ResyncWorkshopItem,
            },
            project_metadata: WorkshopProjectMetadata::default(),
        };

        assert!(!CompatibilityService::supports_library_projection(
            &assessed
        ));
    }

    #[test]
    fn compatibility_service_excludes_web_items_from_library_projection() {
        let assessed = CompatibilityService::assess_catalog_entry(catalog_entry(
            45,
            WorkshopProjectType::Web,
            WorkshopSyncState::UnsupportedType,
            Some("web-45"),
        ));

        assert_eq!(
            assessed.compatibility.reason,
            CompatibilityReason::UnsupportedWebItem
        );
        assert!(!CompatibilityService::supports_library_projection(
            &assessed
        ));
    }

    #[test]
    fn compatibility_service_excludes_missing_project_metadata_from_library_projection() {
        let assessed = CompatibilityService::assess_catalog_entry(catalog_entry(
            46,
            WorkshopProjectType::Video,
            WorkshopSyncState::MissingProjectFile,
            Some("video-46"),
        ));

        assert_eq!(
            assessed.compatibility.reason,
            CompatibilityReason::MissingProjectMetadata
        );
        assert!(!CompatibilityService::supports_library_projection(
            &assessed
        ));
    }

    #[test]
    fn compatibility_service_preserves_scene_runtime_limitation_for_missing_asset() {
        let assessed = CompatibilityService::assess_catalog_entry(catalog_entry(
            47,
            WorkshopProjectType::Scene,
            WorkshopSyncState::MissingPrimaryAsset,
            Some("scene-47"),
        ));

        assert_eq!(
            assessed.compatibility.reason,
            CompatibilityReason::RecognizedButRuntimeUnsupported
        );
        assert!(CompatibilityService::supports_library_projection(&assessed));
    }
}
