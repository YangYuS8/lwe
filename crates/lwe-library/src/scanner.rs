//! Folder scanner for discovering wallpapers
//!
//! Features:
//! - Parallel scanning using rayon
//! - Incremental scanning with change detection
//! - File system watching with debounced events
//! - Automatic file type detection

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant, SystemTime};

use anyhow::{Context, Result};
use notify::RecursiveMode;
use notify_debouncer_mini::{DebounceEventResult, DebouncedEventKind, Debouncer, new_debouncer};
use parking_lot::RwLock;
use rayon::prelude::*;
use tracing::{debug, info, warn};
use walkdir::WalkDir;

use lwe_core::{SourceType, WallpaperItem, WallpaperType};

/// File scanner for discovering wallpapers
#[derive(Debug, Clone)]
pub struct FolderScanner {
    /// Supported video extensions
    video_extensions: HashSet<String>,
    /// Supported image extensions
    image_extensions: HashSet<String>,
}

impl FolderScanner {
    pub fn new() -> Self {
        Self {
            video_extensions: ["mp4", "mkv", "webm", "avi", "mov", "m4v", "wmv", "flv"]
                .iter()
                .map(|s| s.to_string())
                .collect(),

            image_extensions: ["png", "jpg", "jpeg", "webp", "bmp", "tiff", "tif", "gif"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
        }
    }

    /// Scan a folder for wallpapers (single-threaded)
    pub fn scan_folder(&self, path: &Path, recursive: bool) -> Result<Vec<WallpaperItem>> {
        info!("🔍 Scanning folder: {}", path.display());

        if !path.exists() {
            warn!("  ⚠️ Folder does not exist: {}", path.display());
            return Ok(Vec::new());
        }

        let mut items = Vec::new();

        let walker = if recursive {
            WalkDir::new(path)
        } else {
            WalkDir::new(path).max_depth(1)
        };

        for entry in walker.into_iter().filter_map(|e| e.ok()) {
            let file_path = entry.path();

            if !file_path.is_file() {
                continue;
            }

            if let Some(item) = self.process_file(file_path) {
                debug!("  📄 Found: {}", item.name);
                items.push(item);
            }
        }

        info!("  ✓ Found {} wallpapers", items.len());
        Ok(items)
    }

    /// Scan a folder in parallel using rayon
    pub fn scan_folder_parallel(&self, path: &Path, recursive: bool) -> Result<Vec<WallpaperItem>> {
        info!("🔍 Parallel scanning folder: {}", path.display());

        if !path.exists() {
            warn!("  ⚠️ Folder does not exist: {}", path.display());
            return Ok(Vec::new());
        }

        let start = Instant::now();

        // Collect all file paths first
        let walker = if recursive {
            WalkDir::new(path)
        } else {
            WalkDir::new(path).max_depth(1)
        };

        let file_paths: Vec<PathBuf> = walker
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_file())
            .filter(|e| self.is_wallpaper_file(e.path()))
            .map(|e| e.path().to_owned())
            .collect();

        // Process files in parallel
        let items: Vec<WallpaperItem> = file_paths
            .par_iter()
            .filter_map(|path| self.process_file(path))
            .collect();

        let duration = start.elapsed();
        info!(
            "  ✓ Found {} wallpapers in {:?} (parallel)",
            items.len(),
            duration
        );

        Ok(items)
    }

    /// Scan multiple folders in parallel
    pub fn scan_folders_parallel(
        &self,
        folders: &[(PathBuf, bool)],
    ) -> Result<HashMap<PathBuf, Vec<WallpaperItem>>> {
        info!("🔍 Scanning {} folders in parallel", folders.len());
        let start = Instant::now();

        let results: HashMap<PathBuf, Vec<WallpaperItem>> = folders
            .par_iter()
            .map(|(path, recursive)| {
                let items = self
                    .scan_folder_parallel(path, *recursive)
                    .unwrap_or_default();
                (path.clone(), items)
            })
            .collect();

        let total_items: usize = results.values().map(|v| v.len()).sum();
        let duration = start.elapsed();
        info!(
            "  ✓ Found {} total wallpapers across {} folders in {:?}",
            total_items,
            folders.len(),
            duration
        );

        Ok(results)
    }

