//! Thumbnail generation for wallpapers
//!
//! Features:
//! - WebP output format (smaller, better quality)
//! - Persistent cache directory (~/.cache/lwe/card-thumbnails/)
//! - Background generation with async API
//! - GIF animation preview (first frame)
//! - Video frame extraction with ffmpeg

use std::collections::HashSet;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex, OnceLock, mpsc as std_mpsc};
use std::thread;

use anyhow::{Context, Result};
use image::{DynamicImage, GenericImageView, ImageFormat};
use parking_lot::RwLock;
use sha2::{Digest, Sha256};
use tracing::{debug, info, warn};

/// Default thumbnail width
pub const THUMBNAIL_WIDTH: u32 = 320;
/// Default thumbnail height
pub const THUMBNAIL_HEIGHT: u32 = 180;
/// Maximum original preview dimension that may be passed directly to card grids.
pub const MAX_DIRECT_CARD_PREVIEW_DIMENSION: u32 = 320;
/// Number of background workers used for cold card-grid thumbnail generation.
pub const CARD_THUMBNAIL_WORKER_COUNT: usize = 2;
/// WebP quality (0-100, higher = better)
pub const WEBP_QUALITY: u8 = 80;

/// Output format for thumbnails
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThumbnailFormat {
    #[default]
    WebP,
    Png,
    Jpeg,
}

impl ThumbnailFormat {
    pub fn extension(&self) -> &str {
        match self {
            Self::WebP => "webp",
            Self::Png => "png",
            Self::Jpeg => "jpg",
        }
    }

    fn image_format(&self) -> ImageFormat {
        match self {
            Self::WebP => ImageFormat::WebP,
            Self::Png => ImageFormat::Png,
            Self::Jpeg => ImageFormat::Jpeg,
        }
    }
}

/// Thumbnail generator with caching support
pub struct ThumbnailGenerator {
    /// Target width
    pub width: u32,
    /// Target height
    pub height: u32,
    /// Output format
    pub format: ThumbnailFormat,
    /// Cache directory
    cache_dir: PathBuf,
    /// Whether ffmpeg is available
    ffmpeg_available: bool,
}

impl ThumbnailGenerator {
    /// Create a new thumbnail generator with default settings
    pub fn new() -> Self {
        let ffmpeg_available = check_ffmpeg();
        if !ffmpeg_available {
            warn!("⚠️ ffmpeg not found - video thumbnails will not be generated");
        }

        let cache_dir = Self::default_cache_dir();
        if let Err(e) = std::fs::create_dir_all(&cache_dir) {
            warn!("Failed to create thumbnail cache directory: {}", e);
        }

        Self {
            width: THUMBNAIL_WIDTH,
            height: THUMBNAIL_HEIGHT,
            format: ThumbnailFormat::WebP,
            cache_dir,
            ffmpeg_available,
        }
    }

    /// Create generator with custom size
    pub fn with_size(width: u32, height: u32) -> Self {
        let mut generator = Self::new();
        generator.width = width;
        generator.height = height;
        generator
    }

    /// Create generator with custom settings
    pub fn with_options(
        width: u32,
        height: u32,
        format: ThumbnailFormat,
        cache_dir: PathBuf,
    ) -> Self {
        let ffmpeg_available = check_ffmpeg();
        let _ = std::fs::create_dir_all(&cache_dir);

        Self {
            width,
            height,
            format,
            cache_dir,
            ffmpeg_available,
        }
    }

