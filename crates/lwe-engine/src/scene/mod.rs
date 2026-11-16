//! A deliberately bounded native Scene renderer.
//!
//! Preparation performs all I/O and decoding without a GL context. Initialization,
//! drawing, and destruction belong to the engine thread with its context current.

mod audio;
mod dynamic_script;
mod hierarchy;
mod particles;
mod postprocess;
mod prepared_source;
mod properties;
mod property_timeline;
mod renderer;
mod script;
mod shader;
pub(crate) mod shared_texture;
mod spectrum;
pub(crate) mod text;
mod timed_runtime;
pub(crate) mod worker_wake;
pub use timed_runtime::scene_texture_memory_budget;

#[cfg(test)]
mod prepared_texture_identity_tests;

use std::collections::{BTreeMap, BTreeSet};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;

use anyhow::{Context, Result, bail, ensure};
use lwe_scene::{ResourceOrigin, SceneAssets, Texture};
use properties::ProjectProperties;
use serde_json::Value;
use sha2::{Digest, Sha256};

pub(crate) use audio::SceneAudio;
pub use properties::{
    PropertyDefinition, PropertyGroup, PropertyKind, PropertyOption, PropertyValue,
    apply_property_overrides, property_definitions, property_groups,
};
pub use renderer::SceneRenderer;

/// Decoded, immutable capabilities supplied by the application worker. Keys
/// name declared texture controls; no filesystem paths enter the renderer.
pub type SceneResourceChoices = BTreeMap<String, lwe_scene::ImageSource>;

const MAX_JSON: usize = 4 * 1024 * 1024;
const MAX_TEXTURE: usize = 256 * 1024 * 1024;
const MAX_SCENE_RGBA: usize = 512 * 1024 * 1024;
const MAX_LAYERS: usize = 32;
const MAX_PASSES: usize = 64;
pub(crate) const TEXTURE_SLOTS: usize = 8;

#[derive(Debug)]
pub struct UnsupportedFeature {
    pub code: &'static str,
    pub detail: String,
}

impl std::fmt::Display for UnsupportedFeature {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Unsupported Scene feature ({}): {}",
            self.code, self.detail
        )
    }
}
impl std::error::Error for UnsupportedFeature {}

fn unsupported(code: &'static str, detail: impl Into<String>) -> anyhow::Error {
    UnsupportedFeature {
        code,
        detail: detail.into(),
    }
    .into()
}

pub fn unsupported_feature_code(error: &anyhow::Error) -> Option<&'static str> {
    error
        .downcast_ref::<UnsupportedFeature>()
        .map(|feature| feature.code)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SceneSummary {
    pub layer_count: usize,
    pub effect_count: usize,
    pub pass_count: usize,
    pub animated: bool,
}

pub struct PreparedScene {
    pub(crate) canvas: [f32; 2],
    pub(crate) clear: [f32; 4],
    pub(crate) textures: Vec<PreparedTexture>,
    pub(crate) layers: Vec<PreparedLayer>,
    audio: Vec<audio::PreparedAudio>,
    bloom: Option<Arc<postprocess::PreparedBloom>>,
    pub(crate) particles: Vec<PreparedParticleLayer>,
    pub(crate) draw_order: Vec<DrawObject>,
    summary: SceneSummary,
    scripts: Option<Arc<dynamic_script::PreparedScripts>>,
    script_layers: BTreeMap<usize, usize>,
    hierarchy: Option<Arc<SceneHierarchy>>,
}

struct SceneHierarchy {
    graph: hierarchy::Graph,
    // Authored node indices map to render layers without changing draw order.
    layers: Vec<(usize, usize)>,
    initial: Vec<Option<hierarchy::Resolved>>,
}

pub(crate) struct PreparedParticleLayer {
    simulation: particles::PreparedParticles,
    materials: Vec<PreparedPass>,
    aspects: Vec<f32>,
    animations: Vec<Option<lwe_scene::AnimationMetadata>>,
    vertex_capacity: usize,
    animation_reservation: lwe_scene::AtlasMemoryReservation,
}

#[derive(Clone, Copy)]
pub(crate) enum DrawObject {
    Image(usize),
    Particles(usize),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum MaterialKind {
    Image,
    Effect,
    Particle,
}

impl std::fmt::Debug for PreparedScene {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PreparedScene")
            .field("canvas", &self.canvas)
            .field("summary", &self.summary)
            .field("texture_count", &self.textures.len())
            .finish()
    }
}

#[derive(Debug)]
pub(crate) struct PreparedTexture {
    /// Computed on the preparation worker from the bytes actually decoded,
    /// their source namespace and the complete decode contract/metadata.
    pub fingerprint: [u8; 32],
    pub width: u32,
    pub height: u32,
    pub image_width: u32,
    pub image_height: u32,
    pub source_format: u32,
    pub source_flags: u32,
    pub rgba: Vec<u8>,
    pub animation: Option<lwe_scene::AnimationMetadata>,
    pub timed: Option<Arc<lwe_scene::TimedAtlasSource>>,
    pub static_source: Option<Arc<prepared_source::PreparedStaticTexture>>,
    pub image: Option<lwe_scene::ImageSource>,
    pub text_frame: Option<text::TextFrame>,
    pub _rgba_reservation: Option<lwe_scene::AtlasMemoryReservation>,
}

const TEXTURE_IDENTITY_CONTRACT: &[u8] = b"lwe-scene-rgba8-top-mip-identity-v1";

// Length framing prevents ambiguous concatenations of paths and byte payloads.
fn fingerprint_bytes(digest: &mut Sha256, bytes: &[u8]) {
    digest.update((bytes.len() as u64).to_le_bytes());
    digest.update(bytes);
}

fn fingerprint_animation(digest: &mut Sha256, animation: Option<&lwe_scene::AnimationMetadata>) {
    digest.update([u8::from(animation.is_some())]);
    if let Some(animation) = animation {
        for value in [animation.version, animation.width, animation.height] {
            digest.update(value.to_le_bytes());
        }
        digest.update((animation.frames.len() as u64).to_le_bytes());
        for frame in &animation.frames {
            digest.update(frame.image.to_le_bytes());
            digest.update(frame.duration.to_bits().to_le_bytes());
            for coordinate in frame.coordinates {
                digest.update(coordinate.to_bits().to_le_bytes());
            }
        }
    }
}

fn fingerprint_metadata(digest: &mut Sha256, metadata: &lwe_scene::TextureMetadata) {
    for value in [
        metadata.version,
        metadata.body_version,
        metadata.format,
        metadata.flags,
        metadata.storage_width,
        metadata.storage_height,
        metadata.image_width,
        metadata.image_height,
        metadata.image_info_extra,
        metadata.images,
    ] {
        digest.update(value.to_le_bytes());
    }
    digest.update(metadata.container_format.to_le_bytes());
    digest.update([u8::from(metadata.body_extra.is_some())]);
    if let Some(extra) = metadata.body_extra {
        digest.update(extra.to_le_bytes());
    }
    digest.update([u8::from(metadata.video)]);
    digest.update((metadata.mips.len() as u64).to_le_bytes());
    for mip in &metadata.mips {
        for value in [
            mip.image,
            mip.level,
            mip.width,
            mip.height,
            mip.decompressed_size,
            mip.stored_size,
        ] {
            digest.update(value.to_le_bytes());
        }
        digest.update((mip.offset as u64).to_le_bytes());
        digest.update([u8::from(mip.compressed), u8::from(mip.video)]);
    }
    fingerprint_animation(digest, metadata.animation.as_ref());
}

#[derive(Clone, Copy)]
struct TextureSource<'a> {
    origin: ResourceOrigin,
    root: &'a Path,
    path: &'a str,
    binding: &'a str,
    atlas: bool,
}

fn fingerprint_source(digest: &mut Sha256, source: TextureSource<'_>) {
    fingerprint_bytes(digest, TEXTURE_IDENTITY_CONTRACT);
    digest.update([match source.origin {
        ResourceOrigin::Package => 0,
        ResourceOrigin::Project => 1,
        ResourceOrigin::Builtin => 2,
    }]);
    fingerprint_bytes(digest, source.root.as_os_str().as_bytes());
    // SceneAssets has already validated this relative path. Components give
    // its normalized spelling without resolving or reopening any pathname.
    let normalized: std::path::PathBuf = Path::new(source.path).components().collect();
    fingerprint_bytes(digest, normalized.as_os_str().as_bytes());
    fingerprint_bytes(digest, source.binding.as_bytes());
    digest.update([u8::from(source.atlas)]);
}

fn texture_fingerprint(source: TextureSource<'_>, encoded: &[u8], decoded: &Texture) -> [u8; 32] {
    let mut digest = Sha256::new();
    fingerprint_source(&mut digest, source);
    fingerprint_bytes(&mut digest, encoded);
    digest.update(decoded.width.to_le_bytes());
    digest.update(decoded.height.to_le_bytes());
    fingerprint_bytes(&mut digest, &decoded.rgba);
    fingerprint_metadata(&mut digest, &decoded.metadata);
    digest.finalize().into()
}

impl PreparedTexture {
    pub(crate) fn rgba(&self) -> &[u8] {
        if let Some(image) = &self.image {
            image.rgba()
        } else if let Some(frame) = &self.text_frame {
            frame.rgba()
        } else if let Some(source) = &self.static_source {
            source.rgba()
        } else {
            &self.rgba
        }
    }

    #[cfg(test)]
    pub(crate) fn fixture_fingerprint(&self) -> [u8; 32] {
        let mut digest = Sha256::new();
        fingerprint_bytes(&mut digest, TEXTURE_IDENTITY_CONTRACT);
        fingerprint_bytes(&mut digest, b"cpu-authored-fixture");
        for value in [
            self.width,
            self.height,
            self.image_width,
            self.image_height,
            self.source_format,
            self.source_flags,
        ] {
            digest.update(value.to_le_bytes());
        }
        fingerprint_bytes(&mut digest, &self.rgba);
        fingerprint_animation(&mut digest, self.animation.as_ref());
        digest.finalize().into()
    }
}

#[derive(Debug)]
pub(crate) struct PreparedLayer {
    world_2d: Option<hierarchy::Transform>,
    pub size: [f32; 2],
    pub origin: [f32; 2],
    pub scale: [f32; 2],
    pub angle: f32,
    pub origin_z: f32,
    pub angles: [f32; 3],
    pub tint: [f32; 4],
    pub blend: Blend,
    pub composite: Option<shader::Shader>,
    pub stages: Vec<PreparedStage>,
    pub clamp_uvs: bool,
    pub crop_offset: [f32; 2],
    pub no_padding: bool,
    pub text: Option<text::PreparedText>,
}