    /// Process a single file and create WallpaperItem if valid
    pub fn process_file(&self, path: &Path) -> Option<WallpaperItem> {
        let extension = path.extension()?.to_string_lossy().to_lowercase();

        let wallpaper_type = if self.video_extensions.contains(&extension) {
            WallpaperType::Video
        } else if extension == "gif" {
            WallpaperType::Gif
        } else if self.image_extensions.contains(&extension) {
            WallpaperType::Image
        } else {
            return None;
        };

        let name = path.file_stem()?.to_string_lossy().to_string();

        // Get file metadata
        let file_size = fs::metadata(path).ok().map(|m| m.len());

        let mut item =
            WallpaperItem::new(path.to_owned(), name, SourceType::LocalFile, wallpaper_type);

        // Update metadata with file size
        item.metadata.file_size = file_size;

        Some(item)
    }

    /// Check if a file is a supported wallpaper
    pub fn is_wallpaper_file(&self, path: &Path) -> bool {
        if let Some(ext) = path.extension() {
            let ext = ext.to_string_lossy().to_lowercase();
            self.video_extensions.contains(&ext) || self.image_extensions.contains(&ext)
        } else {
            false
        }
    }

    /// Get the wallpaper type for a file
    pub fn get_wallpaper_type(&self, path: &Path) -> Option<WallpaperType> {
        let ext = path.extension()?.to_string_lossy().to_lowercase();

        if self.video_extensions.contains(&ext) {
            Some(WallpaperType::Video)
        } else if ext == "gif" {
            Some(WallpaperType::Gif)
        } else if self.image_extensions.contains(&ext) {
            Some(WallpaperType::Image)
        } else {
            None
        }
    }

    /// Add custom video extension
    pub fn add_video_extension(&mut self, ext: &str) {
        self.video_extensions.insert(ext.to_lowercase());
    }

    /// Add custom image extension
    pub fn add_image_extension(&mut self, ext: &str) {
        self.image_extensions.insert(ext.to_lowercase());
    }
}

impl Default for FolderScanner {
    fn default() -> Self {
        Self::new()
    }
}

/// Result of a scan operation
#[derive(Debug, Clone, Default)]
pub struct ScanResult {
    /// New wallpapers found
    pub added: Vec<WallpaperItem>,
    /// Wallpapers that no longer exist (IDs)
    pub removed: Vec<String>,
    /// Wallpapers that were updated
    pub updated: Vec<WallpaperItem>,
    /// Total files scanned
    pub files_scanned: usize,
    /// Scan duration in milliseconds
    pub duration_ms: u64,
}

impl ScanResult {
    pub fn has_changes(&self) -> bool {
        !self.added.is_empty() || !self.removed.is_empty() || !self.updated.is_empty()
    }

    pub fn merge(&mut self, other: ScanResult) {
        self.added.extend(other.added);
        self.removed.extend(other.removed);
        self.updated.extend(other.updated);
        self.files_scanned += other.files_scanned;
        self.duration_ms += other.duration_ms;
    }
}

/// Incremental scanner that tracks changes
pub struct IncrementalScanner {
    scanner: FolderScanner,
    /// Known files and their modification times
    known_files: HashMap<PathBuf, SystemTime>,
}

impl IncrementalScanner {
    pub fn new() -> Self {
        Self {
            scanner: FolderScanner::new(),
            known_files: HashMap::new(),
        }
    }

