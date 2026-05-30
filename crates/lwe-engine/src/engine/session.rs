//! Wallpaper playback session for a single output
//!
//! A session manages the MPV player for rendering video/image wallpaper
//! on a specific Wayland output via the shared EGL context.

use std::path::PathBuf;

use anyhow::Result;
use tracing::{debug, info, warn};
use wayland_client::protocol::wl_surface::WlSurface;

use lwe_core::OutputInfo;

use crate::egl::{EglContext, EglWindow};
use crate::mpv::{MpvPlayer, VideoConfig};

/// Playback state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackState {
    /// Session created but not playing
    Stopped,
    /// Actively playing
    Playing,
    /// Playback paused
    Paused,
}

/// A wallpaper playback session for a single output
pub struct WallpaperSession {
    /// Output information
    output_info: OutputInfo,
    /// Path to current wallpaper
    wallpaper_path: Option<PathBuf>,
    /// Video configuration
    video_config: VideoConfig,
    /// MPV player instance
    player: Option<MpvPlayer>,
    /// EGL window for this surface
    egl_window: Option<EglWindow>,
    /// Current playback state
    state: PlaybackState,
    /// Current volume (0.0 - 1.0)
    volume: f32,
    /// Whether resources are initialized
    initialized: bool,
    /// Whether OpenGL functions are loaded
    gl_loaded: bool,
}

impl WallpaperSession {
    /// Create a new wallpaper session
    pub fn new(
        wallpaper_path: PathBuf,
        output_info: OutputInfo,
        video_config: VideoConfig,
    ) -> Result<Self> {
        info!(
            "Creating WallpaperSession for {} ({}x{})",
            output_info.name, output_info.width, output_info.height
        );

        Ok(Self {
            output_info,
            wallpaper_path: Some(wallpaper_path),
            video_config,
            player: None,
            egl_window: None,
            state: PlaybackState::Stopped,
            volume: 0.0,
            initialized: false,
            gl_loaded: false,
        })
    }

    /// Initialize rendering resources lazily (on first render)
    fn initialize_resources(
        &mut self,
        egl_context: &EglContext,
        wl_surface: &WlSurface,
        width: i32,
        height: i32,
    ) -> Result<()> {
        if self.initialized {
            return Ok(());
        }

        info!(
            "Initializing rendering resources for {} ({}x{})",
            self.output_info.name, width, height
        );
        // Create EGL window for this surface
        let egl_window = egl_context.create_window(wl_surface, width, height)?;
        info!("  ✓ EGL window created");

        // Make context current and load GL functions
        egl_context.make_current(&egl_window)?;

        if !self.gl_loaded {
            egl_context.load_gl_functions();
            self.gl_loaded = true;
            info!("  ✓ OpenGL functions loaded");
        }

        self.egl_window = Some(egl_window);

        // Create MPV player
        let mut config = self.video_config.clone();
        if let Some(ref path) = self.wallpaper_path {
            config.source = path.to_string_lossy().to_string();
        }

        let mut player = MpvPlayer::new(&config, &self.output_info)?;
        info!("  ✓ MPV player created");

        // Initialize MPV render context with EGL
        player.init_render_context(egl_context)?;
        info!("  ✓ MPV render context initialized");

        // Load the wallpaper file
        if let Some(ref path) = self.wallpaper_path {
            player.load_file(path)?;
            info!("  ✓ Loaded wallpaper: {}", path.display());
        }

        self.player = Some(player);
        self.initialized = true;
        self.state = PlaybackState::Playing;

        info!("✅ Session fully initialized for {}", self.output_info.name);

        Ok(())
    }

