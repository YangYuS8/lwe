//! Lightweight Wayland protocol capability probing for LWE.
//!
//! This module intentionally only inspects protocol availability. It does not
//! create layer surfaces, EGL contexts, or mpv render state.

use wayland_client::protocol::wl_registry::{self, WlRegistry};
use wayland_client::{Connection, Dispatch, QueueHandle};

const WL_COMPOSITOR: &str = "wl_compositor";
const WL_OUTPUT: &str = "wl_output";
const WLR_LAYER_SHELL: &str = "zwlr_layer_shell_v1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WaylandRuntimeSupport {
    Supported,
    Unsupported,
}

impl WaylandRuntimeSupport {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Supported => "supported",
            Self::Unsupported => "unsupported",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WaylandCapabilityReport {
    pub session_available: bool,
    pub display_connectable: bool,
    pub wl_compositor: bool,
    pub wl_output: bool,
    pub zwlr_layer_shell_v1: bool,
    pub output_count: usize,
    pub runtime_support: WaylandRuntimeSupport,
    pub unsupported_reason: Option<String>,
    pub connection_error: Option<String>,
}

impl WaylandCapabilityReport {
    pub fn unsupported_without_session() -> Self {
        Self::unsupported(
            false,
            false,
            false,
            false,
            false,
            0,
            "Wayland session is unavailable because WAYLAND_DISPLAY is not set",
            None,
        )
    }

    pub fn unsupported_connection_error(session_available: bool, error: String) -> Self {
        Self::unsupported(
            session_available,
            false,
            false,
            false,
            false,
            0,
            "Wayland display connection failed",
            Some(error),
        )
    }

    pub fn from_protocols(
        session_available: bool,
        display_connectable: bool,
        wl_compositor: bool,
        wl_output: bool,
        zwlr_layer_shell_v1: bool,
        output_count: usize,
    ) -> Self {
        let unsupported_reason = runtime_unsupported_reason(
            session_available,
            display_connectable,
            wl_compositor,
            wl_output,
            zwlr_layer_shell_v1,
            output_count,
        );

        Self {
            session_available,
            display_connectable,
            wl_compositor,
            wl_output,
            zwlr_layer_shell_v1,
            output_count,
            runtime_support: if unsupported_reason.is_some() {
                WaylandRuntimeSupport::Unsupported
            } else {
                WaylandRuntimeSupport::Supported
            },
            unsupported_reason,
            connection_error: None,
        }
    }

    fn unsupported(
        session_available: bool,
        display_connectable: bool,
        wl_compositor: bool,
        wl_output: bool,
        zwlr_layer_shell_v1: bool,
        output_count: usize,
        reason: &str,
        connection_error: Option<String>,
    ) -> Self {
        Self {
            session_available,
            display_connectable,
            wl_compositor,
            wl_output,
            zwlr_layer_shell_v1,
            output_count,
            runtime_support: WaylandRuntimeSupport::Unsupported,
            unsupported_reason: Some(reason.to_string()),
            connection_error,
        }
    }

    pub fn is_dynamic_wallpaper_supported(&self) -> bool {
        self.runtime_support == WaylandRuntimeSupport::Supported
    }
}

#[derive(Default)]
struct ProbeState {
    wl_compositor: bool,
    wl_output: bool,
    zwlr_layer_shell_v1: bool,
    output_count: usize,
}

impl Dispatch<WlRegistry, ()> for ProbeState {
    fn event(
        state: &mut Self,
        _registry: &WlRegistry,
        event: wl_registry::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global { interface, .. } = event {
            match interface.as_str() {
                WL_COMPOSITOR => state.wl_compositor = true,
                WL_OUTPUT => {
                    state.wl_output = true;
                    state.output_count += 1;
                }
                WLR_LAYER_SHELL => state.zwlr_layer_shell_v1 = true,
                _ => {}
            }
        }
    }
}

pub fn probe_wayland_capabilities() -> WaylandCapabilityReport {
    let session_available = std::env::var_os("WAYLAND_DISPLAY").is_some();
    if !session_available {
        return WaylandCapabilityReport::unsupported_without_session();
    }

    let connection = match Connection::connect_to_env() {
        Ok(connection) => connection,
        Err(error) => {
            return WaylandCapabilityReport::unsupported_connection_error(
                session_available,
                error.to_string(),
            );
        }
    };

    let mut event_queue = connection.new_event_queue::<ProbeState>();
    let qh = event_queue.handle();
    let _registry = connection.display().get_registry(&qh, ());
    let mut state = ProbeState::default();

    if let Err(error) = event_queue.roundtrip(&mut state) {
        return WaylandCapabilityReport::unsupported_connection_error(
            session_available,
            error.to_string(),
        );
    }

    WaylandCapabilityReport::from_protocols(
        session_available,
        true,
        state.wl_compositor,
        state.wl_output,
        state.zwlr_layer_shell_v1,
        state.output_count,
    )
}

fn runtime_unsupported_reason(
    session_available: bool,
    display_connectable: bool,
    wl_compositor: bool,
    wl_output: bool,
    zwlr_layer_shell_v1: bool,
    output_count: usize,
) -> Option<String> {
    if !session_available {
        return Some(
            "Wayland session is unavailable because WAYLAND_DISPLAY is not set".to_string(),
        );
    }
    if !display_connectable {
        return Some("Wayland display connection failed".to_string());
    }
    if !wl_compositor {
        return Some("Wayland compositor does not expose wl_compositor".to_string());
    }
    if !wl_output || output_count == 0 {
        return Some("Wayland compositor does not expose any wl_output globals".to_string());
    }
    if !zwlr_layer_shell_v1 {
        return Some(
            "Wayland compositor does not expose zwlr_layer_shell_v1, which LWE needs for dynamic wallpaper surfaces"
                .to_string(),
        );
    }

    None
}

#[cfg(test)]
mod tests {
    use super::{WaylandCapabilityReport, WaylandRuntimeSupport};

    #[test]
    fn capability_report_marks_layer_shell_compositor_supported() {
        let report = WaylandCapabilityReport::from_protocols(true, true, true, true, true, 2);

        assert!(report.is_dynamic_wallpaper_supported());
        assert_eq!(report.runtime_support, WaylandRuntimeSupport::Supported);
        assert!(report.unsupported_reason.is_none());
    }

    #[test]
    fn capability_report_requires_layer_shell() {
        let report = WaylandCapabilityReport::from_protocols(true, true, true, true, false, 1);

        assert!(!report.is_dynamic_wallpaper_supported());
        assert_eq!(report.runtime_support, WaylandRuntimeSupport::Unsupported);
        assert_eq!(
            report.unsupported_reason.as_deref(),
            Some(
                "Wayland compositor does not expose zwlr_layer_shell_v1, which LWE needs for dynamic wallpaper surfaces"
            )
        );
    }

    #[test]
    fn capability_report_requires_wayland_session() {
        let report = WaylandCapabilityReport::unsupported_without_session();

        assert!(!report.is_dynamic_wallpaper_supported());
        assert!(!report.session_available);
        assert_eq!(
            report.unsupported_reason.as_deref(),
            Some("Wayland session is unavailable because WAYLAND_DISPLAY is not set")
        );
    }
}
