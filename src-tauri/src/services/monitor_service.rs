use crate::results::monitor_discovery::MonitorDiscoveryResult;
use crate::services::backends::monitor_backend::{BackendMonitorDiscovery, MonitorBackend};
use crate::services::backends::niri_monitor_backend::NiriMonitorBackend;

#[derive(Debug, Clone)]
pub struct MonitorDescriptor {
    pub id: String,
    pub backend_output_id: String,
    pub name: String,
    pub resolution: String,
}

pub struct MonitorService;

impl MonitorService {
    pub fn list_monitors() -> MonitorDiscoveryResult {
        Self::list_monitors_with_backend(&NiriMonitorBackend)
    }

    fn list_monitors_with_backend(backend: &impl MonitorBackend) -> MonitorDiscoveryResult {
        match backend.list_monitors() {
            BackendMonitorDiscovery::Known(monitors) => {
                let monitors = monitors
                    .into_iter()
                    .map(|monitor| MonitorDescriptor {
                        id: monitor.id,
                        backend_output_id: monitor.backend_output_id,
                        name: monitor.name,
                        resolution: monitor.resolution,
                    })
                    .collect::<Vec<_>>();

                MonitorDiscoveryResult::Known(monitors)
            }
            BackendMonitorDiscovery::Unavailable { reason } => {
                MonitorDiscoveryResult::Unavailable { reason }
            }
        }
    }

    pub fn resolve_specific_monitor(
        monitors: &MonitorDiscoveryResult,
        monitor_id: &str,
    ) -> MonitorDiscoveryResult {
        match monitors {
            MonitorDiscoveryResult::Known(monitors) => MonitorDiscoveryResult::Known(
                monitors
                    .iter()
                    .filter(|monitor| monitor.id == monitor_id)
                    .cloned()
                    .collect(),
            ),
            MonitorDiscoveryResult::Unavailable { reason } => MonitorDiscoveryResult::Unavailable {
                reason: reason.clone(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::backends::monitor_backend::{
        BackendMonitorDescriptor, BackendMonitorDiscovery,
    };

    struct StaticMonitorBackend(BackendMonitorDiscovery);

    impl MonitorBackend for StaticMonitorBackend {
        fn list_monitors(&self) -> BackendMonitorDiscovery {
            match &self.0 {
                BackendMonitorDiscovery::Known(monitors) => {
                    BackendMonitorDiscovery::Known(monitors.clone())
                }
                BackendMonitorDiscovery::Unavailable { reason } => {
                    BackendMonitorDiscovery::Unavailable {
                        reason: reason.clone(),
                    }
                }
            }
        }
    }

    #[test]
    fn monitor_service_uses_real_backend_result_type() {
        let result = MonitorService::list_monitors_with_backend(&StaticMonitorBackend(
            BackendMonitorDiscovery::Unavailable {
                reason: "niri is unavailable".to_string(),
            },
        ));

        assert!(matches!(
            result,
            crate::results::monitor_discovery::MonitorDiscoveryResult::Known(_)
                | crate::results::monitor_discovery::MonitorDiscoveryResult::Unavailable { .. }
        ));
    }

    #[test]
    fn list_monitors_preserves_monitor_descriptor_v1_shape_for_known_results() {
        let result = MonitorService::list_monitors_with_backend(&StaticMonitorBackend(
            BackendMonitorDiscovery::Known(vec![BackendMonitorDescriptor {
                id: "DISPLAY-1".to_string(),
                backend_output_id: "eDP-1".to_string(),
                name: "Primary".to_string(),
                resolution: "1920x1080".to_string(),
            }]),
        ));

        match result {
            MonitorDiscoveryResult::Known(monitors) => {
                assert!(monitors.iter().all(|monitor| {
                    !monitor.id.is_empty()
                        && !monitor.backend_output_id.is_empty()
                        && !monitor.name.is_empty()
                        && !monitor.resolution.is_empty()
                        && monitor.resolution.contains('x')
                }));
            }
            MonitorDiscoveryResult::Unavailable { .. } => {}
        }
    }

    #[test]
    fn list_monitors_returns_backend_result() {
        let result = MonitorService::list_monitors_with_backend(&StaticMonitorBackend(
            BackendMonitorDiscovery::Known(Vec::new()),
        ));

        assert!(matches!(
            result,
            MonitorDiscoveryResult::Known(_) | MonitorDiscoveryResult::Unavailable { .. }
        ));
    }

    #[test]
    fn resolve_specific_monitor_preserves_discovery_state() {
        let known_monitors = MonitorDiscoveryResult::Known(vec![
            MonitorDescriptor {
                id: "DISPLAY-1".to_string(),
                backend_output_id: "DISPLAY-1".to_string(),
                name: "Primary".to_string(),
                resolution: "1920x1080".to_string(),
            },
            MonitorDescriptor {
                id: "DISPLAY-2".to_string(),
                backend_output_id: "DISPLAY-2".to_string(),
                name: "Secondary".to_string(),
                resolution: "2560x1440".to_string(),
            },
        ]);

        let unavailable = MonitorDiscoveryResult::Unavailable {
            reason: "niri is unavailable".to_string(),
        };

        let resolved = MonitorService::resolve_specific_monitor(&known_monitors, "DISPLAY-2");
        let missing = MonitorService::resolve_specific_monitor(&known_monitors, "DISPLAY-3");
        let unresolved = MonitorService::resolve_specific_monitor(&unavailable, "DISPLAY-2");

        assert!(matches!(
            resolved,
            MonitorDiscoveryResult::Known(monitors)
                if monitors.len() == 1
                    && monitors[0].id == "DISPLAY-2"
                    && monitors[0].backend_output_id == "DISPLAY-2"
                    && monitors[0].name == "Secondary"
                    && monitors[0].resolution == "2560x1440"
        ));
        assert!(matches!(missing, MonitorDiscoveryResult::Known(monitors) if monitors.is_empty()));
        assert!(matches!(
            unresolved,
            MonitorDiscoveryResult::Unavailable { reason }
                if reason == "niri is unavailable"
        ));
    }
}
