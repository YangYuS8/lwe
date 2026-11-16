//! Playback engine for integrated wallpaper rendering
//!
//! This module provides the high-level API for managing wallpaper playback
//! within the GUI process. The engine runs in a dedicated thread and
//! communicates with the GUI via channels.

mod allocator;
mod command;
mod schedule;
mod session;

pub use command::{
    EngineCommand, EngineConfig, EngineEvent, EngineRenderMetrics, EngineStatus, PlaybackSettings,
    SceneGpuBudgetSnapshot, WallpaperSource,
};
pub use session::WallpaperSession;

use schedule::{FrameSchedule, frame_interval};
use session::FrameDemand;

use std::collections::{BTreeSet, HashMap};
use std::ffi::CString;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver as StdReceiver, Sender as StdSender};
use std::thread::{self, JoinHandle};
use std::time::Instant;

use anyhow::{Context, Result};
use calloop::EventLoop;
use calloop::channel::{Sender, channel};
use calloop::ping::{Ping, make_ping};
use calloop_wayland_source::WaylandSource;
use tracing::{debug, error, info, warn};
use wayland_client::protocol::wl_callback::{self, WlCallback};
use wayland_client::protocol::wl_compositor::{self, WlCompositor};
use wayland_client::protocol::wl_output::{self, WlOutput};
use wayland_client::protocol::wl_region::{self, WlRegion};
use wayland_client::protocol::wl_registry::{self, WlRegistry};
use wayland_client::protocol::wl_surface::{self, WlSurface};
use wayland_client::{Connection, Dispatch, QueueHandle};
use wayland_protocols_wlr::layer_shell::v1::client::{
    zwlr_layer_shell_v1::{self, ZwlrLayerShellV1},
    zwlr_layer_surface_v1::{self, Anchor, ZwlrLayerSurfaceV1},
};

use crate::egl::EglContext;
use crate::scene::shared_texture::{ResourcePlan, TexturePool};
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
        let _ = self.commands_tx.send(EngineCommand::Shutdown);
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
        let _ = self.commands_tx.send(EngineCommand::Shutdown);
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
    frame_interval(config.fps_limit)?;
    config
        .playback_settings()
        .validate()
        .map_err(anyhow::Error::msg)?;
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

    let interval = frame_interval(config.fps_limit)?;
    let on_battery = check_battery_status();
    let power_paused = config.pause_on_battery && on_battery;
    let (scene_worker_ping, scene_worker_source) =
        make_ping().context("Failed to create Scene worker notification source")?;
    // Create engine state
    let mut state = EngineState {
        outputs: OutputManager::new(),
        sessions: HashMap::new(),
        scene_textures: TexturePool::new()?,
        output_errors: HashMap::new(),
        events_tx: events_tx.clone(),
        config,
        running: true,
        pending_outputs: HashMap::new(),
        compositor: None,
        layer_shell: None,
        egl_context: None,
        layer_surfaces: HashMap::new(),
        queue_handle: None,
        on_battery,
        power_paused,
        last_battery_check: std::time::Instant::now(),
        frame_interval: interval,
        scene_worker_ping,
    };

    // Create event loop
    let mut event_loop: EventLoop<'static, EngineState> =
        EventLoop::try_new().context("Failed to create event loop")?;

    event_loop
        .handle()
        .insert_source(scene_worker_source, |(), _, state| {
            scene_preparation_ready(state, Instant::now());
        })
        .map_err(|e| anyhow::anyhow!("Failed to register Scene worker notifications: {e:?}"))?;

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
                state.scene_textures.bind_owner(&egl_ctx)?;
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
    let mut loop_result = Ok(());

    while state.running && !shutdown.load(Ordering::Relaxed) {
        // Periodically check battery status
        if state.last_battery_check.elapsed() >= battery_check_interval {
            state.on_battery = check_battery_status();
            state.last_battery_check = std::time::Instant::now();

            synchronize_power_policy(&mut state);
        }

        let wait = next_dispatch_wait(&state, std::time::Instant::now());
        if let Err(error) = event_loop.dispatch(wait, &mut state) {
            let reason = format!("Event loop dispatch failed: {error}");
            let _ = events_tx.send(EngineEvent::Error(reason.clone()));
            loop_result = Err(anyhow::anyhow!(reason));
            break;
        }
        if !state.running || shutdown.load(Ordering::Relaxed) {
            break;
        }

        // Expire requests even before configure or while playback is power-paused.
        expire_pending_applies(&mut state, std::time::Instant::now());

        for (output, session) in &mut state.sessions {
            if let Err(error) = session.poll_audio_health() {
                let reason = format!("Wallpaper audio failed for {output}: {error:#}");
                if state.output_errors.get(output) != Some(&reason) {
                    state.output_errors.insert(output.clone(), reason.clone());
                    let _ = events_tx.send(EngineEvent::OutputError {
                        output: output.clone(),
                        reason,
                    });
                }
            }
        }

        // First-frame, resize and rollback remain presentable while power-paused.
        prepare_scene_cpu_frames(&mut state);
        render_all_surfaces(&mut state);
        prepare_scene_cpu_frames(&mut state);
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
    if let Err(error) = state.scene_textures.close() {
        warn!("Scene texture domain quarantined during shutdown: {error:#}");
    }

    state.running = false;
    let _ = events_tx.send(EngineEvent::Stopped);

    loop_result
}

fn output_demand(
    state: &EngineState,
    info: &LayerSurfaceInfo,
    session: &WallpaperSession,
) -> FrameDemand {
    let demand = session.render_demand(
        info.width as i32,
        info.height as i32,
        info.apply_request.pending,
    );
    effective_demand(state.power_paused, demand)
}

fn effective_demand(power_paused: bool, demand: FrameDemand) -> FrameDemand {
    if power_paused && demand != FrameDemand::Urgent {
        FrameDemand::Idle
    } else {
        demand
    }
}

