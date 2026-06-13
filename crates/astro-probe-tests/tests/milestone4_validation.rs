use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use astro_probe_server::kernel::WorkspaceManager;

static TEST_MUTEX: OnceLock<Mutex<()>> = OnceLock::new();

fn lock_test_env() -> std::sync::MutexGuard<'static, ()> {
    TEST_MUTEX
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

struct EnvGuard {
    temp_path: PathBuf,
}

impl EnvGuard {
    fn new(suffix: &str) -> Self {
        let temp_path = std::env::temp_dir().join(format!(
            "global_cache_{}_{}.db",
            suffix,
            uuid::Uuid::new_v4()
        ));
        std::env::set_var("ASTRO_PROBE_GLOBAL_CACHE_PATH", temp_path.to_str().unwrap());
        Self { temp_path }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        std::env::remove_var("ASTRO_PROBE_GLOBAL_CACHE_PATH");
        if self.temp_path.exists() {
            std::fs::remove_file(&self.temp_path).ok();
        }
    }
}

struct TempProjectGuard {
    temp_dir: PathBuf,
}

impl TempProjectGuard {
    fn new(source_dir: &Path, prefix: &str) -> Self {
        let target_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("target");
        let temp_dir = target_dir.join(format!("{}_{}", prefix, uuid::Uuid::new_v4()));
        copy_dir_all(source_dir, &temp_dir).expect("Failed to copy project directory");
        Self { temp_dir }
    }
}

impl Drop for TempProjectGuard {
    fn drop(&mut self) {
        if self.temp_dir.exists() {
            std::fs::remove_dir_all(&self.temp_dir).ok();
        }
    }
}

fn copy_dir_all(src: impl AsRef<Path>, dst: impl AsRef<Path>) -> std::io::Result<()> {
    std::fs::create_dir_all(&dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        if ty.is_dir() {
            copy_dir_all(entry.path(), dst.as_ref().join(entry.file_name()))?;
        } else {
            std::fs::copy(entry.path(), dst.as_ref().join(entry.file_name()))?;
        }
    }
    Ok(())
}

fn checkpoint_db(db_path: &Path) {
    if let Ok(conn) = rusqlite::Connection::open(db_path) {
        let _ = conn.execute("PRAGMA wal_checkpoint(TRUNCATE);", []);
    }
}

#[tokio::test]
async fn save_dbs_for_inspection() {
    let _lock = lock_test_env();
    let _env = EnvGuard::new("milestone4_save");

    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));

    // 1. Analyze and save complex-spring
    let complex_path = manifest_dir
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("test-samples")
        .join("complex-spring");

    let complex_guard = TempProjectGuard::new(&complex_path, "complex_spring_verify");
    let db_path = complex_guard.temp_dir.join(".astro-probe.db");
    if db_path.exists() {
        std::fs::remove_file(&db_path).ok();
    }

    {
        let manager = WorkspaceManager::new();
        let _ws = manager
            .create_workspace(
                "complex-spring-verify".to_string(),
                complex_guard.temp_dir.to_string_lossy().to_string(),
            )
            .expect("Failed to create complex-spring workspace");
    }

    // Checkpoint DB
    checkpoint_db(&db_path);

    // Copy to our agent directory
    let project_root = manifest_dir.parent().unwrap().parent().unwrap();
    let dest_complex = project_root
        .join(".agents")
        .join("challenger_m4_2")
        .join("complex-spring.db");
    std::fs::create_dir_all(dest_complex.parent().unwrap()).ok();
    if dest_complex.exists() {
        std::fs::remove_file(&dest_complex).ok();
    }
    std::fs::copy(&db_path, &dest_complex).unwrap();

    // 2. Analyze and save medium-spring
    let medium_path = manifest_dir
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("test-samples")
        .join("medium-spring");

    let medium_guard = TempProjectGuard::new(&medium_path, "medium_spring_verify");
    let db_path2 = medium_guard.temp_dir.join(".astro-probe.db");
    if db_path2.exists() {
        std::fs::remove_file(&db_path2).ok();
    }

    {
        let manager2 = WorkspaceManager::new();
        let _ws2 = manager2
            .create_workspace(
                "medium-spring-verify".to_string(),
                medium_guard.temp_dir.to_string_lossy().to_string(),
            )
            .expect("Failed to create medium-spring workspace");
    }

    // Checkpoint DB
    checkpoint_db(&db_path2);

    // Copy to our agent directory
    let dest_medium = project_root
        .join(".agents")
        .join("challenger_m4_2")
        .join("medium-spring.db");
    std::fs::create_dir_all(dest_medium.parent().unwrap()).ok();
    if dest_medium.exists() {
        std::fs::remove_file(&dest_medium).ok();
    }
    std::fs::copy(&db_path2, &dest_medium).unwrap();
}
