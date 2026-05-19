use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use crate::results::settings_persistence::{
    PersistedSettings, SettingsPersistenceLoad, SettingsPersistenceWrite,
};

pub struct SettingsPersistenceService;

pub struct ScopedSettingsPersistenceService {
    path: PathBuf,
}

impl SettingsPersistenceService {
    pub fn for_path(path: PathBuf) -> ScopedSettingsPersistenceService {
        ScopedSettingsPersistenceService { path }
    }

    #[allow(dead_code)]
    pub fn for_user_path() -> Result<ScopedSettingsPersistenceService, String> {
        settings_path().map(Self::for_path)
    }

    pub fn for_test(path: PathBuf) -> ScopedSettingsPersistenceService {
        Self::for_path(path)
    }
}

impl ScopedSettingsPersistenceService {
    pub fn load_settings(&self) -> SettingsPersistenceLoad {
        match fs::read_to_string(&self.path) {
            Ok(contents) => match toml::from_str::<PersistedSettings>(&contents) {
                Ok(settings) => SettingsPersistenceLoad::Loaded(settings),
                Err(reason) => SettingsPersistenceLoad::Unavailable {
                    reason: format!(
                        "Failed to parse settings from {}: {reason}",
                        self.path.display()
                    ),
                },
            },
            Err(error) if error.kind() == ErrorKind::NotFound => {
                SettingsPersistenceLoad::Loaded(PersistedSettings::default())
            }
            Err(error) => SettingsPersistenceLoad::Unavailable {
                reason: format!(
                    "Failed to read settings from {}: {error}",
                    self.path.display()
                ),
            },
        }
    }

    pub fn save_settings(&self, settings: &PersistedSettings) -> SettingsPersistenceWrite {
        if let Some(parent) = self.path.parent() {
            if let Err(error) = fs::create_dir_all(parent) {
                return SettingsPersistenceWrite::Unavailable {
                    reason: format!(
                        "Failed to create settings directory {}: {error}",
                        parent.display()
                    ),
                };
            }
        }

        let contents = match toml::to_string(settings) {
            Ok(contents) => contents,
            Err(error) => {
                return SettingsPersistenceWrite::Unavailable {
                    reason: format!(
                        "Failed to serialize settings for {}: {error}",
                        self.path.display()
                    ),
                };
            }
        };

        let temp_path = atomic_write_path_for(&self.path);

        if let Err(error) = fs::write(&temp_path, contents) {
            return SettingsPersistenceWrite::Unavailable {
                reason: format!(
                    "Failed to write temporary settings file {}: {error}",
                    temp_path.display()
                ),
            };
        }

        match fs::rename(&temp_path, &self.path) {
            Ok(()) => SettingsPersistenceWrite::Saved,
            Err(error) => {
                let _ = fs::remove_file(&temp_path);

                SettingsPersistenceWrite::Unavailable {
                    reason: format!(
                        "Failed to atomically replace settings at {}: {error}",
                        self.path.display()
                    ),
                }
            }
        }
    }
}

fn atomic_write_path_for(path: &Path) -> PathBuf {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("settings.toml");

    path.with_file_name(format!(".{file_name}.tmp"))
}

#[allow(dead_code)]
fn settings_path_from_env(
    xdg_config_home: Option<PathBuf>,
    home: Option<PathBuf>,
) -> Result<PathBuf, String> {
    let base = xdg_config_home.or_else(|| home.map(|home| home.join(".config")));

    match base {
        Some(path) if path.is_absolute() => Ok(path.join("lwe").join("settings.toml")),
        Some(path) => Err(format!(
            "Unable to resolve settings path from non-absolute config root {}",
            path.display()
        )),
        None => Err(
            "Unable to resolve settings path because XDG_CONFIG_HOME and HOME are unset"
                .to_string(),
        ),
    }
}

