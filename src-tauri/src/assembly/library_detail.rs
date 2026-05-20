use crate::assembly::compatibility::compatibility_explanation;
use crate::models::{ItemType, LibraryItemDetail, LibrarySource};
use crate::policies::shared::cover_policy::{CoverArtSource, cover_art_source};
use crate::results::desktop::DesktopPageResult;
use crate::results::workshop::AssessedWorkshopCatalogEntry;
use crate::services::library_service::LibraryService;
use lwe_library::{WorkshopCatalogEntry, WorkshopProjectType};

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

pub fn assemble_library_detail(
    entry: AssessedWorkshopCatalogEntry,
    desktop: &DesktopPageResult,
) -> LibraryItemDetail {
    let desktop_status = LibraryService::desktop_status(desktop);
    let assignment_issue = desktop_status.desktop_assignment_issue.clone();
    let monitor_discovery_issue = desktop_status.monitor_discovery_issue.clone();
    let id = entry.entry.library_item_id.clone().unwrap_or_default();
    let title = entry.entry.title.clone();
    let item_type = item_type_from_project_type(entry.entry.project_type);
    let cover_path = cover_path(&entry.entry);
    let description = entry.project_metadata.description.clone();
    let tags = entry.project_metadata.tags.clone();
    let assigned_monitor_labels = LibraryService::assigned_monitor_labels(desktop, &id);
    let compatibility = compatibility_explanation(&entry.compatibility);

    LibraryItemDetail {
        id,
        title,
        item_type,
        cover_path,
        source: LibrarySource::Workshop,
        compatibility,
        monitors_available: desktop_status.monitors_available,
        monitor_discovery_issue,
        desktop_assignment_issue: assignment_issue,
        desktop_assignments_available: desktop_status.desktop_assignments_available,
        assigned_monitor_labels,
        description,
        tags,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policies::shared::compatibility_policy::{
        CompatibilityDecision, CompatibilityLevel, CompatibilityReason,
    };
    use crate::results::compatibility::CompatibilityNextStep;
    use crate::results::desktop::DesktopPageResult;
    use crate::results::workshop::WorkshopProjectMetadata;
    use lwe_library::{WorkshopCatalogEntry, WorkshopProjectType, WorkshopSyncState};

    #[test]
    fn desktop_apply_flow_library_detail_mentions_assignment_unavailability() {
        let detail = assemble_library_detail(
            AssessedWorkshopCatalogEntry {
                entry: WorkshopCatalogEntry {
                    workshop_id: 7,
                    title: "Forest Scene".to_string(),
                    project_type: WorkshopProjectType::Scene,
                    project_dir: std::path::PathBuf::from("/tmp/7"),
                    cover_path: None,
                    sync_state: WorkshopSyncState::Synced,
                    supported_first_release: true,
                    library_item_id: Some("scene-7".to_string()),
                },
                compatibility: CompatibilityDecision {
                    level: CompatibilityLevel::FullySupported,
                    reason: CompatibilityReason::ReadyForLibrary,
                    next_step: CompatibilityNextStep::None,
                },
                project_metadata: WorkshopProjectMetadata::default(),
            },
            &DesktopPageResult {
                monitors: Vec::new(),
                assignments: std::collections::BTreeMap::new(),
                resolved_assignments: std::collections::BTreeMap::new(),
                library_item_assignments: std::collections::BTreeMap::new(),
                running_outputs: Default::default(),
                restore_issues: Vec::new(),
                runtime_issue: None,
                monitors_available: false,
                monitor_discovery_issue: Some("Monitor discovery is not available yet".to_string()),
                persistence_issue: Some("Desktop persistence is not available yet".to_string()),
                assignments_available: false,
                stale: true,
            },
        );

        assert_eq!(
            detail.compatibility.detail,
            "This item is synchronized locally and available for Library and desktop use."
        );
        assert!(!detail.desktop_assignments_available);
        assert_eq!(
            detail.desktop_assignment_issue.as_deref(),
            Some("Desktop persistence is not available yet")
        );
        assert_eq!(
            detail.monitor_discovery_issue.as_deref(),
            Some("Monitor discovery is not available yet")
        );
    }

    #[test]
    fn desktop_apply_flow_library_detail_includes_assigned_monitor_labels() {
        let mut library_item_assignments = std::collections::BTreeMap::new();
        library_item_assignments.insert("scene-7".to_string(), vec!["Primary".to_string()]);

        let detail = assemble_library_detail(
            AssessedWorkshopCatalogEntry {
                entry: WorkshopCatalogEntry {
                    workshop_id: 7,
                    title: "Forest Scene".to_string(),
                    project_type: WorkshopProjectType::Scene,
                    project_dir: std::path::PathBuf::from("/tmp/7"),
                    cover_path: None,
                    sync_state: WorkshopSyncState::Synced,
                    supported_first_release: true,
                    library_item_id: Some("scene-7".to_string()),
                },
                compatibility: CompatibilityDecision {
                    level: CompatibilityLevel::FullySupported,
                    reason: CompatibilityReason::ReadyForLibrary,
                    next_step: CompatibilityNextStep::None,
                },
                project_metadata: WorkshopProjectMetadata::default(),
            },
            &DesktopPageResult {
                monitors: Vec::new(),
                assignments: std::collections::BTreeMap::new(),
                resolved_assignments: std::collections::BTreeMap::new(),
                library_item_assignments,
                running_outputs: Default::default(),
                restore_issues: Vec::new(),
                runtime_issue: None,
                monitors_available: true,
                monitor_discovery_issue: None,
                persistence_issue: None,
                assignments_available: true,
                stale: false,
            },
        );

        assert_eq!(detail.assigned_monitor_labels, vec!["Primary".to_string()]);
    }
}
