//! Layer shell surface management
//!
//! Handles wlr-layer-shell protocol for creating wallpaper surfaces.
//! Uses smithay-client-toolkit (sctk) for higher-level abstractions.

use anyhow::{Context, Result};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    delegate_compositor, delegate_layer, delegate_output, delegate_registry,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    shell::{
        WaylandSurface,
        wlr_layer::{
            Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler,
            LayerSurface as SctkLayerSurface, LayerSurfaceConfigure,
        },
    },
};
use wayland_client::{
    Connection, QueueHandle,
    globals::registry_queue_init,
    protocol::{wl_output::WlOutput, wl_surface::WlSurface},
};

/// Layer surface configuration
#[derive(Debug, Clone)]
pub struct LayerSurfaceConfig {
    /// Target output name (None = all outputs)
    pub output: Option<String>,
    /// Surface width (0 = match output)
    pub width: u32,
    /// Surface height (0 = match output)
    pub height: u32,
    /// Exclusive zone (-1 = ignore, 0 = auto, >0 = pixels)
    pub exclusive_zone: i32,
    /// Margin from edges
    pub margin: LayerMargin,
    /// Keyboard interactivity
    pub keyboard_interactive: bool,
}

impl Default for LayerSurfaceConfig {
    fn default() -> Self {
        Self {
            output: None,
            width: 0,
            height: 0,
            exclusive_zone: -1,
            margin: LayerMargin::default(),
            keyboard_interactive: false,
        }
    }
}

/// Surface margins
#[derive(Debug, Clone, Default)]
pub struct LayerMargin {
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
    pub left: i32,
}

/// Layer surface state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceState {
    /// Surface is being created
    Pending,
    /// Surface is configured and ready
    Configured,
    /// Surface was closed
    Closed,
}

/// Manages a single layer shell surface for wallpaper rendering
pub struct LayerSurface {
    /// SCTK layer surface
    layer: SctkLayerSurface,
    /// Current state
    state: SurfaceState,
    /// Configured width
    width: u32,
    /// Configured height
    height: u32,
    /// Surface configuration
    #[allow(dead_code)]
    config: LayerSurfaceConfig,
}

impl LayerSurface {
    /// Create a new layer surface for wallpaper rendering
    pub fn new(
        layer_shell: &LayerShell,
        qh: &QueueHandle<WallpaperState>,
        compositor: &CompositorState,
        output: Option<&WlOutput>,
        config: LayerSurfaceConfig,
    ) -> Result<Self> {
        // Create the underlying wl_surface
        let surface = compositor.create_surface(qh);

        // Create layer surface on the background layer (for wallpaper)
        let layer = layer_shell.create_layer_surface(
            qh,
            surface,
            Layer::Background,
            Some("wayvid-wallpaper"),
            output,
        );

        // Configure the layer surface for wallpaper use
        layer.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
        layer.set_exclusive_zone(config.exclusive_zone);
        layer.set_margin(
            config.margin.top,
            config.margin.right,
            config.margin.bottom,
            config.margin.left,
        );

        let interactivity = if config.keyboard_interactive {
            KeyboardInteractivity::Exclusive
        } else {
            KeyboardInteractivity::None
        };
        layer.set_keyboard_interactivity(interactivity);

        if config.width > 0 && config.height > 0 {
            layer.set_size(config.width, config.height);
        }

        layer.commit();

        tracing::debug!("Created layer surface for wallpaper");

        Ok(Self {
            layer,
            state: SurfaceState::Pending,
            width: config.width,
            height: config.height,
            config,
        })
    }

    /// Get current surface state
    pub fn state(&self) -> SurfaceState {
        self.state
    }

    /// Get configured dimensions
    pub fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// Get the underlying Wayland surface
    pub fn wl_surface(&self) -> &WlSurface {
        self.layer.wl_surface()
    }

    /// Handle a configure event
    pub fn handle_configure(&mut self, configure: LayerSurfaceConfigure) {
        let (width, height) = configure.new_size;
        if width > 0 {
            self.width = width;
        }
        if height > 0 {
            self.height = height;
        }
        self.state = SurfaceState::Configured;
        tracing::debug!("Layer surface configured: {}x{}", self.width, self.height);
    }

    /// Mark surface as closed
    pub fn handle_closed(&mut self) {
        self.state = SurfaceState::Closed;
        tracing::debug!("Layer surface closed");
    }

    /// Commit pending changes
    pub fn commit(&self) {
        self.layer.commit();
    }

    /// Attach a buffer to the surface
    pub fn attach(
        &self,
        buffer: Option<&wayland_client::protocol::wl_buffer::WlBuffer>,
        x: i32,
        y: i32,
    ) {
        self.layer.wl_surface().attach(buffer, x, y);
    }

