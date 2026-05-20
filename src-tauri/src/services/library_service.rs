use crate::results::desktop::DesktopPageResult;
use crate::results::library::LibraryProjection;
use crate::results::workshop::{AssessedWorkshopCatalogEntry, WorkshopRefreshResult};
use crate::services::compatibility_service::CompatibilityService;
use crate::services::desktop_service::LIBRARY_RESOLUTION_ISSUE_PREFIX;
use crate::services::workshop_service::WorkshopService;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryDesktopStatus {
    pub monitors_available: bool,
    pub monitor_discovery_issue: Option<String>,
    pub desktop_assignment_issue: Option<String>,
    pub desktop_assignments_available: bool,
    pub stale: bool,
}

fn includes_library_item(entry: &AssessedWorkshopCatalogEntry) -> bool {
    CompatibilityService::supports_library_projection(entry)
}

pub struct LibraryService;

impl LibraryService {
    pub fn projection_from_refresh(refresh: WorkshopRefreshResult) -> LibraryProjection {
        let source_catalog_count = refresh.catalog_entries.len();
        let entries = refresh
            .catalog_entries
            .into_iter()
            .filter(includes_library_item)
            .collect();

        LibraryProjection {
            entries,
            source_catalog_count,
        }
    }

    pub fn load_projection() -> Result<LibraryProjection, String> {
        Ok(Self::projection_from_refresh(
            WorkshopService::refresh_catalog()?,
        ))
    }

    pub fn inspect_item_in_projection(
        projection: &LibraryProjection,
        item_id: &str,
    ) -> Result<AssessedWorkshopCatalogEntry, String> {
        projection
            .entries
            .iter()
            .find(|entry| entry.entry.library_item_id.as_deref() == Some(item_id))
            .cloned()
            .ok_or_else(|| format!("Library item {item_id} not found"))
    }

    pub fn inspect_item(item_id: &str) -> Result<AssessedWorkshopCatalogEntry, String> {
        let projection = Self::load_projection()?;
        Self::inspect_item_in_projection(&projection, item_id)
    }

    pub fn desktop_status(desktop: &DesktopPageResult) -> LibraryDesktopStatus {
        LibraryDesktopStatus {
            monitors_available: desktop.monitors_available,
            monitor_discovery_issue: desktop.monitor_discovery_issue.clone(),
            desktop_assignment_issue: desktop.persistence_issue.clone().or_else(|| {
                desktop
                    .restore_issues
                    .iter()
                    .find(|issue| issue.starts_with(LIBRARY_RESOLUTION_ISSUE_PREFIX))
                    .cloned()
            }),
            desktop_assignments_available: desktop.assignments_available,
            stale: desktop.stale,
        }
    }

