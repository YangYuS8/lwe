#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DesktopApplyResult {
    Applied {
        monitor_id: String,
        item_id: String,
    },
    AppliedWithBackend {
        monitor_id: String,
        item_id: String,
        backend: String,
    },
    Cleared {
        monitor_id: String,
    },
    ClearedWithBackendWarning {
        monitor_id: String,
        warning: String,
    },
    MonitorNotFound {
        monitor_id: String,
    },
    MonitorDiscoveryUnavailable {
        reason: String,
    },
    BackendUnavailable {
        reason: String,
    },
    UnsupportedItem {
        item_id: String,
        reason: String,
    },
    PersistenceUnavailable {
        reason: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_apply_result_distinguishes_unavailable_from_known_failures() {
        let applied = DesktopApplyResult::Applied {
            monitor_id: "DISPLAY-1".to_string(),
            item_id: "scene-7".to_string(),
        };
        let applied_with_backend = DesktopApplyResult::AppliedWithBackend {
            monitor_id: "DISPLAY-1".to_string(),
            item_id: "scene-7".to_string(),
            backend: "lwe_engine_wayland".to_string(),
        };
        let discovery_unavailable = DesktopApplyResult::MonitorDiscoveryUnavailable {
            reason: "Desktop persistence is not available yet".to_string(),
        };
        let backend_unavailable = DesktopApplyResult::BackendUnavailable {
            reason: "The current desktop apply backend is unavailable".to_string(),
        };
        let unsupported_item = DesktopApplyResult::UnsupportedItem {
            item_id: "scene-7".to_string(),
            reason: "Scene wallpapers are recognized but not runnable yet".to_string(),
        };
        let persistence_unavailable = DesktopApplyResult::PersistenceUnavailable {
            reason: "Desktop persistence is not available yet".to_string(),
        };
        let missing = DesktopApplyResult::MonitorNotFound {
            monitor_id: "DISPLAY-2".to_string(),
        };
        let cleared_with_warning = DesktopApplyResult::ClearedWithBackendWarning {
            monitor_id: "DISPLAY-1".to_string(),
            warning: "Runtime was already stopped".to_string(),
        };

        assert!(matches!(applied, DesktopApplyResult::Applied { .. }));
        assert!(matches!(
            applied_with_backend,
            DesktopApplyResult::AppliedWithBackend { .. }
        ));
        assert!(matches!(
            discovery_unavailable,
            DesktopApplyResult::MonitorDiscoveryUnavailable { .. }
        ));
        assert!(matches!(
            backend_unavailable,
            DesktopApplyResult::BackendUnavailable { .. }
        ));
        assert!(matches!(
            unsupported_item,
            DesktopApplyResult::UnsupportedItem { .. }
        ));
        assert!(matches!(
            persistence_unavailable,
            DesktopApplyResult::PersistenceUnavailable { .. }
        ));
        assert!(matches!(
            missing,
            DesktopApplyResult::MonitorNotFound { .. }
        ));
        assert!(matches!(
            cleared_with_warning,
            DesktopApplyResult::ClearedWithBackendWarning { .. }
        ));
    }
}
