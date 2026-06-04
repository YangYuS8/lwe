use crate::results::app_shell::{ObservedCount, ShellSummary};
use crate::results::monitor_discovery::MonitorDiscoveryResult;
use crate::results::workshop::WorkshopRefreshResult;
use crate::services::library_service::LibraryService;
use crate::services::monitor_service::MonitorService;
use crate::services::workshop_service::WorkshopService;

pub struct AppShellService;

impl AppShellService {
    fn connected_monitor_count() -> ObservedCount {
        match MonitorService::list_monitors() {
            MonitorDiscoveryResult::Known(monitors) => ObservedCount::Known(monitors.len()),
            MonitorDiscoveryResult::Unavailable { .. } => ObservedCount::Unknown,
        }
    }

    fn unavailable_summary(steam_available: bool) -> ShellSummary {
        Self::unavailable_summary_with_monitor_count(
            steam_available,
            Self::connected_monitor_count(),
        )
    }

    fn unavailable_summary_with_monitor_count(
        steam_available: bool,
        connected_monitors: ObservedCount,
    ) -> ShellSummary {
        ShellSummary {
            steam_available,
            library_items: ObservedCount::Unknown,
            synced_workshop_items: ObservedCount::Unknown,
            connected_monitors,
        }
    }

    pub fn summary_from_refresh(
        steam_available: bool,
        refresh: WorkshopRefreshResult,
    ) -> ShellSummary {
        Self::summary_from_refresh_with_monitor_count(
            steam_available,
            refresh,
            Self::connected_monitor_count(),
        )
    }

    fn summary_from_refresh_with_monitor_count(
        steam_available: bool,
        refresh: WorkshopRefreshResult,
        connected_monitors: ObservedCount,
    ) -> ShellSummary {
        let synced_workshop_items = refresh.synced_entry_count();
        let library_projection = LibraryService::projection_from_refresh(refresh);

        ShellSummary {
            steam_available,
            library_items: ObservedCount::Known(library_projection.entries.len()),
            synced_workshop_items: ObservedCount::Known(synced_workshop_items),
            connected_monitors,
        }
    }

    pub fn load_summary() -> Result<ShellSummary, String> {
        let steam = match lwe_library::SteamLibrary::try_discover() {
            Some(steam) => steam,
            None => return Ok(Self::unavailable_summary(false)),
        };

        if !steam.has_wallpaper_engine() {
            return Ok(Self::unavailable_summary(true));
        }

        Ok(Self::summary_from_refresh(
            steam.has_wallpaper_engine(),
            WorkshopService::refresh_catalog()?,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assembly::app_shell::assemble_app_shell;
    use crate::policies::shared::compatibility_policy::{
        CompatibilityDecision, CompatibilityLevel, CompatibilityNextStep, CompatibilityReason,
    };
    use crate::results::workshop::{AssessedWorkshopCatalogEntry, WorkshopProjectMetadata};
    use lwe_library::{WorkshopCatalogEntry, WorkshopProjectType, WorkshopSyncState};

    fn assessed_entry(
        workshop_id: u64,
        title: &str,
        project_type: WorkshopProjectType,
        sync_state: WorkshopSyncState,
        library_item_id: Option<&str>,
        compatibility: CompatibilityDecision,
    ) -> AssessedWorkshopCatalogEntry {
        AssessedWorkshopCatalogEntry {
            entry: WorkshopCatalogEntry {
                workshop_id,
                title: title.to_string(),
                project_type,
                project_dir: std::path::PathBuf::from(format!("/tmp/{workshop_id}")),
                cover_path: None,
                sync_state,
                supported_first_release: compatibility.level == CompatibilityLevel::FullySupported,
                library_item_id: library_item_id.map(str::to_string),
            },
            compatibility,
            project_metadata: WorkshopProjectMetadata::default(),
        }
    }

    #[test]
    fn unavailable_steam_leaves_library_and_workshop_counts_unknown() {
        let snapshot = assemble_app_shell(AppShellService::unavailable_summary_with_monitor_count(
            false,
            ObservedCount::Unknown,
        ));

        assert!(!snapshot.steam_available);
        assert_eq!(snapshot.library_count, None);
        assert_eq!(snapshot.workshop_synced_count, None);
        assert_eq!(snapshot.monitor_count, None);
    }

    #[test]
    fn steam_without_wallpaper_engine_content_keeps_library_and_workshop_counts_unknown() {
        let snapshot = assemble_app_shell(AppShellService::unavailable_summary_with_monitor_count(
            true,
            ObservedCount::Known(2),
        ));

        assert!(snapshot.steam_available);
        assert_eq!(snapshot.library_count, None);
        assert_eq!(snapshot.workshop_synced_count, None);
        assert_eq!(snapshot.monitor_count, Some(2));
    }

    #[test]
    fn shell_counts_come_from_one_refresh_result() {
        let snapshot =
            assemble_app_shell(AppShellService::summary_from_refresh_with_monitor_count(
                true,
                WorkshopRefreshResult {
                    catalog_entries: vec![
                        assessed_entry(
                            1,
                            "Synced Video",
                            WorkshopProjectType::Video,
                            WorkshopSyncState::Synced,
                            Some("video-1"),
                            CompatibilityDecision {
                                level: CompatibilityLevel::FullySupported,
                                reason: CompatibilityReason::ReadyForLibrary,
                                next_step: CompatibilityNextStep::None,
                            },
                        ),
                        assessed_entry(
                            2,
                            "Unsupported App",
                            WorkshopProjectType::Other,
                            WorkshopSyncState::UnsupportedType,
                            None,
                            CompatibilityDecision {
                                level: CompatibilityLevel::Unsupported,
                                reason: CompatibilityReason::UnsupportedProjectType,
                                next_step: CompatibilityNextStep::WaitForFutureSupport,
                            },
                        ),
                    ],
                    library_refresh_required: true,
                    served_from_snapshot: false,
                },
                ObservedCount::Known(1),
            ));

        assert_eq!(snapshot.library_count, Some(1));
        assert_eq!(snapshot.workshop_synced_count, Some(1));
    }

    #[test]
    fn shell_summary_reflects_injected_monitor_discovery_result() {
        let summary = AppShellService::summary_from_refresh_with_monitor_count(
            true,
            WorkshopRefreshResult {
                catalog_entries: Vec::new(),
                library_refresh_required: false,
                served_from_snapshot: true,
            },
            ObservedCount::Known(3),
        );

        assert_eq!(summary.connected_monitors, ObservedCount::Known(3));
    }

    #[test]
    fn shared_policy_shell_snapshot_comes_from_result_summary() {
        let summary = AppShellService::summary_from_refresh_with_monitor_count(
            true,
            WorkshopRefreshResult {
                catalog_entries: vec![assessed_entry(
                    1,
                    "Synced Video",
                    WorkshopProjectType::Video,
                    WorkshopSyncState::Synced,
                    Some("video-1"),
                    CompatibilityDecision {
                        level: CompatibilityLevel::FullySupported,
                        reason: CompatibilityReason::ReadyForLibrary,
                        next_step: CompatibilityNextStep::None,
                    },
                )],
                library_refresh_required: true,
                served_from_snapshot: false,
            },
            ObservedCount::Unknown,
        );

        assert!(matches!(summary.library_items, ObservedCount::Known(1)));
        assert!(matches!(
            summary.synced_workshop_items,
            ObservedCount::Known(1)
        ));
    }
}