fn should_request_callback(
    presented: bool,
    demand: FrameDemand,
    pending: bool,
    outstanding: bool,
) -> bool {
    presented && demand == FrameDemand::Continuous && !pending && !outstanding
}

fn next_dispatch_wait(state: &EngineState, now: std::time::Instant) -> std::time::Duration {
    // Commands wake calloop directly; this bound also services cooperative flags
    // and battery polling without the old 60Hz idle timer.
    let mut wait = std::time::Duration::from_secs(1);
    for (output, info) in &state.layer_surfaces {
        if info.apply_request.pending || info.apply_request.awaiting_commit {
            wait = wait.min(info.apply_request.deadline.saturating_duration_since(now));
        }
        if !info.configured {
            continue;
        }
        if let Some(session) = state.sessions.get(output) {
            let demand = output_demand(state, info, session);
            if demand != FrameDemand::Idle
                && (info.frame_pending
                    || info.apply_request.pending
                    || demand == FrameDemand::Urgent)
            {
                wait = wait.min(info.schedule.delay(now));
            }
        }
    }
    wait
}

fn wake_outputs(state: &mut EngineState, output: Option<&str>) {
    for (name, info) in &mut state.layer_surfaces {
        if output.is_none_or(|output| output == name) {
            info.wake();
        }
    }
}

fn scene_retry_wanted(configured: bool, pending: bool, demand: FrameDemand) -> bool {
    configured && pending && demand != FrameDemand::Idle
}

fn scene_ahead_wanted(configured: bool, transaction: bool, demand: FrameDemand) -> bool {
    configured && !transaction && demand == FrameDemand::Continuous
}

/// CPU preparation does not grant presentation readiness or advance accepted state.
/// A speculative timed result retains the global producer permit, so competing
/// sessions use the original gated decoder policy and release any ahead result.
fn prepare_scene_cpu_frames(state: &mut EngineState) {
    let allow_timed = state.sessions.len() == 1;
    let mut failures = Vec::new();
    for (output, session) in &mut state.sessions {
        let wanted = state.layer_surfaces.get(output).is_some_and(|info| {
            scene_ahead_wanted(
                info.configured,
                info.apply_request.pending || info.apply_request.awaiting_commit,
                effective_demand(
                    state.power_paused,
                    session.render_demand(
                        info.width as i32,
                        info.height as i32,
                        info.apply_request.pending,
                    ),
                ),
            )
        });
        if !wanted || !allow_timed {
            session.cancel_scene_ahead();
        }
        if wanted && let Err(error) = session.prepare_scene_ahead(allow_timed) {
            failures.push((output.clone(), error));
        }
    }
    for (output, error) in failures {
        fail_output_render(state, &output, error);
    }
}

fn fail_output_render(state: &mut EngineState, output: &str, error: anyhow::Error) {
    warn!("Frame render error for {}: {:#}", output, error);
    if let Some(info) = state.layer_surfaces.get_mut(output)
        && (info.apply_request.pending || info.apply_request.awaiting_commit)
    {
        info.apply_request.pending = false;
        info.apply_request.awaiting_commit = false;
        let _ = state.events_tx.send(EngineEvent::ApplyFailed {
            request_id: info.apply_request.request_id,
            output: output.to_owned(),
            reason: format!("Failed to render first frame for {output}: {error:#}"),
        });
    } else {
        let reason = format!("Wallpaper rendering failed for {output}: {error:#}");
        state
            .output_errors
            .insert(output.to_owned(), reason.clone());
        let _ = state.events_tx.send(EngineEvent::OutputError {
            output: output.to_owned(),
            reason,
        });
    }
    if let Err(error) = reject_output_candidate(state, output) {
        warn!(
            "Failed to release output {} after apply error: {:#}",
            output, error
        );
    }
}

fn scene_preparation_ready(state: &mut EngineState, now: Instant) {
    for (output, session) in &mut state.sessions {
        if session.take_worker_ready().is_none() {
            continue;
        }
        let Some(info) = state.layer_surfaces.get_mut(output) else {
            continue;
        };
        let demand = effective_demand(
            state.power_paused,
            session.render_demand(
                info.width as i32,
                info.height as i32,
                info.apply_request.pending,
            ),
        );
        if scene_retry_wanted(info.configured, info.frame_pending, demand) {
            info.schedule.preparation_ready(now, state.frame_interval);
        }
    }
}

