use std::fs;
use std::io::{ErrorKind, Read};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::results::desktop_persistence::{DesktopPersistenceLoad, DesktopPersistenceWrite};
use crate::results::session_persistence::{PersistedSessionState, ScenePropertyValues};
use crate::services::atomic_file::atomic_write;

static SESSION_WRITES: Mutex<()> = Mutex::new(());
const MAX_SESSION_BYTES: usize = 1024 * 1024;
const MAX_MONITORS: usize = 256;
const MAX_ITEMS_PER_MONITOR: usize = 128;
const MAX_PROPERTIES_PER_ITEM: usize = 256;
const MAX_TOTAL_PROPERTIES: usize = 16_384;
const MAX_ID_BYTES: usize = 512;
const MAX_PROPERTY_NAME_BYTES: usize = 256;
const MAX_PROPERTY_STRING_BYTES: usize = 256;

pub struct DesktopPersistenceService;

pub struct ScopedDesktopPersistenceService {
    path: PathBuf,
}

impl DesktopPersistenceService {
    pub fn load_state() -> DesktopPersistenceLoad {
        match Self::for_user_path() {
            Ok(service) => service.load_state(),
            Err(reason) => DesktopPersistenceLoad::Unavailable { reason },
        }
    }

    pub fn save_assignment(monitor_id: &str, item_id: &str) -> DesktopPersistenceWrite {
        match Self::for_user_path() {
            Ok(service) => service.save_assignment(monitor_id, item_id),
            Err(reason) => DesktopPersistenceWrite::Unavailable { reason },
        }
    }

    pub fn load_scene_properties(
        monitor_id: &str,
        item_id: &str,
    ) -> Result<ScenePropertyValues, String> {
        Self::for_user_path()?.load_scene_properties(monitor_id, item_id)
    }

    pub fn save_assignment_with_properties(
        monitor_id: &str,
        item_id: &str,
        values: &ScenePropertyValues,
    ) -> DesktopPersistenceWrite {
        match Self::for_user_path() {
            Ok(service) => service.save_assignment_with_properties(monitor_id, item_id, values),
            Err(reason) => DesktopPersistenceWrite::Unavailable { reason },
        }
    }

    pub fn clear_assignment(monitor_id: &str) -> DesktopPersistenceWrite {
        match Self::for_user_path() {
            Ok(service) => service.clear_assignment(monitor_id),
            Err(reason) => DesktopPersistenceWrite::Unavailable { reason },
        }
    }

    pub fn for_user_path() -> Result<ScopedDesktopPersistenceService, String> {
        session_state_path().map(Self::for_path)
    }

    pub fn for_path(path: PathBuf) -> ScopedDesktopPersistenceService {
        ScopedDesktopPersistenceService { path }
    }

    pub fn for_test(path: PathBuf) -> ScopedDesktopPersistenceService {
        Self::for_path(path)
    }

