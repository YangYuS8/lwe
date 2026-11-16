//! MPV video player using libmpv for OpenGL rendering
//!
//! This module provides a high-level wrapper around libmpv for video playback
//! with hardware decoding, HDR tone mapping, and OpenGL rendering support.

use std::ffi::{CString, c_char, c_void};
use std::ptr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Result, anyhow};
use tracing::{debug, info, warn};

use lwe_core::{
    HwdecMode, LayoutMode, OutputInfo,
    hdr::{HdrMetadata, HdrMode, ToneMappingConfig, parse_colorspace, parse_transfer_function},
};

use crate::egl::EglContext;

// mpv_render_param_type constants (from libmpv/render.h)
const MPV_RENDER_PARAM_INVALID: u32 = 0;
const MPV_RENDER_PARAM_API_TYPE: u32 = 1;
const MPV_RENDER_PARAM_OPENGL_INIT_PARAMS: u32 = 2;
const MPV_RENDER_PARAM_OPENGL_FBO: u32 = 3;
const MPV_RENDER_PARAM_FLIP_Y: u32 = 4;

// mpv_render_update_flag constants
const MPV_RENDER_UPDATE_FRAME: u64 = 1;

// OpenGL get_proc_address callback wrapper
extern "C" fn get_proc_address_wrapper(ctx: *mut c_void, name: *const c_char) -> *mut c_void {
    if ctx.is_null() || name.is_null() {
        return ptr::null_mut();
    }

    unsafe {
        let egl_ctx = &*(ctx as *const EglContext);
        let name_str = std::ffi::CStr::from_ptr(name).to_str().unwrap_or("");
        egl_ctx.get_proc_address(name_str) as *mut c_void
    }
}

/// Callback for mpv render context update notification
extern "C" fn render_update_callback(ctx: *mut c_void) {
    if ctx.is_null() {
        return;
    }
    unsafe {
        let flag = &*(ctx as *const AtomicBool);
        flag.store(true, Ordering::Release);
    }
}

/// Video source configuration for MPV
#[derive(Debug, Clone)]
pub struct VideoConfig {
    /// Source path (file path or URL)
    pub source: String,
    /// Whether to loop playback
    pub loop_playback: bool,
    /// Layout mode for video rendering
    pub layout: LayoutMode,
    /// Hardware decoding mode
    pub hwdec: HwdecMode,
    /// Mute audio
    pub mute: bool,
    /// Volume (0.0 - 1.0)
    pub volume: f64,
    /// Start time in seconds
    pub start_time: f64,
    /// Playback rate
    pub playback_rate: f64,
    /// HDR mode
    pub hdr_mode: HdrMode,
    /// Tone mapping configuration
    pub tone_mapping: ToneMappingConfig,
}

impl Default for VideoConfig {
    fn default() -> Self {
        Self {
            source: String::new(),
            loop_playback: true,
            layout: LayoutMode::Fill,
            hwdec: HwdecMode::Auto,
            mute: true,
            volume: 0.0,
            start_time: 0.0,
            playback_rate: 1.0,
            hdr_mode: HdrMode::Auto,
            tone_mapping: ToneMappingConfig::default(),
        }
    }
}

/// MPV-based video player with OpenGL rendering
pub struct MpvPlayer {
    handle: *mut libmpv_sys::mpv_handle,
    render_context: Option<*mut libmpv_sys::mpv_render_context>,
    output_info: OutputInfo,
    cached_dimensions: Option<(i32, i32)>,
    frame_available: Arc<AtomicBool>,
    pending_source: Option<String>,
    source_loaded: bool,
}

// Safety: mpv_handle can be safely sent between threads
unsafe impl Send for MpvPlayer {}