/// Render all configured surfaces
fn render_all_surfaces(state: &mut EngineState) {
    // Get EGL context reference
    if state.egl_context.is_none() {
        return; // No EGL context, can't render
    }

    // Collect output names to avoid borrow issues
    let outputs: Vec<String> = state
        .layer_surfaces
        .iter()
        .filter(|(output, info)| {
            info.configured
                && state.sessions.get(*output).is_some_and(|session| {
                    let demand = output_demand(state, info, session);
                    demand != FrameDemand::Idle
                        && (info.frame_pending
                            || info.apply_request.pending
                            || demand == FrameDemand::Urgent)
                        && info.schedule.ready(std::time::Instant::now())
                })
        })
        .map(|(name, _)| name.clone())
        .collect();
    let mut failed_outputs = Vec::new();

    for output_name in outputs {
        let egl_context = state.egl_context.as_ref().expect("EGL context checked");
        let budget_result = (|| {
            let plan = combined_scene_plan(state, None)?;
            let info = state
                .layer_surfaces
                .get(&output_name)
                .context("Output surface unavailable")?;
            let session = state
                .sessions
                .get(&output_name)
                .context("Output session unavailable")?;
            plan.preflight(
                state.scene_textures.retired_bytes()?,
                session.additional_scene_payload_bytes(info.width as i32, info.height as i32)?,
            )
            .map(|_| ())
        })();
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

        // Render frame. Only a successful swap starts the next presentation
        // interval; command/compositor wakeups cannot shorten it.
        let result = budget_result.and_then(|()| {
            session.render_frame_to_surface_owned(
                egl_context,
                &surface_info.wl_surface,
                surface_info.width as i32,
                surface_info.height as i32,
                is_waiting_for_first_frame,
            )
        });
        let presented = matches!(&result, Ok(true));
        surface_info.schedule.attempted(
            std::time::Instant::now(),
            presented,
            state.frame_interval,
            session.empty_frame_retry(),
        );
        match result {
            Ok(true) => {
                if state.power_paused {
                    session.set_power_paused(true);
                }
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
                    // Retain the previous renderer until the caller accepts this
                    // ACK. A delayed receiver may still cancel the candidate.
                }
            }
            Ok(false) => {
                // Keep readiness for bounded preparation/decoder retries, without emitting
                // a callback/commit for a buffer which did not change.
                surface_info.frame_pending = true;
            }
            Err(e) => {
                fail_output_render(state, &output_name, e);
                continue;
            }
        }

        // The first buffer must be swapped before relying on compositor callbacks.
        // Pending apply is polled above, without accumulating callbacks while decoding.
        if should_request_callback(
            presented,
            effective_demand(
                state.power_paused,
                session.render_demand(surface_info.width as i32, surface_info.height as i32, false),
            ),
            surface_info.apply_request.pending,
            surface_info.frame_callback_requested,
        ) && let Some(qh) = state.queue_handle.as_ref()
        {
            let _callback = surface_info.wl_surface.frame(
                qh,
                FrameCallbackData {
                    output_name: output_name.clone(),
                    surface: surface_info.wl_surface.clone(),
                    epoch: surface_info.callback_epoch,
                },
            );
            surface_info.frame_callback_requested = true;
            surface_info.metrics.callbacks_requested =
                surface_info.metrics.callbacks_requested.saturating_add(1);
            surface_info.wl_surface.commit();
        }
    }
    for output in failed_outputs {
        if let Err(error) = reject_output_candidate(state, &output) {
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
    scene_textures: Rc<TexturePool>,
    output_errors: HashMap<String, String>,
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
    frame_interval: Option<std::time::Duration>,
    scene_worker_ping: Ping,
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
    schedule: FrameSchedule,
    metrics: EngineRenderMetrics,
    callback_epoch: u64,
    /// Current generation remains recorded after its first-frame acknowledgement
    apply_request: ApplyRequest,
    /// Retained until the caller confirms presentation and saves the assignment.
    previous_request: Option<ApplyRequest>,
}

impl LayerSurfaceInfo {
    fn wake(&mut self) {
        self.frame_pending = true;
        self.frame_callback_requested = false;
        self.callback_epoch = self.callback_epoch.wrapping_add(1);
        self.schedule.wake();
    }
}

#[derive(Debug, Clone)]
struct ApplyRequest {
    request_id: u64,
    path: std::path::PathBuf,
    source: WallpaperSource,
    deadline: std::time::Instant,
    pending: bool,
    awaiting_commit: bool,
    committed: bool,
    finalized: bool,
}

impl ApplyRequest {
    #[cfg(test)]
    fn new(request_id: u64, path: std::path::PathBuf, deadline: std::time::Instant) -> Self {
        Self::from_source(request_id, WallpaperSource::Video { path }, deadline)
    }

    fn from_source(request_id: u64, source: WallpaperSource, deadline: std::time::Instant) -> Self {
        Self {
            request_id,
            path: source.path().to_path_buf(),
            source,
            deadline,
            pending: true,
            awaiting_commit: false,
            committed: false,
            finalized: false,
        }
    }

    fn expired(&self, now: std::time::Instant) -> bool {
        (self.pending || self.awaiting_commit) && now >= self.deadline
    }

    fn owns(&self, request_id: u64) -> bool {
        self.request_id == request_id
    }

    fn cancellable(&self, request_id: u64) -> bool {
        self.owns(request_id) && !self.finalized
    }

    /// A committed generation still owns its rollback until persistence either
    /// finalizes it or cancels it. Check before replacing any output resources.
    fn ensure_supersedable(&self) -> Result<()> {
        anyhow::ensure!(
            !self.committed || self.finalized,
            "The output has a committed wallpaper awaiting persistence finalization"
        );
        Ok(())
    }

    fn complete(&mut self, output: &str, now: std::time::Instant) -> Option<EngineEvent> {
        if !self.pending && !self.expired(now) {
            return None;
        }
        let expired = self.expired(now);
        self.pending = false;
        self.awaiting_commit = !expired;
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

    fn commit(&mut self, now: std::time::Instant) -> Result<(), String> {
        if self.committed {
            return Ok(());
        }
        if self.expired(now) {
            return Err(
                "Wallpaper application expired before its presentation was committed".to_string(),
            );
        }
        if self.pending || !self.awaiting_commit {
            return Err(
                "Wallpaper application has no uncommitted first-frame acknowledgement".to_string(),
            );
        }
        self.awaiting_commit = false;
        self.committed = true;
        Ok(())
    }
}

struct FrameCallbackData {
    output_name: String,
    surface: WlSurface,
    epoch: u64,
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
        if let Err(error) = reject_output_candidate(state, &output) {
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
            source,
            output,
        } => {
            debug!("ApplyWallpaper: {:?} to {:?}", source.path(), output);

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
                let request = ApplyRequest::from_source(request_id, source.clone(), deadline);
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

        EngineCommand::CommitApply { request_id, output } => {
            let result = match state.layer_surfaces.get_mut(&output) {
                Some(info) if info.apply_request.owns(request_id) => {
                    info.apply_request.commit(std::time::Instant::now())
                }
                _ => Err("Wallpaper application no longer owns this output".to_string()),
            };
            if result.is_ok() {
                if let Some(session) = state.sessions.get_mut(&output) {
                    session.accept_pending();
                }
                state.output_errors.remove(&output);
            } else if state
                .layer_surfaces
                .get(&output)
                .is_some_and(|info| info.apply_request.owns(request_id))
                && let Err(error) = reject_output_candidate(state, &output)
            {
                warn!("Failed to roll back rejected commit for {output}: {error:#}");
            }
            let _ = state.events_tx.send(EngineEvent::ApplyCommitted {
                request_id,
                output,
                result,
            });
        }

        EngineCommand::FinalizeApply { request_id, output } => {
            if state.layer_surfaces.get(&output).is_some_and(|info| {
                info.apply_request.owns(request_id) && info.apply_request.committed
            }) && let (Some(session), Some(egl)) =
                (state.sessions.get_mut(&output), state.egl_context.as_ref())
            {
                // The host has already saved this generation. Even if GL
                // resource cleanup fails, a delayed cancellation cannot undo it.
                if let Some(info) = state.layer_surfaces.get_mut(&output) {
                    info.apply_request.finalized = true;
                }
                match session.finalize_pending(egl) {
                    Ok(()) => {
                        let mut released_rollback = false;
                        if let Some(info) = state.layer_surfaces.get_mut(&output) {
                            released_rollback = info.previous_request.is_some();
                            info.previous_request = None;
                        }
                        if released_rollback {
                            allocator::reclaim_released_pages("finalized_rollback");
                        }
                    }
                    Err(error) => {
                        let reason = format!(
                            "Failed to finalize wallpaper resources or audio for {output}: {error:#}"
                        );
                        state.output_errors.insert(output.clone(), reason.clone());
                        let _ = state.events_tx.send(EngineEvent::OutputError {
                            output: output.clone(),
                            reason,
                        });
                        warn!(
                            "Failed to release finalized rollback resources for {output}: {error:#}"
                        );
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
                .is_some_and(|info| info.apply_request.cancellable(request_id))
            {
                reject_output_candidate(state, &output)
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
                        state.output_errors.remove(&output_name);
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
            wake_outputs(state, output.as_deref());
        }

        EngineCommand::UpdatePlaybackSettings {
            settings,
            deadline,
            reply,
        } => {
            let result = update_playback_settings(state, settings, deadline);
            let _ = reply.send(result);
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
            let scene_gpu_budget = combined_scene_plan(state, None)
                .and_then(|plan| {
                    let retired_bytes = state.scene_textures.retired_bytes()?;
                    let resident_bytes = state.scene_textures.resident_bytes()?;
                    let immutable_bytes = plan.immutable_bytes()?;
                    Ok(command::SceneGpuBudgetSnapshot {
                        context_generation: state.scene_textures.generation(),
                        immutable_resident_bytes: resident_bytes.saturating_sub(retired_bytes),
                        private_and_reserved_bytes: plan.private_bytes,
                        immutable_reserved_bytes: immutable_bytes
                            .saturating_sub(resident_bytes.saturating_sub(retired_bytes)),
                        retired_bytes,
                        logical_peak_bytes: plan.total(retired_bytes)?,
                        allocation_count: state.scene_textures.allocation_count(),
                        lease_count: state.scene_textures.lease_count(),
                    })
                })
                .unwrap_or_else(|error| {
                    warn!("Scene GPU budget snapshot failed: {error:#}");
                    command::SceneGpuBudgetSnapshot {
                        logical_peak_bytes: u64::MAX,
                        ..Default::default()
                    }
                });
            let status = EngineStatus {
                running: state.running,
                scene_gpu_budget,
                scene_decoded_texture_bytes: scene_decoded_allocations(state)
                    .and_then(decoded_texture_total)
                    .unwrap_or(u64::MAX),
                playback_settings: state.config.playback_settings(),
                on_battery: state.on_battery,
                power_paused: state.power_paused,
                outputs,
                active_wallpapers,
                output_errors: state.output_errors.clone(),
                render_metrics: state
                    .sessions
                    .iter()
                    .map(|(output, session)| {
                        let mut metrics = session.metrics();
                        if let Some(info) = state.layer_surfaces.get(output) {
                            metrics.callbacks_requested = info.metrics.callbacks_requested;
                            metrics.callbacks_received = info.metrics.callbacks_received;
                        }
                        (output.clone(), metrics)
                    })
                    .collect(),
            };
            let _ = state.events_tx.send(EngineEvent::Status(status));
        }

        EngineCommand::Shutdown => {
            debug!("Shutdown requested");
            state.running = false;
        }
    }
}

fn synchronize_power_policy(state: &mut EngineState) {
    let paused = state.config.pause_on_battery && state.on_battery;
    if paused == state.power_paused {
        return;
    }
    for session in state.sessions.values_mut() {
        session.set_power_paused(paused);
    }
    state.power_paused = paused;
    if !paused {
        wake_outputs(state, None);
    }
}

fn update_playback_settings(
    state: &mut EngineState,
    settings: PlaybackSettings,
    deadline: Instant,
) -> std::result::Result<(), String> {
    settings.validate()?;
    if Instant::now() >= deadline {
        return Err("Playback settings request expired before it could be applied".to_string());
    }
    let interval = frame_interval(settings.fps_limit).map_err(|error| error.to_string())?;
    if state.frame_interval != interval {
        for surface in state.layer_surfaces.values_mut() {
            surface.schedule.set_interval(interval);
        }
        state.frame_interval = interval;
    }
    state.config.fps_limit = settings.fps_limit;
    state.config.pause_on_battery = settings.pause_on_battery;
    state.on_battery = check_battery_status();
    state.last_battery_check = Instant::now();
    synchronize_power_policy(state);
    Ok(())
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
fn reject_output_candidate(state: &mut EngineState, output_name: &str) -> Result<bool> {
    if state
        .layer_surfaces
        .get(output_name)
        .is_some_and(|info| info.apply_request.finalized)
    {
        return release_output_resources(state, output_name);
    }
    if let (Some(session), Some(egl)) = (
        state.sessions.get_mut(output_name),
        state.egl_context.as_ref(),
    ) && session.discard_pending(egl)?
        && let Some(info) = state.layer_surfaces.get_mut(output_name)
        && let Some(previous) = info.previous_request.take()
    {
        info.apply_request = previous;
        info.frame_pending = true;
        info.wake();
        allocator::reclaim_released_pages("discarded_candidate");
        return Ok(true);
    }
    release_output_resources(state, output_name)
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
    let released = session_removed || surface_removed;
    if released {
        allocator::reclaim_released_pages("released_output");
    }
    Ok(released)
}

/// Apply wallpaper to a specific output
fn apply_wallpaper_to_output(
    state: &mut EngineState,
    request: ApplyRequest,
    output_name: &str,
    qh: &QueueHandle<EngineState>,
) -> Result<()> {
    if let Some(info) = state.layer_surfaces.get(output_name) {
        info.apply_request.ensure_supersedable()?;
    }
    let reuse = state
        .layer_surfaces
        .get(output_name)
        .is_some_and(|info| info.configured)
        && state
            .sessions
            .get(output_name)
            .is_some_and(WallpaperSession::can_reuse_resources);
    if let WallpaperSource::Scene { prepared, .. } = &request.source {
        let mut plan = combined_scene_plan(state, Some(output_name))?;
        if reuse {
            plan.merge(
                &state
                    .sessions
                    .get(output_name)
                    .context("Reusable output missing")?
                    .replacement_resource_plan()?,
            )?;
        }
        let [width, height] = state
            .layer_surfaces
            .get(output_name)
            .filter(|info| info.configured)
            .map(|info| [info.width as i32, info.height as i32])
            .or_else(|| {
                state
                    .outputs
                    .get(output_name)
                    .map(|output| [output.info.width, output.info.height])
            })
            .context("Output not found")?;
        plan.merge(&prepared.resource_plan_for_output(width, height)?)?;
        plan.preflight(state.scene_textures.retired_bytes()?, 0)?;
        let mut decoded = scene_decoded_allocations(state)?;
        decoded.extend(prepared.cpu_allocations());
        let decoded_bytes = decoded_texture_total(decoded)?;
        anyhow::ensure!(
            decoded_bytes <= crate::scene::shared_texture::PAYLOAD_LIMIT,
            "Scene retained decoded textures exceed the engine's 512 MiB CPU allocation budget"
        );
        let mut audio = state
            .sessions
            .iter()
            .filter(|(name, _)| reuse || name.as_str() != output_name)
            .flat_map(|(_, session)| session.audio_allocations())
            .collect::<std::collections::BTreeMap<_, _>>();
        audio.extend(prepared.audio_allocations());
        check_audio_payload_budget(audio.values().copied())?;
    }
    // Check if we can reuse existing layer surface (hot-swap optimization)
    if let (Some(surface_info), Some(session)) = (
        state.layer_surfaces.get_mut(output_name),
        state.sessions.get_mut(output_name),
    ) && reuse
    {
        info!(
            "Hot-swapping wallpaper for {} (reusing surface)",
            output_name
        );
        session.load_source(
            request.source.clone(),
            state
                .egl_context
                .as_ref()
                .context("EGL context unavailable")?,
        )?;
        if surface_info.previous_request.is_none() || surface_info.apply_request.committed {
            surface_info.previous_request = Some(surface_info.apply_request.clone());
        }
        surface_info.apply_request = request;
        surface_info.frame_pending = true;
        surface_info.wake();
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

    let manual_paused = state
        .sessions
        .get(output_name)
        .is_some_and(WallpaperSession::is_manual_paused);
    release_output_resources(state, output_name)?;

    info!(
        "Creating layer surface for {} ({}x{})",
        output_name, output_info.width, output_info.height
    );

    // Create wl_surface
    let wl_surface = compositor.create_surface(qh, output_name.to_string());
    // The current runtime has no interactive Scene input capability. Leave
    // desktop pointer/touch input to the surfaces below the wallpaper.
    let input_region = compositor.create_region(qh, ());
    wl_surface.set_input_region(Some(&input_region));
    input_region.destroy();

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
            schedule: FrameSchedule::default(),
            metrics: EngineRenderMetrics::default(),
            callback_epoch: 0,
            apply_request: request.clone(),
            previous_request: None,
        },
    );

    // Create wallpaper session
    let mut session =
        WallpaperSession::from_source(request.source, output_info, state.config.video.clone())?;
    session.use_texture_pool(Rc::clone(&state.scene_textures))?;
    session.use_worker_ping(state.scene_worker_ping.clone());
    session.set_power_paused(state.power_paused);
    if manual_paused {
        session.pause();
    }
    state.sessions.insert(output_name.to_string(), session);

    info!("Wallpaper session created for {}", output_name);

    Ok(())
}

fn combined_scene_plan(state: &EngineState, skip_output: Option<&str>) -> Result<ResourcePlan> {
    combine_scene_resource_plans(
        state
            .sessions
            .iter()
            .map(|(name, session)| (name.as_str(), session)),
        skip_output,
        WallpaperSession::resource_plan,
    )
}

/// Reuse keeps all generations; replacing a fresh surface releases only that
/// output before allocation. Other outputs and their reservations remain live.
fn combine_scene_resource_plans<'a, T>(
    sessions: impl IntoIterator<Item = (&'a str, T)>,
    skip_output: Option<&str>,
    mut measure: impl FnMut(T) -> Result<ResourcePlan>,
) -> Result<ResourcePlan> {
    let mut plan = ResourcePlan::default();
    for (output, session) in sessions {
        if skip_output != Some(output) {
            plan.merge(&measure(session)?)?;
        }
    }
    Ok(plan)
}

fn scene_decoded_allocations(state: &EngineState) -> Result<Vec<(usize, u64)>> {
    let mut allocations = Vec::new();
    for session in state.sessions.values() {
        allocations.extend(session.decoded_allocations()?);
    }
    // Requests also retain Arc sources independently of renderer cleanup. They
    // must be counted until their owner releases them, including rollback.
    for info in state.layer_surfaces.values() {
        for request in std::iter::once(&info.apply_request).chain(info.previous_request.iter()) {
            if let WallpaperSource::Scene { prepared, .. } = &request.source {
                allocations.extend(prepared.cpu_allocations());
            }
        }
    }
    Ok(allocations)
}

fn decoded_texture_total(allocations: impl IntoIterator<Item = (usize, u64)>) -> Result<u64> {
    let mut unique = std::collections::BTreeMap::new();
    for (identity, bytes) in allocations {
        if let Some(previous) = unique.insert(identity, bytes) {
            anyhow::ensure!(previous == bytes, "Scene decoded allocation sizes disagree");
        }
    }
    unique.values().try_fold(0_u64, |total, bytes| {
        total
            .checked_add(*bytes)
            .context("Scene decoded allocation total overflow")
    })
}

fn check_audio_payload_budget(bytes: impl IntoIterator<Item = u64>) -> Result<()> {
    let total = bytes.into_iter().try_fold(0_u64, |total, bytes| {
        total
            .checked_add(bytes)
            .context("Scene prepared audio allocation overflow")
    })?;
    anyhow::ensure!(
        total <= 256 * 1024 * 1024,
        "Scene prepared audio exceeds the active and rollback memory budget"
    );
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

impl Dispatch<WlRegion, ()> for EngineState {
    fn event(
        _state: &mut Self,
        _proxy: &WlRegion,
        _event: wl_region::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
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
                    info.wake();
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
                    && (info.apply_request.pending || info.apply_request.awaiting_commit)
                {
                    info.apply_request.pending = false;
                    info.apply_request.awaiting_commit = false;
                    let _ = state.events_tx.send(EngineEvent::ApplyFailed {
                        request_id: info.apply_request.request_id,
                        output: output_name.clone(),
                        reason: "Wallpaper layer surface closed before application completed"
                            .to_string(),
                    });
                } else {
                    let reason = format!("Wallpaper layer surface closed for {output_name}");
                    state
                        .output_errors
                        .insert(output_name.clone(), reason.clone());
                    let _ = state.events_tx.send(EngineEvent::OutputError {
                        output: output_name.clone(),
                        reason,
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
                info.metrics.callbacks_received = info.metrics.callbacks_received.saturating_add(1);
                if info.callback_epoch == data.epoch {
                    info.frame_callback_requested = false;
                    info.frame_pending = true;
                }
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

    #[test]
    fn decoded_allocations_count_arc_alias_once_and_distinct_preparations_separately() {
        assert_eq!(
            decoded_texture_total([(1, 40), (1, 40), (2, 40)]).unwrap(),
            80
        );
        assert!(decoded_texture_total([(1, 40), (1, 41)]).is_err());
        assert!(decoded_texture_total([(1, u64::MAX), (2, 1)]).is_err());
    }
    use std::path::PathBuf;
    use std::time::{Duration, Instant};

    fn policy_test_state() -> EngineState {
        let (events_tx, _) = mpsc::channel();
        EngineState {
            outputs: OutputManager::new(),
            sessions: HashMap::new(),
            scene_textures: TexturePool::new().unwrap(),
            output_errors: HashMap::new(),
            events_tx,
            config: EngineConfig::default(),
            running: true,
            pending_outputs: HashMap::new(),
            compositor: None,
            layer_shell: None,
            egl_context: None,
            layer_surfaces: HashMap::new(),
            queue_handle: None,
            on_battery: false,
            power_paused: false,
            last_battery_check: Instant::now(),
            frame_interval: None,
            scene_worker_ping: make_ping().unwrap().0,
        }
    }

    #[test]
    fn scene_ready_does_not_create_frame_demand_for_idle_or_unconfigured_outputs() {
        for demand in [
            FrameDemand::Idle,
            FrameDemand::Continuous,
            FrameDemand::Urgent,
        ] {
            assert!(!scene_retry_wanted(false, true, demand));
            assert!(!scene_retry_wanted(true, false, demand));
            assert_eq!(
                scene_retry_wanted(true, true, demand),
                demand != FrameDemand::Idle
            );
        }
        assert!(!scene_retry_wanted(
            true,
            true,
            effective_demand(true, FrameDemand::Continuous),
        ));
        assert!(scene_retry_wanted(
            true,
            true,
            effective_demand(true, FrameDemand::Urgent),
        ));
    }

    #[test]
    fn cpu_ahead_does_not_enter_unconfigured_transaction_or_idle_outputs() {
        for demand in [
            FrameDemand::Idle,
            FrameDemand::Urgent,
            FrameDemand::Continuous,
        ] {
            assert!(!scene_ahead_wanted(false, false, demand));
            assert!(!scene_ahead_wanted(true, true, demand));
            assert_eq!(
                scene_ahead_wanted(true, false, demand),
                demand == FrameDemand::Continuous
            );
        }
        assert!(!scene_ahead_wanted(
            true,
            false,
            effective_demand(true, FrameDemand::Continuous)
        ));
    }

    #[test]
    fn invalid_and_expired_live_updates_leave_the_running_policy_unchanged() {
        let mut state = policy_test_state();
        let previous = state.config.playback_settings();
        for (settings, deadline) in [
            (
                PlaybackSettings {
                    fps_limit: Some(0),
                    pause_on_battery: true,
                },
                Instant::now() + Duration::from_secs(1),
            ),
            (
                PlaybackSettings {
                    fps_limit: Some(30),
                    pause_on_battery: true,
                },
                Instant::now(),
            ),
        ] {
            let (reply, received) = mpsc::channel();
            handle_command(
                EngineCommand::UpdatePlaybackSettings {
                    settings,
                    deadline,
                    reply,
                },
                &mut state,
            );
            assert!(received.recv().unwrap().is_err());
            assert_eq!(state.config.playback_settings(), previous);
            assert_eq!(state.frame_interval, None);
        }
    }

    #[test]
    fn live_update_is_acknowledged_without_replacing_output_state() {
        let mut state = policy_test_state();
        state
            .output_errors
            .insert("existing-output".to_string(), "existing error".to_string());
        let requested = PlaybackSettings {
            fps_limit: Some(30),
            pause_on_battery: true,
        };
        let (reply, received) = mpsc::channel();
        handle_command(
            EngineCommand::UpdatePlaybackSettings {
                settings: requested,
                deadline: Instant::now() + Duration::from_secs(1),
                reply,
            },
            &mut state,
        );
        assert_eq!(received.recv().unwrap(), Ok(()));
        assert_eq!(state.config.playback_settings(), requested);
        assert_eq!(state.frame_interval, frame_interval(Some(30)).unwrap());
        assert!(state.running);
        assert_eq!(state.output_errors["existing-output"], "existing error");
    }

    #[test]
    fn disabling_battery_pause_releases_power_pause_but_keeps_manual_pause() {
        let mut state = policy_test_state();
        let mut session = WallpaperSession::new(
            "/authored/video.mp4".into(),
            lwe_core::OutputInfo {
                name: "authored-output".to_string(),
                width: 32,
                height: 32,
                scale: 1.0,
                position: (0, 0),
                active: true,
                hdr_capabilities: Default::default(),
            },
            crate::mpv::VideoConfig::default(),
        )
        .unwrap();
        session.pause();
        state.sessions.insert("authored-output".into(), session);
        state.on_battery = true;
        state.config.pause_on_battery = true;
        synchronize_power_policy(&mut state);
        assert!(state.power_paused);
        state.config.pause_on_battery = false;
        synchronize_power_policy(&mut state);
        assert!(!state.power_paused);
        let session = &state.sessions["authored-output"];
        assert!(session.is_manual_paused());
        assert_eq!(session.state(), session::PlaybackState::Paused);
        assert_eq!(session.wallpaper_path(), Some("/authored/video.mp4"));
    }

    #[test]
    fn static_paused_and_empty_frames_do_not_request_more_callbacks() {
        assert!(!should_request_callback(
            true,
            FrameDemand::Idle,
            false,
            false
        ));
        assert!(!should_request_callback(
            false,
            FrameDemand::Continuous,
            false,
            false
        ));
        assert!(!should_request_callback(
            true,
            FrameDemand::Continuous,
            true,
            false
        ));
        assert!(!should_request_callback(
            true,
            FrameDemand::Continuous,
            false,
            true
        ));
        assert!(should_request_callback(
            true,
            FrameDemand::Continuous,
            false,
            false
        ));
    }

    #[test]
    fn power_pause_preserves_first_frame_resize_and_rollback_demands() {
        assert_eq!(
            effective_demand(true, FrameDemand::Urgent),
            FrameDemand::Urgent
        );
        assert_eq!(
            effective_demand(true, FrameDemand::Continuous),
            FrameDemand::Idle
        );
        assert_eq!(
            effective_demand(false, FrameDemand::Continuous),
            FrameDemand::Continuous
        );
    }

    #[test]
    fn engine_scene_budget_counts_retained_resources_and_rejects_overflow() {
        let mib = 1024 * 1024;
        let mut retained = private_plan(300 * mib);
        retained.merge(&private_plan(212 * mib)).unwrap();
        assert!(retained.preflight(0, 0).is_ok());
        assert!(retained.preflight(0, mib).is_err());
        assert!(private_plan(u64::MAX).merge(&private_plan(1)).is_err());
    }

    fn private_plan(bytes: u64) -> ResourcePlan {
        ResourcePlan {
            private_bytes: bytes,
            ..Default::default()
        }
    }

    #[test]
    fn audio_payload_budget_checks_total_and_overflow_separately_from_gpu() {
        let mib = 1024 * 1024;
        assert!(check_audio_payload_budget([128 * mib, 128 * mib]).is_ok());
        assert!(check_audio_payload_budget([128 * mib, 128 * mib, 1]).is_err());
        assert!(check_audio_payload_budget([u64::MAX, 1]).is_err());
    }

    #[test]
    fn fresh_output_replacement_discards_only_its_previous_reservation() {
        let mib = 1024 * 1024;
        let mut retained =
            combine_scene_resource_plans([("eDP-1", private_plan(400 * mib))], Some("eDP-1"), Ok)
                .unwrap();
        assert_eq!(retained.total(0).unwrap(), 0);
        retained.merge(&private_plan(400 * mib)).unwrap();
        assert!(retained.preflight(0, 0).is_ok());
        let mut retained = combine_scene_resource_plans(
            [("eDP-1", 400 * mib), ("DP-1", 120 * mib)],
            Some("eDP-1"),
            |bytes| Ok(private_plan(bytes)),
        )
        .unwrap();
        assert_eq!(retained.total(0).unwrap(), 120 * mib);
        retained.merge(&private_plan(400 * mib)).unwrap();
        assert!(retained.preflight(0, 0).is_err());
        // The discarded reservation must not even be evaluated.
        assert!(
            combine_scene_resource_plans([("eDP-1", ())], Some("eDP-1"), |_| anyhow::bail!(
                "discarded estimate"
            ),)
            .is_ok()
        );
    }

    #[test]
    fn reused_outputs_include_current_pending_and_rollback_reservations() {
        let mib = 1024 * 1024;
        let retained = combine_scene_resource_plans(
            [
                ("eDP-1", [200 * mib, 60 * mib, 80 * mib]),
                ("DP-1", [100 * mib, 0, 0]),
            ],
            None,
            |parts| {
                let mut generations = ResourcePlan::default();
                for bytes in parts {
                    generations.merge(&private_plan(bytes))?;
                }
                Ok(generations)
            },
        )
        .unwrap();
        assert_eq!(retained.total(0).unwrap(), 440 * mib);
        assert!(retained.preflight(0, 72 * mib).is_ok());
        assert!(retained.preflight(0, 73 * mib).is_err());
        assert!(
            combine_scene_resource_plans([("eDP-1", u64::MAX), ("DP-1", 1)], None, |bytes| Ok(
                private_plan(bytes)
            ),)
            .is_err()
        );
    }

    #[test]
    fn invalid_fps_is_rejected_before_starting_a_wayland_thread() {
        let config = EngineConfig {
            fps_limit: Some(0),
            ..EngineConfig::default()
        };
        assert!(
            spawn_engine(config)
                .err()
                .unwrap()
                .to_string()
                .contains("greater than zero")
        );
    }

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
        assert!(request.expired(deadline));
        assert!(request.owns(7));
    }

    #[test]
    fn unaccepted_ack_expires_but_committed_presentation_does_not() {
        let now = Instant::now();
        let deadline = now + Duration::from_secs(1);
        let mut request = ApplyRequest::new(7, PathBuf::from("/tmp/video.mp4"), deadline);
        assert!(request.commit(now).is_err());
        request.complete("eDP-1", now).unwrap();
        assert!(request.awaiting_commit);
        assert!(request.expired(deadline));
        assert!(request.commit(deadline).is_err());
        assert!(matches!(
            request.complete("eDP-1", deadline),
            Some(EngineEvent::ApplyFailed { .. })
        ));

        let mut request = ApplyRequest::new(8, PathBuf::from("/tmp/video.mp4"), deadline);
        request.complete("eDP-1", now).unwrap();
        request.commit(now).unwrap();
        request.commit(now).unwrap();
        assert!(!request.expired(deadline));
        assert!(request.committed);
    }

    #[test]
    fn failed_candidate_cannot_commit_or_replace_rollback_identity() {
        let mut request = ApplyRequest::new(
            9,
            PathBuf::from("/tmp/video.mp4"),
            Instant::now() + Duration::from_secs(1),
        );
        request.pending = false;
        request.awaiting_commit = false;
        assert!(!request.committed);
        assert!(request.commit(Instant::now()).is_err());
    }

    #[test]
    fn committed_unfinalized_request_rejects_superseding_and_retains_rollback_ownership() {
        let now = Instant::now();
        let deadline = now + Duration::from_secs(1);
        let mut saved = ApplyRequest::new(6, "/authored/saved.mp4".into(), deadline);
        saved.complete("eDP-1", now).unwrap();
        saved.commit(now).unwrap();
        saved.finalized = true;
        let previous_request = Some(saved);

        let mut current = ApplyRequest::new(7, "/authored/committed.mp4".into(), deadline);
        current.complete("eDP-1", now).unwrap();
        current.commit(now).unwrap();
        let rejected = ApplyRequest::new(8, "/authored/rejected.mp4".into(), deadline);
        let error = current.ensure_supersedable().unwrap_err();
        assert!(
            error
                .to_string()
                .contains("awaiting persistence finalization")
        );

        // Rejection must leave the committed generation cancellable, so a save
        // failure can still restore the exact retained previous generation.
        assert!(current.owns(7));
        assert!(!current.owns(rejected.request_id));
        assert!(current.cancellable(7));
        assert!(current.committed && !current.finalized);
        assert_eq!(current.path, PathBuf::from("/authored/committed.mp4"));
        assert_eq!(current.source.path(), current.path);
        let retained = previous_request.as_ref().unwrap();
        assert!(retained.owns(6));
        assert!(retained.finalized);
        assert_eq!(retained.path, PathBuf::from("/authored/saved.mp4"));
        assert_eq!(retained.source.path(), retained.path);

        // Finalizing releases the exclusivity; the retained request is not
        // consumed by the read-only guard itself.
        current.finalized = true;
        current.ensure_supersedable().unwrap();
        assert!(!current.cancellable(7));
        assert!(previous_request.as_ref().unwrap().owns(6));
    }

    #[test]
    fn uncommitted_and_finalized_requests_allow_superseding() {
        let now = Instant::now();
        let deadline = now + Duration::from_secs(1);
        let pending = ApplyRequest::new(10, "/authored/pending.mp4".into(), deadline);
        assert!(pending.pending);
        pending.ensure_supersedable().unwrap();

        let mut acknowledged = ApplyRequest::new(11, "/authored/acknowledged.mp4".into(), deadline);
        acknowledged.complete("eDP-1", now).unwrap();
        assert!(acknowledged.awaiting_commit && !acknowledged.committed);
        acknowledged.ensure_supersedable().unwrap();

        acknowledged.commit(now).unwrap();
        assert!(acknowledged.ensure_supersedable().is_err());
        acknowledged.finalized = true;
        acknowledged.ensure_supersedable().unwrap();
        assert!(acknowledged.committed && acknowledged.finalized);
        assert!(acknowledged.owns(11));
    }

    #[test]
    fn delayed_cancel_cannot_remove_a_saved_finalized_generation() {
        let now = Instant::now();
        let mut request = ApplyRequest::new(
            10,
            PathBuf::from("/tmp/video.mp4"),
            now + Duration::from_secs(1),
        );
        request.complete("eDP-1", now).unwrap();
        request.commit(now).unwrap();
        assert!(request.cancellable(10));
        request.finalized = true;
        assert!(!request.cancellable(10));
        request.commit(now).unwrap();
        assert!(!request.cancellable(10));
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
