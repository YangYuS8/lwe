use std::collections::BTreeMap;
use std::ffi::CString;
use std::marker::PhantomData;
use std::rc::Rc;
use std::sync::Arc;

use anyhow::{Context, Result, bail, ensure};

#[path = "frozen_history.rs"]
mod frozen_history;

#[cfg(test)]
#[path = "renderer_acceptance.rs"]
mod acceptance;

#[cfg(test)]
#[path = "renderer_yakuza_acceptance.rs"]
mod yakuza_acceptance;

use super::shared_texture::{ResourcePlan, SharedTexture, TextureKey, TexturePool, WorkKey};

use super::{
    Blend, PreparedLayer, PreparedPass, PreparedScene, PreparedStage, PreparedTexture,
    TEXTURE_SLOTS, TextureBinding,
};

pub(super) const QUAD: [f32; 30] = [
    -1.0, -1.0, 0.0, 0.0, 0.0, 1.0, -1.0, 0.0, 1.0, 0.0, 1.0, 1.0, 0.0, 1.0, 1.0, -1.0, -1.0, 0.0,
    0.0, 0.0, 1.0, 1.0, 0.0, 1.0, 1.0, -1.0, 1.0, 0.0, 0.0, 1.0,
];
pub(super) const IDENTITY: [f32; 16] = [
    1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
];

pub(super) const MAX_SCENE_GPU_BYTES: u64 = super::shared_texture::PAYLOAD_LIMIT;

// A compositor may withhold background callbacks while an output is hidden or
// powered off. Resume from the retained state instead of replaying unbounded
// simulation work or removing a valid wallpaper for a presentation gap.
fn animation_delta(previous: f64, current: f64) -> f64 {
    let elapsed = (current - previous).max(0.0);
    if elapsed > 2.0 { 0.25 } else { elapsed }
}

// Work images are borrowed only during a sequential render attempt. Accepted
// pixels and unique histories never use these keys.
fn direct_layer(layer: &PreparedLayer) -> bool {
    layer.composite.is_none()
        && layer.stages.len() == 1
        && layer.stages[0].targets.is_empty()
        && layer.stages[0].passes.len() == 1
        && layer.stages[0].passes[0].target.is_none()
        && matches!(
            layer.stages[0].passes[0].label.as_str(),
            "LWE image material" | "genericimage4" | "LWE text material"
        )
}
fn work_offset(layer: &PreparedLayer, canvas: [f32; 2], full: bool) -> usize {
    if full && layer.size.map(|v| v.ceil() as i32) == canvas.map(|v| v.ceil() as i32) {
        if layer.composite.is_some()
            || layer
                .stages
                .iter()
                .flat_map(|s| &s.passes)
                .flat_map(|p| &p.textures)
                .any(|b| matches!(b, TextureBinding::FullFrame))
        {
            2
        } else {
            1
        }
    } else {
        0
    }
}
fn scratch_target_count(layer: &PreparedLayer) -> usize {
    let unnamed = layer
        .stages
        .iter()
        .flat_map(|stage| &stage.passes)
        .filter(|pass| {
            pass.target.is_none() && matches!(pass.kind, super::PreparedPassKind::Render)
        })
        .count()
        .clamp(1, 2);
    let [width, height] = layer.size.map(|v| v.ceil() as i32);
    let named = layer
        .stages
        .iter()
        .map(|stage| {
            stage
                .targets
                .iter()
                .filter(|(name, scale)| {
                    !stage.persistent_targets.contains(*name)
                        && target_dimensions(width, height, **scale) == [width, height]
                })
                .count()
        })
        .max()
        .unwrap_or(0);
    unnamed.max(named + if unnamed > 1 && named > 0 { 2 } else { 1 })
}

fn scratch_layout(
    layers: &[PreparedLayer],
    canvas: [f32; 2],
    full: bool,
) -> BTreeMap<(i32, i32), usize> {
    let mut layout = BTreeMap::new();
    for layer in layers.iter().filter(|l| !direct_layer(l)) {
        let dimensions = (layer.size[0].ceil() as i32, layer.size[1].ceil() as i32);
        let count = work_offset(layer, canvas, full) + scratch_target_count(layer);
        let previous = layout.entry(dimensions).or_insert(0);
        *previous = (*previous).max(count);
    }
    layout
}
fn needs_full_frame(scene: &PreparedScene) -> bool {
    scene.layers.iter().any(|layer| {
        layer.composite.is_some()
            || layer
                .stages
                .iter()
                .flat_map(|stage| &stage.passes)
                .flat_map(|pass| &pass.textures)
                .any(|binding| matches!(binding, TextureBinding::FullFrame))
    })
}

/// Logical RGBA texture/FBO payload, not driver memory or EGL back buffers.
fn rgba_bytes(width: u64, height: u64, count: u64) -> Result<u64> {
    width
        .checked_mul(height)
        .and_then(|pixels| pixels.checked_mul(4))
        .and_then(|bytes| bytes.checked_mul(count))
        .context("Scene RGBA allocation estimate overflow")
}

pub(super) fn output_cache_bytes(width: i32, height: i32, count: u64) -> Result<u64> {
    ensure!(width > 0 && height > 0, "Invalid Scene cache dimensions");
    rgba_bytes(width as u64, height as u64, count)
}
fn private_gpu_bytes(scene: &PreparedScene) -> Result<u64> {
    let mut bytes = 4_u64;
    for layer in &scene.layers {
        for stage in &layer.stages {
            for (name, scale) in &stage.targets {
                if stage.persistent_targets.contains(name) {
                    let [w, h] = target_dimensions(
                        layer.size[0].ceil() as i32,
                        layer.size[1].ceil() as i32,
                        *scale,
                    );
                    bytes = bytes
                        .checked_add(rgba_bytes(w as u64, h as u64, 1)?)
                        .context("Scene history estimate overflow")?;
                }
            }
        }
    }
    // Both the current and fully staged next regions coexist until publication.
    for texture in &scene.textures {
        if let Some(source) = &texture.timed {
            let mut maximum = 0;
            for frame in 0..source.frames().len() {
                let rect = source.frame_neighborhood(frame, 1)?.rect();
                maximum = maximum.max(rgba_bytes(rect.width as u64, rect.height as u64, 2)?);
            }
            bytes = bytes
                .checked_add(maximum)
                .context("Scene timed staging estimate overflow")?;
        }
    }
    for layer in &scene.layers {
        if layer.text.is_some() {
            bytes = bytes
                .checked_add(rgba_bytes(
                    layer.size[0].ceil() as u64,
                    layer.size[1].ceil() as u64,
                    2,
                )?)
                .context("Scene text staging estimate overflow")?;
        }
    }
    for layer in &scene.particles {
        bytes = bytes
            .checked_add(
                (layer.vertex_capacity as u64)
                    .checked_mul(std::mem::size_of::<super::particles::ParticleVertex>() as u64)
                    .context("Scene geometry estimate overflow")?,
            )
            .context("Scene geometry estimate overflow")?;
    }
    Ok(bytes)
}
fn work_plan(scene: &PreparedScene) -> Result<BTreeMap<WorkKey, u64>> {
    let mut work = BTreeMap::new();
    let full = needs_full_frame(scene);
    for ((width, height), count) in scratch_layout(&scene.layers, scene.canvas, full) {
        for slot in 0..count {
            work.insert(
                WorkKey {
                    width,
                    height,
                    slot,
                },
                rgba_bytes(width as u64, height as u64, 1)?,
            );
        }
    }
    if full {
        let [width, height] = scene.canvas.map(|v| v.ceil() as i32);
        for slot in 0..2 {
            work.insert(
                WorkKey {
                    width,
                    height,
                    slot,
                },
                rgba_bytes(width as u64, height as u64, 1)?,
            );
        }
    }
    for layer in &scene.layers {
        let [width, height] = layer.size.map(|v| v.ceil() as i32);
        let offset = work_offset(layer, scene.canvas, full);
        for stage in &layer.stages {
            let mut slots = BTreeMap::<(i32, i32), usize>::new();
            for (name, scale) in &stage.targets {
                if stage.persistent_targets.contains(name) {
                    continue;
                }
                let [w, h] = target_dimensions(width, height, *scale);
                let ordinal = slots.entry((w, h)).or_insert(0);
                let slot = if [w, h] == [width, height] {
                    offset + *ordinal
                } else {
                    *ordinal
                };
                *ordinal += 1;
                work.insert(
                    WorkKey {
                        width: w,
                        height: h,
                        slot,
                    },
                    rgba_bytes(w as u64, h as u64, 1)?,
                );
            }
        }
    }
    Ok(work)
}
fn texture_fingerprint(texture: &PreparedTexture) -> [u8; 32] {
    // Hand-built authored test fixtures have no VFS source. Production
    // preparation always seals this identity on its background worker.
    #[cfg(test)]
    if texture.fingerprint == [0; 32] {
        return texture.fixture_fingerprint();
    }
    texture.fingerprint
}

pub(super) fn resource_plan(scene: &PreparedScene) -> Result<ResourcePlan> {
    let mut plan = ResourcePlan {
        private_bytes: private_gpu_bytes(scene)?,
        work: work_plan(scene)?,
        ..ResourcePlan::default()
    };
    for layer in &scene.layers {
        for pass in layer.stages.iter().flat_map(|stage| &stage.passes) {
            for (slot, binding) in pass.textures.iter().enumerate() {
                if let TextureBinding::Static(index) = binding {
                    let texture = scene
                        .textures
                        .get(*index)
                        .context("Scene texture index unavailable")?;
                    if texture.timed.is_some() {
                        continue;
                    }
                    let key = TextureKey {
                        fingerprint: texture_fingerprint(texture),
                        repeat: pass_texture_repeat_flags(&pass.label, slot, texture.source_flags),
                    };
                    let bytes = rgba_bytes(u64::from(texture.width), u64::from(texture.height), 1)?;
                    if let Some(previous) = plan.textures.insert(key, bytes) {
                        ensure!(
                            previous == bytes,
                            "Scene immutable allocation sizes disagree"
                        );
                    }
                }
            }
        }
    }
    for layer in &scene.particles {
        for pass in &layer.materials {
            if let Some(TextureBinding::Static(index)) = pass.textures.first() {
                let texture = scene
                    .textures
                    .get(*index)
                    .context("Particle texture unavailable")?;
                let key = TextureKey {
                    fingerprint: texture_fingerprint(texture),
                    repeat: false,
                };
                let bytes = rgba_bytes(u64::from(texture.width), u64::from(texture.height), 1)?;
                if let Some(previous) = plan.textures.insert(key, bytes) {
                    ensure!(
                        previous == bytes,
                        "Scene immutable allocation sizes disagree"
                    );
                }
            }
        }
    }
    Ok(plan)
}

pub(super) fn estimated_gpu_bytes(scene: &PreparedScene) -> Result<u64> {
    resource_plan(scene)?.total(0)
}

fn target_dimensions(width: i32, height: i32, scale: f32) -> [i32; 2] {
    [
        ((width as f32 / scale).ceil() as i32).max(1),
        ((height as f32 / scale).ceil() as i32).max(1),
    ]
}

pub(super) fn validate_gpu_budget(scene: &PreparedScene) -> Result<u64> {
    let bytes = estimated_gpu_bytes(scene)?;
    if bytes > MAX_SCENE_GPU_BYTES {
        return Err(lwe_scene::SceneAssetError::LimitExceeded {
            resource: "Scene GPU texture and framebuffer payload".into(),
            size: bytes,
            limit: MAX_SCENE_GPU_BYTES,
        }
        .into());
    }
    Ok(bytes)
}

/// GL state touched by Scene drawing is restored even on an early error.
/// GPU resources and this guard must remain on the context-owning thread.
#[derive(Debug, PartialEq)]
pub(super) struct GlState {
    program: i32,
    vao: i32,
    array_buffer: i32,
    unpack_buffer: i32,
    unpack_alignment: i32,
    unpack_layout: [i32; 5],
    polygon_mode: [i32; 2],
    active_texture: i32,
    textures: [i32; TEXTURE_SLOTS],
    samplers: Option<[i32; TEXTURE_SLOTS]>,
    draw_fbo: i32,
    read_fbo: i32,
    viewport: [i32; 4],
    color_mask: [u8; 4],
    depth_mask: u8,
    clear_color: [f32; 4],
    enabled: [bool; 8],
    blend_factors: [i32; 4],
    blend_equations: [i32; 2],
}

const CAPABILITIES: [u32; 8] = [
    gl::BLEND,
    gl::DEPTH_TEST,
    gl::STENCIL_TEST,
    gl::CULL_FACE,
    gl::SCISSOR_TEST,
    gl::FRAMEBUFFER_SRGB,
    gl::RASTERIZER_DISCARD,
    gl::DITHER,
];

impl GlState {
    pub(super) fn capture() -> Self {
        let mut state = Self {
            program: 0,
            vao: 0,
            array_buffer: 0,
            unpack_buffer: 0,
            unpack_alignment: 4,
            unpack_layout: [0; 5],
            polygon_mode: [0; 2],
            active_texture: 0,
            textures: [0; TEXTURE_SLOTS],
            samplers: None,
            draw_fbo: 0,
            read_fbo: 0,
            viewport: [0; 4],
            color_mask: [1; 4],
            depth_mask: 1,
            clear_color: [0.0; 4],
            enabled: [false; 8],
            blend_factors: [0; 4],
            blend_equations: [0; 2],
        };
        // SAFETY: the public initialization/render contract requires a current
        // context, and every pointer below addresses an appropriately sized array.
        unsafe {
            gl::GetIntegerv(gl::CURRENT_PROGRAM, &mut state.program);
            gl::GetIntegerv(gl::VERTEX_ARRAY_BINDING, &mut state.vao);
            gl::GetIntegerv(gl::ARRAY_BUFFER_BINDING, &mut state.array_buffer);
            gl::GetIntegerv(gl::PIXEL_UNPACK_BUFFER_BINDING, &mut state.unpack_buffer);
            gl::GetIntegerv(gl::UNPACK_ALIGNMENT, &mut state.unpack_alignment);
            for (index, parameter) in [
                gl::UNPACK_ROW_LENGTH,
                gl::UNPACK_SKIP_PIXELS,
                gl::UNPACK_SKIP_ROWS,
                gl::UNPACK_IMAGE_HEIGHT,
                gl::UNPACK_SKIP_IMAGES,
            ]
            .iter()
            .enumerate()
            {
                gl::GetIntegerv(*parameter, &mut state.unpack_layout[index]);
            }
            gl::GetIntegerv(gl::POLYGON_MODE, state.polygon_mode.as_mut_ptr());
            gl::GetIntegerv(gl::ACTIVE_TEXTURE, &mut state.active_texture);
            gl::GetIntegerv(gl::DRAW_FRAMEBUFFER_BINDING, &mut state.draw_fbo);
            gl::GetIntegerv(gl::READ_FRAMEBUFFER_BINDING, &mut state.read_fbo);
            gl::GetIntegerv(gl::VIEWPORT, state.viewport.as_mut_ptr());
            gl::GetBooleanv(gl::COLOR_WRITEMASK, state.color_mask.as_mut_ptr());
            gl::GetBooleanv(gl::DEPTH_WRITEMASK, &mut state.depth_mask);
            gl::GetFloatv(gl::COLOR_CLEAR_VALUE, state.clear_color.as_mut_ptr());
            for (index, capability) in CAPABILITIES.iter().enumerate() {
                state.enabled[index] = gl::IsEnabled(*capability) == gl::TRUE;
            }
            for (index, parameter) in [
                gl::BLEND_SRC_RGB,
                gl::BLEND_DST_RGB,
                gl::BLEND_SRC_ALPHA,
                gl::BLEND_DST_ALPHA,
            ]
            .iter()
            .enumerate()
            {
                gl::GetIntegerv(*parameter, &mut state.blend_factors[index]);
            }
            gl::GetIntegerv(gl::BLEND_EQUATION_RGB, &mut state.blend_equations[0]);
            gl::GetIntegerv(gl::BLEND_EQUATION_ALPHA, &mut state.blend_equations[1]);
            let mut major = 0;
            let mut minor = 0;
            gl::GetIntegerv(gl::MAJOR_VERSION, &mut major);
            gl::GetIntegerv(gl::MINOR_VERSION, &mut minor);
            if major > 3 || (major == 3 && minor >= 3) {
                state.samplers = Some([0; TEXTURE_SLOTS]);
            }
            for slot in 0..TEXTURE_SLOTS {
                gl::ActiveTexture(gl::TEXTURE0 + slot as u32);
                gl::GetIntegerv(gl::TEXTURE_BINDING_2D, &mut state.textures[slot]);
                if let Some(samplers) = state.samplers.as_mut() {
                    gl::GetIntegerv(gl::SAMPLER_BINDING, &mut samplers[slot]);
                }
            }
            gl::ActiveTexture(state.active_texture as u32);
        }
        state
    }