    /// Perform incremental scan, only processing changed files
    pub fn scan_incremental(&mut self, path: &Path, recursive: bool) -> Result<ScanResult> {
        let start = Instant::now();
        let mut result = ScanResult::default();

        if !path.exists() {
            return Ok(result);
        }

        let walker = if recursive {
            WalkDir::new(path)
        } else {
            WalkDir::new(path).max_depth(1)
        };

        let mut current_files: HashSet<PathBuf> = HashSet::new();

        for entry in walker.into_iter().filter_map(|e| e.ok()) {
            let file_path = entry.path();

            if !file_path.is_file() || !self.scanner.is_wallpaper_file(file_path) {
                continue;
            }

            result.files_scanned += 1;
            current_files.insert(file_path.to_owned());

            let modified = fs::metadata(file_path).ok().and_then(|m| m.modified().ok());

            if let Some(modified) = modified {
                if let Some(known_modified) = self.known_files.get(file_path) {
                    // Check if file was modified
                    if &modified != known_modified {
                        if let Some(item) = self.scanner.process_file(file_path) {
                            result.updated.push(item);
                        }
                    }
                } else {
                    // New file
                    if let Some(item) = self.scanner.process_file(file_path) {
                        result.added.push(item);
                    }
                }
                self.known_files.insert(file_path.to_owned(), modified);
            }
        }

        // Find removed files
        let removed_paths: Vec<PathBuf> = self
            .known_files
            .keys()
            .filter(|p| p.starts_with(path) && !current_files.contains(*p))
            .cloned()
            .collect();

        for removed_path in removed_paths {
            let id = WallpaperItem::generate_id(&removed_path);
            result.removed.push(id);
            self.known_files.remove(&removed_path);
        }

        result.duration_ms = start.elapsed().as_millis() as u64;
        Ok(result)
    }

    /// Perform parallel incremental scan
    pub fn scan_incremental_parallel(
        &mut self,
        path: &Path,
        recursive: bool,
    ) -> Result<ScanResult> {
        let start = Instant::now();
        let mut result = ScanResult::default();

        if !path.exists() {
            return Ok(result);
        }

        // Collect all wallpaper files
        let walker = if recursive {
            WalkDir::new(path)
        } else {
            WalkDir::new(path).max_depth(1)
        };

        let file_paths: Vec<PathBuf> = walker
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_file())
            .filter(|e| self.scanner.is_wallpaper_file(e.path()))
            .map(|e| e.path().to_owned())
            .collect();

        result.files_scanned = file_paths.len();
        let current_files: HashSet<PathBuf> = file_paths.iter().cloned().collect();

        // Process files in parallel, checking modification times
        let known_files_ref = &self.known_files;
        let scanner_ref = &self.scanner;

        let changes: Vec<(PathBuf, Option<WallpaperItem>, bool)> = file_paths
            .par_iter()
            .filter_map(|file_path| {
                let modified = fs::metadata(file_path)
                    .ok()
                    .and_then(|m| m.modified().ok())?;

                if let Some(known_modified) = known_files_ref.get(file_path) {
                    if &modified != known_modified {
                        // Updated
                        let item = scanner_ref.process_file(file_path);
                        Some((file_path.clone(), item, false))
                    } else {
                        None
                    }
                } else {
                    // New
                    let item = scanner_ref.process_file(file_path);
                    Some((file_path.clone(), item, true))
                }
            })
            .collect();

        // Update known_files and collect results
        for (file_path, item_opt, is_new) in changes {
            if let Some(item) = item_opt {
                if is_new {
                    result.added.push(item);
                } else {
                    result.updated.push(item);
                }
            }
            if let Ok(modified) = fs::metadata(&file_path).and_then(|m| m.modified()) {
                self.known_files.insert(file_path, modified);
            }
        }

        // Find removed files
        let removed_paths: Vec<PathBuf> = self
            .known_files
            .keys()
            .filter(|p| p.starts_with(path) && !current_files.contains(*p))
            .cloned()
            .collect();

        for removed_path in removed_paths {
            let id = WallpaperItem::generate_id(&removed_path);
            result.removed.push(id);
            self.known_files.remove(&removed_path);
        }

        result.duration_ms = start.elapsed().as_millis() as u64;
        Ok(result)
    }

    /// Clear known files cache
    pub fn clear_cache(&mut self) {
        self.known_files.clear();
    }

    /// Get number of known files
    pub fn known_file_count(&self) -> usize {
        self.known_files.len()
    }
}

impl Default for IncrementalScanner {
    fn default() -> Self {
        Self::new()
    }
}

/// File system event type
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileEvent {
    /// New file created
    Created(PathBuf),
    /// File modified
    Modified(PathBuf),
    /// File deleted
    Deleted(PathBuf),
}

/// File system watcher for real-time change detection
pub struct FileWatcher {
    scanner: FolderScanner,
    #[allow(dead_code)]
    debouncer: Debouncer<notify::RecommendedWatcher>,
    event_rx: Receiver<FileEvent>,
}

