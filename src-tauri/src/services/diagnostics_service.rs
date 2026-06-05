use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

use lwe_library::{SteamLibrary, WorkshopSyncState};

use crate::models::DiagnosticsSnapshot;
use crate::policies::shared::compatibility_policy::CompatibilityLevel;
use crate::results::settings_persistence::{PersistedSettings, SettingsPersistenceLoad};
use crate::services::desktop_service::DesktopService;
use crate::services::library_service::LibraryService;
use crate::services::monitor_service::MonitorService;
use crate::services::settings_persistence_service::SettingsPersistenceService;
use crate::services::settings_service::steam_status_with_discovery;
use crate::services::wayland_capability_service::WaylandCapabilityService;
use crate::services::workshop_service::WorkshopService;

pub struct DiagnosticsService;

impl DiagnosticsService {
    pub fn load_snapshot() -> Result<DiagnosticsSnapshot, String> {
        let settings = Self::load_settings_redacted();
        let desktop = DesktopService::load_page();
        let workshop = WorkshopService::refresh_catalog();
        let library = LibraryService::load_projection();
        let wayland = WaylandCapabilityService::load_report();
        let steam = SteamLibrary::try_discover();
        let (_steam_required, steam_status) = steam_status_with_discovery(steam);

        let mut lines = Vec::new();
        lines.push("LWE diagnostics".to_string());
        lines.push(format!("generated_at_unix: {}", generated_at_unix()));
        lines.push(format!("lwe_version: {}", env!("CARGO_PKG_VERSION")));
        lines.push(
            "package_type: unknown/local build unless reported by package manager".to_string(),
        );
        lines.push(format!(
            "os: {} {}",
            std::env::consts::OS,
            std::env::consts::ARCH
        ));
        lines.push(format!("session_type: {}", env_hint("XDG_SESSION_TYPE")));
        lines.push(format!(
            "current_desktop: {}",
            env_hint("XDG_CURRENT_DESKTOP")
        ));
        lines.push(format!("desktop_session: {}", env_hint("DESKTOP_SESSION")));
        lines.push(format!("wayland_display: {}", env_hint("WAYLAND_DISPLAY")));
        lines.push("support_scope: video wallpapers on verified Wayland/niri are the current runtime focus; scene/web items are recognized for reporting, not runnable.".to_string());
        lines.push(String::new());

        lines.push("Settings".to_string());
        lines.push(format!("language: {}", settings.language));
        lines.push(format!("theme: {}", settings.theme));
        lines.push(format!("steam_web_api_key: {}", settings.steam_key_state));
        lines.push(format!(
            "launch_on_login_preference: {}",
            settings.launch_on_login
        ));
        lines.push(format!("workshop_query: {}", settings.workshop_query));
        lines.push(format!(
            "workshop_age_ratings: {}",
            settings.workshop_age_ratings
        ));
        lines.push(format!(
            "workshop_item_types: {}",
            settings.workshop_item_types
        ));
        lines.push(String::new());

        lines.push("Steam and Workshop".to_string());
        lines.push(format!("steam_discovery: {steam_status}"));
        match &workshop {
            Ok(refresh) => {
                lines.push(format!(
                    "workshop_snapshot_source: {}",
                    if refresh.served_from_snapshot {
                        "warm_snapshot"
                    } else {
                        "full_refresh"
                    }
                ));
                lines.push(format!(
                    "workshop_catalog_items: {}",
                    refresh.catalog_entries.len()
                ));
                lines.push(format!(
                    "workshop_synced_items: {}",
                    refresh.synced_entry_count()
                ));
                lines.push(format_counts(
                    "workshop_sync_states",
                    sync_state_counts(refresh),
                ));
            }
            Err(reason) => lines.push(format!("workshop_catalog_error: {reason}")),
        }
        lines.push(String::new());

        lines.push("Desktop runtime".to_string());
        lines.push(format!(
            "output_discovery_source: {}",
            MonitorService::output_discovery_source().as_str()
        ));
        lines.push(format!(
            "wayland_session_available: {}",
            wayland.session_available
        ));
        lines.push(format!(
            "wayland_display_connectable: {}",
            wayland.display_connectable
        ));
        lines.push(format!(
            "wayland_protocol_wl_compositor: {}",
            wayland.wl_compositor
        ));
        lines.push(format!("wayland_protocol_wl_output: {}", wayland.wl_output));
        lines.push(format!("wayland_output_count: {}", wayland.output_count));
        lines.push(format!(
            "wayland_protocol_zwlr_layer_shell_v1: {}",
            wayland.zwlr_layer_shell_v1
        ));
        lines.push(format!(
            "dynamic_wallpaper_runtime_support: {}",
            wayland.runtime_support.as_str()
        ));
        if let Some(reason) = &wayland.unsupported_reason {
            lines.push(format!(
                "dynamic_wallpaper_runtime_unsupported_reason: {reason}"
            ));
        }
        if let Some(error) = &wayland.connection_error {
            lines.push(format!("wayland_connection_error: {error}"));
        }
        match &desktop {
            Ok(page) => {
                lines.push(format!(
                    "monitor_discovery: {}",
                    if page.monitors_available {
                        "available"
                    } else {
                        "unavailable"
                    }
                ));
                lines.push(format!("monitor_count: {}", page.monitors.len()));
                if let Some(issue) = &page.monitor_discovery_issue {
                    lines.push(format!("monitor_discovery_issue: {issue}"));
                }
                lines.push(format!(
                    "runtime_backend_status: {}",
                    if page.runtime_issue.is_some() {
                        "error"
                    } else if page.running_outputs.is_empty() {
                        "idle"
                    } else {
                        "running"
                    }
                ));
                lines.push(format!(
                    "runtime_running_outputs: {}",
                    page.running_outputs.len()
                ));
                if let Some(issue) = &page.runtime_issue {
                    lines.push(format!("last_backend_initialization_error: {issue}"));
                }
                if !page.restore_issues.is_empty() {
                    lines.push(format!(
                        "restore_issues: {}",
                        page.restore_issues.join(" | ")
                    ));
                }
            }
            Err(reason) => lines.push(format!("desktop_page_error: {reason}")),
        }
        lines.push(String::new());

        lines.push("Library".to_string());
        match &library {
            Ok(projection) => {
                lines.push(format!(
                    "library_projection_source: {}",
                    if projection.served_from_snapshot {
                        "warm_snapshot"
                    } else {
                        "full_refresh"
                    }
                ));
                lines.push(format!("library_items: {}", projection.entries.len()));
                lines.push(format!(
                    "source_catalog_items: {}",
                    projection.source_catalog_count
                ));
                lines.push(format_counts(
                    "compatibility_levels",
                    compatibility_counts(projection),
                ));
                lines.push(format_counts(
                    "runtime_types",
                    runtime_type_counts(projection),
                ));
            }
            Err(reason) => lines.push(format!("library_error: {reason}")),
        }
        lines.push(String::new());
        lines.push("Release validation reminder: required CI checks must pass, and runtime-affecting release candidates require manual validation on a real supported desktop session.".to_string());

        Ok(DiagnosticsSnapshot {
            text: lines.join("\n"),
        })
    }