    pub(super) fn begin(&self) {
        // SAFETY: state capture established the current context on this thread.
        unsafe {
            for capability in CAPABILITIES {
                gl::Disable(capability);
            }
            gl::ColorMask(gl::TRUE, gl::TRUE, gl::TRUE, gl::TRUE);
            gl::DepthMask(gl::FALSE);
            gl::BlendEquationSeparate(gl::FUNC_ADD, gl::FUNC_ADD);
            gl::BindBuffer(gl::PIXEL_UNPACK_BUFFER, 0);
            gl::PixelStorei(gl::UNPACK_ALIGNMENT, 1);
            for parameter in [
                gl::UNPACK_ROW_LENGTH,
                gl::UNPACK_SKIP_PIXELS,
                gl::UNPACK_SKIP_ROWS,
                gl::UNPACK_IMAGE_HEIGHT,
                gl::UNPACK_SKIP_IMAGES,
            ] {
                gl::PixelStorei(parameter, 0);
            }
            gl::PolygonMode(gl::FRONT_AND_BACK, gl::FILL);
            if self.samplers.is_some() {
                for slot in 0..TEXTURE_SLOTS {
                    gl::BindSampler(slot as u32, 0);
                }
            }
        }
    }
}

impl Drop for GlState {
    fn drop(&mut self) {
        // SAFETY: guards never leave their context-owning thread or outlive it.
        unsafe {
            gl::UseProgram(self.program as u32);
            gl::BindVertexArray(self.vao as u32);
            gl::BindBuffer(gl::ARRAY_BUFFER, self.array_buffer as u32);
            gl::BindBuffer(gl::PIXEL_UNPACK_BUFFER, self.unpack_buffer as u32);
            gl::PixelStorei(gl::UNPACK_ALIGNMENT, self.unpack_alignment);
            for (index, parameter) in [
                gl::UNPACK_ROW_LENGTH,
                gl::UNPACK_SKIP_PIXELS,
                gl::UNPACK_SKIP_ROWS,
                gl::UNPACK_IMAGE_HEIGHT,
                gl::UNPACK_SKIP_IMAGES,
            ]
            .iter()
            .enumerate()
            {
                gl::PixelStorei(*parameter, self.unpack_layout[index]);
            }
            if self.polygon_mode[0] == self.polygon_mode[1] {
                gl::PolygonMode(gl::FRONT_AND_BACK, self.polygon_mode[0] as u32);
            } else {
                gl::PolygonMode(gl::FRONT, self.polygon_mode[0] as u32);
                gl::PolygonMode(gl::BACK, self.polygon_mode[1] as u32);
            }
            gl::BindFramebuffer(gl::DRAW_FRAMEBUFFER, self.draw_fbo as u32);
            gl::BindFramebuffer(gl::READ_FRAMEBUFFER, self.read_fbo as u32);
            gl::Viewport(
                self.viewport[0],
                self.viewport[1],
                self.viewport[2],
                self.viewport[3],
            );
            gl::ColorMask(
                self.color_mask[0],
                self.color_mask[1],
                self.color_mask[2],
                self.color_mask[3],
            );
            gl::DepthMask(self.depth_mask);
            gl::ClearColor(
                self.clear_color[0],
                self.clear_color[1],
                self.clear_color[2],
                self.clear_color[3],
            );
            gl::BlendFuncSeparate(
                self.blend_factors[0] as u32,
                self.blend_factors[1] as u32,
                self.blend_factors[2] as u32,
                self.blend_factors[3] as u32,
            );
            gl::BlendEquationSeparate(
                self.blend_equations[0] as u32,
                self.blend_equations[1] as u32,
            );
            for (index, capability) in CAPABILITIES.iter().enumerate() {
                if self.enabled[index] {
                    gl::Enable(*capability);
                } else {
                    gl::Disable(*capability);
                }
            }
            for slot in 0..TEXTURE_SLOTS {
                gl::ActiveTexture(gl::TEXTURE0 + slot as u32);
                gl::BindTexture(gl::TEXTURE_2D, self.textures[slot] as u32);
                if let Some(samplers) = self.samplers {
                    gl::BindSampler(slot as u32, samplers[slot] as u32);
                }
            }
            gl::ActiveTexture(self.active_texture as u32);
        }
    }
}

pub(super) struct GpuTexture {
    pub(super) id: u32,
    width: i32,
    height: i32,
    image_width: i32,
    image_height: i32,
    source_flags: u32,
    shared: Option<Rc<SharedTexture>>,
    uv_transform: Option<[f32; 4]>,
    virtual_resolution: Option<[f32; 4]>,
}

impl GpuTexture {
    fn new(
        width: i32,
        height: i32,
        image_width: i32,
        image_height: i32,
        source_flags: u32,
    ) -> Self {
        Self {
            id: 0,
            width,
            height,
            image_width,
            image_height,
            source_flags,
            shared: None,
            uv_transform: None,
            virtual_resolution: None,
        }
    }

    fn view(lease: Rc<SharedTexture>) -> Self {
        let texture = &lease.texture;
        Self {
            id: texture.id,
            width: texture.width,
            height: texture.height,
            image_width: texture.image_width,
            image_height: texture.image_height,
            source_flags: texture.source_flags,
            uv_transform: texture.uv_transform,
            virtual_resolution: texture.virtual_resolution,
            shared: Some(lease),
        }
    }

    fn allocate(width: i32, height: i32) -> Result<Self> {
        let mut result = Self::new(width, height, width, height, 2);
        unsafe {
            gl::GenTextures(1, &mut result.id);
            gl::BindTexture(gl::TEXTURE_2D, result.id);
            gl::TexImage2D(
                gl::TEXTURE_2D,
                0,
                gl::RGBA8 as i32,
                width,
                height,
                0,
                gl::RGBA,
                gl::UNSIGNED_BYTE,
                std::ptr::null(),
            );
            texture_parameters(false);
        }
        check_gl("allocating Scene work texture")?;
        Ok(result)
    }

    fn upload_text(text: &super::text::TextFrame) -> Result<Self> {
        let mut result = Self::new(
            text.width() as i32,
            text.height() as i32,
            text.width() as i32,
            text.height() as i32,
            2,
        );
        result.uv_transform = Some([1., -1., 0., 1.]);
        unsafe {
            gl::GenTextures(1, &mut result.id);
            gl::BindTexture(gl::TEXTURE_2D, result.id);
            gl::TexImage2D(
                gl::TEXTURE_2D,
                0,
                gl::RGBA8 as i32,
                result.width,
                result.height,
                0,
                gl::RGBA,
                gl::UNSIGNED_BYTE,
                text.rgba().as_ptr().cast(),
            );
            texture_parameters(false);
        }
        check_gl("uploading sealed Scene text")?;
        Ok(result)
    }
    fn upload_region(region: &super::timed_runtime::FrameRegion) -> Result<Self> {
        let roi = region.pixels.layout().rect();
        let mut texture = Self::new(
            roi.width as i32,
            roi.height as i32,
            roi.width as i32,
            roi.height as i32,
            7,
        );
        let frame = region.rect;
        texture.uv_transform = Some([
            frame.width as f32 / roi.width as f32,
            -(frame.height as f32) / roi.height as f32,
            (frame.x - roi.x) as f32 / roi.width as f32,
            (frame.y - roi.y + frame.height) as f32 / roi.height as f32,
        ]);
        texture.virtual_resolution = Some([
            region.virtual_size[0] as f32,
            region.virtual_size[1] as f32,
            region.virtual_size[0] as f32,
            region.virtual_size[1] as f32,
        ]);
        unsafe {
            gl::GenTextures(1, &mut texture.id);
            gl::BindTexture(gl::TEXTURE_2D, texture.id);
            gl::TexImage2D(
                gl::TEXTURE_2D,
                0,
                gl::RGBA8 as i32,
                texture.width,
                texture.height,
                0,
                gl::RGBA,
                gl::UNSIGNED_BYTE,
                region.pixels.rgba().as_ptr().cast(),
            );
            texture_parameters(false);
        }
        check_gl("uploading timed Scene region")?;
        Ok(texture)
    }

    fn upload(texture: &PreparedTexture) -> Result<Self> {
        ensure!(
            texture.width > 0 && texture.height > 0,
            "Invalid zero texture size"
        );
        let mut max_size = 0;
        // SAFETY: the caller initialized GL and holds a current context.
        unsafe {
            gl::GetIntegerv(gl::MAX_TEXTURE_SIZE, &mut max_size);
        }
        ensure!(
            texture.width <= max_size as u32 && texture.height <= max_size as u32,
            "Scene texture exceeds this GPU's maximum dimensions"
        );
        let rgba = texture.rgba();
        let row = texture.width as usize * 4;
        ensure!(
            rgba.len() == row * texture.height as usize,
            "Scene texture RGBA size mismatch"
        );
        // All uploaded images use GL's bottom-up origin. Pass outputs then share
        // the same UV convention, avoiding alternate-pass vertical flips.
        let mut reservation =
            super::timed_runtime::scene_texture_memory_budget().reserve_bytes(rgba.len())?;
        let mut pixels = Vec::with_capacity(rgba.len());
        reservation.resize(pixels.capacity())?;
        for line in rgba.chunks_exact(row).rev() {
            pixels.extend_from_slice(line);
        }
        let mut result = Self::new(
            texture.width as i32,
            texture.height as i32,
            texture.image_width as i32,
            texture.image_height as i32,
            texture.source_flags,
        );
        // SAFETY: pixels contains width*height*4 bytes; unpack PBO is disabled.
        unsafe {
            gl::GenTextures(1, &mut result.id);
            gl::BindTexture(gl::TEXTURE_2D, result.id);
            gl::TexImage2D(
                gl::TEXTURE_2D,
                0,
                gl::RGBA8 as i32,
                result.width,
                result.height,
                0,
                gl::RGBA,
                gl::UNSIGNED_BYTE,
                pixels.as_ptr().cast(),
            );
            texture_parameters(false);
        }
        check_gl("uploading Scene texture")?;
        Ok(result)
    }
    fn resolution(&self) -> [f32; 4] {
        if let Some(resolution) = self.virtual_resolution {
            return resolution;
        }
        [
            self.width as f32,
            self.height as f32,
            self.image_width as f32,
            self.image_height as f32,
        ]
    }
}
impl Drop for GpuTexture {
    fn drop(&mut self) {
        if self.id != 0 && self.shared.is_none() {
            unsafe {
                gl::DeleteTextures(1, &self.id);
            }
        }
    }
}

unsafe fn texture_parameters(repeat: bool) {
    // SAFETY: caller has a texture bound in the current context.
    unsafe {
        gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MIN_FILTER, gl::LINEAR as i32);
        gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MAG_FILTER, gl::LINEAR as i32);
        let wrap = if repeat {
            gl::REPEAT
        } else {
            gl::CLAMP_TO_EDGE
        };
        gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_S, wrap as i32);
        gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_T, wrap as i32);
    }
}

fn pass_texture_repeat_flags(label: &str, slot: usize, flags: u32) -> bool {
    if super::shader::is_legacy_image(label) {
        flags & 2 == 0
    } else {
        (matches!(label, "effects/shine_downsample2" | "effects/waterripple") && slot == 2)
            || (label == "effects/scroll" && slot == 0)
    }
}

#[cfg(test)]
fn pass_texture_repeat(label: &str, slot: usize, texture: &GpuTexture) -> bool {
    pass_texture_repeat_flags(label, slot, texture.source_flags)
}

struct TextureViews {
    source_flags: u32,
    timed: bool,
    key: Option<TextureKey>,
    frame: Option<usize>,
    clamp: Option<Rc<SharedTexture>>,
    repeat: Option<Rc<SharedTexture>>,
}

impl TextureViews {
    fn get(&self, repeat: bool) -> Result<&GpuTexture> {
        (if repeat { &self.repeat } else { &self.clamp })
            .as_ref()
            .map(|lease| &lease.texture)
            .context("Scene readonly sampling variant unavailable")
    }
}

pub(super) struct RenderTarget {
    texture: GpuTexture,
    fbo: u32,
}
impl RenderTarget {
    pub(super) fn new(width: i32, height: i32) -> Result<Self> {
        let mut max_size = 0;
        unsafe {
            gl::GetIntegerv(gl::MAX_TEXTURE_SIZE, &mut max_size);
        }
        ensure!(
            width > 0 && height > 0 && width <= max_size && height <= max_size,
            "Scene render target exceeds GPU dimensions"
        );
        let mut result = Self {
            texture: GpuTexture::new(width, height, width, height, 2),
            fbo: 0,
        };
        // SAFETY: dimensions were bounded; a null client pointer allocates storage.
        unsafe {
            gl::GenTextures(1, &mut result.texture.id);
            gl::BindTexture(gl::TEXTURE_2D, result.texture.id);
            gl::TexImage2D(
                gl::TEXTURE_2D,
                0,
                gl::RGBA8 as i32,
                width,
                height,
                0,
                gl::RGBA,
                gl::UNSIGNED_BYTE,
                std::ptr::null(),
            );
            texture_parameters(false);
            gl::GenFramebuffers(1, &mut result.fbo);
            gl::BindFramebuffer(gl::DRAW_FRAMEBUFFER, result.fbo);
            gl::FramebufferTexture2D(
                gl::DRAW_FRAMEBUFFER,
                gl::COLOR_ATTACHMENT0,
                gl::TEXTURE_2D,
                result.texture.id,
                0,
            );
            ensure!(
                gl::CheckFramebufferStatus(gl::DRAW_FRAMEBUFFER) == gl::FRAMEBUFFER_COMPLETE,
                "Scene framebuffer is incomplete"
            );
        }
        check_gl("allocating Scene framebuffer")?;
        Ok(result)
    }

    fn shared(width: i32, height: i32, slot: usize, pool: &TexturePool) -> Result<Self> {
        let bytes = rgba_bytes(width as u64, height as u64, 1)?;
        let lease = pool.acquire_work(
            WorkKey {
                width,
                height,
                slot,
            },
            bytes,
            || GpuTexture::allocate(width, height),
        )?;
        let mut result = Self {
            texture: GpuTexture::view(lease),
            fbo: 0,
        };
        unsafe {
            gl::GenFramebuffers(1, &mut result.fbo);
            gl::BindFramebuffer(gl::DRAW_FRAMEBUFFER, result.fbo);
            gl::FramebufferTexture2D(
                gl::DRAW_FRAMEBUFFER,
                gl::COLOR_ATTACHMENT0,
                gl::TEXTURE_2D,
                result.texture.id,
                0,
            );
            ensure!(
                gl::CheckFramebufferStatus(gl::DRAW_FRAMEBUFFER) == gl::FRAMEBUFFER_COMPLETE,
                "Scene shared framebuffer incomplete"
            );
        }
        check_gl("attaching Scene borrowed work framebuffer")?;
        Ok(result)
    }

    pub(super) fn framebuffer(&self) -> u32 {
        self.fbo
    }

    pub(super) fn texture_id(&self) -> u32 {
        self.texture.id
    }
}
impl Drop for RenderTarget {
    fn drop(&mut self) {
        if self.fbo != 0 {
            unsafe {
                gl::DeleteFramebuffers(1, &self.fbo);
            }
        }
    }
}

