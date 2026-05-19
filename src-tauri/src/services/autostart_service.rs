use std::fs;
use std::io::ErrorKind;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::{collections::HashSet, ffi::OsString};

const AUTOSTART_ENTRY_FILE_NAME: &str = "wayvid-lwe.desktop";
const AUTOSTART_ENTRY_NAME: &str = "LWE";
pub struct AutostartService;

pub struct ScopedAutostartService {
    config_root: PathBuf,
    system_config_roots: Vec<PathBuf>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum EntrySource {
    User,
    System,
}

struct EffectiveEntry {
    path: PathBuf,
    file_name: OsString,
    contents: String,
    source: EntrySource,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutostartStatus {
    pub state: AutostartState,
    pub entry_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutostartState {
    Enabled,
    Disabled,
    Unavailable { reason: String },
}

impl AutostartService {
    pub fn for_path(config_root: PathBuf) -> ScopedAutostartService {
        ScopedAutostartService {
            config_root,
            system_config_roots: default_system_config_roots(),
        }
    }

    pub fn for_test(config_root: PathBuf) -> ScopedAutostartService {
        Self::for_path(config_root)
    }

    pub fn for_test_with_system_roots(
        config_root: PathBuf,
        system_config_roots: Vec<PathBuf>,
    ) -> ScopedAutostartService {
        ScopedAutostartService {
            config_root,
            system_config_roots,
        }
    }

    #[allow(dead_code)]
    pub fn for_user_path() -> Result<ScopedAutostartService, String> {
        autostart_config_root().map(|config_root| ScopedAutostartService {
            config_root,
            system_config_roots: system_config_roots_from_env(
                std::env::var_os("XDG_CONFIG_DIRS").map(PathBuf::from),
            ),
        })
    }
}

impl ScopedAutostartService {
    pub fn entry_path(&self) -> PathBuf {
        self.config_root
            .join("autostart")
            .join(AUTOSTART_ENTRY_FILE_NAME)
    }

    pub fn status(&self, launch_command: &[&str]) -> AutostartStatus {
        let entry_path = self.entry_path();
        let state = match self.effective_entries() {
            Ok(entries) => {
                if entries
                    .iter()
                    .any(|entry| desktop_entry_is_active(&entry.contents, launch_command))
                {
                    AutostartState::Enabled
                } else {
                    AutostartState::Disabled
                }
            }
            Err(reason) => AutostartState::Unavailable { reason },
        };

        AutostartStatus { state, entry_path }
    }

    pub fn enable(&self, launch_command: &[&str]) -> Result<(), String> {
        let entry_path = self.entry_path();

        if let Some(parent) = entry_path.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                format!(
                    "Failed to create autostart directory {}: {error}",
                    parent.display()
                )
            })?;
        }

        let contents = desktop_entry_contents(launch_command)?;

        fs::write(&entry_path, contents).map_err(|error| {
            format!(
                "Failed to write autostart entry {}: {error}",
                entry_path.display()
            )
        })
    }

    pub fn disable(&self, launch_command: &[&str]) -> Result<(), String> {
        let entry_path = self.entry_path();
        let entries = self.effective_entries()?;
        let active_entries = entries
            .into_iter()
            .filter(|entry| desktop_entry_is_active(&entry.contents, launch_command))
            .collect::<Vec<_>>();

        let user_autostart_dir = self.user_autostart_dir();
        fs::create_dir_all(&user_autostart_dir).map_err(|error| {
            format!(
                "Failed to create autostart directory {}: {error}",
                user_autostart_dir.display()
            )
        })?;

        let mut shadowed_file_names = HashSet::new();

        for entry in &active_entries {
            match entry.source {
                EntrySource::User => {
                    fs::remove_file(&entry.path)
                        .or_else(|error| {
                            if error.kind() == ErrorKind::NotFound {
                                Ok(())
                            } else {
                                Err(error)
                            }
                        })
                        .map_err(|error| {
                            format!(
                                "Failed to remove autostart entry {}: {error}",
                                entry.path.display()
                            )
                        })?;
                }
                EntrySource::System => {
                    let shadow_path = user_autostart_dir.join(&entry.file_name);
                    fs::write(&shadow_path, hidden_desktop_entry_contents()).map_err(|error| {
                        format!(
                            "Failed to write autostart shadow entry {}: {error}",
                            shadow_path.display()
                        )
                    })?;
                    shadowed_file_names.insert(entry.file_name.clone());
                }
            }
        }

        if !shadowed_file_names.contains(&OsString::from(AUTOSTART_ENTRY_FILE_NAME)) {
            match fs::remove_file(&entry_path) {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
                Err(error) => Err(format!(
                    "Failed to remove autostart entry {}: {error}",
                    entry_path.display()
                )),
            }
        } else {
            Ok(())
        }
    }

