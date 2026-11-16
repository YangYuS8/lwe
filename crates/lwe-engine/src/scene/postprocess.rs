//! Global LDR bloom using the user's original material and shader resources.
//!
//! CPU preparation resolves and validates every pass. GL allocation, rendering,
//! and destruction require the renderer's current context on the owning thread.

use std::collections::{BTreeMap, BTreeSet};
use std::marker::PhantomData;
use std::rc::Rc;

use anyhow::{Context, Result, ensure};
use lwe_scene::SceneAssets;
use serde_json::Value;

use super::renderer::{GlState, Program, QUAD, RenderTarget};
use super::shader::{self, Shader};

#[derive(Debug)]
pub(super) struct PreparedBloom {
    shaders: [Shader; 4],
    constants: BTreeMap<String, Vec<f32>>,
}

fn material_shader(
    assets: &SceneAssets,
    material: &str,
    expected_shader: &str,
    inputs: &[&str],
    document: &Value,
) -> Result<Shader> {
    let passes = document["passes"]
        .as_array()
        .context("Bloom material has no passes")?;
    ensure!(
        passes.len() == 1,
        "unsupported_scene: bloom material pass graph"
    );
    let pass = &passes[0];
    super::validate_pass(pass)?;
    ensure!(
        pass["shader"].as_str() == Some(expected_shader),
        "unsupported_scene: bloom material shader {material}"
    );
    let object = pass.as_object().context("Invalid bloom material pass")?;
    ensure!(
        object.keys().all(|key| matches!(
            key.as_str(),
            "shader" | "textures" | "cullmode" | "depthtest" | "depthwrite"
        )),
        "unsupported_scene: additional bloom pass behavior {material}"
    );
    let textures = pass["textures"]
        .as_array()
        .context("Bloom material has no texture inputs")?;
    ensure!(
        textures.len() == inputs.len()
            && textures
                .iter()
                .zip(inputs)
                .all(|(texture, input)| texture.as_str() == Some(*input)),
        "unsupported_scene: bloom material texture graph {material}"
    );
    let shader = shader::load(
        assets,
        expected_shader,
        &BTreeMap::new(),
        &(0..inputs.len()).collect::<BTreeSet<_>>(),
    )?;
    let expected_uniforms: &[&str] = match expected_shader {
        "downsample_quarter_bloom" => &[
            "g_Texture0",
            "g_TexelSize",
            "g_BloomStrength",
            "g_BloomThreshold",
            "g_BloomTint",
        ],
        "downsample_eighth_blur_v" | "blur_h_bloom" => &["g_Texture0", "g_TexelSize"],
        "combine" => &["g_Texture0", "g_Texture1"],
        _ => unreachable!("caller restricts global bloom shaders"),
    };
    for uniform in &shader.uniforms {
        ensure!(
            expected_uniforms.contains(&uniform.name.as_str()),
            "unsupported_scene: additional bloom shader dependency {}",
            uniform.name
        );
        let expected_kind = match uniform.name.as_str() {
            "g_Texture0" | "g_Texture1" => "sampler2D",
            "g_TexelSize" => "vec2",
            "g_BloomStrength" | "g_BloomThreshold" => "float",
            "g_BloomTint" => "vec3",
            _ => {
                return Err(super::unsupported(
                    "shader",
                    format!("bloom uniform {}", uniform.name),
                ));
            }
        };
        ensure!(
            !uniform.array && uniform.kind == expected_kind,
            "unsupported_scene: bloom uniform type {}",
            uniform.name
        );
    }
    for name in expected_uniforms {
        ensure!(
            shader.uniforms.iter().any(|uniform| uniform.name == *name),
            "unsupported_scene: missing bloom uniform {name}"
        );
    }
    Ok(shader)
}