pub(super) struct Program {
    id: u32,
    locations: BTreeMap<String, i32>,
    array_lengths: BTreeMap<String, i32>,
}
impl Program {
    pub(super) fn compile(shader: &super::shader::Shader, label: &str) -> Result<Self> {
        let vertex = ShaderObject::compile(gl::VERTEX_SHADER, &shader.vertex, label)?;
        let fragment = ShaderObject::compile(gl::FRAGMENT_SHADER, &shader.fragment, label)?;
        let mut result = Self {
            id: unsafe { gl::CreateProgram() },
            locations: BTreeMap::new(),
            array_lengths: BTreeMap::new(),
        };
        ensure!(result.id != 0, "Cannot create Scene shader program");
        // SAFETY: shader/program names are owned by these RAII wrappers.
        unsafe {
            gl::AttachShader(result.id, vertex.0);
            gl::AttachShader(result.id, fragment.0);
            gl::BindAttribLocation(result.id, 0, c"a_Position".as_ptr());
            gl::BindAttribLocation(result.id, 1, c"a_TexCoord".as_ptr());
            gl::BindAttribLocation(result.id, 2, c"a_Color".as_ptr());
            gl::BindAttribLocation(result.id, 3, c"a_TexCoordNext".as_ptr());
            gl::BindAttribLocation(result.id, 4, c"a_TexCoordBlend".as_ptr());
            gl::LinkProgram(result.id);
            let mut linked = 0;
            gl::GetProgramiv(result.id, gl::LINK_STATUS, &mut linked);
            if linked == 0 {
                bail!(
                    "Scene shader {label} link failed: {}",
                    program_log(result.id)
                );
            }
        }
        // Uniform arrays can be shorter after link-time dead element removal.
        // Use the linked active range when a shader only needs a prefix of a
        // declared spectrum array.
        let mut uniform_count = 0;
        unsafe {
            gl::GetProgramiv(result.id, gl::ACTIVE_UNIFORMS, &mut uniform_count);
        }
        for index in 0..uniform_count {
            let mut name = [0u8; 256];
            let mut length = 0;
            let mut size = 0;
            let mut kind = 0;
            unsafe {
                gl::GetActiveUniform(
                    result.id,
                    index as u32,
                    name.len() as i32,
                    &mut length,
                    &mut size,
                    &mut kind,
                    name.as_mut_ptr().cast(),
                );
            }
            let name = std::str::from_utf8(&name[..length as usize])
                .context("Invalid linked Scene uniform name")?;
            if let Some(name) = name.strip_suffix("[0]")
                && matches!(
                    name,
                    "g_AudioSpectrum16Left"
                        | "g_AudioSpectrum16Right"
                        | "g_AudioSpectrum32Left"
                        | "g_AudioSpectrum32Right"
                        | "g_AudioSpectrum64Left"
                        | "g_AudioSpectrum64Right"
                )
            {
                ensure!(
                    kind == gl::FLOAT && (1..=64).contains(&size),
                    "Invalid linked Scene spectrum array"
                );
                result.array_lengths.insert(name.to_owned(), size);
            }
        }
        let mut names = vec![
            "g_ModelViewProjectionMatrix".to_owned(),
            "g_ModelMatrix".to_owned(),
            "g_Color4".to_owned(),
            "g_Time".to_owned(),
            "g_BlendMode".to_owned(),
        ];
        for slot in 0..TEXTURE_SLOTS {
            names.push(format!("g_Texture{slot}"));
            names.push(format!("g_Texture{slot}Resolution"));
        }
        names.extend(shader.uniforms.iter().map(|uniform| uniform.name.clone()));
        for name in names {
            let cname = CString::new(name.as_str())?;
            result.locations.insert(name, unsafe {
                gl::GetUniformLocation(result.id, cname.as_ptr())
            });
        }
        Ok(result)
    }
    pub(super) fn id(&self) -> u32 {
        self.id
    }

    pub(super) fn location(&self, name: &str) -> i32 {
        self.locations.get(name).copied().unwrap_or(-1)
    }
    pub(super) fn constants(&self, values: &BTreeMap<String, Vec<f32>>) -> Result<()> {
        for (name, values) in values {
            let location = self.location(name);
            if location < 0 {
                continue;
            }
            // SAFETY: uniform locations belong to the active program; component
            // counts were validated during CPU preparation.
            unsafe {
                match values.len() {
                    1 => gl::Uniform1f(location, values[0]),
                    2 => gl::Uniform2fv(location, 1, values.as_ptr()),
                    3 => gl::Uniform3fv(location, 1, values.as_ptr()),
                    4 => gl::Uniform4fv(location, 1, values.as_ptr()),
                    _ => bail!("Unsupported Scene uniform component count for {name}"),
                }
            }
        }
        Ok(())
    }
    pub(super) fn matrix(&self, value: &[f32; 16]) {
        unsafe {
            gl::UniformMatrix4fv(
                self.location("g_ModelViewProjectionMatrix"),
                1,
                gl::FALSE,
                value.as_ptr(),
            );
            gl::UniformMatrix4fv(
                self.location("g_ModelMatrix"),
                1,
                gl::FALSE,
                IDENTITY.as_ptr(),
            );
        }
    }
}
impl Drop for Program {
    fn drop(&mut self) {
        if self.id != 0 {
            unsafe {
                gl::DeleteProgram(self.id);
            }
        }
    }
}

struct ShaderObject(u32);
impl ShaderObject {
    fn compile(kind: u32, source: &str, label: &str) -> Result<Self> {
        let source = CString::new(source).context("Scene shader contains a NUL byte")?;
        let shader = Self(unsafe { gl::CreateShader(kind) });
        ensure!(shader.0 != 0, "Cannot create Scene shader");
        unsafe {
            gl::ShaderSource(shader.0, 1, &source.as_ptr(), std::ptr::null());
            gl::CompileShader(shader.0);
            let mut compiled = 0;
            gl::GetShaderiv(shader.0, gl::COMPILE_STATUS, &mut compiled);
            if compiled == 0 {
                bail!(
                    "Scene shader {label} compilation failed: {}",
                    shader_log(shader.0)
                );
            }
        }
        Ok(shader)
    }
}
impl Drop for ShaderObject {
    fn drop(&mut self) {
        unsafe {
            gl::DeleteShader(self.0);
        }
    }
}
unsafe fn shader_log(id: u32) -> String {
    let mut size = 0;
    unsafe {
        gl::GetShaderiv(id, gl::INFO_LOG_LENGTH, &mut size);
    }
    let mut bytes = vec![0u8; size.max(1) as usize];
    unsafe {
        gl::GetShaderInfoLog(id, size, std::ptr::null_mut(), bytes.as_mut_ptr().cast());
    }
    String::from_utf8_lossy(&bytes)
        .trim_end_matches('\0')
        .to_owned()
}
unsafe fn program_log(id: u32) -> String {
    let mut size = 0;
    unsafe {
        gl::GetProgramiv(id, gl::INFO_LOG_LENGTH, &mut size);
    }
    let mut bytes = vec![0u8; size.max(1) as usize];
    unsafe {
        gl::GetProgramInfoLog(id, size, std::ptr::null_mut(), bytes.as_mut_ptr().cast());
    }
    String::from_utf8_lossy(&bytes)
        .trim_end_matches('\0')
        .to_owned()
}

pub(super) fn check_gl(stage: &str) -> Result<()> {
    let error = unsafe { gl::GetError() };
    ensure!(
        error == gl::NO_ERROR,
        "OpenGL error 0x{error:x} while {stage}"
    );
    Ok(())
}

struct GpuPass {
    program: Option<Program>,
    kind: super::PreparedPassKind,
    textures: Vec<TextureBinding>,
    constants: BTreeMap<String, Vec<f32>>,
    target: Option<String>,
    blend: Blend,
    label: String,
}
impl GpuPass {
    fn new(pass: &PreparedPass) -> Result<Self> {
        let program = pass
            .shader
            .as_ref()
            .map(|shader| {
                let variant = super::shader::specialize_owned_image_blend(
                    shader,
                    shader_blend_mode(pass.blend),
                );
                Program::compile(variant.as_ref().unwrap_or(shader), &pass.label)
            })
            .transpose()?;
        if let Some(program) = &program {
            ensure!(
                program.location("g_PointerPosition") < 0,
                "Scene shader needs an unavailable pointer input host"
            );
            for (slot, binding) in pass.textures.iter().enumerate() {
                ensure!(
                    !matches!(binding, TextureBinding::Black)
                        || program.location(&format!("g_Texture{slot}")) < 0,
                    "Scene shader {} requires unbound sampler g_Texture{slot}",
                    pass.label
                );
            }
        }
        Ok(Self {
            program,
            kind: pass.kind.clone(),
            textures: pass.textures.clone(),
            constants: pass.constants.clone(),
            target: pass.target.clone(),
            blend: pass.blend,
            label: pass.label.clone(),
        })
    }
}
struct GpuStage {
    passes: Vec<GpuPass>,
    targets: BTreeMap<String, RenderTarget>,
    specs: BTreeMap<String, f32>,
    persistent: std::collections::BTreeSet<String>,
}
impl GpuStage {
    fn new(
        stage: &PreparedStage,
        width: i32,
        height: i32,
        offset: usize,
        pool: &TexturePool,
    ) -> Result<Self> {
        let mut targets = BTreeMap::new();
        let mut slots = BTreeMap::<(i32, i32), usize>::new();
        for (name, scale) in &stage.targets {
            let [w, h] = target_dimensions(width, height, *scale);
            let target = if stage.persistent_targets.contains(name) {
                RenderTarget::new(w, h)?
            } else {
                let slot = slots.entry((w, h)).or_insert(0);
                let index = if [w, h] == [width, height] {
                    offset + *slot
                } else {
                    *slot
                };
                *slot += 1;
                RenderTarget::shared(w, h, index, pool)?
            };
            if stage.persistent_targets.contains(name) {
                unsafe {
                    gl::BindFramebuffer(gl::DRAW_FRAMEBUFFER, target.fbo);
                    gl::ClearColor(0., 0., 0., 0.);
                    gl::Clear(gl::COLOR_BUFFER_BIT);
                }
                check_gl("initializing Scene history")?;
            }
            targets.insert(name.clone(), target);
        }
        Ok(Self {
            specs: stage.targets.clone(),
            persistent: stage.persistent_targets.clone(),
            passes: stage
                .passes
                .iter()
                .map(GpuPass::new)
                .collect::<Result<_>>()?,
            targets,
        })
    }
}
struct GpuLayer {
    size: [f32; 2],
    origin: [f32; 2],
    scale: [f32; 2],
    angle: f32,
    tint: [f32; 4],
    blend: Blend,
    stages: Vec<GpuStage>,
    composite: Option<Program>,
    work_offset: usize,
    direct: bool,
    origin_z: f32,
    angles: [f32; 3],
}
impl GpuLayer {
    fn can_sample_current_frame(&self) -> bool {
        // Only staged layers reserve both canvas work slots. Their FullFrame
        // reads finish before the copy compositor writes the current frame.
        !self.direct && self.composite.is_none() && self.work_offset >= 2
    }

    fn new(
        layer: &PreparedLayer,
        canvas: [f32; 2],
        full_frame: bool,
        pool: &TexturePool,
    ) -> Result<Self> {
        // Crop offsets describe the already-authored layer bounds. Applying
        // them again would translate a cropped model twice. Unpadded animated
        // models use exact frame extents; combined crop/padding remains gated.
        ensure!(
            !layer.no_padding || layer.crop_offset == [0.; 2],
            "Unsupported cropped unpadded Scene model"
        );
        ensure!(
            !layer.clamp_uvs
                || !layer
                    .stages
                    .iter()
                    .flat_map(|stage| &stage.passes)
                    .any(|pass| super::shader::is_legacy_image(&pass.label)),
            "Unsupported legacy shader clamp-UV override"
        );
        let width = layer.size[0].ceil() as i32;
        let height = layer.size[1].ceil() as i32;
        let work_offset = work_offset(layer, canvas, full_frame);
        let stages = layer
            .stages
            .iter()
            .map(|stage| GpuStage::new(stage, width, height, work_offset, pool))
            .collect::<Result<_>>()?;
        Ok(Self {
            direct: direct_layer(layer),
            work_offset,
            origin_z: layer.origin_z,
            angles: layer.angles,
            size: layer.size,
            origin: layer.origin,
            scale: layer.scale,
            angle: layer.angle,
            tint: layer.tint,
            blend: layer.blend,
            stages,
            composite: layer
                .composite
                .as_ref()
                .map(|shader| Program::compile(shader, "LWE authored color blend"))
                .transpose()?,
        })
    }
}

struct GpuParticleLayer {
    runtime: super::particles::ParticleRuntime,
    materials: Vec<GpuPass>,
    vao: u32,
    vbo: u32,
    capacity: usize,
}

impl GpuParticleLayer {
    fn new(layer: &super::PreparedParticleLayer, seed: u64) -> Result<Self> {
        let mut runtime = super::particles::ParticleRuntime::new(&layer.simulation, seed)
            .with_context(|| {
                format!(
                    "Initializing particle system {}",
                    layer.simulation.systems[0].path
                )
            })?;
        runtime.configure_material_aspects(&layer.aspects)?;
        runtime.configure_material_animations(&layer.animations)?;
        let mut result = Self {
            runtime,
            materials: layer
                .materials
                .iter()
                .map(GpuPass::new)
                .collect::<Result<_>>()?,
            vao: 0,
            vbo: 0,
            capacity: layer.vertex_capacity,
        };
        // SAFETY: initialization requires a current context. The bounded,
        // repr(C) ParticleVertex supplies bounded geometry and two atlas UVs.
        unsafe {
            gl::GenVertexArrays(1, &mut result.vao);
            gl::GenBuffers(1, &mut result.vbo);
            ensure!(
                result.vao != 0 && result.vbo != 0,
                "Cannot allocate Scene particle geometry"
            );
            gl::BindVertexArray(result.vao);
            gl::BindBuffer(gl::ARRAY_BUFFER, result.vbo);
            gl::BufferData(
                gl::ARRAY_BUFFER,
                (result.capacity * std::mem::size_of::<super::particles::ParticleVertex>())
                    as isize,
                std::ptr::null(),
                gl::STREAM_DRAW,
            );
            for (index, components, offset) in
                [(0, 2, 0), (1, 2, 8), (2, 4, 28), (3, 2, 16), (4, 1, 24)]
            {
                gl::EnableVertexAttribArray(index);
                gl::VertexAttribPointer(
                    index,
                    components,
                    gl::FLOAT,
                    gl::FALSE,
                    std::mem::size_of::<super::particles::ParticleVertex>() as i32,
                    offset as *const _,
                );
            }
        }
        check_gl("allocating Scene particle geometry")?;
        Ok(result)
    }

    fn draw(
        &mut self,
        seconds: f64,
        canvas: [f32; 2],
        width: i32,
        height: i32,
        target: u32,
        textures: &[TextureViews],
    ) -> Result<()> {
        let frame = self.runtime.advance(seconds)?;
        let matrix = particle_matrix(canvas, width, height);
        for batch in frame.batches {
            ensure!(
                batch.vertices.len() <= self.capacity,
                "Scene particle geometry exceeds its GPU allocation"
            );
            if batch.vertices.is_empty() {
                continue;
            }
            let pass = self
                .materials
                .get(batch.material)
                .context("Particle material index unavailable")?;
            let program = pass
                .program
                .as_ref()
                .context("Particle render program unavailable")?;
            let TextureBinding::Static(index) = pass.textures[0] else {
                bail!("Particle primary texture is not static")
            };
            let texture = textures
                .get(index)
                .context("Particle texture unavailable")?
                .get(false)?;
            // SAFETY: the validated vertex slice supplies exactly the stated
            // bounded byte count; this VBO retains its allocated capacity.
            unsafe {
                gl::BindVertexArray(self.vao);
                gl::BindBuffer(gl::ARRAY_BUFFER, self.vbo);
                gl::BufferSubData(
                    gl::ARRAY_BUFFER,
                    0,
                    std::mem::size_of_val(batch.vertices.as_slice()) as isize,
                    batch.vertices.as_ptr().cast(),
                );
                gl::BindFramebuffer(gl::DRAW_FRAMEBUFFER, target);
                gl::Viewport(0, 0, width, height);
                gl::UseProgram(program.id);
                set_blend(pass.blend);
                gl::ActiveTexture(gl::TEXTURE0);
                gl::BindTexture(gl::TEXTURE_2D, texture.id);
                gl::Uniform1i(program.location("g_Texture0"), 0);
            }
            program.matrix(&matrix);
            program.constants(&pass.constants)?;
            unsafe {
                gl::DrawArrays(gl::TRIANGLES, 0, batch.vertices.len() as i32);
            }
        }
        check_gl("drawing Scene particles")
    }
}

impl Drop for GpuParticleLayer {
    fn drop(&mut self) {
        // SAFETY: the owning SceneRenderer drops before its current GL context.
        unsafe {
            if self.vbo != 0 {
                gl::DeleteBuffers(1, &self.vbo);
            }
            if self.vao != 0 {
                gl::DeleteVertexArrays(1, &self.vao);
            }
        }
    }
}

fn particle_matrix(canvas: [f32; 2], width: i32, height: i32) -> [f32; 16] {
    let output_ratio = width as f32 / height as f32;
    let canvas_ratio = canvas[0] / canvas[1];
    let visible = if output_ratio > canvas_ratio {
        [canvas[0], canvas[0] / output_ratio]
    } else {
        [canvas[1] * output_ratio, canvas[1]]
    };
    [
        2.0 / visible[0],
        0.0,
        0.0,
        0.0,
        0.0,
        2.0 / visible[1],
        0.0,
        0.0,
        0.0,
        0.0,
        1.0,
        0.0,
        -canvas[0] / visible[0],
        -canvas[1] / visible[1],
        0.0,
        1.0,
    ]
}

