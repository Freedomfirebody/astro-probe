use crate::kernel::workspace::{Workspace, WorkspaceState, WorkspaceStatus};
use anyhow::Context;
use astro_probe_db::DbPool;
use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::time::Instant;

use astro_probe_core::traits::{DependencyAnalyzer, FrameworkAnalyzer};
use astro_probe_java::di::DependencyInjectionAnalyzer;
use astro_probe_java::jar::{get_global_cache_path, JarAnalyzer};

fn find_workspace_root() -> Option<PathBuf> {
    if let Ok(current_dir) = std::env::current_dir() {
        let mut dir = current_dir.as_path();
        loop {
            if dir.join("PROJECT.md").exists() {
                return Some(dir.to_path_buf());
            }
            if let Some(parent) = dir.parent() {
                dir = parent;
            } else {
                break;
            }
        }
    }
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut dir = manifest_dir;
    loop {
        if dir.join("PROJECT.md").exists() {
            return Some(dir.to_path_buf());
        }
        if let Some(parent) = dir.parent() {
            dir = parent;
        } else {
            break;
        }
    }
    None
}

fn resolve_path(path_str: &str) -> PathBuf {
    let path = Path::new(path_str);
    if path.is_absolute() {
        return path.to_path_buf();
    }
    if let Some(workspace_root) = find_workspace_root() {
        return workspace_root.join(path);
    }
    if let Ok(current_dir) = std::env::current_dir() {
        return current_dir.join(path);
    }
    path.to_path_buf()
}

pub struct WorkspaceManager {
    workspaces: Arc<RwLock<HashMap<String, WorkspaceState>>>,
}

impl WorkspaceManager {
    pub fn new() -> Self {
        let workspaces: Arc<RwLock<HashMap<String, WorkspaceState>>> =
            Arc::new(RwLock::new(HashMap::new()));
        let workspaces_clone = Arc::clone(&workspaces);

        tokio::spawn(async move {
            loop {
                tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;

                let mut timeout_secs = std::env::var("ASTRO_PROBE_IDLE_TIMEOUT_SECS")
                    .ok()
                    .and_then(|val| val.parse::<u64>().ok())
                    .unwrap_or(10800); // Default 3 hours
                if timeout_secs < 5 {
                    timeout_secs = 5;
                }

                if let Ok(mut guard) = workspaces_clone.write() {
                    for ws_state in guard.values_mut() {
                        if ws_state.workspace.status == WorkspaceStatus::Loaded {
                            let last_acc = if let Ok(la) = ws_state.last_accessed.read() {
                                *la
                            } else {
                                continue;
                            };
                            if last_acc.elapsed().as_secs() >= timeout_secs {
                                ws_state.workspace.status = WorkspaceStatus::Idle;
                                ws_state.db_pool = None; // Drop connection pool
                                tracing::info!(
                                    "Workspace {} transitioned to Idle due to inactivity",
                                    ws_state.workspace.id
                                );
                            }
                        }
                    }
                }
            }
        });

        Self { workspaces }
    }

    fn load_db_pool(&self, project_path: &str) -> anyhow::Result<DbPool> {
        let db_path = Path::new(project_path).join(".astro-probe.db");
        // Ensure parent directories exist
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        // Safely initialize the database and transition to WAL mode using a single connection
        {
            let conn = astro_probe_db::establish_connection(&db_path)?;
            astro_probe_db::init_db(&conn)?;
        }
        let pool = astro_probe_db::establish_connection_pool(&db_path)?;
        Ok(pool)
    }

