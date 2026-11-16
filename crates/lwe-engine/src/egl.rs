//! EGL context management for OpenGL rendering on Wayland

use anyhow::{Context, Result, anyhow, ensure};
use khronos_egl as egl;
use std::cell::Cell;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use wayland_client::Proxy;
use wayland_client::protocol::wl_surface::WlSurface;
use wayland_egl as wegl;

/// EGL context manager for OpenGL rendering on Wayland
pub struct EglContext {
    display: Arc<EglDisplay>,
    config: egl::Config,
    context: egl::Context,
    alive: Arc<AtomicBool>,
    foreign_gl: Cell<bool>,
}

/// Keep the initialized display/library alive until its last context/surface
/// has been released, including early-return paths during initialization.
struct EglDisplay {
    handle: egl::Display,
    instance: egl::DynamicInstance<egl::EGL1_4>,
}

// Safety: the EGL display and loaded entry points support calls from multiple
// threads; Arc prevents termination/library unload while an owned object lives.
// Context currentness remains confined to its rendering thread.
unsafe impl Send for EglDisplay {}
unsafe impl Sync for EglDisplay {}

impl Drop for EglDisplay {
    fn drop(&mut self) {
        if let Err(error) = self.instance.terminate(self.handle) {
            tracing::warn!("Failed to terminate EGL display: {error}");
        }
    }
}

/// Per-surface EGL window
pub struct EglWindow {
    display: Arc<EglDisplay>,
    egl_window: wegl::WlEglSurface,
    egl_surface: Cell<Option<egl::Surface>>,
    width: i32,
    height: i32,
}

impl EglContext {
    /// Initialize EGL display and create OpenGL context
    pub fn new(wl_display: *mut std::ffi::c_void) -> Result<Self> {
        // 1. Load EGL library
        let instance = unsafe {
            egl::DynamicInstance::<egl::EGL1_4>::load_required()
                .context("Failed to load EGL library")?
        };

        // 2. Get EGL display from Wayland display
        let display = unsafe {
            instance
                .get_display(wl_display as egl::NativeDisplayType)
                .context("Failed to get EGL display")?
        };

        // 3. Initialize EGL
        let (major, minor) = instance
            .initialize(display)
            .context("Failed to initialize EGL")?;

        tracing::info!("EGL initialized: {}.{}", major, minor);
        let display = Arc::new(EglDisplay {
            handle: display,
            instance,
        });

        // 4. Bind OpenGL API
        display
            .instance
            .bind_api(egl::OPENGL_API)
            .context("Failed to bind OpenGL API")?;

        // 5. Choose EGL config
        let config_attribs = [
            egl::SURFACE_TYPE,
            egl::WINDOW_BIT,
            egl::RENDERABLE_TYPE,
            egl::OPENGL_BIT,
            egl::RED_SIZE,
            8,
            egl::GREEN_SIZE,
            8,
            egl::BLUE_SIZE,
            8,
            egl::ALPHA_SIZE,
            8,
            egl::DEPTH_SIZE,
            24,
            egl::STENCIL_SIZE,
            8,
            egl::NONE,
        ];

        let configs = display
            .instance
            .choose_first_config(display.handle, &config_attribs)
            .context("Failed to choose EGL config")?
            .ok_or_else(|| anyhow!("No suitable EGL config found"))?;

        tracing::debug!("EGL config selected");

        // 6. Create OpenGL context
        let context_attribs = [
            egl::CONTEXT_MAJOR_VERSION,
            3,
            egl::CONTEXT_MINOR_VERSION,
            0,
            egl::CONTEXT_OPENGL_PROFILE_MASK,
            egl::CONTEXT_OPENGL_CORE_PROFILE_BIT,
            egl::NONE,
        ];

        let context = display
            .instance
            .create_context(display.handle, configs, None, &context_attribs)
            .context("Failed to create EGL context")?;

        tracing::info!("EGL context created successfully");

        Ok(Self {
            display,
            config: configs,
            context,
            alive: Arc::new(AtomicBool::new(true)),
            foreign_gl: Cell::new(false),
        })
    }

    pub(crate) fn scene_context_identity(&self) -> (egl::Context, std::sync::Weak<AtomicBool>) {
        (self.context, Arc::downgrade(&self.alive))
    }

    pub(crate) fn mark_current_context_foreign(&self) -> Result<()> {
        ensure!(
            self.display.instance.get_current_context() == Some(self.context)
                && self.display.instance.get_current_display() == Some(self.display.handle),
            "Foreign GL requires its EGL context to be current"
        );
        // Once foreign GL has been given the context, its state cannot be
        // assumed pristine, even if initialization fails or the player drops.
        self.foreign_gl.set(true);
        Ok(())
    }

    pub(crate) fn has_foreign_gl(&self) -> bool {
        self.foreign_gl.get()
    }