/// Not Send/Sync: GL initialization, rendering and destruction stay on the
/// context-owning thread. The associated PreparedScene is entirely CPU-only.
pub struct SceneRenderer {
    canvas: [f32; 2],
    clear: [f32; 4],
    textures: Vec<TextureViews>,
    layers: Vec<GpuLayer>,
    particles: Vec<GpuParticleLayer>,
    draw_order: Vec<super::DrawObject>,
    black: Option<GpuTexture>,
    copy: Option<Program>,
    vao: u32,
    vbo: u32,
    animated: bool,
    paused: bool,
    drawn: bool,
    last_size: [i32; 2],
    time: f64,
    last_input: Option<f64>,
    rebase_time: bool,
    gpu_payload_bytes: u64,
    resource_plan: ResourcePlan,
    texture_pool: Rc<TexturePool>,
    prepared_bloom: Option<Arc<super::postprocess::PreparedBloom>>,
    bloom: Option<super::postprocess::BloomRenderer>,
    scratch: BTreeMap<(i32, i32), Vec<RenderTarget>>,
    full_frame: Vec<RenderTarget>,
    spectrum: Option<super::spectrum::Spectrum>,
    analysis_active: bool,
    last_spectrum: super::spectrum::SpectrumSnapshot,
    requested_spectrum: super::spectrum::SpectrumSnapshot,
    packet_spectrum: super::spectrum::SpectrumSnapshot,
    accepted_spectrum: super::spectrum::SpectrumSnapshot,
    scripts: Option<super::dynamic_script::ScriptWorker>,
    script_layers: BTreeMap<usize, usize>,
    hidden_script_layers: std::collections::BTreeSet<usize>,
    scripts_ready: bool,
    timed_sources: Vec<super::timed_runtime::Source>,
    text_sources: Vec<super::timed_runtime::TextSource>,
    texture_worker: Option<super::timed_runtime::TimedWorker>,
    worker_wake: Option<super::worker_wake::WorkerWake>,
    packet: Option<super::dynamic_script::FrameOutput>,
    saved_packet: Option<super::dynamic_script::FrameOutput>,
    job_requested: bool,
    ahead_timed: bool,
    vm_requested: bool,
    next_seq: u64,
    accepted_seq: u64,
    accepted_time: f64,
    accepted_text: BTreeMap<usize, String>,
    front: Option<RenderTarget>,
    back: Option<RenderTarget>,
    previous_front: Option<RenderTarget>,
    cache_valid: bool,
    awaiting_swap: bool,
    frozen: bool,
    quarantined: bool,
    timed_reservation: u64,
    frozen_histories: BTreeMap<(usize, usize, String), frozen_history::FrozenHistory>,
    history_reservation: u64,
    work_layout: BTreeMap<(i32, i32), usize>,
    full_frame_needed: bool,
    _context_thread: PhantomData<Rc<()>>,
}

impl SceneRenderer {
    pub(super) fn initialize(scene: &PreparedScene) -> Result<Self> {
        Self::initialize_shared(scene, TexturePool::new()?)
    }

    pub(crate) fn initialize_shared(
        scene: &PreparedScene,
        texture_pool: Rc<TexturePool>,
    ) -> Result<Self> {
        Self::initialize_shared_with_wake(scene, texture_pool, None)
    }

    pub(crate) fn initialize_shared_with_wake(
        scene: &PreparedScene,
        texture_pool: Rc<TexturePool>,
        worker_wake: Option<super::worker_wake::WorkerWake>,
    ) -> Result<Self> {
        texture_pool.ensure_current()?;
        let plan = resource_plan(scene)?;
        let gpu_payload_bytes = validate_gpu_budget(scene)?;
        plan.preflight(texture_pool.retired_bytes()?, 0)?;
        ensure!(
            gl::GetString::is_loaded() && !unsafe { gl::GetString(gl::VERSION) }.is_null(),
            "Scene initialization requires a current OpenGL context"
        );
        let guard = GlState::capture();
        guard.begin();
        let mut renderer = Self {
            canvas: scene.canvas,
            clear: scene.clear,
            textures: Vec::new(),
            layers: Vec::new(),
            particles: Vec::new(),
            draw_order: scene.draw_order.clone(),
            black: None,
            copy: None,
            vao: 0,
            vbo: 0,
            animated: scene.summary().animated,
            paused: false,
            drawn: false,
            last_size: [0; 2],
            time: 0.0,
            last_input: None,
            rebase_time: false,
            gpu_payload_bytes,
            resource_plan: plan.clone(),
            texture_pool,
            prepared_bloom: scene.bloom.clone(),
            bloom: None,
            scratch: BTreeMap::new(),
            full_frame: Vec::new(),
            spectrum: None,
            analysis_active: false,
            last_spectrum: super::spectrum::SpectrumSnapshot::default(),
            requested_spectrum: super::spectrum::SpectrumSnapshot::default(),
            packet_spectrum: super::spectrum::SpectrumSnapshot::default(),
            accepted_spectrum: super::spectrum::SpectrumSnapshot::default(),
            scripts: scene
                .scripts
                .as_ref()
                .map(|plan| {
                    super::dynamic_script::ScriptWorker::spawn_with_wake(plan, worker_wake.clone())
                })
                .transpose()?,
            script_layers: scene.script_layers.clone(),
            hidden_script_layers: std::collections::BTreeSet::new(),
            scripts_ready: false,
            timed_sources: scene
                .textures
                .iter()
                .enumerate()
                .filter_map(|(index, t)| {
                    t.timed.as_ref().map(|source| super::timed_runtime::Source {
                        index,
                        fingerprint: texture_fingerprint(t),
                        source: source.clone(),
                    })
                })
                .collect(),
            text_sources: scene
                .layers
                .iter()
                .enumerate()
                .filter_map(|(layer, l)| {
                    l.text.as_ref().and_then(|prepared| {
                        l.stages
                            .first()?
                            .passes
                            .first()?
                            .textures
                            .first()
                            .and_then(|b| match b {
                                TextureBinding::Static(index) => {
                                    Some(super::timed_runtime::TextSource {
                                        layer,
                                        index: *index,
                                        prepared: prepared.clone(),
                                        value: String::new(),
                                    })
                                }
                                _ => None,
                            })
                    })
                })
                .collect(),
            texture_worker: None,
            worker_wake,
            packet: None,
            saved_packet: None,
            job_requested: false,
            ahead_timed: false,
            vm_requested: false,
            next_seq: 0,
            accepted_seq: 0,
            accepted_time: 0.,
            accepted_text: BTreeMap::new(),
            front: None,
            back: None,
            previous_front: None,
            cache_valid: false,
            awaiting_swap: false,
            frozen: false,
            quarantined: false,
            timed_reservation: scene
                .textures
                .iter()
                .filter_map(|t| t.timed.as_ref())
                .try_fold(0_u64, |sum, source| {
                    let maximum = source
                        .frames()
                        .iter()
                        .enumerate()
                        .map(|(i, _)| {
                            let r = source.frame_neighborhood(i, 1)?.rect();
                            rgba_bytes(r.width as u64, r.height as u64, 2)
                        })
                        .collect::<Result<Vec<_>>>()?
                        .into_iter()
                        .max()
                        .unwrap_or(0);
                    sum.checked_add(maximum)
                        .context("Scene timed allocation overflow")
                })?,
            frozen_histories: BTreeMap::new(),
            history_reservation: scene
                .layers
                .iter()
                .flat_map(|l| l.stages.iter().map(move |s| (l, s)))
                .try_fold(0u64, |sum, (layer, stage)| {
                    stage.persistent_targets.iter().try_fold(sum, |sum, name| {
                        let [w, h] = target_dimensions(
                            layer.size[0].ceil() as i32,
                            layer.size[1].ceil() as i32,
                            stage.targets[name],
                        );
                        sum.checked_add(rgba_bytes(w as u64, h as u64, 1)?)
                            .context("Scene history estimate overflow")
                    })
                })?,
            work_layout: scratch_layout(&scene.layers, scene.canvas, needs_full_frame(scene)),
            full_frame_needed: needs_full_frame(scene),
            _context_thread: PhantomData,
        };
        renderer.textures = scene
            .textures
            .iter()
            .map(|texture| {
                let mut views = TextureViews {
                    source_flags: texture.source_flags,
                    timed: texture.timed.is_some(),
                    key: None,
                    frame: None,
                    clamp: None,
                    repeat: None,
                };
                for repeat in [false, true] {
                    let key = TextureKey {
                        fingerprint: texture_fingerprint(texture),
                        repeat,
                    };
                    if let Some(bytes) = plan.textures.get(&key) {
                        let lease = renderer.texture_pool.acquire(key, *bytes, || {
                            let uploaded = if let Some(text) = &texture.text_frame {
                                GpuTexture::upload_text(text)?
                            } else {
                                GpuTexture::upload(texture)?
                            };
                            unsafe {
                                texture_parameters(repeat);
                            }
                            check_gl("setting immutable Scene sampling policy")?;
                            Ok(uploaded)
                        })?;
                        if repeat {
                            views.repeat = Some(lease);
                        } else {
                            views.clamp = Some(lease);
                        }
                    }
                }
                Ok(views)
            })
            .collect::<Result<_>>()?;
        renderer.black = Some(GpuTexture::upload(&PreparedTexture {
            fingerprint: [0; 32],
            width: 1,
            height: 1,
            image_width: 1,
            image_height: 1,
            source_format: 0,
            source_flags: 2,
            animation: None,
            timed: None,
            static_source: None,
            image: None,
            text_frame: None,
            _rgba_reservation: None,
            rgba: vec![0, 0, 0, 0],
        })?);
        renderer.copy = Some(Program::compile(
            &super::shader::image(),
            "LWE image composite",
        )?);
        unsafe {
            gl::GenVertexArrays(1, &mut renderer.vao);
            gl::BindVertexArray(renderer.vao);
            gl::GenBuffers(1, &mut renderer.vbo);
            gl::BindBuffer(gl::ARRAY_BUFFER, renderer.vbo);
            gl::BufferData(
                gl::ARRAY_BUFFER,
                std::mem::size_of_val(&QUAD) as isize,
                QUAD.as_ptr().cast(),
                gl::STATIC_DRAW,
            );
            gl::EnableVertexAttribArray(0);
            gl::VertexAttribPointer(0, 3, gl::FLOAT, gl::FALSE, 20, std::ptr::null());
            gl::EnableVertexAttribArray(1);
            gl::VertexAttribPointer(1, 2, gl::FLOAT, gl::FALSE, 20, (12usize) as *const _);
        }
        renderer.layers = scene
            .layers
            .iter()
            .map(|layer| {
                GpuLayer::new(
                    layer,
                    scene.canvas,
                    needs_full_frame(scene),
                    &renderer.texture_pool,
                )
            })
            .collect::<Result<_>>()?;
        for (dimensions, count) in
            scratch_layout(&scene.layers, scene.canvas, needs_full_frame(scene))
        {
            renderer.scratch.insert(
                dimensions,
                (0..count)
                    .map(|slot| {
                        RenderTarget::shared(
                            dimensions.0,
                            dimensions.1,
                            slot,
                            &renderer.texture_pool,
                        )
                    })
                    .collect::<Result<_>>()?,
            );
        }
        if needs_full_frame(scene) {
            renderer.full_frame = (0..2)
                .map(|slot| {
                    RenderTarget::shared(
                        scene.canvas[0].ceil() as i32,
                        scene.canvas[1].ceil() as i32,
                        slot,
                        &renderer.texture_pool,
                    )
                })
                .collect::<Result<_>>()?;
        }
        if scene
            .layers
            .iter()
            .flat_map(|layer| &layer.stages)
            .flat_map(|stage| &stage.passes)
            .flat_map(|pass| pass.shader.iter().flat_map(|s| &s.uniforms))
            .any(super::shader::is_audio_spectrum)
            || scene
                .scripts
                .as_ref()
                .is_some_and(|scripts| scripts.uses_audio())
        {
            renderer.spectrum = Some(super::spectrum::Spectrum::new()?);
        }
        renderer.particles = scene
            .particles
            .iter()
            .enumerate()
            .map(|(index, layer)| {
                GpuParticleLayer::new(layer, 0x9e3779b97f4a7c15_u64.wrapping_mul(index as u64 + 1))
            })
            .collect::<Result<_>>()?;
        check_gl("initializing Scene renderer")?;
        Ok(renderer)
    }

    fn rebuild_work(&mut self) -> Result<()> {
        if !self.frozen_histories.is_empty() {
            // Keep all sealed CPU snapshots if any upload fails. Only a complete
            // restore is installed, then the CPU owners release their leases.
            let restored = self
                .frozen_histories
                .iter()
                .map(|(key, pixels)| Ok((key.clone(), pixels.restore()?)))
                .collect::<Result<Vec<_>>>()?;
            for ((layer, stage, name), target) in restored {
                self.layers[layer].stages[stage]
                    .targets
                    .insert(name, target);
            }
            self.frozen_histories.clear();
        }
        if self.scratch.is_empty() {
            for (&(w, h), &count) in &self.work_layout {
                self.scratch.insert(
                    (w, h),
                    (0..count)
                        .map(|slot| RenderTarget::shared(w, h, slot, &self.texture_pool))
                        .collect::<Result<_>>()?,
                );
            }
        }
        if self.full_frame_needed && self.full_frame.is_empty() {
            let [w, h] = self.canvas.map(|v| v.ceil() as i32);
            self.full_frame = (0..2)
                .map(|slot| RenderTarget::shared(w, h, slot, &self.texture_pool))
                .collect::<Result<_>>()?;
        }
        for layer in &mut self.layers {
            for stage in &mut layer.stages {
                let mut slots = BTreeMap::<(i32, i32), usize>::new();
                for (name, scale) in &stage.specs {
                    if stage.persistent.contains(name) {
                        continue;
                    }
                    let [w, h] = target_dimensions(
                        layer.size[0].ceil() as i32,
                        layer.size[1].ceil() as i32,
                        *scale,
                    );
                    let ordinal = slots.entry((w, h)).or_insert(0);
                    let slot = if [w, h] == layer.size.map(|v| v.ceil() as i32) {
                        layer.work_offset + *ordinal
                    } else {
                        *ordinal
                    };
                    *ordinal += 1;
                    if !stage.targets.contains_key(name) {
                        stage.targets.insert(
                            name.clone(),
                            RenderTarget::shared(w, h, slot, &self.texture_pool)?,
                        );
                    }
                }
            }
        }
        Ok(())
    }
    pub(crate) fn frozen_allocation_plan(&self) -> Result<ResourcePlan> {
        let mut plan = self.resource_plan.clone();
        plan.work.clear();
        plan.private_bytes = plan
            .private_bytes
            .checked_sub(self.timed_reservation)
            .and_then(|n| n.checked_sub(self.history_reservation))
            .context("Scene frozen reservation underflow")?;
        if let Some(front) = self
            .front
            .as_ref()
            .filter(|_| self.cache_valid)
            .or(self.previous_front.as_ref())
        {
            plan.private_bytes = plan
                .private_bytes
                .checked_add(rgba_bytes(
                    front.texture.width as u64,
                    front.texture.height as u64,
                    1,
                )?)
                .context("Scene frozen cache estimate overflow")?;
        }
        Ok(plan)
    }
    /// Keep actual presented pixels/history/VM; release only reconstructible inputs.
    pub(crate) fn freeze_for_rollback(&mut self) -> Result<()> {
        self.freeze_with_budget(super::timed_runtime::scene_texture_memory_budget())
    }
    fn freeze_with_budget(&mut self, budget: &lwe_scene::AtlasMemoryBudget) -> Result<()> {
        self.texture_pool.ensure_current()?;
        if self.frozen {
            return Ok(());
        }
        self.texture_worker = None;
        self.job_requested = false;
        self.ahead_timed = false;
        // Reserve/read every history before releasing any of the old targets.
        // A failed budget or readback leaves its live histories and front intact.
        let mut snapshots = BTreeMap::new();
        for (layer_index, layer) in self.layers.iter().enumerate() {
            for (stage_index, stage) in layer.stages.iter().enumerate() {
                for name in &stage.persistent {
                    let target = stage
                        .targets
                        .get(name)
                        .context("Scene live history unavailable for freeze")?;
                    snapshots.insert(
                        (layer_index, stage_index, name.clone()),
                        frozen_history::FrozenHistory::capture(target, budget)?,
                    );
                }
            }
        }
        self.frozen_histories = snapshots;
        // A resize may have installed an unpresented front while a CPU packet
        // is still pending. Retain exactly the last presented cache counted by
        // frozen_allocation_plan; its invalid replacement has no rollback use.
        if !self.cache_valid {
            self.front = self.previous_front.take();
            self.cache_valid = self.front.is_some();
        } else {
            self.previous_front = None;
        }
        self.pause();
        self.frozen = true;
        self.back = None;
        self.bloom = None;
        self.scratch.clear();
        self.full_frame.clear();
        for layer in &mut self.layers {
            for stage in &mut layer.stages {
                stage.targets.clear();
            }
        }
        for view in &mut self.textures {
            if view.timed {
                view.clamp = None;
                view.repeat = None;
                view.key = None;
                view.frame = None;
            }
        }
        self.texture_pool.drain_retired()?;
        Ok(())
    }
    pub(crate) fn thaw(&mut self, paused: bool) {
        self.frozen = false;
        if !paused {
            self.resume();
        }
    }
    pub(crate) fn report_presented(&mut self) {
        if self.awaiting_swap {
            let first_frame = !self.cache_valid && self.previous_front.is_none();
            std::mem::swap(&mut self.front, &mut self.back);
            self.previous_front = None;
            self.cache_valid = true;
            self.awaiting_swap = false;
            self.drawn = true;
            if first_frame && self.accepted_seq == 0 {
                self.time = 0.;
                self.rebase_time = true;
            }
        }
    }
    pub(crate) fn report_present_failed(&mut self) {
        // VM callbacks and unique history copies are not reversible within a
        // generation. A failed swap keeps only its last presented image usable;
        // the engine rejects this renderer instead of resuming advanced state.
        self.quarantined = true;
        self.awaiting_swap = false;
    }
    fn present_cache(&self, target: u32, width: i32, height: i32) -> Result<()> {
        let source = self
            .front
            .as_ref()
            .filter(|_| self.cache_valid)
            .or(self.previous_front.as_ref())
            .context("Scene presented cache unavailable")?;
        unsafe {
            gl::BindFramebuffer(gl::READ_FRAMEBUFFER, source.fbo);
            gl::BindFramebuffer(gl::DRAW_FRAMEBUFFER, target);
            gl::BlitFramebuffer(
                0,
                0,
                source.texture.width,
                source.texture.height,
                0,
                0,
                width,
                height,
                gl::COLOR_BUFFER_BIT,
                gl::NEAREST,
            );
        }
        check_gl("presenting retained Scene pixels")
    }
    pub fn pause(&mut self) {
        self.cancel_ahead_timed();
        self.paused = true;
        if let Some(spectrum) = &self.spectrum {
            spectrum.set_paused(true);
        }
    }

