//! lwe-core: Core types and configuration for LWE
//!
//! This crate provides the foundational types used across all LWE components:
//! - `VideoSource`: Defines various video/wallpaper source types
//! - `WallpaperItem`: Wallpaper metadata for the library
//! - `AppSettings`: Application settings (GUI managed)
//! - `Config`: Legacy CLI configuration format
//! - Layout, HDR, and rendering types
//! - IPC protocol types for daemon communication

pub mod config;
pub mod hdr;
pub mod ipc;
pub mod layout;
pub mod library;
pub mod power;
pub mod settings;
pub mod types;

// Re-exports for convenience
pub use config::{Config, EffectiveConfig, OutputConfig, PowerConfig};
pub use hdr::{
    ColorSpace, HdrMetadata, HdrMode, ToneMappingAlgorithm, ToneMappingConfig, TransferFunction,
};
pub use ipc::{
    IpcRequest, IpcResponse, LibraryFilter, LibraryItem, OutputInfo as IpcOutputInfo, OutputStatus,
    default_socket_path,
};
pub use layout::{LayoutTransform, calculate_layout};
pub use library::{SourceType, WallpaperItem, WallpaperMetadata, WallpaperType};
pub use settings::AppSettings;
pub use types::{
    HwdecMode, LayoutMode, OutputHdrCapabilities, OutputInfo, RenderBackend, VideoSource,
};
