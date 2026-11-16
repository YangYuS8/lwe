//! CPU-only regressions for the production preparation and allocation identity.

use super::*;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let directory = std::env::temp_dir().join(format!(
            "lwe-texture-identity-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&directory).unwrap();
        let fixture = Self(directory);
        fixture.write("project.json", br#"{"type":"scene","file":"scene.json"}"#);
        fixture.write("scene.json", br#"{"general":{"orthogonalprojection":{"width":16,"height":16}},"objects":[{"image":"models/image.json","size":"16 16","origin":"8 8 0"}]}"#);
        fixture.write(
            "models/image.json",
            br#"{"autosize":true,"material":"materials/image.json"}"#,
        );
        fixture.write("materials/image.json", br#"{"passes":[{"shader":"genericimage4","blending":"translucent","textures":["image"]}]}"#);
        fixture.write("materials/image.tex", &texture_bytes([255, 0, 0, 255]));
        fixture
    }

    fn write(&self, name: &str, bytes: &[u8]) {
        let path = self.0.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }

    fn prepare(&self, builtins: Option<&Path>) -> PreparedScene {
        PreparedScene::prepare(&self.0, builtins).unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn texture_bytes(rgba: [u8; 4]) -> Vec<u8> {
    let mut bytes = b"TEXV0005\0TEXI0001\0".to_vec();
    for value in [0_u32, 0, 1, 1, 1, 1, 0] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(b"TEXB0001\0");
    for value in [1_u32, 1, 1, 1, 4] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&rgba);
    bytes
}

fn identity(
    origin: ResourceOrigin,
    root: &Path,
    atlas: bool,
    bytes: &[u8],
    decoded: &Texture,
) -> [u8; 32] {
    texture_fingerprint(
        TextureSource {
            origin,
            root,
            path: "materials/image.tex",
            binding: "image",
            atlas,
        },
        bytes,
        decoded,
    )
}

#[test]
fn replacement_at_same_path_length_and_mtime_changes_prepared_identity() {
    let fixture = Fixture::new();
    let first = fixture.prepare(None);
    let path = fixture.0.join("materials/image.tex");
    let previous = std::fs::metadata(&path).unwrap();
    let bytes = texture_bytes([0, 0, 255, 255]);
    assert_eq!(bytes.len() as u64, previous.len());
    std::fs::write(&path, &bytes).unwrap();
    std::fs::File::open(&path)
        .unwrap()
        .set_times(std::fs::FileTimes::new().set_modified(previous.modified().unwrap()))
        .unwrap();
    let second = fixture.prepare(None);
    assert_eq!(
        std::fs::metadata(&path).unwrap().modified().unwrap(),
        previous.modified().unwrap()
    );
    assert_eq!(first.textures[0].rgba(), [255, 0, 0, 255]);
    assert_eq!(second.textures[0].rgba(), [0, 0, 255, 255]);
    assert_ne!(
        first.textures[0].fingerprint,
        second.textures[0].fingerprint
    );
    assert_eq!(
        second.textures[0].fingerprint,
        fixture.prepare(None).textures[0].fingerprint
    );
}

#[test]
fn identity_uses_the_resolved_descriptor_bytes_after_path_replacement() {
    let fixture = Fixture::new();
    let prepared = fixture.prepare(None);
    let assets = SceneAssets::open(&fixture.0, None).unwrap();
    let resource = assets.resource("materials/image.tex").unwrap();
    fixture.write(
        "materials/replacement.tex",
        &texture_bytes([0, 255, 0, 255]),
    );
    std::fs::rename(
        fixture.0.join("materials/replacement.tex"),
        fixture.0.join("materials/image.tex"),
    )
    .unwrap();
    let bytes = resource
        .read_range(0, resource.len() as usize, MAX_TEXTURE)
        .unwrap();
    let decoded = Texture::decode(&bytes).unwrap();
    assert_eq!(decoded.rgba, [255, 0, 0, 255]);
    assert_eq!(
        identity(
            resource.origin(),
            assets.project_dir(),
            false,
            &bytes,
            &decoded
        ),
        prepared.textures[0].fingerprint
    );
    let replacement = fixture.prepare(None);
    assert_eq!(replacement.textures[0].rgba(), [0, 255, 0, 255]);
    assert_ne!(
        prepared.textures[0].fingerprint,
        replacement.textures[0].fingerprint
    );
}

#[test]
fn source_namespace_precedence_and_root_are_part_of_identity() {
    let project = Fixture::new();
    let builtins = Fixture::new();
    let project_scene = project.prepare(Some(&builtins.0));
    assert_eq!(project_scene.textures[0].rgba(), [255, 0, 0, 255]);
    std::fs::remove_file(project.0.join("materials/image.tex")).unwrap();
    let builtin_scene = project.prepare(Some(&builtins.0));
    assert_eq!(
        builtin_scene.textures[0].rgba(),
        project_scene.textures[0].rgba()
    );
    assert_ne!(
        builtin_scene.textures[0].fingerprint,
        project_scene.textures[0].fingerprint
    );

    let bytes = texture_bytes([255, 0, 0, 255]);
    let name = b"materials/image.tex";
    let mut package = 8_u32.to_le_bytes().to_vec();
    package.extend_from_slice(b"PKGV0023");
    package.extend_from_slice(&1_u32.to_le_bytes());
    package.extend_from_slice(&(name.len() as u32).to_le_bytes());
    package.extend_from_slice(name);
    package.extend_from_slice(&0_u32.to_le_bytes());
    package.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
    package.extend_from_slice(&bytes);
    project.write("scene.pkg", &package);
    project.write("materials/image.tex", &texture_bytes([0, 255, 0, 255]));
    let packaged_scene = project.prepare(Some(&builtins.0));
    assert_eq!(packaged_scene.textures[0].rgba(), [255, 0, 0, 255]);
    assert_ne!(
        packaged_scene.textures[0].fingerprint,
        project_scene.textures[0].fingerprint
    );
    assert_ne!(
        packaged_scene.textures[0].fingerprint,
        builtin_scene.textures[0].fingerprint
    );
    assert_ne!(
        Fixture::new().prepare(None).textures[0].fingerprint,
        project_scene.textures[0].fingerprint
    );
}

#[test]
fn encoded_bytes_decoded_rgba_and_atlas_contract_all_affect_identity() {
    let bytes = texture_bytes([255, 0, 0, 255]);
    let decoded = Texture::decode(&bytes).unwrap();
    let root = Path::new("/fixture");
    let first = identity(ResourceOrigin::Project, root, false, &bytes, &decoded);
    let mut changed_encoding = bytes.clone();
    changed_encoding[0] ^= 1;
    assert_ne!(
        first,
        identity(
            ResourceOrigin::Project,
            root,
            false,
            &changed_encoding,
            &decoded
        )
    );
    let mut changed_rgba = decoded.clone();
    changed_rgba.rgba[0] ^= 1;
    assert_ne!(
        first,
        identity(ResourceOrigin::Project, root, false, &bytes, &changed_rgba)
    );
    assert_ne!(
        first,
        identity(ResourceOrigin::Project, root, true, &bytes, &decoded)
    );
    assert_ne!(
        first,
        identity(ResourceOrigin::Builtin, root, false, &bytes, &decoded)
    );
    assert_ne!(
        first,
        identity(
            ResourceOrigin::Project,
            Path::new("/other"),
            false,
            &bytes,
            &decoded
        )
    );
    for (path, binding) in [
        ("materials/other.tex", "image"),
        ("materials/image.tex", "other"),
    ] {
        assert_ne!(
            first,
            texture_fingerprint(
                TextureSource {
                    origin: ResourceOrigin::Project,
                    root,
                    path,
                    binding,
                    atlas: false,
                },
                &bytes,
                &decoded
            )
        );
    }
}

#[test]
fn all_metadata_dimensions_flags_mips_and_animation_are_sealed() {
    let bytes = texture_bytes([255, 0, 0, 255]);
    let decoded = Texture::decode(&bytes).unwrap();
    let root = Path::new("/fixture");
    let first = identity(ResourceOrigin::Project, root, false, &bytes, &decoded);
    let changes: &[fn(&mut Texture)] = &[
        |t| t.width += 1,
        |t| t.height += 1,
        |t| t.metadata.version += 1,
        |t| t.metadata.body_version += 1,
        |t| t.metadata.format += 1,
        |t| t.metadata.flags ^= 2,
        |t| t.metadata.storage_width += 1,
        |t| t.metadata.storage_height += 1,
        |t| t.metadata.image_width += 1,
        |t| t.metadata.image_height += 1,
        |t| t.metadata.image_info_extra += 1,
        |t| t.metadata.container_format += 1,
        |t| t.metadata.body_extra = Some(0),
        |t| t.metadata.images += 1,
        |t| t.metadata.video = true,
        |t| t.metadata.mips[0].image += 1,
        |t| t.metadata.mips[0].level += 1,
        |t| t.metadata.mips[0].width += 1,
        |t| t.metadata.mips[0].height += 1,
        |t| t.metadata.mips[0].compressed = true,
        |t| t.metadata.mips[0].decompressed_size += 1,
        |t| t.metadata.mips[0].stored_size += 1,
        |t| t.metadata.mips[0].offset += 1,
        |t| t.metadata.mips[0].video = true,
        |t| t.metadata.mips.push(t.metadata.mips[0].clone()),
        |t| {
            t.metadata.animation = Some(lwe_scene::AnimationMetadata {
                version: 2,
                width: 1,
                height: 1,
                frames: vec![lwe_scene::AnimationFrame {
                    image: 0,
                    duration: 0.0,
                    coordinates: [0.0, 0.0, 1.0, 0.0, 0.0, 1.0],
                }],
            })
        },
    ];
    for (index, change) in changes.iter().enumerate() {
        let mut changed = decoded.clone();
        change(&mut changed);
        assert_ne!(
            first,
            identity(ResourceOrigin::Project, root, false, &bytes, &changed),
            "metadata case {index}"
        );
    }
    let mut animated = decoded;
    animated.metadata.animation = Some(lwe_scene::AnimationMetadata {
        version: 2,
        width: 1,
        height: 1,
        frames: vec![lwe_scene::AnimationFrame {
            image: 0,
            duration: 0.0,
            coordinates: [0.0, 0.0, 1.0, 0.0, 0.0, 1.0],
        }],
    });
    let first = identity(ResourceOrigin::Project, root, true, &bytes, &animated);
    let changes: &[fn(&mut lwe_scene::AnimationMetadata)] = &[
        |a| a.version += 1,
        |a| a.width += 1,
        |a| a.height += 1,
        |a| a.frames[0].image += 1,
        |a| a.frames[0].duration = -0.0,
        |a| a.frames[0].coordinates[3] = 0.25,
        |a| a.frames.push(a.frames[0].clone()),
    ];
    for change in changes {
        let mut changed = animated.clone();
        change(changed.metadata.animation.as_mut().unwrap());
        assert_ne!(
            first,
            identity(ResourceOrigin::Project, root, true, &bytes, &changed)
        );
    }
}

#[test]
fn plans_deduplicate_only_the_same_identity_and_fixed_sampling_variant() {
    let fixture = Fixture::new();
    let mut scene = fixture.prepare(None);
    let pass = &mut scene.layers[0].stages[0].passes[0];
    pass.label = "effects/waterripple".into();
    pass.textures = vec![
        TextureBinding::Static(0),
        TextureBinding::Black,
        TextureBinding::Static(0),
    ];
    let plan = scene.resource_plan_for_output(16, 16).unwrap();
    assert_eq!(plan.textures.len(), 2);
    let work = plan.work.values().copied().sum::<u64>();
    assert_eq!(plan.textures.values().copied().sum::<u64>(), 8);
    assert_eq!(plan.immutable_bytes().unwrap(), 8 + work);
    assert!(plan.textures.keys().any(|key| key.repeat));
    assert!(plan.textures.keys().any(|key| !key.repeat));
    let mut retained = plan.clone();
    retained.merge(&plan).unwrap();
    assert_eq!(retained.immutable_bytes().unwrap(), 8 + work);
    assert_eq!(retained.work, plan.work);
    assert_eq!(retained.private_bytes, plan.private_bytes * 2);
    fixture.write("materials/image.tex", &texture_bytes([0, 0, 255, 255]));
    let other = fixture
        .prepare(None)
        .resource_plan_for_output(16, 16)
        .unwrap();
    retained.merge(&other).unwrap();
    assert_eq!(retained.textures.len(), 3);
    assert_eq!(retained.immutable_bytes().unwrap(), 12 + work);
}

#[test]
fn decoded_cpu_payload_counts_capacity_without_identity_deduplication() {
    let fixture = Fixture::new();
    let mut scene = fixture.prepare(None);
    let texture = &scene.textures[0];
    let mut duplicate = PreparedTexture {
        fingerprint: texture.fingerprint,
        width: texture.width,
        height: texture.height,
        image_width: texture.image_width,
        image_height: texture.image_height,
        source_format: texture.source_format,
        source_flags: texture.source_flags,
        rgba: texture.rgba().to_vec(),
        animation: texture.animation.clone(),
        timed: None,
        static_source: None,
        image: None,
        text_frame: None,
        _rgba_reservation: None,
    };
    duplicate.rgba.reserve(32);
    let expected = scene.textures[0]
        .static_source
        .as_ref()
        .unwrap()
        .capacity_bytes() as u64
        + duplicate.rgba.capacity() as u64;
    scene.textures.push(duplicate);
    assert_eq!(scene.decoded_texture_bytes().unwrap(), expected);
    assert!(
        expected
            > scene
                .textures
                .iter()
                .map(|texture| texture.rgba().len() as u64)
                .sum::<u64>()
    );
}

#[test]
fn resource_read_bounds_and_unsafe_paths_still_fail_closed() {
    let fixture = Fixture::new();
    let path = fixture.0.join("materials/image.tex");
    std::fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_len(MAX_TEXTURE as u64 + 1)
        .unwrap();
    let error = PreparedScene::prepare(&fixture.0, None).unwrap_err();
    assert!(
        matches!(error.downcast_ref::<lwe_scene::SceneAssetError>(), Some(lwe_scene::SceneAssetError::LimitExceeded { size, limit, .. }) if *size == MAX_TEXTURE as u64 + 1 && *limit == MAX_TEXTURE as u64)
    );
    fixture.write(
        "materials/image.json",
        br#"{"passes":[{"shader":"genericimage4","textures":["../private"]}]}"#,
    );
    let error = PreparedScene::prepare(&fixture.0, None).unwrap_err();
    assert!(matches!(
        error.downcast_ref::<lwe_scene::SceneAssetError>(),
        Some(lwe_scene::SceneAssetError::UnsafePath { .. })
    ));
}

#[test]
fn immutable_cpu_source_cache_reuses_only_live_identical_actual_reads() {
    let fixture = Fixture::new();
    let first = fixture.prepare(None);
    let second = fixture.prepare(None);
    let first_owner = first.textures[0].static_source.as_ref().unwrap();
    let second_owner = second.textures[0].static_source.as_ref().unwrap();
    assert!(Arc::ptr_eq(first_owner, second_owner));
    let weak = Arc::downgrade(first_owner);
    fixture.write("materials/image.tex", &texture_bytes([0, 0, 255, 255]));
    let changed = fixture.prepare(None);
    assert!(!Arc::ptr_eq(
        first_owner,
        changed.textures[0].static_source.as_ref().unwrap()
    ));
    assert_eq!(changed.textures[0].rgba(), [0, 0, 255, 255]);
    drop(first);
    drop(second);
    assert!(
        weak.upgrade().is_none(),
        "weak cache must not pin old pixels or their reservation"
    );
}

#[test]
fn encoded_image_decodes_actual_first_mip_but_identity_covers_unused_mips() {
    let fixture = Fixture::new();
    let png = |width, height| {
        let mut encoded = std::io::Cursor::new(Vec::new());
        image::RgbaImage::from_pixel(width, height, image::Rgba([4, 5, 6, 7]))
            .write_to(&mut encoded, image::ImageFormat::Png)
            .unwrap();
        encoded.into_inner()
    };
    let mut bytes = b"TEXV0005\0TEXI0001\0".to_vec();
    // Container storage padding is not a decoded allocation. Only mip zero
    // enters ImageReader, while the source identity includes the entire TEX.
    for value in [0_u32, 3, 4, 4, 2, 2, 0] {
        bytes.extend(value.to_le_bytes());
    }
    bytes.extend(b"TEXB0003\0");
    for value in [1_u32, 13, 2] {
        bytes.extend(value.to_le_bytes());
    }
    for size in [2_u32, 1] {
        let encoded = png(size, size);
        for value in [size, size, 0, 0, encoded.len() as u32] {
            bytes.extend(value.to_le_bytes());
        }
        bytes.extend(encoded);
    }
    fixture.write("materials/image.tex", &bytes);
    let first = fixture.prepare(None);
    assert_eq!((first.textures[0].width, first.textures[0].height), (2, 2));
    assert_eq!(first.textures[0].rgba(), [4, 5, 6, 7].repeat(4));
    let last = bytes.last_mut().unwrap();
    *last ^= 1;
    fixture.write("materials/image.tex", &bytes);
    let second = fixture.prepare(None);
    assert_eq!(second.textures[0].rgba(), first.textures[0].rgba());
    assert_ne!(
        second.textures[0].fingerprint,
        first.textures[0].fingerprint
    );
    assert!(!Arc::ptr_eq(
        first.textures[0].static_source.as_ref().unwrap(),
        second.textures[0].static_source.as_ref().unwrap()
    ));
}

pub(super) fn prepare_storage_texture(
    format: u32,
    payload: &[u8],
    compressed: bool,
) -> PreparedTexture {
    let fixture = Fixture::new();
    let stored = if compressed {
        lz4_flex::block::compress(payload)
    } else {
        payload.to_vec()
    };
    let mut bytes = b"TEXV0005\0TEXI0001\0".to_vec();
    for value in [format, 0, 3, 2, 3, 2, 0] {
        bytes.extend(value.to_le_bytes());
    }
    bytes.extend(b"TEXB0003\0");
    for value in [
        1,
        u32::MAX,
        1,
        3,
        2,
        u32::from(compressed),
        if compressed { payload.len() as u32 } else { 0 },
        stored.len() as u32,
    ] {
        bytes.extend(value.to_le_bytes());
    }
    bytes.extend(stored);
    fixture.write("materials/image.tex", &bytes);
    fixture.prepare(None).textures.remove(0)
}

#[test]
fn static_storage_uses_the_decoded_mip_kind_not_just_the_tex_format() {
    use prepared_source::StaticTextureStorage::{R8, Rg8, Rgba8};
    let png_texel = [70, 19, 177, 101];
    let mut png = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(3, 2, image::Rgba(png_texel))
        .write_to(&mut png, image::ImageFormat::Png)
        .unwrap();
    for compressed in [false, true] {
        for (format, native, rgba, storage) in [
            (9, &[70][..], [70, 0, 0, 255], R8),
            (8, &[70, 19][..], [70, 19, 0, 255], Rg8),
        ] {
            let raw = prepare_storage_texture(format, &native.repeat(6), compressed);
            assert_eq!(raw.static_source.as_ref().unwrap().storage, storage);
            assert_eq!(raw.rgba(), rgba.repeat(6));
            assert_eq!(raw.static_source.as_ref().unwrap().capacity_bytes(), 24);

            // Real color and alpha must survive even with a raw-format header
            // and the raw container sentinel (-1).
            let encoded = prepare_storage_texture(format, png.get_ref(), compressed);
            assert_eq!(encoded.static_source.as_ref().unwrap().storage, Rgba8);
            assert_eq!(encoded.rgba(), png_texel.repeat(6));
        }
    }
}
