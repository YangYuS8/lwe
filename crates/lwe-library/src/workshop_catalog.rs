use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WorkshopProjectType {
    Video,
    Scene,
    Web,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WorkshopSyncState {
    Synced,
    MissingProjectFile,
    MissingPrimaryAsset,
    UnsupportedType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkshopCatalogEntry {
    pub workshop_id: u64,
    pub title: String,
    pub project_type: WorkshopProjectType,
    pub project_dir: PathBuf,
    pub cover_path: Option<PathBuf>,
    pub sync_state: WorkshopSyncState,
    pub supported_first_release: bool,
    pub library_item_id: Option<String>,
}

impl WorkshopCatalogEntry {
    pub fn has_cover(&self) -> bool {
        self.cover_path.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::{Path, PathBuf};

    use tempfile::TempDir;

    use crate::workshop::{SteamLibrary, WALLPAPER_ENGINE_APP_ID, WorkshopScanner};

    fn create_scanner() -> (TempDir, WorkshopScanner, PathBuf) {
        let temp_dir = TempDir::new().unwrap();
        let workshop_root = temp_dir
            .path()
            .join("steamapps/workshop/content")
            .join(WALLPAPER_ENGINE_APP_ID.to_string());
        fs::create_dir_all(&workshop_root).unwrap();

        let steam = SteamLibrary {
            root: temp_dir.path().to_path_buf(),
            libraries: Vec::new(),
        };

        (temp_dir, WorkshopScanner::new(steam), workshop_root)
    }

    fn create_item_dir(workshop_root: &Path, workshop_id: u64) -> PathBuf {
        let item_dir = workshop_root.join(workshop_id.to_string());
        fs::create_dir_all(&item_dir).unwrap();
        item_dir
    }

    fn write_project(item_dir: &Path, contents: &str) {
        fs::write(item_dir.join("project.json"), contents).unwrap();
    }

    #[test]
    fn bundled_cover_is_used_when_present() {
        let entry = WorkshopCatalogEntry {
            workshop_id: 101,
            title: "Forest Video".to_string(),
            project_type: WorkshopProjectType::Video,
            project_dir: PathBuf::from("/tmp/431960/101"),
            cover_path: Some(PathBuf::from("/tmp/431960/101/preview.jpg")),
            sync_state: WorkshopSyncState::Synced,
            supported_first_release: true,
            library_item_id: Some("forest-101".to_string()),
        };

        assert!(entry.has_cover());
    }

    #[test]
    fn unsupported_web_item_has_no_cover_requirement() {
        let entry = WorkshopCatalogEntry {
            workshop_id: 202,
            title: "Interactive Web".to_string(),
            project_type: WorkshopProjectType::Web,
            project_dir: PathBuf::from("/tmp/431960/202"),
            cover_path: None,
            sync_state: WorkshopSyncState::UnsupportedType,
            supported_first_release: false,
            library_item_id: None,
        };

        assert!(!entry.has_cover());
        assert!(!entry.supported_first_release);
    }

    #[test]
    fn malformed_project_json_does_not_fail_whole_scan() {
        let (_temp_dir, mut scanner, workshop_root) = create_scanner();

        let malformed_dir = create_item_dir(&workshop_root, 301);
        write_project(&malformed_dir, "{ not valid json }");

        let valid_dir = create_item_dir(&workshop_root, 302);
        write_project(
            &valid_dir,
            r#"{
                "type": "scene",
                "title": "Valid Scene"
            }"#,
        );

        let entries = scanner.scan_catalog().unwrap();

        assert_eq!(entries.len(), 2);

        let malformed = entries
            .iter()
            .find(|entry| entry.workshop_id == 301)
            .unwrap();
        assert_eq!(malformed.project_type, WorkshopProjectType::Other);
        assert_eq!(malformed.sync_state, WorkshopSyncState::MissingProjectFile);
        assert!(!malformed.supported_first_release);

        let valid = entries
            .iter()
            .find(|entry| entry.workshop_id == 302)
            .unwrap();
        assert_eq!(valid.project_type, WorkshopProjectType::Scene);
        assert_eq!(valid.sync_state, WorkshopSyncState::Synced);
        assert!(!valid.supported_first_release);
    }

    #[test]
    fn missing_primary_asset_yields_missing_primary_asset_state() {
        let (_temp_dir, mut scanner, workshop_root) = create_scanner();

        let item_dir = create_item_dir(&workshop_root, 401);
        write_project(
            &item_dir,
            r#"{
                "type": "video",
                "title": "Missing Video",
                "file": "missing.mp4"
            }"#,
        );

        let entries = scanner.scan_catalog().unwrap();
        let entry = entries
            .into_iter()
            .find(|entry| entry.workshop_id == 401)
            .unwrap();

        assert_eq!(entry.project_type, WorkshopProjectType::Video);
        assert_eq!(entry.sync_state, WorkshopSyncState::MissingPrimaryAsset);
        assert!(!entry.supported_first_release);
        assert!(entry.library_item_id.is_none());
    }

    #[test]
    fn unsupported_type_with_preview_uses_bundled_cover_only() {
        let (_temp_dir, mut scanner, workshop_root) = create_scanner();

        let item_dir = create_item_dir(&workshop_root, 501);
        fs::write(item_dir.join("preview.jpg"), b"preview").unwrap();
        write_project(
            &item_dir,
            r#"{
                "type": "web",
                "title": "Web Wallpaper",
                "preview": "preview.jpg"
            }"#,
        );

        let entries = scanner.scan_catalog().unwrap();
        let entry = entries
            .into_iter()
            .find(|entry| entry.workshop_id == 501)
            .unwrap();

        assert_eq!(entry.project_type, WorkshopProjectType::Web);
        assert_eq!(entry.sync_state, WorkshopSyncState::UnsupportedType);
        assert_eq!(entry.cover_path, Some(item_dir.join("preview.jpg")));
        assert!(entry.has_cover());
        assert!(!entry.supported_first_release);
        assert!(entry.library_item_id.is_none());
    }
}
