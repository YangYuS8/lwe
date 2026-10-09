//! Playback engine for integrated wallpaper rendering
//!
//! This module provides the high-level API for managing wallpaper playback
//! within the GUI process. The engine runs in a dedicated thread and
//! communicates with the GUI via channels.

mod command;
mod session;

pub use command::{EngineCommand, EngineConfig, EngineEvent, EngineStatus};
pub use session::WallpaperSession;

use std::collections::{BTreeSet, HashMap};
use std::ffi::CString;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver as StdReceiver, Sender as StdSender};
use std::thread::{self, JoinHandle};

use anyhow::{Context, Result};
use calloop::EventLoop;
use calloop::channel::{Sender, channel};
use calloop_wayland_source::WaylandSource;
use tracing::{debug, error, info, warn};
use wayland_client::protocol::wl_callback::{self, WlCallback};
use wayland_client::protocol::wl_compositor::{self, WlCompositor};
use wayland_client::protocol::wl_output::{self, WlOutput};
use wayland_client::protocol::wl_registry::{self, WlRegistry};
use wayland_client::protocol::wl_surface::{self, WlSurface};
use wayland_client::{Connection, Dispatch, QueueHandle};
use wayland_protocols_wlr::layer_shell::v1::client::{
    zwlr_layer_shell_v1::{self, ZwlrLayerShellV1},
    zwlr_layer_surface_v1::{self, Anchor, ZwlrLayerSurfaceV1},
};

use crate::egl::EglContext;
use crate::wayland::OutputManager;

/// Handle for controlling a running PlaybackEngine from another thread
pub struct EngineHandle {
    /// Command sender (calloop channel, cloneable)
    commands_tx: Sender<EngineCommand>,
    /// Shutdown flag
    shutdown: Arc<AtomicBool>,
    /// Thread handle
    thread: Option<JoinHandle<()>>,
}

impl EngineHandle {
    /// Send a command to the engine
    pub fn send(&self, cmd: EngineCommand) -> Result<()> {
        self.commands_tx
            .send(cmd)
            .map_err(|e| anyhow::anyhow!("Failed to send command: {}", e))
    }

    /// Get a clone of the command sender for external use (e.g., IPC server)
    pub fn command_sender(&self) -> Sender<EngineCommand> {
        self.commands_tx.clone()
    }

    /// Get the shutdown flag for cooperative shutdown
    pub fn shutdown_flag(&self) -> Arc<AtomicBool> {
        self.shutdown.clone()
    }

    /// Request the engine to shut down
    pub fn request_shutdown(&self) {
        self.shutdown.store(true, Ordering::SeqCst);
    }

    /// Wait for the engine thread to finish
    pub fn join(mut self) -> Result<()> {
        if let Some(handle) = self.thread.take() {
            handle
                .join()
                .map_err(|_| anyhow::anyhow!("Engine thread panicked"))?;
        }
        Ok(())
    }

    /// Check if engine is still running
    pub fn is_running(&self) -> bool {
        self.thread
            .as_ref()
            .map(|h| !h.is_finished())
            .unwrap_or(false)
    }
}

impl Drop for EngineHandle {
    fn drop(&mut self) {
        // Signal shutdown
        self.shutdown.store(true, Ordering::SeqCst);
        // Don't join here to avoid blocking in destructor
    }
}

/// Spawn a new PlaybackEngine in a dedicated thread
///
/// This is the recommended way to start the engine, as it handles
/// thread-safety correctly (calloop EventLoop cannot be moved across threads).
///
/// # Arguments
/// * `config` - Engine configuration
///
/// # Returns
/// A tuple of (EngineHandle, Receiver<EngineEvent>)
pub fn spawn_engine(config: EngineConfig) -> Result<(EngineHandle, StdReceiver<EngineEvent>)> {
    // Create event channel (std mpsc, GUI side)
    let (events_tx, events_rx) = mpsc::channel::<EngineEvent>();

    // Create command channel (calloop, for engine thread)
    let (commands_tx, commands_rx) = channel::<EngineCommand>();

    let shutdown = Arc::new(AtomicBool::new(false));
    let shutdown_clone = shutdown.clone();

    let thread = thread::Builder::new()
        .name("lwe-engine".to_string())
        .spawn(move || {
            if let Err(e) = run_engine_thread(config, events_tx, commands_rx, shutdown_clone) {
                error!("Engine thread error: {}", e);
            }
        })
        .context("Failed to spawn engine thread")?;

    let handle = EngineHandle {
        commands_tx,
        shutdown,
        thread: Some(thread),
    };

    Ok((handle, events_rx))
}

