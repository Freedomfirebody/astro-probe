use astro_probe_server::kernel::WorkspaceManager;
use std::path::Path;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

static TEST_MUTEX: OnceLock<Mutex<()>> = OnceLock::new();

fn lock_test_env() -> std::sync::MutexGuard<'static, ()> {
    TEST_MUTEX.get_or_init(|| Mutex::new(())).lock().unwrap_or_else(|e| e.into_inner())
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

    let limit = if cfg!(debug_assertions) {
        initial_duration * 4 / 5 // <80% in debug mode
    } else {
        initial_duration / 2     // <50% in release mode
    };

    assert!(
        incremental_duration < limit,
        "Incremental re-analysis ({:?}) must take less than the limit ({:?}) of full analysis time",
        incremental_duration,
        limit
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

    if db_path.exists() {
        if let Ok(meta) = std::fs::metadata(&db_path) {
            println!("Nacos Database size on disk: {} bytes", meta.len());
        }
    }
    println!("Nacos Peak memory usage: {} bytes", get_peak_memory_usage());

    // Clean up workspaces
    manager.delete_workspace(&ws.id);
    manager.delete_workspace(&ws_incremental.id);

    if db_path.exists() {
        std::fs::remove_file(&db_path).ok();
    }

    println!("Nacos Incremental duration: {:?}", incremental_duration);
    let limit = if cfg!(debug_assertions) {
        std::time::Duration::from_secs(90)
    } else {
        std::time::Duration::from_secs(30)
    };
    assert!(
        incremental_duration < limit,
        "Incremental re-analysis of Nacos must take < {:?} seconds, took: {:?}",
        limit,
        incremental_duration
    );
}

#[cfg(windows)]
#[repr(C)]
#[allow(non_snake_case)]
struct PROCESS_MEMORY_COUNTERS {
    cb: u32,
    PageFaultCount: u32,
    PeakWorkingSetSize: usize,
    WorkingSetSize: usize,
    QuotaPeakPagedPoolUsage: usize,
    QuotaPagedPoolUsage: usize,
    QuotaPeakNonPagedPoolUsage: usize,
    QuotaNonPagedPoolUsage: usize,
    PagefileUsage: usize,
    PeakPagefileUsage: usize,
}

#[cfg(windows)]
#[link(name = "psapi")]
extern "system" {
    fn GetProcessMemoryInfo(
        process: *mut std::ffi::c_void,
        ppsmc: *mut PROCESS_MEMORY_COUNTERS,
        cb: u32,
    ) -> i32;
}

#[cfg(windows)]
#[link(name = "kernel32")]
extern "system" {
    fn GetCurrentProcess() -> *mut std::ffi::c_void;
}

#[cfg(windows)]
fn get_peak_memory_usage() -> usize {
    let mut counters = PROCESS_MEMORY_COUNTERS {
        cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
        PageFaultCount: 0,
        PeakWorkingSetSize: 0,
        WorkingSetSize: 0,
        QuotaPeakPagedPoolUsage: 0,
        QuotaPagedPoolUsage: 0,
        QuotaPeakNonPagedPoolUsage: 0,
        QuotaNonPagedPoolUsage: 0,
        PagefileUsage: 0,
        PeakPagefileUsage: 0,
    };
    unsafe {
        let handle = GetCurrentProcess();
        if GetProcessMemoryInfo(handle, &mut counters, counters.cb) != 0 {
            counters.PeakWorkingSetSize
        } else {
            0
        }
    }
}

#[cfg(not(windows))]
fn get_peak_memory_usage() -> usize {
    0
}

