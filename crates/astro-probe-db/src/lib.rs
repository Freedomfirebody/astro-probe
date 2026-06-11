#![allow(clippy::type_complexity)]

use r2d2::{ManageConnection, Pool};
use rusqlite::Result;
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("Database error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("Connection pool error: {0}")]
    Pool(#[from] r2d2::Error),
}

pub struct SqliteConnectionManager {
    path: PathBuf,
    init: Option<Box<dyn Fn(&mut rusqlite::Connection) -> Result<()> + Send + Sync>>,
}

impl std::fmt::Debug for SqliteConnectionManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SqliteConnectionManager")
            .field("path", &self.path)
            .field("init", &self.init.as_ref().map(|_| "Fn(...)"))
            .finish()
    }
}

impl SqliteConnectionManager {
    pub fn file<P: AsRef<Path>>(path: P) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
            init: None,
        }
    }

    pub fn with_init<F>(mut self, init: F) -> Self
    where
        F: Fn(&mut rusqlite::Connection) -> Result<()> + Send + Sync + 'static,
    {
        self.init = Some(Box::new(init));
        self
    }
}

impl ManageConnection for SqliteConnectionManager {
    type Connection = rusqlite::Connection;
    type Error = rusqlite::Error;

    fn connect(&self) -> std::result::Result<Self::Connection, Self::Error> {
        let mut conn = rusqlite::Connection::open(&self.path)?;
        if let Some(ref init) = self.init {
            init(&mut conn)?;
        }
        Ok(conn)
    }

    fn is_valid(&self, conn: &mut Self::Connection) -> std::result::Result<(), Self::Error> {
        conn.execute_batch("")
    }

    fn has_broken(&self, _conn: &mut Self::Connection) -> bool {
        false
    }
}

pub type DbPool = Pool<SqliteConnectionManager>;

pub fn establish_connection<P: AsRef<Path>>(path: P) -> Result<rusqlite::Connection> {
    let conn = rusqlite::Connection::open(path)?;
    conn.busy_timeout(Duration::from_secs(5))?;
    let journal_mode: String = conn.query_row("PRAGMA journal_mode;", [], |row| row.get(0))?;
    if journal_mode.to_lowercase() != "wal" {
        conn.execute_batch("PRAGMA journal_mode = WAL;")?;
    }
    conn.execute_batch(
        "PRAGMA synchronous = NORMAL;
         PRAGMA cache_size = -64000;
         PRAGMA temp_store = MEMORY;",
    )?;
    Ok(conn)
}

pub fn establish_connection_pool<P: AsRef<Path>>(path: P) -> std::result::Result<DbPool, DbError> {
    let manager = SqliteConnectionManager::file(path).with_init(|c| {
        c.busy_timeout(Duration::from_secs(5))?;
        let journal_mode: String = c.query_row("PRAGMA journal_mode;", [], |row| row.get(0))?;
        if journal_mode.to_lowercase() != "wal" {
            c.execute_batch("PRAGMA journal_mode = WAL;")?;
        }
        c.execute_batch(
            "PRAGMA synchronous = NORMAL;
             PRAGMA cache_size = -64000;
             PRAGMA temp_store = MEMORY;",
        )?;
        Ok(())
    });
    let pool = Pool::builder().max_size(2).build(manager)?;
    Ok(pool)
}

