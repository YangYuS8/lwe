//! lwe-engine: Video rendering engine for LWE
//!
//! This crate provides the core rendering functionality:
//! - Wayland layer-shell surface management
//! - MPV video playback integration  
//! - EGL/OpenGL rendering
//! - Vulkan rendering (optional)
//!
//! # Architecture
//!
//! ```text
//! PlaybackEngine (main API)
//!     ├── WallpaperSession (per-output)
//!     │   ├── LayerSurface (wlr-layer-shell)
//!     │   ├── EglContext (OpenGL rendering)
//!     │   └── MpvPlayer (video decoding)
//!     └── OutputManager (output tracking)
//! ```

pub mod egl;
pub mod engine;
pub mod frame_timing;
pub mod mpv;
pub mod wayland;

// Re-exports - Engine API
pub use engine::{
    EngineCommand, EngineConfig, EngineEvent, EngineHandle, EngineStatus, WallpaperSession,
    spawn_engine,
};

// Re-export calloop Sender for IPC integration
pub use calloop::channel::Sender as CommandSender;

// Re-exports - Low-level components
pub use egl::{EglContext, EglWindow};
pub use frame_timing::FrameTiming;
pub use mpv::{MpvPlayer, VideoConfig};
pub use wayland::{LayerSurface, OutputManager};

// Re-exports from lwe-core
pub use lwe_core::{
    HdrMetadata, HdrMode, HwdecMode, LayoutMode, LayoutTransform, OutputInfo, RenderBackend,
    ToneMappingConfig, calculate_layout,
};