impl FileWatcher {
    /// Create a new file watcher for the given paths
    pub fn new(paths: Vec<(PathBuf, bool)>, debounce_duration: Duration) -> Result<Self> {
        let scanner = FolderScanner::new();
        let (event_tx, event_rx) = channel();

        // Create scanner clone for the debouncer callback
        let scanner_clone = scanner.clone();

        // Create debounced watcher
        let debouncer = new_debouncer(debounce_duration, move |res: DebounceEventResult| {
            if let Ok(events) = res {
                for event in events {
                    let path = event.path;

                    // Only process wallpaper files
                    if path.is_file() && scanner_clone.is_wallpaper_file(&path) {
                        let file_event = match event.kind {
                            DebouncedEventKind::Any => {
                                if path.exists() {
                                    FileEvent::Modified(path)
                                } else {
                                    FileEvent::Deleted(path)
                                }
                            }
                            DebouncedEventKind::AnyContinuous => FileEvent::Modified(path),
                            _ => FileEvent::Modified(path), // Handle future variants
                        };
                        let _ = event_tx.send(file_event);
                    } else if !path.exists() && scanner_clone.is_wallpaper_file(&path) {
                        let _ = event_tx.send(FileEvent::Deleted(path));
                    }
                }
            }
        })
        .context("Failed to create file watcher")?;

        let mut watcher = Self {
            scanner,
            debouncer,
            event_rx,
        };

        // Watch all paths
        for (path, recursive) in &paths {
            let mode = if *recursive {
                RecursiveMode::Recursive
            } else {
                RecursiveMode::NonRecursive
            };
            watcher
                .debouncer
                .watcher()
                .watch(path, mode)
                .with_context(|| format!("Failed to watch path: {}", path.display()))?;
        }

        info!("👁️ Watching {} paths for changes", paths.len());
        Ok(watcher)
    }

    /// Try to receive a file event (non-blocking)
    pub fn try_recv(&self) -> Option<FileEvent> {
        self.event_rx.try_recv().ok()
    }

    /// Receive file event with timeout
    pub fn recv_timeout(&self, timeout: Duration) -> Option<FileEvent> {
        self.event_rx.recv_timeout(timeout).ok()
    }

    /// Process event and return WallpaperItem for created/modified files
    pub fn process_event(&self, event: &FileEvent) -> Option<WallpaperItem> {
        match event {
            FileEvent::Created(path) | FileEvent::Modified(path) => self.scanner.process_file(path),
            FileEvent::Deleted(_) => None,
        }
    }
}

/// Async file watcher using tokio channels
pub struct AsyncFileWatcher {
    scanner: Arc<FolderScanner>,
    watched_paths: Arc<RwLock<Vec<(PathBuf, bool)>>>,
    event_tx: tokio::sync::mpsc::Sender<FileEvent>,
    event_rx: tokio::sync::mpsc::Receiver<FileEvent>,
}

impl AsyncFileWatcher {
    /// Create a new async file watcher
    pub fn new(buffer_size: usize) -> Self {
        let (event_tx, event_rx) = tokio::sync::mpsc::channel(buffer_size);

        Self {
            scanner: Arc::new(FolderScanner::new()),
            watched_paths: Arc::new(RwLock::new(Vec::new())),
            event_tx,
            event_rx,
        }
    }

    /// Add path to watch
    pub fn add_path(&self, path: PathBuf, recursive: bool) {
        self.watched_paths.write().push((path, recursive));
    }

    /// Remove path from watch
    pub fn remove_path(&self, path: &Path) {
        self.watched_paths.write().retain(|(p, _)| p != path);
    }

    /// Receive next event (async)
    pub async fn recv(&mut self) -> Option<FileEvent> {
        self.event_rx.recv().await
    }

    /// Get event sender for external use
    pub fn event_sender(&self) -> tokio::sync::mpsc::Sender<FileEvent> {
        self.event_tx.clone()
    }