    fn effective_entries(&self) -> Result<Vec<EffectiveEntry>, String> {
        let mut seen_file_names = HashSet::new();
        let mut entries = Vec::new();

        for path in self.autostart_entry_paths(&self.user_autostart_dir())? {
            let Some(file_name) = path.file_name().map(OsString::from) else {
                continue;
            };
            seen_file_names.insert(file_name.clone());
            entries.push(EffectiveEntry {
                contents: read_entry_contents(&path)?,
                path,
                file_name,
                source: EntrySource::User,
            });
        }

        for system_root in &self.system_config_roots {
            for path in self.autostart_entry_paths(&system_root.join("autostart"))? {
                let Some(file_name) = path.file_name().map(OsString::from) else {
                    continue;
                };

                if seen_file_names.insert(file_name.clone()) {
                    entries.push(EffectiveEntry {
                        contents: read_entry_contents(&path)?,
                        path,
                        file_name,
                        source: EntrySource::System,
                    });
                }
            }
        }

        Ok(entries)
    }

    fn autostart_entry_paths(&self, dir: &Path) -> Result<Vec<PathBuf>, String> {
        let mut paths = Vec::new();
        let read_dir = match fs::read_dir(dir) {
            Ok(read_dir) => read_dir,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(paths),
            Err(error) => {
                return Err(format!(
                    "Failed to read autostart directory {}: {error}",
                    dir.display()
                ));
            }
        };

        for entry in read_dir {
            let entry = entry.map_err(|error| {
                format!(
                    "Failed to read autostart directory entry {}: {error}",
                    dir.display()
                )
            })?;
            let path = entry.path();

            if path
                .extension()
                .is_some_and(|extension| extension == "desktop")
            {
                paths.push(path);
            }
        }

        paths.sort();
        Ok(paths)
    }

    fn user_autostart_dir(&self) -> PathBuf {
        self.config_root.join("autostart")
    }
}

fn read_entry_contents(path: &Path) -> Result<String, String> {
    fs::read_to_string(path)
        .map_err(|error| format!("Failed to read autostart entry {}: {error}", path.display()))
}

fn desktop_entry_contents(launch_command: &[&str]) -> Result<String, String> {
    let exec_value = desktop_entry_exec_value(launch_command)?;

    Ok(format!(
        "[Desktop Entry]\nType=Application\nName={AUTOSTART_ENTRY_NAME}\nExec={exec_value}\nTerminal=false\n"
    ))
}

fn hidden_desktop_entry_contents() -> String {
    format!("[Desktop Entry]\nType=Application\nName={AUTOSTART_ENTRY_NAME}\nHidden=true\n")
}

fn desktop_entry_exec_value(launch_command: &[&str]) -> Result<String, String> {
    if launch_command.is_empty() {
        return Err(
            "Unable to encode autostart exec line from an empty launch command".to_string(),
        );
    }

    Ok(launch_command
        .iter()
        .map(|part| desktop_entry_exec_arg(part))
        .collect::<Vec<_>>()
        .join(" "))
}

fn desktop_entry_exec_arg(argument: &str) -> String {
    let needs_quotes = argument.is_empty()
        || argument
            .chars()
            .any(|ch| ch.is_whitespace() || matches!(ch, '"' | '\\' | '`' | '$'));

    let mut escaped = String::with_capacity(argument.len());

    for ch in argument.chars() {
        match ch {
            '%' => escaped.push_str("%%"),
            '"' | '\\' | '`' | '$' => {
                escaped.push('\\');
                escaped.push(ch);
            }
            _ => escaped.push(ch),
        }
    }

    if !needs_quotes {
        return escaped;
    }

    format!("\"{escaped}\"")
}

fn desktop_entry_is_active(contents: &str, expected_launch_command: &[&str]) -> bool {
    desktop_entry_is_active_for_desktops(
        contents,
        expected_launch_command,
        &current_desktop_tokens(),
    )
}

