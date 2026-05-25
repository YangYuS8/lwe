use std::path::Path;

use lwe_library::{SteamLibrary, WALLPAPER_ENGINE_APP_ID};

use crate::models::SettingsUpdateInput;
use crate::results::settings_persistence::{
    PersistedSettings, SettingsPersistenceLoad, SettingsPersistenceWrite,
};
use crate::services::autostart_service::{AutostartService, AutostartState};
use crate::services::settings_persistence_service::SettingsPersistenceService;

#[derive(Debug)]
pub(crate) struct SettingsPageData {
    pub language: String,
    pub theme: String,
    pub launch_on_login: bool,
    pub launch_on_login_available: bool,
    pub steam_web_api_key: String,
    pub workshop_query: String,
    pub workshop_age_ratings: Vec<crate::models::WorkshopAgeRating>,
    pub workshop_item_types: Vec<crate::models::WorkshopOnlineItemType>,
    pub steam_required: bool,
    pub steam_status_message: String,
    pub stale: bool,
}

pub struct SettingsService;

impl SettingsService {
    pub(crate) fn load_page() -> Result<SettingsPageData, String> {
        let persistence = SettingsPersistenceService::for_user_path()?;
        let settings = match persistence.load_settings() {
            SettingsPersistenceLoad::Loaded(settings) => settings,
            SettingsPersistenceLoad::Unavailable { reason } => return Err(reason),
        };

        let autostart = match AutostartService::for_user_path() {
            Ok(service) => match current_launch_command() {
                Ok(command) => service.status(&command_refs(&command)).state,
                Err(reason) => AutostartState::Unavailable { reason },
            },
            Err(reason) => AutostartState::Unavailable { reason },
        };

        Ok(Self::build_page_data(settings, autostart))
    }

    pub(crate) fn update_settings(input: SettingsUpdateInput) -> Result<SettingsPageData, String> {
        let persistence = SettingsPersistenceService::for_user_path()?;
        let previous_settings = match persistence.load_settings() {
            SettingsPersistenceLoad::Loaded(settings) => settings,
            SettingsPersistenceLoad::Unavailable { reason } => return Err(reason),
        };
        let mut settings = previous_settings.clone();

        if let Some(language) = input.language {
            settings.language = language;
        }

        if let Some(theme) = input.theme {
            settings.theme = theme;
        }

        if let Some(launch_on_login) = input.launch_on_login {
            settings.launch_on_login = launch_on_login;
        }

        if let Some(steam_web_api_key) = input.steam_web_api_key {
            settings.steam_web_api_key = steam_web_api_key;
        }

        if let Some(workshop_query) = input.workshop_query {
            settings.workshop_query = workshop_query;
        }

        if let Some(workshop_age_ratings) = input.workshop_age_ratings {
            settings.workshop_age_ratings = workshop_age_ratings;
        }

        if let Some(workshop_item_types) = input.workshop_item_types {
            settings.workshop_item_types = workshop_item_types;
        }

        match persistence.save_settings(&settings) {
            SettingsPersistenceWrite::Saved => {
                if let Some(launch_on_login) = input.launch_on_login {
                    if let Err(reason) = Self::apply_launch_on_login(launch_on_login) {
                        return match persistence.save_settings(&previous_settings) {
                            SettingsPersistenceWrite::Saved => Err(reason),
                            SettingsPersistenceWrite::Unavailable {
                                reason: rollback_reason,
                            } => Err(format!(
                                "{reason}; failed to roll back settings persistence: {rollback_reason}"
                            )),
                        };
                    }
                }

                Self::load_page()
            }
            SettingsPersistenceWrite::Unavailable { reason } => Err(reason),
        }
    }

    fn apply_launch_on_login(enabled: bool) -> Result<(), String> {
        let autostart = AutostartService::for_user_path()?;
        let command = current_launch_command()?;
        let command_refs = command_refs(&command);

        if enabled {
            autostart.enable(&command_refs)
        } else {
            autostart.disable(&command_refs)
        }
    }

    fn build_page_data(settings: PersistedSettings, autostart: AutostartState) -> SettingsPageData {
        let (launch_on_login, launch_on_login_available) = match autostart {
            AutostartState::Enabled => (true, true),
            AutostartState::Disabled => (false, true),
            AutostartState::Unavailable { .. } => (settings.launch_on_login, false),
        };

        let (steam_required, steam_status_message) =
            steam_status_with_discovery(SteamLibrary::try_discover());

        SettingsPageData {
            language: settings.language,
            theme: settings.theme,
            launch_on_login,
            launch_on_login_available,
            steam_web_api_key: settings.steam_web_api_key,
            workshop_query: settings.workshop_query,
            workshop_age_ratings: settings.workshop_age_ratings,
            workshop_item_types: settings.workshop_item_types,
            steam_required,
            steam_status_message,
            stale: !launch_on_login_available,
        }
    }