    pub fn create_workspace(
        &self,
        name: String,
        project_path: String,
    ) -> anyhow::Result<Workspace> {
        let id = uuid::Uuid::new_v4().to_string();

        let resolved_path = resolve_path(&project_path).to_string_lossy().to_string();
        let pool = self.load_db_pool(&resolved_path)?;

        // Initialize db schemas and parse java files
        {
            let mut conn = pool.get().context("Failed to get connection from pool")?;
            let parser = astro_probe_java::parser::JavaParser::new();

            let t0 = std::time::Instant::now();
            if let Err(e) = parser.parse_and_populate(&resolved_path, &mut conn) {
                tracing::error!("Failed to parse Java files: {}", e);
                return Err(anyhow::anyhow!("Failed to parse Java files: {}", e));
            }
            println!("parse_and_populate took {:?}", t0.elapsed());

            let t1 = std::time::Instant::now();
            if let Err(e) =
                JarAnalyzer::new().analyze_dependency(Path::new(&resolved_path), &mut conn, &id)
            {
                tracing::error!("Failed to analyze and cache JAR files: {}", e);
                return Err(anyhow::anyhow!(
                    "Failed to analyze and cache JAR files: {}",
                    e
                ));
            }
            println!("JarAnalyzer took {:?}", t1.elapsed());

            let t2 = std::time::Instant::now();
            if let Err(e) = DependencyInjectionAnalyzer::new().analyze(&mut conn) {
                tracing::error!("Failed to run dependency injection analysis: {}", e);
                return Err(anyhow::anyhow!(
                    "Failed to run dependency injection analysis: {}",
                    e
                ));
            }
            println!("DependencyInjectionAnalyzer took {:?}", t2.elapsed());

            let tr = std::time::Instant::now();
            if let Err(e) =
                astro_probe_java::router::SpringMvcRouteAnalyzer::new().analyze(&mut conn)
            {
                tracing::error!("Failed to run route mapping analysis: {}", e);
                return Err(anyhow::anyhow!(
                    "Failed to run route mapping analysis: {}",
                    e
                ));
            }
            println!("SpringMvcRouteAnalyzer took {:?}", tr.elapsed());

            let t3 = std::time::Instant::now();
            let ext_event = astro_probe_java::event::SpringEventLineageExtension::new();
            let ext_async = astro_probe_java::event::AsyncExecutionExtension::new();
            let ext_aop = astro_probe_java::event::SpringAopPointcutExtension::new();
            let extensions: Vec<&dyn astro_probe_core::cg::PointsToSolverExtension> =
                vec![&ext_event, &ext_async, &ext_aop];
            if let Err(e) =
                astro_probe_core::cg::PointsToSolver::new().solve(&mut conn, &extensions)
            {
                tracing::error!("Failed to run call graph analysis: {}", e);
                return Err(anyhow::anyhow!("Failed to run call graph analysis: {}", e));
            }
            println!("PointsToSolver took {:?}", t3.elapsed());

            let t4 = std::time::Instant::now();
            if let Err(e) = astro_probe_core::dfg::DfgAnalyzer::new().analyze(&conn) {
                tracing::error!("Failed to run data flow graph analysis: {}", e);
                return Err(anyhow::anyhow!(
                    "Failed to run data flow graph analysis: {}",
                    e
                ));
            }
            println!("DfgAnalyzer took {:?}", t4.elapsed());
        }

        let ws = Workspace {
            id: id.clone(),
            name,
            project_path: resolved_path,
            status: WorkspaceStatus::Loaded,
        };

        let state = WorkspaceState {
            workspace: ws.clone(),
            db_pool: Some(pool),
            last_accessed: Arc::new(RwLock::new(Instant::now())),
        };

        let mut guard = self
            .workspaces
            .write()
            .map_err(|e| anyhow::anyhow!("RwLock poisoned: {}", e))?;
        guard.insert(id, state);
        Ok(ws)
    }

    pub fn list_workspaces(&self) -> Vec<Workspace> {
        match self.workspaces.read() {
            Ok(guard) => guard
                .values()
                .map(|state| state.workspace.clone())
                .collect(),
            Err(_) => Vec::new(),
        }
    }