    pub(crate) fn activate_analysis(&mut self, paused: bool) {
        self.analysis_active = true;
        if let Some(spectrum) = &self.spectrum {
            spectrum.set_paused(paused);
        }
    }

    pub(crate) fn deactivate_analysis(&mut self) {
        self.analysis_active = false;
        if let Some(spectrum) = &self.spectrum {
            spectrum.set_paused(true);
        }
    }

    pub(crate) fn allocation_plan(&self) -> Result<ResourcePlan> {
        if self.frozen {
            return self.frozen_allocation_plan();
        }
        let mut plan = self.resource_plan.clone();
        for target in self
            .front
            .iter()
            .chain(self.back.iter())
            .chain(self.previous_front.iter())
        {
            plan.private_bytes = plan
                .private_bytes
                .checked_add(rgba_bytes(
                    target.texture.width as u64,
                    target.texture.height as u64,
                    1,
                )?)
                .context("Scene output cache allocation overflow")?;
        }
        plan.private_bytes = plan
            .private_bytes
            .checked_add(self.bloom.as_ref().map_or(0, |b| b.payload_bytes()))
            .context("Scene bloom allocation overflow")?;
        Ok(plan)
    }
    pub(crate) fn retained_history_allocations(&self) -> impl Iterator<Item = (usize, u64)> + '_ {
        self.frozen_histories
            .values()
            .map(|h| (h.allocation_id(), h.capacity_bytes() as u64))
    }
    pub fn gpu_payload_bytes(&self) -> u64 {
        self.allocation_plan()
            .and_then(|p| p.total(0))
            .unwrap_or(u64::MAX)
    }
    pub(crate) fn additional_gpu_bytes(&self, width: i32, height: i32) -> Result<u64> {
        let caches = if self
            .front
            .as_ref()
            .is_some_and(|f| [f.texture.width, f.texture.height] == [width, height])
        {
            if self.back.is_none() {
                output_cache_bytes(width, height, 1)?
            } else {
                0
            }
        } else {
            output_cache_bytes(width, height, 2)?
        };
        let bloom = if self
            .bloom
            .as_ref()
            .is_some_and(|b| b.dimensions() == [width, height])
        {
            0
        } else {
            self.prepared_bloom
                .as_ref()
                .map(|p| p.estimated_bytes(width, height))
                .transpose()?
                .unwrap_or_default()
        };
        caches
            .checked_add(bloom)
            .context("Scene resize estimate overflow")
    }
    pub fn resume(&mut self) {
        self.frozen = false;
        if self.paused {
            self.paused = false;
            self.rebase_time = true;
            if self.analysis_active
                && let Some(spectrum) = &self.spectrum
            {
                spectrum.set_paused(false);
            }
        }
    }
    pub fn resize(&mut self, width: i32, height: i32) -> Result<()> {
        self.texture_pool.ensure_current()?;
        let state = GlState::capture();
        state.begin();
        ensure!(width > 0 && height > 0, "Invalid Scene output dimensions");
        let additional = self.additional_gpu_bytes(width, height)?;
        self.allocation_plan()?
            .preflight(self.texture_pool.retired_bytes()?, additional)?;
        let dimensions_match = self
            .front
            .as_ref()
            .is_some_and(|f| [f.texture.width, f.texture.height] == [width, height]);
        let replacement = if !dimensions_match {
            Some((
                RenderTarget::new(width, height)?,
                RenderTarget::new(width, height)?,
            ))
        } else {
            None
        };
        let new_back = if dimensions_match && self.back.is_none() {
            Some(RenderTarget::new(width, height)?)
        } else {
            None
        };
        let bloom = if let Some(prepared) = self.prepared_bloom.as_ref()
            && self
                .bloom
                .as_ref()
                .is_none_or(|b| b.dimensions() != [width, height])
        {
            Some(super::postprocess::BloomRenderer::initialize(
                prepared, width, height,
            )?)
        } else {
            None
        };
        if let Some((front, back)) = replacement {
            if self.cache_valid {
                self.previous_front = self.front.take();
            }
            self.front = Some(front);
            self.back = Some(back);
            self.cache_valid = false;
        }
        if let Some(back) = new_back {
            self.back = Some(back);
        }
        if let Some(bloom) = bloom {
            self.bloom = Some(bloom);
        }
        if self.last_size != [width, height] {
            self.drawn = false;
        }
        self.rebuild_work()?;
        Ok(())
    }
    pub fn update(&mut self, time_seconds: f64) -> Result<()> {
        ensure!(
            time_seconds.is_finite() && time_seconds >= 0.0,
            "Invalid Scene animation time"
        );
        if !self.paused {
            if let Some(previous) = self.last_input {
                if !self.rebase_time {
                    self.time += animation_delta(previous, time_seconds);
                }
            } else {
                self.time = 0.0;
            }
        }
        self.last_input = Some(time_seconds);
        self.rebase_time = false;
        Ok(())
    }

    /// Draws to the caller's current draw framebuffer. A true result means a
    /// complete valid frame was drawn; EGL swap/first-frame ACK remain host-owned.
    pub fn render(&mut self, width: i32, height: i32, time_seconds: f64) -> Result<bool> {
        let result = self.render_internal(width, height, time_seconds, false, None);
        if result.is_err() {
            self.quarantined = true;
        }
        result
    }

    /// Re-presents the retained scene after a rejected candidate overwrote the
    /// output. Paused scenes keep their animation time but still draw this frame.
    pub fn render_forced(&mut self, width: i32, height: i32, time_seconds: f64) -> Result<bool> {
        let result = self.render_internal(width, height, time_seconds, true, None);
        if result.is_err() {
            self.quarantined = true;
        }
        result
    }

    pub(crate) fn take_worker_ready(&self) -> Option<std::time::Duration> {
        self.worker_wake.as_ref().and_then(|wake| wake.take())
    }

    /// Prepare one next CPU epoch without touching GL or accepted state. The
    /// output mailbox retains timed pixels and its permit until gated upload.
    pub(crate) fn prepare_ahead(&mut self, time_seconds: f64, allow_timed: bool) -> Result<()> {
        if !allow_timed {
            self.cancel_ahead_timed();
        }
        if self.paused
            || self.frozen
            || self.quarantined
            || self.awaiting_swap
            || !self.drawn
            || !self.scripts_ready
            || (self.scripts.is_none() && self.timed_sources.is_empty())
        {
            return Ok(());
        }
        let result = (|| {
            if self.packet.is_none() && !self.vm_requested {
                self.update(time_seconds)?;
                if self.analysis_active {
                    self.last_spectrum = self
                        .spectrum
                        .as_ref()
                        .map(|s| s.snapshot())
                        .unwrap_or_default();
                }
            }
            self.poll_cpu_packet(false, allow_timed, true).map(|_| ())
        })();
        if result.is_err() {
            self.quarantined = true;
        }
        result
    }

    /// Drop only speculative timed work. Its VM packet remains tagged for a
    /// later gated request; normal presentation jobs keep their existing policy.
    pub(crate) fn cancel_ahead_timed(&mut self) {
        if self.ahead_timed {
            self.texture_worker = None;
            self.job_requested = false;
            self.ahead_timed = false;
        }
    }

    // True means a packet exists and any required timed job has been queued,
    // not that its output is ready. Only prepare_packet consumes timed results.
    fn poll_cpu_packet(&mut self, replay: bool, allow_timed: bool, ahead: bool) -> Result<bool> {
        if replay
            && self.packet.as_ref().is_some_and(|p| {
                p.seq != self.accepted_seq || p.time.to_bits() != self.accepted_time.to_bits()
            })
        {
            self.saved_packet = self.packet.take();
        }
        if self.packet.is_none() {
            if replay {
                self.packet = Some(super::dynamic_script::FrameOutput {
                    seq: self.accepted_seq,
                    time: self.accepted_time,
                    states: Vec::new(),
                });
            } else if let Some(worker) = &self.scripts {
                if let Some(packet) = worker.take_frame()? {
                    self.packet_spectrum = if packet.seq == 0 {
                        super::spectrum::SpectrumSnapshot::default()
                    } else {
                        self.requested_spectrum
                    };
                    self.packet = Some(packet);
                    self.vm_requested = false;
                } else {
                    if self.scripts_ready && !self.vm_requested {
                        self.vm_requested =
                            worker.submit_frame(self.next_seq, self.time, self.last_spectrum)?;
                        if self.vm_requested {
                            self.requested_spectrum = self.last_spectrum;
                        }
                    }
                    return Ok(false);
                }
            } else {
                self.packet_spectrum = self.last_spectrum;
                self.packet = Some(super::dynamic_script::FrameOutput {
                    seq: self.next_seq,
                    time: if !self.scripts_ready { 0. } else { self.time },
                    states: Vec::new(),
                });
            }
        }
        if self.job_requested {
            return Ok(true);
        }
        let packet = self
            .packet
            .as_ref()
            .context("Scene frame packet unavailable")?;
        let sources = self
            .timed_sources
            .iter()
            .filter_map(|source| match source.source.frame_at(packet.time) {
                Ok(frame) if self.textures[source.index].frame == Some(frame) => None,
                Ok(_) => Some(Ok(source.clone())),
                Err(error) => Some(Err(anyhow::Error::from(error))),
            })
            .collect::<Result<Vec<_>>>()?;
        let mut texts = Vec::new();
        for source in &self.text_sources {
            if let Some(value) = packet
                .states
                .iter()
                .find(|state| self.script_layers.get(&state.handle) == Some(&source.layer))
                .and_then(|state| state.text.as_ref())
                && self.accepted_text.get(&source.layer) != Some(value)
            {
                let mut source = source.clone();
                source.value = value.clone();
                texts.push(source);
            }
        }
        if !sources.is_empty() || !texts.is_empty() {
            if !allow_timed {
                return Ok(false);
            }
            if self.texture_worker.is_none() {
                self.texture_worker = Some(super::timed_runtime::TimedWorker::new_with_wake(
                    self.worker_wake.clone(),
                )?);
            }
            let worker = self.texture_worker.as_mut().unwrap();
            self.job_requested = worker.request(packet.seq, packet.time, &sources, texts)?;
            if !self.job_requested {
                return Ok(false);
            }
            self.ahead_timed = ahead;
        }
        Ok(true)
    }

    fn prepare_packet(&mut self, replay: bool) -> Result<bool> {
        if replay
            && self
                .textures
                .iter()
                .filter(|v| v.timed)
                .all(|v| v.clamp.is_some())
        {
            return Ok(true);
        }
        if replay {
            // A reconstructed accepted frame cannot consume a next-epoch job.
            self.cancel_ahead_timed();
        }
        if !self.poll_cpu_packet(replay, true, false)? {
            return Ok(false);
        }
        if self.job_requested {
            let packet = self
                .packet
                .as_ref()
                .context("Scene frame packet unavailable")?;
            let worker = self
                .texture_worker
                .as_mut()
                .context("Scene texture worker unavailable")?;
            let Some(ready) = worker.take()? else {
                return Ok(false);
            };
            ensure!(
                ready.seq == packet.seq && ready.time.to_bits() == packet.time.to_bits(),
                "Scene texture job belongs to a different frame epoch"
            );
            // All replacement leases are acquired before mutating accepted views.
            let mut regions = Vec::new();
            for region in &ready.regions {
                let bytes = rgba_bytes(
                    region.pixels.layout().rect().width as u64,
                    region.pixels.layout().rect().height as u64,
                    1,
                )?;
                let lease = self
                    .texture_pool
                    .acquire(region.key, bytes, || GpuTexture::upload_region(region))?;
                regions.push((region.index, region.frame, region.key, lease));
            }
            let mut frames = Vec::new();
            for text in &ready.texts {
                let bytes = rgba_bytes(text.pixels.width() as u64, text.pixels.height() as u64, 1)?;
                let lease = self
                    .texture_pool
                    .acquire(text.key, bytes, || GpuTexture::upload_text(&text.pixels))?;
                frames.push((text.index, text.layer, text.value.clone(), text.key, lease));
            }
            for (index, frame, key, lease) in regions {
                let view = &mut self.textures[index];
                view.clamp = Some(lease);
                view.frame = Some(frame);
                view.key = Some(key);
            }
            for (index, layer, value, key, lease) in frames {
                let view = &mut self.textures[index];
                view.clamp = Some(lease);
                view.key = Some(key);
                self.accepted_text.insert(layer, value);
            }
            self.job_requested = false;
            self.ahead_timed = false;
        }
        let packet = self.packet.take().unwrap();
        if !replay {
            for state in packet.states {
                let Some(index) = self.script_layers.get(&state.handle).copied() else {
                    continue;
                };
                let layer = &mut self.layers[index];
                ensure!(
                    state.angles[0] == layer.angles[0] && state.angles[1] == layer.angles[1],
                    "Script attempted unsupported changing planar rotation"
                );
                ensure!(
                    state.origin[2] == layer.origin_z,
                    "Script attempted unsupported changing image depth"
                );
                layer.origin = [state.origin[0], state.origin[1]];
                layer.scale = [state.scale[0], state.scale[1]];
                layer.angle = state.angles[2];
                layer.angles[2] = state.angles[2];
                layer.tint = [state.color[0], state.color[1], state.color[2], state.alpha];
                if state.visible {
                    self.hidden_script_layers.remove(&index);
                } else {
                    self.hidden_script_layers.insert(index);
                }
            }
            self.accepted_seq = packet.seq;
            self.accepted_time = packet.time;
            self.accepted_spectrum = self.packet_spectrum;
            self.next_seq = packet
                .seq
                .checked_add(1)
                .context("Scene frame sequence exhausted")?;
            self.scripts_ready = true;
        }
        if replay {
            self.packet = self.saved_packet.take();
        }
        self.texture_pool.drain_retired()?;
        Ok(true)
    }
    fn render_internal(
        &mut self,
        width: i32,
        height: i32,
        time_seconds: f64,
        force: bool,
        spectrum_override: Option<super::spectrum::SpectrumSnapshot>,
    ) -> Result<bool> {
        self.texture_pool.ensure_current()?;
        let cached = self
            .front
            .as_ref()
            .filter(|_| self.cache_valid)
            .or(self.previous_front.as_ref());
        let same_size =
            cached.is_some_and(|f| [f.texture.width, f.texture.height] == [width, height]);
        if same_size && force {
            let guard = GlState::capture();
            guard.begin();
            self.present_cache(guard.draw_fbo as u32, width, height)?;
            return Ok(true);
        }
        ensure!(
            !self.quarantined,
            "Scene frame generation is quarantined after a failed render"
        );
        if same_size && (self.frozen || self.paused || (self.drawn && !self.animated)) {
            if !force {
                return Ok(false);
            }
            let guard = GlState::capture();
            guard.begin();
            self.present_cache(guard.draw_fbo as u32, width, height)?;
            return Ok(true);
        }
        if self.awaiting_swap {
            bail!("Scene frame has not been acknowledged by a successful swap");
        }
        let replay = (force || self.paused) && self.scripts_ready;
        let guard = GlState::capture();
        guard.begin();
        self.resize(width, height)?;
        self.update(time_seconds)?;
        if let Some(spectrum) = &self.spectrum {
            spectrum.set_paused(self.paused || !self.analysis_active);
        }
        if !replay {
            if let Some(spectrum) = spectrum_override {
                self.last_spectrum = spectrum;
            } else if self.analysis_active && !self.paused {
                self.last_spectrum = self
                    .spectrum
                    .as_ref()
                    .map(|s| s.snapshot())
                    .unwrap_or_default();
            }
        }
        if !self.prepare_packet(replay)? {
            return Ok(false);
        }
        let caller_target = guard.draw_fbo as u32;
        let final_target = self
            .back
            .as_ref()
            .context("Scene attempt cache unavailable")?
            .fbo;
        let output_target = self
            .bloom
            .as_ref()
            .map(|b| b.scene_framebuffer())
            .unwrap_or(final_target);
        let composition_size = if self.full_frame.is_empty() {
            [width, height]
        } else {
            self.canvas.map(|v| v.ceil() as i32)
        };
        let target = self
            .full_frame
            .first()
            .map(|f| f.fbo)
            .unwrap_or(output_target);
        let spectrum = self.accepted_spectrum;
        let black = self
            .black
            .as_ref()
            .context("Scene renderer has been cleaned up")?;
        let copy = self
            .copy
            .as_ref()
            .context("Scene renderer has been cleaned up")?;
        unsafe {
            gl::BindVertexArray(self.vao);
            gl::BindBuffer(gl::ARRAY_BUFFER, self.vbo);
            gl::BindFramebuffer(gl::DRAW_FRAMEBUFFER, target);
            gl::Viewport(0, 0, composition_size[0], composition_size[1]);
            gl::ClearColor(self.clear[0], self.clear[1], self.clear[2], self.clear[3]);
            gl::Clear(gl::COLOR_BUFFER_BIT);
        }
        let inputs = TextureInputs {
            textures: &self.textures,
            black,
            full_frame: self.full_frame.get(1).map(|frame| &frame.texture),
            replay,
        };
        for object in &self.draw_order {
            let super::DrawObject::Image(index) = *object else {
                let super::DrawObject::Particles(index) = *object else {
                    unreachable!()
                };
                self.particles[index].draw(
                    self.accepted_time,
                    self.canvas,
                    composition_size[0],
                    composition_size[1],
                    target,
                    &self.textures,
                )?;
                // Particle geometry uses a separate VAO. Restore the fullscreen
                // quad before subsequent image/effect passes.
                unsafe {
                    gl::BindVertexArray(self.vao);
                    gl::BindBuffer(gl::ARRAY_BUFFER, self.vbo);
                }
                continue;
            };
            let layer = &self.layers[index];
            if self.hidden_script_layers.contains(&index)
                || layer.scale[0] == 0.0
                || layer.scale[1] == 0.0
            {
                continue;
            }
            let needs_frame = self.full_frame.len() == 2
                && (layer.composite.is_some()
                    || layer
                        .stages
                        .iter()
                        .flat_map(|stage| &stage.passes)
                        .flat_map(|pass| &pass.textures)
                        .any(|binding| matches!(binding, TextureBinding::FullFrame)));
            let sample_current = needs_frame && layer.can_sample_current_frame();
            let inputs = TextureInputs {
                full_frame: self
                    .full_frame
                    .get(usize::from(!sample_current))
                    .map(|frame| &frame.texture),
                ..inputs
            };
            if needs_frame && !sample_current {
                unsafe {
                    gl::BindFramebuffer(gl::READ_FRAMEBUFFER, target);
                    gl::BindFramebuffer(gl::DRAW_FRAMEBUFFER, self.full_frame[1].fbo);
                    gl::BlitFramebuffer(
                        0,
                        0,
                        composition_size[0],
                        composition_size[1],
                        0,
                        0,
                        composition_size[0],
                        composition_size[1],
                        gl::COLOR_BUFFER_BIT,
                        gl::NEAREST,
                    );
                }
            }
            if layer.direct {
                render_direct(
                    layer,
                    &inputs,
                    self.accepted_time as f32,
                    &spectrum,
                    self.canvas,
                    composition_size,
                    target,
                )?;
                continue;
            }
            let scratch = self
                .scratch
                .get(&(layer.size[0].ceil() as i32, layer.size[1].ceil() as i32))
                .context("Scene shared scratch targets unavailable")?;
            let output = render_layer(
                layer,
                &inputs,
                self.accepted_time as f32,
                scratch,
                &spectrum,
            )?;
            let composite = layer.composite.as_ref().unwrap_or(copy);
            unsafe {
                gl::BindFramebuffer(gl::DRAW_FRAMEBUFFER, target);
                gl::Viewport(0, 0, composition_size[0], composition_size[1]);
                gl::UseProgram(composite.id);
                set_blend(layer.blend);
                gl::ActiveTexture(gl::TEXTURE0);
                gl::BindTexture(gl::TEXTURE_2D, output.id);
                gl::Uniform1i(composite.location("g_Texture0"), 0);
                gl::Uniform4fv(
                    composite.location("g_Texture0Resolution"),
                    1,
                    output.resolution().as_ptr(),
                );
                gl::Uniform4f(composite.location("g_Color4"), 1.0, 1.0, 1.0, 1.0);
                if let Some(frame) = self.full_frame.get(1) {
                    gl::ActiveTexture(gl::TEXTURE4);
                    gl::BindTexture(gl::TEXTURE_2D, frame.texture.id);
                    texture_parameters(false);
                    gl::Uniform1i(composite.location("g_Texture4"), 4);
                }
            }
            composite.matrix(&layer_matrix(
                self.canvas,
                layer,
                composition_size[0],
                composition_size[1],
            ));
            unsafe {
                gl::DrawArrays(gl::TRIANGLES, 0, 6);
            }
        }
        if let Some(frame) = self.full_frame.first() {
            unsafe {
                gl::BindFramebuffer(gl::DRAW_FRAMEBUFFER, output_target);
                gl::Viewport(0, 0, width, height);
                gl::Disable(gl::BLEND);
                gl::UseProgram(copy.id);
                gl::ActiveTexture(gl::TEXTURE0);
                gl::BindTexture(gl::TEXTURE_2D, frame.texture.id);
                texture_parameters(false);
                gl::Uniform1i(copy.location("g_Texture0"), 0);
                gl::Uniform4fv(
                    copy.location("g_Texture0Resolution"),
                    1,
                    frame.texture.resolution().as_ptr(),
                );
                gl::Uniform4f(copy.location("g_Color4"), 1.0, 1.0, 1.0, 1.0);
                gl::Uniform1i(copy.location("g_BlendMode"), 0);
            }
            copy.matrix(&cover_matrix(self.canvas, width, height));
            unsafe {
                gl::DrawArrays(gl::TRIANGLES, 0, 6);
            }
        }
        if let Some(bloom) = &self.bloom {
            bloom.composite_to(final_target)?;
        }
        check_gl("rendering Scene frame")?;
        // The attempted image becomes presented only after EGL swap succeeds.
        unsafe {
            gl::BindFramebuffer(gl::READ_FRAMEBUFFER, final_target);
            gl::BindFramebuffer(gl::DRAW_FRAMEBUFFER, caller_target);
            gl::BlitFramebuffer(
                0,
                0,
                width,
                height,
                0,
                0,
                width,
                height,
                gl::COLOR_BUFFER_BIT,
                gl::NEAREST,
            );
        }
        check_gl("copying completed Scene output")?;
        self.awaiting_swap = true;
        self.last_size = [width, height];
        Ok(true)
    }

    /// The host must make this renderer's context current before calling this
    /// method or dropping it. Cleanup is idempotent and precedes EGL destruction.
    pub fn cleanup(&mut self) {
        self.texture_worker = None;
        self.job_requested = false;
        self.ahead_timed = false;
        self.packet = None;
        self.saved_packet = None;
        self.front = None;
        self.back = None;
        self.previous_front = None;
        self.timed_sources.clear();
        self.text_sources.clear();
        self.scripts = None;
        self.script_layers.clear();
        self.hidden_script_layers.clear();
        self.scripts_ready = false;
        self.bloom = None;
        self.spectrum = None;
        self.full_frame.clear();
        self.scratch.clear();
        self.particles.clear();
        self.layers.clear();
        self.textures.clear();
        if let Err(error) = self.texture_pool.drain_retired() {
            tracing::warn!("Shared Scene texture cleanup deferred: {error:#}");
        }
        self.resource_plan = ResourcePlan::default();
        self.copy = None;
        self.black = None;
        unsafe {
            if self.vbo != 0 {
                gl::DeleteBuffers(1, &self.vbo);
                self.vbo = 0;
            }
            if self.vao != 0 {
                gl::DeleteVertexArrays(1, &self.vao);
                self.vao = 0;
            }
        }
        self.drawn = false;
        self.frozen = false;
        self.timed_reservation = 0;
        self.history_reservation = 0;
        self.frozen_histories.clear();
        self.gpu_payload_bytes = 0;
    }
}
impl Drop for SceneRenderer {
    fn drop(&mut self) {
        self.cleanup();
    }
}

