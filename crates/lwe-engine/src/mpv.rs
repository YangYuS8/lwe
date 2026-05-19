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

    /// Check if a new frame is available for rendering
    pub fn has_frame(&self) -> bool {
        // Check if render context update callback was triggered
        if self.frame_available.load(Ordering::Acquire) {
            return true;
        }

        // Also check via mpv_render_context_update if we have a render context
        if let Some(render_ctx) = self.render_context {
            let flags = unsafe { libmpv_sys::mpv_render_context_update(render_ctx) };
            if flags & MPV_RENDER_UPDATE_FRAME != 0 {
                return true;
            }
        }

        false
    }

    /// Render a video frame
    pub fn render(&mut self, width: i32, height: i32, fbo: i32) -> Result<bool> {
        let Some(render_ctx) = self.render_context else {
            debug!("No render context available");
            return Ok(false);
        };

        self.process_events();

        let update_flags = unsafe { libmpv_sys::mpv_render_context_update(render_ctx) };
        let has_new_frame = (update_flags & MPV_RENDER_UPDATE_FRAME) != 0;

        let _ = self.frame_available.swap(false, Ordering::AcqRel);

        if !has_new_frame {
            return Ok(false);
        }

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
