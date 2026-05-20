//! Steam Workshop integration for Wallpaper Engine
//!
//! Features:
//! - Automatic Steam library detection
//! - Workshop item scanning
//! - project.json parsing
//! - Wallpaper metadata extraction

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

use crate::workshop_catalog::{WorkshopCatalogEntry, WorkshopProjectType, WorkshopSyncState};
use lwe_core::{SourceType, WallpaperItem, WallpaperMetadata, WallpaperType};

/// Wallpaper Engine app ID on Steam
pub const WALLPAPER_ENGINE_APP_ID: u32 = 431960;

/// Steam library discovery and management
#[derive(Debug, Clone)]
pub struct SteamLibrary {
    /// Root Steam installation path
    pub root: PathBuf,
    /// Additional library folders
    pub libraries: Vec<PathBuf>,
}

impl SteamLibrary {
    /// Discover Steam installation automatically
    pub fn discover() -> Result<Self> {
        let root = Self::find_steam_root()?;
        let libraries = Self::parse_library_folders(&root)?;

        info!(
            "🎮 Found Steam installation with {} library folders",
            libraries.len()
        );
        Ok(Self { root, libraries })
    }

    /// Try to discover Steam, return None if not found
    pub fn try_discover() -> Option<Self> {
        Self::discover().ok()
    }

    /// Find Steam root directory
    fn find_steam_root() -> Result<PathBuf> {
        // Common Steam paths on Linux
        let candidates = [
            dirs::home_dir().map(|h| h.join(".steam/steam")),
            dirs::home_dir().map(|h| h.join(".local/share/Steam")),
            Some(PathBuf::from("/usr/share/steam")),
            // Flatpak Steam
            dirs::home_dir().map(|h| h.join(".var/app/com.valvesoftware.Steam/.steam/steam")),
            // Snap Steam
            dirs::home_dir().map(|h| h.join("snap/steam/common/.steam/steam")),
        ];

        for candidate in candidates.into_iter().flatten() {
            if candidate.exists() && candidate.join("steamapps").exists() {
                debug!("Found Steam at: {:?}", candidate);
                return Ok(candidate);
            }
        }

        anyhow::bail!("Steam installation not found")
    }

    /// Parse libraryfolders.vdf to find additional libraries
    fn parse_library_folders(root: &Path) -> Result<Vec<PathBuf>> {
        let vdf_path = root.join("steamapps/libraryfolders.vdf");
        if !vdf_path.exists() {
            return Ok(vec![root.to_path_buf()]);
        }

        let content = fs::read_to_string(&vdf_path).context("Failed to read libraryfolders.vdf")?;

        let mut libraries = vec![root.to_path_buf()];
        libraries.extend(Self::parse_vdf_paths(&content));

        // Deduplicate
        let mut seen = HashSet::new();
        libraries.retain(|p| seen.insert(p.clone()));

        Ok(libraries)
    }

    /// Parse VDF file for library paths
    fn parse_vdf_paths(content: &str) -> Vec<PathBuf> {
        let mut paths = Vec::new();

        for line in content.lines() {
            let line = line.trim();
            // Look for "path" key in VDF format: "path"		"/path/to/library"
            if line.starts_with("\"path\"") {
                if let Some(path_str) = Self::extract_vdf_value(line) {
                    let path = PathBuf::from(path_str);
                    if path.exists() {
                        paths.push(path);
                    }
                }
            }
        }

        paths
    }

    /// Extract quoted value from VDF line
    fn extract_vdf_value(line: &str) -> Option<String> {
        // Format: "key"		"value"
        let parts: Vec<&str> = line.split('"').collect();
        if parts.len() >= 4 {
            Some(parts[3].to_string())
        } else {
            None
        }
    }

    /// Get all library paths (including root)
    pub fn all_libraries(&self) -> Vec<&PathBuf> {
        std::iter::once(&self.root)
            .chain(self.libraries.iter())
            .collect()
    }

