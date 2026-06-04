use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use lwe_engine::{EngineCommand, EngineConfig, EngineEvent, EngineHandle, spawn_engine};
use lwe_library::{WeProject, WorkshopProjectType};

use crate::results::desktop::{
    DesktopApplyResult, DesktopPageResult, DesktopResolvedMonitorAssignment,
};
use crate::results::desktop_persistence::{DesktopPersistenceLoad, DesktopPersistenceWrite};
use crate::results::library::LibraryProjection;
use crate::results::monitor_discovery::MonitorDiscoveryResult;
use crate::services::desktop_persistence_service::DesktopPersistenceService;
use crate::services::library_service::LibraryService;
use crate::services::monitor_service::MonitorService;

pub(crate) const LIBRARY_RESOLUTION_ISSUE_PREFIX: &str =
    "Unable to resolve desktop items against the current Library snapshot:";
const REAL_APPLY_BACKEND: &str = "lwe_engine_wayland";
const REAL_APPLY_BACKEND_TIMEOUT: Duration = Duration::from_secs(5);
const BACKEND_NOT_RUNNING_CLEAR_WARNING: &str =
    "Desktop runtime was not running; cleared the saved assignment only";

struct RunningDesktopApplyBackend {
    handle: EngineHandle,
    events: Receiver<EngineEvent>,
}

static DESKTOP_APPLY_BACKEND: OnceLock<Mutex<Option<RunningDesktopApplyBackend>>> = OnceLock::new();
static STARTUP_RESTORE_ISSUES: OnceLock<Mutex<Vec<String>>> = OnceLock::new();

fn desktop_apply_backend_slot() -> &'static Mutex<Option<RunningDesktopApplyBackend>> {
    DESKTOP_APPLY_BACKEND.get_or_init(|| Mutex::new(None))
}

fn startup_restore_issues_slot() -> &'static Mutex<Vec<String>> {
    STARTUP_RESTORE_ISSUES.get_or_init(|| Mutex::new(Vec::new()))
}

#[cfg(test)]
pub(crate) fn real_desktop_flow_test_guard() -> std::sync::MutexGuard<'static, ()> {
    static REAL_DESKTOP_FLOW_TEST_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    REAL_DESKTOP_FLOW_TEST_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .expect("real desktop flow test lock was poisoned")
}

pub struct DesktopService;

impl DesktopService {
    pub(crate) fn library_resolution_issue(reason: &str) -> String {
        format!("{LIBRARY_RESOLUTION_ISSUE_PREFIX} {reason}")
    }

    pub fn load_page() -> Result<DesktopPageResult, String> {
        Self::load_page_with_projection_and_monitors(
            LibraryService::load_projection_snapshot()
                .or_else(|_| LibraryService::load_projection()),
            MonitorService::list_monitors(),
        )
    }

    pub fn restore_saved_assignments() -> Result<(), String> {
        let page = Self::load_page_with_projection_and_monitors(
            LibraryService::load_projection_snapshot()
                .or_else(|_| LibraryService::load_projection()),
            MonitorService::list_monitors_uncached(),
        )?;
        let issues = Self::restore_saved_assignments_with(&page, |monitor, item_id| {
            Self::apply_with_real_backend(monitor, item_id)
        });

        for issue in &issues {
            eprintln!("desktop restore skipped: {issue}");
        }

        Self::record_startup_restore_issues(issues);

        Ok(())
    }

    fn record_startup_restore_issues(issues: Vec<String>) {
        if let Ok(mut stored) = startup_restore_issues_slot().lock() {
            *stored = issues;
        }
    }

    fn take_startup_restore_issues() -> Vec<String> {
        startup_restore_issues_slot()
            .lock()
            .map(|mut issues| std::mem::take(&mut *issues))
            .unwrap_or_default()
    }

    pub(crate) fn load_page_with_projection(
        library_projection: Result<LibraryProjection, String>,
    ) -> Result<DesktopPageResult, String> {
        Self::load_page_with_projection_and_monitors(
            library_projection,
            MonitorService::list_monitors(),
        )
    }

    fn load_page_with_projection_and_monitors(
        library_projection: Result<LibraryProjection, String>,
        monitors: MonitorDiscoveryResult,
    ) -> Result<DesktopPageResult, String> {
        let assignments = match DesktopPersistenceService::for_user_path() {
            Ok(service) => service.load_state(),
            Err(reason) => DesktopPersistenceLoad::Unavailable { reason },
        };

        let mut result = Self::build_page_result(monitors, assignments, library_projection);

        let (running_outputs, runtime_issue) = Self::runtime_running_outputs();
        result.running_outputs = running_outputs;
        result.runtime_issue = runtime_issue;

        for issue in Self::take_startup_restore_issues() {
            if !result.restore_issues.contains(&issue) {
                result.restore_issues.push(issue);
            }
        }

        result.stale =
            result.stale || result.runtime_issue.is_some() || !result.restore_issues.is_empty();

        Ok(result)
    }

    fn library_item_titles(projection: LibraryProjection) -> BTreeMap<String, String> {
        projection
            .entries
            .into_iter()
            .map(|entry| {
                (
                    entry.entry.library_item_id.unwrap_or_default(),
                    entry.entry.title,
                )
            })
            .collect()
    }

    fn assignment_monitor_label(
        monitor_names: &BTreeMap<String, String>,
        monitors_available: bool,
        monitor_id: &str,
    ) -> String {
        if let Some(name) = monitor_names.get(monitor_id) {
            name.clone()
        } else if monitors_available {
            format!("{monitor_id} (missing)")
        } else {
            format!("{monitor_id} (unavailable)")
        }
    }