#[derive(Debug)]
pub(crate) struct PreparedStage {
    pub targets: BTreeMap<String, f32>,
    pub persistent_targets: BTreeSet<String>,
    pub passes: Vec<PreparedPass>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum Blend {
    Replace,
    Alpha,
    Add,
}

#[derive(Debug, Clone)]
pub(crate) enum TextureBinding {
    Static(usize),
    Input,
    Named(String),
    Black,
    FullFrame,
    History(String),
    LayerComposite,
}

#[derive(Debug, Clone)]
pub(crate) enum PreparedPassKind {
    Render,
    Copy { source: TextureBinding },
}

#[derive(Debug)]
pub(crate) struct PreparedPass {
    pub label: String,
    pub kind: PreparedPassKind,
    pub shader: Option<shader::Shader>,
    pub textures: Vec<TextureBinding>,
    pub constants: BTreeMap<String, Vec<f32>>,
    pub target: Option<String>,
    pub blend: Blend,
}

fn json(assets: &SceneAssets, name: &str, properties: &ProjectProperties<'_>) -> Result<Value> {
    let mut value = serde_json::from_slice(&assets.read(name, MAX_JSON)?)
        .with_context(|| format!("Invalid Scene JSON resource {name}"))?;
    properties.resolve_document(&mut value)?;
    Ok(value)
}

fn has_script(value: &Value) -> bool {
    match value {
        Value::Object(object) => object.iter().any(|(key, value)| {
            matches!(key.to_ascii_lowercase().as_str(), "script" | "scenescript")
                || has_script(value)
        }),
        Value::Array(array) => array.iter().any(has_script),
        _ => false,
    }
}

/// Runtime bindings are limited to modeled layer properties.
/// Resource/material/general scripts still pass through the existing gate.
fn extract_runtime_bindings(
    scene: &mut Value,
    project: &Value,
    properties: &ProjectProperties<'_>,
) -> Result<Vec<dynamic_script::Binding>> {
    let scalar_properties: script::PropertySnapshot = property_definitions(project)?
        .into_iter()
        .filter_map(|property| {
            let value = match property.value {
                PropertyValue::Bool(value) => script::ScalarProperty::Bool(value),
                PropertyValue::Slider(value) => script::ScalarProperty::Number(value),
                PropertyValue::Color(_)
                | PropertyValue::Combo(_)
                | PropertyValue::TextInput(_)
                | PropertyValue::TextDisplay
                | PropertyValue::Texture(_) => {
                    return None;
                }
            };
            Some((property.name, value))
        })
        .collect();
    let mut bindings = Vec::new();
    let objects = scene["objects"]
        .as_array_mut()
        .context("Scene has no objects")?;
    let parent_ids: BTreeSet<_> = objects
        .iter()
        .filter_map(|object| object.get("parent").and_then(Value::as_u64))
        .collect();
    for (index, object) in objects.iter_mut().enumerate() {
        let image = object.get("image").is_some();
        let text = object.get("text").is_some();
        let transform = hierarchy::is_transform_only(object)
            && (object.get("parent").is_some()
                || object
                    .get("id")
                    .and_then(Value::as_u64)
                    .is_some_and(|id| parent_ids.contains(&id)));
        let Some(fields) = object.as_object_mut() else {
            bail!("Invalid Scene object");
        };
        for (property, value) in fields.iter_mut() {
            let Some(wrapper) = value.as_object_mut() else {
                continue;
            };
            let Some(source) = wrapper.get("script").and_then(Value::as_str) else {
                continue;
            };
            let input = wrapper
                .get("value")
                .ok_or_else(|| unsupported("scenescript", "script binding has no input value"))?;
            let animation = wrapper.get("animation");
            if animation.is_none()
                && let Some(number) = input.as_f64()
                && script::evaluate(source, number, &scalar_properties).is_ok()
            {
                continue;
            }
            if !(image || text || transform)
                || !(text && property == "text"
                    || matches!(
                        property.as_str(),
                        "visible" | "origin" | "scale" | "angles" | "color" | "alpha"
                    ))
                || wrapper.keys().any(|key| {
                    !matches!(
                        key.as_str(),
                        "script" | "value" | "user" | "scriptproperties" | "animation"
                    )
                })
            {
                return Err(unsupported(
                    "scenescript",
                    "runtime scripts require a modeled image-layer property",
                ));
            }
            let timeline = animation
                .map(|animation| {
                    if !image || text || property != "alpha" || wrapper.contains_key("user") {
                        return Err(unsupported(
                            "timeline",
                            "mixed timelines require an image layer alpha binding",
                        ));
                    }
                    properties.validate_timeline(animation)?;
                    property_timeline::ScalarTimeline::parse(animation)
                })
                .transpose()?;
            bindings.push(dynamic_script::Binding {
                layer: index + 1,
                property: property.clone(),
                source: source.to_owned(),
                input: input.clone(),
                script_properties: wrapper
                    .get("scriptproperties")
                    .cloned()
                    .unwrap_or_else(|| serde_json::json!({})),
                timeline,
            });
            wrapper.remove("script");
            wrapper.remove("scriptproperties");
            wrapper.remove("animation");
            if !wrapper.contains_key("user") {
                *value = wrapper.remove("value").expect("script input checked");
            }
        }
    }
    Ok(bindings)
}

fn static_value(value: &Value) -> Result<&Value> {
    if let Some(object) = value.as_object() {
        if object.contains_key("script") || object.contains_key("scenescript") {
            return Err(unsupported(
                "scenescript",
                "script-driven property bindings are not supported",
            ));
        }
        if object.contains_key("user") || object.contains_key("binding") {
            return Err(unsupported(
                "property_binding",
                "dynamic user property bindings are not supported",
            ));
        }
        return object.get("value").context("Invalid static Scene property");
    }
    Ok(value)
}

fn boolean(object: &Value, key: &str, default: bool) -> Result<bool> {
    object
        .get(key)
        .map(|value| {
            static_value(value)?
                .as_bool()
                .with_context(|| format!("Invalid Scene boolean property {key}"))
        })
        .transpose()
        .map(|value| value.unwrap_or(default))
}

fn numbers(value: &Value) -> Result<Vec<f32>> {
    let value = static_value(value)?;
    let values = match value {
        Value::Number(number) => vec![number.as_f64().context("Invalid Scene number")? as f32],
        Value::String(string) => string
            .split(|c: char| c.is_whitespace() || c == ',')
            .filter(|part| !part.is_empty())
            .map(str::parse::<f32>)
            .collect::<std::result::Result<Vec<_>, _>>()?,
        Value::Array(array) => array
            .iter()
            .map(|value| {
                value
                    .as_f64()
                    .map(|number| number as f32)
                    .context("Invalid numeric Scene array")
            })
            .collect::<Result<Vec<_>>>()?,
        _ => bail!("Invalid numeric Scene property"),
    };
    ensure!(
        !values.is_empty() && values.len() <= 16 && values.iter().all(|value| value.is_finite()),
        "Invalid or non-finite Scene numeric property"
    );
    Ok(values)
}

fn vector<const N: usize>(object: &Value, key: &str, default: [f32; N]) -> Result<[f32; N]> {
    let Some(value) = object.get(key) else {
        return Ok(default);
    };
    numbers(value)?
        .try_into()
        .map_err(|_| anyhow::anyhow!("Scene property {key} must have {N} components"))
}

fn combos(value: Option<&Value>) -> Result<BTreeMap<String, i32>> {
    let Some(value) = value else {
        return Ok(BTreeMap::new());
    };
    let object = value.as_object().context("Invalid shader combos")?;
    object
        .iter()
        .map(|(key, value)| {
            let value = value.as_i64().context("Invalid shader combo value")?;
            Ok((key.clone(), i32::try_from(value)?))
        })
        .collect()
}

fn blend(value: Option<&Value>) -> Result<Blend> {
    match value.and_then(Value::as_str).unwrap_or("normal") {
        "normal" => Ok(Blend::Replace),
        "translucent" | "alpha" => Ok(Blend::Alpha),
        "additive" | "add" => Ok(Blend::Add),
        other => Err(unsupported("shader", format!("blend mode {other}"))),
    }
}

fn validate_pass(pass: &Value) -> Result<()> {
    for key in ["depthtest", "depthwrite"] {
        if pass
            .get(key)
            .and_then(Value::as_str)
            .is_some_and(|value| value != "disabled")
        {
            return Err(unsupported("model", format!("material {key}")));
        }
    }
    if pass
        .get("cullmode")
        .and_then(Value::as_str)
        .is_some_and(|value| value != "nocull")
    {
        return Err(unsupported("model", "material face culling"));
    }
    Ok(())
}

fn prepare_text(assets: &SceneAssets, object: &Value) -> Result<text::PreparedText> {
    for key in [
        "blockalign",
        "limitrows",
        "limituseellipsis",
        "limitwidth",
        "opaquebackground",
        "castshadow",
        "disablepropagation",
    ] {
        if boolean(object, key, false)? {
            return Err(unsupported("text", format!("text {key}")));
        }
    }
    ensure!(
        object
            .get("anchor")
            .is_none_or(|v| v.as_str() == Some("none")),
        "Unsupported text anchor"
    );
    ensure!(
        object
            .get("depthtest")
            .is_none_or(|v| matches!(v.as_str(), Some("enabled" | "disabled"))),
        "Invalid text depth mode"
    );
    let size = vector(object, "size", [0.0; 2])?;
    ensure!(
        size.iter()
            .all(|v| v.is_finite() && (1.0..=4096.0).contains(v) && v.fract() == 0.0),
        "Invalid authored text surface dimensions"
    );
    let font = object["font"]
        .as_str()
        .context("Text object has no font resource")?;
    let resource = assets
        .resource(font)
        .context("Missing authored text font")?;
    let point_size = object["pointsize"]
        .as_f64()
        .context("Text object has no point size")? as f32;
    let padding = u32::try_from(
        object
            .get("padding")
            .map(|v| v.as_u64().context("Invalid text padding"))
            .transpose()?
            .unwrap_or(0),
    )?;
    text::PreparedText::from_resource(
        &resource,
        size[0] as u32,
        size[1] as u32,
        point_size,
        padding,
        object
            .get("horizontalalign")
            .map(|v| v.as_str().context("Invalid text horizontal alignment"))
            .transpose()?
            .unwrap_or("left"),
        object
            .get("verticalalign")
            .map(|v| v.as_str().context("Invalid text vertical alignment"))
            .transpose()?
            .unwrap_or("top"),
    )
}

fn validate_instance(instance: &Value) -> Result<()> {
    if instance.is_null() {
        return Ok(());
    }
    let object = instance.as_object().context("Invalid image instance")?;
    ensure!(
        object.len() <= 6
            && object.keys().all(|k| matches!(
                k.as_str(),
                "id" | "combos"
                    | "textures"
                    | "usertextures"
                    | "constantshadervalues"
                    | "usershadervalues"
            )),
        "Unsupported image instance field"
    );
    if let Some(id) = object.get("id") {
        ensure!(id.as_u64().is_some(), "Invalid image instance identity");
    }
    if has_script(instance) {
        return Err(unsupported("scenescript", "image instance script"));
    }
    for key in ["textures", "usertextures"] {
        if let Some(values) = object.get(key) {
            let values = values
                .as_array()
                .context("Invalid image instance texture slots")?;
            ensure!(
                values.len() <= TEXTURE_SLOTS,
                "Too many instance texture slots"
            );
            ensure!(
                values
                    .iter()
                    .all(|v| v.is_null() || v.as_str().is_some_and(|s| s.len() <= 256)),
                "Invalid instance texture slot"
            );
        }
    }
    Ok(())
}

struct Preparation<'a> {
    assets: &'a SceneAssets,
    properties: &'a ProjectProperties<'a>,
    choices: &'a SceneResourceChoices,
    texture_properties: BTreeSet<String>,
    inspection: lwe_scene::TextureInspectionBudget,
    active_layer_id: Option<u64>,
    textures: Vec<PreparedTexture>,
    texture_indices: BTreeMap<String, usize>,
    rgba_bytes: usize,
    pass_count: usize,
    pass_limit: usize,
}

