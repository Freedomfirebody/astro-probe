use astro_probe_server::kernel::WorkspaceManager;
use std::path::Path;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

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
async fn test_end_to_end_simple_spring() {
    let _lock = lock_test_env();
    let _env = EnvGuard::new("simple_spring");
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let project_path = manifest_dir
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("test-samples")
        .join("simple-spring");

    assert!(
        project_path.exists(),
        "simple-spring test-sample path must exist"
    );

    let guard = TempProjectGuard::new(&project_path, "simple_spring_test");

    let manager = WorkspaceManager::new();
    let ws = manager
        .create_workspace(
            "simple-spring-test".to_string(),
            guard.temp_dir.to_string_lossy().to_string(),
        )
        .expect("Failed to create workspace");

    assert_eq!(ws.name, "simple-spring-test");

    let pool = manager
        .get_db_pool_and_touch(&ws.id)
        .expect("Failed to get DB pool");

    let conn = pool.get().expect("Failed to get connection");

    // Query counts to see what was parsed
    let class_count: i64 = conn
        .query_row("SELECT count(*) FROM classes", [], |r| r.get(0))
        .unwrap();
    let method_count: i64 = conn
        .query_row("SELECT count(*) FROM method_declarations", [], |r| r.get(0))
        .unwrap();
    let class_ann_count: i64 = conn
        .query_row("SELECT count(*) FROM class_annotations", [], |r| r.get(0))
        .unwrap();
    let field_ann_count: i64 = conn
        .query_row("SELECT count(*) FROM field_annotations", [], |r| r.get(0))
        .unwrap();
    println!(
        "Parsed classes: {}, methods: {}, class_annotations: {}, field_annotations: {}",
        class_count, method_count, class_ann_count, field_ann_count
    );

    assert!(
        field_ann_count > 0,
        "Should have parsed some field annotations"
    );

    // Test a basic call graph query
    let mut stmt_cg = conn
        .prepare("SELECT count(*) FROM call_edges")
        .expect("Failed to prepare call edges query");
    let cg_count: i64 = stmt_cg
        .query_row([], |r| r.get(0))
        .expect("Failed to query call edges");
    assert!(cg_count > 0, "Should have generated call edges");

    // Test delete workspace
    let delete_success = manager.delete_workspace(&ws.id);
    assert!(delete_success, "Workspace deletion should succeed");
}

#[test]
fn test_copy_jar_facts_to_local_method_summaries() {
    let _lock = lock_test_env();
    let env = EnvGuard::new("copy_jar");
    let local_conn = rusqlite::Connection::open_in_memory().unwrap();
    astro_probe_db::init_db(&local_conn).unwrap();

    let global_conn = rusqlite::Connection::open(&env.temp_path).unwrap();

    astro_probe_java::jar::init_global_db(&global_conn).unwrap();

    let jar_hash = "mock_jar_hash_123";

    global_conn.execute(
        "INSERT INTO cached_method_summaries (jar_hash, method_fqn, param_index) VALUES (?1, ?2, ?3)",
        [jar_hash, "com.test.Identity.f(java.lang.Object)", "0"],
    ).unwrap();

    astro_probe_java::jar::copy_jar_facts_to_local(&local_conn, jar_hash).unwrap();

    let count: i64 = local_conn.query_row(
        "SELECT count(*) FROM method_summaries WHERE method_fqn = 'com.test.Identity.f(java.lang.Object)' AND param_index = 0",
        [],
        |r| r.get(0)
    ).unwrap();
    assert_eq!(count, 1);
}