    fn build_page_result(
        monitors: MonitorDiscoveryResult,
        assignments: DesktopPersistenceLoad,
        library_projection: Result<LibraryProjection, String>,
    ) -> DesktopPageResult {
        let (monitors, monitors_available, monitor_discovery_issue) = match monitors {
            MonitorDiscoveryResult::Known(monitors) => (monitors, true, None),
            MonitorDiscoveryResult::Unavailable { reason } => (Vec::new(), false, Some(reason)),
        };
        let (assignments, persistence_issue, assignments_available) = match assignments {
            DesktopPersistenceLoad::Loaded(assignments) => (assignments, None, true),
            DesktopPersistenceLoad::Unavailable { reason } => {
                (BTreeMap::new(), Some(reason), false)
            }
        };
        let library_item_titles = library_projection.map(Self::library_item_titles);
        let mut resolved_assignments = BTreeMap::new();
        let mut library_item_assignments = BTreeMap::new();
        let mut restore_issues = Vec::new();
        let monitor_names = monitors
            .iter()
            .map(|monitor| (monitor.id.clone(), monitor.name.clone()))
            .collect::<BTreeMap<_, _>>();

        if let Err(reason) = &library_item_titles {
            restore_issues.push(Self::library_resolution_issue(reason));
        }

        for (monitor_id, item_id) in &assignments {
            let monitor_known = monitor_names.contains_key(monitor_id);
            let monitor_label =
                Self::assignment_monitor_label(&monitor_names, monitors_available, monitor_id);

            match &library_item_titles {
                Ok(library_items) => {
                    if let Some(item_title) = library_items.get(item_id) {
                        library_item_assignments
                            .entry(item_id.clone())
                            .or_insert_with(Vec::new)
                            .push(monitor_label.clone());

                        if monitor_known {
                            resolved_assignments.insert(
                                monitor_id.clone(),
                                DesktopResolvedMonitorAssignment::Restored {
                                    item_id: item_id.clone(),
                                    item_title: item_title.clone(),
                                },
                            );
                        } else if !monitors_available {
                            resolved_assignments.insert(
                                monitor_id.clone(),
                                DesktopResolvedMonitorAssignment::Unavailable {
                                    item_id: item_id.clone(),
                                    item_title: Some(item_title.clone()),
                                    reason: format!(
                                        "Saved assignment for monitor {monitor_id} could not be verified because monitor discovery is unavailable: {}.",
                                        monitor_discovery_issue
                                            .as_deref()
                                            .unwrap_or("Unknown monitor discovery failure")
                                    ),
                                },
                            );
                        } else if monitors_available {
                            resolved_assignments.insert(
                                monitor_id.clone(),
                                DesktopResolvedMonitorAssignment::MissingMonitor {
                                    item_id: item_id.clone(),
                                    item_title: Some(item_title.clone()),
                                },
                            );
                            restore_issues.push(format!(
                                "Saved assignment for missing monitor {monitor_id} still points to {item_title} ({item_id})."
                            ));
                        }
                    } else if monitor_known {
                        resolved_assignments.insert(
                            monitor_id.clone(),
                            DesktopResolvedMonitorAssignment::MissingItem {
                                item_id: item_id.clone(),
                            },
                        );
                    } else if !monitors_available {
                        resolved_assignments.insert(
                            monitor_id.clone(),
                            DesktopResolvedMonitorAssignment::Unavailable {
                                item_id: item_id.clone(),
                                item_title: None,
                                reason: format!(
                                    "Saved assignment for monitor {monitor_id} could not be verified because monitor discovery is unavailable and the referenced Library item {item_id} is missing: {}.",
                                    monitor_discovery_issue
                                        .as_deref()
                                        .unwrap_or("Unknown monitor discovery failure")
                                ),
                            },
                        );
                    } else if monitors_available {
                        resolved_assignments.insert(
                            monitor_id.clone(),
                            DesktopResolvedMonitorAssignment::MissingMonitor {
                                item_id: item_id.clone(),
                                item_title: None,
                            },
                        );
                        restore_issues.push(format!(
                            "Saved assignment for missing monitor {monitor_id} references missing item {item_id}."
                        ));
                    }
                }
                Err(reason) if monitor_known => {
                    resolved_assignments.insert(
                        monitor_id.clone(),
                        DesktopResolvedMonitorAssignment::Unavailable {
                            item_id: item_id.clone(),
                            item_title: None,
                            reason: reason.clone(),
                        },
                    );
                }
                Err(reason) if !monitors_available => {
                    resolved_assignments.insert(
                        monitor_id.clone(),
                        DesktopResolvedMonitorAssignment::Unavailable {
                            item_id: item_id.clone(),
                            item_title: None,
                            reason: format!(
                                "Saved assignment for monitor {monitor_id} could not be verified because monitor discovery is unavailable while the Library snapshot could not be resolved: {reason}"
                            ),
                        },
                    );
                }
                Err(_) => {}
            }
        }

        let stale = !monitors_available
            || !assignments_available
            || library_item_titles.is_err()
            || !restore_issues.is_empty();

        DesktopPageResult {
            monitors,
            assignments,
            resolved_assignments,
            library_item_assignments,
            running_outputs: BTreeSet::new(),
            restore_issues,
            runtime_issue: None,
            monitors_available,
            monitor_discovery_issue,
            persistence_issue,
            assignments_available,
            stale,
        }
    }

    fn runtime_running_outputs() -> (BTreeSet<String>, Option<String>) {
        match Self::runtime_running_outputs_result() {
            Ok(outputs) => (outputs, None),
            Err(reason) => (BTreeSet::new(), Some(reason)),
        }
    }

    fn runtime_running_outputs_result() -> Result<BTreeSet<String>, String> {
        let mut backend_guard = desktop_apply_backend_slot()
            .lock()
            .map_err(|_| "Desktop apply backend lock was poisoned".to_string())?;
        let Some(backend) = backend_guard.as_mut() else {
            return Ok(BTreeSet::new());
        };

        if !backend.handle.is_running() {
            *backend_guard = None;
            return Ok(BTreeSet::new());
        }

        backend
            .handle
            .send(EngineCommand::GetStatus)
            .map_err(|error| format!("Failed to request desktop runtime status: {error}"))?;

        Self::wait_for_status(backend)
    }