/// Internal: Run engine in the current thread
fn run_engine_thread(
    config: EngineConfig,
    events_tx: StdSender<EngineEvent>,
    commands_rx: calloop::channel::Channel<EngineCommand>,
    shutdown: Arc<AtomicBool>,
) -> Result<()> {
    info!("Starting PlaybackEngine in thread");

    // Connect to Wayland compositor
    let connection =
        Connection::connect_to_env().context("Failed to connect to Wayland compositor")?;

    debug!("Connected to Wayland compositor");

    // Get display pointer for EGL
    let display_ptr = connection.backend().display_ptr() as *mut std::ffi::c_void;

    // Create engine state
    let mut state = EngineState {
        outputs: OutputManager::new(),
        sessions: HashMap::new(),
        events_tx: events_tx.clone(),
        config,
        running: true,
        pending_outputs: HashMap::new(),
        compositor: None,
        layer_shell: None,
        egl_context: None,
        layer_surfaces: HashMap::new(),
        queue_handle: None,
        on_battery: check_battery_status(),
        power_paused: false,
        last_battery_check: std::time::Instant::now(),
    };

    // Create event loop
    let mut event_loop: EventLoop<'static, EngineState> =
        EventLoop::try_new().context("Failed to create event loop")?;

    // Register command channel
    event_loop
        .handle()
        .insert_source(commands_rx, |event, _, state| {
            if let calloop::channel::Event::Msg(cmd) = event {
                handle_command(cmd, state);
            }
        })
        .map_err(|e| anyhow::anyhow!("Failed to register command channel: {:?}", e))?;

    // Create Wayland event queue and register with calloop
    let event_queue = connection.new_event_queue::<EngineState>();
    let qh = event_queue.handle();

    // Store queue handle in state for later use
    state.queue_handle = Some(qh.clone());

    // Get the display and request the registry to enumerate globals
    let display = connection.display();
    let _registry = display.get_registry(&qh, ());

    WaylandSource::new(connection.clone(), event_queue)
        .insert(event_loop.handle())
        .map_err(|e| anyhow::anyhow!("Failed to insert Wayland source: {:?}", e))?;

    // Do initial roundtrip to get outputs and globals
    debug!("Performing initial Wayland roundtrip to enumerate outputs");

    // Initialize EGL context after globals are bound
    // We need a few roundtrips first to get compositor and layer_shell
    for _ in 0..3 {
        event_loop
            .dispatch(std::time::Duration::from_millis(50), &mut state)
            .context("Event loop dispatch failed")?;
    }

    // Now initialize EGL if we have compositor and layer_shell
    if state.compositor.is_some() && state.layer_shell.is_some() {
        info!("Initializing EGL context...");
        match EglContext::new(display_ptr) {
            Ok(egl_ctx) => {
                info!("  ✓ EGL context initialized");
                state.egl_context = Some(egl_ctx);
            }
            Err(e) => {
                let reason = format!("Failed to initialize EGL: {e}");
                warn!("  ✗ {}", reason);
                warn!("    Wallpaper rendering will not work");
                let _ = events_tx.send(EngineEvent::Error(reason));
                state.running = false;
                let _ = events_tx.send(EngineEvent::Stopped);
                return Ok(());
            }
        }
    } else {
        let mut missing = Vec::new();
        if state.compositor.is_none() {
            missing.push("wl_compositor");
            warn!("  - wl_compositor not bound");
        }
        if state.layer_shell.is_none() {
            missing.push("zwlr_layer_shell_v1");
            warn!("  - zwlr_layer_shell_v1 not bound");
        }
        let reason = format!(
            "Missing required Wayland globals for wallpaper rendering: {}",
            missing.join(", ")
        );
        warn!("{}", reason);
        let _ = events_tx.send(EngineEvent::Error(reason));
        state.running = false;
        let _ = events_tx.send(EngineEvent::Stopped);
        return Ok(());
    }

    // Send started event after render prerequisites have been checked.
    let _ = events_tx.send(EngineEvent::Started);

    // Main event loop with power management
    let battery_check_interval = std::time::Duration::from_secs(10);

    while !shutdown.load(Ordering::Relaxed) {
        // Periodically check battery status
        if state.last_battery_check.elapsed() >= battery_check_interval {
            state.on_battery = check_battery_status();
            state.last_battery_check = std::time::Instant::now();

            // Handle pause on battery
            if state.config.pause_on_battery {
                if state.on_battery && !state.power_paused {
                    info!("On battery power, pausing playback for power saving");
                    for session in state.sessions.values_mut() {
                        session.pause();
                    }
                    state.power_paused = true;
                } else if !state.on_battery && state.power_paused {
                    info!("On AC power, resuming playback");
                    for session in state.sessions.values_mut() {
                        session.resume();
                    }
                    state.power_paused = false;
                }
            }
        }

        // Calculate frame duration based on power state
        let frame_duration = if state.power_paused {
            // When paused, use longer sleep to save power
            std::time::Duration::from_millis(100)
        } else {
            state
                .config
                .fps_limit
                .map(|fps| std::time::Duration::from_micros(1_000_000 / fps as u64))
                .unwrap_or(std::time::Duration::from_millis(16)) // Default 60fps
        };

        event_loop
            .dispatch(frame_duration, &mut state)
            .context("Event loop dispatch failed")?;

        // Expire requests even before configure or while playback is power-paused.
        expire_pending_applies(&mut state, std::time::Instant::now());

        // Render frames for configured layer surfaces (skip if power paused)
        if !state.power_paused {
            render_all_surfaces(&mut state);
        }
    }

    info!("PlaybackEngine shutting down");

    for output in outputs_with_resources(&state) {
        if let Err(error) = release_output_resources(&mut state, &output) {
            warn!(
                "Failed to release output {} during shutdown: {:#}",
                output, error
            );
        }
    }

    state.running = false;
    let _ = events_tx.send(EngineEvent::Stopped);

    Ok(())
}