    /// Get scanner reference
    pub fn scanner(&self) -> &FolderScanner {
        &self.scanner
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use tempfile::TempDir;

    fn create_test_files(dir: &Path) {
        File::create(dir.join("video1.mp4")).unwrap();
        File::create(dir.join("video2.mkv")).unwrap();
        File::create(dir.join("image1.png")).unwrap();
        File::create(dir.join("document.pdf")).unwrap(); // Should be ignored
    }

    #[test]
    fn test_scan_folder() {
        let temp_dir = TempDir::new().unwrap();
        create_test_files(temp_dir.path());

        let scanner = FolderScanner::new();
        let items = scanner.scan_folder(temp_dir.path(), false).unwrap();

        assert_eq!(items.len(), 3); // 2 videos + 1 image

        let video_count = items
            .iter()
            .filter(|i| i.wallpaper_type == WallpaperType::Video)
            .count();
        let image_count = items
            .iter()
            .filter(|i| i.wallpaper_type == WallpaperType::Image)
            .count();

        assert_eq!(video_count, 2);
        assert_eq!(image_count, 1);
    }

    #[test]
    fn test_scan_folder_parallel() {
        let temp_dir = TempDir::new().unwrap();
        create_test_files(temp_dir.path());

        let scanner = FolderScanner::new();
        let items = scanner
            .scan_folder_parallel(temp_dir.path(), false)
            .unwrap();

        assert_eq!(items.len(), 3);
    }

    #[test]
    fn test_is_wallpaper_file() {
        let scanner = FolderScanner::new();

        assert!(scanner.is_wallpaper_file(Path::new("test.mp4")));
        assert!(scanner.is_wallpaper_file(Path::new("test.PNG"))); // Case insensitive
        assert!(scanner.is_wallpaper_file(Path::new("test.webm")));
        assert!(!scanner.is_wallpaper_file(Path::new("test.txt")));
        assert!(!scanner.is_wallpaper_file(Path::new("test.pdf")));
    }

    #[test]
    fn test_get_wallpaper_type() {
        let scanner = FolderScanner::new();

        assert_eq!(
            scanner.get_wallpaper_type(Path::new("test.mp4")),
            Some(WallpaperType::Video)
        );
        assert_eq!(
            scanner.get_wallpaper_type(Path::new("test.png")),
            Some(WallpaperType::Image)
        );
        assert_eq!(
            scanner.get_wallpaper_type(Path::new("test.gif")),
            Some(WallpaperType::Gif)
        );
        assert_eq!(scanner.get_wallpaper_type(Path::new("test.txt")), None);
    }

    #[test]
    fn test_incremental_scanner() {
        let temp_dir = TempDir::new().unwrap();
        create_test_files(temp_dir.path());

        let mut scanner = IncrementalScanner::new();

        // First scan should find all files as new
        let result1 = scanner.scan_incremental(temp_dir.path(), false).unwrap();
        assert_eq!(result1.added.len(), 3);
        assert!(result1.removed.is_empty());
        assert!(result1.updated.is_empty());

        // Second scan should find no changes
        let result2 = scanner.scan_incremental(temp_dir.path(), false).unwrap();
        assert!(result2.added.is_empty());
        assert!(result2.removed.is_empty());
        assert!(result2.updated.is_empty());

        // Delete a file
        std::fs::remove_file(temp_dir.path().join("video1.mp4")).unwrap();

        // Third scan should detect deletion
        let result3 = scanner.scan_incremental(temp_dir.path(), false).unwrap();
        assert!(result3.added.is_empty());
        assert_eq!(result3.removed.len(), 1);
        assert!(result3.updated.is_empty());
    }

    #[test]
    fn test_scan_result_merge() {
        let mut result1 = ScanResult {
            added: vec![WallpaperItem::new(
                PathBuf::from("/a.mp4"),
                "a".to_string(),
                SourceType::LocalFile,
                WallpaperType::Video,
            )],
            removed: vec!["id1".to_string()],
            updated: vec![],
            files_scanned: 10,
            duration_ms: 100,
        };

        let result2 = ScanResult {
            added: vec![WallpaperItem::new(
                PathBuf::from("/b.mp4"),
                "b".to_string(),
                SourceType::LocalFile,
                WallpaperType::Video,
            )],
            removed: vec!["id2".to_string()],
            updated: vec![],
            files_scanned: 20,
            duration_ms: 200,
        };

        result1.merge(result2);

        assert_eq!(result1.added.len(), 2);
        assert_eq!(result1.removed.len(), 2);
        assert_eq!(result1.files_scanned, 30);
        assert_eq!(result1.duration_ms, 300);
    }
}