    fn wait_for_status(
        backend: &mut RunningDesktopApplyBackend,
    ) -> Result<BTreeSet<String>, String> {
        let deadline = Instant::now() + REAL_APPLY_BACKEND_TIMEOUT;

        loop {
            match Self::recv_backend_event(backend, deadline)? {
                Some(EngineEvent::Status(status)) => {
                    return Ok(status
                        .active_wallpapers
                        .into_iter()
                        .filter_map(|(output, path)| path.map(|_| output))
                        .collect());
                }
                Some(EngineEvent::Error(reason)) => {
                    return Err(format!(
                        "{REAL_APPLY_BACKEND} could not report runtime status: {reason}"
                    ));
                }
                Some(_) => {}
                None => {
                    return Err(format!(
                        "Timed out waiting for {REAL_APPLY_BACKEND} runtime status"
                    ));
                }
            }
        }
    }

    pub(crate) fn restore_saved_assignments_with<F>(
        page: &DesktopPageResult,
        mut apply: F,
    ) -> Vec<String>
    where
        F: FnMut(&crate::services::monitor_service::MonitorDescriptor, &str) -> Result<(), String>,
    {
        let monitor_map = page
            .monitors
            .iter()
            .map(|monitor| (monitor.id.as_str(), monitor))
            .collect::<BTreeMap<_, _>>();
        let mut issues = Vec::new();

        for (monitor_id, assignment) in &page.resolved_assignments {
            if let DesktopResolvedMonitorAssignment::Restored { item_id, .. } = assignment {
                let Some(monitor) = monitor_map.get(monitor_id.as_str()) else {
                    issues.push(format!(
                        "Saved assignment for monitor {monitor_id} was marked restorable but no active monitor descriptor was available."
                    ));
                    continue;
                };

                if let Err(reason) = apply(monitor, item_id) {
                    issues.push(format!(
                        "Failed to restore saved assignment for monitor {monitor_id} with item {item_id}: {reason}"
                    ));
                }
            }
        }

        issues
    }

    pub fn apply_to_monitor(monitor_id: &str, item_id: &str) -> Result<DesktopApplyResult, String> {
        if let Some(result) = Self::unsupported_apply_result(item_id)? {
            return Ok(result);
        }

        let monitors = MonitorService::list_monitors_uncached();

        match MonitorService::resolve_specific_monitor(&monitors, monitor_id) {
            MonitorDiscoveryResult::Known(monitors) if monitors.is_empty() => {
                Ok(DesktopApplyResult::MonitorNotFound {
                    monitor_id: monitor_id.to_string(),
                })
            }
            MonitorDiscoveryResult::Known(monitors) => {
                let monitor = monitors
                    .into_iter()
                    .next()
                    .ok_or_else(|| format!("Monitor {monitor_id} unexpectedly resolved empty"))?;

                if let Err(reason) = Self::apply_with_real_backend(&monitor, item_id) {
                    return Ok(DesktopApplyResult::BackendUnavailable { reason });
                }

                let persistence = match DesktopPersistenceService::for_user_path() {
                    Ok(service) => service,
                    Err(reason) => {
                        return Ok(DesktopApplyResult::PersistenceUnavailable { reason });
                    }
                };

                match persistence.save_assignment(monitor_id, item_id) {
                    DesktopPersistenceWrite::Saved => Ok(DesktopApplyResult::AppliedWithBackend {
                        monitor_id: monitor_id.to_string(),
                        item_id: item_id.to_string(),
                        backend: REAL_APPLY_BACKEND.to_string(),
                    }),
                    DesktopPersistenceWrite::Cleared => {
                        Ok(DesktopApplyResult::PersistenceUnavailable {
                            reason: "Desktop persistence returned a clear result while saving"
                                .to_string(),
                        })
                    }
                    DesktopPersistenceWrite::Unavailable { reason } => {
                        Ok(DesktopApplyResult::PersistenceUnavailable { reason })
                    }
                }
            }
            MonitorDiscoveryResult::Unavailable { reason } => {
                Ok(DesktopApplyResult::MonitorDiscoveryUnavailable { reason })
            }
        }
    }

    fn unsupported_apply_result(item_id: &str) -> Result<Option<DesktopApplyResult>, String> {
        let Ok(entry) = LibraryService::inspect_item(item_id) else {
            return Ok(None);
        };

        if entry.entry.project_type == WorkshopProjectType::Video {
            return Ok(None);
        }

        Ok(Self::unsupported_apply_result_for_project_type(
            item_id,
            entry.entry.project_type,
        ))
    }

    fn unsupported_apply_result_for_project_type(
        item_id: &str,
        project_type: WorkshopProjectType,
    ) -> Option<DesktopApplyResult> {
        if project_type == WorkshopProjectType::Video {
            return None;
        }

        Some(DesktopApplyResult::UnsupportedItem {
            item_id: item_id.to_string(),
            reason: format!(
                "{:?} wallpapers are recognized for compatibility reporting, but the current verified runtime can only apply video wallpapers.",
                project_type
            ),
        })
    }

    fn apply_with_real_backend(
        monitor: &crate::services::monitor_service::MonitorDescriptor,
        item_id: &str,
    ) -> Result<(), String> {
        let path = Self::resolve_real_apply_path(item_id)?;
        let mut backend_guard = Self::ensure_running_apply_backend()?;
        let backend = backend_guard
            .as_mut()
            .ok_or_else(|| "Desktop apply backend failed to initialize".to_string())?;

        Self::wait_for_output(backend, &monitor.backend_output_id)?;
        backend
            .handle
            .send(EngineCommand::ApplyWallpaper {
                path: path.clone(),
                output: Some(monitor.backend_output_id.clone()),
            })
            .map_err(|error| format!("Failed to send real desktop apply command: {error}"))?;
        Self::wait_for_apply(backend, &monitor.backend_output_id, &path)
    }