impl Preparation<'_> {
    fn texture(&mut self, name: &str, atlas: bool) -> Result<TextureBinding> {
        if name == "_rt_FullFrameBuffer" {
            return Ok(TextureBinding::FullFrame);
        }
        if self
            .active_layer_id
            .is_some_and(|id| name == format!("_rt_imageLayerComposite_{id}_a"))
        {
            return Ok(TextureBinding::LayerComposite);
        }
        if name.starts_with("_rt_") || name.starts_with("_alias_") {
            return Err(unsupported(
                "shader",
                format!("implicit runtime texture {name}"),
            ));
        }
        if let Some(index) = self.texture_indices.get(name) {
            if !atlas && self.textures[*index].animation.is_some() {
                return Err(unsupported(
                    "animated_texture",
                    "image animation has no runtime implementation",
                ));
            }
            return Ok(TextureBinding::Static(*index));
        }
        let path = if name.starts_with("materials/") && name.ends_with(".tex") {
            name.to_owned()
        } else {
            format!("materials/{name}.tex")
        };
        // Inspect and decode the same sealed descriptor, retaining its VFS
        // namespace in the identity. No second pathname lookup can change it.
        let resource = self
            .assets
            .resource(&path)
            .with_context(|| format!("Missing Scene texture {path}"))?;
        if resource.len() > MAX_TEXTURE as u64 {
            return Err(lwe_scene::SceneAssetError::LimitExceeded {
                resource: path.clone(),
                size: resource.len(),
                limit: MAX_TEXTURE as u64,
            }
            .into());
        }
        let inspection_bytes = self
            .inspection
            .remaining_mips
            .clamp(1, 32768)
            .next_power_of_two()
            .max(4)
            .checked_mul(3 * std::mem::size_of::<lwe_scene::MipMetadata>())
            .and_then(|n| {
                n.checked_add(
                    self.inspection.remaining_animation_frames.min(4096)
                        * std::mem::size_of::<lwe_scene::AnimationFrame>()
                        + 1024,
                )
            })
            .context("Texture inspection budget overflow")?;
        let inspection_reservation =
            scene_texture_memory_budget().reserve_bytes(inspection_bytes)?;
        let origin = resource.origin();
        let metadata = Texture::inspect_resource_bounded(&resource, &mut self.inspection)?;
        let root = match origin {
            ResourceOrigin::Package | ResourceOrigin::Project => self.assets.project_dir(),
            ResourceOrigin::Builtin => self
                .assets
                .assets_dir()
                .context("Scene builtin texture namespace unavailable")?,
        };
        let source = TextureSource {
            origin,
            root,
            path: &path,
            binding: name,
            atlas,
        };
        let index = self.textures.len();
        let mut encoded_reservation =
            scene_texture_memory_budget().reserve_bytes(resource.len() as usize)?;
        let bytes = resource.read_range(0, resource.len() as usize, MAX_TEXTURE)?;
        encoded_reservation.resize(bytes.capacity())?;
        let actual_metadata = Texture::inspect_bounded(&bytes, &mut self.inspection)?;
        ensure!(
            actual_metadata == metadata,
            "Texture descriptor framing changed during preparation"
        );
        drop(actual_metadata);
        let mut digest = Sha256::new();
        fingerprint_bytes(&mut digest, b"lwe-weak-cpu-texture-source-v1");
        digest.update(scene_texture_memory_budget().domain_id().to_le_bytes());
        fingerprint_source(&mut digest, source);
        fingerprint_bytes(&mut digest, &bytes);
        fingerprint_metadata(&mut digest, &metadata);
        let cache_key = digest.finalize().into();
        if !atlas && metadata.animation.as_ref().is_some_and(|a| a.version == 3) {
            drop(metadata);
            drop(inspection_reservation);
            let timed = if let Some(source) = prepared_source::get_timed(cache_key) {
                drop(bytes);
                drop(encoded_reservation);
                source
            } else {
                let source = Arc::new(lwe_scene::TimedAtlasSource::from_budgeted_bytes(
                    bytes,
                    encoded_reservation,
                    origin,
                    &mut self.inspection,
                    scene_texture_memory_budget(),
                )?);
                prepared_source::put_timed(cache_key, &source);
                source
            };
            let metadata = timed.metadata();
            let mut digest = Sha256::new();
            fingerprint_source(&mut digest, source);
            fingerprint_bytes(&mut digest, timed.encoded_bytes());
            fingerprint_metadata(&mut digest, metadata);
            self.textures.push(PreparedTexture {
                fingerprint: digest.finalize().into(),
                width: metadata.storage_width,
                height: metadata.storage_height,
                image_width: metadata.image_width,
                image_height: metadata.image_height,
                source_format: metadata.format,
                source_flags: metadata.flags,
                rgba: Vec::new(),
                animation: None,
                timed: Some(timed),
                static_source: None,
                image: None,
                text_frame: None,
                _rgba_reservation: None,
            });
        } else {
            let cached = prepared_source::get_static(cache_key);
            let owner = if let Some(owner) = cached {
                drop(metadata);
                drop(inspection_reservation);
                drop(bytes);
                drop(encoded_reservation);
                owner
            } else {
                let mip = metadata.mips.first().context("Scene texture has no mip")?;
                let pixels = (mip.width as usize)
                    .checked_mul(mip.height as usize)
                    .context("Texture size overflow")?;
                let mip_info = (
                    mip.offset,
                    mip.stored_size as usize,
                    mip.compressed,
                    mip.decompressed_size as usize,
                );
                let scratch = pixels
                    .checked_mul(4)
                    // The complete TEX input already has its own reservation. ImageReader
                    // only borrows the first mip; lower mips are never decoder input.
                    // For LZ4-wrapped images, charge the larger decoded container here.
                    .and_then(|n| n.checked_add(mip_info.1.max(mip_info.3).checked_mul(3)?))
                    .and_then(|n| n.checked_add(mip.decompressed_size as usize))
                    .and_then(|n| n.checked_add(4 * 1024 * 1024))
                    .context("Texture decoder working set overflow")?;
                let mut reservation = scene_texture_memory_budget().reserve_bytes(scratch)?;
                drop(metadata);
                drop(inspection_reservation);
                let stored = &bytes[mip_info.0..mip_info.0 + mip_info.1];
                let raw = if mip_info.2 {
                    Some(
                        lz4_flex::block::decompress(stored, mip_info.3)
                            .context("Invalid texture LZ4 data")?,
                    )
                } else {
                    None
                };
                let payload = raw.as_deref().unwrap_or(stored);
                // The decoder detects encoded images by their actual mip
                // signature even when TEX metadata names a raw pixel format.
                let encoded_image = payload.starts_with(b"\x89PNG\r\n\x1a\n")
                    || payload.starts_with(b"\xff\xd8\xff");
                if encoded_image {
                    reservation.resize(
                        scratch
                            .checked_add(
                                pixels
                                    .checked_mul(28)
                                    .context("Texture image working set overflow")?,
                            )
                            .context("Texture image working set overflow")?,
                    )?;
                }
                drop(raw);
                let mut decoded = if atlas {
                    Texture::decode_atlas(&bytes)
                } else {
                    Texture::decode(&bytes)
                }
                .with_context(|| format!("Cannot decode Scene texture {path}"))?;
                let fingerprint = texture_fingerprint(source, &bytes, &decoded);
                drop(bytes);
                drop(encoded_reservation);
                drop(std::mem::take(&mut decoded.metadata.mips));
                let retained = decoded
                    .rgba
                    .capacity()
                    .checked_add(decoded.metadata.animation.as_ref().map_or(0, |a| {
                        a.frames.capacity() * std::mem::size_of::<lwe_scene::AnimationFrame>()
                    }))
                    .context("Texture retained allocation overflow")?;
                reservation.resize(retained)?;
                let owner = Arc::new(prepared_source::PreparedStaticTexture::new(
                    decoded,
                    fingerprint,
                    reservation,
                    !encoded_image,
                ));
                prepared_source::put_static(cache_key, &owner);
                owner
            };
            self.rgba_bytes = self
                .rgba_bytes
                .checked_add(owner.rgba().len())
                .context("Scene texture memory overflow")?;
            ensure!(
                self.rgba_bytes <= MAX_SCENE_RGBA,
                "Scene decoded textures exceed memory budget"
            );
            let mut animation_reservation = owner
                .animation
                .as_ref()
                .map(|a| {
                    scene_texture_memory_budget().reserve_bytes(
                        a.frames.capacity() * std::mem::size_of::<lwe_scene::AnimationFrame>(),
                    )
                })
                .transpose()?;
            let animation = owner.animation.clone();
            if let Some(reservation) = animation_reservation.as_mut() {
                reservation.resize(animation.as_ref().map_or(0, |a| {
                    a.frames.capacity() * std::mem::size_of::<lwe_scene::AnimationFrame>()
                }))?;
            }
            self.textures.push(PreparedTexture {
                fingerprint: owner.fingerprint,
                width: owner.width,
                height: owner.height,
                image_width: owner.image_width,
                image_height: owner.image_height,
                source_format: owner.format,
                source_flags: owner.flags,
                rgba: Vec::new(),
                animation,
                timed: None,
                static_source: Some(owner),
                image: None,
                text_frame: None,
                _rgba_reservation: animation_reservation,
            });
        }
        self.texture_indices.insert(name.to_owned(), index);
        Ok(TextureBinding::Static(index))
    }

    fn selected_texture(&mut self, property: &str) -> Result<Option<TextureBinding>> {
        ensure!(
            self.texture_properties.contains(property),
            "Texture binding does not name a declared resource control"
        );
        let Some(image) = self.choices.get(property) else {
            return Ok(None);
        };
        let key = format!("managed:{property}");
        if let Some(index) = self.texture_indices.get(&key) {
            return Ok(Some(TextureBinding::Static(*index)));
        }
        let mut digest = Sha256::new();
        fingerprint_bytes(&mut digest, b"lwe-managed-resource-binding-v1");
        fingerprint_bytes(&mut digest, property.as_bytes());
        digest.update(image.fingerprint());
        let index = self.textures.len();
        self.textures.push(PreparedTexture {
            fingerprint: digest.finalize().into(),
            width: image.width(),
            height: image.height(),
            image_width: image.width(),
            image_height: image.height(),
            source_format: 0,
            source_flags: 2,
            rgba: Vec::new(),
            animation: None,
            timed: None,
            static_source: None,
            image: Some(image.clone()),
            text_frame: None,
            _rgba_reservation: None,
        });
        self.texture_indices.insert(key, index);
        Ok(Some(TextureBinding::Static(index)))
    }

    fn material(
        &mut self,
        path: &str,
        override_pass: &Value,
        kind: MaterialKind,
        target: Option<String>,
        bindings: &BTreeMap<usize, TextureBinding>,
    ) -> Result<Vec<PreparedPass>> {
        let material = json(self.assets, path, self.properties)?;
        if has_script(&material) {
            return Err(unsupported("scenescript", format!("material {path}")));
        }
        let passes = material["passes"]
            .as_array()
            .context("Scene material has no passes")?;
        ensure!(!passes.is_empty(), "Scene material has no passes");
        if kind == MaterialKind::Particle {
            ensure!(
                passes.len() == 1,
                "Unsupported multi-pass particle material"
            );
        }
        let mut result = Vec::new();
        for pass in passes {
            self.pass_count += 1;
            ensure!(
                self.pass_count <= self.pass_limit,
                "Scene exceeds the render pass budget"
            );
            validate_pass(pass)?;
            let shader_name = pass["shader"]
                .as_str()
                .context("Scene material has no shader")?;
            let mut shader_combos = combos(pass.get("combos"))?;
            shader_combos.extend(combos(override_pass.get("combos"))?);
            let mut textures = vec![TextureBinding::Black; TEXTURE_SLOTS];
            let mut bound_slots = BTreeSet::new();
            // Resolve precedence before decoding. A selected capability shadows
            // the instance fallback, which shadows the material slot. Shadowed
            // resources never consume a second GPU/CPU allocation.
            let mut references = vec![None; TEXTURE_SLOTS];
            for values in [pass.get("textures"), override_pass.get("textures")]
                .into_iter()
                .flatten()
            {
                let values = values
                    .as_array()
                    .context("Invalid Scene texture bindings")?;
                ensure!(
                    values.len() <= TEXTURE_SLOTS,
                    "Scene has too many texture bindings"
                );
                for (slot, value) in values.iter().enumerate() {
                    if !value.is_null() {
                        references[slot] =
                            Some(value.as_str().context("Invalid texture reference")?);
                    }
                }
            }
            let user_textures = override_pass
                .get("usertextures")
                .map(|v| v.as_array().context("Invalid user texture mappings"))
                .transpose()?;
            if let Some(values) = user_textures {
                ensure!(
                    values.len() <= TEXTURE_SLOTS,
                    "Too many user texture mappings"
                );
                for value in values.iter().filter(|v| !v.is_null()) {
                    let name = value
                        .as_str()
                        .context("Invalid user texture property name")?;
                    ensure!(
                        name.is_empty() || self.texture_properties.contains(name),
                        "User texture mapping requires a declared resource control"
                    );
                }
            }
            for (slot, reference) in references.into_iter().enumerate() {
                if bindings.contains_key(&slot) {
                    continue;
                }
                let choice = user_textures
                    .and_then(|values| values.get(slot))
                    .filter(|v| !v.is_null())
                    .map(|v| v.as_str().context("Invalid user texture property name"))
                    .transpose()?;
                if let Some(property) = choice.filter(|name| !name.is_empty())
                    && let Some(binding) = self.selected_texture(property)?
                {
                    textures[slot] = binding;
                    bound_slots.insert(slot);
                    continue;
                }
                if let Some(reference) = reference {
                    textures[slot] = self.texture(reference, kind == MaterialKind::Particle)?;
                    bound_slots.insert(slot);
                }
            }
            for (slot, binding) in bindings {
                ensure!(
                    *slot < TEXTURE_SLOTS,
                    "Scene texture binding slot exceeds limit"
                );
                textures[*slot] = binding.clone();
                bound_slots.insert(*slot);
            }
            if kind == MaterialKind::Effect && !bound_slots.contains(&0) {
                textures[0] = TextureBinding::Input;
            }
            if kind == MaterialKind::Image && shader_name == "genericimage4" {
                textures[1] = TextureBinding::Input;
            }
            let shader = if shader_name == "flat" {
                ensure!(
                    kind == MaterialKind::Image && bound_slots.is_empty(),
                    "Flat quad requires an untextured image material"
                );
                shader::flat_material(&shader_combos)
                    .map_err(|e| unsupported("shader", e.to_string()))?
            } else if shader_name == "genericimage4" {
                ensure!(
                    kind == MaterialKind::Image,
                    "unsupported_scene: genericimage4 non-image material"
                );
                ensure!(
                    bound_slots.contains(&0),
                    "Image material has no primary texture"
                );
                if bound_slots.iter().any(|slot| *slot != 0) {
                    return Err(unsupported(
                        "shader",
                        "genericimage4 extra texture bindings",
                    ));
                }
                let timed = match textures[0] {
                    TextureBinding::Static(index) => {
                        let texture = &self.textures[index];
                        if !(if texture.timed.is_some() {
                            texture.source_flags == 7
                        } else {
                            matches!(texture.source_flags, 0 | 2 | 3)
                        }) {
                            return Err(unsupported(
                                "shader",
                                format!(
                                    "genericimage4 texture format {} flags {} ({}x{}, image {}x{})",
                                    texture.source_format,
                                    texture.source_flags,
                                    texture.width,
                                    texture.height,
                                    texture.image_width,
                                    texture.image_height
                                ),
                            ));
                        }
                        texture.timed.is_some()
                    }
                    _ => false,
                };
                if timed {
                    shader_combos.entry("SPRITESHEET".into()).or_insert(1);
                }
                shader::image_material_variant(&shader_combos, timed)
                    .map_err(|e| unsupported("shader", e.to_string()))?
            } else if shader_name == "genericimage2" {
                ensure!(
                    kind == MaterialKind::Image,
                    "Card image shader requires an image material"
                );
                shader::load_card_image(self.assets, shader_name, &shader_combos, &bound_slots)?
            } else if shader::is_legacy_image(shader_name) {
                ensure!(
                    kind == MaterialKind::Image,
                    "unsupported_scene: legacy image shader used by a non-image material"
                );
                for slot in &bound_slots {
                    let TextureBinding::Static(index) = textures[*slot] else {
                        return Err(unsupported(
                            "shader",
                            "legacy image requires static textures",
                        ));
                    };
                    if !matches!(self.textures[index].source_flags, 0 | 2) {
                        return Err(unsupported("shader", "legacy image texture sampling flags"));
                    }
                }
                if shader_name == "flag" {
                    let TextureBinding::Static(index) = textures[1] else {
                        return Err(unsupported("shader", "flag requires a normal texture"));
                    };
                    let format = i32::try_from(self.textures[index].source_format)?;
                    if shader_combos
                        .get("TEX1FORMAT")
                        .is_some_and(|value| *value != format)
                    {
                        return Err(unsupported("shader", "flag normal texture format mismatch"));
                    }
                    shader_combos.insert("TEX1FORMAT".to_owned(), format);
                }
                shader::load_legacy_image(self.assets, shader_name, &shader_combos, &bound_slots)
                    .map_err(|error| {
                        if error.downcast_ref::<lwe_scene::SceneAssetError>().is_some() {
                            error
                        } else {
                            unsupported("shader", format!("shader {shader_name}: {error:#}"))
                        }
                    })?
            } else if shader_name == "genericparticle" {
                ensure!(
                    kind == MaterialKind::Particle,
                    "unsupported_scene: genericparticle non-particle material"
                );
                let TextureBinding::Static(index) = textures[0] else {
                    bail!("Particle material requires a static texture atlas");
                };
                let texture = &self.textures[index];
                // RGBA upload retains red-channel masks. The original fragment
                // owns R8 alpha interpretation and atlas interpolation.
                shader_combos.insert(
                    "TEX0FORMAT".into(),
                    if texture.source_format == 9 { 9 } else { 0 },
                );
                if texture.animation.is_some() {
                    shader_combos.insert("SPRITESHEET".into(), 1);
                    shader_combos.insert("SPRITESHEETBLEND".into(), 1);
                }
                shader::load_particle(self.assets, &shader_combos, &bound_slots)?
            } else {
                ensure!(
                    kind != MaterialKind::Particle,
                    "unsupported_scene: unsupported particle shader"
                );
                ensure!(
                    matches!(
                        shader_name,
                        "passthrough"
                            | "effects/scroll"
                            | "effects/pulse"
                            | "effects/shake"
                            | "effects/shine_downsample2"
                            | "effects/shine_cast"
                            | "effects/shine_gaussian"
                            | "effects/shine_combine"
                            | "effects/waterripple"
                            | "effects/iris"
                    ) || shader::is_bounded_project_effect(shader_name),
                    "unsupported_scene: unvalidated Scene shader {shader_name}"
                );
                shader::load(self.assets, shader_name, &shader_combos, &bound_slots).map_err(
                    |error| {
                        if error.downcast_ref::<lwe_scene::SceneAssetError>().is_some() {
                            error
                        } else {
                            unsupported("shader", format!("shader {shader_name}: {error:#}"))
                        }
                    },
                )?
            };
            let mut constants = BTreeMap::new();
            let overrides = override_pass
                .get("constantshadervalues")
                .map(|value| value.as_object().context("Invalid shader constants"))
                .transpose()?;
            let defaults = pass
                .get("constantshadervalues")
                .map(|value| value.as_object().context("Invalid shader constants"))
                .transpose()?;
            let mut user_values = BTreeMap::new();
            for mapping in [
                pass.get("usershadervalues"),
                override_pass.get("usershadervalues"),
            ]
            .into_iter()
            .flatten()
            {
                ensure!(
                    kind != MaterialKind::Particle,
                    "unsupported_scene: user shader mappings outside image/effect materials"
                );
                let mapping = mapping.as_object().context("Invalid user shader mapping")?;
                let mut targets = BTreeSet::new();
                for (property, target) in mapping {
                    let target = target
                        .as_str()
                        .filter(|target| !target.is_empty() && target.len() <= 256)
                        .context("Invalid user shader mapping target")?;
                    ensure!(
                        targets.insert(target),
                        "Ambiguous user shader mapping target"
                    );
                    user_values.insert(target.to_owned(), self.properties.shader_value(property)?);
                }
            }
            let mut consumed = BTreeSet::new();
            for uniform in &shader.uniforms {
                if shader::is_audio_spectrum(uniform) {
                    continue;
                }
                if uniform.array {
                    return Err(unsupported(
                        "shader",
                        format!("uniform array {}", uniform.name),
                    ));
                }
                if uniform.kind == "sampler2D" {
                    let slot = uniform
                        .name
                        .strip_prefix("g_Texture")
                        .and_then(|slot| slot.parse::<usize>().ok())
                        .context("Unsupported Scene sampler name")?;
                    ensure!(slot < TEXTURE_SLOTS, "Scene sampler slot exceeds limit");
                    if !bound_slots.contains(&slot)
                        && let Some(name) = &uniform.texture_default
                    {
                        textures[slot] = self.texture(name, false)?;
                    }
                    if matches!(textures[slot], TextureBinding::Black) {
                        return Err(unsupported(
                            "shader",
                            format!("sampler {} has no bound texture", uniform.name),
                        ));
                    }
                    continue;
                }
                if let Some(material_key) = &uniform.material {
                    let value = user_values
                        .get(material_key)
                        .or_else(|| {
                            overrides
                                .and_then(|values| values.get(material_key))
                                .or_else(|| defaults.and_then(|values| values.get(material_key)))
                        })
                        .or(uniform.default.as_ref());
                    if let Some(value) = value {
                        let values = numbers(value)?;
                        let count = match uniform.kind.as_str() {
                            "float" => 1,
                            "vec2" => 2,
                            "vec3" => 3,
                            "vec4" => 4,
                            other => {
                                return Err(unsupported(
                                    "shader",
                                    format!("material uniform type {other}"),
                                ));
                            }
                        };
                        ensure!(
                            values.len() == count,
                            "Scene uniform {} requires {count} components",
                            uniform.name
                        );
                        if count == 1
                            && shader::is_bounded_project_effect(shader_name)
                            && let Some([minimum, maximum]) = uniform.material_range
                        {
                            ensure!(
                                (minimum..=maximum).contains(&values[0]),
                                "Project effect scalar exceeds its declared bounds"
                            );
                        }
                        constants.insert(uniform.name.clone(), values);
                    } else {
                        return Err(unsupported(
                            "shader",
                            format!("material uniform {} has no value or default", uniform.name),
                        ));
                    }
                    consumed.insert(material_key.clone());
                } else {
                    let expected = shader::runtime_uniform_kind(&uniform.name);
                    if expected != Some(uniform.kind.as_str()) {
                        return Err(unsupported(
                            "shader",
                            format!(
                                "unbound or invalid runtime uniform {} ({})",
                                uniform.name, uniform.kind
                            ),
                        ));
                    }
                }
            }
            for values in [overrides, defaults].into_iter().flatten() {
                for (name, value) in values {
                    if !consumed.contains(name) {
                        if shader::legacy_constant_is_inert(shader_name, name) {
                            ensure!(
                                numbers(value)?.len() == 1,
                                "Invalid inert legacy scalar constant"
                            );
                            continue;
                        }
                        return Err(unsupported(
                            "shader",
                            format!("unbound shader constant {name} in {path}"),
                        ));
                    }
                }
            }
            if user_values.keys().any(|name| !consumed.contains(name)) {
                return Err(unsupported(
                    "property_binding",
                    "unbound user shader mapping target",
                ));
            }
            if shader_name == "effects/shake"
                && constants
                    .get("g_Bounds")
                    .is_some_and(|bounds| bounds.len() != 2 || bounds[1] <= bounds[0])
            {
                bail!("Invalid shake bounds: maximum must exceed minimum");
            }
            if constants
                .get("u_pointerSpeed")
                .is_some_and(|value| value.as_slice() != [0.0])
            {
                return Err(unsupported(
                    "shader",
                    "pointer-driven material needs a pointer input host",
                ));
            }
            for name in ["g_AudioFrequencyMin", "g_AudioFrequencyMax"] {
                if let Some(value) = constants.get(name) {
                    ensure!(
                        value.len() == 1
                            && (0.0..=15.0).contains(&value[0])
                            && value[0].fract() == 0.0,
                        "Invalid audio spectrum band index"
                    );
                }
            }
            if let (Some(minimum), Some(maximum)) = (
                constants.get("g_AudioFrequencyMin"),
                constants.get("g_AudioFrequencyMax"),
            ) {
                ensure!(
                    minimum[0] <= maximum[0],
                    "Reversed audio spectrum band range"
                );
            }
            result.push(PreparedPass {
                label: shader_name.to_owned(),
                kind: PreparedPassKind::Render,
                shader: Some(shader),
                textures,
                constants,
                target: target.clone(),
                blend: blend(pass.get("blending"))?,
            });
        }
        Ok(result)
    }

    fn effects(&mut self, object: &Value) -> Result<Vec<PreparedStage>> {
        let Some(effects) = object.get("effects") else {
            return Ok(Vec::new());
        };
        let effects = effects.as_array().context("Invalid image effects")?;
        ensure!(effects.len() <= MAX_PASSES, "Too many image effects");
        let mut result = Vec::new();
        for effect in effects {
            if !boolean(effect, "visible", true)? {
                continue;
            }
            let file = effect["file"]
                .as_str()
                .context("Effect has no resource file")?;
            let definition = json(self.assets, file, self.properties)?;
            if has_script(&definition) || has_script(effect) {
                return Err(unsupported("scenescript", format!("effect {file}")));
            }
            let mut targets = BTreeMap::new();
            let mut persistent_targets = BTreeSet::new();
            if let Some(fbos) = definition.get("fbos") {
                for fbo in fbos.as_array().context("Invalid effect render targets")? {
                    let fbo_object = fbo
                        .as_object()
                        .context("Invalid effect target declaration")?;
                    ensure!(
                        fbo_object.keys().all(|key| matches!(
                            key.as_str(),
                            "name" | "scale" | "format" | "unique"
                        )),
                        "Unsupported effect target metadata"
                    );
                    let name = fbo["name"]
                        .as_str()
                        .filter(|s| !s.is_empty() && s.len() <= 256)
                        .context("Render target has no bounded name")?;
                    let scale = fbo
                        .get("scale")
                        .map(|v| v.as_f64().context("Invalid target scale"))
                        .transpose()?
                        .unwrap_or(1.0) as f32;
                    ensure!(
                        scale.is_finite() && (1.0..=16.0).contains(&scale),
                        "Invalid render target scale"
                    );
                    if fbo
                        .get("format")
                        .map(|v| v.as_str().context("Invalid render target format"))
                        .transpose()?
                        .unwrap_or("rgba_backbuffer")
                        != "rgba_backbuffer"
                    {
                        return Err(unsupported("shader", "effect render target format"));
                    }
                    if boolean(fbo, "unique", false)? {
                        persistent_targets.insert(name.to_owned());
                    }
                    ensure!(
                        targets.insert(name.to_owned(), scale).is_none(),
                        "Duplicate effect render target"
                    );
                }
            }
            ensure!(targets.len() <= 8, "Too many effect render targets");
            let overrides = effect
                .get("passes")
                .map(|v| v.as_array().context("Invalid effect overrides"))
                .transpose()?;
            let definitions = definition["passes"]
                .as_array()
                .context("Effect has no passes")?;
            let render_count = definitions
                .iter()
                .filter(|pass| pass.get("command").is_none())
                .count();
            ensure!(
                overrides.is_none_or(|v| v.len() <= render_count),
                "Effect overrides reference unknown render passes"
            );
            let mut passes = Vec::new();
            let mut written = BTreeSet::new();
            let mut render_index = 0;
            for pass in definitions {
                let target = pass
                    .get("target")
                    .map(|v| {
                        v.as_str()
                            .context("Invalid effect target")
                            .map(str::to_owned)
                    })
                    .transpose()?;
                if let Some(target) = &target {
                    ensure!(
                        targets.contains_key(target),
                        "Unknown effect render target {target}"
                    );
                }
                let resolve = |name: &str| -> Result<TextureBinding> {
                    if name == "previous" {
                        return Ok(TextureBinding::Input);
                    }
                    ensure!(
                        target.as_deref() != Some(name),
                        "Effect samples its own render target {name}"
                    );
                    if persistent_targets.contains(name) {
                        return Ok(TextureBinding::History(name.to_owned()));
                    }
                    ensure!(
                        written.contains(name),
                        "Effect reads unwritten target {name}"
                    );
                    Ok(TextureBinding::Named(name.to_owned()))
                };
                if let Some(command) = pass.get("command") {
                    ensure!(
                        command.as_str() == Some("copy"),
                        "Unsupported effect command"
                    );
                    let object = pass.as_object().context("Invalid copy command")?;
                    ensure!(
                        object
                            .keys()
                            .all(|key| matches!(key.as_str(), "command" | "source" | "target")),
                        "Unsupported copy command field"
                    );
                    let source = resolve(
                        pass["source"]
                            .as_str()
                            .context("Effect copy has no source")?,
                    )?;
                    let target = target.context("Effect copy has no target")?;
                    self.pass_count += 1;
                    ensure!(
                        self.pass_count <= self.pass_limit,
                        "Scene exceeds command/pass budget"
                    );
                    passes.push(PreparedPass {
                        label: "copy".into(),
                        kind: PreparedPassKind::Copy { source },
                        shader: None,
                        textures: Vec::new(),
                        constants: BTreeMap::new(),
                        target: Some(target.clone()),
                        blend: Blend::Replace,
                    });
                    written.insert(target);
                    continue;
                }
                let material = pass["material"]
                    .as_str()
                    .context("Effect pass has no material")?;
                let mut bindings = BTreeMap::new();
                if let Some(bind) = pass.get("bind") {
                    for bind in bind.as_array().context("Invalid effect pass bindings")? {
                        let slot = usize::try_from(
                            bind["index"]
                                .as_u64()
                                .context("Invalid effect texture slot")?,
                        )?;
                        ensure!(slot < TEXTURE_SLOTS, "Effect texture slot exceeds limit");
                        let binding = resolve(
                            bind["name"]
                                .as_str()
                                .context("Invalid effect texture binding")?,
                        )?;
                        ensure!(
                            bindings.insert(slot, binding).is_none(),
                            "Duplicate effect texture slot"
                        );
                    }
                }
                let override_pass = overrides
                    .and_then(|v| v.get(render_index))
                    .unwrap_or(&Value::Null);
                passes.extend(self.material(
                    material,
                    override_pass,
                    MaterialKind::Effect,
                    target.clone(),
                    &bindings,
                )?);
                render_index += 1;
                if let Some(target) = target {
                    written.insert(target);
                }
            }
            ensure!(
                passes.last().is_some_and(
                    |p| matches!(p.kind, PreparedPassKind::Render) && p.target.is_none()
                ),
                "Effect must produce a final render output"
            );
            result.push(PreparedStage {
                targets,
                persistent_targets,
                passes,
            });
        }
        Ok(result)
    }
}