    /// Damage the entire surface for redraw
    pub fn damage_all(&self) {
        self.layer
            .wl_surface()
            .damage_buffer(0, 0, self.width as i32, self.height as i32);
    }
}

/// State for managing wallpaper surfaces
pub struct WallpaperState {
    pub registry_state: RegistryState,
    pub output_state: OutputState,
    pub compositor_state: CompositorState,
    pub layer_shell: LayerShell,
    pub surfaces: Vec<LayerSurface>,
    pub configured: bool,
}

impl WallpaperState {
    /// Create a new wallpaper state from a Wayland connection
    pub fn new(connection: &Connection) -> Result<(Self, wayland_client::EventQueue<Self>)> {
        let (globals, event_queue) =
            registry_queue_init::<Self>(connection).context("Failed to initialize registry")?;
        let qh = event_queue.handle();

        let registry_state = RegistryState::new(&globals);
        let output_state = OutputState::new(&globals, &qh);
        let compositor_state =
            CompositorState::bind(&globals, &qh).context("Failed to bind compositor")?;
        let layer_shell = LayerShell::bind(&globals, &qh)
            .context("Failed to bind layer shell - is wlr-layer-shell supported?")?;

        let state = Self {
            registry_state,
            output_state,
            compositor_state,
            layer_shell,
            surfaces: Vec::new(),
            configured: false,
        };

        Ok((state, event_queue))
    }

    /// Create a wallpaper surface for a specific output
    pub fn create_surface(
        &mut self,
        qh: &QueueHandle<Self>,
        output: Option<&WlOutput>,
        config: LayerSurfaceConfig,
    ) -> Result<usize> {
        let surface = LayerSurface::new(
            &self.layer_shell,
            qh,
            &self.compositor_state,
            output,
            config,
        )?;
        let index = self.surfaces.len();
        self.surfaces.push(surface);
        Ok(index)
    }
}

// Implement required traits for smithay-client-toolkit

impl CompositorHandler for WallpaperState {
    fn scale_factor_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &WlSurface,
        _new_factor: i32,
    ) {
    }

    fn transform_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &WlSurface,
        _new_transform: wayland_client::protocol::wl_output::Transform,
    ) {
    }

    fn frame(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &WlSurface,
        _time: u32,
    ) {
    }

    fn surface_enter(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &WlSurface,
        _output: &WlOutput,
    ) {
    }

    fn surface_leave(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &WlSurface,
        _output: &WlOutput,
    ) {
    }
}

impl OutputHandler for WallpaperState {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }

    fn new_output(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _output: WlOutput) {
        tracing::info!("New output detected");
    }

    fn update_output(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _output: WlOutput) {}

    fn output_destroyed(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _output: WlOutput) {
        tracing::info!("Output removed");
    }
}

impl LayerShellHandler for WallpaperState {
    fn closed(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, layer: &SctkLayerSurface) {
        for surface in &mut self.surfaces {
            if surface.layer.wl_surface() == layer.wl_surface() {
                surface.handle_closed();
                break;
            }
        }
    }

    fn configure(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        layer: &SctkLayerSurface,
        configure: LayerSurfaceConfigure,
        _serial: u32,
    ) {
        for surface in &mut self.surfaces {
            if surface.layer.wl_surface() == layer.wl_surface() {
                surface.handle_configure(configure);
                self.configured = true;
                break;
            }
        }
    }
}

impl ProvidesRegistryState for WallpaperState {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }

    registry_handlers![OutputState];
}

delegate_compositor!(WallpaperState);
delegate_output!(WallpaperState);
delegate_layer!(WallpaperState);
delegate_registry!(WallpaperState);

/// Builder for layer surfaces
pub struct LayerSurfaceBuilder {
    config: LayerSurfaceConfig,
}

impl LayerSurfaceBuilder {
    pub fn new() -> Self {
        Self {
            config: LayerSurfaceConfig::default(),
        }
    }

    pub fn output(mut self, name: impl Into<String>) -> Self {
        self.config.output = Some(name.into());
        self
    }

    pub fn size(mut self, width: u32, height: u32) -> Self {
        self.config.width = width;
        self.config.height = height;
        self
    }

    pub fn exclusive_zone(mut self, zone: i32) -> Self {
        self.config.exclusive_zone = zone;
        self
    }

    pub fn margin(mut self, top: i32, right: i32, bottom: i32, left: i32) -> Self {
        self.config.margin = LayerMargin {
            top,
            right,
            bottom,
            left,
        };
        self
    }

    pub fn keyboard_interactive(mut self, interactive: bool) -> Self {
        self.config.keyboard_interactive = interactive;
        self
    }

    pub fn build(self) -> LayerSurfaceConfig {
        self.config
    }
}

impl Default for LayerSurfaceBuilder {
    fn default() -> Self {
        Self::new()
    }
}