impl MpvPlayer {
    /// Create a new MPV player
    pub fn new(config: &VideoConfig, output_info: &OutputInfo) -> Result<Self> {
        info!("🎬 Initializing libmpv for output {}", output_info.name);

        let handle = unsafe { libmpv_sys::mpv_create() };
        if handle.is_null() {
            return Err(anyhow!("Failed to create MPV handle"));
        }

        let set_option = |name: &str, value: &str| {
            let name_c = CString::new(name).unwrap();
            let value_c = CString::new(value).unwrap();
            unsafe {
                let ret =
                    libmpv_sys::mpv_set_option_string(handle, name_c.as_ptr(), value_c.as_ptr());
                if ret < 0 {
                    warn!("Failed to set option {}={}: error {}", name, value, ret);
                }
            }
        };

        // Basic configuration
        set_option("config", "no");
        set_option("config-dir", "/dev/null");
        set_option("terminal", "no");
        set_option("msg-level", "all=warn");
        set_option("vid", "auto");
        set_option("pause", "no");

        // Layout configuration
        Self::configure_layout(&set_option, config.layout);

        // ===== Critical Performance Optimizations =====
        // Optimized for integrated GPUs (AMD APU, Intel UHD, etc.)

        // Hardware decoding - VAAPI for AMD/Intel on Linux
        let hwdec_str = match config.hwdec {
            HwdecMode::Auto => "vaapi-copy", // VAAPI copy-back for AMD APU
            HwdecMode::Force => "vaapi",     // Direct VAAPI
            HwdecMode::No => "no",
        };
        set_option("hwdec", hwdec_str);
        set_option("hwdec-codecs", "all");

        // Video output - minimal GPU load
        set_option("vo", "libmpv");
        set_option("gpu-api", "opengl");
        set_option("opengl-swapinterval", "0");
        set_option("opengl-pbo", "no"); // Disable PBO on integrated GPU (can cause overhead)

        // Aggressive performance settings for low-power scenarios
        set_option("video-latency-hacks", "yes");
        set_option("correct-pts", "no");
        set_option("framedrop", "decoder+vo");

        // Reduce rendering quality for power efficiency
        set_option("profile", "fast"); // Use fast profile
        set_option("deband", "no");
        set_option("dither-depth", "no");
        set_option("temporal-dither", "no");
        set_option("sigmoid-upscaling", "no");
        set_option("linear-downscaling", "no");
        set_option("linear-upscaling", "no");
        set_option("correct-downscaling", "no");
        set_option("scale", "bilinear");
        set_option("dscale", "bilinear");
        set_option("cscale", "bilinear");
        set_option("fbo-format", "rgba8"); // Simpler FBO format

        // Limit video FPS to reduce GPU load
        set_option("vf", "fps=30"); // Cap video to 30fps for wallpaper use

        // Memory optimization
        set_option("demuxer-max-bytes", "16M");
        set_option("demuxer-max-back-bytes", "4M");
        set_option("cache", "yes");
        set_option("cache-secs", "3");
        set_option("demuxer-readahead-secs", "2");

        // Audio disabled by default for wallpapers (can be re-enabled)
        set_option("audio", "no"); // Disable audio pipeline entirely

        // Playback settings
        if config.loop_playback {
            set_option("loop-file", "inf");
        }

        // Re-enable audio if not muted (overrides the default audio=no)
        if !config.mute {
            set_option("audio", "auto");
            let volume = format!("{}", (config.volume * 100.0) as i64);
            set_option("volume", &volume);
        }

        // Start time
        if config.start_time > 0.0 {
            set_option("start", &format!("{}", config.start_time));
        }

        // Playback rate
        if (config.playback_rate - 1.0).abs() > 0.01 {
            set_option("speed", &format!("{}", config.playback_rate));
        }

        // Initialize MPV
        let ret = unsafe { libmpv_sys::mpv_initialize(handle) };
        if ret < 0 {
            unsafe { libmpv_sys::mpv_terminate_destroy(handle) };
            return Err(anyhow!("Failed to initialize MPV: error {}", ret));
        }

        // Request log messages
        let log_level = CString::new("info").unwrap();
        unsafe {
            libmpv_sys::mpv_request_log_messages(handle, log_level.as_ptr());
        }

        info!("  ✓ MPV initialized successfully");

        Ok(Self {
            handle,
            render_context: None,
            output_info: output_info.clone(),
            cached_dimensions: None,
            frame_available: Arc::new(AtomicBool::new(false)),
            pending_source: Some(config.source.clone()),
            source_loaded: false,
        })
    }

    fn configure_layout(set_option: &impl Fn(&str, &str), layout: LayoutMode) {
        match layout {
            LayoutMode::Fill | LayoutMode::Cover => {
                set_option("keepaspect", "yes");
                set_option("panscan", "1.0");
                set_option("video-align-x", "0");
                set_option("video-align-y", "0");
            }
            LayoutMode::Stretch => {
                set_option("keepaspect", "no");
                set_option("video-unscaled", "no");
            }
            LayoutMode::Contain => {
                set_option("keepaspect", "yes");
                set_option("panscan", "0.0");
                set_option("video-align-x", "0");
                set_option("video-align-y", "0");
            }
            LayoutMode::Centre => {
                set_option("keepaspect", "yes");
                set_option("video-unscaled", "yes");
                set_option("video-align-x", "0");
                set_option("video-align-y", "0");
            }
        }
    }

    /// Initialize OpenGL render context
    pub fn init_render_context(&mut self, egl_context: &EglContext) -> Result<()> {
        if self.render_context.is_some() {
            return Ok(());
        }

        info!("🎨 Initializing mpv render context for OpenGL");

        let get_proc_address: extern "C" fn(*mut c_void, *const i8) -> *mut c_void =
            get_proc_address_wrapper;
        let get_proc_address_ctx = egl_context as *const _ as *mut c_void;

        let opengl_init_params = libmpv_sys::mpv_opengl_init_params {
            get_proc_address: Some(get_proc_address),
            get_proc_address_ctx,
            extra_exts: ptr::null(),
        };

        let api_type = CString::new("opengl").unwrap();
        let params = [
            libmpv_sys::mpv_render_param {
                type_: MPV_RENDER_PARAM_API_TYPE,
                data: api_type.as_ptr() as *mut c_void,
            },
            libmpv_sys::mpv_render_param {
                type_: MPV_RENDER_PARAM_OPENGL_INIT_PARAMS,
                data: &opengl_init_params as *const _ as *mut c_void,
            },
            libmpv_sys::mpv_render_param {
                type_: MPV_RENDER_PARAM_INVALID,
                data: ptr::null_mut(),
            },
        ];

        let mut render_context: *mut libmpv_sys::mpv_render_context = ptr::null_mut();
        egl_context.mark_current_context_foreign()?;
        let ret = unsafe {
            libmpv_sys::mpv_render_context_create(
                &mut render_context,
                self.handle,
                params.as_ptr() as *mut _,
            )
        };

        if ret < 0 {
            return Err(anyhow!(
                "Failed to create mpv render context: error {}",
                ret
            ));
        }

        unsafe {
            libmpv_sys::mpv_render_context_set_update_callback(
                render_context,
                Some(render_update_callback),
                Arc::as_ptr(&self.frame_available) as *mut c_void,
            );
        }

        self.render_context = Some(render_context);
        info!("  ✓ Render context created successfully");

        // Load pending source
        if let Some(source_path) = self.pending_source.take() {
            self.load_source(&source_path)?;
        }

        Ok(())
    }