fn layer_matrix(canvas: [f32; 2], layer: &GpuLayer, width: i32, height: i32) -> [f32; 16] {
    // Cover preserves wallpaper aspect on differently shaped monitor outputs.
    let output_ratio = width as f32 / height as f32;
    let canvas_ratio = canvas[0] / canvas[1];
    let visible = if output_ratio > canvas_ratio {
        [canvas[0], canvas[0] / output_ratio]
    } else {
        [canvas[1] * output_ratio, canvas[1]]
    };
    let (sx, cx) = layer.angles[0].sin_cos();
    let (sy, cy) = layer.angles[1].sin_cos();
    let (sz, cz) = layer.angle.sin_cos();
    let horizontal = layer.size[0] * layer.scale[0];
    let vertical = layer.size[1] * layer.scale[1];
    [
        cz * cy * horizontal / visible[0],
        sz * cy * horizontal / visible[1],
        -sy * horizontal / visible[0],
        0.,
        (cz * sy * sx - sz * cx) * vertical / visible[0],
        (sz * sy * sx + cz * cx) * vertical / visible[1],
        cy * sx * vertical / visible[1],
        0.,
        cz * sy * cx + sz * sx,
        sz * sy * cx - cz * sx,
        cy * cx,
        0.,
        2. * (layer.origin[0] - canvas[0] / 2.) / visible[0],
        2. * (layer.origin[1] - canvas[1] / 2.) / visible[1],
        layer.origin_z / visible[1],
        1.,
    ]
}

fn cover_matrix(canvas: [f32; 2], width: i32, height: i32) -> [f32; 16] {
    let output_ratio = width as f32 / height as f32;
    let canvas_ratio = canvas[0] / canvas[1];
    let mut matrix = IDENTITY;
    if output_ratio > canvas_ratio {
        matrix[5] = output_ratio / canvas_ratio;
    } else {
        matrix[0] = canvas_ratio / output_ratio;
    }
    matrix
}

unsafe fn set_blend(blend: Blend) {
    unsafe {
        match blend {
            Blend::Replace => gl::Disable(gl::BLEND),
            Blend::Alpha => {
                gl::Enable(gl::BLEND);
                gl::BlendFuncSeparate(
                    gl::SRC_ALPHA,
                    gl::ONE_MINUS_SRC_ALPHA,
                    gl::ONE,
                    gl::ONE_MINUS_SRC_ALPHA,
                );
            }
            Blend::Add => {
                gl::Enable(gl::BLEND);
                gl::BlendFuncSeparate(gl::SRC_ALPHA, gl::ONE, gl::ONE, gl::ONE);
            }
        }
    }
}