/// Render all configured surfaces
fn render_all_surfaces(state: &mut EngineState) {
    // Get EGL context reference
    let egl_context = match state.egl_context.as_ref() {
        Some(ctx) => ctx,
        None => return, // No EGL context, can't render
    };

    // Collect output names to avoid borrow issues
    let outputs: Vec<String> = state
        .layer_surfaces
        .iter()
        .filter(|(_, info)| info.configured && (info.frame_pending || info.apply_request.pending))
        .map(|(name, _)| name.clone())
        .collect();
    let mut failed_outputs = Vec::new();

    for output_name in outputs {
        // Get layer surface info
        let surface_info = match state.layer_surfaces.get_mut(&output_name) {
            Some(info) => info,
            None => continue,
        };

        // Clear frame pending flag
        surface_info.frame_pending = false;

        // Get session for this output
        let session = match state.sessions.get_mut(&output_name) {
            Some(s) => s,
            None => continue,
        };

        let is_waiting_for_first_frame = surface_info.apply_request.pending;

        // Render frame
        match session.render_frame_to_surface(
            egl_context,
            &surface_info.wl_surface,
            surface_info.width as i32,
            surface_info.height as i32,
            is_waiting_for_first_frame,
        ) {
            Ok(true) => {
                if let Some(event) = surface_info
                    .apply_request
                    .complete(&output_name, std::time::Instant::now())
                {
                    // A blocking EGL swap may have passed the request deadline.
                    let expired = matches!(&event, EngineEvent::ApplyFailed { .. });
                    let _ = state.events_tx.send(event);
                    if expired {
                        failed_outputs.push(output_name);
                        continue;
                    }
                }
            }
            Ok(false) => {}
            Err(e) => {
                warn!("Frame render error for {}: {:#}", output_name, e);
                if surface_info.apply_request.pending {
                    surface_info.apply_request.pending = false;
                    let _ = state.events_tx.send(EngineEvent::ApplyFailed {
                        request_id: surface_info.apply_request.request_id,
                        output: output_name.clone(),
                        reason: format!("Failed to render first frame for {output_name}: {e:#}"),
                    });
                    failed_outputs.push(output_name);
                    continue;
                }
            }
        }

        // The first buffer must be swapped before relying on compositor callbacks.
        // Pending apply is polled above, without accumulating callbacks while decoding.
        if !surface_info.apply_request.pending
            && !surface_info.frame_callback_requested
            && let Some(qh) = state.queue_handle.as_ref()
        {
            let _callback = surface_info.wl_surface.frame(
                qh,
                FrameCallbackData {
                    output_name: output_name.clone(),
                    surface: surface_info.wl_surface.clone(),
                },
            );
            surface_info.frame_callback_requested = true;
            surface_info.wl_surface.commit();
        }
    }
    for output in failed_outputs {
        if let Err(error) = release_output_resources(state, &output) {
            warn!(
                "Failed to release output {} after apply error: {:#}",
                output, error
            );
        }
    }
}