    fn load_session_at_path(path: &Path) -> Result<PersistedSessionState, String> {
        let file = match fs::File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == ErrorKind::NotFound => {
                return Ok(PersistedSessionState::default());
            }
            Err(error) => {
                return Err(format!(
                    "Failed to read desktop session from {}: {error}",
                    path.display()
                ));
            }
        };
        let metadata = file.metadata().map_err(|error| {
            format!(
                "Failed to inspect desktop session {}: {error}",
                path.display()
            )
        })?;
        if !metadata.is_file() || metadata.len() > MAX_SESSION_BYTES as u64 {
            return Err(format!(
                "Invalid desktop session size or file type at {}",
                path.display()
            ));
        }
        let mut contents = String::new();
        file.take(MAX_SESSION_BYTES as u64 + 1)
            .read_to_string(&mut contents)
            .map_err(|error| {
                format!(
                    "Failed to read desktop session from {}: {error}",
                    path.display()
                )
            })?;
        if contents.len() > MAX_SESSION_BYTES {
            return Err(format!(
                "Desktop session exceeds size limit at {}",
                path.display()
            ));
        }
        // TOML diagnostics can include the offending source line and value.
        let state = toml::from_str::<PersistedSessionState>(&contents)
            .map_err(|_| format!("Failed to parse desktop session from {}", path.display()))?;
        validate_session(&state)?;
        Ok(state)
    }

    fn load_at_path(path: &Path) -> DesktopPersistenceLoad {
        match Self::load_session_at_path(path) {
            Ok(state) => DesktopPersistenceLoad::Loaded(state.assignments),
            Err(reason) => DesktopPersistenceLoad::Unavailable { reason },
        }
    }

    fn save_at_path_with(
        path: &Path,
        state: &PersistedSessionState,
        write: impl FnOnce(&Path, &[u8]) -> std::io::Result<()>,
    ) -> DesktopPersistenceWrite {
        if let Err(reason) = validate_session(state) {
            return DesktopPersistenceWrite::Unavailable { reason };
        }
        let contents = match toml::to_string(state) {
            Ok(contents) if contents.len() <= MAX_SESSION_BYTES => contents,
            Ok(_) => {
                return DesktopPersistenceWrite::Unavailable {
                    reason: "Desktop session exceeds size limit".to_string(),
                };
            }
            Err(_) => {
                return DesktopPersistenceWrite::Unavailable {
                    reason: format!("Failed to serialize desktop session for {}", path.display()),
                };
            }
        };
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
            && let Err(error) = fs::create_dir_all(parent)
        {
            return DesktopPersistenceWrite::Unavailable {
                reason: format!(
                    "Failed to create desktop assignments directory {}: {error}",
                    parent.display()
                ),
            };
        }

        match write(path, contents.as_bytes()) {
            Ok(()) => DesktopPersistenceWrite::Saved,
            Err(error) => DesktopPersistenceWrite::Unavailable {
                reason: format!(
                    "Failed to atomically save desktop assignments to {}: {error}",
                    path.display()
                ),
            },
        }
    }

    fn save_assignment_at_path(
        path: &Path,
        monitor_id: &str,
        item_id: &str,
        values: Option<&ScenePropertyValues>,
    ) -> DesktopPersistenceWrite {
        Self::save_assignment_at_path_with(path, monitor_id, item_id, values, atomic_write)
    }

    fn save_assignment_at_path_with(
        path: &Path,
        monitor_id: &str,
        item_id: &str,
        values: Option<&ScenePropertyValues>,
        write: impl FnOnce(&Path, &[u8]) -> std::io::Result<()>,
    ) -> DesktopPersistenceWrite {
        if let Err(reason) = validate_identity(monitor_id, item_id)
            .and_then(|()| values.map_or(Ok(()), validate_choices))
        {
            return DesktopPersistenceWrite::Unavailable { reason };
        }
        let mut state = match Self::load_session_at_path(path) {
            Ok(state) => state,
            Err(reason) => {
                return DesktopPersistenceWrite::Unavailable { reason };
            }
        };
        state
            .assignments
            .insert(monitor_id.to_string(), item_id.to_string());
        if let Some(values) = values {
            if values.is_empty() {
                if let Some(items) = state.scene_properties.get_mut(monitor_id) {
                    items.remove(item_id);
                    if items.is_empty() {
                        state.scene_properties.remove(monitor_id);
                    }
                }
            } else {
                state
                    .scene_properties
                    .entry(monitor_id.to_string())
                    .or_default()
                    .insert(item_id.to_string(), values.clone());
            }
        }
        Self::save_at_path_with(path, &state, write)
    }

    fn clear_at_path(path: &Path, monitor_id: &str) -> DesktopPersistenceWrite {
        if !valid_key(monitor_id, MAX_ID_BYTES) {
            return DesktopPersistenceWrite::Unavailable {
                reason: "Invalid desktop monitor identity".to_string(),
            };
        }
        let mut state = match Self::load_session_at_path(path) {
            Ok(state) => state,
            Err(reason) => {
                return DesktopPersistenceWrite::Unavailable { reason };
            }
        };

        state.assignments.remove(monitor_id);

        let result = if state.assignments.is_empty() && state.scene_properties.is_empty() {
            match fs::remove_file(path) {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
                Err(error) => Err(format!(
                    "Failed to clear desktop assignments at {}: {error}",
                    path.display()
                )),
            }
        } else {
            match Self::save_at_path_with(path, &state, atomic_write) {
                DesktopPersistenceWrite::Saved => Ok(()),
                DesktopPersistenceWrite::Cleared => Ok(()),
                DesktopPersistenceWrite::Unavailable { reason } => Err(reason),
            }
        };

        match result {
            Ok(()) => DesktopPersistenceWrite::Cleared,
            Err(reason) => DesktopPersistenceWrite::Unavailable { reason },
        }
    }
}

fn valid_key(key: &str, max_bytes: usize) -> bool {
    !key.is_empty() && key.len() <= max_bytes && !key.contains('\0')
}

fn validate_identity(monitor_id: &str, item_id: &str) -> Result<(), String> {
    if valid_key(monitor_id, MAX_ID_BYTES) && valid_key(item_id, MAX_ID_BYTES) {
        Ok(())
    } else {
        Err("Invalid desktop monitor or item identity".to_string())
    }
}

fn normalized_channel(value: &serde_json::Value) -> bool {
    value
        .as_f64()
        .is_some_and(|v| v.is_finite() && (0.0..=1.0).contains(&v))
}