#[test]
fn test_supernode_detection_and_bypass() {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    astro_probe_db::init_db(&conn).unwrap();

    conn.execute(
        "INSERT INTO call_sites (call_id, method_fqn, receiver, method_name, lhs, static_callee) \
         VALUES ('call_1', 'com.test.Main.run()', NULL, 'toString', 'com.test.Main.run()#x', 'java.lang.StringBuilder.toString()')",
        []
    ).unwrap();

    let solver = astro_probe_core::cg::PointsToSolver::new();
    solver.solve(&conn).unwrap();

    let count: i64 = conn
        .query_row(
            "SELECT count(*) FROM points_to_sets \
         WHERE variable_fqn = 'com.test.Main.run()#x' \
         AND alloc_id = 'SupernodeReturn:java.lang.StringBuilder.toString()'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 1);
}

#[test]
fn test_method_summary_propagation_bypass() {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    astro_probe_db::init_db(&conn).unwrap();

    conn.execute(
        "INSERT INTO method_summaries (method_fqn, param_index) VALUES (?1, ?2)",
        ["com.test.Identity.f(java.lang.Object)", "0"],
    )
    .unwrap();

    conn.execute(
        "INSERT INTO source_assignments (lhs, rhs, assignment_type, method_fqn) \
         VALUES ('com.test.Main.run()#arg', 'AllocA', 'ALLOC', 'com.test.Main.run()')",
        [],
    )
    .unwrap();

    conn.execute(
        "INSERT INTO call_sites (call_id, method_fqn, receiver, method_name, lhs, static_callee) \
         VALUES ('call_1', 'com.test.Main.run()', NULL, 'f', 'com.test.Main.run()#ret', 'com.test.Identity.f(java.lang.Object)')",
        []
    ).unwrap();

    conn.execute(
        "INSERT INTO call_arguments (call_id, arg_index, arg_var, arg_type) \
         VALUES ('call_1', 0, 'com.test.Main.run()#arg', 'java.lang.Object')",
        [],
    )
    .unwrap();

    let solver = astro_probe_core::cg::PointsToSolver::new();
    solver.solve(&conn).unwrap();

    let count: i64 = conn
        .query_row(
            "SELECT count(*) FROM points_to_sets \
         WHERE variable_fqn = 'com.test.Main.run()#ret' \
         AND alloc_id = 'AllocA'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 1);
}

#[test]
fn test_transitive_dfg_reduction() {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    astro_probe_db::init_db(&conn).unwrap();

    conn.execute(
        "INSERT INTO source_assignments (lhs, rhs, assignment_type, method_fqn) \
         VALUES ('temp_alloc_1', 'A', 'COPY', 'com.test.Main.run()')",
        [],
    )
    .unwrap();

    conn.execute(
        "INSERT INTO source_assignments (lhs, rhs, assignment_type, method_fqn) \
         VALUES ('B', 'temp_alloc_1', 'FIELD_WRITE', 'com.test.Main.run()')",
        [],
    )
    .unwrap();

    let analyzer = astro_probe_core::dfg::DfgAnalyzer::new();
    analyzer.analyze(&conn).unwrap();

    let count_ab: i64 = conn.query_row(
        "SELECT count(*) FROM lineage_edges WHERE from_node = 'A' AND to_node = 'B' AND edge_type = 'FIELD_WRITE'",
        [],
        |r| r.get(0)
    ).unwrap();
    assert_eq!(count_ab, 1);

    let count_temp: i64 = conn.query_row(
        "SELECT count(*) FROM lineage_edges WHERE from_node = 'temp_alloc_1' OR to_node = 'temp_alloc_1'",
        [],
        |r| r.get(0)
    ).unwrap();
    assert_eq!(count_temp, 0);
}

#[tokio::test]
async fn test_incremental_analysis() {
    let _lock = lock_test_env();
    let _env = EnvGuard::new("incremental");
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let target_dir = manifest_dir
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("target");

    let test_proj_dir = target_dir.join(format!("test_proj_{}", uuid::Uuid::new_v4()));
    let src_dir = test_proj_dir
        .join("src")
        .join("main")
        .join("java")
        .join("com")
        .join("test");
    std::fs::create_dir_all(&src_dir).unwrap();

    let a_content = r#"
package com.test;
public class A {
    public void hello() {
        Class0 c = new Class0();
    }
}
"#;
    std::fs::write(src_dir.join("A.java"), a_content).unwrap();

    for i in 0..20 {
        let class_content = format!(
            "package com.test;\npublic class Class{} {{\n  public void f() {{}}\n}}",
            i
        );
        std::fs::write(src_dir.join(format!("Class{}.java", i)), class_content).unwrap();
    }

    let manager = WorkspaceManager::new();
    let ws_name = format!("test_incremental_{}", uuid::Uuid::new_v4());

    let start_initial = std::time::Instant::now();
    let ws = manager
        .create_workspace(ws_name.clone(), test_proj_dir.to_string_lossy().to_string())
        .expect("Failed to create workspace initial");
    let initial_duration = start_initial.elapsed();

    let pool = manager.get_db_pool_and_touch(&ws.id).expect("Pool failed");
    let conn = pool.get().expect("Conn failed");
    let class_count_initial: i64 = conn
        .query_row("SELECT count(*) FROM classes", [], |r| r.get(0))
        .unwrap();
    assert!(class_count_initial >= 21);

    drop(conn);
    drop(pool);
    drop(manager);

    let a_modified_content = r#"
package com.test;
public class A {
    public void hello() {
        Class0 c = new Class0();
        // modified comment
    }
}
"#;
    std::fs::write(src_dir.join("A.java"), a_modified_content).unwrap();

    let start_re = std::time::Instant::now();
    let manager2 = WorkspaceManager::new();
    let ws2 = manager2
        .create_workspace(ws_name, test_proj_dir.to_string_lossy().to_string())
        .expect("Failed to create workspace incremental");
    let re_duration = start_re.elapsed();

    let pool2 = manager2
        .get_db_pool_and_touch(&ws2.id)
        .expect("Pool2 failed");
    let conn2 = pool2.get().expect("Conn2 failed");
    let class_count_re: i64 = conn2
        .query_row("SELECT count(*) FROM classes", [], |r| r.get(0))
        .unwrap();
    assert_eq!(class_count_re, class_count_initial);

    let a_exists: i64 = conn2
        .query_row(
            "SELECT count(*) FROM classes WHERE fqn = 'com.test.A'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(a_exists, 1);

    println!(
        "Initial run: {:?}, Re-analysis run: {:?}",
        initial_duration, re_duration
    );
    assert!(
        re_duration < initial_duration,
        "Incremental re-analysis must be faster than initial analysis"
    );

    drop(conn2);
    drop(pool2);
    manager2.delete_workspace(&ws2.id);
    std::fs::remove_dir_all(&test_proj_dir).ok();
}