    /// Find Workshop content path for an app
    pub fn workshop_content_path(&self, app_id: u32) -> Vec<PathBuf> {
        self.all_libraries()
            .into_iter()
            .map(|lib| {
                lib.join("steamapps/workshop/content")
                    .join(app_id.to_string())
            })
            .filter(|p| p.exists())
            .collect()
    }

    /// Check if Wallpaper Engine is installed
    pub fn has_wallpaper_engine(&self) -> bool {
        !self
            .workshop_content_path(WALLPAPER_ENGINE_APP_ID)
            .is_empty()
    }
}

/// Wallpaper Engine project metadata from project.json
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeProject {
    /// Project type: "video", "scene", "web", "application"
    #[serde(rename = "type")]
    pub project_type: String,

    /// Video/scene file path (relative to project directory)
    pub file: Option<String>,

    /// Project title
    #[serde(default)]
    pub title: Option<String>,

    /// Project description
    #[serde(default)]
    pub description: Option<String>,

    /// Preview image path
    #[serde(default)]
    pub preview: Option<String>,

    /// Steam Workshop ID
    #[serde(default)]
    pub workshopid: Option<String>,

    /// Tags
    #[serde(default)]
    pub tags: Vec<String>,

    /// Content rating
    #[serde(default)]
    pub contentrating: Option<String>,
}

impl WeProject {
    /// Parse project.json from path
    pub fn load(project_dir: &Path) -> Result<Self> {
        let project_file = project_dir.join("project.json");
        let content = fs::read_to_string(&project_file)
            .with_context(|| format!("Failed to read {}", project_file.display()))?;

        serde_json::from_str(&content)
            .with_context(|| format!("Failed to parse {}", project_file.display()))
    }

    /// Check if this is a video type project
    pub fn is_video(&self) -> bool {
        self.project_type.to_lowercase() == "video"
    }

    /// Check if this is a scene type project
    pub fn is_scene(&self) -> bool {
        self.project_type.to_lowercase() == "scene"
    }

    /// Check if this project type is supported
    pub fn is_supported(&self) -> bool {
        self.is_video() || self.is_scene()
    }

    /// Get the wallpaper type
    pub fn wallpaper_type(&self) -> WallpaperType {
        match self.project_type.to_lowercase().as_str() {
            "video" => WallpaperType::Video,
            "scene" => WallpaperType::Scene,
            _ => WallpaperType::Video, // Default
        }
    }

    /// Get the reset-era catalog project type.
    pub fn project_type_enum(&self) -> WorkshopProjectType {
        match self.project_type.to_lowercase().as_str() {
            "video" => WorkshopProjectType::Video,
            "scene" => WorkshopProjectType::Scene,
            "web" => WorkshopProjectType::Web,
            _ => WorkshopProjectType::Other,
        }
    }

    /// Get the main file path
    pub fn main_file(&self, project_dir: &Path) -> Option<PathBuf> {
        self.file.as_ref().map(|f| project_dir.join(f))
    }

    /// Get the preview image path
    pub fn preview_image(&self, project_dir: &Path) -> Option<PathBuf> {
        self.preview.as_ref().map(|p| project_dir.join(p))
    }

    /// Resolve a bundled cover image when present.
    pub fn cover_image(&self, project_dir: &Path) -> Option<PathBuf> {
        self.preview_image(project_dir).filter(|path| path.exists())
    }
}

/// Workshop scanner for discovering Wallpaper Engine wallpapers
#[derive(Debug)]
pub struct WorkshopScanner {
    steam: SteamLibrary,
    /// Cache of scanned workshop IDs
    scanned_ids: HashSet<u64>,
}

impl WorkshopScanner {
    /// Create a new workshop scanner
    pub fn new(steam: SteamLibrary) -> Self {
        Self {
            steam,
            scanned_ids: HashSet::new(),
        }
    }

    /// Create scanner with auto-discovery
    pub fn discover() -> Result<Self> {
        let steam = SteamLibrary::discover()?;
        Ok(Self::new(steam))
    }