fn valid_choice(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Bool(_) => true,
        serde_json::Value::Number(number) => number.as_f64().is_some_and(f64::is_finite),
        serde_json::Value::Array(channels) => {
            channels.len() == 3 && channels.iter().all(normalized_channel)
        }
        // The project declaration validates string values before apply.
        // An empty string can clear a text field.
        serde_json::Value::String(value) => {
            value.len() <= MAX_PROPERTY_STRING_BYTES && !value.contains('\0')
        }
        _ => false,
    }
}

fn validate_choices(values: &ScenePropertyValues) -> Result<(), String> {
    if values.len() > MAX_PROPERTIES_PER_ITEM
        || values
            .iter()
            .any(|(name, value)| !valid_key(name, MAX_PROPERTY_NAME_BYTES) || !valid_choice(value))
    {
        return Err("Invalid or oversized Scene property choices".to_string());
    }
    Ok(())
}

fn validate_session(state: &PersistedSessionState) -> Result<(), String> {
    if state.assignments.len() > MAX_MONITORS || state.scene_properties.len() > MAX_MONITORS {
        return Err("Desktop session monitor count exceeds limit".to_string());
    }
    for (monitor, item) in &state.assignments {
        validate_identity(monitor, item)?;
    }
    let mut total_properties = 0;
    for (monitor, items) in &state.scene_properties {
        if !valid_key(monitor, MAX_ID_BYTES) || items.len() > MAX_ITEMS_PER_MONITOR {
            return Err("Invalid or oversized Scene property item selection".to_string());
        }
        for (item, choices) in items {
            validate_identity(monitor, item)?;
            validate_choices(choices)?;
            total_properties += choices.len();
            if total_properties > MAX_TOTAL_PROPERTIES {
                return Err("Desktop session property count exceeds limit".to_string());
            }
        }
    }
    Ok(())
}

fn session_state_path_from_env(
    xdg_config_home: Option<PathBuf>,
    home: Option<PathBuf>,
) -> Result<PathBuf, String> {
    let base = xdg_config_home.or_else(|| home.map(|home| home.join(".config")));

    match base {
        Some(path) if path.is_absolute() => Ok(path.join("lwe").join("session.toml")),
        Some(path) => Err(format!(
            "Unable to resolve desktop persistence path from non-absolute config root {}",
            path.display()
        )),
        None => Err(
            "Unable to resolve desktop persistence path because XDG_CONFIG_HOME and HOME are unset"
                .to_string(),
        ),
    }
}

