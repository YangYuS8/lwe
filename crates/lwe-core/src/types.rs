//! Video source types
//!
//! Defines the various sources for wallpapers/videos that wayvid can display.

use serde::{Deserialize, Serialize};

/// Video source specification
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(tag = "type")]
pub enum VideoSource {
    /// Single video file
    File { path: String },

    /// Directory containing videos (playlist)
    Directory { path: String },

    /// HTTP/HTTPS URL stream
    Url { url: String },

    /// RTSP stream
    Rtsp { url: String },

    /// Pipe input (stdin or named pipe)
    Pipe {
        #[serde(default)]
        path: String, // Empty string means stdin
    },

    /// GIF or image sequence
    ImageSequence {
        path: String,
        #[serde(default = "default_fps")]
        fps: f64,
    },

    /// Wallpaper Engine video project
    #[serde(rename = "WeProject")]
    WeProject { path: String },

    /// Wallpaper Engine scene project (JSON-based with layers)
    #[serde(rename = "WeScene")]
    WeScene { path: String },
}

// Manual Eq implementation for VideoSource (treating f64 as bits)
impl Eq for VideoSource {}

// Manual Hash implementation for VideoSource
impl std::hash::Hash for VideoSource {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        match self {
            VideoSource::File { path } => {
                0u8.hash(state);
                path.hash(state);
            }
            VideoSource::Directory { path } => {
                1u8.hash(state);
                path.hash(state);
            }
            VideoSource::Url { url } => {
                2u8.hash(state);
                url.hash(state);
            }
            VideoSource::Rtsp { url } => {
                3u8.hash(state);
                url.hash(state);
            }
            VideoSource::Pipe { path } => {
                4u8.hash(state);
                path.hash(state);
            }
            VideoSource::ImageSequence { path, fps } => {
                5u8.hash(state);
                path.hash(state);
                // Hash fps as bits to avoid f64 comparison issues
                fps.to_bits().hash(state);
            }
            VideoSource::WeProject { path } => {
                6u8.hash(state);
                path.hash(state);
            }
            VideoSource::WeScene { path } => {
                7u8.hash(state);
                path.hash(state);
            }
        }
    }
}

fn default_fps() -> f64 {
    30.0
}

impl VideoSource {
    /// Get the source path/URL as string for MPV
    pub fn get_mpv_path(&self) -> String {
        match self {
            VideoSource::File { path } => expand_tilde(path),
            VideoSource::Directory { path } => expand_tilde(path),
            VideoSource::Url { url } => url.clone(),
            VideoSource::Rtsp { url } => url.clone(),
            VideoSource::Pipe { path } => {
                if path.is_empty() {
                    "fd://0".to_string() // stdin
                } else {
                    expand_tilde(path)
                }
            }
            VideoSource::ImageSequence { path, .. } => expand_tilde(path),
            VideoSource::WeProject { path } => expand_tilde(path),
            VideoSource::WeScene { path } => expand_tilde(path),
        }
    }

    /// Get the source as a display string (for UI/logging)
    pub fn get_source_string(&self) -> &str {
        match self {
            VideoSource::File { path } => path,
            VideoSource::Directory { path } => path,
            VideoSource::Url { url } => url,
            VideoSource::Rtsp { url } => url,
            VideoSource::Pipe { path } => {
                if path.is_empty() {
                    "stdin"
                } else {
                    path
                }
            }
            VideoSource::ImageSequence { path, .. } => path,
            VideoSource::WeProject { path } => path,
            VideoSource::WeScene { path } => path,
        }
    }

    /// Check if this is a streaming source (needs special handling)
    #[inline]
    pub fn is_streaming(&self) -> bool {
        matches!(
            self,
            VideoSource::Url { .. } | VideoSource::Rtsp { .. } | VideoSource::Pipe { .. }
        )
    }

    /// Check if this is an image sequence (needs loop handling)
    #[inline]
    pub fn is_image_sequence(&self) -> bool {
        matches!(self, VideoSource::ImageSequence { .. })
    }

    /// Check if this is a scene wallpaper (needs scene renderer)
    #[inline]
    pub fn is_scene(&self) -> bool {
        matches!(self, VideoSource::WeScene { .. })
    }
}

