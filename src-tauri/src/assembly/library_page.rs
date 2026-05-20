use crate::assembly::compatibility::compatibility_summary;
use crate::models::LibraryPageSnapshot;
use crate::models::{ItemType, LibraryItemSummary, LibrarySource, WorkshopAgeRating};
use crate::policies::shared::cover_policy::{CoverArtSource, cover_art_source};
use crate::results::desktop::DesktopPageResult;
use crate::results::library::LibraryProjection;
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

fn assemble_library_summary(entry: AssessedWorkshopCatalogEntry) -> LibraryItemSummary {
    let age_rating = entry
        .project_metadata
        .inferred_age_rating
        .as_ref()
        .map(|rating| match rating.as_str() {
            "r_18" => WorkshopAgeRating::R18,
            "pg_13" => WorkshopAgeRating::Pg13,
            _ => WorkshopAgeRating::G,
        })
        .unwrap_or(WorkshopAgeRating::G);

    LibraryItemSummary {
        id: entry.entry.library_item_id.clone().unwrap_or_default(),
        title: entry.entry.title.clone(),
        item_type: item_type_from_project_type(entry.entry.project_type),
        cover_path: cover_path(&entry.entry),
        age_rating,
        source: LibrarySource::Workshop,
        compatibility: compatibility_summary(&entry.compatibility),
        favorite: false,
        assigned_monitor_labels: Vec::new(),
    }
}

pub fn assemble_library_page(
    result: LibraryProjection,
    desktop: &DesktopPageResult,
) -> LibraryPageSnapshot {
    let desktop_status = LibraryService::desktop_status(desktop);
    let stale =
        (result.entries.is_empty() && result.source_catalog_count == 0) || desktop_status.stale;

    LibraryPageSnapshot {
        items: result
            .entries
            .into_iter()
            .map(|entry| {
                let item_id = entry.entry.library_item_id.clone().unwrap_or_default();
                let mut summary = assemble_library_summary(entry);
                summary.assigned_monitor_labels =
                    LibraryService::assigned_monitor_labels(desktop, &item_id);
                summary
            })
            .collect(),
        selected_item_id: None,
        monitors_available: desktop_status.monitors_available,
        monitor_discovery_issue: desktop_status.monitor_discovery_issue,
        desktop_assignment_issue: desktop_status.desktop_assignment_issue,
        desktop_assignments_available: desktop_status.desktop_assignments_available,
        stale,
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
    use crate::results::library::LibraryProjection;
    use crate::results::workshop::{AssessedWorkshopCatalogEntry, WorkshopProjectMetadata};
    use lwe_library::{WorkshopCatalogEntry, WorkshopProjectType, WorkshopSyncState};

    fn assessed_entry() -> AssessedWorkshopCatalogEntry {
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
        }
    }

    #[test]
    fn assembler_turns_library_projection_entries_into_page_snapshot() {
        let snapshot = assemble_library_page(
            LibraryProjection {
                entries: vec![assessed_entry()],
                source_catalog_count: 1,
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

        assert_eq!(snapshot.items.len(), 1);
        assert_eq!(snapshot.items[0].id, "scene-7");
        assert_eq!(snapshot.items[0].title, "Forest Scene");
        assert!(!snapshot.desktop_assignments_available);
        assert_eq!(
            snapshot.desktop_assignment_issue.as_deref(),
            Some("Desktop persistence is not available yet")
        );
        assert_eq!(
            snapshot.monitor_discovery_issue.as_deref(),
            Some("Monitor discovery is not available yet")
        );
        assert!(snapshot.stale);
    }

    #[test]
    fn desktop_apply_flow_library_page_includes_assigned_monitor_labels_for_matching_items() {
        let mut assignments = std::collections::BTreeMap::new();
        assignments.insert("scene-7".to_string(), vec!["Primary".to_string()]);

        let snapshot = assemble_library_page(
            LibraryProjection {
                entries: vec![assessed_entry()],
                source_catalog_count: 1,
            },
            &DesktopPageResult {
                monitors: Vec::new(),
                assignments: std::collections::BTreeMap::new(),
                resolved_assignments: std::collections::BTreeMap::new(),
                library_item_assignments: assignments,
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

        assert_eq!(
            snapshot.items[0].assigned_monitor_labels,
            vec!["Primary".to_string()]
        );
    }
}