fn desktop_entry_is_active_for_desktops(
    contents: &str,
    expected_launch_command: &[&str],
    current_desktops: &[String],
) -> bool {
    let Some(group_lines) = desktop_entry_group_lines(contents, "Desktop Entry") else {
        return false;
    };

    let mut has_application_type = false;
    let mut exec_matches = false;
    let mut hidden = false;
    let mut try_exec = None;
    let mut only_show_in = None;
    let mut not_show_in = None;
    let mut gnome_autostart_enabled = true;

    for line in group_lines {
        match line {
            "Type=Application" => has_application_type = true,
            "Hidden=true" => hidden = true,
            "X-GNOME-Autostart-enabled=false" => gnome_autostart_enabled = false,
            _ if line.starts_with("TryExec=") => {
                try_exec = Some(line["TryExec=".len()..].trim());
            }
            _ if line.starts_with("OnlyShowIn=") => {
                only_show_in = Some(parse_desktop_list(&line["OnlyShowIn=".len()..]));
            }
            _ if line.starts_with("NotShowIn=") => {
                not_show_in = Some(parse_desktop_list(&line["NotShowIn=".len()..]));
            }
            _ if line.starts_with("Exec=") => {
                exec_matches =
                    desktop_entry_exec_matches(&line["Exec=".len()..], expected_launch_command)
            }
            _ => {}
        }
    }

    let try_exec_matches = try_exec
        .map(try_exec_is_executable_in_environment)
        .unwrap_or(true);

    let gnome_autostart_allows_session =
        gnome_autostart_enabled || !is_gnome_session(current_desktops);

    let only_show_in_matches = only_show_in
        .map(|allowed| desktop_list_matches_current_session(&allowed, current_desktops))
        .unwrap_or(true);

    let not_show_in_matches = not_show_in
        .map(|blocked| desktop_list_matches_current_session(&blocked, current_desktops))
        .unwrap_or(false);

    has_application_type
        && exec_matches
        && !hidden
        && gnome_autostart_allows_session
        && try_exec_matches
        && only_show_in_matches
        && !not_show_in_matches
}