/// Expand ~ to home directory
fn expand_tilde(path: &str) -> String {
    if path.starts_with('~')
        && let Some(home) = dirs::home_dir()
    {
        return path.replacen('~', &home.to_string_lossy(), 1);
    }
    path.to_string()
}

/// Layout/scaling mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default)]
pub enum LayoutMode {
    /// Scale and crop to fill entire output (maintains aspect ratio)
    #[default]
    Fill,

    /// Scale to fit inside output (maintains aspect ratio, may have letterbox)
    Contain,

    /// Stretch to fill output (ignores aspect ratio)
    Stretch,

    /// Like Fill (alias for compatibility)
    Cover,

    /// Center without scaling
    Centre,
}

/// HDR capabilities of an output
#[derive(Debug, Clone)]
pub struct OutputHdrCapabilities {
    /// Whether the output supports HDR
    pub hdr_supported: bool,

    /// Maximum luminance in nits (if available)
    pub max_luminance: Option<f64>,

    /// Minimum luminance in nits (if available)
    pub min_luminance: Option<f64>,

    /// Supported transfer functions (EOTFs)
    pub supported_eotf: Vec<crate::hdr::TransferFunction>,
}

impl Default for OutputHdrCapabilities {
    fn default() -> Self {
        Self {
            hdr_supported: false,
            max_luminance: Some(203.0), // Typical SDR peak brightness
            min_luminance: Some(0.0),
            supported_eotf: vec![crate::hdr::TransferFunction::Srgb],
        }
    }
}

/// Output information
#[derive(Debug, Clone)]
pub struct OutputInfo {
    /// Output name (e.g., "HDMI-A-1", "eDP-1")
    pub name: String,

    /// Physical width in pixels
    pub width: i32,

    /// Physical height in pixels
    pub height: i32,

    /// Scale factor (1, 2, or fractional like 1.5)
    pub scale: f64,

    /// Logical position (x, y)
    pub position: (i32, i32),

    /// Whether output is currently active (for future hot-plug support)
    pub active: bool,

    /// HDR capabilities of this output
    pub hdr_capabilities: OutputHdrCapabilities,
}

impl OutputInfo {
    /// Get logical dimensions (physical / scale)
    pub fn logical_size(&self) -> (f64, f64) {
        (
            self.width as f64 / self.scale,
            self.height as f64 / self.scale,
        )
    }
}

/// Playback state
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlaybackState {
    Playing,
    Paused,
    Stopped,
    Error,
}

/// Hardware decode preference
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum HwdecMode {
    /// Try hardware decode, fallback to software
    #[default]
    Auto,

    /// Force hardware decode only
    Force,

    /// Disable hardware decode
    No,
}

impl From<bool> for HwdecMode {
    fn from(enabled: bool) -> Self {
        if enabled {
            HwdecMode::Auto
        } else {
            HwdecMode::No
        }
    }
}

/// Render backend selection
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RenderBackend {
    /// Auto-detect best backend (prefer Vulkan if available)
    #[default]
    Auto,

    /// Force OpenGL (EGL) backend
    #[serde(rename = "opengl")]
    OpenGL,

    /// Force Vulkan backend
    Vulkan,
}

impl RenderBackend {
    /// Check if Vulkan backend is requested (explicit or auto)
    pub fn wants_vulkan(&self) -> bool {
        matches!(self, RenderBackend::Auto | RenderBackend::Vulkan)
    }

    /// Check if OpenGL backend is requested (explicit or auto)
    pub fn wants_opengl(&self) -> bool {
        matches!(self, RenderBackend::Auto | RenderBackend::OpenGL)
    }

    /// Get display name for UI
    pub fn display_name(&self) -> &'static str {
        match self {
            RenderBackend::Auto => "Auto",
            RenderBackend::OpenGL => "OpenGL",
            RenderBackend::Vulkan => "Vulkan",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_video_source_serialization() {
        let source = VideoSource::File {
            path: "/path/to/video.mp4".to_string(),
        };
        let yaml = serde_yaml::to_string(&source).unwrap();
        assert!(yaml.contains("type: File"));
    }

    #[test]
    fn test_expand_tilde() {
        let expanded = expand_tilde("~/videos/test.mp4");
        assert!(!expanded.starts_with('~'));
    }

    #[test]
    fn test_layout_mode_default() {
        let mode: LayoutMode = Default::default();
        assert_eq!(mode, LayoutMode::Fill);
    }
}
