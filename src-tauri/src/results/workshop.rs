use crate::results::compatibility::CompatibilityAssessment;
use lwe_library::{WorkshopCatalogEntry, WorkshopSyncState};

#[derive(Debug, Clone, Default)]
pub struct WorkshopProjectMetadata {
    pub description: Option<String>,
    pub tags: Vec<String>,
    pub inferred_age_rating: Option<String>,
}

#[derive(Debug, Clone)]
pub struct AssessedWorkshopCatalogEntry {
    pub entry: WorkshopCatalogEntry,
    pub compatibility: CompatibilityAssessment,
    pub project_metadata: WorkshopProjectMetadata,
}

#[derive(Debug, Clone)]
pub struct WorkshopRefreshResult {
    pub catalog_entries: Vec<AssessedWorkshopCatalogEntry>,
    pub library_refresh_required: bool,
    pub served_from_snapshot: bool,
}

impl WorkshopRefreshResult {
    pub fn synced_entry_count(&self) -> usize {
        self.catalog_entries
            .iter()
            .filter(|entry| entry.entry.sync_state == WorkshopSyncState::Synced)
            .count()
    }
}

#[derive(Debug, Clone)]
pub struct WorkshopInspection {
    pub requested_workshop_id: String,
    pub entry: AssessedWorkshopCatalogEntry,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_policy_workshop_refresh_result_tracks_library_invalidation() {
        let result = WorkshopRefreshResult {
            catalog_entries: Vec::new(),
            library_refresh_required: true,
            served_from_snapshot: false,
        };

        assert!(result.library_refresh_required);
        assert!(!result.served_from_snapshot);
    }
}