    pub fn delete_workspace(&self, id: &str) -> bool {
        let state = {
            let mut guard = match self.workspaces.write() {
                Ok(g) => g,
                Err(_) => return false,
            };
            guard.remove(id)
        };

        if let Some(state) = state {
            let project_path = state.workspace.project_path.clone();
            // Drop connection pool to release file lock before deletion
            drop(state.db_pool);

            // Remove the mapping entry from workspace_jars in the global cache
            if let Ok(global_conn) = astro_probe_db::establish_connection(get_global_cache_path()) {
                let _ =
                    global_conn.execute("DELETE FROM workspace_jars WHERE workspace_id = ?1", [id]);
            }

            // Delete the database file and WAL/SHM files outside the lock with retries
            let db_path = Path::new(&project_path).join(".astro-probe.db");
            let wal_path = Path::new(&project_path).join(".astro-probe.db-wal");
            let shm_path = Path::new(&project_path).join(".astro-probe.db-shm");

            // Delete WAL and SHM files first, then main database file
            let paths_to_delete = vec![wal_path, shm_path, db_path];
            for path in paths_to_delete {
                if path.exists() {
                    let mut deleted = false;
                    for attempt in 1..=20 {
                        match std::fs::remove_file(&path) {
                            Ok(_) => {
                                tracing::info!("Successfully deleted file: {:?}", path);
                                deleted = true;
                                break;
                            }
                            Err(e) => {
                                tracing::warn!(
                                    "Attempt {} to delete {:?} failed: {}. Retrying in 100ms...",
                                    attempt,
                                    path,
                                    e
                                );
                                std::thread::sleep(std::time::Duration::from_millis(100));
                            }
                        }
                    }
                    if !deleted {
                        tracing::error!("Failed to delete file {:?} after 20 attempts", path);
                    }
                }
            }
            true
        } else {
            false
        }
    }

    pub fn start_workspace(&self, id: &str) -> Option<Workspace> {
        let mut guard = self.workspaces.write().ok()?;
        if let Some(state) = guard.get_mut(id) {
            state.workspace.status = WorkspaceStatus::Loaded;
            if state.db_pool.is_none() {
                match self.load_db_pool(&state.workspace.project_path) {
                    Ok(pool) => {
                        state.db_pool = Some(pool);
                    }
                    Err(e) => {
                        tracing::error!("Failed to start workspace {}: {}", id, e);
                        return None;
                    }
                }
            }
            if let Ok(mut last_acc) = state.last_accessed.write() {
                *last_acc = Instant::now();
            }
            Some(state.workspace.clone())
        } else {
            None
        }
    }

    pub fn stop_workspace(&self, id: &str) -> Option<Workspace> {
        let mut guard = self.workspaces.write().ok()?;
        if let Some(state) = guard.get_mut(id) {
            state.workspace.status = WorkspaceStatus::Unloaded;
            state.db_pool = None; // drops pool
            Some(state.workspace.clone())
        } else {
            None
        }
    }

    pub fn get_db_pool_and_touch(&self, id: &str) -> Option<DbPool> {
        // 1. Acquire read lock
        let (project_path, should_load) = {
            let guard = self.workspaces.read().ok()?;
            let state = guard.get(id)?;
            if state.workspace.status == WorkspaceStatus::Loaded {
                if let Some(ref pool) = state.db_pool {
                    // Update last accessed
                    if let Ok(mut last_acc) = state.last_accessed.write() {
                        *last_acc = Instant::now();
                    }
                    return Some(pool.clone());
                } else {
                    // Loaded but pool is None
                    (state.workspace.project_path.clone(), true)
                }
            } else if state.workspace.status == WorkspaceStatus::Idle {
                (state.workspace.project_path.clone(), true)
            } else {
                // Status is Unloaded, return None
                return None;
            }
        };

        if should_load {
            // 2. Drop read lock and load pool outside the lock
            match self.load_db_pool(&project_path) {
                Ok(pool) => {
                    // 3. Acquire write lock
                    let mut guard = self.workspaces.write().ok()?;
                    if let Some(state) = guard.get_mut(id) {
                        // Re-verify the workspace state
                        if state.workspace.status == WorkspaceStatus::Idle
                            || state.workspace.status == WorkspaceStatus::Loaded
                        {
                            if let Some(ref existing_pool) = state.db_pool {
                                // Already loaded by another concurrent thread, return it and don't overwrite
                                if let Ok(mut last_acc) = state.last_accessed.write() {
                                    *last_acc = Instant::now();
                                }
                                return Some(existing_pool.clone());
                            }
                            state.workspace.status = WorkspaceStatus::Loaded;
                            state.db_pool = Some(pool.clone());
                            if let Ok(mut last_acc) = state.last_accessed.write() {
                                *last_acc = Instant::now();
                            }
                            return Some(pool);
                        }
                    }
                }
                Err(e) => {
                    tracing::error!("Failed to load database pool for workspace {}: {}", id, e);
                }
            }
        }

        None
    }
}