    /// Create EGL window surface for a Wayland surface
    pub fn create_window(
        &self,
        wl_surface: &WlSurface,
        width: i32,
        height: i32,
    ) -> Result<EglWindow> {
        // 1. Create Wayland EGL window
        let surface_id = wl_surface.id();

        let egl_window = wegl::WlEglSurface::new(surface_id, width, height)
            .context("Failed to create wl_egl_window")?;

        // 2. Create EGL window surface
        let egl_surface = unsafe {
            self.display
                .instance
                .create_window_surface(
                    self.display.handle,
                    self.config,
                    egl_window.ptr() as egl::NativeWindowType,
                    None,
                )
                .context("Failed to create EGL window surface")?
        };

        tracing::debug!("EGL window surface created: {}x{}", width, height);

        Ok(EglWindow {
            display: Arc::clone(&self.display),
            egl_window,
            egl_surface: Cell::new(Some(egl_surface)),
            width,
            height,
        })
    }

    /// Make this context current for rendering
    pub fn make_current(&self, window: &EglWindow) -> Result<()> {
        ensure!(
            Arc::ptr_eq(&self.display, &window.display),
            "EGL window belongs to a different display"
        );
        let surface = window
            .egl_surface
            .get()
            .context("EGL window was destroyed")?;
        self.display
            .instance
            .make_current(
                self.display.handle,
                Some(surface),
                Some(surface),
                Some(self.context),
            )
            .context("Failed to make EGL context current")?;
        Ok(())
    }

    /// Unbind the current EGL context (make no context current)
    pub fn make_current_none(&self) -> Result<()> {
        self.display
            .instance
            .make_current(self.display.handle, None, None, None)
            .context("Failed to unbind EGL context")?;
        Ok(())
    }

    /// Destroy an EGL window surface (must be called before dropping EglWindow when switching)
    pub fn destroy_surface(&self, window: &EglWindow) -> Result<()> {
        ensure!(
            Arc::ptr_eq(&self.display, &window.display),
            "EGL window belongs to a different display"
        );
        let Some(surface) = window.egl_surface.get() else {
            return Ok(());
        };
        // First unbind the context from this surface
        self.display
            .instance
            .make_current(self.display.handle, None, None, None)
            .context("Failed to unbind EGL context")?;

        // Destroy the EGL surface
        self.display
            .instance
            .destroy_surface(self.display.handle, surface)
            .context("Failed to destroy EGL surface")?;
        window.egl_surface.set(None);

        tracing::debug!("EGL surface destroyed");
        Ok(())
    }

    /// Swap buffers to display rendered frame
    pub fn swap_buffers(&self, window: &EglWindow) -> Result<()> {
        ensure!(
            Arc::ptr_eq(&self.display, &window.display),
            "EGL window belongs to a different display"
        );
        let surface = window
            .egl_surface
            .get()
            .context("EGL window was destroyed")?;
        self.display
            .instance
            .swap_buffers(self.display.handle, surface)
            .context("Failed to swap EGL buffers")?;
        Ok(())
    }

    /// Get OpenGL function address (for loading GL functions)
    pub fn get_proc_address(&self, name: &str) -> *const std::ffi::c_void {
        self.display
            .instance
            .get_proc_address(name)
            .map(|f| f as *const std::ffi::c_void)
            .unwrap_or(std::ptr::null())
    }

    /// Load OpenGL functions using this context
    pub fn load_gl_functions(&self) {
        gl::load_with(|s| self.get_proc_address(s));
    }
}

impl EglWindow {
    /// Get window width
    pub fn width(&self) -> i32 {
        self.width
    }

    /// Get window height
    pub fn height(&self) -> i32 {
        self.height
    }

    /// Resize the EGL window
    pub fn resize(&mut self, width: i32, height: i32) -> Result<()> {
        if self.width == width && self.height == height {
            return Ok(());
        }

        self.egl_window.resize(width, height, 0, 0);
        self.width = width;
        self.height = height;

        tracing::debug!("EGL window resized to {}x{}", width, height);
        Ok(())
    }
}

impl Drop for EglWindow {
    fn drop(&mut self) {
        if let Some(surface) = self.egl_surface.take() {
            // Normal session cleanup has already destroyed this surface. This
            // fallback also covers failed renderer initialization and unwind.
            if let Err(error) =
                self.display
                    .instance
                    .make_current(self.display.handle, None, None, None)
            {
                tracing::warn!("Failed to unbind EGL surface during cleanup: {error}");
            }
            if let Err(error) = self
                .display
                .instance
                .destroy_surface(self.display.handle, surface)
            {
                tracing::warn!("Failed to destroy EGL surface during cleanup: {error}");
            }
        }
    }
}

impl Drop for EglContext {
    fn drop(&mut self) {
        // Invalidate texture domains before the driver may recycle the native
        // context handle for a subsequently created EGL context.
        self.alive.store(false, Ordering::Release);
        if let Err(error) = self.make_current_none() {
            tracing::warn!("Failed to unbind EGL context during cleanup: {error:#}");
        }
        if let Err(error) = self
            .display
            .instance
            .destroy_context(self.display.handle, self.context)
        {
            tracing::warn!("Failed to destroy EGL context: {error}");
        }
    }
}

// Safety: ownership is moved to the engine thread before EGL operations. The
// display owner is reference counted, but the context is deliberately not Sync.
unsafe impl Send for EglContext {}