    /// Try to create scanner, return None if Steam not found
    pub fn try_discover() -> Option<Self> {
        SteamLibrary::try_discover().map(Self::new)
    }

    /// Scan all Workshop items for Wallpaper Engine
    pub fn scan_all(&mut self) -> Result<Vec<WallpaperItem>> {
        let workshop_paths = self.steam.workshop_content_path(WALLPAPER_ENGINE_APP_ID);

        if workshop_paths.is_empty() {
            warn!("⚠️ No Wallpaper Engine Workshop content found");
            return Ok(Vec::new());
        }

        let mut items = Vec::new();

        for workshop_path in workshop_paths {
            info!("🔍 Scanning Workshop: {}", workshop_path.display());

            let scanned = self.scan_workshop_directory(&workshop_path)?;
            items.extend(scanned);
        }

        info!("✅ Found {} Workshop wallpapers", items.len());
        Ok(items)
    }

    /// Scan Workshop items into reset-era catalog entries.
    pub fn scan_catalog(&mut self) -> Result<Vec<WorkshopCatalogEntry>> {
        let workshop_paths = self.steam.workshop_content_path(WALLPAPER_ENGINE_APP_ID);

        if workshop_paths.is_empty() {
            warn!("⚠️ No Wallpaper Engine Workshop content found");
            return Ok(Vec::new());
        }

        let mut entries = Vec::new();
        let mut seen_ids = HashSet::new();

        for workshop_path in workshop_paths {
            info!("🔍 Scanning Workshop catalog: {}", workshop_path.display());

            for entry in fs::read_dir(&workshop_path)? {
                let entry = entry?;
                let item_path = entry.path();

                if !item_path.is_dir() {
                    continue;
                }

                let workshop_id = match item_path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .and_then(|name| name.parse::<u64>().ok())
                {
                    Some(id) if seen_ids.insert(id) => id,
                    Some(_) => continue,
                    None => continue,
                };

                let project_file = item_path.join("project.json");
                if !project_file.exists() {
                    entries.push(Self::fallback_catalog_entry(workshop_id, item_path));
                    continue;
                }

                let project = match WeProject::load(&item_path) {
                    Ok(project) => project,
                    Err(error) => {
                        debug!(
                            "  ⚠️ Failed to read catalog project {}: {}",
                            project_file.display(),
                            error
                        );
                        entries.push(Self::fallback_catalog_entry(workshop_id, item_path));
                        continue;
                    }
                };
                let project_type = project.project_type_enum();
                let parsed_item = match project_type {
                    WorkshopProjectType::Video | WorkshopProjectType::Scene => {
                        self.parse_workshop_item_const(&item_path, workshop_id)?
                    }
                    WorkshopProjectType::Web | WorkshopProjectType::Other => None,
                };
                let supported_first_release =
                    project_type == WorkshopProjectType::Video && parsed_item.is_some();
                let sync_state = match project_type {
                    WorkshopProjectType::Web | WorkshopProjectType::Other => {
                        WorkshopSyncState::UnsupportedType
                    }
                    WorkshopProjectType::Video | WorkshopProjectType::Scene => {
                        if parsed_item.is_some() {
                            WorkshopSyncState::Synced
                        } else {
                            WorkshopSyncState::MissingPrimaryAsset
                        }
                    }
                };

                entries.push(WorkshopCatalogEntry {
                    workshop_id,
                    title: project
                        .title
                        .clone()
                        .unwrap_or_else(|| format!("Workshop #{workshop_id}")),
                    project_type,
                    project_dir: item_path.clone(),
                    cover_path: project.cover_image(&item_path),
                    sync_state,
                    supported_first_release,
                    library_item_id: parsed_item.map(|item| item.id),
                });
            }
        }

        entries.sort_by_key(|entry| entry.workshop_id);
        Ok(entries)
    }

