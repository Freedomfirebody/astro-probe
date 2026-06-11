use astro_probe_server::kernel::WorkspaceManager;
use std::path::Path;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

static TEST_MUTEX: OnceLock<Mutex<()>> = OnceLock::new();

fn lock_test_env() -> std::sync::MutexGuard<'static, ()> {
    TEST_MUTEX.get_or_init(|| Mutex::new(())).lock().unwrap()
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

#[tokio::test]
async fn test_perf_benchmark_medium_spring() {
    let _lock = lock_test_env();
    let _env = EnvGuard::new("med_spring_perf");
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let project_path = manifest_dir
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("test-samples")
        .join("medium-spring");

    assert!(
        project_path.exists(),
        "medium-spring test-sample path must exist"
    );

    let guard = TempProjectGuard::new(&project_path, "medium_spring_perf");

    let db_path = guard.temp_dir.join(".astro-probe.db");
    if db_path.exists() {
        std::fs::remove_file(&db_path).ok();
    }

    let manager = WorkspaceManager::new();

    // 1. Initial Analysis
    println!("Starting initial analysis of medium-spring...");
    let start_initial = Instant::now();
    let ws = manager
        .create_workspace(
            "medium-spring-initial".to_string(),
            guard.temp_dir.to_string_lossy().to_string(),
        )
        .expect("Failed to create initial workspace");
    let initial_duration = start_initial.elapsed();
    println!(
        "Initial analysis of medium-spring took: {:?}",
        initial_duration
    );

    // 2. Modify a single file
    let file_to_modify = guard
        .temp_dir
        .join("src")
        .join("main")
        .join("java")
        .join("com")
        .join("example")
        .join("medium")
        .join("service")
        .join("impl")
        .join("UserServiceImpl.java");

    assert!(file_to_modify.exists(), "UserServiceImpl.java must exist");
    let original_content = std::fs::read_to_string(&file_to_modify).expect("Failed to read file");

    let modified_content = format!("{}\n// benchmark comment\n", original_content);
    std::fs::write(&file_to_modify, &modified_content).expect("Failed to write modified file");

    // 3. Incremental Analysis
    println!("Starting incremental analysis of medium-spring...");
    manager.stop_workspace(&ws.id);
    std::thread::sleep(std::time::Duration::from_millis(500));
    let start_incremental = Instant::now();
    let ws_incremental = manager
        .create_workspace(
            "medium-spring-incremental".to_string(),
            guard.temp_dir.to_string_lossy().to_string(),
        )
        .expect("Failed to create incremental workspace");
    let incremental_duration = start_incremental.elapsed();
    println!(
        "Incremental analysis of medium-spring took: {:?}",
        incremental_duration
    );

    // Clean up workspaces
    manager.delete_workspace(&ws.id);
    manager.delete_workspace(&ws_incremental.id);

    let speedup = incremental_duration.as_secs_f64() / initial_duration.as_secs_f64();
    println!("medium-spring Speedup ratio: {:.2}%", speedup * 100.0);

    assert!(
        incremental_duration < initial_duration / 2,
        "Incremental re-analysis ({:?}) must take <50% of the full analysis time ({:?})",
        incremental_duration,
        initial_duration
    );
}

#[tokio::test]
async fn test_perf_benchmark_nacos() {
    let _lock = lock_test_env();
    let _env = EnvGuard::new("nacos_perf");
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let project_path = manifest_dir
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("test-samples")
        .join("Nacos");

    assert!(project_path.exists(), "Nacos test-sample path must exist");

    let guard = TempProjectGuard::new(&project_path, "nacos_perf");

    let db_path = guard.temp_dir.join(".astro-probe.db");
    if db_path.exists() {
        std::fs::remove_file(&db_path).ok();
    }

    let manager = WorkspaceManager::new();

    // 1. Initial Analysis
    println!("Starting initial analysis of Nacos...");
    let start_initial = Instant::now();
    let ws = manager
        .create_workspace(
            "nacos-initial".to_string(),
            guard.temp_dir.to_string_lossy().to_string(),
        )
        .expect("Failed to create initial workspace");
    let initial_duration = start_initial.elapsed();
    println!("Initial analysis of Nacos took: {:?}", initial_duration);

    // 2. Modify a single file
    let file_to_modify = guard
        .temp_dir
        .join("address")
        .join("src")
        .join("main")
        .join("java")
        .join("com")
        .join("alibaba")
        .join("nacos")
        .join("address")
        .join("AddressServer.java");

    assert!(file_to_modify.exists(), "AddressServer.java must exist");
    let original_content = std::fs::read_to_string(&file_to_modify).expect("Failed to read file");

    let modified_content = format!("{}\n// benchmark comment\n", original_content);
    std::fs::write(&file_to_modify, &modified_content).expect("Failed to write modified file");

    // 3. Incremental Analysis
    println!("Starting incremental analysis of Nacos...");
    manager.stop_workspace(&ws.id);
    std::thread::sleep(std::time::Duration::from_millis(500));
    let start_incremental = Instant::now();
    let ws_incremental = manager
        .create_workspace(
            "nacos-incremental".to_string(),
            guard.temp_dir.to_string_lossy().to_string(),
        )
        .expect("Failed to create incremental workspace");
    let incremental_duration = start_incremental.elapsed();
    println!(
        "Incremental analysis of Nacos took: {:?}",
        incremental_duration
    );

    // Clean up file modification
    std::fs::write(&file_to_modify, &original_content).expect("Failed to restore file");

    // Clean up workspaces
    manager.delete_workspace(&ws.id);
    manager.delete_workspace(&ws_incremental.id);

    if db_path.exists() {
        std::fs::remove_file(&db_path).ok();
    }

    println!("Nacos Incremental duration: {:?}", incremental_duration);
    assert!(
        incremental_duration < std::time::Duration::from_secs(30),
        "Incremental re-analysis of Nacos must take < 30 seconds, took: {:?}",
        incremental_duration
    );
}