#[allow(dead_code)]
fn settings_path() -> Result<PathBuf, String> {
    settings_path_from_env(
        std::env::var_os("XDG_CONFIG_HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from),
        std::env::var_os("HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from),
    )
}

impl ScopedSettingsPersistenceService {
    #[allow(dead_code)]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use crate::models::{WorkshopAgeRating, WorkshopOnlineItemType};
    use crate::results::settings_persistence::{PersistedSettings, SettingsPersistenceLoad};

    use super::{SettingsPersistenceService, atomic_write_path_for, settings_path_from_env};

    fn test_settings_path() -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();

        std::env::temp_dir().join(format!("settings-persistence-service-{unique}.toml"))
    }

    #[test]
    fn settings_persistence_returns_defaults_when_file_is_missing() {
        let path = test_settings_path();
        let service = SettingsPersistenceService::for_test(path);

        let loaded = service.load_settings();

        assert_eq!(
            loaded,
            SettingsPersistenceLoad::Loaded(PersistedSettings::default())
        );
    }

    #[test]
    fn settings_path_uses_lwe_config_root() {
        let path = settings_path_from_env(
            Some(PathBuf::from("/tmp/config")),
            Some(PathBuf::from("/tmp/home")),
        )
        .unwrap();

        assert_eq!(path, PathBuf::from("/tmp/config/lwe/settings.toml"));
    }

    #[test]
    fn settings_persistence_loads_real_toml_with_inline_comments() {
        let path = test_settings_path();
        std::fs::write(
            &path,
            "language = \"en\" # user language\ntheme = \"dark\"\nlaunch_on_login = true\nsteam_web_api_key = \"abc123\"\nworkshop_query = \"rain\"\nworkshop_age_ratings = [\"g\", \"pg_13\"]\nworkshop_item_types = [\"video\", \"application\"]\n",
        )
        .unwrap();
        let service = SettingsPersistenceService::for_test(path);

        let loaded = service.load_settings();

        assert_eq!(
            loaded,
            SettingsPersistenceLoad::Loaded(PersistedSettings {
                language: "en".to_string(),
                theme: "dark".to_string(),
                launch_on_login: true,
                steam_web_api_key: "abc123".to_string(),
                workshop_query: "rain".to_string(),
                workshop_age_ratings: vec![WorkshopAgeRating::G, WorkshopAgeRating::Pg13],
                workshop_item_types: vec![
                    WorkshopOnlineItemType::Video,
                    WorkshopOnlineItemType::Application,
                ],
            })
        );
    }

    #[test]
    fn settings_persistence_loads_partial_toml_with_defaults_for_missing_fields() {
        let path = test_settings_path();
        std::fs::write(&path, "language = \"fr\"\n").unwrap();
        let service = SettingsPersistenceService::for_test(path);

        let loaded = service.load_settings();

        assert_eq!(
            loaded,
            SettingsPersistenceLoad::Loaded(PersistedSettings {
                language: "fr".to_string(),
                theme: "system".to_string(),
                launch_on_login: false,
                steam_web_api_key: String::new(),
                workshop_query: String::new(),
                workshop_age_ratings: vec![WorkshopAgeRating::G, WorkshopAgeRating::Pg13],
                workshop_item_types: vec![
                    WorkshopOnlineItemType::Video,
                    WorkshopOnlineItemType::Scene,
                    WorkshopOnlineItemType::Web,
                    WorkshopOnlineItemType::Application,
                ],
            })
        );
    }

    #[test]
    fn settings_persistence_round_trips_mvp_settings_through_toml_file() {
        let path = test_settings_path();
        let service = SettingsPersistenceService::for_test(path.clone());
        let settings = PersistedSettings {
            language: "en".to_string(),
            theme: "dark".to_string(),
            launch_on_login: true,
            steam_web_api_key: "key-123".to_string(),
            workshop_query: "forest".to_string(),
            workshop_age_ratings: vec![WorkshopAgeRating::G, WorkshopAgeRating::R18],
            workshop_item_types: vec![
                WorkshopOnlineItemType::Scene,
                WorkshopOnlineItemType::Application,
            ],
        };

        assert!(matches!(
            service.save_settings(&settings),
            crate::results::settings_persistence::SettingsPersistenceWrite::Saved
        ));

        let contents = std::fs::read_to_string(&path).unwrap();
        assert!(contents.contains("language = \"en\""));
        assert!(contents.contains("theme = \"dark\""));
        assert!(contents.contains("launch_on_login = true"));
        assert!(contents.contains("steam_web_api_key = \"key-123\""));
        assert!(contents.contains("workshop_query = \"forest\""));
        assert!(contents.contains("workshop_age_ratings = [\"g\", \"r_18\"]"));
        assert!(contents.contains("workshop_item_types = [\"scene\", \"application\"]"));

        let loaded = service.load_settings();

        assert_eq!(loaded, SettingsPersistenceLoad::Loaded(settings));
    }

    #[test]
    fn settings_persistence_atomic_save_cleans_up_temp_file() {
        let path = test_settings_path();
        let service = SettingsPersistenceService::for_test(path.clone());
        let temp_path = atomic_write_path_for(&path);

        assert!(matches!(
            service.save_settings(&PersistedSettings::default()),
            crate::results::settings_persistence::SettingsPersistenceWrite::Saved
        ));

        assert!(!temp_path.exists());
    }
}