    fn load_source(&mut self, path: &str) -> Result<()> {
        info!("  📁 Loading source: {}", path);

        let cmd = CString::new("loadfile").unwrap();
        let path_c = CString::new(path)?;
        let mode = CString::new("replace").unwrap();

        let mut args = [
            cmd.as_ptr(),
            path_c.as_ptr(),
            mode.as_ptr(),
            std::ptr::null(),
        ];

        let ret = unsafe { libmpv_sys::mpv_command(self.handle, args.as_mut_ptr()) };
        if ret < 0 {
            warn!("Failed to load source: error {}", ret);
            return Err(anyhow!("Failed to load source: error {}", ret));
        }

        info!("  ✓ Source loaded successfully");
        self.source_loaded = true;
        Ok(())
    }

    /// Configure HDR handling
    pub fn configure_hdr(
        &mut self,
        hdr_mode: HdrMode,
        tone_mapping: &ToneMappingConfig,
    ) -> Result<()> {
        info!("🎨 Configuring HDR handling...");

        match hdr_mode {
            HdrMode::Disable => {
                info!("  HDR mode: Disabled (forced SDR)");
                return Ok(());
            }
            HdrMode::Force => {
                info!("  HDR mode: Force (always apply HDR processing)");
                self.configure_tone_mapping(tone_mapping)?;
                return Ok(());
            }
            HdrMode::Auto => {
                info!("  HDR mode: Auto (detect from video)");
            }
        }

        match self.get_hdr_metadata() {
            Some(metadata) => {
                info!("  📊 Video HDR metadata detected");
                if metadata.is_hdr() {
                    info!(
                        "  ✨ HDR content detected: {}",
                        metadata.format_description()
                    );
                    self.configure_tone_mapping(tone_mapping)?;
                } else {
                    info!("  📺 SDR content detected");
                }
            }
            None => {
                debug!("  ⚠️  Could not detect HDR metadata");
                self.configure_tone_mapping(tone_mapping)?;
            }
        }

        Ok(())
    }

    fn configure_tone_mapping(&self, config: &ToneMappingConfig) -> Result<()> {
        info!("  🎨 Configuring tone mapping for HDR → SDR");

        let mut optimized_config = config.clone();

        if let Some(metadata) = self.get_hdr_metadata() {
            optimized_config.optimize_for_content(&metadata);
        }

        let set_option = |name: &str, value: &str| {
            let name_c = CString::new(name).unwrap();
            let value_c = CString::new(value).unwrap();
            unsafe {
                let ret = libmpv_sys::mpv_set_option_string(
                    self.handle,
                    name_c.as_ptr(),
                    value_c.as_ptr(),
                );
                if ret < 0 {
                    warn!("    Failed to set {}={}: error {}", name, value, ret);
                } else {
                    debug!("    ✓ Set {}={}", name, value);
                }
            }
        };

        set_option("tone-mapping", optimized_config.algorithm.as_mpv_str());
        set_option("tone-mapping-mode", &optimized_config.mode);

        if optimized_config.compute_peak {
            set_option("hdr-compute-peak", "yes");
        } else {
            set_option("hdr-compute-peak", "no");
        }

        if optimized_config.algorithm.uses_param() {
            set_option(
                "tone-mapping-param",
                &format!("{:.2}", optimized_config.param),
            );
        }

        set_option("target-trc", "srgb");
        set_option("target-prim", "bt.709");
        set_option("target-peak", "203");

        info!("  ✓ Tone mapping configured");
        Ok(())
    }

    /// Load a video/image file for playback
    pub fn load_file(&mut self, path: &std::path::Path) -> Result<()> {
        let path_str = path.to_string_lossy();
        self.load_source(&path_str)
    }

    /// Consume a pending render-context update and report whether a new frame is ready.
    pub fn consume_frame_update(&mut self, force_poll: bool) -> bool {
        if !self.frame_available.swap(false, Ordering::AcqRel) && !force_poll {
            return false;
        }

        self.process_events();

        self.render_context
            .map(|render_ctx| {
                let flags = unsafe { libmpv_sys::mpv_render_context_update(render_ctx) };
                flags & MPV_RENDER_UPDATE_FRAME != 0
            })
            .unwrap_or(false)
    }