    /// Get default cache directory
    pub fn default_cache_dir() -> PathBuf {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| dirs::cache_dir().unwrap_or_else(|| PathBuf::from(".")))
            .join(".cache")
            .join("lwe")
            .join("card-thumbnails")
    }

    /// Create a card-grid thumbnail selector without probing video thumbnail support.
    pub fn for_card_grid() -> Self {
        let cache_dir = Self::default_cache_dir();
        if let Err(e) = std::fs::create_dir_all(&cache_dir) {
            warn!("Failed to create thumbnail cache directory: {}", e);
        }

        Self {
            width: THUMBNAIL_WIDTH,
            height: THUMBNAIL_HEIGHT,
            format: ThumbnailFormat::WebP,
            cache_dir,
            ffmpeg_available: false,
        }
    }

    /// Get cache path for a source file
    pub fn cache_path(&self, source_path: &Path) -> PathBuf {
        let hash = hash_source_identity(source_path);
        self.cache_dir
            .join(format!("{}.{}", hash, self.format.extension()))
    }

    /// Get the cache directory used by this generator.
    pub fn cache_dir(&self) -> &Path {
        &self.cache_dir
    }

    /// Check if thumbnail exists in cache
    pub fn is_cached(&self, source_path: &Path) -> bool {
        self.cache_path(source_path).exists()
    }

    /// Get cached thumbnail if exists
    pub fn get_cached(&self, source_path: &Path) -> Option<ThumbnailResult> {
        let cache_path = self.cache_path(source_path);
        if !cache_path.exists() {
            return None;
        }

        let data = std::fs::read(&cache_path).ok()?;
        let img = image::load_from_memory(&data).ok()?;
        let (width, height) = img.dimensions();

        Some(ThumbnailResult {
            data,
            width,
            height,
            original_width: 0, // Unknown from cache
            original_height: 0,
            format: self.format.extension().to_string(),
            cached: true,
        })
    }

    /// Return a fresh cached thumbnail path for a source file when one exists.
    pub fn cached_path(&self, source_path: &Path) -> Option<PathBuf> {
        let cache_path = self.cache_path(source_path);
        cache_path.exists().then_some(cache_path)
    }

    /// Generate a thumbnail and return the cache path written for it.
    pub fn generate_cached_path(&self, source_path: &Path) -> Result<PathBuf> {
        let cache_path = self.cache_path(source_path);
        self.generate(source_path)?;
        Ok(cache_path)
    }

    /// Select a card-grid-safe cover path for bundled Workshop preview media.
    ///
    /// Static small images can be passed through directly. GIFs and oversized images use
    /// cached static thumbnails. On a cold cache this schedules bounded background work and
    /// returns `None`, allowing the UI to render its existing placeholder instead of decoding
    /// expensive preview media in the hot render path.
    pub fn card_grid_cover_path(&self, source_path: &Path) -> Option<PathBuf> {
        if !source_path.exists() {
            return None;
        }

        if let Some(cache_path) = self.cached_path(source_path) {
            return Some(cache_path);
        }

        if can_render_directly_in_card_grid(source_path) {
            return Some(source_path.to_path_buf());
        }

        request_card_thumbnail_generation(source_path.to_path_buf());
        None
    }

    /// Generate thumbnail for a wallpaper file
    pub fn generate(&self, path: &Path) -> Result<ThumbnailResult> {
        // Check cache first
        if let Some(cached) = self.get_cached(path) {
            debug!("Using cached thumbnail for: {}", path.display());
            return Ok(cached);
        }

        let extension = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_lowercase())
            .unwrap_or_default();

        let result = match extension.as_str() {
            // Video files
            "mp4" | "mkv" | "webm" | "avi" | "mov" | "m4v" | "wmv" | "flv" => {
                self.generate_video_thumbnail(path)?
            }
            // Animated GIF - extract first frame
            "gif" => self.generate_gif_thumbnail(path)?,
            // Image files
            "png" | "jpg" | "jpeg" | "webp" | "bmp" | "tiff" | "tif" => {
                self.generate_image_thumbnail(path)?
            }
            _ => {
                anyhow::bail!("Unsupported file type: {}", extension)
            }
        };

        // Save to cache
        let cache_path = self.cache_path(path);
        if let Err(e) = std::fs::write(&cache_path, &result.data) {
            warn!("Failed to cache thumbnail: {}", e);
        } else {
            debug!("Cached thumbnail at: {}", cache_path.display());
        }

        Ok(result)
    }

    /// Generate thumbnail for image file
    fn generate_image_thumbnail(&self, path: &Path) -> Result<ThumbnailResult> {
        debug!("Generating image thumbnail for: {}", path.display());

        let img = image::open(path).context("Failed to open image")?;
        let (orig_width, orig_height) = img.dimensions();

        // Resize maintaining aspect ratio
        let thumbnail = img.thumbnail(self.width, self.height);
        let (thumb_width, thumb_height) = thumbnail.dimensions();

        // Encode to output format
        let data = encode_image(&thumbnail, self.format)?;

        Ok(ThumbnailResult {
            data,
            width: thumb_width,
            height: thumb_height,
            original_width: orig_width,
            original_height: orig_height,
            format: self.format.extension().to_string(),
            cached: false,
        })
    }

    /// Generate thumbnail for GIF (first frame)
    fn generate_gif_thumbnail(&self, path: &Path) -> Result<ThumbnailResult> {
        debug!("Generating GIF thumbnail for: {}", path.display());

        // Open GIF and get first frame
        let img = image::open(path).context("Failed to open GIF")?;
        let (orig_width, orig_height) = img.dimensions();

        // Resize maintaining aspect ratio
        let thumbnail = img.thumbnail(self.width, self.height);
        let (thumb_width, thumb_height) = thumbnail.dimensions();

        // Encode to output format
        let data = encode_image(&thumbnail, self.format)?;

        Ok(ThumbnailResult {
            data,
            width: thumb_width,
            height: thumb_height,
            original_width: orig_width,
            original_height: orig_height,
            format: self.format.extension().to_string(),
            cached: false,
        })
    }

    /// Generate thumbnail for video file using ffmpeg
    fn generate_video_thumbnail(&self, path: &Path) -> Result<ThumbnailResult> {
        if !self.ffmpeg_available {
            anyhow::bail!("ffmpeg not available for video thumbnail generation");
        }

        debug!("Generating video thumbnail for: {}", path.display());

        // Create temp file for output
        let temp_path = std::env::temp_dir().join(format!(
            "lwe_thumb_{}_{}.png",
            std::process::id(),
            rand_suffix()
        ));

        // Run ffmpeg to extract frame at 1 second (or 10% for longer videos)
        let duration = get_video_duration(path).unwrap_or(10.0);
        let seek_time = if duration > 30.0 {
            (duration * 0.1).min(10.0) // 10% but max 10 seconds
        } else if duration > 3.0 {
            1.0
        } else {
            0.0
        };

        let success = run_ffmpeg_extract(path, &temp_path, seek_time, self.width, self.height);

        if !success {
            // Retry at 0 seconds
            let success = run_ffmpeg_extract(path, &temp_path, 0.0, self.width, self.height);
            if !success {
                anyhow::bail!("Failed to extract video frame with ffmpeg");
            }
        }

        // Read the generated thumbnail
        let png_data = std::fs::read(&temp_path).context("Failed to read generated thumbnail")?;

        // Clean up temp file
        let _ = std::fs::remove_file(&temp_path);

        // Get dimensions from the generated image
        let img =
            image::load_from_memory(&png_data).context("Failed to load generated thumbnail")?;
        let (width, height) = img.dimensions();

        // Convert to output format if needed
        let data = if self.format == ThumbnailFormat::Png {
            png_data
        } else {
            encode_image(&img, self.format)?
        };

        // Get original video dimensions
        let (orig_width, orig_height) = get_video_dimensions(path).unwrap_or((0, 0));

        Ok(ThumbnailResult {
            data,
            width,
            height,
            original_width: orig_width,
            original_height: orig_height,
            format: self.format.extension().to_string(),
            cached: false,
        })
    }

    /// Check if video thumbnail generation is available
    pub fn can_generate_video_thumbnails(&self) -> bool {
        self.ffmpeg_available
    }

    /// Clear thumbnail cache
    pub fn clear_cache(&self) -> Result<usize> {
        let mut count = 0;
        if self.cache_dir.exists() {
            for entry in std::fs::read_dir(&self.cache_dir)?.flatten() {
                let path = entry.path();
                if path.is_file() && std::fs::remove_file(&path).is_ok() {
                    count += 1;
                }
            }
        }
        info!("Cleared {} cached thumbnails", count);
        Ok(count)
    }

    /// Get cache statistics
    pub fn cache_stats(&self) -> CacheStats {
        let mut stats = CacheStats::default();

        if let Ok(entries) = std::fs::read_dir(&self.cache_dir) {
            for entry in entries.flatten() {
                if let Ok(metadata) = entry.metadata() {
                    if metadata.is_file() {
                        stats.count += 1;
                        stats.total_bytes += metadata.len();
                    }
                }
            }
        }

        stats
    }
}