    #[cfg(test)]
    fn load_page_for_test(
        settings_path: std::path::PathBuf,
        config_root: std::path::PathBuf,
    ) -> Result<SettingsPageData, String> {
        let settings = match SettingsPersistenceService::for_test(settings_path).load_settings() {
            SettingsPersistenceLoad::Loaded(settings) => settings,
            SettingsPersistenceLoad::Unavailable { reason } => return Err(reason),
        };
        let command = vec![test_launch_command()];
        let autostart = AutostartService::for_test(config_root)
            .status(&command_refs(&command))
            .state;

        Ok(Self::build_page_data(settings, autostart))
    }

    #[cfg(test)]
    fn update_settings_for_test(
        settings_path: std::path::PathBuf,
        config_root: std::path::PathBuf,
        input: SettingsUpdateInput,
    ) -> Result<SettingsPageData, String> {
        let persistence = SettingsPersistenceService::for_test(settings_path.clone());
        let previous_settings = match persistence.load_settings() {
            SettingsPersistenceLoad::Loaded(settings) => settings,
            SettingsPersistenceLoad::Unavailable { reason } => return Err(reason),
        };
        let mut settings = previous_settings.clone();

        if let Some(language) = input.language {
            settings.language = language;
        }

        if let Some(theme) = input.theme {
            settings.theme = theme;
        }

        if let Some(launch_on_login) = input.launch_on_login {
            settings.launch_on_login = launch_on_login;
        }

        if let Some(steam_web_api_key) = input.steam_web_api_key {
            settings.steam_web_api_key = steam_web_api_key;
        }

        if let Some(workshop_query) = input.workshop_query {
            settings.workshop_query = workshop_query;
        }

        if let Some(workshop_age_ratings) = input.workshop_age_ratings {
            settings.workshop_age_ratings = workshop_age_ratings;
        }

        if let Some(workshop_item_types) = input.workshop_item_types {
            settings.workshop_item_types = workshop_item_types;
        }

        match persistence.save_settings(&settings) {
            SettingsPersistenceWrite::Saved => {
                if let Some(launch_on_login) = input.launch_on_login {
                    let autostart = AutostartService::for_test(config_root.clone());
                    let command = vec![test_launch_command()];
                    let command_refs = command_refs(&command);
                    let apply_result = if launch_on_login {
                        autostart.enable(&command_refs)
                    } else {
                        autostart.disable(&command_refs)
                    };

                    if let Err(reason) = apply_result {
                        return match persistence.save_settings(&previous_settings) {
                            SettingsPersistenceWrite::Saved => Err(reason),
                            SettingsPersistenceWrite::Unavailable {
                                reason: rollback_reason,
                            } => Err(format!(
                                "{reason}; failed to roll back settings persistence: {rollback_reason}"
                            )),
                        };
                    }
                }

                Self::load_page_for_test(settings_path, config_root)
            }
            SettingsPersistenceWrite::Unavailable { reason } => Err(reason),
        }
    }
}

fn steam_status_with_discovery(steam: Option<SteamLibrary>) -> (bool, String) {
    match steam {
        Some(steam) => steam_status_for_library(&steam),
        None => (
            true,
            "Steam was not detected. Steam is required for Workshop subscription and local Workshop synchronization. Online search also needs a Steam Web API key in Settings."
                .to_string(),
        ),
    }
}

fn steam_status_for_library(steam: &SteamLibrary) -> (bool, String) {
    let workshop_paths = steam.workshop_content_path(WALLPAPER_ENGINE_APP_ID);
    if workshop_paths.is_empty() {
        return (
            true,
            "Steam is available, but Wallpaper Engine Workshop content was not found. Install Wallpaper Engine in Steam and subscribe to wallpapers before refreshing local Workshop content."
                .to_string(),
        );
    }

    if !workshop_paths
        .iter()
        .any(|path| path_has_child_directory(path))
    {
        return (
            true,
            "Steam and Wallpaper Engine are available, but no local Wallpaper Engine Workshop content was found yet. Subscribe in Steam, wait for sync, then refresh the Workshop catalog."
                .to_string(),
        );
    }

    (
        true,
        "Steam, Wallpaper Engine, and local Workshop content are available. Online search still uses the saved Steam Web API key and does not imply local synchronization."
            .to_string(),
    )
}

