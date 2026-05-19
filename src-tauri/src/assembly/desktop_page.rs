use crate::models::{
    DesktopMissingMonitorRestore, DesktopMonitorSummary, DesktopPageSnapshot, DesktopRestoreState,
    RuntimeStatus,
};
use crate::results::desktop::{DesktopPageResult, DesktopResolvedMonitorAssignment};

fn runtime_status_for_assignment(
    assignment: Option<&DesktopResolvedMonitorAssignment>,
    assignments_available: bool,
) -> RuntimeStatus {
    if !assignments_available {
        return RuntimeStatus::Unsupported;
    }

    match assignment {
        Some(DesktopResolvedMonitorAssignment::Restored { .. }) => RuntimeStatus::Running,
        Some(
            DesktopResolvedMonitorAssignment::MissingMonitor { .. }
            | DesktopResolvedMonitorAssignment::MissingItem { .. }
            | DesktopResolvedMonitorAssignment::Unavailable { .. },
        ) => RuntimeStatus::Error,
        None => RuntimeStatus::Idle,
    }
}

pub fn assemble_desktop_page(result: DesktopPageResult) -> DesktopPageSnapshot {
    let DesktopPageResult {
        monitors,
        assignments: _,
        resolved_assignments,
        library_item_assignments: _,
        restore_issues,
        monitors_available,
        monitor_discovery_issue,
        persistence_issue,
        assignments_available,
        stale,
    } = result;

    let known_monitor_ids = monitors
        .iter()
        .map(|monitor| monitor.id.clone())
        .collect::<std::collections::BTreeSet<_>>();

    DesktopPageSnapshot {
        monitors: monitors
            .into_iter()
            .map(|monitor| {
                let monitor_id = monitor.id;
                let assignment = resolved_assignments.get(&monitor_id);

                DesktopMonitorSummary {
                    current_wallpaper_title: match assignment {
                        Some(DesktopResolvedMonitorAssignment::Restored { item_title, .. }) => {
                            Some(item_title.clone())
                        }
                        _ => None,
                    },
                    current_cover_path: None,
                    current_item_id: assignment.map(|assignment| match assignment {
                        DesktopResolvedMonitorAssignment::Restored { item_id, .. }
                        | DesktopResolvedMonitorAssignment::MissingMonitor { item_id, .. }
                        | DesktopResolvedMonitorAssignment::MissingItem { item_id }
                        | DesktopResolvedMonitorAssignment::Unavailable { item_id, .. } => {
                            item_id.clone()
                        }
                    }),
                    display_name: monitor.name,
                    monitor_id: monitor_id.clone(),
                    resolution: monitor.resolution,
                    clear_supported: assignment.is_some() && assignments_available,
                    restore_state: assignment.map(|assignment| match assignment {
                        DesktopResolvedMonitorAssignment::Restored { .. } => {
                            DesktopRestoreState::Restored
                        }
                        DesktopResolvedMonitorAssignment::MissingMonitor { .. } => {
                            DesktopRestoreState::MissingMonitor
                        }
                        DesktopResolvedMonitorAssignment::MissingItem { .. } => {
                            DesktopRestoreState::MissingItem
                        }
                        DesktopResolvedMonitorAssignment::Unavailable { .. } => {
                            DesktopRestoreState::Unavailable
                        }
                    }),
                    restore_issue: assignment.and_then(|assignment| match assignment {
                        DesktopResolvedMonitorAssignment::Restored { .. } => None,
                        DesktopResolvedMonitorAssignment::MissingMonitor { .. } => Some(
                            "Saved assignment targets a monitor that is not currently available."
                                .to_string(),
                        ),
                        DesktopResolvedMonitorAssignment::MissingItem { item_id } => Some(format!(
                            "Saved assignment references missing Library item {item_id}."
                        )),
                        DesktopResolvedMonitorAssignment::Unavailable { reason, .. } => {
                            Some(reason.clone())
                        }
                    }),
                    runtime_status: runtime_status_for_assignment(
                        assignment,
                        assignments_available,
                    ),
                }
            })
            .collect(),
        missing_monitor_restores: resolved_assignments
            .iter()
            .filter(|(monitor_id, _)| !known_monitor_ids.contains(*monitor_id))
            .filter_map(|(monitor_id, assignment)| match assignment {
                DesktopResolvedMonitorAssignment::MissingMonitor {
                    item_id,
                    item_title,
                } => Some(DesktopMissingMonitorRestore {
                    monitor_id: monitor_id.clone(),
                    current_item_id: item_id.clone(),
                    current_wallpaper_title: item_title.clone(),
                    restore_state: DesktopRestoreState::MissingMonitor,
                    restore_issue: Some(
                        "Saved assignment targets a monitor that is not currently available."
                            .to_string(),
                    ),
                }),
                DesktopResolvedMonitorAssignment::Unavailable {
                    item_id,
                    item_title,
                    reason,
                } => Some(DesktopMissingMonitorRestore {
                    monitor_id: monitor_id.clone(),
                    current_item_id: item_id.clone(),
                    current_wallpaper_title: item_title.clone(),
                    restore_state: DesktopRestoreState::Unavailable,
                    restore_issue: Some(reason.clone()),
                }),
                _ => None,
            })
            .collect(),
        monitors_available,
        monitor_discovery_issue,
        persistence_issue,
        assignments_available,
        restore_issues,
        stale,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::models::RuntimeStatus;
    use crate::services::monitor_service::MonitorDescriptor;

    #[test]
    fn desktop_apply_flow_assembler_preserves_known_monitors_and_assignments() {
        let mut assignments = BTreeMap::new();
        assignments.insert("DISPLAY-1".to_string(), "Forest Scene".to_string());

        let snapshot = assemble_desktop_page(DesktopPageResult {
            monitors: vec![MonitorDescriptor {
                id: "DISPLAY-1".to_string(),
                backend_output_id: "DISPLAY-1".to_string(),
                name: "Primary".to_string(),
                resolution: "1920x1080".to_string(),
            }],
            assignments,
            resolved_assignments: BTreeMap::from([(
                "DISPLAY-1".to_string(),
                DesktopResolvedMonitorAssignment::Restored {
                    item_id: "Forest Scene".to_string(),
                    item_title: "Forest Scene".to_string(),
                },
            )]),
            library_item_assignments: BTreeMap::new(),
            restore_issues: Vec::new(),
            monitors_available: true,
            monitor_discovery_issue: None,
            persistence_issue: None,
            assignments_available: true,
            stale: false,
        });

        assert_eq!(snapshot.monitors.len(), 1);
        assert_eq!(snapshot.monitors[0].display_name, "Primary");
        assert_eq!(snapshot.monitors[0].resolution, "1920x1080");
        assert_eq!(
            snapshot.monitors[0].current_wallpaper_title.as_deref(),
            Some("Forest Scene")
        );
        assert!(snapshot.missing_monitor_restores.is_empty());
        assert!(!snapshot.stale);
        assert!(snapshot.assignments_available);
        assert!(snapshot.monitor_discovery_issue.is_none());
        assert!(snapshot.persistence_issue.is_none());
        assert_eq!(
            snapshot.monitors[0].current_item_id.as_deref(),
            Some("Forest Scene")
        );
        assert_eq!(
            snapshot.monitors[0].restore_state,
            Some(DesktopRestoreState::Restored)
        );
        assert!(snapshot.monitors[0].clear_supported);
        assert_eq!(snapshot.monitors[0].runtime_status, RuntimeStatus::Running);
    }

    #[test]
    fn desktop_apply_flow_assembler_keeps_runtime_status_unsupported_when_only_assignments_are_unavailable()
     {
        let snapshot = assemble_desktop_page(DesktopPageResult {
            monitors: vec![MonitorDescriptor {
                id: "DISPLAY-1".to_string(),
                backend_output_id: "DISPLAY-1".to_string(),
                name: "Primary".to_string(),
                resolution: "1920x1080".to_string(),
            }],
            assignments: BTreeMap::new(),
            resolved_assignments: BTreeMap::new(),
            library_item_assignments: BTreeMap::new(),
            restore_issues: Vec::new(),
            monitors_available: true,
            monitor_discovery_issue: None,
            persistence_issue: Some("Desktop persistence is not available yet".to_string()),
            assignments_available: false,
            stale: true,
        });

        assert_eq!(snapshot.monitors.len(), 1);
        assert_eq!(
            snapshot.monitors[0].runtime_status,
            RuntimeStatus::Unsupported
        );
        assert!(!snapshot.monitors[0].clear_supported);
    }

    #[test]
    fn desktop_apply_flow_assembler_marks_idle_monitors_when_real_runtime_path_is_available() {
        let snapshot = assemble_desktop_page(DesktopPageResult {
            monitors: vec![MonitorDescriptor {
                id: "DISPLAY-1".to_string(),
                backend_output_id: "DISPLAY-1".to_string(),
                name: "Primary".to_string(),
                resolution: "1920x1080".to_string(),
            }],
            assignments: BTreeMap::new(),
            resolved_assignments: BTreeMap::new(),
            library_item_assignments: BTreeMap::new(),
            restore_issues: Vec::new(),
            monitors_available: true,
            monitor_discovery_issue: None,
            persistence_issue: None,
            assignments_available: true,
            stale: false,
        });

        assert_eq!(snapshot.monitors[0].runtime_status, RuntimeStatus::Idle);
        assert!(!snapshot.monitors[0].clear_supported);
    }

    #[test]
    fn desktop_apply_flow_assembler_marks_degraded_assignments_as_runtime_errors() {
        let snapshot = assemble_desktop_page(DesktopPageResult {
            monitors: vec![MonitorDescriptor {
                id: "DISPLAY-1".to_string(),
                backend_output_id: "DISPLAY-1".to_string(),
                name: "Primary".to_string(),
                resolution: "1920x1080".to_string(),
            }],
            assignments: BTreeMap::from([("DISPLAY-1".to_string(), "missing-item".to_string())]),
            resolved_assignments: BTreeMap::from([(
                "DISPLAY-1".to_string(),
                DesktopResolvedMonitorAssignment::MissingItem {
                    item_id: "missing-item".to_string(),
                },
            )]),
            library_item_assignments: BTreeMap::new(),
            restore_issues: Vec::new(),
            monitors_available: true,
            monitor_discovery_issue: None,
            persistence_issue: None,
            assignments_available: true,
            stale: true,
        });

        assert_eq!(snapshot.monitors[0].runtime_status, RuntimeStatus::Error);
    }

    #[test]
    fn desktop_apply_flow_assembler_preserves_restore_state_and_page_issues() {
        let mut resolved_assignments = BTreeMap::new();
        resolved_assignments.insert(
            "DISPLAY-1".to_string(),
            crate::results::desktop::DesktopResolvedMonitorAssignment::MissingItem {
                item_id: "missing-item".to_string(),
            },
        );

        let snapshot = assemble_desktop_page(DesktopPageResult {
            monitors: vec![MonitorDescriptor {
                id: "DISPLAY-1".to_string(),
                backend_output_id: "DISPLAY-1".to_string(),
                name: "Primary".to_string(),
                resolution: "1920x1080".to_string(),
            }],
            assignments: BTreeMap::from([(
                "DISPLAY-1".to_string(),
                "missing-item".to_string(),
            )]),
            resolved_assignments,
            library_item_assignments: BTreeMap::new(),
            restore_issues: vec![
                "Saved assignment for missing monitor DISPLAY-3 still points to Forest Scene (scene-7)."
                    .to_string(),
            ],
            monitors_available: true,
            monitor_discovery_issue: None,
            persistence_issue: None,
            assignments_available: true,
            stale: false,
        });

        assert_eq!(
            snapshot.monitors[0].current_item_id.as_deref(),
            Some("missing-item")
        );
        assert!(snapshot.monitors[0].current_wallpaper_title.is_none());
        assert_eq!(
            snapshot.monitors[0].restore_state,
            Some(crate::models::DesktopRestoreState::MissingItem)
        );
        assert!(snapshot.missing_monitor_restores.is_empty());
        assert_eq!(snapshot.restore_issues.len(), 1);
    }

    #[test]
    fn desktop_apply_flow_assembler_exposes_missing_monitor_restores_structurally() {
        let snapshot = assemble_desktop_page(DesktopPageResult {
            monitors: vec![MonitorDescriptor {
                id: "DISPLAY-1".to_string(),
                backend_output_id: "DISPLAY-1".to_string(),
                name: "Primary".to_string(),
                resolution: "1920x1080".to_string(),
            }],
            assignments: BTreeMap::new(),
            resolved_assignments: BTreeMap::from([(
                "DISPLAY-9".to_string(),
                crate::results::desktop::DesktopResolvedMonitorAssignment::MissingMonitor {
                    item_id: "scene-7".to_string(),
                    item_title: Some("Forest Scene".to_string()),
                },
            )]),
            library_item_assignments: BTreeMap::new(),
            restore_issues: Vec::new(),
            monitors_available: true,
            monitor_discovery_issue: None,
            persistence_issue: None,
            assignments_available: true,
            stale: false,
        });

        assert_eq!(snapshot.missing_monitor_restores.len(), 1);
        assert_eq!(snapshot.missing_monitor_restores[0].monitor_id, "DISPLAY-9");
        assert_eq!(
            snapshot.missing_monitor_restores[0].restore_state,
            crate::models::DesktopRestoreState::MissingMonitor
        );
        assert_eq!(
            snapshot.missing_monitor_restores[0]
                .current_wallpaper_title
                .as_deref(),
            Some("Forest Scene")
        );
    }

    #[test]
    fn desktop_apply_flow_assembler_exposes_unavailable_monitor_restores_structurally() {
        let snapshot = assemble_desktop_page(DesktopPageResult {
            monitors: vec![],
            assignments: BTreeMap::from([("DISPLAY-9".to_string(), "scene-7".to_string())]),
            resolved_assignments: BTreeMap::from([(
                "DISPLAY-9".to_string(),
                crate::results::desktop::DesktopResolvedMonitorAssignment::Unavailable {
                    item_id: "scene-7".to_string(),
                    item_title: Some("Forest Scene".to_string()),
                    reason: "Monitor discovery unavailable for DISPLAY-9".to_string(),
                },
            )]),
            library_item_assignments: BTreeMap::new(),
            restore_issues: Vec::new(),
            monitors_available: false,
            monitor_discovery_issue: Some("niri unavailable".to_string()),
            persistence_issue: None,
            assignments_available: true,
            stale: true,
        });

        assert_eq!(snapshot.missing_monitor_restores.len(), 1);
        assert_eq!(snapshot.missing_monitor_restores[0].monitor_id, "DISPLAY-9");
        assert_eq!(
            snapshot.missing_monitor_restores[0].restore_state,
            crate::models::DesktopRestoreState::Unavailable
        );
        assert_eq!(
            snapshot.missing_monitor_restores[0].current_item_id,
            "scene-7"
        );
        assert_eq!(
            snapshot.missing_monitor_restores[0]
                .current_wallpaper_title
                .as_deref(),
            Some("Forest Scene")
        );
        assert_eq!(
            snapshot.missing_monitor_restores[0]
                .restore_issue
                .as_deref(),
            Some("Monitor discovery unavailable for DISPLAY-9")
        );
    }
}