impl PreparedScene {
    pub fn prepare(project_dir: &Path, assets_root: Option<&Path>) -> Result<Self> {
        Self::prepare_with_properties(project_dir, assets_root, &BTreeMap::new())
    }

    pub fn property_definitions(
        project_dir: &Path,
        assets_root: Option<&Path>,
    ) -> Result<Vec<PropertyDefinition>> {
        let assets = SceneAssets::open(project_dir, assets_root)?;
        let project: Value = serde_json::from_slice(&assets.read("project.json", MAX_JSON)?)
            .context("Invalid Scene project metadata")?;
        ensure!(
            project["type"]
                .as_str()
                .is_some_and(|kind| kind.eq_ignore_ascii_case("scene")),
            "Project is not a Scene wallpaper"
        );
        property_definitions(&project)
    }

    pub fn prepare_with_properties(
        project_dir: &Path,
        assets_root: Option<&Path>,
        overrides: &BTreeMap<String, Value>,
    ) -> Result<Self> {
        Self::prepare_with_resource_choices(project_dir, assets_root, overrides, &BTreeMap::new())
    }

    pub fn prepare_with_resource_choices(
        project_dir: &Path,
        assets_root: Option<&Path>,
        overrides: &BTreeMap<String, Value>,
        choices: &SceneResourceChoices,
    ) -> Result<Self> {
        let assets = SceneAssets::open(project_dir, assets_root)?;
        let mut project: Value = serde_json::from_slice(&assets.read("project.json", MAX_JSON)?)
            .context("Invalid Scene project metadata")?;
        ensure!(
            project["type"]
                .as_str()
                .is_some_and(|kind| kind.eq_ignore_ascii_case("scene")),
            "Project is not a Scene wallpaper"
        );
        if !overrides.is_empty() {
            apply_property_overrides(&mut project, overrides)?;
        }
        let scene_file = project["file"]
            .as_str()
            .context("Scene project has no entry resource")?;
        let texture_properties: BTreeSet<_> = property_definitions(&project)?
            .into_iter()
            .filter(|property| property.kind == PropertyKind::Texture)
            .map(|property| property.name)
            .collect();
        ensure!(
            choices.keys().all(|name| texture_properties.contains(name)),
            "Resource choice does not name a declared texture control"
        );
        let properties = ProjectProperties::from_project(&project)?;
        let mut scene: Value = serde_json::from_slice(&assets.read(scene_file, MAX_JSON)?)
            .context("Invalid Scene entry JSON")?;
        let mut bindings = extract_runtime_bindings(&mut scene, &project, &properties)?;
        properties.resolve_scene_document(&mut scene)?;
        if has_script(&scene) {
            return Err(unsupported("scenescript", "scene contains script bindings"));
        }
        let general = &scene["general"];
        for (key, code) in [
            ("hdr", "hdr"),
            ("cameraparallax", "camera"),
            ("camerashake", "camera"),
            ("fogdistance", "shader"),
            ("fogheight", "shader"),
        ] {
            if boolean(general, key, false)? {
                return Err(unsupported(code, key));
            }
        }
        if general
            .get("zoom")
            .is_some_and(|value| value.as_f64() != Some(1.0))
        {
            return Err(unsupported("camera", "camera zoom"));
        }
        let projection = &general["orthogonalprojection"];
        if boolean(projection, "auto", false)? {
            return Err(unsupported("camera", "automatic orthographic projection"));
        }
        let width = projection["width"]
            .as_f64()
            .ok_or_else(|| unsupported("camera", "perspective projection"))?
            as f32;
        let height = projection["height"]
            .as_f64()
            .ok_or_else(|| unsupported("camera", "perspective projection"))?
            as f32;
        ensure!(
            [width, height]
                .iter()
                .all(|value| value.is_finite() && (1.0..=16384.0).contains(value)),
            "Invalid orthographic Scene dimensions"
        );
        if let Some(camera) = scene.get("camera") {
            if let Some(paths) = camera.get("paths")
                && !paths.as_array().is_some_and(Vec::is_empty)
            {
                return Err(unsupported("camera", "camera paths are not supported"));
            }
            let eye = vector(camera, "eye", [0.0, 0.0, 1.0])?;
            let center = vector(camera, "center", [0.0; 3])?;
            let up = vector(camera, "up", [0.0, 1.0, 0.0])?;
            // The inspected native runtime loader resets eye/center/up for a
            // fixed orthographic scene without camera paths. Parallel saved XY
            // therefore does not translate runtime layers (it is not a lookAt
            // input here). Keep tilted/path/automatic cases outside this subset.
            if eye[..2] != center[..2] {
                return Err(unsupported(
                    "camera",
                    "tilted orthographic camera is not supported",
                ));
            }
            if eye[2] <= center[2] || up != [0.0, 1.0, 0.0] {
                return Err(unsupported(
                    "camera",
                    "tilted or reversed orthographic camera",
                ));
            }
        }
        let clear = vector(general, "clearcolor", [0.0; 3])?;
        let objects = scene["objects"]
            .as_array()
            .context("Scene has no objects")?;
        ensure!(
            !objects.is_empty() && objects.len() <= MAX_LAYERS,
            "Scene layer count exceeds supported range"
        );
        for binding in &mut bindings {
            binding.input = objects[binding.layer - 1][&binding.property].clone();
            if matches!(
                binding.property.as_str(),
                "origin" | "scale" | "angles" | "color"
            ) {
                let values = numbers(&binding.input)?;
                let vector = match values.as_slice() {
                    [scalar] => [*scalar; 3],
                    [x, y, z] => [*x, *y, *z],
                    _ => bail!("Invalid vector script binding input"),
                };
                binding.input = serde_json::json!(vector);
            }
        }
        let mut controlled_handles: BTreeSet<_> =
            bindings.iter().map(|binding| binding.layer).collect();
        let hierarchy = hierarchy::prepare(objects, &controlled_handles, [width, height])?;
        let scripts = if bindings.is_empty() {
            None
        } else {
            let states = objects
                .iter()
                .enumerate()
                .map(|(index, object)| {
                    let alpha = object
                        .get("alpha")
                        .map(numbers)
                        .transpose()?
                        .map(|v| v[0])
                        .unwrap_or(1.0);
                    Ok(dynamic_script::LayerState {
                        handle: index + 1,
                        origin: vector(object, "origin", [width / 2.0, height / 2.0, 0.0])?,
                        scale: vector(object, "scale", [1.0; 3])?,
                        angles: vector(object, "angles", [0.0; 3])?,
                        color: vector(object, "color", [1.0; 3])?,
                        alpha,
                        visible: boolean(object, "visible", true)?,
                        text: object
                            .get("text")
                            .and_then(Value::as_str)
                            .map(str::to_owned),
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            let snapshot = property_definitions(&project)?
                .into_iter()
                .filter(|property| {
                    !matches!(
                        property.kind,
                        PropertyKind::Texture | PropertyKind::TextInput | PropertyKind::TextDisplay
                    )
                })
                .map(|property| Ok((property.name, serde_json::to_value(property.value)?)))
                .collect::<Result<serde_json::Map<_, _>>>()?;
            Some(Arc::new(dynamic_script::PreparedScripts::prepare(
                bindings,
                states,
                Value::Object(snapshot),
            )?))
        };
        let initial_placements = hierarchy.resolve(
            scripts
                .as_ref()
                .map_or(&[], |scripts| scripts.initial_states()),
        )?;
        let mut hierarchy_layers = Vec::new();
        let pass_limit = if scripts.is_some() { 128 } else { MAX_PASSES };
        let mut authored_objects = objects.clone();
        let mut handles: Vec<_> = (1..=authored_objects.len()).collect();
        if let Some(scripts) = &scripts {
            for created in scripts.created_layers() {
                let state = &created.state;
                controlled_handles.insert(state.handle);
                handles.push(state.handle);
                authored_objects.push(serde_json::json!({ "image": created.path, "origin": state.origin, "scale": state.scale, "angles": state.angles, "color": state.color, "alpha": state.alpha, "visible": state.visible }));
            }
            ensure!(
                authored_objects.len() <= 128,
                "Dynamic Scene layer count exceeds its budget"
            );
        }
        let mut script_layers = BTreeMap::new();
        let mut preparation = Preparation {
            assets: &assets,
            properties: &properties,
            choices,
            texture_properties,
            inspection: lwe_scene::TextureInspectionBudget::default(),
            active_layer_id: None,
            textures: Vec::new(),
            texture_indices: BTreeMap::new(),
            rgba_bytes: 0,
            pass_count: 0,
            pass_limit,
        };
        let mut layers = Vec::new();
        let mut particles = Vec::new();
        let mut draw_order = Vec::new();
        let mut particle_vertices = 0_usize;
        let mut particle_work = 0_usize;
        let bloom = postprocess::PreparedBloom::prepare(&assets, general, &mut |path| {
            json(&assets, path, &properties)
        })?
        .map(Arc::new);
        let mut effect_count = 0;
        for (index, (object, handle)) in authored_objects.iter().zip(handles).enumerate() {
            let hierarchy_member = hierarchy.contains(index);
            let placement = initial_placements.get(index).and_then(Option::as_ref);
            preparation.active_layer_id = object
                .get("id")
                .map(|id| id.as_u64().context("Invalid layer identity"))
                .transpose()?;
            let script_state = if controlled_handles.contains(&handle) {
                scripts.as_ref().and_then(|scripts| {
                    scripts
                        .initial_states()
                        .iter()
                        .find(|state| state.handle == handle)
                })
            } else {
                None
            };
            // Keep revealable images and parents validated even when a branch
            // starts hidden. A locally hidden, uncontrolled leaf cannot reveal
            // in this generation and keeps the existing resource skip.
            if (!boolean(object, "visible", true)?
                && script_state.is_none()
                && !hierarchy.has_children(index))
                || object.get("sound").is_some()
                || hierarchy.is_transform_only(index)
            {
                continue;
            }
            if object.get("particle").is_some() {
                let simulation = particles::PreparedParticles::prepare(object, &mut |path| {
                    json(&assets, path, &properties)
                })?;
                simulation.validate_runtime()?;
                let vertex_capacity = simulation.max_vertices()?;
                particle_vertices = particle_vertices
                    .checked_add(vertex_capacity)
                    .context("Scene particle vertex total overflow")?;
                particle_work = particle_work
                    .checked_add(simulation.max_work()?)
                    .context("Scene particle work total overflow")?;
                ensure!(
                    particle_vertices <= 1_000_000 && particle_work <= 1_000_000,
                    "Scene particles exceed combined work or vertex budget"
                );
                let materials = simulation
                    .material_paths()
                    .map(|path| {
                        let mut passes = preparation.material(
                            path,
                            &Value::Null,
                            MaterialKind::Particle,
                            None,
                            &BTreeMap::new(),
                        )?;
                        Ok(passes.remove(0))
                    })
                    .collect::<Result<Vec<_>>>()?;
                let aspects = materials
                    .iter()
                    .map(|pass| {
                        let TextureBinding::Static(texture) = pass.textures[0] else {
                            bail!("Particle material has no static primary texture")
                        };
                        let texture = &preparation.textures[texture];
                        Ok(texture.height as f32 / texture.width as f32)
                    })
                    .collect::<Result<Vec<_>>>()?;
                let animation_bytes = materials.iter().try_fold(
                    materials.len() * std::mem::size_of::<Option<lwe_scene::AnimationMetadata>>(),
                    |bytes, pass| {
                        let frames = match pass.textures[0] {
                            TextureBinding::Static(index) => preparation.textures[index]
                                .animation
                                .as_ref()
                                .map_or(0, |a| {
                                    a.frames.capacity()
                                        * std::mem::size_of::<lwe_scene::AnimationFrame>()
                                }),
                            _ => 0,
                        };
                        bytes
                            .checked_add(frames)
                            .context("Particle animation allocation overflow")
                    },
                )?;
                let mut animation_reservation =
                    scene_texture_memory_budget().reserve_bytes(animation_bytes)?;
                let animations: Vec<_> = materials
                    .iter()
                    .map(|pass| match pass.textures[0] {
                        TextureBinding::Static(index) => {
                            preparation.textures[index].animation.clone()
                        }
                        _ => None,
                    })
                    .collect();
                let animation_bytes = animations.iter().try_fold(
                    animations.capacity()
                        * std::mem::size_of::<Option<lwe_scene::AnimationMetadata>>(),
                    |bytes, a| {
                        bytes
                            .checked_add(a.as_ref().map_or(0, |a| {
                                a.frames.capacity()
                                    * std::mem::size_of::<lwe_scene::AnimationFrame>()
                            }))
                            .context("Particle animation allocation overflow")
                    },
                )?;
                animation_reservation.resize(animation_bytes)?;
                draw_order.push(DrawObject::Particles(particles.len()));
                particles.push(PreparedParticleLayer {
                    simulation,
                    materials,
                    aspects,
                    animations,
                    vertex_capacity,
                    animation_reservation,
                });
                continue;
            }
            let is_text = object.get("text").is_some();
            let image = object.get("image").and_then(Value::as_str);
            if image.is_none() && !is_text {
                let feature = if object.get("model").is_some() {
                    "model"
                } else {
                    "unknown_object"
                };
                return Err(unsupported(
                    feature,
                    "object requires a modeled image or text surface",
                ));
            }
            ensure!(
                !(is_text && image.is_some()),
                "Ambiguous image and text object"
            );
            let perspective = boolean(object, "perspective", false)?;
            for key in ["puppet", "animation", "mesh"] {
                if object.get(key).is_some() {
                    return Err(unsupported("puppet", format!("image {key}")));
                }
            }
            let prepared_text = if is_text {
                Some(prepare_text(&assets, object)?)
            } else {
                None
            };
            let model = if let Some(text) = &prepared_text {
                serde_json::json!({"width":text.width,"height":text.height})
            } else {
                json(&assets, image.expect("image checked"), &properties)?
            };
            if boolean(&model, "perspective", false)? {
                return Err(unsupported("camera", "perspective image model"));
            }
            for key in ["puppet", "animation", "mesh"] {
                if model.get(key).is_some() {
                    return Err(unsupported("puppet", format!("image model {key}")));
                }
            }
            if model.get("parent").is_some() || model["options"].get("parent").is_some() {
                return Err(unsupported(
                    "hierarchy",
                    "image model parent bindings are not supported",
                ));
            }
            if has_script(&model) {
                return Err(unsupported("scenescript", format!("image model {image:?}")));
            }
            let fullscreen = boolean(&model, "fullscreen", false)?;
            let passthrough = boolean(&model, "passthrough", false)?;
            if hierarchy_member && (fullscreen || passthrough) {
                return Err(unsupported("hierarchy", "Fullscreen Scene hierarchy"));
            }
            ensure!(
                fullscreen == passthrough,
                "Unsupported partial fullscreen model"
            );
            let instance = object.get("instance").unwrap_or(&Value::Null);
            validate_instance(instance)?;
            let base_passes = if let Some(text) = &prepared_text {
                ensure!(
                    instance.is_null(),
                    "Text material instances are unsupported"
                );
                let value = script_state
                    .and_then(|s| s.text.as_deref())
                    .or_else(|| object["text"].as_str())
                    .context("Text object has no literal input")?;
                let frame = text.rasterize(value)?;
                let index = preparation.textures.len();
                preparation.textures.push(PreparedTexture {
                    fingerprint: frame.fingerprint(),
                    width: frame.width(),
                    height: frame.height(),
                    image_width: frame.width(),
                    image_height: frame.height(),
                    source_format: 0,
                    source_flags: 2,
                    rgba: Vec::new(),
                    animation: None,
                    timed: None,
                    static_source: None,
                    image: None,
                    text_frame: Some(frame),
                    _rgba_reservation: None,
                });
                preparation.pass_count += 1;
                ensure!(
                    preparation.pass_count <= preparation.pass_limit,
                    "Scene exceeds render pass budget"
                );
                let mut textures = vec![TextureBinding::Black; TEXTURE_SLOTS];
                textures[0] = TextureBinding::Static(index);
                textures[1] = TextureBinding::Input;
                vec![PreparedPass {
                    label: "LWE text material".into(),
                    kind: PreparedPassKind::Render,
                    shader: Some(shader::text_material()?),
                    textures,
                    constants: BTreeMap::new(),
                    target: None,
                    blend: Blend::Alpha,
                }]
            } else {
                let material = model["material"]
                    .as_str()
                    .context("Image model has no material")?;
                preparation.material(
                    material,
                    instance,
                    MaterialKind::Image,
                    None,
                    &BTreeMap::new(),
                )?
            };
            let effects = preparation.effects(object)?;
            effect_count += effects.len();
            let mut stages = vec![PreparedStage {
                targets: BTreeMap::new(),
                persistent_targets: BTreeSet::new(),
                passes: base_passes,
            }];
            stages.extend(effects);
            let model_size = match (model.get("width"), model.get("height")) {
                (Some(width), Some(height)) => {
                    let width = numbers(width)?;
                    let height = numbers(height)?;
                    ensure!(
                        width.len() == 1 && height.len() == 1,
                        "Invalid image model dimensions"
                    );
                    [width[0], height[0]]
                }
                (None, None) if boolean(&model, "autosize", false)? && !fullscreen => {
                    let TextureBinding::Static(index) = stages[0].passes[0].textures[0] else {
                        bail!("Autosized image requires a static primary texture");
                    };
                    let texture = &preparation.textures[index];
                    [texture.image_width as f32, texture.image_height as f32]
                }
                (None, None) => [width, height],
                _ => bail!("Incomplete image model dimensions"),
            };
            let size = vector(object, "size", model_size)?;
            if fullscreen {
                ensure!(
                    size == [width, height],
                    "Fullscreen model must retain canvas dimensions"
                );
            }
            ensure!(
                size.iter()
                    .all(|value| value.is_finite() && (1.0..=16384.0).contains(value)),
                "Invalid image layer size"
            );
            if placement
                .is_some_and(|resolved| !resolved.world.fits_projected_quad(size, [width, height]))
            {
                return Err(unsupported(
                    "hierarchy",
                    "Scene hierarchy exceeds projected numeric range",
                ));
            }
            let origin = vector(object, "origin", [width / 2.0, height / 2.0, 0.0])?;
            let angles = vector(object, "angles", [0.0; 3])?;
            // Under this fixed orthographic camera, a coplanar image has the
            // same affine XY projection. Depth or out-of-plane rotation still
            // requires a separate 3D camera/model path.
            if perspective && (origin[2] != 0.0 || angles[0] != 0.0 || angles[1] != 0.0) {
                return Err(unsupported(
                    "camera",
                    "perspective image requires a coplanar orthographic transform",
                ));
            }
            if !is_text && (angles[0] != 0.0 || angles[1] != 0.0) {
                return Err(unsupported("model", "3D image rotation"));
            }
            let scale = vector(object, "scale", [1.0; 3])?;
            ensure!(
                script_state.is_some() || (scale[0] != 0.0 && scale[1] != 0.0),
                "Invalid zero image scale"
            );
            let tint = vector(object, "color", [1.0; 3])?;
            let alpha = object
                .get("alpha")
                .map(numbers)
                .transpose()?
                .map(|values| values[0])
                .unwrap_or(1.0);
            ensure!((0.0..=1.0).contains(&alpha), "Invalid image alpha");
            let color_mode = object
                .get("colorBlendMode")
                .map(|value| value.as_u64().context("Invalid image color blend mode"))
                .transpose()?
                .unwrap_or(0);
            if hierarchy_member
                && (color_mode != 0
                    || stages.iter().flat_map(|stage| &stage.passes).any(|pass| {
                        pass.textures
                            .iter()
                            .any(|texture| matches!(texture, TextureBinding::FullFrame))
                    }))
            {
                return Err(unsupported(
                    "hierarchy",
                    "Scene hierarchy with full-frame input or composite",
                ));
            }
            let composite = if color_mode == 0 {
                None
            } else {
                ensure!(
                    matches!(color_mode, 22 | 28 | 30 | 31),
                    "Unsupported image color blend mode {color_mode}"
                );
                Some(shader::load_image_composite(&assets, color_mode as i32)?)
            };
            let layer_blend = if fullscreen || composite.is_some() {
                Blend::Replace
            } else {
                stages[0].passes[0].blend
            };
            draw_order.push(DrawObject::Image(layers.len()));
            if script_state.is_some() {
                script_layers.insert(handle, layers.len());
            }
            if hierarchy_member {
                hierarchy_layers.push((index, layers.len()));
            }
            layers.push(PreparedLayer {
                world_2d: placement
                    .filter(|_| hierarchy.is_child(index))
                    .map(|resolved| resolved.world),
                size,
                origin: [origin[0], origin[1]],
                scale: [scale[0], scale[1]],
                angle: angles[2],
                origin_z: origin[2],
                angles,
                tint: [tint[0], tint[1], tint[2], alpha],
                blend: layer_blend,
                composite,
                stages,
                clamp_uvs: boolean(object, "clampuvs", false)?,
                crop_offset: vector(
                    object,
                    "cropoffset",
                    vector(&model, "cropoffset", [0.0; 2])?,
                )?,
                no_padding: boolean(object, "nopadding", boolean(&model, "nopadding", false)?)?,
                text: prepared_text,
            });
        }
        ensure!(!draw_order.is_empty(), "Scene has no renderable layers");
        let audio = audio::prepare_audio(&assets, &scene)?;
        let pass_count = preparation.pass_count + usize::from(bloom.is_some()) * 4;
        ensure!(
            pass_count <= pass_limit,
            "Scene exceeds the render pass budget including bloom"
        );
        let legacy_animated = layers
            .iter()
            .flat_map(|layer| &layer.stages)
            .flat_map(|stage| &stage.passes)
            .any(|pass| {
                pass.shader.as_ref().is_some_and(|shader| {
                    shader::legacy_is_animated(&pass.label, shader, &pass.constants)
                })
            });
        let summary = SceneSummary {
            layer_count: draw_order.len(),
            effect_count,
            pass_count,
            animated: preparation.textures.iter().any(|t| t.timed.is_some())
                || scripts.is_some()
                || effect_count != 0
                || !particles.is_empty()
                || legacy_animated
                || layers
                    .iter()
                    .flat_map(|layer| &layer.stages)
                    .flat_map(|stage| &stage.passes)
                    .filter_map(|pass| pass.shader.as_ref())
                    .flat_map(|shader| &shader.uniforms)
                    .any(shader::is_audio_spectrum),
        };
        let prepared = Self {
            canvas: [width, height],
            clear: [
                clear[0],
                clear[1],
                clear[2],
                if boolean(general, "clearenabled", true)? {
                    1.0
                } else {
                    0.0
                },
            ],
            textures: preparation.textures,
            layers,
            audio,
            bloom,
            particles,
            draw_order,
            summary,
            scripts,
            script_layers,
            hierarchy: (!hierarchy_layers.is_empty()).then(|| {
                Arc::new(SceneHierarchy {
                    graph: hierarchy,
                    layers: hierarchy_layers,
                    initial: initial_placements,
                })
            }),
        };
        renderer::validate_gpu_budget(&prepared)?;
        Ok(prepared)
    }

    pub fn summary(&self) -> SceneSummary {
        self.summary
    }

    pub fn prepared_audio_bytes(&self) -> u64 {
        audio::prepared_audio_bytes(&self.audio)
    }

    /// Physical owners, not content hashes. Font geometry uses its documented
    /// conservative reservation because Fontdue exposes no actual capacity API.
    pub(crate) fn cpu_allocations(&self) -> impl Iterator<Item = (usize, u64)> + '_ {
        self.textures
            .iter()
            .map(|t| {
                if let Some(source) = &t.timed {
                    (
                        source.allocation_id(),
                        source.retained_capacity_bytes() as u64,
                    )
                } else if let Some(source) = &t.static_source {
                    (Arc::as_ptr(source) as usize, source.capacity_bytes() as u64)
                } else if let Some(image) = &t.image {
                    (image.allocation_id(), image.capacity_bytes() as u64)
                } else if let Some(frame) = &t.text_frame {
                    (frame.allocation_id(), frame.capacity_bytes() as u64)
                } else {
                    (
                        t.rgba.as_ptr() as usize,
                        t._rgba_reservation
                            .as_ref()
                            .map_or(t.rgba.capacity(), |r| r.bytes())
                            as u64,
                    )
                }
            })
            .chain(self.particles.iter().map(|p| {
                (
                    p.animations.as_ptr() as usize,
                    p.animation_reservation.bytes() as u64,
                )
            }))
            .chain(self.textures.iter().filter_map(|t| {
                t.static_source.as_ref().and(t.animation.as_ref()).map(|a| {
                    (
                        a.frames.as_ptr() as usize,
                        t._rgba_reservation.as_ref().map_or(0, |r| r.bytes()) as u64,
                    )
                })
            }))
            .chain(
                self.layers
                    .iter()
                    .filter_map(|l| l.text.as_ref().map(text::PreparedText::font_allocation)),
            )
    }

    #[cfg(test)]
    pub(crate) fn decoded_texture_bytes(&self) -> Result<u64> {
        let mut owners = BTreeMap::new();
        for (id, bytes) in self.cpu_allocations() {
            owners.entry(id).or_insert(bytes);
        }
        owners.values().try_fold(0_u64, |n, bytes| {
            n.checked_add(*bytes)
                .context("Scene CPU allocation total overflow")
        })
    }

    pub(crate) fn initialize_audio(&self) -> Result<SceneAudio> {
        SceneAudio::new(&self.audio)
    }

    pub(crate) fn audio_allocations(&self) -> impl Iterator<Item = (usize, u64)> + '_ {
        self.audio
            .iter()
            .map(|sound| (sound.clip_identity(), sound.payload_bytes()))
    }

    pub fn estimated_gpu_bytes_for_output(&self, width: i32, height: i32) -> Result<u64> {
        self.resource_plan_for_output(width, height)?.total(0)
    }

    pub(crate) fn resource_plan_for_output(
        &self,
        width: i32,
        height: i32,
    ) -> Result<shared_texture::ResourcePlan> {
        let mut plan = renderer::resource_plan(self)?;
        plan.private_bytes = plan
            .private_bytes
            .checked_add(
                self.bloom
                    .as_ref()
                    .map(|bloom| bloom.estimated_bytes(width, height))
                    .transpose()?
                    .unwrap_or_default(),
            )
            .context("Scene output allocation estimate overflow")?;
        plan.private_bytes = plan
            .private_bytes
            .checked_add(renderer::output_cache_bytes(width, height, 2)?)
            .context("Scene output cache estimate overflow")?;
        Ok(plan)
    }

    /// Logical texture/FBO payload; excludes driver and output back buffers.
    pub fn estimated_gpu_bytes(&self) -> Result<u64> {
        renderer::estimated_gpu_bytes(self)
    }

    /// Requires a current OpenGL context on the calling engine thread.
    pub fn initialize(&self) -> Result<SceneRenderer> {
        SceneRenderer::initialize(self)
    }

    pub(crate) fn initialize_shared(
        &self,
        pool: Rc<shared_texture::TexturePool>,
        worker_wake: Option<worker_wake::WorkerWake>,
    ) -> Result<SceneRenderer> {
        SceneRenderer::initialize_shared_with_wake(self, pool, worker_wake)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    include!("instance_material_tests.rs");
    include!("hierarchy_tests.rs");

    pub(super) struct Fixture(pub(super) PathBuf);
    impl Fixture {
        pub(super) fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let directory = std::env::temp_dir().join(format!(
                "lwe-scene-ir-{}-{stamp}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&directory).unwrap();
            let fixture = Self(directory);
            fixture.write("project.json", br#"{"type":"scene","file":"scene.json"}"#);
            fixture.scene(
                serde_json::json!([{"image":"models/image.json","size":"16 16","origin":"8 8 0"}]),
            );
            fixture.write(
                "models/image.json",
                br#"{"autosize":true,"material":"materials/image.json"}"#,
            );
            fixture.write("materials/image.json", br#"{"passes":[{"shader":"genericimage4","blending":"translucent","textures":["image"]}]}"#);
            let mut texture = b"TEXV0005\0TEXI0001\0".to_vec();
            for value in [0_u32, 0, 1, 1, 1, 1, 0] {
                texture.extend_from_slice(&value.to_le_bytes());
            }
            texture.extend_from_slice(b"TEXB0001\0");
            for value in [1_u32, 1, 1, 1, 4] {
                texture.extend_from_slice(&value.to_le_bytes());
            }
            texture.extend_from_slice(&[255, 0, 0, 255]);
            fixture.write("materials/image.tex", &texture);
            fixture
        }
        fn write(&self, path: &str, bytes: &[u8]) {
            let path = self.0.join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, bytes).unwrap();
        }
        pub(super) fn scene(&self, objects: Value) {
            let scene = serde_json::json!({"general":{"orthogonalprojection":{"width":16,"height":16}},"objects":objects});
            self.write("scene.json", &serde_json::to_vec(&scene).unwrap());
        }
        fn shake_shader(&self) {
            self.write("shaders/effects/shake.vert", b"uniform mat4 g_ModelViewProjectionMatrix;\nuniform vec2 g_Bounds; // {\"material\":\"bounds\",\"default\":\"0 1\"}\nattribute vec3 a_Position;\nvoid main(){gl_Position=mul(vec4(a_Position,1),g_ModelViewProjectionMatrix);}");
            self.write("shaders/effects/shake.frag", b"uniform sampler2D g_Texture0;\nvoid main(){gl_FragColor=texSample2D(g_Texture0,vec2(0));}");
            self.write(
                "materials/effects/shake.json",
                br#"{"passes":[{"shader":"effects/shake"}]}"#,
            );
        }

        fn legacy_shader(&self, name: &str, fragment: &str) {
            self.write(&format!("shaders/{name}.vert"), b"uniform mat4 g_ModelViewProjectionMatrix;\nattribute vec3 a_Position;\nattribute vec2 a_TexCoord;\nvarying vec2 v_TexCoord;\nvoid main(){v_TexCoord=a_TexCoord;gl_Position=mul(vec4(a_Position,1),g_ModelViewProjectionMatrix);}");
            self.write(&format!("shaders/{name}.frag"), fragment.as_bytes());
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    pub(super) fn timeline_alpha() -> Value {
        serde_json::json!({
            "value": 1,
            "script": "export function update(value){return value * 0.5;}",
            "animation": {
                "options": {"fps":4,"length":4,"mode":"single","wraploop":null},
                "c0": [{"frame":0,"value":0.25},{"frame":1,"value":0.75}]
            }
        })
    }

    #[test]
    fn prepares_mixed_alpha_and_rejects_unmodeled_property_timelines() {
        let fixture = Fixture::new();
        let mut alpha = timeline_alpha();
        fixture.scene(serde_json::json!([{"image":"models/image.json","alpha":alpha}]));
        let prepared = PreparedScene::prepare(&fixture.0, None).unwrap();
        assert!(prepared.summary().animated);
        assert_eq!(
            prepared.scripts.as_ref().unwrap().initial_states()[0].alpha,
            0.125
        );

        fixture.scene(serde_json::json!([{"image":"models/image.json","scale":alpha}]));
        let error = PreparedScene::prepare(&fixture.0, None).unwrap_err();
        assert_eq!(unsupported_feature_code(&error), Some("timeline"));

        alpha.as_object_mut().unwrap().remove("script");
        fixture.scene(serde_json::json!([{"image":"models/image.json","alpha":alpha}]));
        let error = PreparedScene::prepare(&fixture.0, None).unwrap_err();
        assert_eq!(unsupported_feature_code(&error), Some("timeline"));
    }

    #[test]
    fn dynamic_init_preloads_created_images_without_losing_layer_states() {
        let fixture = Fixture::new();
        let source = "let parts=[];let sound=engine.registerAudioBuffers(64);export function init(){parts.push(thisLayer);for(let i=1;i<4;i++){parts.push(thisScene.createLayer('models/image.json'));}}export function update(){for(let i=0;i<4;i++){let part=parts[i];part.origin=new Vec3(10+i*4,8,0);part.scale=new Vec3(1,sound.average[i]*2,1);}}";
        fixture.scene(serde_json::json!([{"image":"models/image.json","size":"4 4","origin":"8 8 0","scale":"0 0 0","visible":{"script":source,"value":true}}]));
        let prepared = PreparedScene::prepare(&fixture.0, None).unwrap();
        assert_eq!(prepared.summary().layer_count, 4);
        assert_eq!(prepared.summary().pass_count, 4);
        assert!(prepared.summary().animated);
        assert_eq!(prepared.textures.len(), 1);
        assert_eq!(prepared.script_layers.len(), 4);
        assert_eq!(prepared.layers[1].size, [1.0; 2]);
        let states = prepared.scripts.as_ref().unwrap().initial_states();
        assert_eq!(states[3].origin, [22.0, 8.0, 0.0]);
        assert_eq!(states[3].scale, [1.0, 0.0, 1.0]);
    }

    #[test]
    fn dynamic_init_does_not_admit_missing_or_unsupported_created_resources() {
        let fixture = Fixture::new();
        fixture.scene(serde_json::json!([{"image":"models/image.json","size":"4 4","visible":{"script":"export function init(){thisScene.createLayer('models/missing.json');}export function update(){}","value":true}}]));
        assert!(PreparedScene::prepare(&fixture.0, None).is_err());
        fixture.scene(serde_json::json!([{"image":"models/image.json","size":"4 4","visible":{"script":"export function init(){thisScene.createLayer('../private.json');}export function update(){}","value":true}}]));
        let error = PreparedScene::prepare(&fixture.0, None).unwrap_err();
        assert_eq!(unsupported_feature_code(&error), Some("scenescript"));
    }

    #[test]
    fn orthographic_perspective_images_require_a_coplanar_transform() {
        let fixture = Fixture::new();
        fixture.scene(serde_json::json!([{"image":"models/image.json","size":"4 4","perspective":true,"origin":"8 8 0"}]));
        assert!(PreparedScene::prepare(&fixture.0, None).is_ok());
        for origin in ["8 8 1", "8 8 -1"] {
            fixture.scene(serde_json::json!([{"image":"models/image.json","size":"4 4","perspective":true,"origin":origin}]));
            let error = PreparedScene::prepare(&fixture.0, None).unwrap_err();
            assert_eq!(unsupported_feature_code(&error), Some("camera"));
        }
    }

    #[test]
    fn prepares_authored_image_fixture_without_a_desktop_or_gl_context() {
        let fixture = Fixture::new();
        let prepared = PreparedScene::prepare(&fixture.0, None).unwrap();
        assert_eq!(
            prepared.summary(),
            SceneSummary {
                layer_count: 1,
                effect_count: 0,
                pass_count: 1,
                animated: false
            }
        );
        assert_eq!(prepared.textures[0].rgba(), [255, 0, 0, 255]);
        let debug = format!("{prepared:?}");
        assert!(!debug.contains("rgba"));
    }

    #[test]
    fn project_defaults_are_shared_by_scene_and_effect_resources() {
        let fixture = Fixture::new();
        fixture.write(
            "project.json",
            br#"{"type":"scene","file":"scene.json","general":{"properties":{
            "hidden":{"type":"bool","value":false},
            "opacity":{"type":"slider","value":0.25,"min":0,"max":1},
            "tint":{"type":"color","value":"0.1 0.2 0.3"},
            "scale":{"type":"slider","value":2,"min":1,"max":4}
        }}}"#,
        );
        fixture.shake_shader();
        fixture.write("effects/shake/effect.json", br#"{"fbos":[{"name":"unused","scale":{"user":"scale","value":1}}],"passes":[{"material":"materials/effects/shake.json"}]}"#);
        fixture.scene(serde_json::json!([
            {"image":"models/hidden-missing.json","visible":{"user":"hidden","value":true}},
            {"image":"models/image.json","alpha":{"user":"opacity","value":1},
             "color":{"user":"tint","value":"1 1 1"},
             "effects":[{"file":"effects/shake/effect.json"}]}
        ]));
        let prepared = PreparedScene::prepare(&fixture.0, None).unwrap();
        assert_eq!(prepared.summary().layer_count, 1);
        assert_eq!(prepared.layers[0].tint, [0.1, 0.2, 0.3, 0.25]);
        assert_eq!(prepared.layers[0].stages[1].targets["unused"], 2.0);
    }

    #[test]
    fn authored_visibility_skips_hidden_objects_and_regates_enabled_ones() {
        let fixture = Fixture::new();
        fixture.write(
            "project.json",
            br#"{"type":"scene","file":"scene.json","general":{"properties":{
            "hdr":{"type":"bool","value":true},
            "visible":{"type":"bool","value":false}
        }}}"#,
        );
        fixture.write("scene.json", br#"{"general":{"hdr":{"user":"hdr","value":false},"orthogonalprojection":{"width":16,"height":16}},"objects":[{"image":"models/image.json"}]}"#);
        let error = PreparedScene::prepare(&fixture.0, None).unwrap_err();
        assert_eq!(unsupported_feature_code(&error), Some("hdr"));
        assert!(error.to_string().contains("hdr"));
        fixture.scene(serde_json::json!([
            {"particle":"particles/unsupported.json","visible":{"user":"visible","value":true}},
            {"image":"models/image.json"}
        ]));
        let prepared = PreparedScene::prepare(&fixture.0, None).unwrap();
        assert_eq!(prepared.summary().layer_count, 1);
        fixture.scene(serde_json::json!([
            {"particle":"particles/unsupported.json","visible":true},
            {"image":"models/image.json"}
        ]));
        fixture.write(
            "particles/unsupported.json",
            br#"{"unknown_behavior":true}"#,
        );
        let error = PreparedScene::prepare(&fixture.0, None).unwrap_err();
        assert_eq!(unsupported_feature_code(&error), Some("particle"));
    }

    #[test]
    fn scene_image_rotation_preserves_radians() {
        let fixture = Fixture::new();
        fixture.scene(serde_json::json!([{
            "image":"models/image.json", "angles":"0 0 1.5707963267948966"
        }]));
        let prepared = PreparedScene::prepare(&fixture.0, None).unwrap();
        assert!((prepared.layers[0].angle - std::f32::consts::FRAC_PI_2).abs() < 1e-6);
    }

    #[test]
    fn project_property_overrides_change_the_prepared_layer_without_editing_assets() {
        let fixture = Fixture::new();
        fixture.write("project.json", br#"{"type":"scene","file":"scene.json","general":{"properties":{"opacity":{"type":"slider","value":1,"min":0,"max":1}}}}"#);
        fixture.scene(
            serde_json::json!([{"image":"models/image.json","alpha":{"user":"opacity","value":1}}]),
        );
        let before = std::fs::read(fixture.0.join("project.json")).unwrap();
        let prepared = PreparedScene::prepare_with_properties(
            &fixture.0,
            None,
            &BTreeMap::from([("opacity".into(), serde_json::json!(0.25))]),
        )
        .unwrap();
        assert_eq!(prepared.layers[0].tint[3], 0.25);
        assert_eq!(
            std::fs::read(fixture.0.join("project.json")).unwrap(),
            before
        );
        assert!(
            PreparedScene::prepare_with_properties(
                &fixture.0,
                None,
                &BTreeMap::from([("opacity".into(), serde_json::json!(1.01))])
            )
            .is_err()
        );
        assert_eq!(
            PreparedScene::prepare(&fixture.0, None).unwrap().layers[0].tint[3],
            1.0
        );
    }

    #[test]
    fn grouped_project_prepares_scalar_controls_and_rejects_group_values() {
        let fixture = Fixture::new();
        fixture.write("project.json", br#"{"type":"scene","file":"scene.json","general":{"properties":{"elements":{"type":"group","text":"ELEMENTS","value":"","order":101},"opacity":{"type":"slider","value":0.4,"min":0,"max":1,"order":102}}}}"#);
        let source = "let opacity=0;export function applyUserProperties(p){opacity=p.opacity;}export function update(v){return opacity;}";
        fixture.scene(
            serde_json::json!([{"image":"models/image.json","alpha":{"script":source,"value":1}}]),
        );
        let before = std::fs::read(fixture.0.join("project.json")).unwrap();
        let prepared = PreparedScene::prepare(&fixture.0, None).unwrap();
        assert_eq!(prepared.layers[0].tint[3], 0.4);
        assert!(prepared.scripts.is_none());
        let prepared = PreparedScene::prepare_with_properties(
            &fixture.0,
            None,
            &BTreeMap::from([("opacity".into(), serde_json::json!(0.8))]),
        )
        .unwrap();
        assert_eq!(prepared.layers[0].tint[3], 0.8);
        let definitions = PreparedScene::property_definitions(&fixture.0, None).unwrap();
        assert_eq!(definitions.len(), 1);
        assert_eq!(definitions[0].name, "opacity");
        let error = PreparedScene::prepare_with_properties(
            &fixture.0,
            None,
            &BTreeMap::from([
                ("elements".into(), serde_json::json!("")),
                ("opacity".into(), serde_json::json!(0.9)),
            ]),
        )
        .unwrap_err();
        assert_eq!(unsupported_feature_code(&error), Some("property_binding"));
        fixture.scene(serde_json::json!([{"image":"models/image.json","alpha":{"user":"elements","value":1}}]));
        assert_eq!(
            unsupported_feature_code(&PreparedScene::prepare(&fixture.0, None).unwrap_err()),
            Some("property_binding")
        );
        fixture.scene(serde_json::json!([{"image":"models/image.json","alpha":{"script":"let opacity=0;export function applyUserProperties(p){opacity=p.elements;}export function update(v){return opacity;}","value":1}}]));
        assert_eq!(
            unsupported_feature_code(&PreparedScene::prepare(&fixture.0, None).unwrap_err()),
            Some("scenescript")
        );
        assert_eq!(
            std::fs::read(fixture.0.join("project.json")).unwrap(),
            before
        );
    }

    #[test]
    fn grouped_dynamic_scripts_apply_control_snapshots_without_header_values() {
        let fixture = Fixture::new();
        fixture.write("project.json", br#"{"type":"scene","file":"scene.json","general":{"properties":{"elements":{"type":"group","text":"ELEMENTS","value":""},"opacity":{"type":"slider","value":0.4,"min":0,"max":1}}}}"#);
        let source = "export function applyUserProperties(p){thisLayer.alpha=p.opacity;}export function update(){}";
        fixture.scene(serde_json::json!([{"image":"models/image.json","visible":{"script":source,"value":true}}]));
        let prepared = PreparedScene::prepare(&fixture.0, None).unwrap();
        assert_eq!(
            prepared.scripts.as_ref().unwrap().initial_states()[0].alpha,
            0.4
        );
        let prepared = PreparedScene::prepare_with_properties(
            &fixture.0,
            None,
            &BTreeMap::from([("opacity".into(), serde_json::json!(0.75))]),
        )
        .unwrap();
        assert_eq!(
            prepared.scripts.as_ref().unwrap().initial_states()[0].alpha,
            0.75
        );
        fixture.scene(serde_json::json!([{"image":"models/image.json","visible":{"script":"export function applyUserProperties(p){thisLayer.alpha=p.elements;}export function update(){}","value":true}}]));
        assert_eq!(
            unsupported_feature_code(&PreparedScene::prepare(&fixture.0, None).unwrap_err()),
            Some("scenescript")
        );
        // Resource declarations remain outside the scalar/VM value snapshot.
        // An empty selection uses the authored instance fallback.
        fixture.write("project.json", br#"{"type":"scene","file":"scene.json","general":{"properties":{"glitch1":{"type":"group","index":1,"order":101,"text":"ELEMENTS","value":""},"crt":{"type":"group","index":7,"order":107,"text":"CRT","value":""},"custombackground":{"type":"scenetexture","order":106,"value":""}}}}"#);
        fixture.scene(serde_json::json!([{"image":"models/image.json"}]));
        assert!(PreparedScene::prepare(&fixture.0, None).is_ok());
        fixture.write("project.json",br#"{"type":"scene","file":"scene.json","general":{"properties":{"unsupported":{"type":"unknown","value":""}}}}"#);
        assert_eq!(
            unsupported_feature_code(&PreparedScene::prepare(&fixture.0, None).unwrap_err()),
            Some("property_binding")
        );
    }

    #[test]
    fn invalid_reachable_property_resources_fail_before_gl_initialization() {
        let fixture = Fixture::new();
        fixture.write("project.json", br#"{"type":"scene","file":"scene.json","general":{"properties":{"flag":{"type":"bool","value":true}}}}"#);
        fixture.write(
            "models/image.json",
            br#"{"material":"materials/image.json","ignored":{"user":"missing","value":true}}"#,
        );
        let error = PreparedScene::prepare(&fixture.0, None).unwrap_err();
        assert_eq!(unsupported_feature_code(&error), Some("property_binding"));
        fixture.write(
            "models/image.json",
            br#"{"material":"materials/image.json"}"#,
        );
        fixture.write("materials/image.json", br#"{"passes":[{"shader":"genericimage4","blending":"translucent","textures":["image"],"ignored":{"user":"flag","value":true,"binding":"expression"}}]}"#);
        let error = PreparedScene::prepare(&fixture.0, None).unwrap_err();
        assert_eq!(unsupported_feature_code(&error), Some("property_binding"));
    }

    #[test]
    fn rejects_framebuffer_budget_before_any_gl_allocation() {
        let fixture = Fixture::new();
        fixture.shake_shader();
        fixture.write(
            "effects/shake/effect.json",
            br#"{"passes":[{"material":"materials/effects/shake.json"}]}"#,
        );
        fixture.scene(serde_json::json!([{
            "image": "models/image.json", "size": "16384 16384", "effects":[{"file":"effects/shake/effect.json"}]
        }]));
        let error = PreparedScene::prepare(&fixture.0, None).unwrap_err();
        assert!(matches!(
            error.downcast_ref::<lwe_scene::SceneAssetError>(),
            Some(lwe_scene::SceneAssetError::LimitExceeded { limit, .. })
                if *limit == renderer::MAX_SCENE_GPU_BYTES
        ));
    }

    #[test]
    fn repeated_effects_are_preserved_in_order_with_per_pass_constants() {
        let fixture = Fixture::new();
        fixture.shake_shader();
        fixture.write(
            "effects/shake/effect.json",
            br#"{"passes":[{"material":"materials/effects/shake.json"}]}"#,
        );
        fixture.scene(serde_json::json!([{"image":"models/image.json","effects":[
            {"file":"effects/shake/effect.json","passes":[{"constantshadervalues":{"bounds":"0 1"}}]},
            {"file":"effects/shake/effect.json","passes":[{"constantshadervalues":{"bounds":"0.2 0.8"}}]}
        ]}]));
        let prepared = PreparedScene::prepare(&fixture.0, None).unwrap();
        assert_eq!(prepared.summary().pass_count, 3);
        assert_eq!(
            prepared.layers[0].stages[2].passes[0].constants["g_Bounds"],
            [0.2, 0.8]
        );
    }

    #[test]
    fn rejects_unwritten_effect_targets_and_framebuffer_feedback() {
        let fixture = Fixture::new();
        fixture.shake_shader();
        fixture.scene(serde_json::json!([{"image":"models/image.json","effects":[{"file":"effects/shake/effect.json"}]}]));
        fixture.write("effects/shake/effect.json", br#"{"fbos":[{"name":"buffer","scale":2}],"passes":[{"material":"materials/effects/shake.json","bind":[{"name":"buffer","index":0}]}]}"#);
        let error = PreparedScene::prepare(&fixture.0, None).unwrap_err();
        assert!(error.to_string().contains("unwritten target"));
        fixture.write("effects/shake/effect.json", br#"{"fbos":[{"name":"buffer","scale":2}],"passes":[{"material":"materials/effects/shake.json","target":"buffer"},{"material":"materials/effects/shake.json","target":"buffer","bind":[{"name":"buffer","index":0}]}]}"#);
        let error = PreparedScene::prepare(&fixture.0, None).unwrap_err();
        assert!(error.to_string().contains("own render target"));
    }

    #[test]
    fn unsupported_object_and_shader_combo_do_not_receive_runnable_reports() {
        let fixture = Fixture::new();
        fixture.scene(serde_json::json!([{"particle":"particles/snow.json"}]));
        fixture.write("particles/snow.json", br#"{"unknown_behavior":true}"#);
        let error = PreparedScene::prepare(&fixture.0, None).unwrap_err();
        assert_eq!(unsupported_feature_code(&error), Some("particle"));
        fixture.scene(serde_json::json!([{"image":"models/image.json"}]));
        fixture.write("materials/image.json", br#"{"passes":[{"shader":"genericimage4","textures":["image"],"combos":{"SKINNING":1}}]}"#);
        let error = PreparedScene::prepare(&fixture.0, None).unwrap_err();
        assert_eq!(unsupported_feature_code(&error), Some("shader"));
    }

    #[test]
    fn material_uniform_types_are_validated_before_opengl_binding() {
        let fixture = Fixture::new();
        fixture.shake_shader();
        fixture.scene(serde_json::json!([{"image":"models/image.json","effects":[{"file":"effects/shake/effect.json"}]}]));
        fixture.write(
            "effects/shake/effect.json",
            br#"{"passes":[{"material":"materials/effects/shake.json"}]}"#,
        );
        fixture.write("shaders/effects/shake.vert",b"uniform int g_Bounds; // {\"material\":\"bounds\",\"default\":1}\nvoid main(){gl_Position=vec4(0);}");
        let error = PreparedScene::prepare(&fixture.0, None).unwrap_err();
        assert_eq!(unsupported_feature_code(&error), Some("shader"));
        assert!(error.to_string().contains("uniform type int"));
    }

    #[test]
    fn material_passes_keep_their_source_texture_and_read_the_prior_composite() {
        let fixture = Fixture::new();
        fixture.write("materials/image.json",br#"{"passes":[{"shader":"genericimage4","blending":"translucent","textures":["image"]},{"shader":"genericimage4","blending":"translucent","textures":["image"]}]}"#);
        let prepared = PreparedScene::prepare(&fixture.0, None).unwrap();
        assert_eq!(prepared.summary().pass_count, 2);
        let second = &prepared.layers[0].stages[0].passes[1];
        assert!(matches!(second.textures[0], TextureBinding::Static(0)));
        assert!(matches!(second.textures[1], TextureBinding::Input));
    }

    #[test]
    fn legacy_flow_preserves_mask_sampling_metadata_and_time_animation() {
        let fixture = Fixture::new();
        fixture.legacy_shader("flowimage", "uniform sampler2D g_Texture0;\nuniform sampler2D g_Texture1;\nuniform float g_Time;\nvarying vec2 v_TexCoord;\nvoid main(){gl_FragColor=texSample2D(g_Texture0,v_TexCoord+vec2(g_Time*.01,0))*texSample2D(g_Texture1,v_TexCoord);}");
        fixture.write("materials/image.json", br#"{"passes":[{"shader":"flowimage","textures":["image","mask"],"constantshadervalues":{"Power":1}}]}"#);
        let mut texture = std::fs::read(fixture.0.join("materials/image.tex")).unwrap();
        texture[22..26].copy_from_slice(&2_u32.to_le_bytes());
        fixture.write("materials/mask.tex", &texture);
        let prepared = PreparedScene::prepare(&fixture.0, None).unwrap();
        assert!(prepared.summary().animated);
        let pass = &prepared.layers[0].stages[0].passes[0];
        assert!(matches!(pass.textures[1], TextureBinding::Static(1)));
        assert_eq!(prepared.textures[1].source_format, 0);
        assert_eq!(prepared.textures[0].source_flags, 0);
        assert_eq!(prepared.textures[1].source_flags, 2);
        texture[22..26].copy_from_slice(&1_u32.to_le_bytes());
        fixture.write("materials/mask.tex", &texture);
        assert!(
            PreparedScene::prepare(&fixture.0, None)
                .unwrap_err()
                .to_string()
                .contains("sampling flags")
        );
    }

    #[test]
    fn legacy_model_dimensions_and_scrolling_are_used_without_object_size() {
        let fixture = Fixture::new();
        fixture.scene(serde_json::json!([{"image":"models/image.json"}]));
        fixture.write(
            "models/image.json",
            br#"{"width":256,"height":128,"material":"materials/image.json"}"#,
        );
        fixture.legacy_shader("genericimage", "uniform sampler2D g_Texture0;\nuniform float g_Time;\nuniform float g_ScrollX; // {\"material\":\"Scroll 1 X\",\"default\":0}\nvarying vec2 v_TexCoord;\nvoid main(){gl_FragColor=texSample2D(g_Texture0,v_TexCoord+vec2(g_Time*g_ScrollX,0));}");
        fixture.write(
            "materials/image.json",
            br#"{"passes":[{"shader":"genericimage","textures":["image"]}]}"#,
        );
        let prepared = PreparedScene::prepare(&fixture.0, None).unwrap();
        assert_eq!(prepared.layers[0].size, [256.0, 128.0]);
        assert!(!prepared.summary().animated);
        fixture.write("materials/image.json", br#"{"passes":[{"shader":"genericimage","textures":["image"],"constantshadervalues":{"Scroll 1 X":-0.06}}]}"#);
        assert!(
            PreparedScene::prepare(&fixture.0, None)
                .unwrap()
                .summary()
                .animated
        );
        fixture.scene(serde_json::json!([{"image":"models/image.json","size":"32 64"}]));
        assert_eq!(
            PreparedScene::prepare(&fixture.0, None).unwrap().layers[0].size,
            [32.0, 64.0]
        );
        fixture.write(
            "models/image.json",
            br#"{"width":256,"material":"materials/image.json"}"#,
        );
        assert!(PreparedScene::prepare(&fixture.0, None).is_err());
    }

    #[test]
    fn legacy_user_shader_colors_use_validated_project_choices() {
        let fixture = Fixture::new();
        fixture.write("project.json", br#"{"type":"scene","file":"scene.json","general":{"properties":{"elements":{"type":"group","text":"ELEMENTS","value":""},"flagcolor":{"type":"color","value":"0.1 0.2 0.3"}}}}"#);
        fixture.legacy_shader("flag", "uniform sampler2D g_Texture0;\nuniform sampler2D g_Texture1;\nuniform sampler2D g_Texture2;\nuniform vec3 g_Color1; // {\"material\":\"color1\",\"default\":\"1 1 1\"}\nvarying vec2 v_TexCoord;\nvoid main(){gl_FragColor=texSample2D(g_Texture0,v_TexCoord)*texSample2D(g_Texture1,v_TexCoord)*texSample2D(g_Texture2,v_TexCoord)*vec4(g_Color1,1);}");
        fixture.write("materials/image.json", br#"{"passes":[{"shader":"flag","textures":["image","image","image"],"constantshadervalues":{"color1":"1 0 0"},"usershadervalues":{"flagcolor":"color1"}}]}"#);
        let prepared = PreparedScene::prepare(&fixture.0, None).unwrap();
        assert_eq!(
            prepared.layers[0].stages[0].passes[0].constants["g_Color1"],
            [0.1, 0.2, 0.3]
        );
        let choices =
            BTreeMap::from([("flagcolor".to_owned(), serde_json::json!([0.4, 0.5, 0.6]))]);
        let prepared = PreparedScene::prepare_with_properties(&fixture.0, None, &choices).unwrap();
        assert_eq!(
            prepared.layers[0].stages[0].passes[0].constants["g_Color1"],
            [0.4, 0.5, 0.6]
        );
        for mapping in [
            serde_json::json!({"missing":"color1"}),
            serde_json::json!({"elements":"color1"}),
            serde_json::json!({"flagcolor":"unknown"}),
            serde_json::json!({"flagcolor":{}}),
        ] {
            fixture.write("materials/image.json", &serde_json::to_vec(&serde_json::json!({"passes":[{"shader":"flag","textures":["image","image","image"],"usershadervalues":mapping}]})).unwrap());
            assert!(PreparedScene::prepare(&fixture.0, None).is_err());
        }
        fixture.write("materials/image.json", br#"{"passes":[{"shader":"flag","textures":["image","image","image"],"combos":{"TEX1FORMAT":4}}]}"#);
        assert!(
            PreparedScene::prepare(&fixture.0, None)
                .unwrap_err()
                .to_string()
                .contains("format mismatch")
        );
    }

    #[test]
    fn legacy_inert_keys_do_not_admit_unknown_or_malformed_constants() {
        let fixture = Fixture::new();
        fixture.legacy_shader("flowimage", "uniform sampler2D g_Texture0;\nuniform sampler2D g_Texture1;\nvarying vec2 v_TexCoord;\nvoid main(){gl_FragColor=texSample2D(g_Texture0,v_TexCoord)*texSample2D(g_Texture1,v_TexCoord);}");
        for constants in [
            serde_json::json!({"Power":[1,2]}),
            serde_json::json!({"Other":1}),
            serde_json::json!([]),
            serde_json::json!({"Power":"NaN"}),
        ] {
            fixture.write("materials/image.json", &serde_json::to_vec(&serde_json::json!({"passes":[{"shader":"flowimage","textures":["image","image"],"constantshadervalues":constants}]})).unwrap());
            assert!(PreparedScene::prepare(&fixture.0, None).is_err());
        }
    }

    #[test]
    fn optional_mask_samplers_require_textures_only_when_the_mask_branch_is_used() {
        for (shader_name, slot) in [("effects/shake", 3), ("effects/shine_downsample2", 1)] {
            let fixture = Fixture::new();
            fixture.shake_shader();
            fixture.scene(serde_json::json!([{"image":"models/image.json","effects":[{"file":"effects/test/effect.json"}]}]));
            fixture.write(
                "effects/test/effect.json",
                br#"{"passes":[{"material":"materials/effects/test.json"}]}"#,
            );
            fixture.write(
                "materials/effects/test.json",
                &serde_json::to_vec(&serde_json::json!({"passes":[{"shader":shader_name}]}))
                    .unwrap(),
            );
            fixture.write(&format!("shaders/{shader_name}.vert"),b"uniform mat4 g_ModelViewProjectionMatrix;\nattribute vec3 a_Position;\nvoid main(){gl_Position=mul(vec4(a_Position,1),g_ModelViewProjectionMatrix);}");
            let source = format!(
                "uniform sampler2D g_Texture0;\nuniform sampler2D g_Texture{slot}; // {{\"combo\":\"MASK\"}}\nvoid main(){{\nvec4 color=texSample2D(g_Texture0,vec2(0));\n#if MASK\ncolor.rgb*=texSample2D(g_Texture{slot},vec2(0)).r;\n#endif\ngl_FragColor=color;\n}}"
            );
            fixture.write(&format!("shaders/{shader_name}.frag"), source.as_bytes());
            assert!(
                PreparedScene::prepare(&fixture.0, None).is_ok(),
                "MASK=0 incorrectly required slot {slot}"
            );
            fixture.scene(serde_json::json!([{"image":"models/image.json","effects":[{"file":"effects/test/effect.json","passes":[{"combos":{"MASK":1}}]}]}]));
            let error = PreparedScene::prepare(&fixture.0, None).unwrap_err();
            assert_eq!(unsupported_feature_code(&error), Some("shader"));
            assert!(
                error
                    .to_string()
                    .contains(&format!("g_Texture{slot} has no bound texture"))
            );
        }
    }

    #[test]
    fn image_and_model_parent_fields_are_not_silently_ignored() {
        let fixture = Fixture::new();
        for parent in [
            serde_json::json!(17),
            serde_json::json!(-1),
            serde_json::json!(0),
            Value::Null,
        ] {
            fixture.scene(serde_json::json!([{"image":"models/image.json","parent":parent}]));
            let error = PreparedScene::prepare(&fixture.0, None).unwrap_err();
            assert_eq!(unsupported_feature_code(&error), Some("hierarchy"));
        }
        fixture.scene(serde_json::json!([{"image":"models/image.json"}]));
        for model in [
            serde_json::json!({"autosize":true,"parent":5,"material":"materials/image.json"}),
            serde_json::json!({"autosize":true,"options":{"parent":"layer"},"material":"materials/image.json"}),
        ] {
            fixture.write("models/image.json", &serde_json::to_vec(&model).unwrap());
            let error = PreparedScene::prepare(&fixture.0, None).unwrap_err();
            assert_eq!(unsupported_feature_code(&error), Some("hierarchy"));
        }
    }

    #[test]
    fn canonical_static_camera_ignores_parallel_saved_xy_but_rejects_other_modes() {
        let fixture = Fixture::new();
        let mut scene = serde_json::json!({
            "general":{"orthogonalprojection":{"width":16,"height":16}},
            "objects":[{"image":"models/image.json"}]
        });
        for camera in [
            serde_json::json!({"eye":"1 0 1","center":"0 0 0"}),
            serde_json::json!({"eye":"0 0 1","center":"1 0 0"}),
            serde_json::json!({"eye":"0 0 1","center":"0 0 0","paths":["camera/path.json"]}),
            serde_json::json!({"paths":null}),
            serde_json::json!({"up":"0 0 1"}),
        ] {
            scene["camera"] = camera;
            fixture.write("scene.json", &serde_json::to_vec(&scene).unwrap());
            let error = PreparedScene::prepare(&fixture.0, None).unwrap_err();
            assert_eq!(unsupported_feature_code(&error), Some("camera"));
        }
        for camera in [
            serde_json::json!({"eye":"0 0 1","center":"0 0 0","up":"0 1 0"}),
            serde_json::json!({"eye":"175.39784 95.07546 0","center":"175.39784 95.07546 -1","up":"0 1 0"}),
            serde_json::json!({"eye":"0 1 1","center":"0 1 0","paths":[]}),
        ] {
            scene["camera"] = camera;
            fixture.write("scene.json", &serde_json::to_vec(&scene).unwrap());
            let prepared = PreparedScene::prepare(&fixture.0, None).unwrap();
            assert_eq!(prepared.layers[0].origin, [8.0, 8.0]);
        }
        scene["general"]["orthogonalprojection"]["auto"] = Value::Bool(true);
        fixture.write("scene.json", &serde_json::to_vec(&scene).unwrap());
        let error = PreparedScene::prepare(&fixture.0, None).unwrap_err();
        assert_eq!(unsupported_feature_code(&error), Some("camera"));
    }

    #[test]
    fn numeric_properties_reject_nan_and_wrong_dimensions() {
        assert!(numbers(&Value::String("NaN 1".into())).is_err());
        assert!(vector::<2>(&serde_json::json!({"size":"1 2 3"}), "size", [1.0; 2]).is_err());
        assert_eq!(
            vector::<3>(&serde_json::json!({"origin":"1 2 0"}), "origin", [0.0; 3]).unwrap(),
            [1.0, 2.0, 0.0]
        );
    }

    #[test]
    fn dynamic_property_bindings_are_not_silently_defaulted() {
        let value = serde_json::json!({"value":0.5,"user":"slider"});
        let error = numbers(&value).unwrap_err();
        assert_eq!(unsupported_feature_code(&error), Some("property_binding"));
        for key in ["script", "scenescript"] {
            let error = numbers(&serde_json::json!({"value":0.5,(key):"program"})).unwrap_err();
            assert_eq!(unsupported_feature_code(&error), Some("scenescript"));
        }
        assert!(has_script(
            &serde_json::json!({"effects":[{"passes":[{"script":"x"}]}]})
        ));
    }

    #[test]
    fn unsupported_blending_and_depth_have_explicit_feature_codes() {
        let error = blend(Some(&Value::String("mystery".into()))).unwrap_err();
        assert_eq!(unsupported_feature_code(&error), Some("shader"));
        let error = validate_pass(&serde_json::json!({"depthtest":"enabled"})).unwrap_err();
        assert_eq!(unsupported_feature_code(&error), Some("model"));
    }
}