/// Internal engine state
struct EngineState {
    /// Output manager for tracking Wayland outputs
    outputs: OutputManager,
    /// Active wallpaper sessions per output
    sessions: HashMap<String, WallpaperSession>,
    /// Event sender to GUI (std mpsc for cross-thread compatibility)
    events_tx: StdSender<EngineEvent>,
    /// Engine configuration
    config: EngineConfig,
    /// Whether engine is running
    running: bool,
    /// Pending output info (name -> wl_output global name)
    pending_outputs: HashMap<u32, PendingOutput>,
    /// Wayland compositor
    compositor: Option<WlCompositor>,
    /// Layer shell for creating background surfaces
    layer_shell: Option<ZwlrLayerShellV1>,
    /// EGL context for OpenGL rendering
    egl_context: Option<EglContext>,
    /// Layer surfaces per output (output_name -> surface info)
    layer_surfaces: HashMap<String, LayerSurfaceInfo>,
    /// Queue handle for creating Wayland objects
    queue_handle: Option<QueueHandle<EngineState>>,
    /// Whether currently on battery power
    on_battery: bool,
    /// Whether playback is paused due to power saving
    power_paused: bool,
    /// Last battery check time
    last_battery_check: std::time::Instant,
}

/// Layer surface state for an output
struct LayerSurfaceInfo {
    /// The wl_surface
    wl_surface: WlSurface,
    /// The layer surface
    layer_surface: ZwlrLayerSurfaceV1,
    /// Configured width
    width: u32,
    /// Configured height  
    height: u32,
    /// Whether surface is configured
    configured: bool,
    /// A configure/frame callback has made this surface ready to render
    frame_pending: bool,
    /// A compositor frame callback has been requested and has not fired yet
    frame_callback_requested: bool,
    /// Current generation remains recorded after its first-frame acknowledgement
    apply_request: ApplyRequest,
}

#[derive(Debug, Clone)]
struct ApplyRequest {
    request_id: u64,
    path: std::path::PathBuf,
    deadline: std::time::Instant,
    pending: bool,
}

impl ApplyRequest {
    fn new(request_id: u64, path: std::path::PathBuf, deadline: std::time::Instant) -> Self {
        Self {
            request_id,
            path,
            deadline,
            pending: true,
        }
    }

    fn expired(&self, now: std::time::Instant) -> bool {
        self.pending && now >= self.deadline
    }

    fn owns(&self, request_id: u64) -> bool {
        self.request_id == request_id
    }

    fn complete(&mut self, output: &str, now: std::time::Instant) -> Option<EngineEvent> {
        if !self.pending {
            return None;
        }
        let expired = self.expired(now);
        self.pending = false;
        Some(if expired {
            EngineEvent::ApplyFailed {
                request_id: self.request_id,
                output: output.to_string(),
                reason:
                    "Wallpaper application deadline expired before the first frame was accepted"
                        .to_string(),
            }
        } else {
            EngineEvent::WallpaperApplied {
                request_id: self.request_id,
                output: output.to_string(),
                path: self.path.clone(),
            }
        })
    }
}

struct FrameCallbackData {
    output_name: String,
    surface: WlSurface,
}

fn expire_pending_applies(state: &mut EngineState, now: std::time::Instant) {
    let expired: Vec<_> = state
        .layer_surfaces
        .iter()
        .filter(|(_, info)| info.apply_request.expired(now))
        .map(|(output, _)| output.clone())
        .collect();
    for output in expired {
        if let Some(event) = state
            .layer_surfaces
            .get_mut(&output)
            .and_then(|info| info.apply_request.complete(&output, now))
        {
            let _ = state.events_tx.send(event);
        }
        if let Err(error) = release_output_resources(state, &output) {
            warn!("Failed to release expired application for {output}: {error:#}");
        }
    }
}

/// Pending output information during enumeration
#[allow(dead_code)]
struct PendingOutput {
    /// Global name
    name: u32,
    /// WlOutput object
    wl_output: WlOutput,
    /// Output name (from name event)
    output_name: Option<String>,
    /// Width
    width: i32,
    /// Height
    height: i32,
    /// Position X
    x: i32,
    /// Position Y
    y: i32,
    /// Scale factor
    scale: i32,
    /// Done event received
    done: bool,
}