fn current_desktop_tokens() -> Vec<String> {
    std::env::var_os("XDG_CURRENT_DESKTOP")
        .map(|value| {
            value
                .to_string_lossy()
                .split(':')
                .map(str::trim)
                .filter(|part| !part.is_empty())
                .map(|part| part.to_string())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn parse_desktop_list(value: &str) -> Vec<String> {
    value
        .split(';')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(|part| part.to_string())
        .collect()
}

fn desktop_list_matches_current_session(list: &[String], current_desktops: &[String]) -> bool {
    current_desktops.iter().any(|current| {
        list.iter()
            .any(|listed| listed.eq_ignore_ascii_case(current.as_str()))
    })
}

fn is_gnome_session(current_desktops: &[String]) -> bool {
    current_desktops.iter().any(|desktop| {
        let normalized = desktop.to_ascii_lowercase();
        normalized == "gnome" || normalized.contains("gnome")
    })
}

fn try_exec_is_executable_in_environment(try_exec: &str) -> bool {
    if try_exec.is_empty() {
        return false;
    }

    let try_exec_path = Path::new(try_exec);

    if try_exec_path.components().count() > 1 || try_exec_path.is_absolute() {
        return is_executable_file(try_exec_path);
    }

    std::env::var_os("PATH")
        .map(|path| {
            std::env::split_paths(&path)
                .any(|directory| is_executable_file(&directory.join(try_exec_path)))
        })
        .unwrap_or(false)
}

fn is_executable_file(path: &Path) -> bool {
    fs::metadata(path)
        .map(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

fn desktop_entry_group_lines<'a>(contents: &'a str, group_name: &str) -> Option<Vec<&'a str>> {
    let mut in_target_group = false;
    let mut lines = Vec::new();

    for raw_line in contents.lines() {
        let line = raw_line.trim();

        if line.starts_with('[') && line.ends_with(']') {
            let header = &line[1..line.len() - 1];

            if in_target_group {
                break;
            }

            in_target_group = header == group_name;
            continue;
        }

        if in_target_group {
            lines.push(line);
        }
    }

    in_target_group.then_some(lines)
}

fn desktop_entry_exec_matches(exec_value: &str, expected_launch_command: &[&str]) -> bool {
    match parse_desktop_entry_exec_value(exec_value) {
        Ok(parsed_exec) => inherited_exec_matches(&parsed_exec, expected_launch_command),
        Err(_) => false,
    }
}

fn inherited_exec_matches(parsed_exec: &[String], expected_launch_command: &[&str]) -> bool {
    match (parsed_exec.first(), expected_launch_command.first()) {
        (Some(actual_program), Some(expected_program)) => {
            actual_program == expected_program
                && !parsed_exec
                    .iter()
                    .skip(1)
                    .any(|arg| matches!(arg.as_str(), "--help" | "--version"))
        }
        _ => false,
    }
}

fn parse_desktop_entry_exec_value(exec_value: &str) -> Result<Vec<String>, String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut chars = exec_value.chars().peekable();
    let mut in_quotes = false;

    while let Some(ch) = chars.next() {
        match ch {
            '"' => in_quotes = !in_quotes,
            '\\' => match chars.next() {
                Some(next) => current.push(next),
                None => return Err("Autostart Exec line ends with a trailing escape".to_string()),
            },
            '%' => match chars.next() {
                Some('%') => current.push('%'),
                Some(field_code) => {
                    return Err(format!(
                        "Autostart Exec line contains field code %{field_code}"
                    ));
                }
                None => return Err("Autostart Exec line ends with a trailing %".to_string()),
            },
            ch if ch.is_whitespace() && !in_quotes => {
                if !current.is_empty() {
                    parts.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(ch),
        }
    }

    if in_quotes {
        return Err("Autostart Exec line has an unclosed quote".to_string());
    }

    if !current.is_empty() {
        parts.push(current);
    }

    Ok(parts)
}

fn autostart_config_root() -> Result<PathBuf, String> {
    autostart_config_root_from_env(
        std::env::var_os("XDG_CONFIG_HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from),
        std::env::var_os("HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from),
    )
}

fn default_system_config_roots() -> Vec<PathBuf> {
    system_config_roots_from_env(std::env::var_os("XDG_CONFIG_DIRS").map(PathBuf::from))
}

fn system_config_roots_from_env(xdg_config_dirs: Option<PathBuf>) -> Vec<PathBuf> {
    match xdg_config_dirs {
        Some(value) => std::env::split_paths(&value)
            .filter(|path| path.is_absolute())
            .collect(),
        None => vec![PathBuf::from("/etc/xdg")],
    }
}

fn autostart_config_root_from_env(
    xdg_config_home: Option<PathBuf>,
    home: Option<PathBuf>,
) -> Result<PathBuf, String> {
    let config_root = xdg_config_home.or_else(|| home.map(|home| home.join(".config")));

    match config_root {
        Some(path) if path.is_absolute() => Ok(path),
        Some(path) => Err(format!(
            "Unable to resolve autostart path from non-absolute config root {}",
            path.display()
        )),
        None => Err(
            "Unable to resolve autostart path because XDG_CONFIG_HOME and HOME are unset"
                .to_string(),
        ),
    }
}

#[allow(dead_code)]
fn autostart_dir_for(config_root: &Path) -> PathBuf {
    config_root.join("autostart")
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{AutostartService, autostart_config_root_from_env};

    fn test_config_root() -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();

        std::env::temp_dir().join(format!("autostart-service-{unique}"))
    }

    #[test]
    fn autostart_service_uses_graphical_session_desktop_entry_under_config_autostart() {
        let config_root = test_config_root();
        let service = AutostartService::for_test(config_root.clone());
        let launch_command = [
            "/opt/lwe/bin/lwe",
            "--profile",
            "My Project",
            "say \"hi\"",
            "100%",
        ];

        assert_eq!(
            service.entry_path(),
            config_root.join("autostart").join("wayvid-lwe.desktop")
        );
        assert_eq!(
            service.status(&launch_command),
            super::AutostartStatus {
                state: super::AutostartState::Disabled,
                entry_path: config_root.join("autostart").join("wayvid-lwe.desktop"),
            }
        );

        service.enable(&launch_command).unwrap();

        let contents = std::fs::read_to_string(service.entry_path()).unwrap();
        assert!(contents.contains("[Desktop Entry]"));
        assert!(contents.contains("Type=Application"));
        assert!(contents.contains("Name=LWE"));
        assert!(
            contents.contains(
                "Exec=/opt/lwe/bin/lwe --profile \"My Project\" \"say \\\"hi\\\"\" 100%%"
            )
        );
        assert_eq!(
            service.status(&launch_command).state,
            super::AutostartState::Enabled
        );

        service.disable(&launch_command).unwrap();

        assert!(!service.entry_path().exists());
        assert_eq!(
            service.status(&launch_command).state,
            super::AutostartState::Disabled
        );
    }

    #[test]
    fn autostart_service_reports_enabled_when_system_entry_exists_without_user_override() {
        let config_root = test_config_root();
        let system_root = config_root.join("system");
        let service = AutostartService::for_test_with_system_roots(
            config_root.clone(),
            vec![system_root.clone()],
        );

        std::fs::create_dir_all(system_root.join("autostart")).unwrap();
        std::fs::write(
            system_root.join("autostart").join("wayvid-lwe.desktop"),
            "[Desktop Entry]\nType=Application\nExec=\"/opt/lwe/bin/lwe\" --profile \"My Project\" \"say \\\"hi\\\"\" 100%%\nTerminal=false\n",
        )
        .unwrap();

        assert_eq!(
            service
                .status(&[
                    "/opt/lwe/bin/lwe",
                    "--profile",
                    "My Project",
                    "say \"hi\"",
                    "100%",
                ])
                .state,
            super::AutostartState::Enabled
        );
    }

    #[test]
    fn autostart_service_hidden_user_shadow_disables_inherited_system_entry() {
        let config_root = test_config_root();
        let system_root = config_root.join("system");
        let service = AutostartService::for_test_with_system_roots(
            config_root.clone(),
            vec![system_root.clone()],
        );

        std::fs::create_dir_all(system_root.join("autostart")).unwrap();
        std::fs::write(
            system_root.join("autostart").join("wayvid-lwe.desktop"),
            "[Desktop Entry]\nType=Application\nExec=\"/opt/lwe/bin/lwe\" --profile \"My Project\" \"say \\\"hi\\\"\" 100%%\nTerminal=false\n",
        )
        .unwrap();

        service
            .disable(&[
                "/opt/lwe/bin/lwe",
                "--profile",
                "My Project",
                "say \"hi\"",
                "100%",
            ])
            .unwrap();

        let user_contents = std::fs::read_to_string(service.entry_path()).unwrap();
        assert!(user_contents.contains("Hidden=true"));
        assert_eq!(
            service
                .status(&[
                    "/opt/lwe/bin/lwe",
                    "--profile",
                    "My Project",
                    "say \"hi\"",
                    "100%",
                ])
                .state,
            super::AutostartState::Disabled
        );
    }

    #[test]
    fn autostart_service_does_not_shadow_inactive_inherited_system_entry() {
        let config_root = test_config_root();
        let system_root = config_root.join("system");
        let service = AutostartService::for_test_with_system_roots(
            config_root.clone(),
            vec![system_root.clone()],
        );

        std::fs::create_dir_all(system_root.join("autostart")).unwrap();
        std::fs::write(
            system_root.join("autostart").join("wayvid-lwe.desktop"),
            "[Desktop Entry]\nType=Application\nExec=\"/opt/lwe/bin/lwe\" --profile \"My Project\" \"say \\\"hi\\\"\" 100%%\nOnlyShowIn=GNOME;\nTerminal=false\n",
        )
        .unwrap();

        service
            .disable(&[
                "/opt/lwe/bin/lwe",
                "--profile",
                "My Project",
                "say \"hi\"",
                "100%",
            ])
            .unwrap();

        assert!(!service.entry_path().exists());
    }

    #[test]
    fn autostart_service_shadows_inherited_lwe_entry_with_different_flags() {
        let config_root = test_config_root();
        let system_root = config_root.join("system");
        let service = AutostartService::for_test_with_system_roots(
            config_root.clone(),
            vec![system_root.clone()],
        );

        std::fs::create_dir_all(system_root.join("autostart")).unwrap();
        std::fs::write(
            system_root.join("autostart").join("wayvid-lwe.desktop"),
            "[Desktop Entry]\nType=Application\nExec=\"/opt/lwe/bin/lwe\" --start-hidden --profile \"Other Project\"\nTerminal=false\n",
        )
        .unwrap();

        service
            .disable(&[
                "/opt/lwe/bin/lwe",
                "--profile",
                "My Project",
                "say \"hi\"",
                "100%",
            ])
            .unwrap();

        let user_contents = std::fs::read_to_string(service.entry_path()).unwrap();
        assert!(user_contents.contains("Hidden=true"));
    }

    #[test]
    fn autostart_service_shadows_inherited_lwe_entry_with_non_default_filename() {
        let config_root = test_config_root();
        let system_root = config_root.join("system");
        let service = AutostartService::for_test_with_system_roots(
            config_root.clone(),
            vec![system_root.clone()],
        );

        std::fs::create_dir_all(system_root.join("autostart")).unwrap();
        std::fs::write(
            system_root.join("autostart").join("custom-lwe.desktop"),
            "[Desktop Entry]\nType=Application\nExec=\"/opt/lwe/bin/lwe\" --start-hidden --profile \"Other Project\"\nTerminal=false\n",
        )
        .unwrap();

        service
            .disable(&[
                "/opt/lwe/bin/lwe",
                "--profile",
                "My Project",
                "say \"hi\"",
                "100%",
            ])
            .unwrap();

        let shadow_path = config_root.join("autostart").join("custom-lwe.desktop");
        let shadow_contents = std::fs::read_to_string(shadow_path).unwrap();
        assert!(shadow_contents.contains("Hidden=true"));
    }

    #[test]
    fn autostart_service_treats_equivalent_exec_with_different_name_as_enabled() {
        let config_root = test_config_root();
        let service = AutostartService::for_test(config_root);

        std::fs::create_dir_all(service.entry_path().parent().unwrap()).unwrap();
        std::fs::write(
            service.entry_path(),
            "[Desktop Entry]\nType=Application\nName=Launch Wallpaper Engine\nExec=\"/opt/lwe/bin/lwe\" --profile \"My Project\" \"say \\\"hi\\\"\" 100%%\nTerminal=false\n",
        )
        .unwrap();

        assert_eq!(
            service
                .status(&[
                    "/opt/lwe/bin/lwe",
                    "--profile",
                    "My Project",
                    "say \"hi\"",
                    "100%",
                ])
                .state,
            super::AutostartState::Enabled
        );
    }

    #[test]
    fn autostart_service_treats_user_lwe_entry_with_different_flags_as_enabled() {
        let config_root = test_config_root();
        let service = AutostartService::for_test(config_root);

        std::fs::create_dir_all(service.entry_path().parent().unwrap()).unwrap();
        std::fs::write(
            service.entry_path(),
            "[Desktop Entry]\nType=Application\nExec=\"/opt/lwe/bin/lwe\" --start-hidden --profile \"Other Project\"\nTerminal=false\n",
        )
        .unwrap();

        assert_eq!(
            service
                .status(&[
                    "/opt/lwe/bin/lwe",
                    "--profile",
                    "My Project",
                    "say \"hi\"",
                    "100%",
                ])
                .state,
            super::AutostartState::Enabled
        );
    }

    #[test]
    fn autostart_service_discovers_user_lwe_entry_with_non_default_filename() {
        let config_root = test_config_root();
        let service = AutostartService::for_test(config_root.clone());

        std::fs::create_dir_all(config_root.join("autostart")).unwrap();
        std::fs::write(
            config_root.join("autostart").join("custom-lwe.desktop"),
            "[Desktop Entry]\nType=Application\nExec=\"/opt/lwe/bin/lwe\" --start-hidden --profile \"Other Project\"\nTerminal=false\n",
        )
        .unwrap();

        assert_eq!(
            service
                .status(&[
                    "/opt/lwe/bin/lwe",
                    "--profile",
                    "My Project",
                    "say \"hi\"",
                    "100%",
                ])
                .state,
            super::AutostartState::Enabled
        );
    }

    #[test]
    fn autostart_service_treats_inherited_lwe_entry_with_different_flags_as_enabled() {
        let config_root = test_config_root();
        let system_root = config_root.join("system");
        let service = AutostartService::for_test_with_system_roots(
            config_root.clone(),
            vec![system_root.clone()],
        );

        std::fs::create_dir_all(system_root.join("autostart")).unwrap();
        std::fs::write(
            system_root.join("autostart").join("wayvid-lwe.desktop"),
            "[Desktop Entry]\nType=Application\nExec=\"/opt/lwe/bin/lwe\" --start-hidden --profile \"Other Project\"\nTerminal=false\n",
        )
        .unwrap();

        assert_eq!(
            service
                .status(&[
                    "/opt/lwe/bin/lwe",
                    "--profile",
                    "My Project",
                    "say \"hi\"",
                    "100%",
                ])
                .state,
            super::AutostartState::Enabled
        );
    }

    #[test]
    fn autostart_service_treats_inherited_lwe_help_entry_as_disabled() {
        let config_root = test_config_root();
        let system_root = config_root.join("system");
        let service = AutostartService::for_test_with_system_roots(
            config_root.clone(),
            vec![system_root.clone()],
        );

        std::fs::create_dir_all(system_root.join("autostart")).unwrap();
        std::fs::write(
            system_root.join("autostart").join("wayvid-lwe.desktop"),
            "[Desktop Entry]\nType=Application\nExec=\"/opt/lwe/bin/lwe\" --help\nTerminal=false\n",
        )
        .unwrap();

        assert_eq!(
            service
                .status(&[
                    "/opt/lwe/bin/lwe",
                    "--profile",
                    "My Project",
                    "say \"hi\"",
                    "100%",
                ])
                .state,
            super::AutostartState::Disabled
        );
    }

    #[test]
    fn autostart_service_ignores_action_group_keys_when_classifying_status() {
        let config_root = test_config_root();
        let service = AutostartService::for_test(config_root);

        std::fs::create_dir_all(service.entry_path().parent().unwrap()).unwrap();
        std::fs::write(
            service.entry_path(),
            "[Desktop Entry]\nType=Application\nName=Launch Wallpaper Engine\nExec=\"/opt/lwe/bin/lwe\" --profile \"My Project\" \"say \\\"hi\\\"\" 100%%\nTerminal=false\n\n[Desktop Action Broken]\nExec=/usr/bin/other-app\nHidden=true\n",
        )
        .unwrap();

        assert_eq!(
            service
                .status(&[
                    "/opt/lwe/bin/lwe",
                    "--profile",
                    "My Project",
                    "say \"hi\"",
                    "100%",
                ])
                .state,
            super::AutostartState::Enabled
        );
    }

    #[test]
    fn autostart_service_treats_missing_tryexec_as_disabled() {
        let config_root = test_config_root();
        let service = AutostartService::for_test(config_root.clone());
        let missing_tryexec = config_root.join("missing-lwe-bin");

        std::fs::create_dir_all(service.entry_path().parent().unwrap()).unwrap();
        std::fs::write(
            service.entry_path(),
            format!(
                "[Desktop Entry]\nType=Application\nExec=\"/opt/lwe/bin/lwe\" --profile \"My Project\" \"say \\\"hi\\\"\" 100%%\nTryExec={}\nTerminal=false\n",
                missing_tryexec.display()
            ),
        )
        .unwrap();

        assert_eq!(
            service
                .status(&[
                    "/opt/lwe/bin/lwe",
                    "--profile",
                    "My Project",
                    "say \"hi\"",
                    "100%",
                ])
                .state,
            super::AutostartState::Disabled
        );
    }

    #[test]
    fn autostart_service_treats_disabled_gnome_autostart_flag_as_disabled() {
        let config_root = test_config_root();
        let service = AutostartService::for_test(config_root);

        std::fs::create_dir_all(service.entry_path().parent().unwrap()).unwrap();
        std::fs::write(
            service.entry_path(),
            "[Desktop Entry]\nType=Application\nExec=\"/opt/lwe/bin/lwe\" --profile \"My Project\" \"say \\\"hi\\\"\" 100%%\nX-GNOME-Autostart-enabled=false\nTerminal=false\n",
        )
        .unwrap();

        assert!(!super::desktop_entry_is_active_for_desktops(
            &std::fs::read_to_string(service.entry_path()).unwrap(),
            &[
                "/opt/lwe/bin/lwe",
                "--profile",
                "My Project",
                "say \"hi\"",
                "100%",
            ],
            &["GNOME".to_string()],
        ));
    }

    #[test]
    fn autostart_service_ignores_gnome_autostart_flag_on_non_gnome_sessions() {
        let config_root = test_config_root();
        let service = AutostartService::for_test(config_root);

        std::fs::create_dir_all(service.entry_path().parent().unwrap()).unwrap();
        std::fs::write(
            service.entry_path(),
            "[Desktop Entry]\nType=Application\nExec=\"/opt/lwe/bin/lwe\" --profile \"My Project\" \"say \\\"hi\\\"\" 100%%\nX-GNOME-Autostart-enabled=false\nTerminal=false\n",
        )
        .unwrap();

        assert!(super::desktop_entry_is_active_for_desktops(
            &std::fs::read_to_string(service.entry_path()).unwrap(),
            &[
                "/opt/lwe/bin/lwe",
                "--profile",
                "My Project",
                "say \"hi\"",
                "100%",
            ],
            &["KDE".to_string()],
        ));
    }

    #[test]
    fn autostart_service_treats_only_show_in_mismatch_as_disabled() {
        let config_root = test_config_root();
        let service = AutostartService::for_test(config_root);

        std::fs::create_dir_all(service.entry_path().parent().unwrap()).unwrap();
        std::fs::write(
            service.entry_path(),
            "[Desktop Entry]\nType=Application\nExec=\"/opt/lwe/bin/lwe\" --profile \"My Project\" \"say \\\"hi\\\"\" 100%%\nOnlyShowIn=GNOME;\nTerminal=false\n",
        )
        .unwrap();

        assert!(!super::desktop_entry_is_active_for_desktops(
            &std::fs::read_to_string(service.entry_path()).unwrap(),
            &[
                "/opt/lwe/bin/lwe",
                "--profile",
                "My Project",
                "say \"hi\"",
                "100%",
            ],
            &["KDE".to_string()],
        ));
    }

    #[test]
    fn autostart_service_treats_not_show_in_match_as_disabled() {
        let config_root = test_config_root();
        let service = AutostartService::for_test(config_root);

        std::fs::create_dir_all(service.entry_path().parent().unwrap()).unwrap();
        std::fs::write(
            service.entry_path(),
            "[Desktop Entry]\nType=Application\nExec=\"/opt/lwe/bin/lwe\" --profile \"My Project\" \"say \\\"hi\\\"\" 100%%\nNotShowIn=GNOME;\nTerminal=false\n",
        )
        .unwrap();

        assert!(!super::desktop_entry_is_active_for_desktops(
            &std::fs::read_to_string(service.entry_path()).unwrap(),
            &[
                "/opt/lwe/bin/lwe",
                "--profile",
                "My Project",
                "say \"hi\"",
                "100%",
            ],
            &["GNOME".to_string(), "ubuntu:GNOME".to_string()],
        ));
    }

    #[test]
    fn autostart_service_treats_stale_desktop_entry_as_disabled() {
        let config_root = test_config_root();
        let service = AutostartService::for_test(config_root);

        std::fs::create_dir_all(service.entry_path().parent().unwrap()).unwrap();
        std::fs::write(
            service.entry_path(),
            "[Desktop Entry]\nType=Application\nName=LWE\nTerminal=false\n",
        )
        .unwrap();

        assert_eq!(
            service
                .status(&[
                    "/opt/lwe/bin/lwe",
                    "--profile",
                    "My Project",
                    "say \"hi\"",
                    "100%",
                ])
                .state,
            super::AutostartState::Disabled
        );
    }

    #[test]
    fn autostart_service_treats_hidden_desktop_entry_as_disabled() {
        let config_root = test_config_root();
        let service = AutostartService::for_test(config_root);

        std::fs::create_dir_all(service.entry_path().parent().unwrap()).unwrap();
        std::fs::write(
            service.entry_path(),
            "[Desktop Entry]\nType=Application\nName=LWE\nExec=/opt/lwe/bin/lwe\nHidden=true\nTerminal=false\n",
        )
        .unwrap();

        assert_eq!(
            service
                .status(&[
                    "/opt/lwe/bin/lwe",
                    "--profile",
                    "My Project",
                    "say \"hi\"",
                    "100%",
                ])
                .state,
            super::AutostartState::Disabled
        );
    }

    #[test]
    fn autostart_service_treats_wrong_exec_target_as_disabled() {
        let config_root = test_config_root();
        let service = AutostartService::for_test(config_root);

        std::fs::create_dir_all(service.entry_path().parent().unwrap()).unwrap();
        std::fs::write(
            service.entry_path(),
            "[Desktop Entry]\nType=Application\nName=LWE\nExec=/usr/bin/other-app\nTerminal=false\n",
        )
        .unwrap();

        assert_eq!(
            service
                .status(&[
                    "/opt/lwe/bin/lwe",
                    "--profile",
                    "My Project",
                    "say \"hi\"",
                    "100%",
                ])
                .state,
            super::AutostartState::Disabled
        );
    }

    #[test]
    fn autostart_service_reports_unavailable_when_entry_cannot_be_read() {
        let config_root = test_config_root();
        let service = AutostartService::for_test(config_root);

        std::fs::create_dir_all(service.entry_path()).unwrap();

        assert!(matches!(
            service
                .status(&[
                    "/opt/lwe/bin/lwe",
                    "--profile",
                    "My Project",
                    "say \"hi\"",
                    "100%",
                ])
                .state,
            super::AutostartState::Unavailable { .. }
        ));
    }

    #[test]
    fn autostart_service_falls_back_to_home_config_root() {
        let root =
            autostart_config_root_from_env(None, Some(std::path::PathBuf::from("/tmp/home")))
                .unwrap();

        assert_eq!(root, std::path::PathBuf::from("/tmp/home/.config"));
    }
}
