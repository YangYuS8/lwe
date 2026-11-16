use super::{atomic_file, scene_service::SceneService};
use lwe_engine::scene::{
    PropertyKind, SceneResourceChoices, property_definitions, scene_texture_memory_budget,
};
use lwe_scene::{ImageSource, SceneAssets};
use serde::Serialize;
use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::Read,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
};
use tauri_plugin_dialog::DialogExt;

const MAX_BYTES: usize = 32 * 1024 * 1024;
const MAX_STORE_BYTES: u64 = 512 * 1024 * 1024;
const MAX_STORE_FILES: usize = 256;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportedSceneImage {
    pub token: String,
    pub width: u32,
    pub height: u32,
}

pub struct SceneResourceService;
impl SceneResourceService {
    pub fn choose(
        app: &tauri::AppHandle,
        item_id: &str,
        name: &str,
    ) -> Result<Option<ImportedSceneImage>, String> {
        let entry = super::library_service::LibraryService::inspect_item(item_id)?;
        if entry.entry.project_type != lwe_library::WorkshopProjectType::Scene {
            return Err("Images are available only for Scene wallpapers".into());
        }
        Self::validate_control(&entry.entry.project_dir, name)?;
        let Some(selected) = app
            .dialog()
            .file()
            .add_filter("PNG / JPEG", &["png", "jpg", "jpeg"])
            .blocking_pick_file()
        else {
            return Ok(None);
        };
        let path = selected
            .into_path()
            .map_err(|_| "The selected image is not a local file")?;
        Self::import_selected(&path).map(Some)
    }
    pub fn validate_control(project_dir: &Path, name: &str) -> Result<(), String> {
        let project = read_project(project_dir)?;
        let definitions = property_definitions(&project).map_err(|e| e.to_string())?;
        if definitions
            .iter()
            .any(|p| p.name == name && p.kind == PropertyKind::Texture)
        {
            Ok(())
        } else {
            Err("The selected Scene has no matching image property".into())
        }
    }
    pub fn import_selected(path: &Path) -> Result<ImportedSceneImage, String> {
        ImageStore::new(data_root()?).import(path)
    }
    pub fn split_choices(
        project: &serde_json::Value,
        values: &BTreeMap<String, serde_json::Value>,
    ) -> Result<(BTreeMap<String, serde_json::Value>, SceneResourceChoices), String> {
        let definitions = property_definitions(project).map_err(|e| e.to_string())?;
        let mut scalars = BTreeMap::new();
        let mut images = SceneResourceChoices::new();
        for (name, value) in values {
            let definition = definitions
                .iter()
                .find(|p| p.name == *name)
                .ok_or("Scene selection has no declared property")?;
            match definition.kind {
                PropertyKind::Texture => {
                    let token = value
                        .as_str()
                        .filter(|v| valid_token(v))
                        .ok_or("Invalid Scene image selection")?;
                    images.insert(name.clone(), ImageStore::new(data_root()?).resolve(token)?);
                }
                PropertyKind::TextDisplay => {
                    return Err("Scene selection cannot change a read-only property".into());
                }
                _ => {
                    scalars.insert(name.clone(), value.clone());
                }
            }
        }
        Ok((scalars, images))
    }
    pub fn prepare_choices(
        project_dir: &Path,
        values: &BTreeMap<String, serde_json::Value>,
    ) -> Result<(BTreeMap<String, serde_json::Value>, SceneResourceChoices), String> {
        Self::split_choices(&read_project(project_dir)?, values)
    }
}
fn read_project(project_dir: &Path) -> Result<serde_json::Value, String> {
    let root = SceneService::assets_root(project_dir)?;
    let assets = SceneAssets::open(project_dir, root.as_deref()).map_err(|e| e.to_string())?;
    let bytes = assets
        .read("project.json", 4 * 1024 * 1024)
        .map_err(|e| e.to_string())?;
    serde_json::from_slice(&bytes).map_err(|_| "Invalid Scene project metadata".into())
}
pub fn valid_token(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|hash| {
        hash.len() == 64
            && hash
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    })
}
fn token(image: &ImageSource) -> String {
    let hash: String = image
        .encoded_sha256()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    format!("sha256:{hash}")
}
fn data_root() -> Result<PathBuf, String> {
    let root = std::env::var_os("XDG_DATA_HOME")
        .filter(|p| !p.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|p| PathBuf::from(p).join(".local/share")))
        .ok_or("Cannot locate the user data directory")?;
    if !root.is_absolute() {
        return Err("User data directory must be absolute".into());
    }
    Ok(root.join("lwe/scene-resources"))
}
struct ImageStore {
    root: PathBuf,
}
struct EncodedImageBytes {
    bytes: Vec<u8>,
    _lease: lwe_scene::AtlasMemoryReservation,
}
impl ImageStore {
    fn new(root: PathBuf) -> Self {
        Self { root }
    }
    fn read(path: &Path) -> Result<EncodedImageBytes, String> {
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(path)
            .map_err(|_| "Cannot open the selected Scene image")?;
        let metadata = file
            .metadata()
            .map_err(|_| "Cannot inspect the selected Scene image")?;
        if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_BYTES as u64 {
            return Err("Scene image must be a regular file within 32 MiB".into());
        }
        let length = metadata.len() as usize;
        let mut lease = scene_texture_memory_budget()
            .reserve_bytes(length + 1)
            .map_err(|e| e.to_string())?;
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(length + 1)
            .map_err(|_| "Cannot allocate Scene image input")?;
        lease.resize(bytes.capacity()).map_err(|e| e.to_string())?;
        file.take((length + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|_| "Cannot read the selected Scene image")?;
        if bytes.len() != length {
            return Err("Scene image changed while reading".into());
        }
        Ok(EncodedImageBytes {
            bytes,
            _lease: lease,
        })
    }
    fn resolve(&self, value: &str) -> Result<ImageSource, String> {
        if !valid_token(value) {
            return Err("Invalid Scene image token".into());
        }
        let path = self.root.join(&value[7..]);
        let encoded = Self::read(&path)?;
        let image = ImageSource::decode(&encoded.bytes, scene_texture_memory_budget())
            .map_err(|e| e.to_string())?;
        if token(&image) != value {
            return Err("Stored Scene image content changed".into());
        }
        Ok(image)
    }
    fn import(&self, source: &Path) -> Result<ImportedSceneImage, String> {
        let encoded = Self::read(source)?;
        let bytes = &encoded.bytes;
        let image =
            ImageSource::decode(bytes, scene_texture_memory_budget()).map_err(|e| e.to_string())?;
        let token = token(&image);
        fs::create_dir_all(&self.root).map_err(|_| "Cannot create Scene image storage")?;
        if !fs::symlink_metadata(&self.root)
            .map_err(|_| "Cannot inspect Scene image storage")?
            .is_dir()
        {
            return Err("Scene image storage must be a directory".into());
        }
        fs::set_permissions(&self.root, fs::Permissions::from_mode(0o700))
            .map_err(|_| "Cannot secure Scene image storage")?;
        let import_lock = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(self.root.join(".import.lock"))
            .map_err(|_| "Cannot lock Scene image storage")?;
        let lock_metadata = import_lock
            .metadata()
            .map_err(|_| "Cannot inspect Scene image storage lock")?;
        if !lock_metadata.is_file() || lock_metadata.len() != 0 {
            return Err("Invalid Scene image storage lock".into());
        }
        import_lock
            .lock()
            .map_err(|_| "Cannot lock Scene image storage")?;
        let target = self.root.join(&token[7..]);
        match fs::symlink_metadata(&target) {
            Ok(_) => {
                let existing = Self::read(&target)?;
                if existing.bytes != *bytes {
                    return Err("Stored Scene image conflicts with imported content".into());
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let mut count = 0;
                let mut total = 0u64;
                for entry in
                    fs::read_dir(&self.root).map_err(|_| "Cannot inspect Scene image storage")?
                {
                    let entry = entry.map_err(|_| "Cannot inspect Scene image storage")?;
                    let metadata = fs::symlink_metadata(entry.path())
                        .map_err(|_| "Cannot inspect Scene image storage")?;
                    if entry.file_name() == ".import.lock" {
                        continue;
                    }
                    if !metadata.is_file() {
                        return Err("Unexpected entry in Scene image storage".into());
                    }
                    count += 1;
                    total = total
                        .checked_add(metadata.len())
                        .ok_or("Scene image storage size overflow")?;
                }
                if count >= MAX_STORE_FILES || total + bytes.len() as u64 > MAX_STORE_BYTES {
                    return Err("Scene image storage limit reached".into());
                }
                if let Err(error) = atomic_file::atomic_create(&target, bytes)
                    && (error.kind() != std::io::ErrorKind::AlreadyExists
                        || Self::read(&target)?.bytes != *bytes)
                {
                    return Err("Cannot publish the imported Scene image".into());
                }
            }
            Err(_) => return Err("Cannot inspect stored Scene image".into()),
        }
        Ok(ImportedSceneImage {
            token,
            width: image.width(),
            height: image.height(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::io::Cursor;
    fn png() -> Vec<u8> {
        let mut bytes = Cursor::new(Vec::new());
        image::RgbaImage::from_pixel(2, 2, image::Rgba([2, 3, 4, 255]))
            .write_to(&mut bytes, image::ImageFormat::Png)
            .unwrap();
        bytes.into_inner()
    }
    #[test]
    fn text_inputs_stay_literal_and_display_metadata_cannot_be_submitted() {
        let project = json!({"general": {"properties": {
            "help": {"type": "text", "text": "Read me", "value": ""},
            "message": {"type": "textinput", "text": "Message", "value": "Author caption"}
        }}});
        for value in [json!(""), json!("sha256:".to_string() + &"a".repeat(64))] {
            let choices = BTreeMap::from([("message".to_string(), value)]);
            let (scalars, images) =
                SceneResourceService::split_choices(&project, &choices).unwrap();
            assert_eq!(scalars, choices);
            assert!(images.is_empty());
        }
        let choices = BTreeMap::from([("help".to_string(), json!(""))]);
        let error = SceneResourceService::split_choices(&project, &choices)
            .err()
            .unwrap();
        assert!(error.contains("read-only"));
    }

    #[test]
    fn imported_content_survives_source_removal_and_detects_store_tampering() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("external.png");
        fs::write(&path, png()).unwrap();
        let store = ImageStore::new(temp.path().join("store"));
        let selected = store.import(&path).unwrap();
        assert!(valid_token(&selected.token));
        let again = store.import(&path).unwrap();
        assert_eq!(selected.token, again.token);
        fs::remove_file(&path).unwrap();
        assert_eq!(store.resolve(&selected.token).unwrap().width(), 2);
        fs::write(store.root.join(&selected.token[7..]), b"bad").unwrap();
        assert!(store.resolve(&selected.token).is_err());
    }
    #[test]
    fn tokens_and_sources_reject_paths_urls_symlinks_and_invalid_images() {
        for token in [
            "",
            "../image",
            "file:///image",
            "https://image",
            "sha256:abc",
        ] {
            assert!(!valid_token(token));
        }
        let temp = tempfile::tempdir().unwrap();
        let bad = temp.path().join("bad");
        fs::write(&bad, b"not an image").unwrap();
        let store = ImageStore::new(temp.path().join("store"));
        assert!(store.import(&bad).is_err());
        assert!(!store.root.exists());
        let path = temp.path().join("real.png");
        fs::write(&path, png()).unwrap();
        let link = temp.path().join("link.png");
        std::os::unix::fs::symlink(&path, &link).unwrap();
        assert!(store.import(&link).is_err());
    }

    #[test]
    #[ignore = "private child process for the cross-process quota regression"]
    fn image_store_process_import_probe() {
        let root = PathBuf::from(std::env::var_os("LWE_IMAGE_PROBE_ROOT").expect("owned fixture"));
        let source =
            PathBuf::from(std::env::var_os("LWE_IMAGE_PROBE_SOURCE").expect("owned source"));
        let report =
            PathBuf::from(std::env::var_os("LWE_IMAGE_PROBE_REPORT").expect("owned report"));
        fs::write(report.with_extension("ready"), b"ready").unwrap();
        let result = ImageStore::new(root).import(&source);
        let outcome = result
            .map(|image| image.token)
            .unwrap_or_else(|error| error);
        fs::write(report, outcome).unwrap();
    }

    #[test]
    fn independent_processes_serialize_the_last_store_slot() {
        use std::{
            process::{Command, Stdio},
            time::{Duration, Instant},
        };
        struct OwnedChildren(Vec<std::process::Child>);
        impl Drop for OwnedChildren {
            fn drop(&mut self) {
                for child in &mut self.0 {
                    if matches!(child.try_wait(), Ok(None)) {
                        let _ = child.kill();
                    }
                    let _ = child.wait();
                }
            }
        }
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("store");
        fs::create_dir(&root).unwrap();
        for number in 0..MAX_STORE_FILES - 1 {
            fs::write(root.join(format!("{number:064x}")), b"fixture").unwrap();
        }
        let held = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(root.join(".import.lock"))
            .unwrap();
        held.lock().unwrap();
        let mut children = OwnedChildren(Vec::new());
        let mut reports = Vec::new();
        for number in 0..2 {
            let source = temp.path().join(format!("source{number}.png"));
            let mut bytes = Cursor::new(Vec::new());
            image::RgbaImage::from_pixel(2, 2, image::Rgba([number as u8, 3, 4, 255]))
                .write_to(&mut bytes, image::ImageFormat::Png)
                .unwrap();
            fs::write(&source, bytes.into_inner()).unwrap();
            let report = temp.path().join(format!("report{number}.txt"));
            children.0.push(
                Command::new(std::env::current_exe().unwrap())
                    .args([
                        "services::scene_resource_service::tests::image_store_process_import_probe",
                        "--ignored",
                        "--exact",
                        "--test-threads=1",
                    ])
                    .env("LWE_IMAGE_PROBE_ROOT", &root)
                    .env("LWE_IMAGE_PROBE_SOURCE", source)
                    .env("LWE_IMAGE_PROBE_REPORT", &report)
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                    .unwrap(),
            );
            reports.push(report);
        }
        let deadline = Instant::now() + Duration::from_secs(10);
        while !reports
            .iter()
            .all(|path| path.with_extension("ready").is_file())
        {
            assert!(
                Instant::now() < deadline,
                "owned import children did not start"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        std::thread::sleep(Duration::from_millis(100));
        assert!(
            children
                .0
                .iter_mut()
                .all(|child| child.try_wait().unwrap().is_none()),
            "imports must respect a lock held by another process"
        );
        assert!(reports.iter().all(|path| !path.exists()));
        drop(held);
        for child in &mut children.0 {
            assert!(child.wait().unwrap().success());
        }
        let results: Vec<_> = reports
            .iter()
            .map(|path| fs::read_to_string(path).unwrap())
            .collect();
        assert_eq!(results.iter().filter(|value| valid_token(value)).count(), 1);
        assert_eq!(
            results
                .iter()
                .filter(|value| *value == "Scene image storage limit reached")
                .count(),
            1
        );
        assert_eq!(fs::read_dir(root).unwrap().count(), MAX_STORE_FILES + 1);
    }
}