/// Handle incoming command from GUI
fn handle_command(cmd: EngineCommand, state: &mut EngineState) {
    match cmd {
        EngineCommand::ApplyWallpaper {
            request_id,
            deadline,
            path,
            output,
        } => {
            debug!("ApplyWallpaper: {:?} to {:?}", path, output);

            let outputs_to_apply: Vec<String> = match output {
                Some(name) => vec![name],
                None => state.outputs.output_names().map(String::from).collect(),
            };

            // Get queue handle - need to clone to avoid borrow issues
            let qh = match state.queue_handle.clone() {
                Some(qh) => qh,
                None => {
                    error!("Queue handle not available");
                    for output in outputs_to_apply {
                        let _ = state.events_tx.send(EngineEvent::ApplyFailed {
                            request_id,
                            output,
                            reason: "Queue handle not available".to_string(),
                        });
                    }
                    return;
                }
            };

            for output_name in outputs_to_apply {
                let request = ApplyRequest::new(request_id, path.clone(), deadline);
                if request.expired(std::time::Instant::now()) {
                    let _ = state.events_tx.send(EngineEvent::ApplyFailed {
                        request_id,
                        output: output_name,
                        reason: "Wallpaper application deadline expired before processing"
                            .to_string(),
                    });
                    continue;
                }
                match apply_wallpaper_to_output(state, request, &output_name, &qh) {
                    Ok(()) => {}
                    Err(e) => {
                        error!("Failed to apply wallpaper to {}: {}", output_name, e);
                        let _ = state.events_tx.send(EngineEvent::ApplyFailed {
                            request_id,
                            output: output_name.clone(),
                            reason: e.to_string(),
                        });
                        if state
                            .layer_surfaces
                            .get(&output_name)
                            .is_some_and(|info| info.apply_request.owns(request_id))
                            && let Err(error) = release_output_resources(state, &output_name)
                        {
                            warn!(
                                "Failed to release rejected application for {output_name}: {error:#}"
                            );
                        }
                    }
                }
            }
        }

        EngineCommand::CancelApply { request_id, output } => {
            // Include acknowledged generations: the receiver may have timed out
            // before observing their ACK. A stale cancellation cannot remove a retry.
            let result = if state
                .layer_surfaces
                .get(&output)
                .is_some_and(|info| info.apply_request.owns(request_id))
            {
                release_output_resources(state, &output)
                    .map(|_| ())
                    .map_err(|error| format!("Failed to cancel wallpaper application: {error:#}"))
            } else {
                Ok(())
            };
            let _ = state.events_tx.send(EngineEvent::ApplyCancelled {
                request_id,
                output,
                result,
            });
        }

        EngineCommand::ClearWallpaper { output } => {
            debug!("ClearWallpaper: {:?}", output);

            let explicit_output = output.is_some();
            let outputs_to_clear: Vec<String> = match output {
                Some(name) => vec![name],
                None => outputs_with_resources(state),
            };

            for output_name in outputs_to_clear {
                match release_output_resources(state, &output_name) {
                    Ok(cleared) if cleared || explicit_output => {
                        let _ = state.events_tx.send(EngineEvent::WallpaperCleared {
                            output: output_name,
                        });
                    }
                    Ok(_) => {}
                    Err(error) => {
                        let _ = state.events_tx.send(EngineEvent::OutputError {
                            output: output_name.clone(),
                            reason: format!(
                                "Failed to clear wallpaper for {output_name}: {error:#}"
                            ),
                        });
                    }
                }
            }
        }

        EngineCommand::SetVolume { output, volume } => {
            debug!("SetVolume: {} = {}", output, volume);
            if let Some(session) = state.sessions.get_mut(&output) {
                session.set_volume(volume);
            }
        }

        EngineCommand::Pause { output } => {
            debug!("Pause: {:?}", output);
            let sessions: Vec<&mut WallpaperSession> = match &output {
                Some(name) => state.sessions.get_mut(name).into_iter().collect(),
                None => state.sessions.values_mut().collect(),
            };
            for session in sessions {
                session.pause();
            }
        }

        EngineCommand::Resume { output } => {
            debug!("Resume: {:?}", output);
            let sessions: Vec<&mut WallpaperSession> = match &output {
                Some(name) => state.sessions.get_mut(name).into_iter().collect(),
                None => state.sessions.values_mut().collect(),
            };
            for session in sessions {
                session.resume();
            }
        }

        EngineCommand::GetOutputs => {
            debug!("GetOutputs requested");
            let outputs: Vec<_> = state
                .outputs
                .ready_outputs()
                .map(|(_, info)| info.clone())
                .collect();
            let _ = state.events_tx.send(EngineEvent::OutputsList(outputs));
        }

        EngineCommand::GetStatus => {
            debug!("GetStatus requested");
            let outputs: Vec<_> = state
                .outputs
                .ready_outputs()
                .map(|(_, info)| info.clone())
                .collect();
            let active_wallpapers = state
                .sessions
                .iter()
                .filter(|(_, session)| session.can_reuse_resources())
                .map(|(name, session)| {
                    (
                        name.clone(),
                        session.wallpaper_path().map(std::path::PathBuf::from),
                    )
                })
                .collect();
            let status = EngineStatus {
                running: state.running,
                outputs,
                active_wallpapers,
            };
            let _ = state.events_tx.send(EngineEvent::Status(status));
        }

        EngineCommand::Shutdown => {
            debug!("Shutdown requested");
            // Shutdown is handled by the shutdown flag, not here
        }
    }
}