fn session_state_path() -> Result<PathBuf, String> {
    session_state_path_from_env(
        std::env::var_os("XDG_CONFIG_HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from),
        std::env::var_os("HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from),
    )
}

impl ScopedDesktopPersistenceService {
    pub fn load_state(&self) -> DesktopPersistenceLoad {
        DesktopPersistenceService::load_at_path(&self.path)
    }

    pub fn save_assignment(&self, monitor_id: &str, item_id: &str) -> DesktopPersistenceWrite {
        let Ok(_write_guard) = SESSION_WRITES.lock() else {
            return DesktopPersistenceWrite::Unavailable {
                reason: "Desktop assignment persistence lock was poisoned".to_string(),
            };
        };
        DesktopPersistenceService::save_assignment_at_path(&self.path, monitor_id, item_id, None)
    }

    pub fn load_scene_properties(
        &self,
        monitor_id: &str,
        item_id: &str,
    ) -> Result<ScenePropertyValues, String> {
        validate_identity(monitor_id, item_id)?;
        let state = DesktopPersistenceService::load_session_at_path(&self.path)?;
        Ok(state
            .scene_properties
            .get(monitor_id)
            .and_then(|items| items.get(item_id))
            .cloned()
            .unwrap_or_default())
    }

    /// Save the assignment and this monitor/item's complete choice map in one
    /// atomic session replacement. An empty map restores authored defaults.
    pub fn save_assignment_with_properties(
        &self,
        monitor_id: &str,
        item_id: &str,
        values: &ScenePropertyValues,
    ) -> DesktopPersistenceWrite {
        let Ok(_write_guard) = SESSION_WRITES.lock() else {
            return DesktopPersistenceWrite::Unavailable {
                reason: "Desktop assignment persistence lock was poisoned".to_string(),
            };
        };
        DesktopPersistenceService::save_assignment_at_path(
            &self.path,
            monitor_id,
            item_id,
            Some(values),
        )
    }

    pub fn clear_assignment(&self, monitor_id: &str) -> DesktopPersistenceWrite {
        let Ok(_write_guard) = SESSION_WRITES.lock() else {
            return DesktopPersistenceWrite::Unavailable {
                reason: "Desktop assignment persistence lock was poisoned".to_string(),
            };
        };
        DesktopPersistenceService::clear_at_path(&self.path, monitor_id)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::ffi::OsString;
    use std::path::PathBuf;
    use std::sync::MutexGuard;
    use std::time::{SystemTime, UNIX_EPOCH};

    use crate::results::desktop_persistence::{DesktopPersistenceLoad, DesktopPersistenceWrite};
    use crate::results::session_persistence::{PersistedSessionState, ScenePropertyValues};
    use serde_json::json;

    use super::{DesktopPersistenceService, session_state_path_from_env};

    struct EnvGuard {
        xdg_config_home: Option<OsString>,
        home: Option<OsString>,
        _lock: MutexGuard<'static, ()>,
    }

    impl EnvGuard {
        fn set_config_root(config_root: &std::path::Path) -> Self {
            let lock = env_lock();
            let xdg_config_home = std::env::var_os("XDG_CONFIG_HOME");
            let home = std::env::var_os("HOME");

            unsafe {
                std::env::set_var("XDG_CONFIG_HOME", config_root);
                std::env::set_var("HOME", config_root);
            }

            Self {
                xdg_config_home,
                home,
                _lock: lock,
            }
        }

        fn set_home_only(home_root: &std::path::Path) -> Self {
            let lock = env_lock();
            let xdg_config_home = std::env::var_os("XDG_CONFIG_HOME");
            let home = std::env::var_os("HOME");

            unsafe {
                std::env::remove_var("XDG_CONFIG_HOME");
                std::env::set_var("HOME", home_root);
            }

            Self {
                xdg_config_home,
                home,
                _lock: lock,
            }
        }
    }

    fn env_lock() -> MutexGuard<'static, ()> {
        crate::test_env::env_lock()
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.xdg_config_home {
                Some(value) => unsafe { std::env::set_var("XDG_CONFIG_HOME", value) },
                None => unsafe { std::env::remove_var("XDG_CONFIG_HOME") },
            }

            match &self.home {
                Some(value) => unsafe { std::env::set_var("HOME", value) },
                None => unsafe { std::env::remove_var("HOME") },
            }
        }
    }

    fn test_state_path() -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();

        std::env::temp_dir().join(format!("desktop-persistence-service-{unique}.toml"))
    }

    #[test]
    fn desktop_assignment_persistence_round_trips_through_toml() {
        let path = test_state_path();
        let service = DesktopPersistenceService::for_test(path.clone());

        assert!(matches!(
            service.save_assignment("eDP-1", "item-1"),
            DesktopPersistenceWrite::Saved
        ));

        let persisted = std::fs::read_to_string(&path).unwrap();
        let parsed: toml::Value = toml::from_str(&persisted).unwrap();
        assert_eq!(parsed["assignments"]["eDP-1"].as_str(), Some("item-1"));

        let loaded = service.load_state();
        assert!(matches!(
            loaded,
            DesktopPersistenceLoad::Loaded(assignments)
                if assignments.get("eDP-1") == Some(&"item-1".to_string())
        ));
    }

    #[test]
    fn concurrent_monitor_saves_preserve_all_assignments() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("session.toml");
        let barrier = std::sync::Barrier::new(12);
        std::thread::scope(|scope| {
            for index in 0..12 {
                let path = &path;
                let barrier = &barrier;
                scope.spawn(move || {
                    barrier.wait();
                    assert!(matches!(
                        DesktopPersistenceService::for_path(path.clone()).save_assignment(
                            &format!("DISPLAY-{index}"),
                            &format!("video-{index}"),
                        ),
                        DesktopPersistenceWrite::Saved
                    ));
                });
            }
        });
        let DesktopPersistenceLoad::Loaded(assignments) =
            DesktopPersistenceService::for_path(path.clone()).load_state()
        else {
            panic!("all assignments should remain readable");
        };
        assert_eq!(assignments.len(), 12);
        for index in 0..12 {
            assert_eq!(
                assignments[&format!("DISPLAY-{index}")],
                format!("video-{index}")
            );
        }
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
    }

    #[test]
    fn persistence_service_round_trips_assignments() {
        let path = test_state_path();
        let service = DesktopPersistenceService::for_test(path.clone());

        assert!(matches!(
            service.load_state(),
            crate::results::desktop_persistence::DesktopPersistenceLoad::Loaded(_)
        ));

        assert!(matches!(
            service.save_assignment("eDP-1", "item-1"),
            crate::results::desktop_persistence::DesktopPersistenceWrite::Saved
        ));

        let persisted = std::fs::read_to_string(&path).unwrap();
        let parsed: toml::Value = toml::from_str(&persisted).unwrap();
        assert_eq!(parsed["assignments"]["eDP-1"].as_str(), Some("item-1"));

        let loaded = service.load_state();
        match loaded {
            crate::results::desktop_persistence::DesktopPersistenceLoad::Loaded(assignments) => {
                assert_eq!(assignments.get("eDP-1").unwrap(), "item-1");
            }
            _ => panic!("expected loaded assignments"),
        }
    }

    #[test]
    fn default_service_methods_use_user_session_path() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let config_root =
            std::env::temp_dir().join(format!("desktop-persistence-default-service-{unique}"));
        let _guard = EnvGuard::set_config_root(&config_root);

        assert!(matches!(
            DesktopPersistenceService::load_state(),
            DesktopPersistenceLoad::Loaded(assignments) if assignments.is_empty()
        ));

        assert!(matches!(
            DesktopPersistenceService::save_assignment("eDP-1", "item-1"),
            DesktopPersistenceWrite::Saved
        ));

        assert!(matches!(
            DesktopPersistenceService::load_state(),
            DesktopPersistenceLoad::Loaded(assignments)
                if assignments.get("eDP-1") == Some(&"item-1".to_string())
        ));

        assert!(matches!(
            DesktopPersistenceService::clear_assignment("eDP-1"),
            DesktopPersistenceWrite::Cleared
        ));
    }

    #[test]
    fn load_state_returns_empty_assignments_when_file_is_missing() {
        let path = test_state_path();
        let service = DesktopPersistenceService::for_test(path);

        let result = service.load_state();

        assert!(matches!(
            result,
            DesktopPersistenceLoad::Loaded(assignments) if assignments.is_empty()
        ));
    }

    #[test]
    fn clear_assignment_removes_saved_monitor_assignment() {
        let path = test_state_path();
        let service = DesktopPersistenceService::for_test(path);

        assert!(matches!(
            service.save_assignment("DISPLAY-1", "wallpaper-1"),
            DesktopPersistenceWrite::Saved
        ));

        let result = service.clear_assignment("DISPLAY-1");

        assert!(matches!(result, DesktopPersistenceWrite::Cleared));

        assert!(matches!(
            service.load_state(),
            DesktopPersistenceLoad::Loaded(assignments) if assignments == BTreeMap::new()
        ));
    }

    #[test]
    fn load_state_returns_unavailable_for_invalid_toml() {
        let path = test_state_path();
        std::fs::write(&path, "not toml").unwrap();

        let service = DesktopPersistenceService::for_test(path);
        let result = service.load_state();

        assert!(matches!(
            result,
            DesktopPersistenceLoad::Unavailable { reason }
                if !reason.is_empty()
        ));
    }

    #[test]
    fn session_path_uses_lwe_config_root() {
        let path = session_state_path_from_env(
            Some(PathBuf::from("/tmp/config")),
            Some(PathBuf::from("/tmp/home")),
        )
        .unwrap();

        assert_eq!(path, PathBuf::from("/tmp/config/lwe/session.toml"));
    }

    #[test]
    fn session_path_falls_back_to_home_config_root() {
        let path = session_state_path_from_env(None, Some(PathBuf::from("/tmp/home"))).unwrap();

        assert_eq!(path, PathBuf::from("/tmp/home/.config/lwe/session.toml"));
    }

    #[test]
    fn session_path_rejects_relative_config_roots() {
        let result = session_state_path_from_env(Some(PathBuf::from("relative")), None);

        assert!(matches!(result, Err(reason) if reason.contains("non-absolute config root")));
    }

    #[test]
    fn default_service_uses_home_fallback_when_xdg_config_home_is_unset() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let home_root =
            std::env::temp_dir().join(format!("desktop-persistence-home-fallback-{unique}"));
        let _guard = EnvGuard::set_home_only(&home_root);

        assert!(matches!(
            DesktopPersistenceService::save_assignment("eDP-1", "item-1"),
            DesktopPersistenceWrite::Saved
        ));

        let persisted =
            std::fs::read_to_string(home_root.join(".config/lwe/session.toml")).unwrap();
        let parsed: toml::Value = toml::from_str(&persisted).unwrap();
        assert_eq!(parsed["assignments"]["eDP-1"].as_str(), Some("item-1"));
    }

    fn choices() -> ScenePropertyValues {
        BTreeMap::from([
            ("enabled".to_string(), json!(true)),
            ("speed".to_string(), json!(1.25)),
            ("color".to_string(), json!([0.25, 0.5, 1.0])),
            ("legacy_color".to_string(), json!("0.125 0.25 0.5")),
            ("selection".to_string(), json!("second")),
        ])
    }

    #[test]
    fn legacy_assignments_load_with_empty_scene_properties() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("session.toml");
        std::fs::write(&path, "[assignments]\neDP-1 = 'old-video'\n").unwrap();
        let service = DesktopPersistenceService::for_path(path.clone());
        assert!(
            service
                .load_scene_properties("eDP-1", "scene-1")
                .unwrap()
                .is_empty()
        );
        assert!(matches!(
            service.save_assignment("HDMI-1", "other-video"),
            DesktopPersistenceWrite::Saved
        ));
        let state: PersistedSessionState =
            toml::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        assert_eq!(state.assignments.len(), 2);
        assert!(state.scene_properties.is_empty());
        let missing = DesktopPersistenceService::for_path(directory.path().join("missing.toml"));
        assert!(
            missing
                .load_scene_properties("eDP-1", "scene-1")
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn assignment_and_choices_round_trip_in_the_same_session_file() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("session.toml");
        let service = DesktopPersistenceService::for_path(path.clone());
        let values = choices();
        assert!(matches!(
            service.save_assignment_with_properties("DISPLAY-1", "scene-1", &values),
            DesktopPersistenceWrite::Saved
        ));
        assert_eq!(
            service
                .load_scene_properties("DISPLAY-1", "scene-1")
                .unwrap(),
            values
        );
        let state: PersistedSessionState =
            toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(state.assignments["DISPLAY-1"], "scene-1");
        assert_eq!(state.scene_properties["DISPLAY-1"]["scene-1"], values);
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
    }

    #[test]
    fn grouped_control_values_round_trip_without_persisting_headers() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("session.toml");
        let service = DesktopPersistenceService::for_path(path.clone());
        let project = json!({"general":{"properties":{
            "sectionHeader":{"type":"group","text":"ELEMENTS","value":"","order":101},
            "enabled":{"type":"bool","value":true,"order":102},
            "opacity":{"type":"slider","value":0.25,"order":103}
        }}});
        let groups = lwe_engine::scene::property_groups(&project).unwrap();
        let values: ScenePropertyValues = lwe_engine::scene::property_definitions(&project)
            .unwrap()
            .into_iter()
            .map(|control| (control.name, serde_json::to_value(control.value).unwrap()))
            .collect();
        assert_eq!(values.len(), 2);
        assert!(!values.contains_key("sectionHeader"));
        assert!(matches!(
            service.save_assignment_with_properties("DISPLAY", "scene", &values),
            DesktopPersistenceWrite::Saved
        ));
        let bytes = std::fs::read_to_string(&path).unwrap();
        assert!(!bytes.contains("sectionHeader"));
        assert!(!bytes.contains("ELEMENTS"));
        let saved = service.load_scene_properties("DISPLAY", "scene").unwrap();
        assert_eq!(saved, values);
        let mut restored = project.clone();
        lwe_engine::scene::apply_property_overrides(&mut restored, &saved).unwrap();
        assert_eq!(restored, project);
        assert_eq!(
            lwe_engine::scene::property_groups(&restored).unwrap(),
            groups
        );
        assert!(matches!(
            service.save_assignment_with_properties("DISPLAY", "scene", &BTreeMap::new()),
            DesktopPersistenceWrite::Saved
        ));
        assert!(
            service
                .load_scene_properties("DISPLAY", "scene")
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            lwe_engine::scene::property_groups(&project).unwrap(),
            groups
        );
    }

    #[test]
    fn ordinary_assignment_save_and_last_clear_preserve_all_choices() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("session.toml");
        let service = DesktopPersistenceService::for_path(path.clone());
        let values = choices();
        for (monitor, item) in [
            ("DISPLAY-1", "scene-1"),
            ("DISPLAY-1", "scene-2"),
            ("DISPLAY-2", "scene-1"),
        ] {
            assert!(matches!(
                service.save_assignment_with_properties(monitor, item, &values),
                DesktopPersistenceWrite::Saved
            ));
        }
        assert!(matches!(
            service.save_assignment("DISPLAY-1", "video-1"),
            DesktopPersistenceWrite::Saved
        ));
        assert!(matches!(
            service.clear_assignment("DISPLAY-1"),
            DesktopPersistenceWrite::Cleared
        ));
        assert!(matches!(
            service.clear_assignment("DISPLAY-2"),
            DesktopPersistenceWrite::Cleared
        ));
        assert!(path.is_file());
        let state = DesktopPersistenceService::load_session_at_path(&path).unwrap();
        assert!(state.assignments.is_empty());
        for (monitor, item) in [
            ("DISPLAY-1", "scene-1"),
            ("DISPLAY-1", "scene-2"),
            ("DISPLAY-2", "scene-1"),
        ] {
            assert_eq!(state.scene_properties[monitor][item], values);
        }
    }

    #[test]
    fn empty_choice_map_resets_only_the_selected_monitor_and_item() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("session.toml");
        let service = DesktopPersistenceService::for_path(path.clone());
        let values = choices();
        for (monitor, item) in [
            ("DISPLAY-1", "scene-1"),
            ("DISPLAY-1", "scene-2"),
            ("DISPLAY-2", "scene-1"),
        ] {
            assert!(matches!(
                service.save_assignment_with_properties(monitor, item, &values),
                DesktopPersistenceWrite::Saved
            ));
        }
        assert!(matches!(
            service.save_assignment_with_properties("DISPLAY-1", "scene-1", &BTreeMap::new()),
            DesktopPersistenceWrite::Saved
        ));
        let state = DesktopPersistenceService::load_session_at_path(&path).unwrap();
        assert!(!state.scene_properties["DISPLAY-1"].contains_key("scene-1"));
        assert_eq!(state.scene_properties["DISPLAY-1"]["scene-2"], values);
        assert_eq!(state.scene_properties["DISPLAY-2"]["scene-1"], values);
        assert!(matches!(
            service.save_assignment_with_properties("DISPLAY-1", "scene-2", &BTreeMap::new()),
            DesktopPersistenceWrite::Saved
        ));
        let state = DesktopPersistenceService::load_session_at_path(&path).unwrap();
        assert!(!state.scene_properties.contains_key("DISPLAY-1"));
        assert_eq!(state.assignments["DISPLAY-1"], "scene-2");
        assert_eq!(state.assignments["DISPLAY-2"], "scene-1");
    }

    #[test]
    fn concurrent_save_and_clear_keep_other_assignments_and_property_selections() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("session.toml");
        let service = DesktopPersistenceService::for_path(path.clone());
        assert!(matches!(
            service.save_assignment_with_properties("CLEAR", "scene-old", &choices()),
            DesktopPersistenceWrite::Saved
        ));
        let barrier = std::sync::Barrier::new(13);
        std::thread::scope(|scope| {
            for index in 0..12 {
                let path = &path;
                let barrier = &barrier;
                scope.spawn(move || {
                    barrier.wait();
                    let service = DesktopPersistenceService::for_path(path.clone());
                    assert!(matches!(
                        service.save_assignment_with_properties(
                            &format!("DISPLAY-{index}"),
                            "scene",
                            &choices()
                        ),
                        DesktopPersistenceWrite::Saved
                    ));
                    assert!(matches!(
                        service.save_assignment(&format!("DISPLAY-{index}"), "video"),
                        DesktopPersistenceWrite::Saved
                    ));
                });
            }
            let barrier = &barrier;
            scope.spawn(|| {
                barrier.wait();
                assert!(matches!(
                    service.clear_assignment("CLEAR"),
                    DesktopPersistenceWrite::Cleared
                ));
            });
        });
        let state = DesktopPersistenceService::load_session_at_path(&path).unwrap();
        assert_eq!(state.assignments.len(), 12);
        assert!(!state.assignments.contains_key("CLEAR"));
        assert_eq!(state.scene_properties.len(), 13);
        for index in 0..12 {
            let monitor = format!("DISPLAY-{index}");
            assert_eq!(state.assignments[&monitor], "video");
            assert_eq!(state.scene_properties[&monitor]["scene"], choices());
        }
        assert_eq!(state.scene_properties["CLEAR"]["scene-old"], choices());
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
    }

    #[test]
    fn corrupt_or_oversized_state_is_redacted_and_never_overwritten() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("session.toml");
        let service = DesktopPersistenceService::for_path(path.clone());
        for contents in [
            "[assignments]\neDP-1 = 'DO-NOT-PRINT'.broken\n".to_string(),
            "[scene_properties.DISPLAY.item]\nsecret = { nested = 'DO-NOT-PRINT' }\n".to_string(),
            format!(
                "[scene_properties.DISPLAY.item]\nsecret = 'DO-NOT-PRINT{}'\n",
                "x".repeat(super::MAX_PROPERTY_STRING_BYTES)
            ),
            " ".repeat(super::MAX_SESSION_BYTES + 1),
        ] {
            std::fs::write(&path, &contents).unwrap();
            let reason = service
                .load_scene_properties("DISPLAY", "item")
                .unwrap_err();
            assert!(!reason.contains("DO-NOT-PRINT"));
            assert!(matches!(
                service.load_state(),
                DesktopPersistenceLoad::Unavailable { .. }
            ));
            assert!(matches!(
                service.save_assignment("DISPLAY", "video"),
                DesktopPersistenceWrite::Unavailable { .. }
            ));
            assert!(matches!(
                service.save_assignment_with_properties("DISPLAY", "scene", &choices()),
                DesktopPersistenceWrite::Unavailable { .. }
            ));
            assert!(matches!(
                service.clear_assignment("DISPLAY"),
                DesktopPersistenceWrite::Unavailable { .. }
            ));
            assert_eq!(std::fs::read(&path).unwrap(), contents.as_bytes());
        }
    }

    #[test]
    fn literal_text_choices_round_trip_including_an_empty_value() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("session.toml");
        let service = DesktopPersistenceService::for_path(path.clone());
        let values = BTreeMap::from([
            ("cleared".to_string(), json!("")),
            ("caption".to_string(), json!(" 作者文字\n<caption> ")),
            ("boundary".to_string(), json!("🌙".repeat(64))),
        ]);
        assert!(matches!(
            service.save_assignment_with_properties("DISPLAY", "scene", &values),
            DesktopPersistenceWrite::Saved
        ));
        assert_eq!(
            DesktopPersistenceService::for_path(path)
                .load_scene_properties("DISPLAY", "scene")
                .unwrap(),
            values
        );
    }

    #[test]
    fn invalid_choices_or_serialized_size_keep_previous_session_bytes() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("session.toml");
        let service = DesktopPersistenceService::for_path(path.clone());
        assert!(matches!(
            service.save_assignment_with_properties("DISPLAY", "scene", &choices()),
            DesktopPersistenceWrite::Saved
        ));
        let previous = std::fs::read(&path).unwrap();
        for value in [
            json!(null),
            json!({"nested":true}),
            json!([0.5, 0.5]),
            json!([0.5, 0.5, 2.0]),
            json!([0.5, true, 1.0]),
            json!("x".repeat(super::MAX_PROPERTY_STRING_BYTES + 1)),
            json!("invalid\0choice"),
        ] {
            let values = BTreeMap::from([("property".to_string(), value)]);
            assert!(matches!(
                service.save_assignment_with_properties("DISPLAY", "scene", &values),
                DesktopPersistenceWrite::Unavailable { .. }
            ));
            assert_eq!(std::fs::read(&path).unwrap(), previous);
        }
        let too_many = (0..=super::MAX_PROPERTIES_PER_ITEM)
            .map(|i| (format!("p{i}"), json!(true)))
            .collect();
        assert!(matches!(
            service.save_assignment_with_properties("DISPLAY", "scene", &too_many),
            DesktopPersistenceWrite::Unavailable { .. }
        ));
        let mut oversized = DesktopPersistenceService::load_session_at_path(&path).unwrap();
        let values = (0..super::MAX_PROPERTIES_PER_ITEM)
            .map(|i| (format!("{i:0256}"), json!("0.1 0.2 0.3")))
            .collect::<ScenePropertyValues>();
        for index in 0..16 {
            oversized
                .scene_properties
                .entry("DISPLAY".to_string())
                .or_default()
                .insert(format!("scene-{index}"), values.clone());
        }
        assert!(matches!(
            DesktopPersistenceService::save_at_path_with(
                &path,
                &oversized,
                crate::services::atomic_file::atomic_write
            ),
            DesktopPersistenceWrite::Unavailable { .. }
        ));
        assert_eq!(std::fs::read(&path).unwrap(), previous);
    }

    #[test]
    fn atomic_writer_failure_keeps_both_assignment_and_choices_previous_bytes() {
        use std::io::Write;
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("session.toml");
        let service = DesktopPersistenceService::for_path(path.clone());
        assert!(matches!(
            service.save_assignment_with_properties("DISPLAY", "scene-old", &choices()),
            DesktopPersistenceWrite::Saved
        ));
        let previous = std::fs::read(&path).unwrap();
        let next = BTreeMap::from([("enabled".to_string(), json!(false))]);
        let result = DesktopPersistenceService::save_assignment_at_path_with(
            &path,
            "DISPLAY",
            "scene-new",
            Some(&next),
            |path, contents| {
                let prepared: PersistedSessionState =
                    toml::from_str(std::str::from_utf8(contents).unwrap()).unwrap();
                assert_eq!(prepared.assignments["DISPLAY"], "scene-new");
                assert_eq!(prepared.scene_properties["DISPLAY"]["scene-new"], next);
                assert_eq!(prepared.scene_properties["DISPLAY"]["scene-old"], choices());
                // Fault at the atomic-writer boundary after a private partial
                // temporary write. Common atomic_file tests cover its writer
                // and replacement failure paths with the real utility.
                let mut temporary = tempfile::NamedTempFile::new_in(path.parent().unwrap())?;
                temporary.write_all(&contents[..5])?;
                Err(std::io::ErrorKind::StorageFull.into())
            },
        );
        assert!(matches!(
            result,
            DesktopPersistenceWrite::Unavailable { .. }
        ));
        assert_eq!(std::fs::read(&path).unwrap(), previous);
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
    }
}
