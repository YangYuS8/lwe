use std::collections::BTreeMap;
use std::fs;
use std::io::ErrorKind;
use std::path::PathBuf;

use crate::results::desktop_persistence::{DesktopPersistenceLoad, DesktopPersistenceWrite};
use crate::results::session_persistence::PersistedSessionState;

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

    fn load_at_path(path: &std::path::Path) -> DesktopPersistenceLoad {
        match fs::read_to_string(path) {
            Ok(contents) => match toml::from_str::<PersistedSessionState>(&contents) {
                Ok(state) => DesktopPersistenceLoad::Loaded(state.assignments),
                Err(reason) => DesktopPersistenceLoad::Unavailable {
                    reason: format!(
                        "Failed to parse desktop assignments from {}: {reason}",
                        path.display()
                    ),
                },
            },
            Err(error) if error.kind() == ErrorKind::NotFound => {
                DesktopPersistenceLoad::Loaded(BTreeMap::new())
            }
            Err(error) => DesktopPersistenceLoad::Unavailable {
                reason: format!(
                    "Failed to read desktop assignments from {}: {error}",
                    path.display()
                ),
            },
        }
    }

    fn save_at_path(
        path: &std::path::Path,
        assignments: &BTreeMap<String, String>,
    ) -> DesktopPersistenceWrite {
        if let Some(parent) = path.parent() {
            if let Err(error) = fs::create_dir_all(parent) {
                return DesktopPersistenceWrite::Unavailable {
                    reason: format!(
                        "Failed to create desktop assignments directory {}: {error}",
                        parent.display()
                    ),
                };
            }
        }

        let contents = match toml::to_string(&PersistedSessionState {
            assignments: assignments.clone(),
        }) {
            Ok(contents) => contents,
            Err(error) => {
                return DesktopPersistenceWrite::Unavailable {
                    reason: format!(
                        "Failed to serialize desktop assignments for {}: {error}",
                        path.display()
                    ),
                };
            }
        };

        match fs::write(path, contents) {
            Ok(()) => DesktopPersistenceWrite::Saved,
            Err(error) => DesktopPersistenceWrite::Unavailable {
                reason: format!(
                    "Failed to write desktop assignments to {}: {error}",
                    path.display()
                ),
            },
        }
    }

    fn save_assignment_at_path(
        path: &std::path::Path,
        monitor_id: &str,
        item_id: &str,
    ) -> DesktopPersistenceWrite {
        let mut assignments = match Self::load_at_path(path) {
            DesktopPersistenceLoad::Loaded(assignments) => assignments,
            DesktopPersistenceLoad::Unavailable { reason } => {
                return DesktopPersistenceWrite::Unavailable { reason };
            }
        };

        assignments.insert(monitor_id.to_string(), item_id.to_string());
        Self::save_at_path(path, &assignments)
    }

    fn clear_at_path(path: &std::path::Path, monitor_id: &str) -> DesktopPersistenceWrite {
        let mut assignments = match Self::load_at_path(path) {
            DesktopPersistenceLoad::Loaded(assignments) => assignments,
            DesktopPersistenceLoad::Unavailable { reason } => {
                return DesktopPersistenceWrite::Unavailable { reason };
            }
        };

        assignments.remove(monitor_id);

        let result = if assignments.is_empty() {
            match fs::remove_file(path) {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
                Err(error) => Err(format!(
                    "Failed to clear desktop assignments at {}: {error}",
                    path.display()
                )),
            }
        } else {
            match Self::save_at_path(path, &assignments) {
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
        DesktopPersistenceService::save_assignment_at_path(&self.path, monitor_id, item_id)
    }

    pub fn clear_assignment(&self, monitor_id: &str) -> DesktopPersistenceWrite {
        DesktopPersistenceService::clear_at_path(&self.path, monitor_id)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::ffi::OsString;
    use std::path::PathBuf;
    use std::sync::{Mutex, MutexGuard, OnceLock};
    use std::time::{SystemTime, UNIX_EPOCH};

    use crate::results::desktop_persistence::{DesktopPersistenceLoad, DesktopPersistenceWrite};

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
        static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

        ENV_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .expect("desktop persistence env lock was poisoned")
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
}