impl Default for ThumbnailGenerator {
    fn default() -> Self {
        Self::new()
    }
}

/// Result of thumbnail generation
#[derive(Debug, Clone)]
pub struct ThumbnailResult {
    /// Image data
    pub data: Vec<u8>,
    /// Thumbnail width
    pub width: u32,
    /// Thumbnail height
    pub height: u32,
    /// Original media width
    pub original_width: u32,
    /// Original media height
    pub original_height: u32,
    /// Image format
    pub format: String,
    /// Whether loaded from cache
    pub cached: bool,
}

/// Cache statistics
#[derive(Debug, Clone, Default)]
pub struct CacheStats {
    pub count: usize,
    pub total_bytes: u64,
}

impl CacheStats {
    pub fn total_mb(&self) -> f64 {
        self.total_bytes as f64 / (1024.0 * 1024.0)
    }
}

/// Request for background thumbnail generation
#[derive(Debug)]
pub struct ThumbnailRequest {
    pub source_path: PathBuf,
    pub wallpaper_id: String,
    pub priority: ThumbnailPriority,
}

/// Priority for thumbnail generation
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ThumbnailPriority {
    Low = 0,
    Normal = 1,
    High = 2,
}

/// Result sent back after background generation
#[derive(Debug)]
pub struct ThumbnailResponse {
    pub wallpaper_id: String,
    pub result: Result<ThumbnailResult, String>,
}