impl PreparedBloom {
    pub(super) fn prepare(
        assets: &SceneAssets,
        general: &Value,
        json: &mut impl FnMut(&str) -> Result<Value>,
    ) -> Result<Option<Self>> {
        if !super::boolean(general, "bloom", false)? {
            return Ok(None);
        }
        if super::boolean(general, "hdr", false)? {
            return Err(super::unsupported("hdr", "HDR bloom"));
        }
        let shaders = [
            material_shader(
                assets,
                "materials/util/downsample_quarter_bloom.json",
                "downsample_quarter_bloom",
                &["_rt_FullFrameBuffer"],
                &json("materials/util/downsample_quarter_bloom.json")?,
            )?,
            material_shader(
                assets,
                "materials/util/downsample_eighth_blur_v.json",
                "downsample_eighth_blur_v",
                &["_rt_4FrameBuffer"],
                &json("materials/util/downsample_eighth_blur_v.json")?,
            )?,
            material_shader(
                assets,
                "materials/util/blur_h_bloom.json",
                "blur_h_bloom",
                &["_rt_8FrameBuffer"],
                &json("materials/util/blur_h_bloom.json")?,
            )?,
            material_shader(
                assets,
                "materials/util/combine_ldr.json",
                "combine",
                &["_rt_FullFrameBuffer", "_rt_Bloom"],
                &json("materials/util/combine_ldr.json")?,
            )?,
        ];
        let mut constants = BTreeMap::new();
        for uniform in &shaders[0].uniforms {
            let expected_material = match uniform.name.as_str() {
                "g_BloomStrength" => "bloomstrength",
                "g_BloomThreshold" => "bloomthreshold",
                "g_BloomTint" => "bloomtint",
                _ => continue,
            };
            let Some(material) = &uniform.material else {
                return Err(super::unsupported(
                    "shader",
                    format!("missing bloom metadata {}", uniform.name),
                ));
            };
            ensure!(
                material == expected_material,
                "unsupported_scene: bloom uniform material mapping"
            );
            let value = general
                .get(material)
                .or(uniform.default.as_ref())
                .with_context(|| format!("Missing bloom material property {material}"))?;
            let values = super::numbers(value)?;
            ensure!(
                values.len() == if uniform.kind == "vec3" { 3 } else { 1 },
                "Invalid bloom material property {material}"
            );
            constants.insert(uniform.name.clone(), values);
        }
        Ok(Some(Self { shaders, constants }))
    }

    /// Logical RGBA8 texture payload; excludes driver metadata and EGL buffers.
    pub(super) fn estimated_bytes(&self, width: i32, height: i32) -> Result<u64> {
        layout_bytes(width, height)
    }
}

fn dimensions(width: i32, height: i32, divisor: i32) -> [i32; 2] {
    // Native render-target resize divides positive screen dimensions as integers
    // and clamps each reduced dimension to at least two pixels.
    [(width / divisor).max(2), (height / divisor).max(2)]
}

fn layout_bytes(width: i32, height: i32) -> Result<u64> {
    ensure!(
        width >= 2 && height >= 2,
        "unsupported_scene: bloom output dimensions must be at least two pixels"
    );
    let quarter = dimensions(width, height, 4);
    let eighth = dimensions(width, height, 8);
    [[width, height], quarter, eighth, eighth]
        .iter()
        .try_fold(0_u64, |bytes, size| {
            bytes
                .checked_add(size[0] as u64 * size[1] as u64 * 4)
                .context("Bloom GPU allocation estimate overflow")
        })
}

struct QuadGeometry {
    vao: u32,
    vbo: u32,
}

impl QuadGeometry {
    fn new() -> Result<Self> {
        let mut geometry = Self { vao: 0, vbo: 0 };
        // SAFETY: the caller established a current GL context. QUAD contains six
        // interleaved position/UV vertices, and all pointers stay within it.
        unsafe {
            gl::GenVertexArrays(1, &mut geometry.vao);
            gl::GenBuffers(1, &mut geometry.vbo);
            ensure!(
                geometry.vao != 0 && geometry.vbo != 0,
                "Cannot create bloom geometry"
            );
            gl::BindVertexArray(geometry.vao);
            gl::BindBuffer(gl::ARRAY_BUFFER, geometry.vbo);
            gl::BufferData(
                gl::ARRAY_BUFFER,
                std::mem::size_of_val(&QUAD) as isize,
                QUAD.as_ptr().cast(),
                gl::STATIC_DRAW,
            );
            gl::EnableVertexAttribArray(0);
            gl::VertexAttribPointer(0, 3, gl::FLOAT, gl::FALSE, 20, std::ptr::null());
            gl::EnableVertexAttribArray(1);
            gl::VertexAttribPointer(
                1,
                2,
                gl::FLOAT,
                gl::FALSE,
                20,
                (3 * std::mem::size_of::<f32>()) as *const _,
            );
        }
        check_gl("allocating bloom geometry")?;
        Ok(geometry)
    }
}

