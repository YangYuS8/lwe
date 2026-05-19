use crate::policies::shared::compatibility_policy::{CompatibilityLevel, compatibility_decision};
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
        entry.compatibility.level == CompatibilityLevel::FullySupported
            && entry.entry.library_item_id.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policies::shared::compatibility_policy::CompatibilityReason;
    use lwe_library::{WorkshopProjectType, WorkshopSyncState};
    use std::path::PathBuf;

    fn synced_scene_entry() -> WorkshopCatalogEntry {
        WorkshopCatalogEntry {
            workshop_id: 42,
            title: "Forest Scene".to_string(),
            project_type: WorkshopProjectType::Scene,
            project_dir: PathBuf::from("/tmp/42"),
            cover_path: None,
            sync_state: WorkshopSyncState::Synced,
            supported_first_release: true,
            library_item_id: Some("scene-42".to_string()),
        }
    }

    #[test]
    fn compatibility_service_assesses_catalog_entries_once_for_service_consumers() {
        let assessed = CompatibilityService::assess_catalog_entry(synced_scene_entry());

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
        let assessed = CompatibilityService::assess_catalog_entry(synced_scene_entry());

        assert!(CompatibilityService::supports_library_projection(&assessed));
    }
}