/// Background thumbnail generation service
pub struct ThumbnailService {
    generator: Arc<ThumbnailGenerator>,
    request_tx: tokio::sync::mpsc::Sender<ThumbnailRequest>,
    response_rx: tokio::sync::mpsc::Receiver<ThumbnailResponse>,
    pending_count: Arc<RwLock<usize>>,
}

impl ThumbnailService {
    /// Create a new thumbnail service with specified worker count
    pub fn new(worker_count: usize) -> Self {
        let (request_tx, request_rx) = tokio::sync::mpsc::channel::<ThumbnailRequest>(1000);
        let (response_tx, response_rx) = tokio::sync::mpsc::channel::<ThumbnailResponse>(1000);

        let generator = Arc::new(ThumbnailGenerator::new());
        let pending_count = Arc::new(RwLock::new(0));

        // Spawn worker tasks
        let request_rx = Arc::new(tokio::sync::Mutex::new(request_rx));
        for _ in 0..worker_count {
            let generator = generator.clone();
            let rx = request_rx.clone();
            let tx = response_tx.clone();
            let pending = pending_count.clone();

            tokio::spawn(async move {
                loop {
                    let request = {
                        let mut guard = rx.lock().await;
                        guard.recv().await
                    };

                    match request {
                        Some(req) => {
                            let result = generator.generate(&req.source_path);
                            let response = ThumbnailResponse {
                                wallpaper_id: req.wallpaper_id,
                                result: result.map_err(|e| e.to_string()),
                            };
                            let _ = tx.send(response).await;
                            *pending.write() -= 1;
                        }
                        None => break,
                    }
                }
            });
        }

        Self {
            generator,
            request_tx,
            response_rx,
            pending_count,
        }
    }

    /// Submit a thumbnail request
    pub async fn request(&self, request: ThumbnailRequest) -> Result<()> {
        *self.pending_count.write() += 1;
        self.request_tx
            .send(request)
            .await
            .map_err(|_| anyhow::anyhow!("Thumbnail service channel closed"))
    }

    /// Try to receive a completed thumbnail
    pub fn try_recv(&mut self) -> Option<ThumbnailResponse> {
        self.response_rx.try_recv().ok()
    }

    /// Receive next completed thumbnail (async)
    pub async fn recv(&mut self) -> Option<ThumbnailResponse> {
        self.response_rx.recv().await
    }

    /// Get number of pending requests
    pub fn pending_count(&self) -> usize {
        *self.pending_count.read()
    }

