use crate::policies::shared::support_policy::supports_first_release;
use lwe_library::{WorkshopCatalogEntry, WorkshopProjectType, WorkshopSyncState};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompatibilityLevel {
    FullySupported,
    PartiallySupported,
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompatibilityReason {
    ReadyForLibrary,
    RecognizedButRuntimeUnsupported,
    MissingProjectMetadata,
    MissingPrimaryAsset,
    UnsupportedWebItem,
    UnsupportedProjectType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompatibilityNextStep {
    None,
    OpenInSteam,
    ResyncWorkshopItem,
    WaitForFutureSupport,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompatibilityDecision {
    pub level: CompatibilityLevel,
    pub reason: CompatibilityReason,
    pub next_step: CompatibilityNextStep,
}

pub fn compatibility_decision(entry: &WorkshopCatalogEntry) -> CompatibilityDecision {
    let supported_first_release =
        entry.supported_first_release && supports_first_release(entry.project_type);

    match (
        supported_first_release,
        entry.sync_state,
        entry.project_type,
    ) {
        (true, WorkshopSyncState::Synced, _) => CompatibilityDecision {
            level: CompatibilityLevel::FullySupported,
            reason: CompatibilityReason::ReadyForLibrary,
            next_step: CompatibilityNextStep::None,
        },
        (false, WorkshopSyncState::Synced, WorkshopProjectType::Scene) => CompatibilityDecision {
            level: CompatibilityLevel::PartiallySupported,
            reason: CompatibilityReason::RecognizedButRuntimeUnsupported,
            next_step: CompatibilityNextStep::WaitForFutureSupport,
        },
        (_, WorkshopSyncState::MissingProjectFile, _) => CompatibilityDecision {
            level: CompatibilityLevel::Unsupported,
            reason: CompatibilityReason::MissingProjectMetadata,
            next_step: CompatibilityNextStep::ResyncWorkshopItem,
        },
        (_, WorkshopSyncState::MissingPrimaryAsset, WorkshopProjectType::Video) => {
            CompatibilityDecision {
                level: CompatibilityLevel::PartiallySupported,
                reason: CompatibilityReason::MissingPrimaryAsset,
                next_step: CompatibilityNextStep::ResyncWorkshopItem,
            }
        }
        (_, WorkshopSyncState::MissingPrimaryAsset, WorkshopProjectType::Scene) => {
            CompatibilityDecision {
                level: CompatibilityLevel::PartiallySupported,
                reason: CompatibilityReason::RecognizedButRuntimeUnsupported,
                next_step: CompatibilityNextStep::WaitForFutureSupport,
            }
        }
        (_, WorkshopSyncState::UnsupportedType, WorkshopProjectType::Web) => {
            CompatibilityDecision {
                level: CompatibilityLevel::Unsupported,
                reason: CompatibilityReason::UnsupportedWebItem,
                next_step: CompatibilityNextStep::WaitForFutureSupport,
            }
        }
        _ => CompatibilityDecision {
            level: CompatibilityLevel::Unsupported,
            reason: CompatibilityReason::UnsupportedProjectType,
            next_step: CompatibilityNextStep::WaitForFutureSupport,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn unsupported_web_entry() -> WorkshopCatalogEntry {
        WorkshopCatalogEntry {
            workshop_id: 55,
            title: "Web".to_string(),
            project_type: WorkshopProjectType::Web,
            project_dir: PathBuf::from("/tmp/55"),
            cover_path: None,
            sync_state: WorkshopSyncState::UnsupportedType,
            supported_first_release: false,
            library_item_id: None,
        }
    }

    #[test]
    fn shared_policy_compatibility_decision_returns_reason_codes() {
        let decision = compatibility_decision(&unsupported_web_entry());

        assert_eq!(decision.level, CompatibilityLevel::Unsupported);
        assert_eq!(decision.reason, CompatibilityReason::UnsupportedWebItem);
    }

    #[test]
    fn shared_policy_support_matrix_is_authoritative_for_synced_items() {
        let entry = WorkshopCatalogEntry {
            workshop_id: 77,
            title: "Unexpected Type".to_string(),
            project_type: WorkshopProjectType::Other,
            project_dir: PathBuf::from("/tmp/77"),
            cover_path: None,
            sync_state: WorkshopSyncState::Synced,
            supported_first_release: true,
            library_item_id: None,
        };

        let decision = compatibility_decision(&entry);

        assert_eq!(decision.level, CompatibilityLevel::Unsupported);
        assert_eq!(decision.reason, CompatibilityReason::UnsupportedProjectType);
    }

    #[test]
    fn synced_scene_is_recognized_without_claiming_runtime_support() {
        let entry = WorkshopCatalogEntry {
            workshop_id: 78,
            title: "Scene".to_string(),
            project_type: WorkshopProjectType::Scene,
            project_dir: PathBuf::from("/tmp/78"),
            cover_path: None,
            sync_state: WorkshopSyncState::Synced,
            supported_first_release: false,
            library_item_id: Some("scene-78".to_string()),
        };

        let decision = compatibility_decision(&entry);

        assert_eq!(decision.level, CompatibilityLevel::PartiallySupported);
        assert_eq!(
            decision.reason,
            CompatibilityReason::RecognizedButRuntimeUnsupported
        );
        assert_eq!(
            decision.next_step,
            CompatibilityNextStep::WaitForFutureSupport
        );
    }

    #[test]
    fn synced_video_is_ready_for_runtime_use() {
        let entry = WorkshopCatalogEntry {
            workshop_id: 79,
            title: "Video".to_string(),
            project_type: WorkshopProjectType::Video,
            project_dir: PathBuf::from("/tmp/79"),
            cover_path: None,
            sync_state: WorkshopSyncState::Synced,
            supported_first_release: true,
            library_item_id: Some("video-79".to_string()),
        };

        let decision = compatibility_decision(&entry);

        assert_eq!(decision.level, CompatibilityLevel::FullySupported);
        assert_eq!(decision.reason, CompatibilityReason::ReadyForLibrary);
        assert_eq!(decision.next_step, CompatibilityNextStep::None);
    }

    #[test]
    fn compatibility_decision_exposes_structured_reason_and_guidance() {
        let entry = WorkshopCatalogEntry {
            workshop_id: 9,
            title: "Broken Video".to_string(),
            project_type: WorkshopProjectType::Video,
            project_dir: std::path::PathBuf::from("/tmp/9"),
            cover_path: None,
            sync_state: WorkshopSyncState::MissingPrimaryAsset,
            supported_first_release: false,
            library_item_id: None,
        };

        let decision = compatibility_decision(&entry);

        assert_eq!(decision.level, CompatibilityLevel::PartiallySupported);
        assert_eq!(decision.reason, CompatibilityReason::MissingPrimaryAsset);
        assert_eq!(
            decision.next_step,
            CompatibilityNextStep::ResyncWorkshopItem
        );
    }

    #[test]
    fn missing_asset_scene_still_prefers_runtime_limitation_guidance() {
        let entry = WorkshopCatalogEntry {
            workshop_id: 10,
            title: "Broken Scene".to_string(),
            project_type: WorkshopProjectType::Scene,
            project_dir: std::path::PathBuf::from("/tmp/10"),
            cover_path: None,
            sync_state: WorkshopSyncState::MissingPrimaryAsset,
            supported_first_release: false,
            library_item_id: None,
        };

        let decision = compatibility_decision(&entry);

        assert_eq!(decision.level, CompatibilityLevel::PartiallySupported);
        assert_eq!(
            decision.reason,
            CompatibilityReason::RecognizedButRuntimeUnsupported
        );
        assert_eq!(
            decision.next_step,
            CompatibilityNextStep::WaitForFutureSupport
        );
    }

    #[test]
    fn compatibility_policy_reason_and_guidance_codes_are_serializable() {
        assert_eq!(
            serde_json::to_string(&CompatibilityReason::MissingPrimaryAsset).unwrap(),
            "\"missing_primary_asset\""
        );
        assert_eq!(
            serde_json::to_string(&CompatibilityNextStep::WaitForFutureSupport).unwrap(),
            "\"wait_for_future_support\""
        );
    }
}