struct TextureInputs<'a> {
    textures: &'a [TextureViews],
    black: &'a GpuTexture,
    full_frame: Option<&'a GpuTexture>,
    replay: bool,
}
fn resolve_binding<'a>(
    binding: &TextureBinding,
    stage: &'a GpuStage,
    input: Option<&'a GpuTexture>,
    inputs: &TextureInputs<'a>,
    label: &str,
    slot: usize,
) -> Result<&'a GpuTexture> {
    match binding {
        TextureBinding::Static(index) => {
            let texture = inputs
                .textures
                .get(*index)
                .context("Scene texture index unavailable")?;
            texture
                .get(!texture.timed && pass_texture_repeat_flags(label, slot, texture.source_flags))
        }
        TextureBinding::Input | TextureBinding::LayerComposite => Ok(input.unwrap_or(inputs.black)),
        TextureBinding::Named(name) | TextureBinding::History(name) => {
            let name = if inputs.replay {
                stage
                    .passes
                    .iter()
                    .find_map(|p| match &p.kind {
                        super::PreparedPassKind::Copy {
                            source: TextureBinding::Named(source),
                        } if source == name => p.target.as_ref(),
                        _ => None,
                    })
                    .unwrap_or(name)
            } else {
                name
            };
            stage
                .targets
                .get(name)
                .map(|target| &target.texture)
                .context("Scene named/history target unavailable")
        }
        TextureBinding::Black => Ok(inputs.black),
        TextureBinding::FullFrame => inputs
            .full_frame
            .context("Scene full-frame texture unavailable"),
    }
}
enum CopySource<'a> {
    Texture(&'a GpuTexture),
    Target(&'a RenderTarget),
}

fn copy_texture(source: CopySource<'_>, target: &RenderTarget) -> Result<()> {
    let (source, mut read, temporary) = match source {
        CopySource::Texture(texture) => (texture, 0, true),
        CopySource::Target(target) => (&target.texture, target.fbo, false),
    };
    ensure!(source.id != target.texture.id, "Scene copy feedback loop");
    ensure!(
        [source.width, source.height] == [target.texture.width, target.texture.height],
        "Scene copy dimensions disagree"
    );
    unsafe {
        if temporary {
            gl::GenFramebuffers(1, &mut read);
            gl::BindFramebuffer(gl::READ_FRAMEBUFFER, read);
            gl::FramebufferTexture2D(
                gl::READ_FRAMEBUFFER,
                gl::COLOR_ATTACHMENT0,
                gl::TEXTURE_2D,
                source.id,
                0,
            );
            if gl::CheckFramebufferStatus(gl::READ_FRAMEBUFFER) != gl::FRAMEBUFFER_COMPLETE {
                gl::DeleteFramebuffers(1, &read);
                bail!("Scene copy input framebuffer incomplete");
            }
        } else {
            // RenderTarget construction validated its immutable attachment.
            gl::BindFramebuffer(gl::READ_FRAMEBUFFER, read);
        }
        gl::BindFramebuffer(gl::DRAW_FRAMEBUFFER, target.fbo);
        gl::BlitFramebuffer(
            0,
            0,
            source.width,
            source.height,
            0,
            0,
            target.texture.width,
            target.texture.height,
            gl::COLOR_BUFFER_BIT,
            gl::NEAREST,
        );
        if temporary {
            gl::DeleteFramebuffers(1, &read);
        }
    }
    check_gl("copying persistent Scene history")
}
fn shader_blend_mode(blend: Blend) -> i32 {
    match blend {
        Blend::Replace => 0,
        Blend::Alpha => 1,
        Blend::Add => 2,
    }
}

fn bind_pass(
    program: &Program,
    pass: &GpuPass,
    resolved: &[&GpuTexture],
    time: f32,
    spectrum: &super::spectrum::SpectrumSnapshot,
    tint: [f32; 4],
) -> Result<()> {
    unsafe {
        gl::UseProgram(program.id);
        for (slot, texture) in resolved.iter().enumerate() {
            if program.location(&format!("g_Texture{slot}")) < 0 {
                continue;
            }
            gl::ActiveTexture(gl::TEXTURE0 + slot as u32);
            gl::BindTexture(gl::TEXTURE_2D, texture.id);
            if !matches!(pass.textures[slot], TextureBinding::Static(_)) {
                texture_parameters(pass_texture_repeat_flags(
                    &pass.label,
                    slot,
                    texture.source_flags,
                ));
            }
            gl::Uniform1i(program.location(&format!("g_Texture{slot}")), slot as i32);
            gl::Uniform4fv(
                program.location(&format!("g_Texture{slot}Resolution")),
                1,
                texture.resolution().as_ptr(),
            );
            gl::Uniform4fv(
                program.location(&format!("g_Texture{slot}UVTransform")),
                1,
                texture.uv_transform.unwrap_or([1., 1., 0., 0.]).as_ptr(),
            );
        }
        gl::Uniform1f(program.location("g_Time"), time);
        for (name, values) in [
            ("g_AudioSpectrum16Left", spectrum.left.as_slice()),
            ("g_AudioSpectrum16Right", spectrum.right.as_slice()),
            ("g_AudioSpectrum32Left", spectrum.left32.as_slice()),
            ("g_AudioSpectrum32Right", spectrum.right32.as_slice()),
            ("g_AudioSpectrum64Left", spectrum.script_left.as_slice()),
            ("g_AudioSpectrum64Right", spectrum.script_right.as_slice()),
        ] {
            let count = program
                .array_lengths
                .get(name)
                .copied()
                .unwrap_or(values.len() as i32)
                .min(values.len() as i32);
            gl::Uniform1fv(program.location(name), count, values.as_ptr());
        }
        let primary = resolved
            .first()
            .context("Scene primary sampler unavailable")?;
        gl::Uniform2f(
            program.location("g_TexelSize"),
            1.0 / primary.width as f32,
            1.0 / primary.height as f32,
        );
        gl::Uniform2f(program.location("g_PointerPosition"), 0., 0.);
        gl::UniformMatrix4fv(
            program.location("g_ModelViewProjectionMatrixInverse"),
            1,
            gl::FALSE,
            IDENTITY.as_ptr(),
        );
        gl::Uniform1i(
            program.location("g_BlendMode"),
            shader_blend_mode(pass.blend),
        );
        gl::Uniform4fv(program.location("g_Color4"), 1, tint.as_ptr());
    }
    program.constants(&pass.constants)
}
fn render_direct(
    layer: &GpuLayer,
    inputs: &TextureInputs<'_>,
    time: f32,
    spectrum: &super::spectrum::SpectrumSnapshot,
    canvas: [f32; 2],
    size: [i32; 2],
    target: u32,
) -> Result<()> {
    let stage = &layer.stages[0];
    let pass = &stage.passes[0];
    let program = pass
        .program
        .as_ref()
        .context("Scene direct shader unavailable")?;
    let resolved = pass
        .textures
        .iter()
        .enumerate()
        .map(|(slot, b)| resolve_binding(b, stage, None, inputs, &pass.label, slot))
        .collect::<Result<Vec<_>>>()?;
    unsafe {
        gl::BindFramebuffer(gl::DRAW_FRAMEBUFFER, target);
        gl::Viewport(0, 0, size[0], size[1]);
        set_blend(layer.blend);
    }
    bind_pass(program, pass, &resolved, time, spectrum, layer.tint)?;
    program.matrix(&layer_matrix(canvas, layer, size[0], size[1]));
    unsafe {
        gl::DrawArrays(gl::TRIANGLES, 0, 6);
    }
    check_gl("drawing direct Scene layer")
}
fn pass_reads(pass: &GpuPass) -> impl Iterator<Item = &TextureBinding> {
    pass.textures.iter().chain(match &pass.kind {
        super::PreparedPassKind::Copy { source } => Some(source),
        super::PreparedPassKind::Render => None,
    })
}
fn render_layer<'a>(
    layer: &'a GpuLayer,
    inputs: &TextureInputs<'a>,
    time: f32,
    scratch: &'a [RenderTarget],
    spectrum: &super::spectrum::SpectrumSnapshot,
) -> Result<&'a GpuTexture> {
    let mut current: Option<&'a GpuTexture> = None;
    for (stage_index, stage) in layer.stages.iter().enumerate() {
        let mut stage_input = current;
        // A pooled named target can alias an earlier stage's output. Move the
        // live stage input to a free existing work image before overwriting it;
        // this is a copy, not another allocation or an effect resolution change.
        if stage_index != 0
            && stage.passes.iter().any(|p| {
                pass_reads(p)
                    .any(|b| matches!(b, TextureBinding::Input | TextureBinding::LayerComposite))
            })
            && let Some(input) = stage_input
        {
            let written = stage
                .passes
                .iter()
                .filter_map(|p| p.target.as_ref())
                .filter_map(|name| stage.targets.get(name))
                .map(|t| t.texture.id)
                .collect::<Vec<_>>();
            if written.contains(&input.id) {
                let spare = scratch
                    .iter()
                    .skip(layer.work_offset)
                    .find(|t| !written.contains(&t.texture.id))
                    .context("Scene stage input has no free borrowed work image")?;
                copy_texture(CopySource::Texture(input), spare)?;
                stage_input = Some(&spare.texture);
            }
        }
        for (pass_index, pass) in stage.passes.iter().enumerate() {
            let input = if stage_index == 0 {
                current
            } else {
                stage_input
            };
            if let super::PreparedPassKind::Copy { source } = &pass.kind {
                if inputs.replay {
                    continue;
                }
                let source = match source {
                    TextureBinding::Named(name) | TextureBinding::History(name) => {
                        CopySource::Target(
                            stage
                                .targets
                                .get(name)
                                .context("Scene named/history target unavailable")?,
                        )
                    }
                    _ => CopySource::Texture(resolve_binding(
                        source,
                        stage,
                        input,
                        inputs,
                        &pass.label,
                        0,
                    )?),
                };
                let target = stage
                    .targets
                    .get(pass.target.as_ref().context("Scene copy target missing")?)
                    .context("Scene copy target unavailable")?;
                copy_texture(source, target)?;
                continue;
            }
            if inputs.replay && pass.target.as_ref().is_some_and(|name| stage.passes.iter().any(|p| matches!(&p.kind, super::PreparedPassKind::Copy { source: TextureBinding::Named(source) } if source == name))) { continue; }
            let resolved = pass
                .textures
                .iter()
                .enumerate()
                .map(|(slot, b)| resolve_binding(b, stage, input, inputs, &pass.label, slot))
                .collect::<Result<Vec<_>>>()?;
            let program = pass
                .program
                .as_ref()
                .context("Scene render shader unavailable")?;
            let active: Vec<_> = resolved
                .iter()
                .enumerate()
                .filter(|(slot, _)| program.location(&format!("g_Texture{slot}")) >= 0)
                .map(|(_, t)| *t)
                .collect();
            let future_reads = stage.passes[pass_index + 1..]
                .iter()
                .flat_map(pass_reads)
                .filter(|binding| match binding {
                    TextureBinding::Input | TextureBinding::LayerComposite => stage_index != 0,
                    TextureBinding::Named(_) | TextureBinding::History(_) => true,
                    _ => false,
                })
                .map(|binding| {
                    resolve_binding(binding, stage, stage_input, inputs, &pass.label, 0)
                        .map(|texture| texture.id)
                })
                .collect::<Result<std::collections::BTreeSet<_>>>()?;
            let target = if let Some(name) = &pass.target {
                stage
                    .targets
                    .get(name)
                    .context("Scene output target unavailable")?
            } else {
                scratch
                    .iter()
                    .skip(layer.work_offset)
                    .rev()
                    .find(|target| {
                        !active.iter().any(|t| t.id == target.texture.id)
                            && !future_reads.contains(&target.texture.id)
                    })
                    .context("Scene scratch framebuffer feedback loop")?
            };
            ensure!(
                !active.iter().any(|t| t.id == target.texture.id),
                "Scene render pass {} samples its output texture {} (active {:?})",
                pass.label,
                target.texture.id,
                active.iter().map(|t| t.id).collect::<Vec<_>>()
            );
            unsafe {
                gl::BindFramebuffer(gl::DRAW_FRAMEBUFFER, target.fbo);
                gl::Viewport(0, 0, target.texture.width, target.texture.height);
                gl::Disable(gl::BLEND);
                gl::ClearColor(0., 0., 0., 0.);
                gl::Clear(gl::COLOR_BUFFER_BIT);
                if stage_index != 0 {
                    set_blend(pass.blend);
                }
            }
            bind_pass(
                program,
                pass,
                &resolved,
                time,
                spectrum,
                if stage_index == 0 {
                    layer.tint
                } else {
                    [1.; 4]
                },
            )?;
            program.matrix(&IDENTITY);
            unsafe {
                gl::DrawArrays(gl::TRIANGLES, 0, 6);
            }
            check_gl(&format!("drawing Scene pass {}", pass.label))?;
            if pass.target.is_none() {
                current = Some(&target.texture);
            }
        }
    }
    current.context("Scene image produced no frame")
}

#[cfg(test)]
#[path = "renderer_miku_acceptance.rs"]
mod miku_acceptance;

#[cfg(test)]
#[path = "renderer_history_tests.rs"]
mod history_tests;

#[cfg(test)]
#[path = "renderer_ahead_tests.rs"]
mod ahead_tests;

#[cfg(test)]
#[path = "renderer_sharing_tests.rs"]
mod sharing_tests;

#[cfg(test)]
#[path = "renderer_owned_blend_tests.rs"]
mod owned_blend_tests;

#[cfg(test)]
#[path = "renderer_copy_fbo_tests.rs"]
mod copy_fbo_tests;