    /// Get reference to generator
    pub fn generator(&self) -> &ThumbnailGenerator {
        &self.generator
    }
}

// ========== Helper Functions ==========

/// Encode image to specified format
fn encode_image(img: &DynamicImage, format: ThumbnailFormat) -> Result<Vec<u8>> {
    let mut buffer = Vec::new();
    img.write_to(&mut Cursor::new(&mut buffer), format.image_format())
        .context("Failed to encode thumbnail")?;
    Ok(buffer)
}

/// Returns true when the original preview is cheap enough for direct card-grid rendering.
pub fn can_render_directly_in_card_grid(path: &Path) -> bool {
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.to_ascii_lowercase())
        .unwrap_or_default();

    if extension == "gif" {
        return false;
    }

    if !matches!(
        extension.as_str(),
        "png" | "jpg" | "jpeg" | "webp" | "bmp" | "tiff" | "tif"
    ) {
        return false;
    }

    match image::image_dimensions(path) {
        Ok((width, height)) => {
            width <= MAX_DIRECT_CARD_PREVIEW_DIMENSION
                && height <= MAX_DIRECT_CARD_PREVIEW_DIMENSION
        }
        Err(_) => false,
    }
}

fn request_card_thumbnail_generation(source_path: PathBuf) {
    let queue = card_thumbnail_queue();
    let Ok(mut queued_paths) = queue.queued_paths.lock() else {
        return;
    };

    if !queued_paths.insert(source_path.clone()) {
        return;
    }
    drop(queued_paths);

    if queue.sender.send(source_path).is_err() {
        warn!("Failed to queue card thumbnail generation");
    }
}

struct CardThumbnailQueue {
    sender: std_mpsc::Sender<PathBuf>,
    queued_paths: Arc<Mutex<HashSet<PathBuf>>>,
}

fn card_thumbnail_queue() -> &'static CardThumbnailQueue {
    static QUEUE: OnceLock<CardThumbnailQueue> = OnceLock::new();
    QUEUE.get_or_init(|| {
        let (sender, receiver) = std_mpsc::channel::<PathBuf>();
        let receiver = Arc::new(Mutex::new(receiver));
        let queued_paths = Arc::new(Mutex::new(HashSet::new()));

        for _ in 0..CARD_THUMBNAIL_WORKER_COUNT {
            let receiver = receiver.clone();
            let queued_paths = queued_paths.clone();

            thread::spawn(move || {
                let generator = ThumbnailGenerator::for_card_grid();
                loop {
                    let source_path = {
                        let Ok(receiver) = receiver.lock() else {
                            break;
                        };
                        receiver.recv()
                    };

                    let Ok(source_path) = source_path else {
                        break;
                    };

                    if let Err(error) = generator.generate_cached_path(&source_path) {
                        debug!(
                            "Failed to generate card thumbnail for {}: {}",
                            source_path.display(),
                            error
                        );
                    }

                    if let Ok(mut queued_paths) = queued_paths.lock() {
                        queued_paths.remove(&source_path);
                    }
                }
            });
        }

        CardThumbnailQueue {
            sender,
            queued_paths,
        }
    })
}

/// Hash a path for cache filename
#[cfg(test)]
fn hash_path(path: &Path) -> String {
    let mut hasher = Sha256::new();
    hasher.update(path.to_string_lossy().as_bytes());
    let result = hasher.finalize();
    hex::encode(&result[..16]) // First 16 bytes = 32 hex chars
}

fn hash_source_identity(path: &Path) -> String {
    let mut hasher = Sha256::new();
    hasher.update(path.to_string_lossy().as_bytes());

    if let Ok(metadata) = std::fs::metadata(path) {
        hasher.update(metadata.len().to_le_bytes());
        if let Ok(modified) = metadata.modified() {
            if let Ok(duration) = modified.duration_since(std::time::UNIX_EPOCH) {
                hasher.update(duration.as_nanos().to_le_bytes());
            }
        }
    }

    let result = hasher.finalize();
    hex::encode(&result[..16])
}

/// Generate random suffix for temp files
fn rand_suffix() -> u32 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    (now.as_nanos() % u32::MAX as u128) as u32
}