fn outputs_with_resources(state: &EngineState) -> Vec<String> {
    state
        .sessions
        .keys()
        .chain(state.layer_surfaces.keys())
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

/// Release render resources before their underlying Wayland protocol objects.
fn release_output_resources(state: &mut EngineState, output_name: &str) -> Result<bool> {
    if let Some(session) = state.sessions.get_mut(output_name)
        && let Some(egl_context) = state.egl_context.as_ref()
    {
        session.cleanup_egl(egl_context)?;
    }
    let session_removed = state.sessions.remove(output_name).is_some();
    let surface_removed = if let Some(info) = state.layer_surfaces.remove(output_name) {
        info.layer_surface.destroy();
        info.wl_surface.destroy();
        true
    } else {
        false
    };
    Ok(session_removed || surface_removed)
}

/// Apply wallpaper to a specific output
fn apply_wallpaper_to_output(
    state: &mut EngineState,
    request: ApplyRequest,
    output_name: &str,
    qh: &QueueHandle<EngineState>,
) -> Result<()> {
    // Check if we can reuse existing layer surface (hot-swap optimization)
    if let (Some(surface_info), Some(session)) = (
        state.layer_surfaces.get_mut(output_name),
        state.sessions.get_mut(output_name),
    ) && surface_info.configured
        && session.can_reuse_resources()
    {
        info!(
            "Hot-swapping wallpaper for {} (reusing surface)",
            output_name
        );
        surface_info.apply_request = request;
        session.load_new_wallpaper(&surface_info.apply_request.path)?;
        return Ok(());
    }

    // Get output info
    let output_state = state.outputs.get(output_name).context("Output not found")?;
    let output_info = output_state.info.clone();
    let wl_output = output_state.wl_output.clone();

    // Check prerequisites
    let compositor = state
        .compositor
        .as_ref()
        .cloned()
        .context("Compositor not available")?;
    let layer_shell = state
        .layer_shell
        .as_ref()
        .cloned()
        .context("Layer shell not available")?;

    release_output_resources(state, output_name)?;

    info!(
        "Creating layer surface for {} ({}x{})",
        output_name, output_info.width, output_info.height
    );

    // Create wl_surface
    let wl_surface = compositor.create_surface(qh, output_name.to_string());

    // Create layer surface on background layer
    let layer_surface = layer_shell.get_layer_surface(
        &wl_surface,
        Some(&wl_output),
        zwlr_layer_shell_v1::Layer::Background,
        CString::new("lwe").unwrap().into_string().unwrap(),
        qh,
        output_name.to_string(),
    );

    // Configure layer surface
    // Anchor to all edges for full coverage
    layer_surface.set_anchor(Anchor::Top | Anchor::Bottom | Anchor::Left | Anchor::Right);
    // Use -1 to ignore other layer surfaces' exclusive zones (e.g., top bars)
    layer_surface.set_exclusive_zone(-1);
    layer_surface.set_keyboard_interactivity(zwlr_layer_surface_v1::KeyboardInteractivity::None);
    // Set size to 0,0 to let compositor determine full output size
    layer_surface.set_size(0, 0);

    // Commit to trigger configure
    wl_surface.commit();

    // Store layer surface info
    state.layer_surfaces.insert(
        output_name.to_string(),
        LayerSurfaceInfo {
            wl_surface,
            layer_surface,
            width: output_info.width as u32,
            height: output_info.height as u32,
            configured: false,
            frame_pending: false,
            frame_callback_requested: false,
            apply_request: request.clone(),
        },
    );

    // Create wallpaper session
    let session = WallpaperSession::new(request.path, output_info, state.config.video.clone())?;
    state.sessions.insert(output_name.to_string(), session);

    info!("Wallpaper session created for {}", output_name);

    Ok(())
}

// Wayland dispatch implementation for wl_registry
impl Dispatch<WlRegistry, ()> for EngineState {
    fn event(
        state: &mut Self,
        registry: &WlRegistry,
        event: wl_registry::Event,
        _data: &(),
        _conn: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        match event {
            wl_registry::Event::Global {
                name,
                interface,
                version,
            } => {
                match interface.as_str() {
                    "wl_compositor" => {
                        debug!(
                            "Found wl_compositor global: name={}, version={}",
                            name, version
                        );
                        let compositor: WlCompositor = registry.bind(name, version.min(4), qh, ());
                        state.compositor = Some(compositor);
                        info!("Bound wl_compositor");
                    }
                    "zwlr_layer_shell_v1" => {
                        debug!(
                            "Found zwlr_layer_shell_v1 global: name={}, version={}",
                            name, version
                        );
                        let layer_shell: ZwlrLayerShellV1 =
                            registry.bind(name, version.min(4), qh, ());
                        state.layer_shell = Some(layer_shell);
                        info!("Bound zwlr_layer_shell_v1");
                    }
                    "wl_output" => {
                        debug!("Found wl_output global: name={}, version={}", name, version);
                        // Bind the output
                        let output: WlOutput = registry.bind(name, version.min(4), qh, name);
                        // Store pending output
                        state.pending_outputs.insert(
                            name,
                            PendingOutput {
                                name,
                                wl_output: output,
                                output_name: None,
                                width: 0,
                                height: 0,
                                x: 0,
                                y: 0,
                                scale: 1,
                                done: false,
                            },
                        );
                    }
                    _ => {}
                }
            }
            wl_registry::Event::GlobalRemove { name } => {
                // Check if this was an output
                if let Some(pending) = state.pending_outputs.remove(&name)
                    && let Some(output_name) = &pending.output_name
                {
                    info!("Output removed: {}", output_name);
                    state.outputs.remove_output(output_name);
                    let _ = state
                        .events_tx
                        .send(EngineEvent::OutputRemoved(output_name.clone()));
                }
            }
            _ => {}
        }
    }
}

// Wayland dispatch implementation for wl_output
impl Dispatch<WlOutput, u32> for EngineState {
    fn event(
        state: &mut Self,
        _output: &WlOutput,
        event: wl_output::Event,
        global_name: &u32,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        let Some(pending) = state.pending_outputs.get_mut(global_name) else {
            return;
        };

        match event {
            wl_output::Event::Geometry {
                x,
                y,
                physical_width: _,
                physical_height: _,
                subpixel: _,
                make: _,
                model: _,
                transform: _,
            } => {
                pending.x = x;
                pending.y = y;
            }
            wl_output::Event::Mode {
                flags: wayland_client::WEnum::Value(mode),
                width,
                height,
                refresh: _,
            } if mode.contains(wl_output::Mode::Current) => {
                // Only use current mode
                pending.width = width;
                pending.height = height;
            }
            wl_output::Event::Scale { factor } => {
                pending.scale = factor;
            }
            wl_output::Event::Name { name } => {
                pending.output_name = Some(name);
            }
            wl_output::Event::Done => {
                pending.done = true;
                // Finalize the output
                let output_name = pending
                    .output_name
                    .clone()
                    .unwrap_or_else(|| format!("output-{}", global_name));

                info!(
                    "Output ready: {} ({}x{} @ {},{})",
                    output_name, pending.width, pending.height, pending.x, pending.y
                );

                // Add to output manager
                state
                    .outputs
                    .add_output(output_name.clone(), pending.wl_output.clone());
                state
                    .outputs
                    .update_mode(&output_name, pending.width, pending.height);
                state
                    .outputs
                    .update_geometry(&output_name, pending.x, pending.y);
                state.outputs.update_scale(&output_name, pending.scale);
                state.outputs.mark_ready(&output_name);

                // Send event to GUI
                let info = lwe_core::OutputInfo {
                    name: output_name.clone(),
                    width: pending.width,
                    height: pending.height,
                    scale: pending.scale as f64,
                    position: (pending.x, pending.y),
                    active: true,
                    hdr_capabilities: lwe_core::OutputHdrCapabilities::default(),
                };
                let _ = state.events_tx.send(EngineEvent::OutputAdded(info));
            }
            _ => {}
        }
    }
}

// Dispatch for wl_compositor (no events to handle)
impl Dispatch<WlCompositor, ()> for EngineState {
    fn event(
        _state: &mut Self,
        _proxy: &WlCompositor,
        _event: wl_compositor::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        // wl_compositor has no events
    }
}

// Dispatch for wl_surface
impl Dispatch<WlSurface, String> for EngineState {
    fn event(
        _state: &mut Self,
        _proxy: &WlSurface,
        _event: wl_surface::Event,
        _data: &String,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        // We handle surface events via layer_surface
    }
}

// Dispatch for zwlr_layer_shell_v1 (no events)
impl Dispatch<ZwlrLayerShellV1, ()> for EngineState {
    fn event(
        _state: &mut Self,
        _proxy: &ZwlrLayerShellV1,
        _event: zwlr_layer_shell_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        // zwlr_layer_shell_v1 has no events
    }
}

// Dispatch for zwlr_layer_surface_v1
impl Dispatch<ZwlrLayerSurfaceV1, String> for EngineState {
    fn event(
        state: &mut Self,
        layer_surface: &ZwlrLayerSurfaceV1,
        event: zwlr_layer_surface_v1::Event,
        output_name: &String,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        // Delayed events from a destroyed surface must not mutate its replacement.
        if state
            .layer_surfaces
            .get(output_name)
            .is_none_or(|info| info.layer_surface != *layer_surface)
        {
            return;
        }
        match event {
            zwlr_layer_surface_v1::Event::Configure {
                serial,
                width,
                height,
            } => {
                debug!(
                    "Layer surface configure for {}: {}x{} (serial={})",
                    output_name, width, height, serial
                );

                // Update layer surface info
                if let Some(info) = state.layer_surfaces.get_mut(output_name) {
                    info.width = width;
                    info.height = height;
                    info.configured = true;
                    info.frame_pending = true;
                }

                // Acknowledge the configure
                layer_surface.ack_configure(serial);

                // Commit to apply
                if let Some(info) = state.layer_surfaces.get(output_name) {
                    info.wl_surface.commit();
                }

                info!(
                    "Layer surface configured for {}: {}x{}",
                    output_name, width, height
                );
            }
            zwlr_layer_surface_v1::Event::Closed => {
                info!("Layer surface closed for {}", output_name);
                if let Some(info) = state.layer_surfaces.get_mut(output_name)
                    && info.apply_request.pending
                {
                    info.apply_request.pending = false;
                    let _ = state.events_tx.send(EngineEvent::ApplyFailed {
                        request_id: info.apply_request.request_id,
                        output: output_name.clone(),
                        reason: "Wallpaper layer surface closed before application completed"
                            .to_string(),
                    });
                }
                if let Err(error) = release_output_resources(state, output_name) {
                    let _ = state.events_tx.send(EngineEvent::OutputError {
                        output: output_name.clone(),
                        reason: format!("Failed to release closed output {output_name}: {error:#}"),
                    });
                }
            }
            _ => {}
        }
    }
}

// Dispatch for frame callback
impl Dispatch<WlCallback, FrameCallbackData> for EngineState {
    fn event(
        state: &mut Self,
        _callback: &WlCallback,
        event: wl_callback::Event,
        data: &FrameCallbackData,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        if let wl_callback::Event::Done { callback_data: _ } = event {
            // Frame callback triggered - mark surface ready for rendering
            if let Some(info) = state.layer_surfaces.get_mut(&data.output_name)
                && info.wl_surface == data.surface
            {
                info.frame_callback_requested = false;
                info.frame_pending = true;
            }
        }
    }
}

/// Check if the system is running on battery power
fn check_battery_status() -> bool {
    let power_supply = std::path::Path::new("/sys/class/power_supply");

    if let Ok(entries) = std::fs::read_dir(power_supply) {
        for entry in entries.flatten() {
            let path = entry.path();

            // Check if this is a battery (type = "Battery")
            let type_path = path.join("type");
            if let Ok(device_type) = std::fs::read_to_string(&type_path)
                && device_type.trim().to_lowercase() != "battery"
            {
                continue;
            }

            // Check battery status
            let status_path = path.join("status");
            if let Ok(status) = std::fs::read_to_string(status_path) {
                let status = status.trim().to_lowercase();
                if status == "discharging" {
                    return true;
                }
            }
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::{Duration, Instant};

    #[test]
    fn expired_first_frame_reports_failure_for_its_generation() {
        let deadline = Instant::now();
        let mut request = ApplyRequest::new(7, PathBuf::from("/tmp/video.mp4"), deadline);
        assert!(matches!(
            request.complete("eDP-1", deadline),
            Some(EngineEvent::ApplyFailed { request_id: 7, output, .. }) if output == "eDP-1"
        ));
        assert!(request.complete("eDP-1", deadline).is_none());
    }

    #[test]
    fn accepted_first_frame_acknowledges_once_and_remains_cancellable() {
        let now = Instant::now();
        let deadline = now + Duration::from_secs(1);
        let path = PathBuf::from("/tmp/video.mp4");
        let mut request = ApplyRequest::new(7, path.clone(), deadline);
        assert!(matches!(
            request.complete("eDP-1", now),
            Some(EngineEvent::WallpaperApplied { request_id: 7, output, path: applied_path })
                if output == "eDP-1" && applied_path == path
        ));
        assert!(request.complete("eDP-1", now).is_none());
        assert!(!request.expired(deadline));
        assert!(request.owns(7));
    }

    #[test]
    fn same_path_retry_is_not_owned_by_old_cancellation() {
        let path = PathBuf::from("/tmp/video.mp4");
        let deadline = Instant::now() + Duration::from_secs(1);
        let old = ApplyRequest::new(7, path.clone(), deadline);
        let retry = ApplyRequest::new(8, path, deadline);
        assert!(old.owns(7));
        assert!(!retry.owns(7));
        assert!(retry.owns(8));
    }
}
