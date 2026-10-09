use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InvalidatedPage {
    Library,
    Workshop,
    Desktop,
    Settings,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppShellPatch {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workshop_synced_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub library_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub monitor_count: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionOutcome<T> {
    pub ok: bool,
    pub message: Option<String>,
    pub shell_patch: Option<AppShellPatch>,
    pub current_update: Option<T>,
    pub invalidations: Vec<InvalidatedPage>,
}

impl<T> ActionOutcome<T> {
    pub fn success(current_update: Option<T>) -> Self {
        Self {
            ok: true,
            message: None,
            shell_patch: None,
            current_update,
            invalidations: Vec::new(),
        }
    }
}