    fn load_settings_redacted() -> RedactedSettings {
        match SettingsPersistenceService::for_user_path().map(|service| service.load_settings()) {
            Ok(SettingsPersistenceLoad::Loaded(settings)) => RedactedSettings::from(settings),
            Ok(SettingsPersistenceLoad::Unavailable { reason }) | Err(reason) => RedactedSettings {
                language: format!("unavailable ({reason})"),
                theme: "unavailable".to_string(),
                launch_on_login: "unavailable".to_string(),
                steam_key_state: "unknown; value redacted".to_string(),
                workshop_query: "unavailable".to_string(),
                workshop_age_ratings: "unavailable".to_string(),
                workshop_item_types: "unavailable".to_string(),
            },
        }
    }
}

struct RedactedSettings {
    language: String,
    theme: String,
    launch_on_login: String,
    steam_key_state: String,
    workshop_query: String,
    workshop_age_ratings: String,
    workshop_item_types: String,
}

impl From<PersistedSettings> for RedactedSettings {
    fn from(settings: PersistedSettings) -> Self {
        Self {
            language: settings.language,
            theme: settings.theme,
            launch_on_login: settings.launch_on_login.to_string(),
            steam_key_state: if settings.steam_web_api_key.trim().is_empty() {
                "not set"
            } else {
                "set; value redacted"
            }
            .to_string(),
            workshop_query: settings.workshop_query,
            workshop_age_ratings: settings
                .workshop_age_ratings
                .iter()
                .map(|rating| format!("{rating:?}"))
                .collect::<Vec<_>>()
                .join(","),
            workshop_item_types: settings
                .workshop_item_types
                .iter()
                .map(|item_type| format!("{item_type:?}"))
                .collect::<Vec<_>>()
                .join(","),
        }
    }
}