    /// Render a frame to a Wayland surface
    pub fn render_frame_to_surface(
        &mut self,
        egl_context: &EglContext,
        wl_surface: &WlSurface,
        width: i32,
        height: i32,
        force_frame_poll: bool,
    ) -> Result<bool> {
        // Lazy initialization
        if !self.initialized {
            self.initialize_resources(egl_context, wl_surface, width, height)?;
        }

        if self.state != PlaybackState::Playing {
            return Ok(false);
        }

        // Render MPV frame only if we have a frame ready
        if let Some(ref mut player) = self.player {
            let egl_window = match self.egl_window.as_mut() {
                Some(w) => w,
                None => return Ok(false),
            };
            let resized = egl_window.width() != width || egl_window.height() != height;
            let has_frame = player.consume_frame_update(force_frame_poll);

            if !has_frame && !resized {
                return Ok(false);
            }

            if resized {
                egl_window.resize(width, height)?;
            }

            // Make the context current only when there is actual render work.
            egl_context.make_current(egl_window)?;

            // Clear background
            unsafe {
                gl::ClearColor(0.0, 0.0, 0.0, 1.0);
                gl::Clear(gl::COLOR_BUFFER_BIT);
                gl::Viewport(0, 0, width, height);
            }

            if has_frame {
                // Render the frame
                match player.render(width, height, 0) {
                    Ok(true) => {
                        // Swap buffers only after rendering a valid frame
                        egl_context.swap_buffers(egl_window)?;
                        player.report_swap();
                        return Ok(true);
                    }
                    Ok(false) => return Ok(false),
                    Err(e) => {
                        warn!("MPV render error: {}", e);
                        return Ok(false);
                    }
                }
            }
        }

        Ok(false)
    }

    /// Render a frame (legacy method for compatibility)
    pub fn render_frame(&mut self) -> Result<()> {
        // This method is no longer used - rendering is done via render_frame_to_surface
        Ok(())
    }

    /// Pause playback
    pub fn pause(&mut self) {
        if self.state == PlaybackState::Playing {
            debug!("Pausing session for {}", self.output_info.name);
            if let Some(player) = &mut self.player {
                let _ = player.pause();
            }
            self.state = PlaybackState::Paused;
        }
    }

    /// Resume playback
    pub fn resume(&mut self) {
        if self.state == PlaybackState::Paused {
            debug!("Resuming session for {}", self.output_info.name);
            if let Some(player) = &mut self.player {
                let _ = player.resume();
            }
            self.state = PlaybackState::Playing;
        }
    }

    /// Set volume
    pub fn set_volume(&mut self, volume: f32) {
        self.volume = volume.clamp(0.0, 1.0);
        if let Some(player) = &mut self.player {
            let _ = player.set_volume((self.volume * 100.0) as f64);
        }
    }

    /// Load a new wallpaper without recreating the EGL surface (hot-swap)
    /// This provides seamless wallpaper transitions without flicker
    pub fn load_new_wallpaper(&mut self, path: &std::path::Path) -> Result<()> {
        info!(
            "Hot-swapping wallpaper for {}: {}",
            self.output_info.name,
            path.display()
        );

        self.wallpaper_path = Some(path.to_path_buf());

        if let Some(ref mut player) = self.player {
            player.load_file(path)?;
            info!("  ✓ New wallpaper loaded: {}", path.display());
        } else {
            // Player not initialized yet, will be loaded on first render
            debug!("Player not yet initialized, wallpaper will load on first render");
        }

        Ok(())
    }

    /// Get current wallpaper path
    pub fn wallpaper_path(&self) -> Option<&str> {
        self.wallpaper_path
            .as_ref()
            .map(|p| p.to_str().unwrap_or(""))
    }

    /// Get current playback state
    pub fn state(&self) -> PlaybackState {
        self.state
    }

    /// Get output name
    pub fn output_name(&self) -> &str {
        &self.output_info.name
    }

    /// Cleanup EGL resources before destroying the session
    /// This must be called when switching wallpapers to properly release EGL surfaces
    pub fn cleanup_egl(&mut self, egl_context: &crate::egl::EglContext) {
        // First stop MPV to release OpenGL resources
        if let Some(player) = self.player.take() {
            drop(player);
        }

        // Destroy EGL surface properly
        if let Some(ref egl_window) = self.egl_window {
            if let Err(e) = egl_context.destroy_surface(egl_window) {
                warn!("Failed to destroy EGL surface: {}", e);
            }
        }
        self.egl_window = None;
        self.initialized = false;
    }
}

impl Drop for WallpaperSession {
    fn drop(&mut self) {
        debug!("Dropping WallpaperSession for {}", self.output_info.name);

        // Stop playback first
        if let Some(player) = self.player.take() {
            drop(player);
        }

        // Release EGL window
        if let Some(egl_window) = self.egl_window.take() {
            drop(egl_window);
        }
    }
}