    /// Render a video frame
    pub fn render(&mut self, width: i32, height: i32, fbo: i32) -> Result<bool> {
        let Some(render_ctx) = self.render_context else {
            debug!("No render context available");
            return Ok(false);
        };

        debug!(
            "🎬 Rendering NEW frame: {}x{} to FBO {}",
            width, height, fbo
        );

        let fbo_data = libmpv_sys::mpv_opengl_fbo {
            fbo,
            w: width,
            h: height,
            internal_format: 0,
        };

        let flip_y: i32 = 1;

        let params = [
            libmpv_sys::mpv_render_param {
                type_: MPV_RENDER_PARAM_OPENGL_FBO,
                data: &fbo_data as *const _ as *mut c_void,
            },
            libmpv_sys::mpv_render_param {
                type_: MPV_RENDER_PARAM_FLIP_Y,
                data: &flip_y as *const _ as *mut c_void,
            },
            libmpv_sys::mpv_render_param {
                type_: MPV_RENDER_PARAM_INVALID,
                data: ptr::null_mut(),
            },
        ];

        let ret =
            unsafe { libmpv_sys::mpv_render_context_render(render_ctx, params.as_ptr() as *mut _) };

        if ret < 0 {
            warn!("mpv render error: {}", ret);
            return Ok(false);
        }

        Ok(true)
    }

    /// Report frame swap
    pub fn report_swap(&self) {
        if let Some(render_ctx) = self.render_context {
            unsafe {
                libmpv_sys::mpv_render_context_report_swap(render_ctx);
            }
        }
    }

    fn process_events(&mut self) {
        loop {
            let event = unsafe { libmpv_sys::mpv_wait_event(self.handle, 0.0) };
            if event.is_null() {
                break;
            }

            let event_id = unsafe { (*event).event_id };

            if event_id == 0 {
                break;
            }

            match event_id {
                7 => {
                    // MPV_EVENT_FILE_LOADED
                    info!("📺 MPV: file loaded");
                    self.cached_dimensions = None;
                }
                16 => {
                    // MPV_EVENT_PLAYBACK_RESTART
                    info!("📺 MPV: playback restart");
                }
                20 => {
                    // MPV_EVENT_VIDEO_RECONFIG
                    info!("📺 MPV: video reconfig");
                    self.cached_dimensions = None;
                }
                _ => {
                    debug!("📺 MPV event: id={}", event_id);
                }
            }
        }
    }

    /// Get video dimensions
    pub fn get_video_dimensions(&mut self) -> Option<(i32, i32)> {
        if let Some(dims) = self.cached_dimensions {
            return Some(dims);
        }

        let width = self.get_property_i64("dwidth")?;
        let height = self.get_property_i64("dheight")?;

        if width > 0 && height > 0 {
            let dims = (width as i32, height as i32);
            self.cached_dimensions = Some(dims);
            Some(dims)
        } else {
            None
        }
    }

    fn get_property_i64(&self, name: &str) -> Option<i64> {
        let prop_name = CString::new(name).ok()?;
        let mut value: i64 = 0;

        let ret = unsafe {
            libmpv_sys::mpv_get_property(
                self.handle,
                prop_name.as_ptr(),
                4, // MPV_FORMAT_INT64
                &mut value as *mut i64 as *mut c_void,
            )
        };

        (ret == 0).then_some(value)
    }

    fn get_property_string(&self, name: &str) -> Option<String> {
        let prop_name = CString::new(name).ok()?;
        let mut value_ptr: *mut c_char = std::ptr::null_mut();

        let ret = unsafe {
            libmpv_sys::mpv_get_property(
                self.handle,
                prop_name.as_ptr(),
                1, // MPV_FORMAT_STRING
                &mut value_ptr as *mut *mut c_char as *mut c_void,
            )
        };

        if ret == 0 && !value_ptr.is_null() {
            let c_str = unsafe { std::ffi::CStr::from_ptr(value_ptr) };
            let result = c_str.to_string_lossy().into_owned();
            unsafe {
                libmpv_sys::mpv_free(value_ptr as *mut c_void);
            }
            Some(result)
        } else {
            None
        }
    }

    fn get_property_f64(&self, name: &str) -> Option<f64> {
        let prop_name = CString::new(name).ok()?;
        let mut value: f64 = 0.0;

        let ret = unsafe {
            libmpv_sys::mpv_get_property(
                self.handle,
                prop_name.as_ptr(),
                5, // MPV_FORMAT_DOUBLE
                &mut value as *mut f64 as *mut c_void,
            )
        };

        (ret == 0).then_some(value)
    }

    /// Get HDR metadata from video
    pub fn get_hdr_metadata(&self) -> Option<HdrMetadata> {
        let colorspace_str = self.get_property_string("video-params/colorspace")?;
        let gamma_str = self.get_property_string("video-params/gamma")?;
        let primaries_str = self.get_property_string("video-params/primaries")?;

        let color_space = parse_colorspace(&colorspace_str);
        let transfer_function = parse_transfer_function(&gamma_str);
        let peak_luminance = self.get_property_f64("video-params/sig-peak");

        Some(HdrMetadata {
            color_space,
            transfer_function,
            primaries: primaries_str,
            peak_luminance,
            avg_luminance: None,
            min_luminance: None,
        })
    }