fn path_has_child_directory(path: &Path) -> bool {
    std::fs::read_dir(path)
        .map(|entries| {
            entries.filter_map(Result::ok).any(|entry| {
                entry
                    .file_type()
                    .map(|file_type| file_type.is_dir())
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false)
}

fn current_launch_command() -> Result<Vec<String>, String> {
    let path = std::env::current_exe()
        .map_err(|error| format!("Unable to resolve current executable for autostart: {error}"))?;

    Ok(vec![
        path_to_command_part(&path)?,
        "--start-hidden".to_string(),
    ])
}

#[cfg(test)]
fn test_launch_command() -> String {
    "/opt/lwe/bin/lwe".to_string()
}

fn path_to_command_part(path: &std::path::Path) -> Result<String, String> {
    path.to_str().map(str::to_string).ok_or_else(|| {
        format!(
            "Unable to use non-UTF-8 executable path {} for autostart",
            path.display()
        )
    })
}

fn command_refs(command: &[String]) -> Vec<&str> {
    command.iter().map(String::as_str).collect()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use crate::models::SettingsUpdateInput;
    use crate::services::autostart_service::{AutostartService, AutostartState};

    use super::{SettingsService, steam_status_with_discovery};

    fn unique_test_path(prefix: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();

        std::env::temp_dir().join(format!("{prefix}-{unique}"))
    }

    #[test]
    fn settings_service_load_page_reflects_persisted_settings_and_autostart_state() {
        let settings_path = unique_test_path("settings-service").with_extension("toml");
        let config_root = unique_test_path("settings-service-config");

        std::fs::write(
            &settings_path,
            "language = \"en\"\ntheme = \"dark\"\nlaunch_on_login = false\n",
        )
        .unwrap();

        let autostart = AutostartService::for_test(config_root.clone());
        autostart.enable(&["/opt/lwe/bin/lwe"]).unwrap();

        let result = SettingsService::load_page_for_test(settings_path, config_root).unwrap();

        assert_eq!(result.language, "en");
        assert_eq!(result.theme, "dark");
        assert!(result.launch_on_login);
        assert!(result.launch_on_login_available);
        assert!(result.steam_required);
        assert!(!result.stale);
        assert!(result.steam_status_message.contains("Steam"));
    }

    #[test]
    fn settings_service_uses_persisted_launch_preference_when_autostart_is_unavailable() {
        let result = SettingsService::build_page_data(
            crate::results::settings_persistence::PersistedSettings {
                language: "system".to_string(),
                theme: "system".to_string(),
                launch_on_login: true,
                steam_web_api_key: "api-key".to_string(),
                workshop_query: "nature".to_string(),
                workshop_age_ratings: vec![crate::models::WorkshopAgeRating::G],
                workshop_item_types: vec![crate::models::WorkshopOnlineItemType::Video],
            },
            AutostartState::Unavailable {
                reason: "missing XDG config root".to_string(),
            },
        );

        assert!(result.launch_on_login);
        assert!(!result.launch_on_login_available);
        assert!(result.stale);
        assert_eq!(result.steam_web_api_key, "api-key");
        assert_eq!(result.workshop_query, "nature");
        assert_eq!(result.workshop_age_ratings.len(), 1);
        assert_eq!(result.workshop_item_types.len(), 1);
    }

    #[test]
    fn steam_status_distinguishes_missing_steam_without_network_or_api_calls() {
        let (_required, message) = steam_status_with_discovery(None);

        assert!(message.contains("Steam was not detected"));
        assert!(message.contains("Steam Web API key"));
    }

    #[test]
    fn steam_status_distinguishes_wallpaper_engine_missing_from_empty_content() {
        let root = unique_test_path("settings-steam-root");
        std::fs::create_dir_all(root.join("steamapps")).unwrap();

        let (_required, missing_engine_message) =
            steam_status_with_discovery(Some(lwe_library::SteamLibrary {
                root: root.clone(),
                libraries: Vec::new(),
            }));

        assert!(missing_engine_message.contains("Wallpaper Engine Workshop content was not found"));

        let workshop_root = root
            .join("steamapps/workshop/content")
            .join(lwe_library::WALLPAPER_ENGINE_APP_ID.to_string());
        std::fs::create_dir_all(&workshop_root).unwrap();

        let (_required, empty_content_message) =
            steam_status_with_discovery(Some(lwe_library::SteamLibrary {
                root,
                libraries: Vec::new(),
            }));

        assert!(empty_content_message.contains("no local Wallpaper Engine Workshop content"));

        std::fs::create_dir_all(workshop_root.join("123456")).unwrap();
        let populated_root = workshop_root
            .ancestors()
            .nth(4)
            .expect("test root should be available")
            .to_path_buf();
        let (_required, ready_message) =
            steam_status_with_discovery(Some(lwe_library::SteamLibrary {
                root: populated_root,
                libraries: Vec::new(),
            }));

        assert!(ready_message.contains("local Workshop content are available"));
        assert!(ready_message.contains("does not imply local synchronization"));
    }

    #[test]
    fn settings_service_update_persists_values_and_updates_autostart() {
        let settings_path = unique_test_path("settings-update").with_extension("toml");
        let config_root = unique_test_path("settings-update-config");

        let result = SettingsService::update_settings_for_test(
            settings_path.clone(),
            config_root.clone(),
            SettingsUpdateInput {
                language: Some("fr".to_string()),
                theme: Some("dark".to_string()),
                launch_on_login: Some(true),
                steam_web_api_key: Some("test-key".to_string()),
                workshop_query: Some("forest".to_string()),
                workshop_age_ratings: Some(vec![
                    crate::models::WorkshopAgeRating::G,
                    crate::models::WorkshopAgeRating::Pg13,
                ]),
                workshop_item_types: Some(vec![
                    crate::models::WorkshopOnlineItemType::Video,
                    crate::models::WorkshopOnlineItemType::Application,
                ]),
            },
        )
        .unwrap();

        assert_eq!(result.language, "fr");
        assert_eq!(result.theme, "dark");
        assert!(result.launch_on_login);
        assert_eq!(result.steam_web_api_key, "test-key");
        assert_eq!(result.workshop_query, "forest");

        let contents = std::fs::read_to_string(settings_path).unwrap();
        assert!(contents.contains("language = \"fr\""));
        assert!(contents.contains("theme = \"dark\""));
        assert!(contents.contains("launch_on_login = true"));
        assert!(contents.contains("steam_web_api_key = \"test-key\""));
        assert!(contents.contains("workshop_query = \"forest\""));

        assert_eq!(
            AutostartService::for_test(config_root)
                .status(&["/opt/lwe/bin/lwe"])
                .state,
            AutostartState::Enabled
        );
    }

    #[test]
    fn settings_service_returns_error_when_persisted_settings_are_unreadable() {
        let settings_path = unique_test_path("settings-invalid").with_extension("toml");
        let config_root = unique_test_path("settings-invalid-config");

        std::fs::write(&settings_path, "language = [broken\n").unwrap();

        let error = SettingsService::load_page_for_test(settings_path.clone(), config_root)
            .expect_err("invalid settings should not be surfaced as defaults");

        assert!(error.contains("Failed to parse settings from"));
        assert!(error.contains(&settings_path.display().to_string()));
    }

    #[test]
    fn settings_service_does_not_change_autostart_when_save_fails() {
        let blocked_parent = unique_test_path("settings-blocked-parent");
        let settings_path = blocked_parent.join("settings.toml");
        let config_root = unique_test_path("settings-save-failure-config");

        std::fs::write(&blocked_parent, "not a directory").unwrap();

        let error = SettingsService::update_settings_for_test(
            settings_path,
            config_root.clone(),
            SettingsUpdateInput {
                language: None,
                theme: None,
                launch_on_login: Some(true),
                steam_web_api_key: None,
                workshop_query: None,
                workshop_age_ratings: None,
                workshop_item_types: None,
            },
        )
        .expect_err("save failure should abort settings update");

        assert!(error.contains("Failed"));
        assert_eq!(
            AutostartService::for_test(config_root)
                .status(&["/opt/lwe/bin/lwe"])
                .state,
            AutostartState::Disabled
        );
    }

    #[test]
    fn settings_service_rolls_back_persisted_settings_when_autostart_update_fails() {
        let settings_path = unique_test_path("settings-rollback").with_extension("toml");
        let blocked_config_root = unique_test_path("settings-rollback-config");

        std::fs::write(
            &settings_path,
            "language = \"en\"\ntheme = \"system\"\nlaunch_on_login = false\n",
        )
        .unwrap();
        std::fs::write(&blocked_config_root, "not a directory").unwrap();

        let error = SettingsService::update_settings_for_test(
            settings_path.clone(),
            blocked_config_root,
            SettingsUpdateInput {
                language: Some("fr".to_string()),
                theme: None,
                launch_on_login: Some(true),
                steam_web_api_key: Some("new-key".to_string()),
                workshop_query: Some("new-query".to_string()),
                workshop_age_ratings: Some(vec![crate::models::WorkshopAgeRating::R18]),
                workshop_item_types: Some(vec![crate::models::WorkshopOnlineItemType::Web]),
            },
        )
        .expect_err("autostart failure should roll back saved settings");

        assert!(error.contains("Failed to create autostart directory"));

        let contents = std::fs::read_to_string(settings_path).unwrap();
        assert!(contents.contains("language = \"en\""));
        assert!(contents.contains("theme = \"system\""));
        assert!(contents.contains("launch_on_login = false"));
        assert!(!contents.contains("steam_web_api_key = \"new-key\""));
        assert!(!contents.contains("workshop_query = \"new-query\""));
    }
}
