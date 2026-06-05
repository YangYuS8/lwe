use lwe_engine::{WaylandCapabilityReport, probe_wayland_capabilities};

pub struct WaylandCapabilityService;

impl WaylandCapabilityService {
    pub fn load_report() -> WaylandCapabilityReport {
        probe_wayland_capabilities()
    }

    pub fn ensure_dynamic_wallpaper_supported() -> Result<WaylandCapabilityReport, String> {
        let report = Self::load_report();
        if report.is_dynamic_wallpaper_supported() {
            Ok(report)
        } else {
            Err(Self::unsupported_reason(&report))
        }
    }

    pub fn unsupported_reason(report: &WaylandCapabilityReport) -> String {
        report
            .unsupported_reason
            .clone()
            .or_else(|| report.connection_error.clone())
            .unwrap_or_else(|| "Wayland dynamic wallpaper runtime is unavailable".to_string())
    }
}

#[cfg(test)]
mod tests {
    use lwe_engine::WaylandCapabilityReport;

    use super::WaylandCapabilityService;

    #[test]
    fn wayland_capability_service_formats_missing_protocol_reason() {
        let report = WaylandCapabilityReport::from_protocols(true, true, true, true, false, 1);

        assert_eq!(
            WaylandCapabilityService::unsupported_reason(&report),
            "Wayland compositor does not expose zwlr_layer_shell_v1, which LWE needs for dynamic wallpaper surfaces"
        );
    }
}