    /// Pause playback
    pub fn pause(&mut self) -> Result<()> {
        let prop = CString::new("pause").unwrap();
        let value = CString::new("yes").unwrap();
        let ret = unsafe {
            libmpv_sys::mpv_set_option_string(self.handle, prop.as_ptr(), value.as_ptr())
        };
        if ret < 0 {
            return Err(anyhow!("Failed to pause: error {}", ret));
        }
        Ok(())
    }

    /// Resume playback
    pub fn resume(&mut self) -> Result<()> {
        let prop = CString::new("pause").unwrap();
        let value = CString::new("no").unwrap();
        let ret = unsafe {
            libmpv_sys::mpv_set_option_string(self.handle, prop.as_ptr(), value.as_ptr())
        };
        if ret < 0 {
            return Err(anyhow!("Failed to resume: error {}", ret));
        }
        Ok(())
    }

    /// Set volume (0.0 - 1.0)
    pub fn set_volume(&mut self, volume: f64) -> Result<()> {
        let prop = CString::new("volume").unwrap();
        let value = CString::new(format!("{}", (volume * 100.0) as i64)).unwrap();
        let ret = unsafe {
            libmpv_sys::mpv_set_option_string(self.handle, prop.as_ptr(), value.as_ptr())
        };
        if ret < 0 {
            return Err(anyhow!("Failed to set volume: error {}", ret));
        }
        Ok(())
    }
}

impl Drop for MpvPlayer {
    fn drop(&mut self) {
        debug!("Dropping MPV player for {}", self.output_info.name);

        if let Some(render_ctx) = self.render_context {
            unsafe {
                libmpv_sys::mpv_render_context_free(render_ctx);
            }
        }

        if !self.handle.is_null() {
            unsafe {
                libmpv_sys::mpv_terminate_destroy(self.handle);
            }
        }
    }
}

/// A separate libmpv core for a prepared Scene sound. It has no video output,
/// render context or EGL dependency. The descriptor stays alive until the core
/// and all of its internal workers have synchronously terminated.
pub(crate) struct MpvAudioPlayer {
    handle: *mut libmpv_sys::mpv_handle,
    _source: Arc<std::fs::File>,
    loaded: bool,
    failure: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SceneAudioFormat {
    Flac,
    Mp3,
}

// The private handle has one owner; libmpv's client API permits cross-thread use.
unsafe impl Send for MpvAudioPlayer {}

impl MpvAudioPlayer {
    pub(crate) fn new(source: Arc<std::fs::File>, format: SceneAudioFormat) -> Result<Self> {
        Self::create(source, false, None, Some(format))
    }

    #[cfg(test)]
    pub(crate) fn new_null_output(
        source: Arc<std::fs::File>,
        format: SceneAudioFormat,
    ) -> Result<Self> {
        Self::create(source, true, None, Some(format))
    }

    fn create(
        source: Arc<std::fs::File>,
        null_output: bool,
        private_device: Option<&str>,
        format: Option<SceneAudioFormat>,
    ) -> Result<Self> {
        use std::os::fd::AsRawFd;
        let handle = unsafe { libmpv_sys::mpv_create() };
        anyhow::ensure!(!handle.is_null(), "Failed to create Scene audio player");
        let mut player = Self {
            handle,
            _source: source,
            loaded: false,
            failure: None,
        };
        for (name, value) in [
            ("config", "no"),
            ("terminal", "no"),
            ("load-scripts", "no"),
            ("ytdl", "no"),
            ("input-terminal", "no"),
            ("msg-level", "all=error"),
            ("vid", "no"),
            ("sid", "no"),
            ("audio-display", "no"),
            ("vo", "null"),
            ("idle", "yes"),
            ("pause", "yes"),
            ("mute", "yes"),
            ("volume", "0"),
            ("loop-file", "inf"),
            ("keep-open", "no"),
            ("cache", "no"),
            ("replaygain", "no"),
            ("demuxer-max-bytes", "8388608"),
            ("demuxer-readahead-secs", "0"),
            ("audio-client-name", "LWE Scene audio"),
        ] {
            player.set_option(name, value)?;
        }
        if format == Some(SceneAudioFormat::Mp3) {
            player.set_option("demuxer", "lavf")?;
            player.set_option("demuxer-lavf-format", "mp3")?;
        }
        if null_output {
            player.set_option("ao", "null")?;
        }
        if let Some(device) = private_device {
            // Used only by the opt-in acceptance test, which owns this sink.
            player.set_option("audio-device", device)?;
            player.set_option(
                "audio-client-name",
                &format!("LWE Scene audio acceptance {}", std::process::id()),
            )?;
        }
        let result = unsafe { libmpv_sys::mpv_initialize(handle) };
        anyhow::ensure!(
            result >= 0,
            "Failed to initialize Scene audio: error {result}"
        );
        let path = format!("/proc/self/fd/{}", player._source.as_raw_fd());
        let args = [
            CString::new("loadfile")?,
            CString::new(path)?,
            CString::new("replace")?,
        ];
        let mut pointers = args
            .iter()
            .map(|value| value.as_ptr())
            .chain(std::iter::once(ptr::null()))
            .collect::<Vec<_>>();
        let result = unsafe { libmpv_sys::mpv_command(handle, pointers.as_mut_ptr()) };
        anyhow::ensure!(
            result >= 0,
            "Failed to queue prepared Scene audio: error {result}"
        );
        // Decoder/resource loading belongs to libmpv's workers. The engine must
        // poll readiness before accepting a candidate; this call never waits for
        // FILE_LOADED and never unpauses or unmutes the new source.
        player.poll_ready()?;
        Ok(player)
    }