    pub fn assigned_monitor_labels(desktop: &DesktopPageResult, item_id: &str) -> Vec<String> {
        desktop
            .library_item_assignments
            .get(item_id)
            .cloned()
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::results::desktop::DesktopPageResult;
    use crate::results::library::LibraryProjection;
    use crate::results::workshop::AssessedWorkshopCatalogEntry;

    use super::LibraryService;

    #[test]
    fn service_layer_library_service_uses_application_projection_result() {
        let result = LibraryProjection {
            entries: Vec::new(),
            source_catalog_count: 0,
        };

        assert!(result.entries.is_empty());
        assert_eq!(result.source_catalog_count, 0);
    }

    #[test]
    fn service_layer_library_service_preserves_unavailable_desktop_assignment_state() {
        let status = LibraryService::desktop_status(&DesktopPageResult {
            monitors: Vec::new(),
            assignments: BTreeMap::new(),
            resolved_assignments: BTreeMap::new(),
            library_item_assignments: BTreeMap::new(),
            running_outputs: Default::default(),
            restore_issues: Vec::new(),
            runtime_issue: None,
            monitors_available: false,
            monitor_discovery_issue: Some("Monitor discovery is not available yet".to_string()),
            persistence_issue: Some("Desktop persistence is not available yet".to_string()),
            assignments_available: false,
            stale: true,
        });

        assert_eq!(
            status.monitor_discovery_issue.as_deref(),
            Some("Monitor discovery is not available yet")
        );
        assert!(!status.monitors_available);
        assert_eq!(
            status.desktop_assignment_issue.as_deref(),
            Some("Desktop persistence is not available yet")
        );
        assert!(!status.desktop_assignments_available);
        assert!(status.stale);
    }

    #[test]
    fn desktop_apply_flow_library_service_reads_assigned_monitor_labels_from_desktop_state() {
        let status = LibraryService::assigned_monitor_labels(
            &DesktopPageResult {
                monitors: Vec::new(),
                assignments: BTreeMap::new(),
                resolved_assignments: BTreeMap::new(),
                library_item_assignments: BTreeMap::from([(
                    "scene-7".to_string(),
                    vec!["Primary".to_string(), "DISPLAY-2 (missing)".to_string()],
                )]),
                running_outputs: Default::default(),
                restore_issues: Vec::new(),
                runtime_issue: None,
                monitors_available: true,
                monitor_discovery_issue: None,
                persistence_issue: None,
                assignments_available: true,
                stale: false,
            },
            "scene-7",
        );

        assert_eq!(
            status,
            vec!["Primary".to_string(), "DISPLAY-2 (missing)".to_string()]
        );
    }

    #[test]
    fn service_layer_library_service_surfaces_library_resolution_issue_as_assignment_issue() {
        let status = LibraryService::desktop_status(&DesktopPageResult {
            monitors: Vec::new(),
            assignments: BTreeMap::new(),
            resolved_assignments: BTreeMap::new(),
            library_item_assignments: BTreeMap::new(),
            running_outputs: Default::default(),
            restore_issues: vec![
                "Unable to resolve desktop items against the current Library snapshot: Library refresh failed"
                    .to_string(),
            ],
            runtime_issue: None,
            monitors_available: true,
            monitor_discovery_issue: None,
            persistence_issue: None,
            assignments_available: true,
            stale: true,
        });

        assert_eq!(
            status.desktop_assignment_issue.as_deref(),
            Some(
                "Unable to resolve desktop items against the current Library snapshot: Library refresh failed"
            )
        );
    }

    #[test]
    fn service_layer_library_service_inspects_item_from_existing_projection() {
        let entry = AssessedWorkshopCatalogEntry {
            entry: lwe_library::WorkshopCatalogEntry {
                workshop_id: 7,
                title: "Forest Scene".to_string(),
                project_type: lwe_library::WorkshopProjectType::Scene,
                project_dir: std::path::PathBuf::from("/tmp/7"),
                cover_path: None,
                sync_state: lwe_library::WorkshopSyncState::Synced,
                supported_first_release: true,
                library_item_id: Some("scene-7".to_string()),
            },
            compatibility: crate::policies::shared::compatibility_policy::compatibility_decision(
                &lwe_library::WorkshopCatalogEntry {
                    workshop_id: 7,
                    title: "Forest Scene".to_string(),
                    project_type: lwe_library::WorkshopProjectType::Scene,
                    project_dir: std::path::PathBuf::from("/tmp/7"),
                    cover_path: None,
                    sync_state: lwe_library::WorkshopSyncState::Synced,
                    supported_first_release: true,
                    library_item_id: Some("scene-7".to_string()),
                },
            ),
            project_metadata: Default::default(),
        };

        let item = LibraryService::inspect_item_in_projection(
            &LibraryProjection {
                entries: vec![entry],
                source_catalog_count: 1,
            },
            "scene-7",
        )
        .unwrap();

        assert_eq!(item.entry.library_item_id.as_deref(), Some("scene-7"));
        assert_eq!(item.entry.title, "Forest Scene");
    }
}