/// Check if ffmpeg is available
fn check_ffmpeg() -> bool {
    Command::new("ffmpeg")
        .arg("-version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Run ffmpeg to extract a frame
fn run_ffmpeg_extract(
    input: &Path,
    output: &Path,
    seek_time: f64,
    width: u32,
    height: u32,
) -> bool {
    Command::new("ffmpeg")
        .args([
            "-y",
            "-ss",
            &seek_time.to_string(),
            "-i",
            input.to_str().unwrap_or_default(),
            "-vframes",
            "1",
            "-vf",
            &format!(
                "scale={}:{}:force_original_aspect_ratio=decrease",
                width, height
            ),
            "-f",
            "image2",
            output.to_str().unwrap_or_default(),
        ])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Get video dimensions using ffprobe
pub fn get_video_dimensions(path: &Path) -> Option<(u32, u32)> {
    let output = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=width,height",
            "-of",
            "csv=p=0:s=x",
            path.to_str()?,
        ])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let parts: Vec<&str> = stdout.trim().split('x').collect();

    if parts.len() == 2 {
        let width = parts[0].parse().ok()?;
        let height = parts[1].parse().ok()?;
        Some((width, height))
    } else {
        None
    }
}

/// Get video duration using ffprobe
pub fn get_video_duration(path: &Path) -> Option<f64> {
    let output = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            "-of",
            "default=noprint_wrappers=1:nokey=1",
            path.to_str()?,
        ])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout.trim().parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_thumbnail_generator_creation() {
        let generator = ThumbnailGenerator::new();
        assert_eq!(generator.width, THUMBNAIL_WIDTH);
        assert_eq!(generator.height, THUMBNAIL_HEIGHT);
        assert_eq!(generator.format, ThumbnailFormat::WebP);
        assert!(generator.cache_dir().ends_with("lwe/card-thumbnails"));
    }

    #[test]
    fn test_thumbnail_generator_custom_size() {
        let generator = ThumbnailGenerator::with_size(640, 360);
        assert_eq!(generator.width, 640);
        assert_eq!(generator.height, 360);
    }

    #[test]
    fn test_image_thumbnail() {
        let temp_dir = TempDir::new().unwrap();
        let image_path = temp_dir.path().join("test.png");

        // Create a simple test image
        let img = DynamicImage::new_rgb8(100, 100);
        img.save(&image_path).unwrap();

        let generator = ThumbnailGenerator::with_options(
            THUMBNAIL_WIDTH,
            THUMBNAIL_HEIGHT,
            ThumbnailFormat::Png, // Use PNG for test compatibility
            temp_dir.path().join("cache"),
        );
        let result = generator.generate(&image_path).unwrap();

        assert!(!result.data.is_empty());
        assert!(result.width <= THUMBNAIL_WIDTH);
        assert!(result.height <= THUMBNAIL_HEIGHT);
        assert_eq!(result.original_width, 100);
        assert_eq!(result.original_height, 100);
    }

    #[test]
    fn test_thumbnail_caching() {
        let temp_dir = TempDir::new().unwrap();
        let image_path = temp_dir.path().join("test.png");
        let cache_dir = temp_dir.path().join("cache");

        // Create a simple test image
        let img = DynamicImage::new_rgb8(100, 100);
        img.save(&image_path).unwrap();

        let generator = ThumbnailGenerator::with_options(
            THUMBNAIL_WIDTH,
            THUMBNAIL_HEIGHT,
            ThumbnailFormat::Png,
            cache_dir,
        );

        // First generation should not be cached
        let result1 = generator.generate(&image_path).unwrap();
        assert!(!result1.cached);

        // Second generation should be cached
        let result2 = generator.generate(&image_path).unwrap();
        assert!(result2.cached);
    }

    #[test]
    fn gif_card_cover_uses_static_first_frame_thumbnail() {
        let temp_dir = TempDir::new().unwrap();
        let gif_path = temp_dir.path().join("preview.gif");
        let cache_dir = temp_dir.path().join("cache");

        let img = DynamicImage::new_rgb8(640, 360);
        img.save(&gif_path).unwrap();

        let generator = ThumbnailGenerator::with_options(
            THUMBNAIL_WIDTH,
            THUMBNAIL_HEIGHT,
            ThumbnailFormat::Png,
            cache_dir,
        );

        assert_eq!(generator.card_grid_cover_path(&gif_path), None);

        let cache_path = generator.generate_cached_path(&gif_path).unwrap();
        let card_path = generator.card_grid_cover_path(&gif_path).unwrap();

        assert_eq!(card_path, cache_path);
        assert_eq!(
            card_path.extension().and_then(|ext| ext.to_str()),
            Some("png")
        );
    }

    #[test]
    fn large_image_card_cover_uses_cached_thumbnail_when_available() {
        let temp_dir = TempDir::new().unwrap();
        let image_path = temp_dir.path().join("preview.jpg");
        let cache_dir = temp_dir.path().join("cache");

        let img = DynamicImage::new_rgb8(1600, 900);
        img.save(&image_path).unwrap();

        let generator = ThumbnailGenerator::with_options(
            THUMBNAIL_WIDTH,
            THUMBNAIL_HEIGHT,
            ThumbnailFormat::Png,
            cache_dir,
        );

        assert_eq!(generator.card_grid_cover_path(&image_path), None);

        let cache_path = generator.generate_cached_path(&image_path).unwrap();
        assert_eq!(
            generator.card_grid_cover_path(&image_path),
            Some(cache_path)
        );
    }

    #[test]
    fn small_static_card_cover_can_use_original_preview() {
        let temp_dir = TempDir::new().unwrap();
        let image_path = temp_dir.path().join("preview.jpg");
        let cache_dir = temp_dir.path().join("cache");

        let img = DynamicImage::new_rgb8(256, 144);
        img.save(&image_path).unwrap();

        let generator = ThumbnailGenerator::with_options(
            THUMBNAIL_WIDTH,
            THUMBNAIL_HEIGHT,
            ThumbnailFormat::Png,
            cache_dir,
        );

        assert_eq!(
            generator.card_grid_cover_path(&image_path),
            Some(image_path)
        );
    }

    #[test]
    fn cache_path_changes_when_source_identity_changes() {
        let temp_dir = TempDir::new().unwrap();
        let image_path = temp_dir.path().join("preview.png");
        let cache_dir = temp_dir.path().join("cache");

        let img = DynamicImage::new_rgb8(100, 100);
        img.save(&image_path).unwrap();

        let generator = ThumbnailGenerator::with_options(
            THUMBNAIL_WIDTH,
            THUMBNAIL_HEIGHT,
            ThumbnailFormat::Png,
            cache_dir,
        );
        let first_path = generator.cache_path(&image_path);

        let img = DynamicImage::new_rgb8(101, 101);
        img.save(&image_path).unwrap();
        let second_path = generator.cache_path(&image_path);

        assert_ne!(first_path, second_path);
    }

    #[test]
    fn test_cache_stats() {
        let temp_dir = TempDir::new().unwrap();
        let cache_dir = temp_dir.path().join("cache");

        let generator = ThumbnailGenerator::with_options(
            THUMBNAIL_WIDTH,
            THUMBNAIL_HEIGHT,
            ThumbnailFormat::Png,
            cache_dir,
        );

        // Initial stats should be empty
        let stats = generator.cache_stats();
        assert_eq!(stats.count, 0);
        assert_eq!(stats.total_bytes, 0);
    }

    #[test]
    fn test_thumbnail_format() {
        assert_eq!(ThumbnailFormat::WebP.extension(), "webp");
        assert_eq!(ThumbnailFormat::Png.extension(), "png");
        assert_eq!(ThumbnailFormat::Jpeg.extension(), "jpg");
    }

    #[test]
    fn test_hash_path() {
        let path1 = Path::new("/home/user/wallpaper.mp4");
        let path2 = Path::new("/home/user/wallpaper.mp4");
        let path3 = Path::new("/home/user/other.mp4");

        // Same paths should produce same hash
        assert_eq!(hash_path(path1), hash_path(path2));

        // Different paths should produce different hash
        assert_ne!(hash_path(path1), hash_path(path3));
    }
}