fn generated_at_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}

fn env_hint(name: &str) -> String {
    std::env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "unknown".to_string())
}

fn format_counts(label: &str, counts: BTreeMap<String, usize>) -> String {
    let summary = counts
        .into_iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>()
        .join(", ");
    format!("{label}: {summary}")
}

fn sync_state_counts(
    refresh: &crate::results::workshop::WorkshopRefreshResult,
) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::from([
        ("synced".to_string(), 0),
        ("missing_project".to_string(), 0),
        ("missing_asset".to_string(), 0),
        ("unsupported_type".to_string(), 0),
    ]);

    for entry in &refresh.catalog_entries {
        let key = match entry.entry.sync_state {
            WorkshopSyncState::Synced => "synced",
            WorkshopSyncState::MissingProjectFile => "missing_project",
            WorkshopSyncState::MissingPrimaryAsset => "missing_asset",
            WorkshopSyncState::UnsupportedType => "unsupported_type",
        };
        *counts.entry(key.to_string()).or_default() += 1;
    }

    counts
}

fn compatibility_counts(
    projection: &crate::results::library::LibraryProjection,
) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::from([
        ("fully_supported".to_string(), 0),
        ("partially_supported".to_string(), 0),
        ("unsupported".to_string(), 0),
    ]);

    for entry in &projection.entries {
        let key = match entry.compatibility.level {
            CompatibilityLevel::FullySupported => "fully_supported",
            CompatibilityLevel::PartiallySupported => "partially_supported",
            CompatibilityLevel::Unsupported => "unsupported",
        };
        *counts.entry(key.to_string()).or_default() += 1;
    }

    counts
}

fn runtime_type_counts(
    projection: &crate::results::library::LibraryProjection,
) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();

    for entry in &projection.entries {
        let key = format!("{:?}", entry.entry.project_type).to_ascii_lowercase();
        *counts.entry(key).or_default() += 1;
    }

    counts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostics_redacts_steam_api_key_state() {
        let redacted = RedactedSettings::from(PersistedSettings {
            language: "en".to_string(),
            theme: "dark".to_string(),
            launch_on_login: true,
            steam_web_api_key: "secret-api-key".to_string(),
            workshop_query: "forest".to_string(),
            workshop_age_ratings: Vec::new(),
            workshop_item_types: Vec::new(),
        });

        let line = format!("steam_web_api_key: {}", redacted.steam_key_state);
        assert!(line.contains("redacted"));
        assert!(!line.contains("secret-api-key"));
    }

    #[test]
    fn diagnostics_format_counts_keeps_sorted_key_value_shape() {
        let line = format_counts(
            "library_projection_source",
            BTreeMap::from([
                ("full_refresh".to_string(), 1),
                ("warm_snapshot".to_string(), 2),
            ]),
        );

        assert_eq!(
            line,
            "library_projection_source: full_refresh=1, warm_snapshot=2"
        );
    }
}