    fn resolve_real_apply_path(item_id: &str) -> Result<PathBuf, String> {
        let entry = LibraryService::inspect_item(item_id).map_err(|reason| {
            format!("Library item {item_id} is unavailable for real desktop apply: {reason}")
        })?;

        if entry.entry.project_type != WorkshopProjectType::Video {
            return Err(format!(
                "Library item {item_id} uses {:?}, but the current real desktop apply path only supports synced video items on this machine",
                entry.entry.project_type
            ));
        }

        let project = WeProject::load(&entry.entry.project_dir).map_err(|error| {
            format!(
                "Library item {item_id} could not load project metadata for real desktop apply: {error}"
            )
        })?;
        let path = project.main_file(&entry.entry.project_dir).ok_or_else(|| {
            format!(
                "Library item {item_id} does not expose a primary video asset for real desktop apply"
            )
        })?;

        if !path.is_file() {
            return Err(format!(
                "Library item {item_id} points to missing video asset {}",
                path.display()
            ));
        }

        Ok(path)
    }

    fn ensure_running_apply_backend()
    -> Result<std::sync::MutexGuard<'static, Option<RunningDesktopApplyBackend>>, String> {
        let mut backend = desktop_apply_backend_slot()
            .lock()
            .map_err(|_| "Desktop apply backend lock was poisoned".to_string())?;
        let should_restart = backend
            .as_ref()
            .map(|backend| !backend.handle.is_running())
            .unwrap_or(true);

        if should_restart {
            *backend = Some(Self::start_apply_backend()?);
        }

