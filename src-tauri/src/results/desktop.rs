use std::collections::{BTreeMap, BTreeSet};

use crate::services::monitor_service::MonitorDescriptor;

#[path = "desktop_apply.rs"]
mod desktop_apply_result;

pub use desktop_apply_result::DesktopApplyResult;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DesktopResolvedMonitorAssignment {
    Restored {
        item_id: String,
        item_title: String,
    },
    MissingMonitor {
        item_id: String,
        item_title: Option<String>,
    },
    MissingItem {
        item_id: String,
    },
    Unavailable {
        item_id: String,
        item_title: Option<String>,
        reason: String,
    },
}

#[derive(Debug, Clone)]
pub struct DesktopPageResult {
    pub monitors: Vec<MonitorDescriptor>,
    pub assignments: BTreeMap<String, String>,
    pub resolved_assignments: BTreeMap<String, DesktopResolvedMonitorAssignment>,
    pub library_item_assignments: BTreeMap<String, Vec<String>>,
    pub running_outputs: BTreeSet<String>,
    pub restore_issues: Vec<String>,
    pub runtime_issue: Option<String>,
    pub monitors_available: bool,
    pub monitor_discovery_issue: Option<String>,
    pub persistence_issue: Option<String>,
    pub assignments_available: bool,
    pub stale: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_page_result_tracks_unavailable_assignments_separately_from_empty_monitors() {
        let result = DesktopPageResult {
            monitors: Vec::new(),
            assignments: BTreeMap::new(),
            resolved_assignments: BTreeMap::from([(
                "DISPLAY-2".to_string(),
                DesktopResolvedMonitorAssignment::MissingMonitor {
                    item_id: "scene-7".to_string(),
                    item_title: Some("Forest Scene".to_string()),
                },
            )]),
            library_item_assignments: BTreeMap::new(),
            running_outputs: BTreeSet::new(),
            restore_issues: vec![
                "Saved assignment for missing monitor DISPLAY-2 still points to scene-7."
                    .to_string(),
            ],
            runtime_issue: None,
            monitors_available: false,
            monitor_discovery_issue: None,
            persistence_issue: Some("Desktop persistence is not available yet".to_string()),
            assignments_available: false,
            stale: true,
        };

        assert!(result.monitors.is_empty());
        assert!(!result.monitors_available);
        assert!(!result.assignments_available);
        assert!(result.persistence_issue.is_some());
        assert!(result.monitor_discovery_issue.is_none());
        assert!(matches!(
            result.resolved_assignments.get("DISPLAY-2"),
            Some(DesktopResolvedMonitorAssignment::MissingMonitor { .. })
        ));
        assert_eq!(result.restore_issues.len(), 1);
    }
}
