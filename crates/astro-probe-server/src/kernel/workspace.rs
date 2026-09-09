use astro_probe_db::DbPool;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, RwLock};
use std::time::Instant;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum WorkspaceStatus {
    #[serde(rename = "loaded")]
    Loaded,
    #[serde(rename = "unloaded")]
    Unloaded,
    #[serde(rename = "idle")]
    Idle,
}

impl std::fmt::Display for WorkspaceStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WorkspaceStatus::Loaded => write!(f, "loaded"),
            WorkspaceStatus::Unloaded => write!(f, "unloaded"),
            WorkspaceStatus::Idle => write!(f, "idle"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workspace {
    pub id: String,
    pub name: String,
    pub project_path: String,
    pub status: WorkspaceStatus,
    pub db_path: String,
}

#[derive(Clone)]
pub struct WorkspaceState {
    pub workspace: Workspace,
    pub db_pool: Option<DbPool>,
    pub last_accessed: Arc<RwLock<Instant>>,
}