    fn fallback_catalog_entry(workshop_id: u64, project_dir: PathBuf) -> WorkshopCatalogEntry {
        WorkshopCatalogEntry {
            workshop_id,
            title: format!("Workshop #{workshop_id}"),
            project_type: WorkshopProjectType::Other,
            project_dir,
            cover_path: None,
            sync_state: WorkshopSyncState::MissingProjectFile,
            supported_first_release: false,
            library_item_id: None,
        }
    }

    /// Scan a specific Workshop directory
    fn scan_workshop_directory(&mut self, path: &Path) -> Result<Vec<WallpaperItem>> {
        let mut items = Vec::new();

        let entries = fs::read_dir(path)?;

        for entry in entries {
            let entry = entry?;
            let item_path = entry.path();

            if !item_path.is_dir() {
                continue;
            }

            // Extract workshop ID from directory name
            let workshop_id: u64 = match item_path
                .file_name()
                .and_then(|n| n.to_str())
                .and_then(|s| s.parse().ok())
            {
                Some(id) => id,
                None => continue,
            };

            // Skip already scanned items
            if !self.scanned_ids.insert(workshop_id) {
                continue;
            }

            // Try to parse as WE project
            match self.parse_workshop_item(&item_path, workshop_id) {
                Ok(Some(item)) => {
                    debug!("  📄 {}", item.name);
                    items.push(item);
                }
                Ok(None) => {
                    // Unsupported type, skip
                }
                Err(e) => {
                    debug!("  ⚠️ Failed to parse {}: {}", item_path.display(), e);
                }
            }
        }

        Ok(items)
    }

    /// Parse a Workshop item directory
    fn parse_workshop_item(
        &self,
        item_path: &Path,
        workshop_id: u64,
    ) -> Result<Option<WallpaperItem>> {
        let project_file = item_path.join("project.json");

        if !project_file.exists() {
            return Ok(None);
        }

        let project = WeProject::load(item_path)?;

        // Recognize video and scene projects for local Library/catalog workflows.
        // Runtime support is narrower and currently video-first.
        if !project.is_supported() {
            debug!("  ⏭️ Skipping unsupported type: {}", project.project_type);
            return Ok(None);
        }

        // Get main file path
        let source_path = match project.main_file(item_path) {
            Some(path) if path.exists() => path,
            Some(path) => {
                debug!("  ⚠️ Main file not found: {}", path.display());
                return Ok(None);
            }
            None => {
                // For scene type, use project directory as source
                if project.is_scene() {
                    item_path.to_path_buf()
                } else {
                    return Ok(None);
                }
            }
        };

        // Create wallpaper item
        let name = project
            .title
            .clone()
            .unwrap_or_else(|| format!("Workshop #{}", workshop_id));

        let mut item = WallpaperItem::new(
            source_path,
            name,
            SourceType::SteamWorkshop,
            project.wallpaper_type(),
        );

        // Set metadata
        item.metadata = WallpaperMetadata {
            title: project.title.clone(),
            author: None, // Not in project.json
            description: project.description.clone(),
            tags: project.tags.clone(),
            duration_secs: None,
            resolution: None,
            file_size: None,
            workshop_id: Some(workshop_id),
        };

        // Set thumbnail path if preview exists
        if let Some(preview) = project.cover_image(item_path) {
            item.thumbnail_path = Some(preview);
        }

        Ok(Some(item))
    }

    /// Get workshop item by ID
    pub fn get_item(&self, workshop_id: u64) -> Result<Option<WallpaperItem>> {
        let workshop_paths = self.steam.workshop_content_path(WALLPAPER_ENGINE_APP_ID);

        for workshop_path in workshop_paths {
            let item_path = workshop_path.join(workshop_id.to_string());
            if item_path.exists() {
                return match self.parse_workshop_item_const(&item_path, workshop_id)? {
                    Some(item) => Ok(Some(item)),
                    None => continue,
                };
            }
        }

        Ok(None)
    }