        Ok(backend)
    }

    fn start_apply_backend() -> Result<RunningDesktopApplyBackend, String> {
        let (handle, events) = spawn_engine(EngineConfig::default())
            .map_err(|error| format!("Failed to start {REAL_APPLY_BACKEND}: {error}"))?;
        let mut backend = RunningDesktopApplyBackend { handle, events };

        Self::wait_for_backend_start(&mut backend)?;

        Ok(backend)
    }

    fn wait_for_backend_start(backend: &mut RunningDesktopApplyBackend) -> Result<(), String> {
        let deadline = Instant::now() + REAL_APPLY_BACKEND_TIMEOUT;

        loop {
            match Self::recv_backend_event(backend, deadline)? {
                Some(EngineEvent::Started) => return Ok(()),
                Some(EngineEvent::Error(reason)) => {
                    return Err(format!("{REAL_APPLY_BACKEND} failed to start: {reason}"));
                }
                Some(_) => {}
                None => {
                    return Err(format!(
                        "Timed out waiting for {REAL_APPLY_BACKEND} to start"
                    ));
                }
            }
        }
    }

    fn wait_for_output(
        backend: &mut RunningDesktopApplyBackend,
        output_name: &str,
    ) -> Result<(), String> {
        let deadline = Instant::now() + REAL_APPLY_BACKEND_TIMEOUT;
        backend
            .handle
            .send(EngineCommand::GetOutputs)
            .map_err(|error| {
                format!("Failed to request outputs from {REAL_APPLY_BACKEND}: {error}")
            })?;

        loop {
            match Self::recv_backend_event(backend, deadline)? {
                Some(EngineEvent::OutputAdded(info)) => {
                    if Self::output_added_matches(&info, output_name) {
                        return Ok(());
                    }
                }
                Some(EngineEvent::OutputsList(outputs)) => {
                    Self::ensure_output_list_contains(&outputs, output_name)?;
                    return Ok(());
                }
                Some(EngineEvent::Error(reason)) => {
                    return Err(format!(
                        "{REAL_APPLY_BACKEND} could not prepare output {output_name}: {reason}"
                    ));
                }
                Some(_) => {}
                None => {
                    return Err(format!(
                        "Timed out waiting for output {output_name} in {REAL_APPLY_BACKEND}"
                    ));
                }
            }
        }
    }

    fn output_added_matches(output: &lwe_engine::OutputInfo, output_name: &str) -> bool {
        output.name == output_name
    }

    fn ensure_output_list_contains(
        outputs: &[lwe_engine::OutputInfo],
        output_name: &str,
    ) -> Result<(), String> {
        if outputs.iter().any(|output| output.name == output_name) {
            return Ok(());
        }

        let available = outputs
            .iter()
            .map(|output| output.name.as_str())
            .collect::<Vec<_>>();

        if available.is_empty() {
            Err(format!(
                "{REAL_APPLY_BACKEND} did not report any active outputs while preparing {output_name}. Confirm the Wayland session exposes outputs to the runtime backend."
            ))
        } else {
            Err(format!(
                "{REAL_APPLY_BACKEND} reported outputs [{}] but not requested output {output_name}. Reselect the monitor in LWE and check for a compositor output-name mismatch.",
                available.join(", ")
            ))
        }
    }

    fn wait_for_apply(
        backend: &mut RunningDesktopApplyBackend,
        output_name: &str,
        path: &Path,
    ) -> Result<(), String> {
        let deadline = Instant::now() + REAL_APPLY_BACKEND_TIMEOUT;

        loop {
            match Self::recv_backend_event(backend, deadline)? {
                Some(EngineEvent::WallpaperApplied {
                    output,
                    path: applied_path,
                }) if output == output_name && applied_path == path => return Ok(()),
                Some(EngineEvent::Error(reason)) => {
                    return Err(format!(
                        "{REAL_APPLY_BACKEND} failed to apply {} to {output_name}: {reason}",
                        path.display()
                    ));
                }
                Some(_) => {}
                None => {
                    return Err(Self::apply_timeout_message(output_name, path));
                }
            }
        }
    }

    fn apply_timeout_message(output_name: &str, path: &Path) -> String {
        format!(
            "Timed out waiting for {REAL_APPLY_BACKEND} to apply {} to {output_name}. Check Wayland layer-shell, EGL, mpv, and video asset availability.",
            path.display()
        )
    }

    fn recv_backend_event(
        backend: &mut RunningDesktopApplyBackend,
        deadline: Instant,
    ) -> Result<Option<EngineEvent>, String> {
        let now = Instant::now();
        if now >= deadline {
            return Ok(None);
        }

        match backend
            .events
            .recv_timeout(deadline.saturating_duration_since(now))
        {
            Ok(event) => Ok(Some(event)),
            Err(RecvTimeoutError::Timeout) => Ok(None),
            Err(RecvTimeoutError::Disconnected) => Err(format!(
                "{REAL_APPLY_BACKEND} stopped before completing the desktop apply request"
            )),
        }
    }

    pub fn clear_monitor(monitor_id: &str) -> Result<DesktopApplyResult, String> {
        let monitors = MonitorService::list_monitors_uncached();

        match MonitorService::resolve_specific_monitor(&monitors, monitor_id) {
            MonitorDiscoveryResult::Known(monitors) if monitors.is_empty() => {
                Ok(DesktopApplyResult::MonitorNotFound {
                    monitor_id: monitor_id.to_string(),
                })
            }
            MonitorDiscoveryResult::Known(monitors) => {
                let monitor = monitors
                    .into_iter()
                    .next()
                    .ok_or_else(|| format!("Monitor {monitor_id} unexpectedly resolved empty"))?;

                let runtime_warning = Self::clear_with_real_backend(&monitor).err();

                let persistence = match DesktopPersistenceService::for_user_path() {
                    Ok(service) => service,
                    Err(reason) => {
                        return Ok(DesktopApplyResult::PersistenceUnavailable { reason });
                    }
                };

                match persistence.clear_assignment(monitor_id) {
                    DesktopPersistenceWrite::Cleared => match runtime_warning {
                        Some(warning) => Ok(DesktopApplyResult::ClearedWithBackendWarning {
                            monitor_id: monitor_id.to_string(),
                            warning,
                        }),
                        None => Ok(DesktopApplyResult::Cleared {
                            monitor_id: monitor_id.to_string(),
                        }),
                    },
                    DesktopPersistenceWrite::Saved => {
                        Ok(DesktopApplyResult::PersistenceUnavailable {
                            reason: "Desktop persistence returned a save result while clearing"
                                .to_string(),
                        })
                    }
                    DesktopPersistenceWrite::Unavailable { reason } => {
                        Ok(DesktopApplyResult::PersistenceUnavailable { reason })
                    }
                }
            }
            MonitorDiscoveryResult::Unavailable { reason } => {
                Ok(DesktopApplyResult::MonitorDiscoveryUnavailable { reason })
            }
        }
    }

    fn clear_with_real_backend(
        monitor: &crate::services::monitor_service::MonitorDescriptor,
    ) -> Result<(), String> {
        let mut backend_guard = desktop_apply_backend_slot()
            .lock()
            .map_err(|_| "Desktop apply backend lock was poisoned".to_string())?;

        let Some(backend) = backend_guard.as_mut() else {
            return Err(BACKEND_NOT_RUNNING_CLEAR_WARNING.to_string());
        };

        if !backend.handle.is_running() {
            *backend_guard = None;
            return Err(BACKEND_NOT_RUNNING_CLEAR_WARNING.to_string());
        }

        Self::wait_for_output(backend, &monitor.backend_output_id)?;
        backend
            .handle
            .send(EngineCommand::ClearWallpaper {
                output: Some(monitor.backend_output_id.clone()),
            })
            .map_err(|error| format!("Failed to send real desktop clear command: {error}"))?;

        Self::wait_for_clear(backend, &monitor.backend_output_id)
    }

    fn wait_for_clear(
        backend: &mut RunningDesktopApplyBackend,
        output_name: &str,
    ) -> Result<(), String> {
        let deadline = Instant::now() + REAL_APPLY_BACKEND_TIMEOUT;

        loop {
            match Self::recv_backend_event(backend, deadline)? {
                Some(EngineEvent::WallpaperCleared { output }) if output == output_name => {
                    return Ok(());
                }
                Some(EngineEvent::Error(reason)) => {
                    return Err(format!(
                        "{REAL_APPLY_BACKEND} failed to clear output {output_name}: {reason}"
                    ));
                }
                Some(_) => {}
                None => {
                    return Err(format!(
                        "Timed out waiting for {REAL_APPLY_BACKEND} to clear output {output_name}"
                    ));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;
    use crate::results::desktop::DesktopApplyResult;
    use crate::results::desktop::DesktopResolvedMonitorAssignment;
    use crate::results::library::LibraryProjection;
    use crate::results::monitor_discovery::MonitorDiscoveryResult;
    use crate::results::workshop::AssessedWorkshopCatalogEntry;
    use crate::services::library_service::LibraryService;
    use crate::services::monitor_service::MonitorService;
    use lwe_engine::OutputInfo;
    use lwe_library::WeProject;
    use lwe_library::{WorkshopCatalogEntry, WorkshopProjectType, WorkshopSyncState};

    struct EnvGuard {
        xdg_config_home: Option<OsString>,
        home: Option<OsString>,
        _lock: std::sync::MutexGuard<'static, ()>,
    }

    impl EnvGuard {
        fn set_config_root(config_root: &Path) -> Self {
            let lock = crate::test_env::env_lock();
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

    fn test_config_root() -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();

        std::env::temp_dir().join(format!("desktop-service-config-{unique}"))
    }

    fn known_monitor_id() -> Option<String> {
        match MonitorService::list_monitors() {
            MonitorDiscoveryResult::Known(monitors) => {
                monitors.into_iter().next().map(|monitor| monitor.id)
            }
            MonitorDiscoveryResult::Unavailable { .. } => None,
        }
    }

    fn known_monitor() -> Option<crate::services::monitor_service::MonitorDescriptor> {
        match MonitorService::list_monitors() {
            MonitorDiscoveryResult::Known(monitors) => monitors.into_iter().next(),
            MonitorDiscoveryResult::Unavailable { .. } => None,
        }
    }

    fn real_supported_library_item_id() -> Option<String> {
        let projection = LibraryService::load_projection_snapshot()
            .or_else(|_| LibraryService::load_projection())
            .ok()?;

        projection.entries.into_iter().find_map(|entry| {
            if entry.entry.project_type != WorkshopProjectType::Video {
                return None;
            }

            let project = WeProject::load(&entry.entry.project_dir).ok()?;
            let main_file = project.main_file(&entry.entry.project_dir)?;

            if !main_file.is_file() {
                return None;
            }

            entry.entry.library_item_id
        })
    }

    fn real_desktop_tests_enabled() -> bool {
        std::env::var("LWE_REAL_DESKTOP_TESTS")
            .map(|value| matches!(value.as_str(), "1" | "true" | "TRUE" | "yes" | "YES"))
            .unwrap_or(false)
    }

    fn library_item(item_id: &str, title: &str) -> AssessedWorkshopCatalogEntry {
        AssessedWorkshopCatalogEntry {
            entry: WorkshopCatalogEntry {
                workshop_id: 7,
                title: title.to_string(),
                project_type: WorkshopProjectType::Scene,
                project_dir: std::path::PathBuf::from("/tmp/7"),
                cover_path: None,
                sync_state: WorkshopSyncState::Synced,
                supported_first_release: true,
                library_item_id: Some(item_id.to_string()),
            },
            compatibility: crate::policies::shared::compatibility_policy::compatibility_decision(
                &WorkshopCatalogEntry {
                    workshop_id: 7,
                    title: title.to_string(),
                    project_type: WorkshopProjectType::Scene,
                    project_dir: std::path::PathBuf::from("/tmp/7"),
                    cover_path: None,
                    sync_state: WorkshopSyncState::Synced,
                    supported_first_release: true,
                    library_item_id: Some(item_id.to_string()),
                },
            ),
            project_metadata: Default::default(),
        }
    }

    #[test]
    fn desktop_apply_flow_load_page_restores_known_assignments_and_reports_degraded_entries() {
        let mut assignments = BTreeMap::new();
        assignments.insert("DISPLAY-1".to_string(), "scene-7".to_string());
        assignments.insert("DISPLAY-2".to_string(), "missing-item".to_string());
        assignments.insert("DISPLAY-3".to_string(), "scene-7".to_string());

        let result = DesktopService::build_page_result(
            MonitorDiscoveryResult::Known(vec![
                crate::services::monitor_service::MonitorDescriptor {
                    id: "DISPLAY-1".to_string(),
                    backend_output_id: "DISPLAY-1".to_string(),
                    name: "Primary".to_string(),
                    resolution: "1920x1080".to_string(),
                },
                crate::services::monitor_service::MonitorDescriptor {
                    id: "DISPLAY-2".to_string(),
                    backend_output_id: "DISPLAY-2".to_string(),
                    name: "Secondary".to_string(),
                    resolution: "2560x1440".to_string(),
                },
            ]),
            DesktopPersistenceLoad::Loaded(assignments),
            Ok(LibraryProjection {
                entries: vec![library_item("scene-7", "Forest Scene")],
                source_catalog_count: 1,
                served_from_snapshot: false,
            }),
        );

        assert!(matches!(
            result.resolved_assignments.get("DISPLAY-1"),
            Some(DesktopResolvedMonitorAssignment::Restored { item_id, item_title })
                if item_id == "scene-7" && item_title == "Forest Scene"
        ));
        assert!(matches!(
            result.resolved_assignments.get("DISPLAY-2"),
            Some(DesktopResolvedMonitorAssignment::MissingItem { item_id })
                if item_id == "missing-item"
        ));
        assert_eq!(
            result.library_item_assignments.get("scene-7"),
            Some(&vec![
                "Primary".to_string(),
                "DISPLAY-3 (missing)".to_string()
            ])
        );
        assert!(matches!(
            result.resolved_assignments.get("DISPLAY-3"),
            Some(DesktopResolvedMonitorAssignment::MissingMonitor { item_id, item_title })
                if item_id == "scene-7" && item_title.as_deref() == Some("Forest Scene")
        ));
        assert!(result.stale);
        assert_eq!(
            result.restore_issues,
            vec!["Saved assignment for missing monitor DISPLAY-3 still points to Forest Scene (scene-7).".to_string()]
        );
    }

    #[test]
    fn desktop_apply_flow_load_page_reports_library_resolution_issue_without_assignments() {
        let result = DesktopService::build_page_result(
            MonitorDiscoveryResult::Known(vec![]),
            DesktopPersistenceLoad::Loaded(BTreeMap::new()),
            Err("Library refresh failed".to_string()),
        );

        assert!(result.stale);
        assert_eq!(
            result.restore_issues,
            vec![
                "Unable to resolve desktop items against the current Library snapshot: Library refresh failed"
                    .to_string()
            ]
        );
    }

    #[test]
    fn desktop_apply_flow_load_page_keeps_assignments_when_monitor_discovery_is_unavailable() {
        let result = DesktopService::build_page_result(
            MonitorDiscoveryResult::Unavailable {
                reason: "niri unavailable".to_string(),
            },
            DesktopPersistenceLoad::Loaded(BTreeMap::from([(
                "DISPLAY-9".to_string(),
                "scene-7".to_string(),
            )])),
            Ok(LibraryProjection {
                entries: vec![library_item("scene-7", "Forest Scene")],
                source_catalog_count: 1,
                served_from_snapshot: false,
            }),
        );

        assert!(!result.monitors_available);
        assert_eq!(
            result.monitor_discovery_issue.as_deref(),
            Some("niri unavailable")
        );
        assert_eq!(
            result.library_item_assignments.get("scene-7"),
            Some(&vec!["DISPLAY-9 (unavailable)".to_string()])
        );
        assert!(matches!(
            result.resolved_assignments.get("DISPLAY-9"),
            Some(DesktopResolvedMonitorAssignment::Unavailable { item_id, item_title, reason })
                if item_id == "scene-7"
                    && item_title.as_deref() == Some("Forest Scene")
                    && reason.contains("DISPLAY-9")
                    && reason.contains("niri unavailable")
        ));
        assert!(result.stale);
    }

    #[test]
    fn desktop_apply_flow_load_page_reflects_real_monitor_and_persistence_availability() {
        let result = DesktopService::load_page().unwrap();
        let DesktopPageResult {
            monitors_available,
            stale,
            assignments_available,
            monitor_discovery_issue,
            persistence_issue,
            monitors,
            restore_issues,
            ..
        } = result;

        assert_eq!(
            stale,
            !monitors_available || !assignments_available || !restore_issues.is_empty()
        );

        if monitors_available {
            assert!(monitor_discovery_issue.is_none());
        } else {
            assert!(monitors.is_empty());
            assert!(monitor_discovery_issue.is_some());
        }

        if assignments_available {
            assert!(persistence_issue.is_none());
        } else {
            assert!(persistence_issue.is_some());
        }
    }

    #[test]
    fn desktop_apply_flow_load_page_prefers_unified_toml_session_state_over_legacy_json() {
        let _guard = real_desktop_flow_test_guard();
        let config_root = test_config_root();
        fs::create_dir_all(config_root.join("lwe")).unwrap();
        fs::create_dir_all(config_root.join("wayvid")).unwrap();
        fs::write(
            config_root.join("lwe").join("session.toml"),
            "[assignments]\nDISPLAY-1 = \"scene-from-session\"\n",
        )
        .unwrap();
        fs::write(
            config_root.join("wayvid").join("desktop-state.json"),
            "{\"assignments\":{\"DISPLAY-1\":\"scene-from-legacy\"}}",
        )
        .unwrap();
        let _env = EnvGuard::set_config_root(&config_root);

        let result = DesktopService::load_page().unwrap();

        assert_eq!(
            result.assignments.get("DISPLAY-1").map(String::as_str),
            Some("scene-from-session")
        );
    }

    #[test]
    fn desktop_apply_flow_startup_restore_applies_only_restorable_assignments() {
        let page = DesktopPageResult {
            monitors: vec![crate::services::monitor_service::MonitorDescriptor {
                id: "DISPLAY-1".to_string(),
                backend_output_id: "DISPLAY-1".to_string(),
                name: "Primary".to_string(),
                resolution: "1920x1080".to_string(),
            }],
            assignments: BTreeMap::from([
                ("DISPLAY-1".to_string(), "scene-7".to_string()),
                ("DISPLAY-2".to_string(), "missing-item".to_string()),
            ]),
            resolved_assignments: BTreeMap::from([
                (
                    "DISPLAY-1".to_string(),
                    DesktopResolvedMonitorAssignment::Restored {
                        item_id: "scene-7".to_string(),
                        item_title: "Forest Scene".to_string(),
                    },
                ),
                (
                    "DISPLAY-2".to_string(),
                    DesktopResolvedMonitorAssignment::MissingItem {
                        item_id: "missing-item".to_string(),
                    },
                ),
            ]),
            library_item_assignments: BTreeMap::new(),
            running_outputs: Default::default(),
            restore_issues: Vec::new(),
            runtime_issue: None,
            monitors_available: true,
            monitor_discovery_issue: None,
            persistence_issue: None,
            assignments_available: true,
            stale: false,
        };
        let mut applied = Vec::new();

        DesktopService::restore_saved_assignments_with(&page, |monitor, item_id| {
            applied.push((monitor.id.clone(), item_id.to_string()));
            Ok(())
        });

        assert_eq!(
            applied,
            vec![("DISPLAY-1".to_string(), "scene-7".to_string())]
        );
    }

    #[test]
    fn desktop_apply_flow_rejects_non_video_before_backend_resolution() {
        let result = DesktopService::unsupported_apply_result_for_project_type(
            "web-7",
            WorkshopProjectType::Web,
        );

        assert!(matches!(
            result,
            Some(DesktopApplyResult::UnsupportedItem { item_id, reason })
                if item_id == "web-7"
                    && reason.contains("Web wallpapers")
                    && reason.contains("only apply video wallpapers")
        ));
    }

    #[test]
    fn desktop_apply_flow_allows_video_to_continue_to_backend_resolution() {
        let result = DesktopService::unsupported_apply_result_for_project_type(
            "video-7",
            WorkshopProjectType::Video,
        );

        assert!(result.is_none());
    }

    #[test]
    fn desktop_apply_flow_startup_restore_issues_are_consumed_after_surface() {
        DesktopService::record_startup_restore_issues(vec![
            "Failed to restore saved assignment for monitor DISPLAY-1 with item scene-7: backend unavailable".to_string(),
        ]);

        let issues = DesktopService::take_startup_restore_issues();
        let second_read = DesktopService::take_startup_restore_issues();

        assert_eq!(
            issues,
            vec!["Failed to restore saved assignment for monitor DISPLAY-1 with item scene-7: backend unavailable".to_string()]
        );
        assert!(second_read.is_empty());
    }

    #[test]
    fn desktop_apply_flow_load_page_marks_runtime_issue_as_stale_and_deduplicates_restore_issues() {
        DesktopService::record_startup_restore_issues(vec![
            "restore warning".to_string(),
            "restore warning".to_string(),
        ]);

        let mut result = DesktopService::build_page_result(
            MonitorDiscoveryResult::Known(vec![
                crate::services::monitor_service::MonitorDescriptor {
                    id: "DISPLAY-1".to_string(),
                    backend_output_id: "eDP-1".to_string(),
                    name: "Primary".to_string(),
                    resolution: "1920x1080".to_string(),
                },
            ]),
            DesktopPersistenceLoad::Loaded(BTreeMap::new()),
            Ok(LibraryProjection {
                entries: vec![],
                source_catalog_count: 0,
                served_from_snapshot: false,
            }),
        );
        result.runtime_issue =
            Some("Timed out waiting for lwe_engine_wayland runtime status".to_string());

        for issue in DesktopService::take_startup_restore_issues() {
            if !result.restore_issues.contains(&issue) {
                result.restore_issues.push(issue);
            }
        }
        result.stale =
            result.stale || result.runtime_issue.is_some() || !result.restore_issues.is_empty();

        assert!(result.stale);
        assert_eq!(
            result.runtime_issue.as_deref(),
            Some("Timed out waiting for lwe_engine_wayland runtime status")
        );
        assert_eq!(result.restore_issues, vec!["restore warning".to_string()]);
    }

    #[test]
    fn desktop_apply_flow_output_list_reports_actionable_mismatch() {
        let result = DesktopService::ensure_output_list_contains(
            &[OutputInfo {
                name: "HDMI-A-1".to_string(),
                width: 1920,
                height: 1080,
                scale: 1.0,
                position: (0, 0),
                active: true,
                hdr_capabilities: Default::default(),
            }],
            "eDP-1",
        );

        assert!(matches!(result, Err(reason) if reason.contains("HDMI-A-1")
            && reason.contains("eDP-1")
            && reason.contains("output-name mismatch")));
    }

    #[test]
    fn desktop_apply_flow_empty_output_list_reports_actionable_runtime_issue() {
        let result = DesktopService::ensure_output_list_contains(&[], "eDP-1");

        assert!(
            matches!(result, Err(reason) if reason.contains("did not report any active outputs")
            && reason.contains("Wayland session"))
        );
    }

    #[test]
    fn desktop_apply_flow_apply_timeout_mentions_runtime_layers() {
        let message = DesktopService::apply_timeout_message("eDP-1", Path::new("/tmp/video.mp4"));

        assert!(message.contains("Timed out"));
        assert!(message.contains("Wayland layer-shell"));
        assert!(message.contains("EGL"));
        assert!(message.contains("mpv"));
        assert!(message.contains("video asset"));
    }

    #[test]
    fn desktop_apply_flow_apply_to_monitor_reflects_current_monitor_discovery_state() {
        let _guard = real_desktop_flow_test_guard();
        let Some(monitor_id) = known_monitor_id() else {
            let result = DesktopService::apply_to_monitor("DISPLAY-1", "wallpaper-1").unwrap();

            assert!(matches!(
                result,
                DesktopApplyResult::MonitorDiscoveryUnavailable { .. }
            ));
            return;
        };

        let result = DesktopService::apply_to_monitor(&monitor_id, "wallpaper-1").unwrap();

        assert!(matches!(
            result,
            DesktopApplyResult::Applied { .. }
                | DesktopApplyResult::AppliedWithBackend { .. }
                | DesktopApplyResult::BackendUnavailable { .. }
                | DesktopApplyResult::PersistenceUnavailable { .. }
        ));
    }

    #[test]
    fn desktop_apply_flow_apply_to_monitor_avoids_placeholder_result_for_real_monitor_and_library_item()
     {
        if !real_desktop_tests_enabled() {
            eprintln!("skipping real desktop apply test; set LWE_REAL_DESKTOP_TESTS=1 to enable");
            return;
        }

        let monitor = known_monitor().expect("expected a real discovered monitor on this machine");
        let item_id = real_supported_library_item_id()
            .expect("expected one real supported video Library item on this machine");

        let result = DesktopService::apply_to_monitor(&monitor.id, &item_id).unwrap();

        assert!(
            !matches!(
                result,
                DesktopApplyResult::Applied { .. }
                    | DesktopApplyResult::PersistenceUnavailable { .. }
            ),
            "expected a real backend apply result for {} and {}, got {:?}",
            monitor.id,
            item_id,
            result
        );

        let clear_result = DesktopService::clear_monitor(&monitor.id).unwrap();
        assert!(matches!(
            clear_result,
            DesktopApplyResult::Cleared { .. }
                | DesktopApplyResult::ClearedWithBackendWarning { .. }
        ));
    }

    #[test]
    fn desktop_apply_flow_clear_monitor_reflects_current_monitor_discovery_state() {
        if !real_desktop_tests_enabled() {
            eprintln!("skipping real desktop clear test; set LWE_REAL_DESKTOP_TESTS=1 to enable");
            return;
        }

        let _guard = real_desktop_flow_test_guard();

        let Some(monitor_id) = known_monitor_id() else {
            let result = DesktopService::clear_monitor("DISPLAY-1").unwrap();

            assert!(matches!(
                result,
                DesktopApplyResult::MonitorDiscoveryUnavailable { .. }
            ));
            return;
        };

        let item_id = real_supported_library_item_id()
            .expect("expected one real supported video Library item on this machine");

        let apply_result = DesktopService::apply_to_monitor(&monitor_id, &item_id).unwrap();
        assert!(
            matches!(apply_result, DesktopApplyResult::AppliedWithBackend { .. }),
            "expected real backend apply before clear, got {apply_result:?}"
        );

        let result = DesktopService::clear_monitor(&monitor_id).unwrap();

        assert!(matches!(result, DesktopApplyResult::Cleared { .. }));
    }
}