#[cfg(test)]
#[path = "renderer_fullframe_source_tests.rs"]
mod fullframe_source_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_estimate_and_allocation_share_float_rounding() {
        assert_eq!(target_dimensions(100, 100, 100.0 / 7.0), [7, 7]);
        assert_eq!(target_dimensions(101, 103, 2.0), [51, 52]);
    }

    #[test]
    fn legacy_sampler_flags_do_not_change_modern_effect_sampling() {
        let repeat = GpuTexture::new(4, 4, 2, 2, 0);
        let clamp = GpuTexture::new(4, 4, 2, 2, 2);
        for texture in [&repeat, &clamp] {
            assert!(!pass_texture_repeat("effects/iris", 0, texture));
            assert!(!pass_texture_repeat("effects/waterripple", 0, texture));
            assert!(pass_texture_repeat("effects/waterripple", 2, texture));
            assert!(pass_texture_repeat("effects/shine_downsample2", 2, texture));
            assert!(!pass_texture_repeat("genericparticle", 0, texture));
        }
        for label in ["genericimage", "flowimage", "swayimage", "flag"] {
            for slot in 0..TEXTURE_SLOTS {
                assert!(pass_texture_repeat(label, slot, &repeat));
                assert!(!pass_texture_repeat(label, slot, &clamp));
            }
        }
        assert_eq!(clamp.resolution(), [4.0, 4.0, 2.0, 2.0]);
    }

    #[test]
    #[ignore = "requires an explicit EGL/OpenGL offscreen acceptance run"]
    fn offscreen_legacy_sampling_preserves_padding_y_math_and_wrap() -> Result<()> {
        use khronos_egl as egl;
        let instance = unsafe { egl::DynamicInstance::<egl::EGL1_4>::load_required()? };
        let display = unsafe { instance.get_display(egl::DEFAULT_DISPLAY) }
            .context("No offscreen EGL display")?;
        instance.initialize(display)?;
        instance.bind_api(egl::OPENGL_API)?;
        let config = instance
            .choose_first_config(
                display,
                &[
                    egl::SURFACE_TYPE,
                    egl::PBUFFER_BIT,
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
                    egl::NONE,
                ],
            )?
            .context("No EGL configuration")?;
        let context = instance.create_context(
            display,
            config,
            None,
            &[
                egl::CONTEXT_MAJOR_VERSION,
                3,
                egl::CONTEXT_MINOR_VERSION,
                0,
                egl::NONE,
            ],
        )?;
        let surface = instance.create_pbuffer_surface(
            display,
            config,
            &[egl::WIDTH, 2, egl::HEIGHT, 2, egl::NONE],
        )?;
        instance.make_current(display, Some(surface), Some(surface), Some(context))?;
        gl::load_with(|name| {
            instance
                .get_proc_address(name)
                .map(|function| function as *const std::ffi::c_void)
                .unwrap_or(std::ptr::null())
        });

        // These authored shader inputs are independent fixture code. Loading
        // them through the production adapter exercises its actual vertex and
        // sampling wrappers, rather than reproducing those wrappers in a test.
        struct Fixture(std::path::PathBuf);
        impl Drop for Fixture {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos();
        let fixture = Fixture(
            std::env::temp_dir().join(format!("lwe-legacy-gl-{}-{nonce}", std::process::id())),
        );
        std::fs::create_dir(&fixture.0)?;
        std::fs::create_dir(fixture.0.join("shaders"))?;
        std::fs::write(
            fixture.0.join("shaders/genericimage.vert"),
            "attribute vec3 a_Position;\nattribute vec2 a_TexCoord;\nuniform mat4 g_ModelViewProjectionMatrix;\nvarying vec2 v_TexCoord;\nvoid main(){gl_Position=g_ModelViewProjectionMatrix*vec4(a_Position,1.0);v_TexCoord=a_TexCoord;}\n",
        )?;
        let assets = lwe_scene::SceneAssets::open(&fixture.0, None)?;
        let mut rgba = [255, 0, 255, 255].repeat(16);
        for (index, color) in [
            (0, [255, 0, 0, 255]),
            (1, [255, 255, 0, 255]),
            (4, [0, 0, 255, 255]),
            (5, [0, 255, 0, 255]),
        ] {
            rgba[index * 4..index * 4 + 4].copy_from_slice(&color);
        }
        let orientation = [
            0, 0, 255, 255, 0, 255, 0, 255, 255, 0, 0, 255, 255, 255, 0, 255,
        ];
        let tinted = [0, 0, 96, 255, 0, 32, 0, 255, 64, 0, 0, 255, 64, 32, 0, 255];
        let tint = [0.5, 0.25, 0.75, 0.5];
        let mut cases = vec![
            ("v_TexCoord", 2, [1.0; 4], orientation),
            (
                "vec2(v_TexCoord.x,v_TexCoord.y*0.5+0.125)",
                2,
                [1.0; 4],
                [
                    64, 0, 191, 255, 64, 255, 0, 255, 191, 0, 64, 255, 191, 255, 0, 255,
                ],
            ),
            ("v_TexCoord+vec2(1.0,0.0)", 0, [1.0; 4], orientation),
            (
                "v_TexCoord+vec2(1.0,0.0)",
                2,
                [1.0; 4],
                [255, 0, 255, 255].repeat(4).try_into().unwrap(),
            ),
            ("v_TexCoord", 2, tint, tinted),
            (
                "negative odd power",
                2,
                [1.0; 4],
                [
                    124, 0, 255, 255, 124, 255, 0, 255, 96, 0, 0, 255, 96, 255, 0, 255,
                ],
            ),
        ];
        let original_assets_path = std::env::var_os("LWE_SCENE_TEST_ASSETS");
        if original_assets_path.is_some() {
            cases.push(("original genericimage", 2, [1.0; 4], orientation));
            cases.push(("original genericimage", 2, tint, tinted));
            cases.push((
                "original genericimage negative scroll",
                0,
                [1.0; 4],
                [
                    255, 0, 255, 255, 0, 0, 255, 255, 255, 0, 255, 255, 255, 0, 0, 255,
                ],
            ));
        }
        for (expression, flags, tint, expected) in cases {
            let negative_scroll = expression == "original genericimage negative scroll";
            let (shader, constants) = if expression.starts_with("original genericimage") {
                // An empty project root prevents the authored fixture shaders
                // from shadowing the installed original assets in this case.
                let original_project = fixture.0.join("original");
                std::fs::create_dir_all(&original_project)?;
                let original_assets = lwe_scene::SceneAssets::open(
                    &original_project,
                    original_assets_path.as_deref().map(std::path::Path::new),
                )?;
                let shader = super::super::shader::load_legacy_image(
                    &original_assets,
                    "genericimage",
                    &BTreeMap::new(),
                    &std::collections::BTreeSet::from([0]),
                )?;
                let mut constants = shader
                    .uniforms
                    .iter()
                    .filter_map(|uniform| {
                        uniform.default.as_ref().map(|value| {
                            super::super::numbers(value).map(|value| (uniform.name.clone(), value))
                        })
                    })
                    .collect::<Result<BTreeMap<_, _>>>()?;
                if negative_scroll {
                    constants.insert("g_ScrollX".to_owned(), vec![-0.5]);
                }
                (shader, constants)
            } else {
                let body = if expression == "negative odd power" {
                    // The signed cubic is the same operation used by the flag
                    // family, with a varying input that is negative here. A
                    // constant-folded pow would not exercise the driver path.
                    "vec4 color=texSample2D(g_Texture0,v_TexCoord);color.r=0.5+pow(v_TexCoord.y-0.625,3.0);gl_FragColor=color;".to_owned()
                } else {
                    format!("gl_FragColor=texSample2D(g_Texture0,{expression});")
                };
                std::fs::write(
                    fixture.0.join("shaders/genericimage.frag"),
                    format!(
                        "uniform sampler2D g_Texture0;\nvarying vec2 v_TexCoord;\nvoid main(){{{body}}}\n"
                    ),
                )?;
                (
                    super::super::shader::load_legacy_image(
                        &assets,
                        "genericimage",
                        &BTreeMap::new(),
                        &std::collections::BTreeSet::from([0]),
                    )?,
                    BTreeMap::new(),
                )
            };
            let prepared = PreparedScene {
                scripts: None,
                script_layers: BTreeMap::new(),
                audio: Vec::new(),
                bloom: None,
                particles: Vec::new(),
                draw_order: vec![super::super::DrawObject::Image(0)],
                canvas: [2.0, 2.0],
                clear: [0.0, 0.0, 0.0, 1.0],
                textures: vec![PreparedTexture {
                    static_source: None,

                    timed: None,
                    image: None,
                    text_frame: None,
                    _rgba_reservation: None,

                    fingerprint: [0; 32],
                    width: 4,
                    height: 4,
                    image_width: 2,
                    image_height: 2,
                    source_format: 0,
                    source_flags: flags,
                    animation: None,
                    rgba: rgba.clone(),
                }],
                layers: vec![PreparedLayer {
                    clamp_uvs: false,
                    crop_offset: [0.; 2],
                    no_padding: false,
                    text: None,
                    origin_z: 0.,
                    angles: [0.; 3],

                    size: [2.0, 2.0],
                    origin: [1.0, 1.0],
                    scale: [1.0; 2],
                    angle: 0.0,
                    tint,
                    blend: Blend::Alpha,
                    composite: None,
                    stages: vec![PreparedStage {
                        persistent_targets: std::collections::BTreeSet::new(),

                        targets: BTreeMap::new(),
                        passes: vec![PreparedPass {
                            kind: crate::scene::PreparedPassKind::Render,

                            label: "genericimage".into(),
                            shader: Some(shader),
                            textures: vec![TextureBinding::Static(0)],
                            constants,
                            target: None,
                            blend: Blend::Replace,
                        }],
                    }],
                }],
                summary: super::super::SceneSummary {
                    layer_count: 1,
                    effect_count: 0,
                    pass_count: 1,
                    animated: negative_scroll,
                },
            };
            let mut renderer = prepared.initialize()?;
            assert_eq!(renderer.textures[0].source_flags, flags);
            assert_eq!(renderer.scratch[&(2, 2)][0].texture.source_flags, 2);
            assert_eq!(
                renderer.gpu_payload_bytes(),
                prepared.estimated_gpu_bytes()?
            );
            assert!(renderer.render(2, 2, 0.0)?);
            renderer.report_presented();
            if negative_scroll {
                assert!(renderer.render(2, 2, 1.0)?);
                renderer.report_presented();
            }
            let mut pixels = [0_u8; 16];
            unsafe {
                gl::ReadPixels(
                    0,
                    0,
                    2,
                    2,
                    gl::RGBA,
                    gl::UNSIGNED_BYTE,
                    pixels.as_mut_ptr().cast(),
                );
            }
            for (actual, expected) in pixels.into_iter().zip(expected) {
                assert!(
                    actual.abs_diff(expected) <= 1,
                    "legacy padding/Y/wrap/tint result {pixels:?} for {expression}, flags={flags}, tint={tint:?}"
                );
            }
            renderer.cleanup();
            assert_eq!(renderer.gpu_payload_bytes(), 0);
        }
        check_gl("offscreen legacy sampling")?;
        instance.make_current(display, None, None, None)?;
        instance.destroy_surface(display, surface)?;
        instance.destroy_context(display, context)?;
        instance.terminate(display)?;
        Ok(())
    }

    #[test]
    #[ignore = "requires an explicit EGL/OpenGL offscreen acceptance run"]
    fn offscreen_pixels_and_gl_state_are_preserved() -> Result<()> {
        use khronos_egl as egl;
        let instance = unsafe { egl::DynamicInstance::<egl::EGL1_4>::load_required()? };
        let display = unsafe { instance.get_display(egl::DEFAULT_DISPLAY) }
            .context("No offscreen EGL display")?;
        instance.initialize(display)?;
        instance.bind_api(egl::OPENGL_API)?;
        let config = instance
            .choose_first_config(
                display,
                &[
                    egl::SURFACE_TYPE,
                    egl::PBUFFER_BIT,
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
                    egl::NONE,
                ],
            )?
            .context("No EGL configuration")?;
        let context = instance.create_context(
            display,
            config,
            None,
            &[
                egl::CONTEXT_MAJOR_VERSION,
                3,
                egl::CONTEXT_MINOR_VERSION,
                0,
                egl::NONE,
            ],
        )?;
        let surface = instance.create_pbuffer_surface(
            display,
            config,
            &[egl::WIDTH, 2, egl::HEIGHT, 2, egl::NONE],
        )?;
        instance.make_current(display, Some(surface), Some(surface), Some(context))?;
        gl::load_with(|name| {
            instance
                .get_proc_address(name)
                .map(|function| function as *const std::ffi::c_void)
                .unwrap_or(std::ptr::null())
        });
        let prepared = PreparedScene {
            scripts: None,
            script_layers: BTreeMap::new(),
            audio: Vec::new(),
            bloom: None,
            particles: Vec::new(),
            draw_order: vec![super::super::DrawObject::Image(0)],
            canvas: [2.0, 2.0],
            clear: [0.0, 0.0, 0.0, 1.0],
            textures: vec![PreparedTexture {
                static_source: None,

                timed: None,
                image: None,
                text_frame: None,
                _rgba_reservation: None,

                fingerprint: [0; 32],
                width: 2,
                height: 2,
                image_width: 2,
                image_height: 2,
                source_format: 0,
                source_flags: 2,
                animation: None,
                rgba: vec![
                    255, 0, 0, 255, 255, 255, 0, 255, 0, 0, 255, 255, 0, 255, 0, 255,
                ],
            }],
            layers: vec![PreparedLayer {
                clamp_uvs: false,
                crop_offset: [0.; 2],
                no_padding: false,
                text: None,
                origin_z: 0.,
                angles: [0.; 3],

                size: [2.0, 2.0],
                origin: [1.0, 1.0],
                scale: [1.0; 2],
                angle: 0.0,
                tint: [1.0; 4],
                blend: Blend::Alpha,
                composite: None,
                stages: vec![PreparedStage {
                    persistent_targets: std::collections::BTreeSet::new(),

                    targets: BTreeMap::new(),
                    passes: vec![PreparedPass {
                        kind: crate::scene::PreparedPassKind::Render,

                        label: "authored orientation fixture".into(),
                        shader: Some(super::super::shader::image()),
                        textures: vec![TextureBinding::Static(0)],
                        constants: BTreeMap::new(),
                        target: None,
                        blend: Blend::Replace,
                    }],
                }],
            }],
            summary: super::super::SceneSummary {
                layer_count: 1,
                effect_count: 0,
                pass_count: 1,
                animated: false,
            },
        };
        // Caller-owned state deliberately conflicts with Scene rendering. In
        // particular a nonzero unpack PBO/row stride must not corrupt uploads.
        let mut caller_vao = 0;
        let mut caller_buffer = 0;
        unsafe {
            gl::GenVertexArrays(1, &mut caller_vao);
            gl::BindVertexArray(caller_vao);
            gl::GenBuffers(1, &mut caller_buffer);
            gl::BindBuffer(gl::PIXEL_UNPACK_BUFFER, caller_buffer);
            gl::BufferData(
                gl::PIXEL_UNPACK_BUFFER,
                64,
                std::ptr::null(),
                gl::STATIC_DRAW,
            );
            gl::PixelStorei(gl::UNPACK_ALIGNMENT, 8);
            gl::PixelStorei(gl::UNPACK_ROW_LENGTH, 3);
            gl::PixelStorei(gl::UNPACK_SKIP_PIXELS, 1);
            gl::ActiveTexture(gl::TEXTURE0 + 5);
            gl::Enable(gl::SCISSOR_TEST);
            gl::Scissor(1, 1, 1, 1);
            gl::ColorMask(gl::FALSE, gl::TRUE, gl::FALSE, gl::TRUE);
            gl::Enable(gl::BLEND);
            gl::BlendFuncSeparate(gl::ONE, gl::DST_ALPHA, gl::ZERO, gl::ONE);
            gl::BlendEquationSeparate(gl::FUNC_REVERSE_SUBTRACT, gl::FUNC_SUBTRACT);
            gl::PolygonMode(gl::FRONT_AND_BACK, gl::LINE);
        }
        let before = GlState::capture();
        let mut renderer = prepared.initialize()?;
        assert_eq!(renderer.scratch[&(2, 2)].len(), 1);
        assert_eq!(
            renderer.gpu_payload_bytes(),
            prepared.estimated_gpu_bytes()?
        );
        assert_eq!(
            before,
            GlState::capture(),
            "initialization changed caller GL state"
        );
        assert!(renderer.render(2, 2, 0.0)?);
        renderer.report_presented();
        let after = GlState::capture();
        assert_eq!(before, after, "rendering changed caller GL state");
        drop(after);
        drop(before);
        let mut pixels = [0_u8; 16];
        unsafe {
            gl::ReadPixels(
                0,
                0,
                2,
                2,
                gl::RGBA,
                gl::UNSIGNED_BYTE,
                pixels.as_mut_ptr().cast(),
            );
        }
        assert_eq!(
            pixels,
            [
                0, 0, 255, 255, 0, 255, 0, 255, 255, 0, 0, 255, 255, 255, 0, 255
            ],
            "pixels lost orientation or caller state interfered with drawing"
        );
        assert!(
            !renderer.render(2, 2, 1.0)?,
            "static scene redrew without invalidation"
        );
        unsafe {
            gl::Disable(gl::SCISSOR_TEST);
            gl::ColorMask(gl::TRUE, gl::TRUE, gl::TRUE, gl::TRUE);
            gl::ClearColor(0.0, 1.0, 1.0, 1.0);
            gl::Clear(gl::COLOR_BUFFER_BIT);
        }
        assert!(
            !renderer.render(2, 2, 2.0)?,
            "normal static rendering must remain idle"
        );
        renderer.pause();
        assert!(
            renderer.render_forced(2, 2, 3.0)?,
            "rollback must redraw even a paused static scene"
        );
        unsafe {
            gl::ReadPixels(
                0,
                0,
                2,
                2,
                gl::RGBA,
                gl::UNSIGNED_BYTE,
                pixels.as_mut_ptr().cast(),
            );
        }
        assert_eq!(
            pixels,
            [
                0, 0, 255, 255, 0, 255, 0, 255, 255, 0, 0, 255, 255, 255, 0, 255
            ],
            "rollback retained the failed candidate's pixels"
        );
        renderer.cleanup();
        drop(renderer);
        let mut composite = prepared;
        composite.clear = [0.25, 0.5, 0.75, 1.0];
        composite.textures[0].rgba = [255, 0, 0, 128].repeat(4);
        composite.textures.push(PreparedTexture {
            timed: None,
            static_source: None,
            image: None,
            text_frame: None,
            _rgba_reservation: None,

            fingerprint: [0; 32],
            width: 2,
            height: 2,
            image_width: 2,
            image_height: 2,
            source_format: 0,
            source_flags: 2,
            animation: None,
            rgba: [0, 0, 255, 128].repeat(4),
        });
        composite.layers[0].stages[0].passes = (0..2)
            .map(|index| PreparedPass {
                kind: crate::scene::PreparedPassKind::Render,

                label: "authored alpha material pass".into(),
                shader: Some(super::super::shader::image_material()),
                textures: vec![TextureBinding::Static(index), TextureBinding::Input],
                constants: BTreeMap::new(),
                target: None,
                blend: Blend::Alpha,
            })
            .collect();
        let mut renderer = composite.initialize()?;
        assert_eq!(renderer.scratch[&(2, 2)].len(), 2);
        assert!(renderer.render(2, 2, 0.0)?);
        renderer.report_presented();
        unsafe {
            gl::ReadPixels(
                0,
                0,
                2,
                2,
                gl::RGBA,
                gl::UNSIGNED_BYTE,
                pixels.as_mut_ptr().cast(),
            );
        }
        for pixel in pixels.as_chunks::<4>().0 {
            assert!(
                pixel[0].abs_diff(80) <= 2
                    && pixel[1].abs_diff(32) <= 2
                    && pixel[2].abs_diff(176) <= 2
                    && pixel[3] == 255,
                "material pass alpha was lost or multiplied twice: {pixel:?}"
            );
        }
        renderer.cleanup();
        assert_eq!(renderer.gpu_payload_bytes(), 0);
        drop(renderer);

        // Reusing two scratch targets must retain each effect's original input
        // across multiple unnamed writes, including a half-size named target.
        composite.textures[0].rgba = [51, 0, 0, 255].repeat(4);
        composite.layers[0].stages[0].passes.truncate(1);
        composite.layers[0].stages[0].passes[0].shader = Some(super::super::shader::image());
        composite.layers[0].stages[0].passes[0].textures = vec![TextureBinding::Static(0)];
        let effect_pass = |target: Option<&str>, expression: &str, bindings| {
            let mut shader = super::super::shader::image();
            shader.fragment = format!(
                "#version 130\nin vec2 v_UV; out vec4 lwe_FragColor; uniform sampler2D g_Texture0; uniform sampler2D g_Texture1; void main(){{lwe_FragColor={expression};}}"
            );
            PreparedPass {
                kind: crate::scene::PreparedPassKind::Render,

                label: "authored retained effect input".into(),
                shader: Some(shader),
                textures: bindings,
                constants: BTreeMap::new(),
                target: target.map(str::to_owned),
                blend: Blend::Replace,
            }
        };
        composite.layers[0].stages.push(PreparedStage {
            persistent_targets: std::collections::BTreeSet::new(),

            targets: BTreeMap::from([("saved".into(), 2.0)]),
            passes: vec![
                effect_pass(
                    Some("saved"),
                    "texture(g_Texture0,v_UV)",
                    vec![TextureBinding::Input],
                ),
                effect_pass(
                    None,
                    "texture(g_Texture0,v_UV)+vec4(0.1,0,0,0)",
                    vec![TextureBinding::Input],
                ),
                effect_pass(
                    None,
                    "vec4(texture(g_Texture0,v_UV).r+texture(g_Texture1,v_UV).r,0,0,1)",
                    vec![TextureBinding::Input, TextureBinding::Named("saved".into())],
                ),
            ],
        });
        composite.layers[0].stages.push(PreparedStage {
            persistent_targets: std::collections::BTreeSet::new(),

            targets: BTreeMap::new(),
            passes: vec![effect_pass(
                None,
                "texture(g_Texture0,v_UV)",
                vec![TextureBinding::Input],
            )],
        });
        let mut renderer = composite.initialize()?;
        assert_eq!(renderer.scratch[&(2, 2)].len(), 2);
        assert!(renderer.render(2, 2, 0.0)?);
        renderer.report_presented();
        unsafe {
            gl::ReadPixels(
                0,
                0,
                2,
                2,
                gl::RGBA,
                gl::UNSIGNED_BYTE,
                pixels.as_mut_ptr().cast(),
            );
        }
        for pixel in pixels.as_chunks::<4>().0 {
            assert_eq!(
                pixel,
                &[102, 0, 0, 255],
                "effect input or named target was overwritten"
            );
        }
        renderer.cleanup();
        drop(renderer);
        unsafe {
            gl::BindBuffer(gl::PIXEL_UNPACK_BUFFER, 0);
            gl::BindVertexArray(0);
            gl::DeleteBuffers(1, &caller_buffer);
            gl::DeleteVertexArrays(1, &caller_vao);
        }
        check_gl("offscreen state acceptance")?;
        instance.make_current(display, None, None, None)?;
        instance.destroy_surface(display, surface)?;
        instance.destroy_context(display, context)?;
        instance.terminate(display)?;
        Ok(())
    }

    #[test]
    fn aspect_cover_and_image_transform_are_centered() {
        let layer = GpuLayer {
            work_offset: 0,
            direct: false,
            origin_z: 0.,
            angles: [0.; 3],
            size: [1920.0, 1080.0],
            origin: [960.0, 540.0],
            scale: [1.0; 2],
            angle: 0.0,
            tint: [1.0; 4],
            blend: Blend::Alpha,
            stages: Vec::new(),
            composite: None,
        };
        assert_eq!(layer_matrix([1920.0, 1080.0], &layer, 1920, 1080), IDENTITY);
        let matrix = layer_matrix([1920.0, 1080.0], &layer, 1080, 1920);
        assert!(matrix[0] > 1.0);
        assert_eq!(matrix[5], 1.0);
        assert_eq!([matrix[12], matrix[13]], [0.0, 0.0]);
    }
}
#[test]
fn long_compositor_gaps_resume_without_unbounded_particle_catchup() {
    assert_eq!(animation_delta(12.0, 72.0), 0.25);
    assert_eq!(animation_delta(12.0, 11.0), 0.0);
    assert_eq!(animation_delta(12.0, 13.0), 1.0);
    assert!((animation_delta(12.0, 12.033) - 0.033).abs() < 1e-9);
}