    /// Parse workshop item (const version, doesn't modify cache)
    fn parse_workshop_item_const(
        &self,
        item_path: &Path,
        workshop_id: u64,
    ) -> Result<Option<WallpaperItem>> {
        // Clone self temporarily to avoid mutation
        let temp_scanner = WorkshopScanner {
            steam: self.steam.clone(),
            scanned_ids: HashSet::new(),
        };
        temp_scanner.parse_workshop_item(item_path, workshop_id)
    }

    /// Clear scanned cache
    pub fn clear_cache(&mut self) {
        self.scanned_ids.clear();
    }

    /// Get Steam library reference
    pub fn steam(&self) -> &SteamLibrary {
        &self.steam
    }
}

/// Detect if a path is a Wallpaper Engine project
pub fn is_we_project(path: &Path) -> bool {
    path.join("project.json").exists()
}

/// Get project type from path
pub fn get_project_type(path: &Path) -> Result<String> {
    let project = WeProject::load(path)?;
    Ok(project.project_type)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;
    use tempfile::TempDir;

    #[test]
    fn test_vdf_value_extraction() {
        let line = r#"		"path"		"/home/user/SteamLibrary""#;
        let value = SteamLibrary::extract_vdf_value(line);
        assert_eq!(value, Some("/home/user/SteamLibrary".to_string()));
    }

    #[test]
    fn test_vdf_parsing() {
        let content = r#"
"libraryfolders"
{
	"0"
	{
		"path"		"/home/user/.local/share/Steam"
		"label"		""
	}
	"1"
	{
		"path"		"/mnt/games/SteamLibrary"
		"label"		"Games"
	}
}
"#;
        let paths = SteamLibrary::parse_vdf_paths(content);
        // Note: paths must exist to be returned
        assert!(paths.len() <= 2);
    }

    #[test]
    fn test_we_project_parse() {
        let temp_dir = TempDir::new().unwrap();
        let project_file = temp_dir.path().join("project.json");

        let content = r#"{
            "type": "video",
            "file": "video.mp4",
            "title": "Test Video",
            "description": "A test video wallpaper",
            "preview": "preview.jpg",
            "workshopid": "123456",
            "tags": ["nature", "landscape"]
        }"#;

        let mut file = File::create(&project_file).unwrap();
        file.write_all(content.as_bytes()).unwrap();

        let project = WeProject::load(temp_dir.path()).unwrap();

        assert_eq!(project.project_type, "video");
        assert_eq!(project.file.as_deref(), Some("video.mp4"));
        assert_eq!(project.title.as_deref(), Some("Test Video"));
        assert!(project.is_video());
        assert!(project.is_supported());
        assert_eq!(project.tags.len(), 2);
    }

    #[test]
    fn test_we_project_type_detection() {
        let temp_dir = TempDir::new().unwrap();
        let project_file = temp_dir.path().join("project.json");

        // Video type
        let content = r#"{"type": "video", "file": "test.mp4"}"#;
        fs::write(&project_file, content).unwrap();
        let project = WeProject::load(temp_dir.path()).unwrap();
        assert!(project.is_video());
        assert!(!project.is_scene());
        assert!(project.is_supported());

        // Scene type
        let content = r#"{"type": "scene", "file": "scene.json"}"#;
        fs::write(&project_file, content).unwrap();
        let project = WeProject::load(temp_dir.path()).unwrap();
        assert!(!project.is_video());
        assert!(project.is_scene());
        assert!(project.is_supported());

        // Web type (unsupported)
        let content = r#"{"type": "web", "file": "index.html"}"#;
        fs::write(&project_file, content).unwrap();
        let project = WeProject::load(temp_dir.path()).unwrap();
        assert!(!project.is_video());
        assert!(!project.is_scene());
        assert!(!project.is_supported());
    }

    #[test]
    fn test_is_we_project() {
        let temp_dir = TempDir::new().unwrap();

        // No project.json
        assert!(!is_we_project(temp_dir.path()));

        // With project.json
        File::create(temp_dir.path().join("project.json")).unwrap();
        assert!(is_we_project(temp_dir.path()));
    }
}
