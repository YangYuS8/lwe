use crate::assembly::card_cover::card_cover_path;
use crate::assembly::compatibility::compatibility_summary;
use crate::models::LibraryPageSnapshot;
use crate::models::{ItemType, LibraryItemSummary, LibrarySource, WorkshopAgeRating};
use crate::policies::shared::support_policy::supports_first_release;
use crate::results::desktop::DesktopPageResult;
use crate::results::library::LibraryProjection;
use crate::results::workshop::AssessedWorkshopCatalogEntry;
use crate::services::library_service::LibraryService;
use lwe_library::WorkshopProjectType;

fn item_type_from_project_type(project_type: WorkshopProjectType) -> ItemType {
    match project_type {
        WorkshopProjectType::Video => ItemType::Video,
        WorkshopProjectType::Scene => ItemType::Scene,
        WorkshopProjectType::Web => ItemType::Web,
        WorkshopProjectType::Other => ItemType::Application,
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
        workshop_id: entry.entry.workshop_id.to_string(),
        title: entry.entry.title.clone(),
        item_type: item_type_from_project_type(entry.entry.project_type),
        cover_path: card_cover_path(&entry.entry),
        age_rating,
        source: LibrarySource::Workshop,
        compatibility: compatibility_summary(&entry.compatibility),
        apply_supported: supports_first_release(entry.entry.project_type),
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
        assessed_entry_with(
            "video-7",
            "Forest Video",
            WorkshopProjectType::Video,
            CompatibilityDecision {
                level: CompatibilityLevel::FullySupported,
                reason: CompatibilityReason::ReadyForLibrary,
                next_step: CompatibilityNextStep::None,
            },
        )
    }

    fn assessed_entry_with(
        item_id: &str,
        title: &str,
        project_type: WorkshopProjectType,
        compatibility: CompatibilityDecision,
    ) -> AssessedWorkshopCatalogEntry {
        AssessedWorkshopCatalogEntry {
            entry: WorkshopCatalogEntry {
                workshop_id: 7,
                title: title.to_string(),
                project_type,
                project_dir: std::path::PathBuf::from("/tmp/7"),
                cover_path: None,
                sync_state: WorkshopSyncState::Synced,
                supported_first_release: matches!(project_type, WorkshopProjectType::Video),
                library_item_id: Some(item_id.to_string()),
            },
            compatibility,
            project_metadata: WorkshopProjectMetadata::default(),
        }
    }

    fn desktop_page() -> DesktopPageResult {
        DesktopPageResult {
            monitors: Vec::new(),
            assignments: std::collections::BTreeMap::new(),
            resolved_assignments: std::collections::BTreeMap::new(),
            library_item_assignments: std::collections::BTreeMap::new(),
            running_outputs: Default::default(),
            restore_issues: Vec::new(),
            runtime_issue: None,
            monitors_available: true,
            monitor_discovery_issue: None,
            persistence_issue: None,
            assignments_available: true,
            stale: false,
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
        assert_eq!(snapshot.items[0].id, "video-7");
        assert_eq!(snapshot.items[0].title, "Forest Video");
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
        assignments.insert("video-7".to_string(), vec!["Primary".to_string()]);

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

    #[test]
    fn support_matrix_library_page_keeps_scene_visible_but_not_applyable() {
        let snapshot = assemble_library_page(
            LibraryProjection {
                entries: vec![assessed_entry_with(
                    "scene-7",
                    "Forest Scene",
                    WorkshopProjectType::Scene,
                    CompatibilityDecision {
                        level: CompatibilityLevel::PartiallySupported,
                        reason: CompatibilityReason::RecognizedButRuntimeUnsupported,
                        next_step: CompatibilityNextStep::WaitForFutureSupport,
                    },
                )],
                source_catalog_count: 1,
            },
            &desktop_page(),
        );

        assert_eq!(snapshot.items.len(), 1);
        assert_eq!(snapshot.items[0].id, "scene-7");
        assert_eq!(snapshot.items[0].item_type, ItemType::Scene);
        assert!(!snapshot.items[0].apply_supported);
        assert_eq!(
            snapshot.items[0].compatibility.reason_code,
            "recognized_but_runtime_unsupported"
        );
    }
}
