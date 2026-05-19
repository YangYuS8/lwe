use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ItemType {
    Video,
    Scene,
    Web,
    Application,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkshopAgeRating {
    G,
    #[serde(rename = "pg_13")]
    Pg13,
    #[serde(rename = "r_18")]
    R18,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WorkshopOnlineItemType {
    Video,
    Scene,
    Web,
    Application,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkshopOnlineSearchInput {
    pub query: String,
    pub age_ratings: Vec<WorkshopAgeRating>,
    pub item_types: Vec<WorkshopOnlineItemType>,
    pub page: u32,
    pub page_size: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkshopOnlineItem {
    pub id: String,
    pub title: String,
    pub preview_url: Option<String>,
    pub tags: Vec<String>,
    pub item_type: WorkshopOnlineItemType,
    pub age_rating: WorkshopAgeRating,
    pub age_rating_reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkshopOnlineSearchResult {
    pub query: String,
    pub page: u32,
    pub page_size: u32,
    pub has_more: bool,
    pub total_approx: Option<u32>,
    pub items: Vec<WorkshopOnlineItem>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkshopSyncStatus {
    Synced,
    MissingProject,
    MissingAsset,
    UnsupportedType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompatibilityBadge {
    FullySupported,
    PartiallySupported,
    Unsupported,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompatibilitySummaryModel {
    pub badge: CompatibilityBadge,
    pub reason_code: String,
    pub summary_copy: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompatibilityExplanationModel {
    pub badge: CompatibilityBadge,
    pub reason_code: String,
    pub summary_copy: String,
    pub headline: String,
    pub detail: String,
    pub next_step: crate::results::compatibility::CompatibilityNextStep,
    pub next_step_copy: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LibrarySource {
    Local,
    Workshop,
    Core,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeStatus {
    Running,
    Idle,
    Unsupported,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DesktopRestoreState {
    Restored,
    MissingMonitor,
    MissingItem,
    Unavailable,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopMissingMonitorRestore {
    pub monitor_id: String,
    pub current_item_id: String,
    pub current_wallpaper_title: Option<String>,
    pub restore_state: DesktopRestoreState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub restore_issue: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppShellSnapshot {
    pub app_name: String,
    pub code_name: String,
    pub steam_available: bool,
    pub library_count: Option<usize>,
    pub workshop_synced_count: Option<usize>,
    pub monitor_count: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkshopItemSummary {
    pub id: String,
    pub title: String,
    pub item_type: ItemType,
    pub cover_path: Option<String>,
    pub sync_status: WorkshopSyncStatus,
    pub compatibility: CompatibilitySummaryModel,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkshopPageSnapshot {
    pub items: Vec<WorkshopItemSummary>,
    pub selected_item_id: Option<String>,
    pub stale: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkshopItemDetail {
    pub id: String,
    pub title: String,
    pub item_type: ItemType,
    pub cover_path: Option<String>,
    pub sync_status: WorkshopSyncStatus,
    pub compatibility: CompatibilityExplanationModel,
    pub tags: Vec<String>,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryItemSummary {
    pub id: String,
    pub title: String,
    pub item_type: ItemType,
    pub cover_path: Option<String>,
    pub age_rating: WorkshopAgeRating,
    pub source: LibrarySource,
    pub compatibility: CompatibilitySummaryModel,
    pub favorite: bool,
    pub assigned_monitor_labels: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryPageSnapshot {
    pub items: Vec<LibraryItemSummary>,
    pub selected_item_id: Option<String>,
    pub monitors_available: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub monitor_discovery_issue: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub desktop_assignment_issue: Option<String>,
    pub desktop_assignments_available: bool,
    pub stale: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryItemDetail {
    pub id: String,
    pub title: String,
    pub item_type: ItemType,
    pub cover_path: Option<String>,
    pub source: LibrarySource,
    pub compatibility: CompatibilityExplanationModel,
    pub monitors_available: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub monitor_discovery_issue: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub desktop_assignment_issue: Option<String>,
    pub desktop_assignments_available: bool,
    pub assigned_monitor_labels: Vec<String>,
    pub description: Option<String>,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopMonitorSummary {
    pub monitor_id: String,
    pub display_name: String,
    pub resolution: String,
    pub current_wallpaper_title: Option<String>,
    pub current_cover_path: Option<String>,
    pub current_item_id: Option<String>,
    pub clear_supported: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub restore_state: Option<DesktopRestoreState>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub restore_issue: Option<String>,
    pub runtime_status: RuntimeStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopPageSnapshot {
    pub monitors: Vec<DesktopMonitorSummary>,
    pub missing_monitor_restores: Vec<DesktopMissingMonitorRestore>,
    pub monitors_available: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub monitor_discovery_issue: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub persistence_issue: Option<String>,
    pub assignments_available: bool,
    pub restore_issues: Vec<String>,
    pub stale: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsUpdateInput {
    pub language: Option<String>,
    pub theme: Option<String>,
    pub launch_on_login: Option<bool>,
    pub steam_web_api_key: Option<String>,
    pub workshop_query: Option<String>,
    pub workshop_age_ratings: Option<Vec<WorkshopAgeRating>>,
    pub workshop_item_types: Option<Vec<WorkshopOnlineItemType>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsPageSnapshot {
    pub language: String,
    pub theme: String,
    pub launch_on_login: bool,
    pub launch_on_login_available: bool,
    pub steam_web_api_key: String,
    pub workshop_query: String,
    pub workshop_age_ratings: Vec<WorkshopAgeRating>,
    pub workshop_item_types: Vec<WorkshopOnlineItemType>,
    pub steam_required: bool,
    pub steam_status_message: String,
    pub stale: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action_outcome::AppShellPatch;
    use crate::results::compatibility::CompatibilityNextStep;

    fn summary_compatibility() -> CompatibilitySummaryModel {
        CompatibilitySummaryModel {
            badge: CompatibilityBadge::FullySupported,
            reason_code: "ready_for_library".to_string(),
            summary_copy: "Ready to use".to_string(),
        }
    }

    #[test]
    fn workshop_item_summary_uses_cover_or_placeholder_shape() {
        let item = WorkshopItemSummary {
            id: "42".to_string(),
            title: "Forest Scene".to_string(),
            item_type: ItemType::Scene,
            cover_path: None,
            sync_status: WorkshopSyncStatus::Synced,
            compatibility: summary_compatibility(),
        };

        assert_eq!(item.id, "42");
        assert_eq!(item.item_type, ItemType::Scene);
        assert!(item.cover_path.is_none());
    }

    #[test]
    fn models_serialize_using_safe_ids_and_constrained_labels() {
        let item = WorkshopItemSummary {
            id: "9007199254740993".to_string(),
            title: "Forest Scene".to_string(),
            item_type: ItemType::Scene,
            cover_path: None,
            sync_status: WorkshopSyncStatus::Synced,
            compatibility: summary_compatibility(),
        };

        let value = serde_json::to_value(&item).unwrap();

        assert_eq!(value["id"], "9007199254740993");
        assert_eq!(value["itemType"], "scene");
        assert_eq!(value["syncStatus"], "synced");
        assert_eq!(value["compatibility"]["badge"], "fully_supported");
        assert_eq!(value["compatibility"]["reasonCode"], "ready_for_library");
    }

    #[test]
    fn compatibility_models_serialize_explanation_payload() {
        let item = WorkshopItemDetail {
            id: "42".to_string(),
            title: "Forest Scene".to_string(),
            item_type: ItemType::Scene,
            cover_path: None,
            sync_status: WorkshopSyncStatus::Synced,
            compatibility: CompatibilityExplanationModel {
                badge: CompatibilityBadge::FullySupported,
                reason_code: "ready_for_library".to_string(),
                summary_copy: "Ready to use".to_string(),
                headline: "Ready to use".to_string(),
                detail:
                    "This item is synchronized locally and available for Library and desktop use."
                        .to_string(),
                next_step: CompatibilityNextStep::None,
                next_step_copy: None,
            },
            tags: Vec::new(),
            description: None,
        };

        let value = serde_json::to_value(&item).unwrap();

        assert_eq!(value["compatibility"]["headline"], "Ready to use");
        assert_eq!(value["compatibility"]["summaryCopy"], "Ready to use");
        assert_eq!(value["compatibility"]["nextStep"], "none");
        assert!(value["compatibility"]["nextStepCopy"].is_null());
    }

    #[test]
    fn app_shell_patch_omits_absent_counts() {
        let patch = AppShellPatch {
            workshop_synced_count: Some(2),
            library_count: None,
            monitor_count: None,
        };

        let value = serde_json::to_value(&patch).unwrap();

        assert_eq!(value["workshopSyncedCount"], 2);
        assert!(value.get("libraryCount").is_none());
        assert!(value.get("monitorCount").is_none());
    }

    #[test]
    fn app_shell_snapshot_allows_unknown_counts() {
        let snapshot = AppShellSnapshot {
            app_name: "LWE".to_string(),
            code_name: "lwe".to_string(),
            steam_available: false,
            library_count: None,
            workshop_synced_count: None,
            monitor_count: None,
        };

        let value = serde_json::to_value(&snapshot).unwrap();

        assert!(value["libraryCount"].is_null());
        assert!(value["workshopSyncedCount"].is_null());
        assert!(value["monitorCount"].is_null());
    }

    #[test]
    fn settings_models_serialize_editable_mvp_fields() {
        let update = SettingsUpdateInput {
            language: Some("en".to_string()),
            theme: Some("system".to_string()),
            launch_on_login: Some(true),
            steam_web_api_key: Some("test-api-key".to_string()),
            workshop_query: Some("forest".to_string()),
            workshop_age_ratings: Some(vec![WorkshopAgeRating::G, WorkshopAgeRating::Pg13]),
            workshop_item_types: Some(vec![
                WorkshopOnlineItemType::Video,
                WorkshopOnlineItemType::Scene,
            ]),
        };

        let snapshot = SettingsPageSnapshot {
            language: "en".to_string(),
            theme: "system".to_string(),
            launch_on_login: true,
            launch_on_login_available: true,
            steam_web_api_key: "abcd1234".to_string(),
            workshop_query: "forest".to_string(),
            workshop_age_ratings: vec![WorkshopAgeRating::G, WorkshopAgeRating::Pg13],
            workshop_item_types: vec![WorkshopOnlineItemType::Video, WorkshopOnlineItemType::Scene],
            steam_required: true,
            steam_status_message: "Steam is required for Workshop features".to_string(),
            stale: false,
        };

        let update_value = serde_json::to_value(&update).unwrap();
        let snapshot_value = serde_json::to_value(&snapshot).unwrap();

        assert_eq!(update_value["language"], "en");
        assert_eq!(update_value["theme"], "system");
        assert_eq!(update_value["launchOnLogin"], true);
        assert_eq!(update_value["steamWebApiKey"], "test-api-key");
        assert_eq!(update_value["workshopQuery"], "forest");
        assert_eq!(update_value["workshopAgeRatings"][0], "g");
        assert_eq!(update_value["workshopAgeRatings"][1], "pg_13");
        assert_eq!(update_value["workshopItemTypes"][0], "video");
        assert_eq!(update_value["workshopItemTypes"][1], "scene");
        assert_eq!(snapshot_value["language"], "en");
        assert_eq!(snapshot_value["theme"], "system");
        assert_eq!(snapshot_value["launchOnLogin"], true);
        assert_eq!(snapshot_value["launchOnLoginAvailable"], true);
        assert_eq!(snapshot_value["steamWebApiKey"], "abcd1234");
        assert_eq!(snapshot_value["workshopQuery"], "forest");
        assert_eq!(snapshot_value["workshopAgeRatings"][0], "g");
        assert_eq!(snapshot_value["workshopAgeRatings"][1], "pg_13");
        assert_eq!(snapshot_value["workshopItemTypes"][0], "video");
        assert_eq!(snapshot_value["workshopItemTypes"][1], "scene");
        assert_eq!(snapshot_value["steamRequired"], true);
        assert_eq!(
            snapshot_value["steamStatusMessage"],
            "Steam is required for Workshop features"
        );
        assert_eq!(snapshot_value["stale"], false);
    }

    #[test]
    fn online_workshop_models_serialize_requested_filters_and_reason() {
        let input = WorkshopOnlineSearchInput {
            query: "neon".to_string(),
            age_ratings: vec![WorkshopAgeRating::Pg13],
            item_types: vec![WorkshopOnlineItemType::Application],
            page: 1,
            page_size: 24,
        };
        let result = WorkshopOnlineSearchResult {
            query: "neon".to_string(),
            page: 1,
            page_size: 24,
            has_more: false,
            total_approx: Some(1),
            items: vec![WorkshopOnlineItem {
                id: "123".to_string(),
                title: "Neon App".to_string(),
                preview_url: Some("https://example.com/cover.jpg".to_string()),
                tags: vec!["Application".to_string()],
                item_type: WorkshopOnlineItemType::Application,
                age_rating: WorkshopAgeRating::Pg13,
                age_rating_reason: "Contains mature content markers: suggestive".to_string(),
            }],
        };

        let input_value = serde_json::to_value(&input).unwrap();
        let result_value = serde_json::to_value(&result).unwrap();

        assert_eq!(input_value["query"], "neon");
        assert_eq!(input_value["ageRatings"][0], "pg_13");
        assert_eq!(input_value["itemTypes"][0], "application");
        assert_eq!(result_value["items"][0]["itemType"], "application");
        assert_eq!(result_value["items"][0]["ageRating"], "pg_13");
        assert_eq!(
            result_value["items"][0]["ageRatingReason"],
            "Contains mature content markers: suggestive"
        );
    }

    #[test]
    fn desktop_apply_flow_models_serialize_restore_state_and_assignment_labels() {
        let snapshot = DesktopPageSnapshot {
            monitors: vec![DesktopMonitorSummary {
                monitor_id: "DISPLAY-1".to_string(),
                display_name: "Primary".to_string(),
                resolution: "1920x1080".to_string(),
                current_wallpaper_title: None,
                current_cover_path: None,
                current_item_id: Some("scene-7".to_string()),
                clear_supported: true,
                restore_state: Some(DesktopRestoreState::Restored),
                restore_issue: None,
                runtime_status: RuntimeStatus::Unsupported,
            }],
            missing_monitor_restores: vec![DesktopMissingMonitorRestore {
                monitor_id: "DISPLAY-2".to_string(),
                current_item_id: "scene-8".to_string(),
                current_wallpaper_title: Some("Ocean Scene".to_string()),
                restore_state: DesktopRestoreState::MissingMonitor,
                restore_issue: Some("Saved assignment targets a monitor that is not currently available.".to_string()),
            }],
            monitors_available: true,
            monitor_discovery_issue: None,
            persistence_issue: None,
            assignments_available: true,
            restore_issues: vec![
                "Saved assignment for missing monitor DISPLAY-2 still points to Forest Scene (scene-7)."
                    .to_string(),
            ],
            stale: false,
        };

        let library_item = LibraryItemSummary {
            id: "scene-7".to_string(),
            title: "Forest Scene".to_string(),
            item_type: ItemType::Scene,
            cover_path: None,
            age_rating: WorkshopAgeRating::G,
            source: LibrarySource::Workshop,
            compatibility: summary_compatibility(),
            favorite: false,
            assigned_monitor_labels: vec!["Primary".to_string()],
        };

        let desktop_value = serde_json::to_value(&snapshot).unwrap();
        let library_value = serde_json::to_value(&library_item).unwrap();

        assert_eq!(desktop_value["monitors"][0]["restoreState"], "restored");
        assert_eq!(
            desktop_value["missingMonitorRestores"][0]["restoreState"],
            "missing_monitor"
        );
        assert_eq!(
            desktop_value["restoreIssues"][0],
            "Saved assignment for missing monitor DISPLAY-2 still points to Forest Scene (scene-7)."
        );
        assert_eq!(library_value["assignedMonitorLabels"][0], "Primary");
    }
}