pub fn init_db(conn: &rusqlite::Connection) -> Result<()> {
    conn.execute("BEGIN IMMEDIATE TRANSACTION;", [])?;

    let create_result = (|| -> Result<()> {
        conn.execute(
            "CREATE TABLE IF NOT EXISTS call_edges (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                caller TEXT NOT NULL,
                callee TEXT NOT NULL,
                is_virtual INTEGER NOT NULL DEFAULT 0,
                UNIQUE(caller, callee)
            );",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_call_edges_caller ON call_edges(caller);",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_call_edges_callee ON call_edges(callee);",
            [],
        )?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS lineage_edges (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                from_node TEXT NOT NULL,
                to_node TEXT NOT NULL,
                edge_type TEXT NOT NULL,
                UNIQUE(from_node, to_node)
            );",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_lineage_from ON lineage_edges(from_node);",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_lineage_to ON lineage_edges(to_node);",
            [],
        )?;

        // Tables for Points-To Analysis Engine
        conn.execute(
            "CREATE TABLE IF NOT EXISTS classes (
                fqn TEXT PRIMARY KEY,
                kind TEXT NOT NULL
            );",
            [],
        )?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS class_hierarchy (
                class_fqn TEXT NOT NULL,
                parent_fqn TEXT NOT NULL,
                PRIMARY KEY (class_fqn, parent_fqn)
            );",
            [],
        )?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS method_declarations (
                method_fqn TEXT NOT NULL,
                class_fqn TEXT NOT NULL,
                method_name TEXT NOT NULL,
                params TEXT NOT NULL,
                PRIMARY KEY (method_fqn, params)
            );",
            [],
        )?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS allocation_sites (
                alloc_id TEXT PRIMARY KEY,
                class_fqn TEXT NOT NULL,
                method_fqn TEXT NOT NULL
            );",
            [],
        )?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS source_assignments (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                lhs TEXT NOT NULL,
                rhs TEXT NOT NULL,
                assignment_type TEXT NOT NULL,
                method_fqn TEXT NOT NULL
            );",
            [],
        )?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS call_sites (
                call_id TEXT PRIMARY KEY,
                method_fqn TEXT NOT NULL,
                receiver TEXT,
                method_name TEXT NOT NULL,
                lhs TEXT,
                static_callee TEXT
            );",
            [],
        )?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS call_arguments (
                call_id TEXT NOT NULL,
                arg_index INTEGER NOT NULL,
                arg_var TEXT NOT NULL,
                arg_type TEXT,
                PRIMARY KEY (call_id, arg_index)
            );",
            [],
        )?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS points_to_sets (
                variable_fqn TEXT NOT NULL,
                alloc_id TEXT NOT NULL,
                PRIMARY KEY (variable_fqn, alloc_id)
            );",
            [],
        )?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS library_classes (
                fqn TEXT PRIMARY KEY
            );",
            [],
        )?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS class_annotations (
                class_fqn TEXT NOT NULL,
                annotation_name TEXT NOT NULL,
                PRIMARY KEY (class_fqn, annotation_name)
            );",
            [],
        )?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS field_annotations (
                class_fqn TEXT NOT NULL,
                field_name TEXT NOT NULL,
                annotation_name TEXT NOT NULL,
                PRIMARY KEY (class_fqn, field_name, annotation_name)
            );",
            [],
        )?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS method_annotations (
                method_fqn TEXT NOT NULL,
                annotation_name TEXT NOT NULL,
                PRIMARY KEY (method_fqn, annotation_name)
            );",
            [],
        )?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS parameter_annotations (
                method_fqn TEXT NOT NULL,
                parameter_name TEXT NOT NULL,
                annotation_name TEXT NOT NULL,
                PRIMARY KEY (method_fqn, parameter_name, annotation_name)
            );",
            [],
        )?;

        // Milestone 2 tables
        conn.execute(
            "CREATE TABLE IF NOT EXISTS file_hashes (
                file_path TEXT PRIMARY KEY,
                hash TEXT NOT NULL
            );",
            [],
        )?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS file_facts_metadata (
                file_path TEXT,
                class_fqn TEXT,
                PRIMARY KEY (file_path, class_fqn)
            );",
            [],
        )?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS method_summaries (
                method_fqn TEXT NOT NULL,
                param_index INTEGER NOT NULL,
                PRIMARY KEY (method_fqn, param_index)
            );",
            [],
        )?;

        Ok(())
    })();

    match create_result {
        Ok(_) => {
            conn.execute("COMMIT;", [])?;
            Ok(())
        }
        Err(e) => {
            let _ = conn.execute("ROLLBACK;", []);
            Err(e)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_establish_connection_and_pragmas() {
        let db_path = std::env::temp_dir().join(format!("test_db_{}.db", uuid::Uuid::new_v4()));
        let conn = establish_connection(&db_path).unwrap();

        // Verify journal mode is WAL
        let journal_mode: String = conn
            .query_row("PRAGMA journal_mode;", [], |row| row.get(0))
            .unwrap();
        assert_eq!(journal_mode.to_uppercase(), "WAL");

        // Verify synchronous is NORMAL (1)
        let synchronous: i64 = conn
            .query_row("PRAGMA synchronous;", [], |row| row.get(0))
            .unwrap();
        assert_eq!(synchronous, 1);

        // Verify busy timeout is 5000ms
        let busy_timeout: i64 = conn
            .query_row("PRAGMA busy_timeout;", [], |row| row.get(0))
            .unwrap();
        assert_eq!(busy_timeout, 5000);

        // Clean up
        let _ = std::fs::remove_file(&db_path);
    }

    #[test]
    fn test_establish_connection_pool_and_pragmas() {
        let db_path = std::env::temp_dir().join(format!("test_db_{}.db", uuid::Uuid::new_v4()));
        let pool = establish_connection_pool(&db_path).unwrap();
        let conn = pool.get().unwrap();

        let journal_mode: String = conn
            .query_row("PRAGMA journal_mode;", [], |row| row.get(0))
            .unwrap();
        assert_eq!(journal_mode.to_uppercase(), "WAL");

        let synchronous: i64 = conn
            .query_row("PRAGMA synchronous;", [], |row| row.get(0))
            .unwrap();
        assert_eq!(synchronous, 1);

        let busy_timeout: i64 = conn
            .query_row("PRAGMA busy_timeout;", [], |row| row.get(0))
            .unwrap();
        assert_eq!(busy_timeout, 5000);

        // Clean up
        drop(conn);
        drop(pool);
        let _ = std::fs::remove_file(&db_path);
    }

    #[test]
    fn test_init_db_creates_tables_and_indexes() {
        let db_path = std::env::temp_dir().join(format!("test_db_{}.db", uuid::Uuid::new_v4()));
        let conn = establish_connection(&db_path).unwrap();
        init_db(&conn).unwrap();

        // Verify call_edges table exists
        let has_call_edges: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='call_edges';",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(has_call_edges, 1);

        // Verify lineage_edges table exists
        let has_lineage_edges: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='lineage_edges';",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(has_lineage_edges, 1);

        // Clean up
        drop(conn);
        let _ = std::fs::remove_file(&db_path);
    }

    #[test]
    fn test_sqlite_concurrency() {
        use std::thread;
        let db_path = std::env::temp_dir().join(format!("test_db_{}.db", uuid::Uuid::new_v4()));
        let pool = establish_connection_pool(&db_path).unwrap();

        // Initialize the DB structure
        {
            let conn = pool.get().unwrap();
            init_db(&conn).unwrap();
        }

        let num_threads = 10;
        let inserts_per_thread = 50;
        let mut handles = Vec::new();

        for t in 0..num_threads {
            let pool_clone = pool.clone();
            let handle = thread::spawn(move || {
                for i in 0..inserts_per_thread {
                    let conn = pool_clone.get().unwrap();
                    let caller = format!("caller_t{}_{}", t, i);
                    let callee = format!("callee_t{}_{}", t, i);
                    let res = conn.execute(
                        "INSERT OR IGNORE INTO call_edges (caller, callee, is_virtual) VALUES (?1, ?2, 0)",
                        [&caller, &callee],
                    );
                    assert!(res.is_ok(), "Insert failed: {:?}", res.err());
                }
            });
            handles.push(handle);
        }

        for handle in handles {
            handle.join().unwrap();
        }

        // Verify all rows are inserted
        let conn = pool.get().unwrap();
        let count: i64 = conn
            .query_row("SELECT count(*) FROM call_edges;", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, num_threads * inserts_per_thread);

        // Clean up
        drop(conn);
        drop(pool);
        let _ = std::fs::remove_file(&db_path);
    }
}