impl Default for WorkspaceManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[tokio::test]
    async fn test_list_workspaces_poisoned() {
        let manager = WorkspaceManager::new();
        // Poison the lock
        let lock_clone = Arc::clone(&manager.workspaces);
        let handle = thread::spawn(move || {
            let _guard = lock_clone.write().unwrap();
            panic!("Poisoning the lock intentionally");
        });
        let _ = handle.join();

        // Ensure list_workspaces does not panic and returns an empty list
        let workspaces = manager.list_workspaces();
        assert!(workspaces.is_empty());
    }

    #[tokio::test]
    async fn test_get_db_pool_and_touch_concurrency_avoidance() {
        let temp_dir = std::env::temp_dir();
        let path_a = temp_dir.join(format!("test_dir_a_{}", uuid::Uuid::new_v4()));
        let path_b = temp_dir.join(format!("test_dir_b_{}", uuid::Uuid::new_v4()));

        std::fs::create_dir_all(&path_a).unwrap();
        std::fs::create_dir_all(&path_b).unwrap();

        let path_a_str = path_a.to_str().unwrap().to_string();
        let path_b_str = path_b.to_str().unwrap().to_string();

        let manager = WorkspaceManager::new();

        // 1. Create workspace under path_a. This populates state.db_pool with pool_a.
        let ws = manager
            .create_workspace("test_workspace".to_string(), path_a_str.clone())
            .unwrap();

        // 2. Put workspace into Idle state, but KEEP the db_pool as Some(pool_a)
        {
            let mut guard = manager.workspaces.write().unwrap();
            let state = guard.get_mut(&ws.id).unwrap();
            state.workspace.status = WorkspaceStatus::Idle;
            // Change project path to path_b so that any new load_db_pool call will connect to path_b
            state.workspace.project_path = path_b_str.clone();
        }

        // 3. Call get_db_pool_and_touch.
        // It should see status is Idle, and should_load = true.
        // It will drop the read lock and call load_db_pool(&path_b), getting pool_b.
        // Then it acquires the write lock, sees state.db_pool is already Some(pool_a),
        // and returns pool_a WITHOUT overwriting state.db_pool.
        let returned_pool = manager.get_db_pool_and_touch(&ws.id).unwrap();

        // 4. Verify that returned_pool is pool_a (pointing to path_a), not pool_b (pointing to path_b).
        // We do this by inserting a dummy record through returned_pool,
        // and then verifying if it exists in path_a's DB vs path_b's DB.
        let conn = returned_pool.get().unwrap();
        conn.execute(
            "INSERT INTO call_edges (caller, callee) VALUES ('caller_test', 'callee_test');",
            [],
        )
        .unwrap();

        // Open connections directly to files to check where the data was written
        let db_file_a = path_a.join(".astro-probe.db");
        let db_file_b = path_b.join(".astro-probe.db");

        let conn_a = rusqlite::Connection::open(db_file_a).unwrap();
        let count_a: i64 = conn_a.query_row(
            "SELECT count(*) FROM call_edges WHERE caller='caller_test' AND callee='callee_test';",
            [],
            |r| r.get(0)
        ).unwrap();

        let conn_b = rusqlite::Connection::open(db_file_b).unwrap();
        let count_b: i64 = conn_b.query_row(
            "SELECT count(*) FROM call_edges WHERE caller='caller_test' AND callee='callee_test';",
            [],
            |r| r.get(0)
        ).unwrap();

        assert_eq!(
            count_a, 1,
            "The record should have been written to database A (pool_a)"
        );
        assert_eq!(
            count_b, 0,
            "The record should NOT have been written to database B (pool_b)"
        );

        // Clean up
        drop(conn);
        drop(returned_pool);
        drop(manager);

        // Wait briefly for manager's background thread or drop to complete
        std::thread::sleep(std::time::Duration::from_millis(100));

        let _ = std::fs::remove_dir_all(&path_a);
        let _ = std::fs::remove_dir_all(&path_b);
    }
}