    fn set_option(&mut self, name: &str, value: &str) -> Result<()> {
        let name = CString::new(name)?;
        let value = CString::new(value)?;
        let result = unsafe {
            libmpv_sys::mpv_set_option_string(self.handle, name.as_ptr(), value.as_ptr())
        };
        anyhow::ensure!(
            result >= 0,
            "Failed to configure Scene audio option: error {result}"
        );
        Ok(())
    }

    pub(crate) fn poll_ready(&mut self) -> Result<bool> {
        // Bound work even if the decoder is producing events continuously.
        for _ in 0..64 {
            let event = unsafe { libmpv_sys::mpv_wait_event(self.handle, 0.0) };
            anyhow::ensure!(!event.is_null(), "Scene audio returned an invalid event");
            let event = unsafe { &*event };
            match event.event_id {
                libmpv_sys::mpv_event_id_MPV_EVENT_NONE => break,
                libmpv_sys::mpv_event_id_MPV_EVENT_FILE_LOADED => self.loaded = true,
                libmpv_sys::mpv_event_id_MPV_EVENT_END_FILE => {
                    if !event.data.is_null() {
                        let end =
                            unsafe { &*(event.data as *const libmpv_sys::mpv_event_end_file) };
                        if end.reason
                            == libmpv_sys::mpv_end_file_reason_MPV_END_FILE_REASON_ERROR as i32
                        {
                            self.failure = Some(format!(
                                "Prepared Scene audio playback failed: error {}",
                                end.error
                            ));
                        }
                    }
                }
                libmpv_sys::mpv_event_id_MPV_EVENT_SHUTDOWN => {
                    self.failure = Some("Scene audio stopped unexpectedly".to_string())
                }
                _ => {}
            }
        }
        if let Some(reason) = &self.failure {
            return Err(anyhow!(reason.clone()));
        }
        Ok(self.loaded)
    }

    pub(crate) fn set_policy(&mut self, active: bool, paused: bool, volume: f64) -> Result<()> {
        anyhow::ensure!(
            volume.is_finite() && (0.0..=1.0).contains(&volume),
            "Invalid Scene audio volume"
        );
        // Fail closed during every transition: unmute is the final operation.
        self.set_flag("mute", true)?;
        let name = CString::new("volume")?;
        let mut percent = volume * 100.0;
        let result = unsafe {
            libmpv_sys::mpv_set_property(
                self.handle,
                name.as_ptr(),
                libmpv_sys::mpv_format_MPV_FORMAT_DOUBLE,
                (&mut percent as *mut f64).cast(),
            )
        };
        anyhow::ensure!(
            result >= 0,
            "Failed to set Scene audio volume: error {result}"
        );
        self.set_flag("pause", !active || paused)?;
        self.set_flag("mute", !active || volume == 0.0)
    }

    fn set_flag(&mut self, name: &str, value: bool) -> Result<()> {
        let name = CString::new(name)?;
        let mut flag = i32::from(value);
        let result = unsafe {
            libmpv_sys::mpv_set_property(
                self.handle,
                name.as_ptr(),
                libmpv_sys::mpv_format_MPV_FORMAT_FLAG,
                (&mut flag as *mut i32).cast(),
            )
        };
        anyhow::ensure!(
            result >= 0,
            "Failed to update Scene audio playback: error {result}"
        );
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn property(&self, name: &str) -> Option<String> {
        let name = CString::new(name).unwrap();
        let value = unsafe { libmpv_sys::mpv_get_property_string(self.handle, name.as_ptr()) };
        if value.is_null() {
            return None;
        }
        let result = unsafe { std::ffi::CStr::from_ptr(value) }
            .to_string_lossy()
            .into_owned();
        unsafe {
            libmpv_sys::mpv_free(value.cast());
        }
        Some(result)
    }
}

impl Drop for MpvAudioPlayer {
    fn drop(&mut self) {
        if !self.handle.is_null() {
            unsafe {
                libmpv_sys::mpv_terminate_destroy(self.handle);
            }
            self.handle = ptr::null_mut();
        }
    }
}

#[cfg(test)]
mod audio_tests {
    use super::MpvAudioPlayer;
    use std::fs::File;
    use std::io::{Read, Seek, SeekFrom, Write};
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::process::{Child, ChildStdout, Command, Stdio};
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    fn authored_pcm() -> Arc<File> {
        let descriptor =
            unsafe { libc::memfd_create(c"lwe-authored-pcm".as_ptr(), libc::MFD_CLOEXEC) };
        assert!(descriptor >= 0);
        let mut source = unsafe { File::from_raw_fd(descriptor) };
        let samples = 8_000_u32;
        let bytes = samples * 2;
        let mut wav = Vec::new();
        wav.extend(b"RIFF");
        wav.extend((bytes + 36).to_le_bytes());
        wav.extend(b"WAVEfmt ");
        wav.extend(16_u32.to_le_bytes());
        wav.extend(1_u16.to_le_bytes());
        wav.extend(1_u16.to_le_bytes());
        wav.extend(8_000_u32.to_le_bytes());
        wav.extend(16_000_u32.to_le_bytes());
        wav.extend(2_u16.to_le_bytes());
        wav.extend(16_u16.to_le_bytes());
        wav.extend(b"data");
        wav.extend(bytes.to_le_bytes());
        for sample in 0..samples {
            let value = (8_000.0
                * (2.0 * std::f64::consts::PI * 440.0 * f64::from(sample) / 8_000.0).sin())
                as i16;
            wav.extend(value.to_le_bytes());
        }
        source.write_all(&wav).unwrap();
        source.seek(SeekFrom::Start(0)).unwrap();
        Arc::new(source)
    }