impl Drop for QuadGeometry {
    fn drop(&mut self) {
        // SAFETY: BloomRenderer is confined to its context-owning thread.
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

pub(super) struct BloomRenderer {
    full: RenderTarget,
    quarter: RenderTarget,
    eighth: RenderTarget,
    bloom: RenderTarget,
    programs: [Program; 4],
    constants: BTreeMap<String, Vec<f32>>,
    geometry: QuadGeometry,
    size: [i32; 2],
    payload_bytes: u64,
    _context_thread: PhantomData<Rc<()>>,
}

impl BloomRenderer {
    pub(super) fn initialize(prepared: &PreparedBloom, width: i32, height: i32) -> Result<Self> {
        let payload_bytes = prepared.estimated_bytes(width, height)?;
        let state = GlState::capture();
        state.begin();
        let quarter = dimensions(width, height, 4);
        let eighth = dimensions(width, height, 8);
        let programs = [
            Program::compile(&prepared.shaders[0], "global bloom extract")?,
            Program::compile(&prepared.shaders[1], "global bloom first blur")?,
            Program::compile(&prepared.shaders[2], "global bloom second blur")?,
            Program::compile(&prepared.shaders[3], "global bloom combine")?,
        ];
        Ok(Self {
            full: RenderTarget::new(width, height)?,
            quarter: RenderTarget::new(quarter[0], quarter[1])?,
            eighth: RenderTarget::new(eighth[0], eighth[1])?,
            bloom: RenderTarget::new(eighth[0], eighth[1])?,
            programs,
            constants: prepared.constants.clone(),
            geometry: QuadGeometry::new()?,
            size: [width, height],
            payload_bytes,
            _context_thread: PhantomData,
        })
    }

    pub(super) fn dimensions(&self) -> [i32; 2] {
        self.size
    }
    pub(super) fn scene_framebuffer(&self) -> u32 {
        self.full.framebuffer()
    }
    pub(super) fn payload_bytes(&self) -> u64 {
        self.payload_bytes
    }

    pub(super) fn composite_to(&self, framebuffer: u32) -> Result<()> {
        ensure!(
            ![
                self.full.framebuffer(),
                self.quarter.framebuffer(),
                self.eighth.framebuffer(),
                self.bloom.framebuffer()
            ]
            .contains(&framebuffer),
            "Bloom cannot composite into its own input framebuffer"
        );
        let state = GlState::capture();
        state.begin();
        let quarter = dimensions(self.size[0], self.size[1], 4);
        let eighth = dimensions(self.size[0], self.size[1], 8);
        self.draw(
            0,
            self.quarter.framebuffer(),
            quarter,
            &[self.full.texture_id()],
        )?;
        self.draw(
            1,
            self.eighth.framebuffer(),
            eighth,
            &[self.quarter.texture_id()],
        )?;
        self.draw(
            2,
            self.bloom.framebuffer(),
            eighth,
            &[self.eighth.texture_id()],
        )?;
        self.draw(
            3,
            framebuffer,
            self.size,
            &[self.full.texture_id(), self.bloom.texture_id()],
        )?;
        check_gl("compositing global bloom")
    }

    fn draw(&self, pass: usize, framebuffer: u32, size: [i32; 2], textures: &[u32]) -> Result<()> {
        let program = &self.programs[pass];
        // SAFETY: targets, textures, geometry, and programs are owned by this
        // renderer. Each draw reads only completed earlier targets.
        unsafe {
            gl::BindFramebuffer(gl::DRAW_FRAMEBUFFER, framebuffer);
            gl::Viewport(0, 0, size[0], size[1]);
            gl::UseProgram(program.id());
            gl::BindVertexArray(self.geometry.vao);
            for (slot, texture) in textures.iter().enumerate() {
                gl::ActiveTexture(gl::TEXTURE0 + slot as u32);
                gl::BindTexture(gl::TEXTURE_2D, *texture);
                gl::Uniform1i(program.location(&format!("g_Texture{slot}")), slot as i32);
            }
            gl::Uniform2f(
                program.location("g_TexelSize"),
                1.0 / self.size[0] as f32,
                1.0 / self.size[1] as f32,
            );
        }
        if pass == 0 {
            program.constants(&self.constants)?;
        }
        // SAFETY: QUAD has exactly six vertices.
        unsafe {
            gl::DrawArrays(gl::TRIANGLES, 0, 6);
        }
        Ok(())
    }
}

fn check_gl(operation: &str) -> Result<()> {
    // SAFETY: every caller requires the current context on this thread.
    let error = unsafe { gl::GetError() };
    ensure!(
        error == gl::NO_ERROR,
        "OpenGL error 0x{error:x} while {operation}"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bloom_budget_includes_every_target_and_small_dimensions() {
        assert_eq!(layout_bytes(3840, 2160).unwrap(), 36_288_000);
        assert!(layout_bytes(1, 1).is_err());
        assert_eq!(layout_bytes(2, 2).unwrap(), 64);
        assert_eq!(layout_bytes(9, 9).unwrap(), (81 + 4 + 4 + 4) * 4);
        assert_eq!(dimensions(101, 103, 8), [12, 12]);
        assert!(layout_bytes(0, 1).is_err());
        assert!(layout_bytes(1, -1).is_err());
    }

    struct Fixture(std::path::PathBuf);
    impl Fixture {
        fn new() -> Self {
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let root =
                std::env::temp_dir().join(format!("lwe-bloom-{}-{stamp}", std::process::id()));
            std::fs::create_dir(&root).unwrap();
            let fixture = Self(root);
            let vertex = "attribute vec3 a_Position;\nattribute vec2 a_TexCoord;\nvarying vec2 v_TexCoord;\nvoid main(){v_TexCoord=a_TexCoord;gl_Position=vec4(a_Position,1.0);}";
            // Authored fixtures exercise the pass graph and bindings; production
            // always loads the user's shader resources and embeds no bloom math.
            let shaders = [
                (
                    "downsample_quarter_bloom",
                    "uniform sampler2D g_Texture0;\nuniform vec2 g_TexelSize;\nuniform float g_BloomStrength; // {\"material\":\"bloomstrength\",\"default\":2}\nuniform float g_BloomThreshold; // {\"material\":\"bloomthreshold\",\"default\":0.65}\nuniform vec3 g_BloomTint; // {\"material\":\"bloomtint\",\"default\":\"1 1 1\"}\nvarying vec2 v_TexCoord;\nvoid main(){vec3 c=(texture2D(g_Texture0,v_TexCoord-g_TexelSize).rgb+texture2D(g_Texture0,v_TexCoord+g_TexelSize).rgb)*0.5;gl_FragColor=vec4(max(c-vec3(g_BloomThreshold),vec3(0))*g_BloomStrength*g_BloomTint,1);}",
                ),
                (
                    "downsample_eighth_blur_v",
                    "uniform sampler2D g_Texture0;\nuniform vec2 g_TexelSize;\nvarying vec2 v_TexCoord;\nvoid main(){vec2 d=vec2(g_TexelSize.x*8.0,0);gl_FragColor=vec4((texture2D(g_Texture0,v_TexCoord-d).rgb+texture2D(g_Texture0,v_TexCoord).rgb+texture2D(g_Texture0,v_TexCoord+d).rgb)/3.0,1);}",
                ),
                (
                    "blur_h_bloom",
                    "uniform sampler2D g_Texture0;\nuniform vec2 g_TexelSize;\nvarying vec2 v_TexCoord;\nvoid main(){vec2 d=vec2(0,g_TexelSize.y*8.0);gl_FragColor=vec4((texture2D(g_Texture0,v_TexCoord-d).rgb+texture2D(g_Texture0,v_TexCoord).rgb+texture2D(g_Texture0,v_TexCoord+d).rgb)/3.0,1);}",
                ),
                (
                    "combine",
                    "uniform sampler2D g_Texture0;\nuniform sampler2D g_Texture1;\nvarying vec2 v_TexCoord;\nvoid main(){gl_FragColor=vec4(texture2D(g_Texture0,v_TexCoord).rgb+texture2D(g_Texture1,v_TexCoord).rgb,1);}",
                ),
            ];
            for (name, fragment) in shaders {
                fixture.write(&format!("shaders/{name}.vert"), vertex);
                fixture.write(&format!("shaders/{name}.frag"), fragment);
            }
            for (material, shader, inputs) in [
                (
                    "downsample_quarter_bloom",
                    "downsample_quarter_bloom",
                    vec!["_rt_FullFrameBuffer"],
                ),
                (
                    "downsample_eighth_blur_v",
                    "downsample_eighth_blur_v",
                    vec!["_rt_4FrameBuffer"],
                ),
                ("blur_h_bloom", "blur_h_bloom", vec!["_rt_8FrameBuffer"]),
                (
                    "combine_ldr",
                    "combine",
                    vec!["_rt_FullFrameBuffer", "_rt_Bloom"],
                ),
            ] {
                fixture.write(&format!("materials/util/{material}.json"), &serde_json::json!({"passes":[{"shader":shader,"textures":inputs,"cullmode":"nocull","depthtest":"disabled","depthwrite":"disabled"}]}).to_string());
            }
            fixture
        }
        fn write(&self, name: &str, content: &str) {
            let path = self.0.join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, content).unwrap();
        }
        fn prepare(&self, general: &Value) -> Result<Option<PreparedBloom>> {
            let assets = SceneAssets::open(&self.0, None)?;
            PreparedBloom::prepare(&assets, general, &mut |name| {
                Ok(serde_json::from_slice(
                    &assets.read(name, super::super::MAX_JSON)?,
                )?)
            })
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn bloom_preparation_resolves_constants_and_rejects_changed_graphs() -> Result<()> {
        let fixture = Fixture::new();
        let prepared = fixture.prepare(&serde_json::json!({"bloom":true,"bloomstrength":1.58,"bloomthreshold":0.61,"bloomtint":"0.01 0.02 0.5"}))?.unwrap();
        assert_eq!(prepared.constants["g_BloomStrength"], [1.58]);
        assert_eq!(prepared.constants["g_BloomTint"], [0.01, 0.02, 0.5]);
        assert!(
            fixture
                .prepare(&serde_json::json!({"bloom":true,"hdr":true}))
                .is_err()
        );
        assert!(
            fixture
                .prepare(&serde_json::json!({"bloom":true,"bloomtint":[1,2]}))
                .is_err()
        );
        fixture.write(
            "materials/util/blur_h_bloom.json",
            r#"{"passes":[{"shader":"blur_h_bloom","textures":["_rt_FullFrameBuffer"]}]}"#,
        );
        assert!(fixture.prepare(&serde_json::json!({"bloom":true})).is_err());
        // Disabled bloom does not access material resources.
        assert!(
            fixture
                .prepare(&serde_json::json!({"bloom":false}))?
                .is_none()
        );
        Ok(())
    }

    #[test]
    fn bloom_does_not_accept_unbound_shader_inputs() -> Result<()> {
        let fixture = Fixture::new();
        fixture.write(
            "shaders/blur_h_bloom.frag",
            "uniform sampler2D g_Texture0;\nuniform sampler2D g_Texture1;\nuniform vec2 g_TexelSize;\nvarying vec2 v_TexCoord;\nvoid main(){gl_FragColor=texture2D(g_Texture0,v_TexCoord)+texture2D(g_Texture1,v_TexCoord);}",
        );
        let error = fixture
            .prepare(&serde_json::json!({"bloom":true}))
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("additional bloom shader dependency g_Texture1")
        );
        Ok(())
    }

    struct OffscreenContext {
        instance: khronos_egl::DynamicInstance<khronos_egl::EGL1_4>,
        display: khronos_egl::Display,
        context: Option<khronos_egl::Context>,
        surface: Option<khronos_egl::Surface>,
    }
    impl OffscreenContext {
        fn new() -> Result<Self> {
            use khronos_egl as egl;
            let instance = unsafe { egl::DynamicInstance::<egl::EGL1_4>::load_required()? };
            let display = unsafe { instance.get_display(egl::DEFAULT_DISPLAY) }
                .context("No offscreen EGL display")?;
            instance.initialize(display)?;
            let mut result = Self {
                instance,
                display,
                context: None,
                surface: None,
            };
            result.instance.bind_api(egl::OPENGL_API)?;
            let config = result
                .instance
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
            result.context = Some(result.instance.create_context(
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
            )?);
            result.surface = Some(result.instance.create_pbuffer_surface(
                display,
                config,
                &[egl::WIDTH, 64, egl::HEIGHT, 64, egl::NONE],
            )?);
            result.instance.make_current(
                display,
                result.surface,
                result.surface,
                result.context,
            )?;
            gl::load_with(|name| {
                result
                    .instance
                    .get_proc_address(name)
                    .map(|function| function as *const std::ffi::c_void)
                    .unwrap_or(std::ptr::null())
            });
            Ok(result)
        }
    }
    impl Drop for OffscreenContext {
        fn drop(&mut self) {
            let _ = self.instance.make_current(self.display, None, None, None);
            if let Some(surface) = self.surface {
                let _ = self.instance.destroy_surface(self.display, surface);
            }
            if let Some(context) = self.context {
                let _ = self.instance.destroy_context(self.display, context);
            }
            let _ = self.instance.terminate(self.display);
        }
    }

    fn fill_bright_square(renderer: &BloomRenderer) {
        let state = GlState::capture();
        state.begin();
        unsafe {
            gl::BindFramebuffer(gl::DRAW_FRAMEBUFFER, renderer.scene_framebuffer());
            gl::ClearColor(0.0, 0.0, 0.0, 1.0);
            gl::Clear(gl::COLOR_BUFFER_BIT);
            gl::Enable(gl::SCISSOR_TEST);
            gl::Scissor(28, 28, 8, 8);
            gl::ClearColor(1.0, 1.0, 1.0, 1.0);
            gl::Clear(gl::COLOR_BUFFER_BIT);
        }
    }

    fn pixels(target: &RenderTarget) -> Vec<u8> {
        let mut result = vec![0_u8; 64 * 64 * 4];
        unsafe {
            gl::BindFramebuffer(gl::READ_FRAMEBUFFER, target.framebuffer());
            gl::BindBuffer(gl::PIXEL_PACK_BUFFER, 0);
            gl::PixelStorei(gl::PACK_ALIGNMENT, 1);
            gl::ReadPixels(
                0,
                0,
                64,
                64,
                gl::RGBA,
                gl::UNSIGNED_BYTE,
                result.as_mut_ptr().cast(),
            );
        }
        result
    }

    fn particle_padding_and_orientation(shader: &Shader, cutout: bool) -> Result<()> {
        let state = GlState::capture();
        state.begin();
        let program = Program::compile(shader, "authored padded particle texture")?;
        let source = RenderTarget::new(2, 2)?;
        let output = RenderTarget::new(2, 2)?;
        let geometry = QuadGeometry::new()?;
        // A 2x1 authored image occupies 2x2 storage. Match upload's reversal of
        // the whole storage rows and the CPU quad's UV0 at its bottom edge.
        let top_down = [[255_u8, 0, 0, 38, 0, 255, 0, 255], [0; 8]];
        let uploaded = [top_down[1], top_down[0]].concat();
        unsafe {
            gl::ActiveTexture(gl::TEXTURE0);
            gl::BindTexture(gl::TEXTURE_2D, source.texture_id());
            gl::TexSubImage2D(
                gl::TEXTURE_2D,
                0,
                0,
                0,
                2,
                2,
                gl::RGBA,
                gl::UNSIGNED_BYTE,
                uploaded.as_ptr().cast(),
            );
            gl::BindFramebuffer(gl::DRAW_FRAMEBUFFER, output.framebuffer());
            gl::Viewport(0, 0, 2, 2);
            gl::UseProgram(program.id());
            gl::BindVertexArray(geometry.vao);
            gl::VertexAttrib4f(2, 1.0, 1.0, 1.0, 1.0);
            gl::Uniform1i(program.location("g_Texture0"), 0);
        }
        program.matrix(&super::super::renderer::IDENTITY);
        program.constants(&BTreeMap::from([
            ("g_Overbright".to_owned(), vec![1.0]),
            ("g_CutoutStart".to_owned(), vec![0.1]),
            ("g_CutoutEnd".to_owned(), vec![0.2]),
            ("g_CutoutOpacity".to_owned(), vec![1.0]),
        ]))?;
        let mut readback = [0_u8; 16];
        unsafe {
            gl::DrawArrays(gl::TRIANGLES, 0, 6);
            gl::BindFramebuffer(gl::READ_FRAMEBUFFER, output.framebuffer());
            gl::BindBuffer(gl::PIXEL_PACK_BUFFER, 0);
            gl::PixelStorei(gl::PACK_ALIGNMENT, 1);
            gl::ReadPixels(
                0,
                0,
                2,
                2,
                gl::RGBA,
                gl::UNSIGNED_BYTE,
                readback.as_mut_ptr().cast(),
            );
        }
        assert_eq!(&readback[..11], &[0, 0, 0, 0, 0, 0, 0, 0, 255, 0, 0]);
        assert_eq!(&readback[12..], &[0, 255, 0, 255]);
        if cutout {
            assert!(
                (122..=126).contains(&readback[11]),
                "Unexpected cutout alpha: {}",
                readback[11]
            );
        } else {
            assert_eq!(readback[11], 38);
        }
        check_gl("particle padding and orientation acceptance")?;
        Ok(())
    }

    #[test]
    #[ignore = "requires an explicit EGL/OpenGL offscreen acceptance run"]
    fn offscreen_bloom_halo_threshold_tint_and_state() -> Result<()> {
        let _context = OffscreenContext::new()?;
        let fixture = Fixture::new();
        let general = serde_json::json!({"bloom":true,"bloomstrength":2.0,"bloomthreshold":0.1,"bloomtint":[1.0,0.5,0.25]});
        let prepared = fixture.prepare(&general)?.unwrap();
        let renderer = BloomRenderer::initialize(&prepared, 64, 64)?;
        let output = RenderTarget::new(64, 64)?;
        fill_bright_square(&renderer);
        unsafe {
            gl::UseProgram(0);
            gl::BindFramebuffer(gl::DRAW_FRAMEBUFFER, output.framebuffer());
            gl::Viewport(3, 2, 19, 20);
            gl::Enable(gl::BLEND);
            gl::Enable(gl::SCISSOR_TEST);
            gl::Scissor(1, 2, 3, 4);
            gl::ColorMask(gl::FALSE, gl::TRUE, gl::FALSE, gl::TRUE);
        }
        let before = GlState::capture();
        renderer.composite_to(output.framebuffer())?;
        assert_eq!(GlState::capture(), before);
        drop(before);
        assert!(renderer.composite_to(renderer.scene_framebuffer()).is_err());
        let readback = pixels(&output);
        let pixel = |x: usize, y: usize| &readback[(y * 64 + x) * 4..(y * 64 + x + 1) * 4];
        assert_eq!(pixel(32, 32), [255, 255, 255, 255]);
        assert_eq!(pixel(0, 0), [0, 0, 0, 255]);
        let halo = pixel(20, 20);
        assert!(
            halo[0] > halo[1] && halo[1] > halo[2] && halo[2] > 0,
            "Missing tinted bloom halo: {halo:?}"
        );
        let state = GlState::capture();
        state.begin();
        let names = [
            renderer.full.texture_id(),
            renderer.quarter.texture_id(),
            renderer.eighth.texture_id(),
            renderer.bloom.texture_id(),
        ];
        for (texture, expected) in names.into_iter().zip([[64, 64], [16, 16], [8, 8], [8, 8]]) {
            let mut size = [0; 2];
            unsafe {
                gl::BindTexture(gl::TEXTURE_2D, texture);
                gl::GetTexLevelParameteriv(gl::TEXTURE_2D, 0, gl::TEXTURE_WIDTH, &mut size[0]);
                gl::GetTexLevelParameteriv(gl::TEXTURE_2D, 0, gl::TEXTURE_HEIGHT, &mut size[1]);
            }
            assert_eq!(size, expected);
        }
        drop(state);
        drop(renderer);
        let prepared = fixture
            .prepare(&serde_json::json!({"bloom":true,"bloomthreshold":1.0}))?
            .unwrap();
        let renderer = BloomRenderer::initialize(&prepared, 64, 64)?;
        fill_bright_square(&renderer);
        renderer.composite_to(output.framebuffer())?;
        let readback = pixels(&output);
        assert_eq!(
            &readback[(20 * 64 + 20) * 4..(20 * 64 + 20 + 1) * 4],
            [0, 0, 0, 255]
        );
        check_gl("authored offscreen bloom acceptance")?;
        Ok(())
    }

    #[test]
    #[ignore = "requires an explicit tiny EGL/OpenGL offscreen run"]
    fn offscreen_private_bloom_resize_keeps_old_layout_and_charges_full_replacement() -> Result<()>
    {
        let _context = OffscreenContext::new()?;
        let fixture = Fixture::new();
        let bloom = std::sync::Arc::new(
            fixture
                .prepare(&serde_json::json!({"bloom":true}))?
                .unwrap(),
        );
        let scene = super::super::PreparedScene {
            canvas: [2.0, 2.0],
            clear: [0.0, 0.0, 0.0, 1.0],
            textures: Vec::new(),
            layers: Vec::new(),
            audio: Vec::new(),
            bloom: Some(std::sync::Arc::clone(&bloom)),
            particles: Vec::new(),
            draw_order: Vec::new(),
            summary: super::super::SceneSummary {
                layer_count: 0,
                effect_count: 0,
                pass_count: 0,
                animated: false,
            },
            scripts: None,
            script_layers: BTreeMap::new(),
            hierarchy: None,
        };
        let mut renderer = scene.initialize()?;
        renderer.resize(2, 2)?;
        let old_layout = bloom.estimated_bytes(2, 2)?;
        let replacement = renderer.additional_gpu_bytes(16, 16)?;
        assert_eq!(replacement, bloom.estimated_bytes(16, 16)?);
        assert_eq!(renderer.allocation_plan()?.private_bytes, old_layout + 4);
        let mut combined = super::super::shared_texture::ResourcePlan {
            private_bytes: super::super::shared_texture::PAYLOAD_LIMIT
                - (old_layout + 4)
                - replacement
                + 1,
            ..Default::default()
        };
        combined.merge(&renderer.allocation_plan()?)?;
        assert!(
            combined.preflight(0, replacement - old_layout).is_ok(),
            "fixture must distinguish a delta-only underestimate"
        );
        assert!(combined.preflight(0, replacement).is_err());

        let output = RenderTarget::new(2, 2)?;
        unsafe {
            gl::BindFramebuffer(gl::FRAMEBUFFER, output.framebuffer());
        }
        renderer.render_forced(2, 2, 0.0)?;
        let before = pixels(&output);
        let bytes = renderer.gpu_payload_bytes();
        assert!(
            renderer.resize(32768, 32768).is_err(),
            "over-budget replacement reached GL allocation"
        );
        assert_eq!(
            renderer.additional_gpu_bytes(2, 2)?,
            0,
            "failed resize discarded old private layout"
        );
        assert_eq!(renderer.gpu_payload_bytes(), bytes);
        unsafe {
            gl::BindFramebuffer(gl::FRAMEBUFFER, output.framebuffer());
        }
        renderer.render_forced(2, 2, 1.0)?;
        assert_eq!(pixels(&output), before);
        renderer.resize(16, 16)?;
        assert_eq!(renderer.allocation_plan()?.private_bytes, replacement + 4);
        renderer.cleanup();
        assert_eq!(renderer.gpu_payload_bytes(), 0);
        Ok(())
    }

    #[test]
    #[ignore = "requires original user-owned assets and an explicit offscreen run"]
    fn offscreen_original_bloom_and_effect_shader_assets() -> Result<()> {
        let _context = OffscreenContext::new()?;
        let project = std::env::var_os("LWE_SCENE_TEST_PROJECT")
            .context("Set LWE_SCENE_TEST_PROJECT to the user-owned Scene directory")?;
        let builtins = std::env::var_os("LWE_SCENE_TEST_ASSETS")
            .context("Set LWE_SCENE_TEST_ASSETS to the user-owned engine assets directory")?;
        let assets = SceneAssets::open(
            std::path::Path::new(&project),
            Some(std::path::Path::new(&builtins)),
        )?;
        for (name, overrides, slots) in [
            (
                "effects/waterripple",
                BTreeMap::from([
                    ("MASK".into(), 1),
                    ("SPECULAR".into(), 0),
                    ("PERSPECTIVE".into(), 0),
                ]),
                BTreeSet::from([0, 1, 2]),
            ),
            (
                "effects/iris",
                BTreeMap::from([("MASK".into(), 1), ("BACKGROUND".into(), 0)]),
                BTreeSet::from([0, 1]),
            ),
        ] {
            let shader = shader::load(&assets, name, &overrides, &slots)?;
            let _program = Program::compile(&shader, name)?;
        }
        let particle = shader::load_particle(
            &assets,
            &BTreeMap::from([("REFRACT".into(), 0)]),
            &BTreeSet::from([0]),
        )?;
        let _particle_program = Program::compile(&particle, "original genericparticle fragment")?;
        particle_padding_and_orientation(&particle, false)?;
        let particle_cutout = shader::load_particle(
            &assets,
            &BTreeMap::from([("CUTOUT".into(), 1)]),
            &BTreeSet::from([0]),
        )?;
        particle_padding_and_orientation(&particle_cutout, true)?;
        let mut json = |name: &str| {
            Ok(serde_json::from_slice(
                &assets.read(name, super::super::MAX_JSON)?,
            )?)
        };
        let prepared = PreparedBloom::prepare(&assets, &serde_json::json!({"bloom":true,"bloomstrength":2.0,"bloomthreshold":0.1,"bloomtint":[1.0,0.5,0.25]}), &mut json)?.unwrap();
        let renderer = BloomRenderer::initialize(&prepared, 64, 64)?;
        let output = RenderTarget::new(64, 64)?;
        fill_bright_square(&renderer);
        renderer.composite_to(output.framebuffer())?;
        let readback = pixels(&output);
        let halo = &readback[(24 * 64 + 24) * 4..(24 * 64 + 24 + 1) * 4];
        assert!(
            halo[0] > halo[1] && halo[1] >= halo[2] && halo[0] > 0,
            "Original shaders produced no tinted halo: {halo:?}"
        );
        drop(renderer);
        let prepared = PreparedBloom::prepare(
            &assets,
            &serde_json::json!({"bloom":true,"bloomthreshold":1.0}),
            &mut json,
        )?
        .unwrap();
        let renderer = BloomRenderer::initialize(&prepared, 64, 64)?;
        fill_bright_square(&renderer);
        renderer.composite_to(output.framebuffer())?;
        let readback = pixels(&output);
        assert_eq!(
            &readback[(24 * 64 + 24) * 4..(24 * 64 + 24 + 1) * 4],
            [0, 0, 0, 255]
        );
        check_gl("original bloom and effect shader acceptance")?;
        Ok(())
    }
}
