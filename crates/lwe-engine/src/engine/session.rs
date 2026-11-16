//! Per-output playback with CPU-prepared Scene resources and a shared EGL surface.

use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use calloop::ping::Ping;
use lwe_core::OutputInfo;
use wayland_client::protocol::wl_surface::WlSurface;

use super::schedule::EmptyFrameRetry;
use super::{EngineRenderMetrics, WallpaperSource};
use crate::egl::{EglContext, EglWindow};
use crate::mpv::{MpvPlayer, VideoConfig};
use crate::scene::shared_texture::{ResourcePlan, TexturePool};
use crate::scene::worker_wake::WorkerWake;
use crate::scene::{SceneAudio, SceneRenderer};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackState {
    Stopped,
    Playing,
    Paused,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FrameDemand {
    Idle,
    Continuous,
    Urgent,
}

fn frame_demand(
    state: PlaybackState,
    first_frame: bool,
    redraw: bool,
    resized: bool,
    continuous: bool,
) -> FrameDemand {
    if first_frame || redraw || resized {
        FrameDemand::Urgent
    } else if state == PlaybackState::Playing && continuous {
        FrameDemand::Continuous
    } else {
        FrameDemand::Idle
    }
}

fn frame_retry(source: &WallpaperSource, pending: Option<&WallpaperSource>) -> EmptyFrameRetry {
    if pending.unwrap_or(source).is_scene() {
        EmptyFrameRetry::ScenePreparation
    } else {
        EmptyFrameRetry::Decoder
    }
}

fn presentation_renderer<'a, T>(current: Option<&'a T>, pending: Option<&'a T>) -> Option<&'a T> {
    pending.or(current)
}

#[derive(Debug)]
struct SceneClock {
    elapsed: Duration,
    started: Option<Instant>,
}

impl SceneClock {
    fn new() -> Self {
        Self {
            elapsed: Duration::ZERO,
            started: Some(Instant::now()),
        }
    }

    fn seconds(&self) -> f64 {
        (self.elapsed + self.started.map(|t| t.elapsed()).unwrap_or_default()).as_secs_f64()
    }

    fn pause(&mut self) {
        if let Some(started) = self.started.take() {
            self.elapsed += started.elapsed();
        }
    }

    fn resume(&mut self) {
        if self.started.is_none() {
            self.started = Some(Instant::now());
        }
    }
}

enum RendererInstance {
    Video(MpvPlayer),
    Scene {
        renderer: Box<SceneRenderer>,
        clock: SceneClock,
        animated: bool,
        drawn: bool,
        audio: SceneAudio,
        audio_error: Option<String>,
    },
}

impl RendererInstance {
    fn prepare_scene_ahead(&mut self, allow_timed: bool) -> Result<()> {
        if let Self::Scene {
            renderer,
            clock,
            animated,
            drawn,
            audio,
            ..
        } = self
            && *animated
            && *drawn
            && audio.poll_ready()?
        {
            renderer.prepare_ahead(clock.seconds(), allow_timed)?;
        }
        Ok(())
    }
    fn cancel_scene_ahead(&mut self) {
        if let Self::Scene { renderer, .. } = self {
            renderer.cancel_ahead_timed();
        }
    }
    fn allocation_plan(&self) -> Result<ResourcePlan> {
        match self {
            Self::Video(_) => Ok(ResourcePlan::default()),
            Self::Scene { renderer, .. } => renderer.allocation_plan(),
        }
    }

    fn additional_gpu_bytes(&self, width: i32, height: i32) -> Result<u64> {
        match self {
            Self::Video(_) => Ok(0),
            Self::Scene { renderer, .. } => renderer.additional_gpu_bytes(width, height),
        }
    }

    fn continuous(&self) -> bool {
        match self {
            Self::Video(_) => true,
            Self::Scene { animated, .. } => *animated,
        }
    }

    fn create(
        source: &WallpaperSource,
        config: &VideoConfig,
        output: &OutputInfo,
        egl: &EglContext,
        texture_pool: Rc<TexturePool>,
        worker_ping: Option<Ping>,
    ) -> Result<Self> {
        match source {
            WallpaperSource::Video { path } => {
                let mut config = config.clone();
                config.source = path.to_string_lossy().into_owned();
                config.volume = 0.0;
                let mut player = MpvPlayer::new(&config, output)?;
                player.init_render_context(egl)?;
                Ok(Self::Video(player))
            }
            WallpaperSource::Scene { prepared, .. } => Ok(Self::Scene {
                renderer: Box::new(
                    prepared.initialize_shared(texture_pool, worker_ping.map(WorkerWake::new))?,
                ),
                clock: SceneClock::new(),
                animated: prepared.summary().animated,
                drawn: false,
                audio: prepared.initialize_audio()?,
                audio_error: None,
            }),
        }
    }

    fn render(
        &mut self,
        width: i32,
        height: i32,
        force: bool,
        resized: bool,
        redraw: bool,
        allow_canvas_replacement: bool,
    ) -> Result<bool> {
        match self {
            Self::Video(player) => {
                let ready = player.consume_frame_update(force);
                // libmpv can redraw its previous frame even while paused.
                if !ready && !redraw && !resized {
                    return Ok(false);
                }
                unsafe {
                    gl::ClearColor(0.0, 0.0, 0.0, 1.0);
                    gl::Clear(gl::COLOR_BUFFER_BIT);
                    gl::Viewport(0, 0, width, height);
                }
                player.render(width, height, 0)
            }
            Self::Scene {
                renderer,
                clock,
                animated,
                drawn,
                audio,
                ..
            } => {
                if !audio.poll_ready()? {
                    return Ok(false);
                }
                if *drawn && !*animated && !force && !resized && !redraw {
                    return Ok(false);
                }
                renderer.set_canvas_replacement_allowed(allow_canvas_replacement);
                let rendered = if redraw || force || resized {
                    renderer.render_forced(width, height, clock.seconds())
                } else {
                    renderer.render(width, height, clock.seconds())
                };
                renderer.set_canvas_replacement_allowed(false);
                let rendered = rendered?;
                *drawn |= rendered;
                Ok(rendered)
            }
        }
    }

    fn report_swap(&mut self) {
        match self {
            Self::Video(player) => player.report_swap(),
            Self::Scene { renderer, .. } => renderer.report_presented(),
        }
    }

    fn report_swap_failed(&mut self) {
        if let Self::Scene { renderer, .. } = self {
            renderer.report_present_failed();
        }
    }
    fn pause(&mut self) {
        match self {
            Self::Video(player) => {
                let _ = player.pause();
            }
            Self::Scene {
                renderer,
                clock,
                audio,
                audio_error,
                ..
            } => {
                clock.pause();
                renderer.pause();
                if let Err(error) = audio.set_paused(true) {
                    *audio_error = Some(format!("Cannot pause Scene audio: {error:#}"));
                }
            }
        }
    }

    fn resume(&mut self) {
        match self {
            Self::Video(player) => {
                let _ = player.resume();
            }
            Self::Scene {
                renderer,
                clock,
                audio,
                audio_error,
                ..
            } => {
                clock.resume();
                renderer.resume();
                if let Err(error) = audio.set_paused(false) {
                    *audio_error = Some(format!("Cannot resume Scene audio: {error:#}"));
                }
            }
        }
    }

    fn volume(&mut self, volume: f32) {
        match self {
            Self::Video(player) => {
                let _ = player.set_volume(volume as f64);
            }
            Self::Scene {
                audio, audio_error, ..
            } => {
                if let Err(error) = audio.set_master_volume(volume as f64) {
                    *audio_error = Some(format!("Cannot update Scene audio volume: {error:#}"));
                }
            }
        }
    }

    fn deactivate_audio(&mut self) -> Result<()> {
        if let Self::Scene {
            renderer, audio, ..
        } = self
        {
            audio.deactivate()?;
            renderer.deactivate_analysis();
        }
        Ok(())
    }

    fn freeze_for_rollback(&mut self) -> Result<()> {
        match self {
            Self::Video(_) => {
                self.volume(0.0);
                self.pause();
            }
            Self::Scene {
                renderer, clock, ..
            } => {
                renderer.freeze_for_rollback()?;
                clock.pause();
            }
        }
        Ok(())
    }

    fn rollback_audio_pause(&mut self, paused: bool) {
        if let Self::Scene {
            audio, audio_error, ..
        } = self
            && let Err(error) = audio.set_paused(paused)
        {
            *audio_error = Some(format!(
                "Cannot update rollback Scene audio pause: {error:#}"
            ));
        }
    }

    fn activate_audio(&mut self, paused: bool, volume: f32) -> Result<()> {
        if let Self::Scene {
            renderer, audio, ..
        } = self
        {
            audio.activate(paused, volume as f64)?;
            renderer.activate_analysis(paused);
        }
        Ok(())
    }

    fn poll_audio_health(&mut self) -> Result<()> {
        if let Self::Scene {
            audio, audio_error, ..
        } = self
        {
            if let Some(error) = audio_error.as_ref() {
                anyhow::bail!("{error}");
            }
            if let Err(error) = audio.poll_ready() {
                *audio_error = Some(format!("Scene audio failed: {error:#}"));
                return Err(error);
            }
        }
        Ok(())
    }
}

/// A renderer and a replaceable candidate share one LWE-owned output surface.
pub struct WallpaperSession {
    output_info: OutputInfo,
    source: WallpaperSource,
    video_config: VideoConfig,
    renderer: Option<RendererInstance>,
    pending: Option<(WallpaperSource, RendererInstance)>,
    rollback: Option<(WallpaperSource, RendererInstance)>,
    egl_window: Option<EglWindow>,
    texture_pool: Option<Rc<TexturePool>>,
    worker_ping: Option<Ping>,
    state: PlaybackState,
    manual_paused: bool,
    power_paused: bool,
    volume: f32,
    volume_override: Option<f32>,
    gl_loaded: bool,
    redraw_needed: bool,
    metrics: EngineRenderMetrics,
}

impl WallpaperSession {
    pub fn new(path: PathBuf, output_info: OutputInfo, video_config: VideoConfig) -> Result<Self> {
        Self::from_source(WallpaperSource::Video { path }, output_info, video_config)
    }

    pub fn from_source(
        source: WallpaperSource,
        output_info: OutputInfo,
        video_config: VideoConfig,
    ) -> Result<Self> {
        let volume = video_config.volume.clamp(0.0, 1.0) as f32;
        Ok(Self {
            output_info,
            source,
            video_config,
            renderer: None,
            pending: None,
            rollback: None,
            egl_window: None,
            texture_pool: None,
            worker_ping: None,
            state: PlaybackState::Stopped,
            manual_paused: false,
            power_paused: false,
            volume,
            volume_override: None,
            gl_loaded: false,
            redraw_needed: false,
            metrics: EngineRenderMetrics::default(),
        })
    }

    pub(super) fn render_demand(&self, width: i32, height: i32, first_frame: bool) -> FrameDemand {
        let resized = self
            .egl_window
            .as_ref()
            .is_some_and(|window| window.width() != width || window.height() != height);
        let renderer = presentation_renderer(
            self.renderer.as_ref(),
            self.pending.as_ref().map(|(_, renderer)| renderer),
        );
        frame_demand(
            self.state,
            first_frame,
            self.redraw_needed,
            resized,
            renderer.is_some_and(RendererInstance::continuous),
        )
    }

    pub(super) fn metrics(&self) -> EngineRenderMetrics {
        let mut metrics = self.metrics;
        metrics.scene_payload_bytes = self.scene_payload_bytes();
        metrics
    }

    pub(super) fn empty_frame_retry(&self) -> EmptyFrameRetry {
        frame_retry(
            &self.source,
            self.pending.as_ref().map(|(source, _)| source),
        )
    }

    /// Only the renderer currently selected for presentation may accelerate a
    /// retry. A frozen rollback/cancelled candidate's late ping is irrelevant.
    pub(super) fn take_worker_ready(&mut self) -> Option<Duration> {
        let renderer = presentation_renderer(
            self.renderer.as_ref(),
            self.pending.as_ref().map(|(_, renderer)| renderer),
        );
        let latency = match renderer? {
            RendererInstance::Scene { renderer, .. } => renderer.take_worker_ready()?,
            RendererInstance::Video(_) => return None,
        };
        self.metrics.record_scene_worker_dispatch(latency);
        Some(latency)
    }

    pub(super) fn use_worker_ping(&mut self, ping: Ping) {
        self.worker_ping = Some(ping);
    }

    pub(super) fn prepare_scene_ahead(&mut self, allow_timed: bool) -> Result<()> {
        if self.state == PlaybackState::Playing
            && self.pending.is_none()
            && self.rollback.is_none()
            && let Some(renderer) = &mut self.renderer
        {
            renderer.prepare_scene_ahead(allow_timed)?;
        }
        Ok(())
    }

    pub(super) fn cancel_scene_ahead(&mut self) {
        for renderer in self
            .renderer
            .iter_mut()
            .chain(self.pending.iter_mut().map(|(_, r)| r))
            .chain(self.rollback.iter_mut().map(|(_, r)| r))
        {
            renderer.cancel_scene_ahead();
        }
    }

    pub(crate) fn use_texture_pool(&mut self, pool: Rc<TexturePool>) -> Result<()> {
        anyhow::ensure!(
            self.renderer.is_none() && self.pending.is_none() && self.rollback.is_none(),
            "Cannot change an initialized Scene texture domain"
        );
        self.texture_pool = Some(pool);
        Ok(())
    }

    pub(super) fn resource_plan(&self) -> Result<ResourcePlan> {
        if self.renderer.is_none()
            && let WallpaperSource::Scene { prepared, .. } = &self.source
        {
            return prepared
                .resource_plan_for_output(self.output_info.width, self.output_info.height);
        }
        let mut plan = ResourcePlan::default();
        for renderer in self
            .renderer
            .iter()
            .chain(self.pending.iter().map(|(_, renderer)| renderer))
            .chain(self.rollback.iter().map(|(_, renderer)| renderer))
        {
            plan.merge(&renderer.allocation_plan()?)?;
        }
        Ok(plan)
    }

    pub(super) fn replacement_resource_plan(&self) -> Result<ResourcePlan> {
        // An uncommitted pending candidate is discarded before allocation. The
        // saved/current generation alone remains available for rollback.
        let mut plan = ResourcePlan::default();
        for renderer in self
            .renderer
            .iter()
            .chain(self.rollback.iter().map(|(_, r)| r))
        {
            let measured = match renderer {
                RendererInstance::Scene { renderer, .. } => renderer.frozen_allocation_plan()?,
                RendererInstance::Video(_) => ResourcePlan::default(),
            };
            plan.merge(&measured)?;
        }
        Ok(plan)
    }
    fn scene_payload_bytes(&self) -> u64 {
        self.resource_plan()
            .and_then(|plan| plan.total(0))
            .unwrap_or(u64::MAX)
    }

    pub(super) fn reserved_scene_payload_bytes(&self) -> Result<u64> {
        self.resource_plan()?.total(0)
    }

    pub(super) fn additional_scene_payload_bytes(&self, width: i32, height: i32) -> Result<u64> {
        if let Some((_, renderer)) = &self.pending {
            return renderer.additional_gpu_bytes(width, height);
        }
        if let Some(renderer) = &self.renderer {
            return renderer.additional_gpu_bytes(width, height);
        }
        if let WallpaperSource::Scene { prepared, .. } = &self.source {
            return Ok(prepared
                .estimated_gpu_bytes_for_output(width, height)?
                .saturating_sub(self.reserved_scene_payload_bytes()?));
        }
        Ok(0)
    }

    pub(super) fn audio_allocations(&self) -> Vec<(usize, u64)> {
        std::iter::once(&self.source)
            .chain(self.pending.as_ref().map(|(source, _)| source))
            .chain(self.rollback.as_ref().map(|(source, _)| source))
            .flat_map(|source| match source {
                WallpaperSource::Scene { prepared, .. } => {
                    prepared.audio_allocations().collect::<Vec<_>>()
                }
                WallpaperSource::Video { .. } => Vec::new(),
            })
            .collect()
    }

    pub(super) fn decoded_allocations(&self) -> Result<Vec<(usize, u64)>> {
        let mut allocations: Vec<_> = std::iter::once(&self.source)
            .chain(self.pending.as_ref().map(|(source, _)| source))
            .chain(self.rollback.as_ref().map(|(source, _)| source))
            .flat_map(|source| match source {
                WallpaperSource::Scene { prepared, .. } => {
                    prepared.cpu_allocations().collect::<Vec<_>>()
                }
                WallpaperSource::Video { .. } => Vec::new(),
            })
            .collect();
        for renderer in self
            .renderer
            .iter()
            .chain(self.pending.as_ref().map(|(_, r)| r))
            .chain(self.rollback.as_ref().map(|(_, r)| r))
        {
            if let RendererInstance::Scene { renderer, .. } = renderer {
                allocations.extend(renderer.retained_history_allocations());
            }
        }
        Ok(allocations)
    }

    pub(super) fn poll_audio_health(&mut self) -> Result<()> {
        if let Some(renderer) = &mut self.renderer {
            renderer.poll_audio_health()?;
        }
        if let Some((_, renderer)) = &mut self.rollback {
            renderer.poll_audio_health()?;
        }
        Ok(())
    }

    fn initialize_resources(
        &mut self,
        egl: &EglContext,
        surface: &WlSurface,
        width: i32,
        height: i32,
    ) -> Result<()> {
        if self.renderer.is_some() {
            return Ok(());
        }
        if self.egl_window.is_none() {
            self.egl_window = Some(egl.create_window(surface, width, height)?);
        }
        egl.make_current(
            self.egl_window
                .as_ref()
                .context("Renderer has no EGL window")?,
        )?;
        if !self.gl_loaded {
            egl.load_gl_functions();
            self.gl_loaded = true;
        }
        if self.texture_pool.is_none() {
            self.texture_pool = Some(TexturePool::new()?);
        }
        self.texture_pool
            .as_ref()
            .expect("texture domain initialized")
            .bind_owner(egl)?;
        let mut renderer = RendererInstance::create(
            &self.source,
            &self.video_config,
            &self.output_info,
            egl,
            Rc::clone(
                self.texture_pool
                    .as_ref()
                    .expect("texture domain initialized"),
            ),
            self.worker_ping.clone(),
        )?;
        renderer.volume(self.source_volume(&self.source));
        let paused = self.manual_paused || self.power_paused;
        if paused {
            renderer.pause();
        }
        self.renderer = Some(renderer);
        self.state = if paused {
            PlaybackState::Paused
        } else {
            PlaybackState::Playing
        };
        Ok(())
    }

    pub fn render_frame_to_surface(
        &mut self,
        egl: &EglContext,
        surface: &WlSurface,
        width: i32,
        height: i32,
        force: bool,
    ) -> Result<bool> {
        self.render_frame_to_surface_inner(egl, surface, width, height, force, false)
    }

    // The engine owns this context and serializes every GL producer on one
    // thread. The public entry point cannot make that guarantee.
    pub(super) fn render_frame_to_surface_owned(
        &mut self,
        egl: &EglContext,
        surface: &WlSurface,
        width: i32,
        height: i32,
        force: bool,
    ) -> Result<bool> {
        self.render_frame_to_surface_inner(egl, surface, width, height, force, true)
    }

    fn render_frame_to_surface_inner(
        &mut self,
        egl: &EglContext,
        surface: &WlSurface,
        width: i32,
        height: i32,
        force: bool,
        engine_owned: bool,
    ) -> Result<bool> {
        if self.state == PlaybackState::Stopped && self.egl_window.is_some() {
            return Ok(false);
        }
        self.initialize_resources(egl, surface, width, height)?;
        let is_pending = self.pending.is_some();
        let window = self
            .egl_window
            .as_mut()
            .context("Renderer has no EGL window")?;
        let resized = window.width() != width || window.height() != height;
        if self.state != PlaybackState::Playing
            && !is_pending
            && !self.redraw_needed
            && !resized
            && !force
        {
            return Ok(false);
        }
        if resized {
            window.resize(width, height)?;
        }
        egl.make_current(window)?;
        let renderer = if let Some((_, candidate)) = &mut self.pending {
            candidate
        } else {
            self.renderer.as_mut().context("Renderer unavailable")?
        };
        self.metrics.render_attempts = self.metrics.render_attempts.saturating_add(1);
        let frame_started = Instant::now();
        if renderer.render(
            width,
            height,
            force || is_pending,
            resized,
            self.redraw_needed,
            engine_owned && !egl.has_foreign_gl(),
        )? {
            self.metrics.draws = self.metrics.draws.saturating_add(1);
            if let Err(error) = egl.swap_buffers(window) {
                renderer.report_swap_failed();
                return Err(error);
            }
            self.metrics.swaps = self.metrics.swaps.saturating_add(1);
            self.metrics.record_frame_duration(frame_started.elapsed());
            renderer.report_swap();
            self.redraw_needed = false;
            return Ok(true);
        }
        self.metrics.empty_frames = self.metrics.empty_frames.saturating_add(1);
        Ok(false)
    }

    /// Build a candidate with the correct context current; retain the old renderer.
    pub fn load_source(&mut self, source: WallpaperSource, egl: &EglContext) -> Result<()> {
        let window = self
            .egl_window
            .as_ref()
            .context("Cannot replace uninitialized renderer")?;
        egl.make_current(window)?;
        self.pending = None;
        if let Some(pool) = &self.texture_pool {
            pool.drain_retired()?;
        }
        if let Some(previous) = &mut self.renderer {
            previous.freeze_for_rollback()?;
        }
        let candidate = RendererInstance::create(
            &source,
            &self.video_config,
            &self.output_info,
            egl,
            Rc::clone(
                self.texture_pool
                    .as_ref()
                    .context("Scene texture domain unavailable")?,
            ),
            self.worker_ping.clone(),
        );
        let mut candidate = match candidate {
            Ok(candidate) => candidate,
            Err(error) => {
                if let Some(previous) = &mut self.renderer {
                    if self.state == PlaybackState::Playing {
                        previous.resume();
                    } else if let RendererInstance::Scene { renderer, .. } = previous {
                        renderer.thaw(true);
                    }
                }
                return Err(error);
            }
        };
        candidate.volume(0.0);
        if self.state == PlaybackState::Paused {
            candidate.pause();
        }
        self.rollback = None;
        self.pending = Some((source, candidate));
        Ok(())
    }

    /// Called after the host accepts the first frame. Rollback remains possible
    /// until the host confirms the commit and saves the assignment.
    pub(crate) fn accept_pending(&mut self) {
        if let Some((source, mut renderer)) = self.pending.take() {
            renderer.volume(self.source_volume(&source));
            let previous_source = std::mem::replace(&mut self.source, source);
            if let Some(previous) = self.renderer.replace(renderer) {
                self.rollback = Some((previous_source, previous));
            }
        }
    }

    pub(crate) fn finalize_pending(&mut self, egl: &EglContext) -> Result<()> {
        egl.make_current(
            self.egl_window
                .as_ref()
                .context("Candidate has no EGL window")?,
        )?;
        if let Some((_, previous)) = &mut self.rollback {
            previous.deactivate_audio()?;
        }
        let volume = self.source_volume(&self.source);
        if let Some(renderer) = &mut self.renderer {
            renderer.activate_audio(self.manual_paused || self.power_paused, volume)?;
        }
        self.rollback = None;
        Ok(())
    }

    pub(crate) fn discard_pending(&mut self, egl: &EglContext) -> Result<bool> {
        if self.pending.is_none() && self.rollback.is_none() {
            return Ok(false);
        }
        egl.make_current(
            self.egl_window
                .as_ref()
                .context("Candidate has no EGL window")?,
        )?;
        self.pending = None;
        if let Some((source, mut renderer)) = self.rollback.take() {
            renderer.volume(self.source_volume(&source));
            if self.state == PlaybackState::Playing {
                renderer.resume();
            }
            self.source = source;
            self.renderer = Some(renderer);
        }
        if let Some(RendererInstance::Scene { renderer, .. }) = &mut self.renderer {
            renderer.thaw(self.state == PlaybackState::Paused);
        }
        if self.state == PlaybackState::Playing
            && let Some(renderer) = &mut self.renderer
        {
            renderer.resume();
        }
        self.redraw_needed = true;
        Ok(true)
    }

    pub fn pause(&mut self) {
        self.manual_paused = true;
        self.synchronize_pause();
    }

    pub fn resume(&mut self) {
        self.manual_paused = false;
        self.synchronize_pause();
    }

    pub(super) fn set_power_paused(&mut self, paused: bool) {
        self.power_paused = paused;
        self.synchronize_pause();
    }

    pub(super) fn is_manual_paused(&self) -> bool {
        self.manual_paused
    }

    /// Policy pause never overwrites the user's intent. The renderer clock and
    /// decoder only resume when both factors allow playback; rollback stays
    /// hidden and is resumed by discard_pending only if effective playback is on.
    fn synchronize_pause(&mut self) {
        if self.manual_paused || self.power_paused {
            if self.state == PlaybackState::Paused {
                return;
            }
            if let Some(renderer) = &mut self.renderer {
                renderer.pause();
            }
            if let Some((_, renderer)) = &mut self.pending {
                renderer.pause();
            }
            if let Some((_, renderer)) = &mut self.rollback {
                renderer.pause();
            }
            self.state = PlaybackState::Paused;
        } else if self.state == PlaybackState::Paused {
            if let Some(renderer) = &mut self.renderer {
                renderer.resume();
            }
            if let Some((_, renderer)) = &mut self.pending {
                renderer.resume();
            }
            if let Some((_, renderer)) = &mut self.rollback {
                renderer.rollback_audio_pause(false);
            }
            self.state = PlaybackState::Playing;
        }
    }

    pub fn set_volume(&mut self, volume: f32) {
        self.volume = volume.clamp(0.0, 1.0);
        self.volume_override = Some(self.volume);
        if let Some(renderer) = &mut self.renderer {
            renderer.volume(self.volume);
        }
        if let Some((_, renderer)) = &mut self.rollback
            && matches!(renderer, RendererInstance::Scene { .. })
        {
            renderer.volume(self.volume);
        }
    }

    fn source_volume(&self, source: &WallpaperSource) -> f32 {
        self.volume_override.unwrap_or(match source {
            WallpaperSource::Scene { .. } => 1.0,
            WallpaperSource::Video { .. } => self.volume,
        })
    }

    pub fn wallpaper_path(&self) -> Option<&str> {
        self.source.path().to_str()
    }
    pub fn state(&self) -> PlaybackState {
        self.state
    }
    pub fn can_reuse_resources(&self) -> bool {
        self.renderer.is_some() && self.egl_window.is_some() && self.state != PlaybackState::Stopped
    }
    pub fn output_name(&self) -> &str {
        &self.output_info.name
    }

    pub fn cleanup_egl(&mut self, egl: &EglContext) -> Result<()> {
        if self.renderer.is_some() || self.pending.is_some() || self.rollback.is_some() {
            egl.make_current(
                self.egl_window
                    .as_ref()
                    .context("Renderer has no EGL window during cleanup")?,
            )?;
        }
        self.pending = None;
        self.rollback = None;
        self.renderer = None;
        self.state = PlaybackState::Stopped;
        if let Some(window) = &self.egl_window {
            egl.destroy_surface(window)?;
        }
        self.egl_window = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        EmptyFrameRetry, FrameDemand, PlaybackState, SceneClock, WallpaperSession, WallpaperSource,
        frame_demand, frame_retry, presentation_renderer,
    };

    fn authored_session() -> WallpaperSession {
        WallpaperSession::new(
            "/authored/video.mp4".into(),
            lwe_core::OutputInfo {
                name: "authored-output".into(),
                width: 32,
                height: 32,
                scale: 1.0,
                position: (0, 0),
                active: true,
                hdr_capabilities: Default::default(),
            },
            crate::mpv::VideoConfig::default(),
        )
        .unwrap()
    }

    #[test]
    fn only_the_presented_generation_consumes_a_worker_notification() {
        use crate::scene::worker_wake::WorkerWake;
        let (ping, _source) = calloop::ping::make_ping().unwrap();
        let current = WorkerWake::new(ping.clone());
        let candidate = WorkerWake::new(ping);
        current.notify();
        assert!(
            presentation_renderer(Some(&current), Some(&candidate))
                .unwrap()
                .take()
                .is_none()
        );
        candidate.notify();
        assert!(
            presentation_renderer(Some(&current), Some(&candidate))
                .unwrap()
                .take()
                .is_some()
        );
        // After accepting the candidate, the old renderer is rollback-only.
        assert!(
            presentation_renderer(Some(&candidate), None)
                .unwrap()
                .take()
                .is_none()
        );
        assert!(current.take().is_some());
        let late = candidate.clone();
        drop(candidate);
        late.notify();
        assert!(presentation_renderer::<WorkerWake>(None, None).is_none());
        assert!(current.take().is_none());
    }

    #[test]
    fn pending_source_selects_preparation_retries_in_both_scene_video_directions() {
        struct Project(std::path::PathBuf);
        impl Drop for Project {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("lwe-scene-retry-{}-{stamp}", std::process::id(),));
        std::fs::create_dir(&path).unwrap();
        let project = Project(path);
        std::fs::create_dir(project.0.join("models")).unwrap();
        std::fs::create_dir(project.0.join("materials")).unwrap();
        for (path, contents) in [
            ("project.json", r#"{"type":"scene","file":"scene.json"}"#),
            (
                "scene.json",
                r#"{"general":{"orthogonalprojection":{"width":16,"height":16}},"objects":[{"image":"models/image.json","size":"16 16","origin":"8 8 0"}]}"#,
            ),
            (
                "models/image.json",
                r#"{"solidlayer":true,"material":"materials/image.json"}"#,
            ),
            (
                "materials/image.json",
                r#"{"passes":[{"shader":"flat","blending":"translucent"}]}"#,
            ),
        ] {
            std::fs::write(project.0.join(path), contents).unwrap();
        }
        let scene = WallpaperSource::Scene {
            path: project.0.clone(),
            prepared: std::sync::Arc::new(
                crate::scene::PreparedScene::prepare(&project.0, None).unwrap(),
            ),
        };
        let video = WallpaperSource::Video {
            path: "/authored/video.mp4".into(),
        };
        assert_eq!(frame_retry(&video, None), EmptyFrameRetry::Decoder);
        assert_eq!(frame_retry(&scene, None), EmptyFrameRetry::ScenePreparation);
        assert_eq!(
            frame_retry(&video, Some(&scene)),
            EmptyFrameRetry::ScenePreparation
        );
        assert_eq!(frame_retry(&scene, Some(&video)), EmptyFrameRetry::Decoder);
        assert_eq!(
            authored_session().empty_frame_retry(),
            EmptyFrameRetry::Decoder
        );
    }

    #[test]
    fn resume_on_battery_only_clears_manual_pause_until_power_allows_playback() {
        let mut session = authored_session();
        session.pause();
        session.set_power_paused(true);
        session.resume();
        assert!(!session.is_manual_paused());
        assert!(session.power_paused);
        assert_eq!(session.state(), PlaybackState::Paused);
        assert_eq!(session.render_demand(32, 32, false), FrameDemand::Idle);
        assert_eq!(session.render_demand(32, 32, true), FrameDemand::Urgent);
        session.set_power_paused(false);
        assert_eq!(session.state(), PlaybackState::Playing);
    }

    #[test]
    fn battery_unpause_preserves_manual_pause_and_forced_redraw() {
        let mut session = authored_session();
        session.pause();
        session.set_power_paused(true);
        session.set_power_paused(false);
        assert!(session.is_manual_paused());
        assert_eq!(session.state(), PlaybackState::Paused);
        assert_eq!(session.render_demand(32, 32, false), FrameDemand::Idle);
        session.redraw_needed = true;
        assert_eq!(session.render_demand(32, 32, false), FrameDemand::Urgent);
        session.redraw_needed = false;
        session.resume();
        assert_eq!(session.state(), PlaybackState::Playing);
    }

    #[test]
    fn battery_first_frame_does_not_create_a_manual_pause() {
        let mut session = authored_session();
        session.set_power_paused(true);
        assert_eq!(session.state(), PlaybackState::Paused);
        assert_eq!(session.render_demand(32, 32, true), FrameDemand::Urgent);
        // The post-swap policy reinforcement must not behave like user Pause.
        session.set_power_paused(true);
        assert!(!session.is_manual_paused());
        session.set_power_paused(false);
        assert_eq!(session.state(), PlaybackState::Playing);
    }

    #[test]
    fn pause_before_initialization_preserves_the_first_frame_request() {
        let output = lwe_core::OutputInfo {
            name: "test-output".into(),
            width: 32,
            height: 32,
            scale: 1.0,
            position: (0, 0),
            active: true,
            hdr_capabilities: Default::default(),
        };
        let mut session = WallpaperSession::new(
            "/authored/video.mp4".into(),
            output,
            crate::mpv::VideoConfig::default(),
        )
        .unwrap();
        session.pause();
        assert_eq!(session.state(), PlaybackState::Paused);
        assert_eq!(session.render_demand(32, 32, true), FrameDemand::Urgent);
        assert_eq!(session.render_demand(32, 32, false), FrameDemand::Idle);
        session.resume();
        assert_eq!(session.state(), PlaybackState::Playing);
    }

    #[test]
    fn static_or_paused_renderers_do_not_sustain_frame_callbacks() {
        assert_eq!(
            frame_demand(PlaybackState::Playing, false, false, false, false),
            FrameDemand::Idle
        );
        assert_eq!(
            frame_demand(PlaybackState::Paused, false, false, false, true),
            FrameDemand::Idle
        );
        assert_eq!(
            frame_demand(PlaybackState::Playing, false, false, false, true),
            FrameDemand::Continuous
        );
    }

    #[test]
    fn paused_first_frame_resize_and_rollback_are_urgent() {
        for (first_frame, redraw, resized) in [
            (true, false, false),
            (false, true, false),
            (false, false, true),
        ] {
            assert_eq!(
                frame_demand(PlaybackState::Paused, first_frame, redraw, resized, false),
                FrameDemand::Urgent
            );
        }
        assert_eq!(
            frame_demand(PlaybackState::Stopped, true, false, false, false),
            FrameDemand::Urgent
        );
    }

    #[test]
    fn paused_scene_clock_does_not_advance_and_resume_is_idempotent() {
        let mut clock = SceneClock::new();
        clock.pause();
        let elapsed = clock.seconds();
        clock.pause();
        assert_eq!(clock.seconds(), elapsed);
        clock.resume();
        let started = clock.started;
        clock.resume();
        assert_eq!(clock.started, started);
    }
}