    #[test]
    fn null_output_decoder_keeps_candidates_silent_and_releases_owned_descriptor() {
        // This exercises libmpv decoding/policy with ao=null. It is deliberately
        // separate from physical audio output or desktop acceptance.
        let source = authored_pcm();
        let weak = Arc::downgrade(&source);
        let mut player = MpvAudioPlayer::create(source, true, None, None).unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        while !player.poll_ready().unwrap() {
            assert!(
                Instant::now() < deadline,
                "authored PCM decoder did not load"
            );
            std::thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(player.property("pause").as_deref(), Some("yes"));
        assert_eq!(player.property("mute").as_deref(), Some("yes"));
        assert_eq!(player.property("loop-file").as_deref(), Some("inf"));
        assert_eq!(player.property("vo").as_deref(), Some("null"));
        player.set_policy(false, false, 1.0).unwrap();
        assert_eq!(player.property("pause").as_deref(), Some("yes"));
        assert_eq!(player.property("mute").as_deref(), Some("yes"));
        player.set_policy(true, true, 0.5).unwrap();
        assert_eq!(player.property("pause").as_deref(), Some("yes"));
        assert_eq!(player.property("mute").as_deref(), Some("no"));
        assert_eq!(player.property("volume").as_deref(), Some("50.000000"));
        player.set_policy(true, false, 0.0).unwrap();
        assert_eq!(player.property("pause").as_deref(), Some("no"));
        assert_eq!(player.property("mute").as_deref(), Some("yes"));
        assert!(player.set_policy(true, false, f64::NAN).is_err());
        assert!(weak.upgrade().is_some());
        drop(player);
        assert!(
            weak.upgrade().is_none(),
            "source survived private core termination"
        );
    }

    fn pactl(args: &[&str]) -> String {
        let result = Command::new("pactl")
            .args(args)
            .output()
            .expect("pactl is required for the opt-in private-sink test");
        assert!(
            result.status.success(),
            "Private audio pactl action failed: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        String::from_utf8(result.stdout).unwrap().trim().to_string()
    }

    struct OwnedSink {
        name: String,
        module: Option<String>,
    }
    impl OwnedSink {
        fn new() -> Self {
            let unique = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let name = format!("lwe_audio_acceptance_{}_{unique}", std::process::id());
            let module = pactl(&[
                "load-module",
                "module-null-sink",
                &format!("sink_name={name}"),
                "rate=48000",
                "channels=2",
                "sink_properties=device.description=LWE_Audio_Acceptance",
            ]);
            assert!(
                module.parse::<u32>().is_ok(),
                "private sink did not return its module identity"
            );
            Self {
                name,
                module: Some(module),
            }
        }
        fn unload(mut self) {
            pactl(&["unload-module", self.module.as_ref().unwrap()]);
            self.module = None;
        }
    }
    impl Drop for OwnedSink {
        fn drop(&mut self) {
            if let Some(module) = self.module.take() {
                let result = Command::new("pactl")
                    .args(["unload-module", &module])
                    .output();
                if !result.is_ok_and(|result| result.status.success()) {
                    eprintln!("private Scene audio sink cleanup failed");
                }
            }
        }
    }

    struct OwnedMonitor {
        child: Child,
        output: ChildStdout,
    }
    impl OwnedMonitor {
        fn new(sink: &OwnedSink) -> Self {
            let mut child = Command::new("parec")
                .args([
                    "--raw",
                    "--format=s16le",
                    "--rate=48000",
                    "--channels=2",
                    "--latency-msec=20",
                    "--process-time-msec=20",
                    &format!("--device={}.monitor", sink.name),
                ])
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .expect("parec is required for private-sink PCM acceptance");
            let output = child.stdout.take().unwrap();
            let flags = unsafe { libc::fcntl(output.as_raw_fd(), libc::F_GETFL) };
            assert!(flags >= 0);
            assert!(
                unsafe { libc::fcntl(output.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) }
                    >= 0
            );
            Self { child, output }
        }
        fn read_pcm(&mut self, milliseconds: usize) -> Vec<i16> {
            let count = milliseconds * 48 * 2 * 2;
            let mut bytes = vec![0_u8; count];
            let mut read = 0;
            let deadline = Instant::now() + Duration::from_secs(5);
            while read < count {
                assert!(
                    Instant::now() < deadline,
                    "private monitor PCM read timed out"
                );
                match self.output.read(&mut bytes[read..]) {
                    Ok(0) => panic!("private monitor ended before PCM acceptance"),
                    Ok(count) => read += count,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        let mut descriptor = libc::pollfd {
                            fd: self.output.as_raw_fd(),
                            events: libc::POLLIN,
                            revents: 0,
                        };
                        assert!(unsafe { libc::poll(&mut descriptor, 1, 100) } >= 0);
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                    Err(error) => panic!("private monitor read failed: {error}"),
                }
            }
            bytes
                .as_chunks::<2>()
                .0
                .iter()
                .map(|sample| i16::from_le_bytes([sample[0], sample[1]]))
                .collect()
        }
        fn settled_rms(&mut self, phase: &str) -> f64 {
            // Consume capture buffers continuously instead of sleeping while
            // old samples accumulate in the child pipe.
            let _ = self.read_pcm(1000);
            let samples = self.read_pcm(300);
            let rms = (samples
                .iter()
                .map(|sample| f64::from(*sample).powi(2))
                .sum::<f64>()
                / samples.len() as f64)
                .sqrt();
            eprintln!(
                "scene_audio_pcm {}",
                serde_json::json!({"phase":phase,"rms_s16":rms,"samples":samples.len(),"private_monitor":true})
            );
            rms
        }
    }
    impl Drop for OwnedMonitor {
        fn drop(&mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }

    #[test]
    fn private_pulse_sink_observes_candidate_activation_pause_mute_and_drop() {
        if std::env::var("LWE_REAL_SCENE_AUDIO_TESTS").as_deref() != Ok("1") {
            eprintln!("skipping private-sink PCM acceptance; set LWE_REAL_SCENE_AUDIO_TESTS=1");
            return;
        }
        let original_sink = pactl(&["get-default-sink"]);
        let original_source = pactl(&["get-default-source"]);
        let sink = OwnedSink::new();
        assert_eq!(
            pactl(&["get-default-sink"]),
            original_sink,
            "private test changed the default output"
        );
        assert_eq!(
            pactl(&["get-default-source"]),
            original_source,
            "private test changed the default input"
        );
        let mut monitor = OwnedMonitor::new(&sink);
        let source = authored_pcm();
        let weak = Arc::downgrade(&source);
        let device = format!("pulse/{}", sink.name);
        let mut player = MpvAudioPlayer::create(source, false, Some(&device), None).unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        while !player.poll_ready().unwrap() {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(
            player.property("audio-device").as_deref(),
            Some(device.as_str())
        );
        assert!(
            monitor.settled_rms("candidate") <= 1.0,
            "candidate produced PCM before activation"
        );
        // If an output backend initializes only after unpause, initialize it
        // while mute+zero-volume remain in effect, then verify the actual sink.
        player.set_policy(true, false, 0.0).unwrap();
        let sinks: serde_json::Value =
            serde_json::from_str(&pactl(&["--format=json", "list", "sinks"])).unwrap();
        let private_index = sinks
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["name"].as_str() == Some(sink.name.as_str()))
            .unwrap()["index"]
            .as_u64()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            let inputs: serde_json::Value =
                serde_json::from_str(&pactl(&["--format=json", "list", "sink-inputs"])).unwrap();
            let own: Vec<_> = inputs
                .as_array()
                .unwrap()
                .iter()
                .filter(|entry| {
                    entry["properties"]["application.process.id"].as_str()
                        == Some(std::process::id().to_string().as_str())
                })
                .collect();
            if !own.is_empty() {
                assert!(
                    own.iter()
                        .all(|entry| entry["sink"].as_u64() == Some(private_index)),
                    "Scene audio stream escaped its private sink"
                );
                break;
            }
            assert!(
                Instant::now() < deadline,
                "audio output did not attach to the private sink"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        player.set_policy(true, false, 0.8).unwrap();
        eprintln!(
            "scene_audio_activation_state {}",
            serde_json::json!({"pause":player.property("pause"),"mute":player.property("mute"),"volume":player.property("volume"),"current_ao":player.property("current-ao"),"time_pos":player.property("time-pos")})
        );
        assert!(
            monitor.settled_rms("activated") > 1000.0,
            "activated audio had no sampled PCM"
        );
        player.set_policy(true, true, 0.8).unwrap();
        assert!(
            monitor.settled_rms("paused") <= 1.0,
            "paused audio continued producing PCM"
        );
        player.set_policy(true, false, 0.0).unwrap();
        assert!(
            monitor.settled_rms("muted") <= 1.0,
            "zero-volume audio produced PCM"
        );
        drop(player);
        assert!(
            weak.upgrade().is_none(),
            "player did not release its prepared descriptor"
        );
        assert!(
            monitor.settled_rms("dropped") <= 1.0,
            "audio survived private core termination"
        );
        drop(monitor);
        sink.unload();
        assert_eq!(pactl(&["get-default-sink"]), original_sink);
        assert_eq!(pactl(&["get-default-source"]), original_source);
        eprintln!(
            "scene_audio_pcm_complete {{\"default_output_unchanged\":true,\"default_input_unchanged\":true,\"owned_sink_unloaded\":true,\"owned_capture_joined\":true,\"owned_descriptor_released\":true}}"
        );
    }
}
