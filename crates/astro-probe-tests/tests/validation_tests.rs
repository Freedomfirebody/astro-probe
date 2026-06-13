use rusqlite::Connection;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use tempfile::NamedTempFile;
use zip::write::SimpleFileOptions;

use astro_probe_core::cg::PointsToSolver;
use astro_probe_core::dfg::DfgAnalyzer;
use astro_probe_java::jar::{copy_jar_facts_to_local, init_global_db, parse_jar_file};
use astro_probe_server::kernel::WorkspaceManager;

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

struct TestSupernodeExtension;
impl astro_probe_core::cg::PointsToSolverExtension for TestSupernodeExtension {
    fn is_supernode(&self, target: &str) -> bool {
        target.contains("java.lang.Object.toString")
            || target.contains("java.lang.StringBuilder.toString")
            || target.contains("java.lang.StringBuffer.toString")
    }
}

#[tokio::test]
async fn test_validation_to_string_call_chains_bounded() {
    // 1. Verify that querying call chains through java.lang.Object.toString() returns bounded results.
    let mut conn = Connection::open_in_memory().unwrap();
    astro_probe_db::init_db(&conn).unwrap();

    // Insert method declaration for Object.toString()
    conn.execute(
        "INSERT INTO method_declarations (method_fqn, class_fqn, method_name, params) \
         VALUES ('java.lang.Object.toString()', 'java.lang.Object', 'toString', '')",
        [],
    )
    .unwrap();

    // Call site from user code calling Object.toString()
    conn.execute(
        "INSERT INTO call_sites (call_id, method_fqn, receiver, method_name, lhs, static_callee) \
         VALUES ('call_1', 'com.test.Main.run()', NULL, 'toString', 'com.test.Main.run()#x', 'java.lang.Object.toString()')",
        []
    ).unwrap();

    // Run solver
    let solver = PointsToSolver::new();
    let ext = TestSupernodeExtension;
    solver.solve(&mut conn, &[&ext]).unwrap();

    // Verify Object.toString() gets bypassed as a supernode and assigns a dummy allocation
    let count: i64 = conn
        .query_row(
            "SELECT count(*) FROM points_to_sets \
         WHERE variable_fqn = 'com.test.Main.run()#x' \
         AND alloc_id = 'SupernodeReturn:java.lang.Object.toString()'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        count, 1,
        "Object.toString() should be bypassed and return a supernode allocation"
    );

    // Verify there are no call edges starting from java.lang.Object.toString()
    let edges_count: i64 = conn
        .query_row(
            "SELECT count(*) FROM call_edges WHERE caller LIKE 'java.lang.Object.toString%'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        edges_count, 0,
        "No call edges should originate from Object.toString() because its body is bypassed"
    );
}

#[test]
fn test_validation_method_summaries_from_bytecode() {
    let _lock = lock_test_env();
    // 2. Verify that method summaries are generated for library JAR methods and correctly used in points-to propagation.
    let class_bytes = [
        0xca, 0xfe, 0xba, 0xbe, // magic
        0x00, 0x00, // minor
        0x00, 0x34, // major (52)
        0x00, 0x08, // constant pool count (8 entries: 1 to 7)
        // 1: Class (Identity)
        0x07, 0x00, 0x03, // 2: Class (java/lang/Object)
        0x07, 0x00, 0x04, // 3: Utf8 "Identity"
        0x01, 0x00, 0x08, b'I', b'd', b'e', b'n', b't', b'i', b't', b'y',
        // 4: Utf8 "java/lang/Object"
        0x01, 0x00, 0x10, b'j', b'a', b'v', b'a', b'/', b'l', b'a', b'n', b'g', b'/', b'O', b'b',
        b'j', b'e', b'c', b't', // 5: Utf8 "f"
        0x01, 0x00, 0x01, b'f',
        // 6: Utf8 descriptor: (Ljava/lang/Object;)Ljava/lang/Object;
        0x01, 0x00, 0x26, b'(', b'L', b'j', b'a', b'v', b'a', b'/', b'l', b'a', b'n', b'g', b'/',
        b'O', b'b', b'j', b'e', b'c', b't', b';', b')', b'L', b'j', b'a', b'v', b'a', b'/', b'l',
        b'a', b'n', b'g', b'/', b'O', b'b', b'j', b'e', b'c', b't', b';',
        // 7: Utf8 "Code"
        0x01, 0x00, 0x04, b'C', b'o', b'd', b'e', 0x00, 0x21, // access flags: public super
        0x00, 0x01, // this class
        0x00, 0x02, // super class
        0x00, 0x00, // interfaces count
        0x00, 0x00, // fields count
        0x00, 0x01, // methods count
        // Method 1
        0x00, 0x09, // access flags: public static
        0x00, 0x05, // name index ("f")
        0x00, 0x06, // descriptor index
        0x00, 0x01, // attributes count
        // Code attribute
        0x00, 0x07, // attribute name ("Code")
        0x00, 0x00, 0x00, 0x0e, // attribute length (14)
        0x00, 0x01, // max stack (1)
        0x00, 0x01, // max locals (1)
        0x00, 0x00, 0x00, 0x02, // code length (2)
        0x2a, 0xb0, // code: aload_0 (opcode 42), areturn (opcode 176)
        0x00, 0x00, // exception table length
        0x00, 0x00, // attributes count
        0x00, 0x00, // class attributes count
    ];

    // Create a temporary zip (JAR) file
    let jar_file = NamedTempFile::new().unwrap();
    let mut zip = zip::ZipWriter::new(jar_file.as_file());
    zip.start_file("Identity.class", SimpleFileOptions::default())
        .unwrap();

    zip.write_all(&class_bytes).unwrap();
    zip.finish().unwrap();

    // Initialize global cache DB
    let env = EnvGuard::new("val_bytecode");
    let mut global_conn = rusqlite::Connection::open(&env.temp_path).unwrap();
    init_global_db(&mut global_conn).unwrap();

    // Parse the JAR
    let jar_hash = "identity_jar_hash_999";
    parse_jar_file(jar_file.path(), jar_hash, &mut global_conn).unwrap();

    // Verify that the method summary is generated in the global cache
    let cache_summary_count: i64 = global_conn
        .query_row(
            "SELECT count(*) FROM cached_method_summaries \
         WHERE method_fqn = 'Identity.f(java.lang.Object)' AND param_index = 0",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        cache_summary_count, 1,
        "Method summary should be generated from class bytecode"
    );

    // Copy to a local database
    let mut local_conn = rusqlite::Connection::open_in_memory().unwrap();
    astro_probe_db::init_db(&local_conn).unwrap();
    copy_jar_facts_to_local(&mut local_conn, jar_hash).unwrap();

    // Verify copy
    let local_summary_count: i64 = local_conn
        .query_row(
            "SELECT count(*) FROM method_summaries \
         WHERE method_fqn = 'Identity.f(java.lang.Object)' AND param_index = 0",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        local_summary_count, 1,
        "Method summary should be copied to the local DB"
    );

    // Perform points-to propagation using the summary
    local_conn
        .execute(
            "INSERT INTO source_assignments (lhs, rhs, assignment_type, method_fqn) \
         VALUES ('com.test.Main.run()#arg', 'AllocA', 'ALLOC', 'com.test.Main.run()')",
            [],
        )
        .unwrap();

    local_conn.execute(
        "INSERT INTO call_sites (call_id, method_fqn, receiver, method_name, lhs, static_callee) \
         VALUES ('call_1', 'com.test.Main.run()', NULL, 'f', 'com.test.Main.run()#ret', 'Identity.f(java.lang.Object)')",
        []
    ).unwrap();

    local_conn
        .execute(
            "INSERT INTO call_arguments (call_id, arg_index, arg_var, arg_type) \
         VALUES ('call_1', 0, 'com.test.Main.run()#arg', 'java.lang.Object')",
            [],
        )
        .unwrap();

    let solver = PointsToSolver::new();
    solver.solve(&mut local_conn, &[]).unwrap();

    // Verify AllocA propagated to the return value of the call
    let pts_count: i64 = local_conn
        .query_row(
            "SELECT count(*) FROM points_to_sets \
         WHERE variable_fqn = 'com.test.Main.run()#ret' \
         AND alloc_id = 'AllocA'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        pts_count, 1,
        "Points-to set should propagate AllocA across summary bypass"
    );
}

#[test]
fn test_validation_dfg_transitive_reduction() {
    // 3. Verify that the transitive reduction of DFG chains is correct and collapses intermediate variables.
    let conn = Connection::open_in_memory().unwrap();
    astro_probe_db::init_db(&conn).unwrap();

    // Setup mock declarations
    conn.execute(
        "INSERT INTO method_declarations (method_fqn, class_fqn, method_name, params) \
         VALUES ('com.test.Main.run()', 'com.test.Main', 'run', '')",
        [],
    )
    .unwrap();

    // 1. Collapsible chain 1: temp_void_call_
    conn.execute(
        "INSERT INTO source_assignments (lhs, rhs, assignment_type, method_fqn) \
         VALUES ('com.test.Main.run()#temp_void_call_1', 'com.test.Main.run()#A', 'COPY', 'com.test.Main.run()')",
        []
    ).unwrap();
    conn.execute(
        "INSERT INTO source_assignments (lhs, rhs, assignment_type, method_fqn) \
         VALUES ('com.test.Main.run()#B', 'com.test.Main.run()#temp_void_call_1', 'COPY', 'com.test.Main.run()')",
        []
    ).unwrap();

    // 2. Collapsible chain 2: temp_alloc_
    conn.execute(
        "INSERT INTO source_assignments (lhs, rhs, assignment_type, method_fqn) \
         VALUES ('com.test.Main.run()#temp_alloc_2', 'com.test.Main.run()#C', 'COPY', 'com.test.Main.run()')",
        []
    ).unwrap();
    conn.execute(
        "INSERT INTO source_assignments (lhs, rhs, assignment_type, method_fqn) \
         VALUES ('com.test.Main.run()#D', 'com.test.Main.run()#temp_alloc_2', 'COPY', 'com.test.Main.run()')",
        []
    ).unwrap();

    // 3. Non-collapsible chain: normal variable
    conn.execute(
        "INSERT INTO source_assignments (lhs, rhs, assignment_type, method_fqn) \
         VALUES ('com.test.Main.run()#normal_var', 'com.test.Main.run()#E', 'COPY', 'com.test.Main.run()')",
        []
    ).unwrap();
    conn.execute(
        "INSERT INTO source_assignments (lhs, rhs, assignment_type, method_fqn) \
         VALUES ('com.test.Main.run()#F', 'com.test.Main.run()#normal_var', 'COPY', 'com.test.Main.run()')",
        []
    ).unwrap();

    // Run analyzer
    let dfg = DfgAnalyzer::new();
    dfg.analyze(&conn).unwrap();

    // Check that A -> B exists (collapsed temp_void_call_1)
    let count_ab: i64 = conn.query_row(
        "SELECT count(*) FROM lineage_edges WHERE from_node = 'com.test.Main.run()#A' AND to_node = 'com.test.Main.run()#B'",
        [],
        |r| r.get(0)
    ).unwrap();
    assert_eq!(
        count_ab, 1,
        "A -> B should exist directly due to transitive reduction collapsing temp_void_call_1"
    );

    // Check that C -> D exists (collapsed temp_alloc_2)
    let count_cd: i64 = conn.query_row(
        "SELECT count(*) FROM lineage_edges WHERE from_node = 'com.test.Main.run()#C' AND to_node = 'com.test.Main.run()#D'",
        [],
        |r| r.get(0)
    ).unwrap();
    assert_eq!(
        count_cd, 1,
        "C -> D should exist directly due to transitive reduction collapsing temp_alloc_2"
    );

    // Check that E -> F does NOT exist directly, because normal_var is not collapsible
    let count_ef: i64 = conn.query_row(
        "SELECT count(*) FROM lineage_edges WHERE from_node = 'com.test.Main.run()#E' AND to_node = 'com.test.Main.run()#F'",
        [],
        |r| r.get(0)
    ).unwrap();
    assert_eq!(
        count_ef, 0,
        "E -> F should not exist directly because normal_var is not collapsible"
    );

    // Check that E -> normal_var and normal_var -> F exist
    let count_e_norm: i64 = conn.query_row(
        "SELECT count(*) FROM lineage_edges WHERE from_node = 'com.test.Main.run()#E' AND to_node = 'com.test.Main.run()#normal_var'",
        [],
        |r| r.get(0)
    ).unwrap();
    let count_norm_f: i64 = conn.query_row(
        "SELECT count(*) FROM lineage_edges WHERE from_node = 'com.test.Main.run()#normal_var' AND to_node = 'com.test.Main.run()#F'",
        [],
        |r| r.get(0)
    ).unwrap();
    assert_eq!(count_e_norm, 1);
    assert_eq!(count_norm_f, 1);
}

#[tokio::test]
async fn test_validation_medium_spring_call_chains() {
    let _lock = lock_test_env();
    let _env = EnvGuard::new("med_spring_val");
    // 4. Verify the integration call chain in medium-spring: OrderService -> UserService -> ProductService.
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

    let guard = TempProjectGuard::new(&project_path, "medium_spring_validation");

    // We make sure the previous db is deleted to run a clean analysis
    let db_path = guard.temp_dir.join(".astro-probe.db");
    if db_path.exists() {
        std::fs::remove_file(&db_path).ok();
    }

    let manager = WorkspaceManager::new();
    let ws = manager
        .create_workspace(
            "medium-spring-validation".to_string(),
            guard.temp_dir.to_string_lossy().to_string(),
        )
        .expect("Failed to create workspace");

    let pool = manager
        .get_db_pool_and_touch(&ws.id)
        .expect("Failed to get DB pool");
    let conn = pool.get().expect("Failed to get connection");

    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let debug_path = std::path::Path::new(&manifest_dir)
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("debug_info.txt");
    let mut debug_out = std::fs::File::create(debug_path).unwrap();
    use std::io::Write as _;

    writeln!(debug_out, "=== FIELD ASSIGNMENTS ===").unwrap();
    let mut stmt = conn.prepare("SELECT lhs, rhs, assignment_type FROM source_assignments WHERE assignment_type IN ('FIELD_WRITE', 'FIELD_READ')").unwrap();
    let rows = stmt
        .query_map([], |row| {
            let lhs: String = row.get(0)?;
            let rhs: String = row.get(1)?;
            let assignment_type: String = row.get(2)?;
            Ok((lhs, rhs, assignment_type))
        })
        .unwrap();
    for r in rows.flatten() {
        writeln!(debug_out, "FIELD: {} = {:?}({})", r.0, r.1, r.2).unwrap();
    }

    writeln!(debug_out, "=== ALLOCATION SITES ===").unwrap();
    let mut stmt = conn
        .prepare("SELECT alloc_id, class_fqn, method_fqn FROM allocation_sites")
        .unwrap();
    let rows = stmt
        .query_map([], |row| {
            let alloc_id: String = row.get(0)?;
            let class_fqn: String = row.get(1)?;
            let method_fqn: String = row.get(2)?;
            Ok((alloc_id, class_fqn, method_fqn))
        })
        .unwrap();
    for r in rows.flatten() {
        writeln!(debug_out, "ALLOC: {} | {} | {}", r.0, r.1, r.2).unwrap();
    }

    writeln!(debug_out, "=== SPRING POINTS TO SETS ===").unwrap();
    let mut stmt = conn.prepare("SELECT variable_fqn, alloc_id FROM points_to_sets WHERE variable_fqn LIKE '%Spring%' OR alloc_id LIKE '%Spring%'").unwrap();
    let rows = stmt
        .query_map([], |row| {
            let var: String = row.get(0)?;
            let alloc: String = row.get(1)?;
            Ok((var, alloc))
        })
        .unwrap();
    for r in rows.flatten() {
        writeln!(debug_out, "PTS: {} -> {}", r.0, r.1).unwrap();
    }

    writeln!(debug_out, "=== METHOD DECLARATIONS ===").unwrap();
    let mut stmt = conn
        .prepare("SELECT method_fqn, params FROM method_declarations")
        .unwrap();
    let rows = stmt
        .query_map([], |row| {
            let fqn: String = row.get(0)?;
            let params: String = row.get(1)?;
            Ok((fqn, params))
        })
        .unwrap();
    for r in rows.flatten() {
        writeln!(debug_out, "DECL: {} | params={}", r.0, r.1).unwrap();
    }

    let sa_count: i64 = conn
        .query_row("SELECT count(*) FROM source_assignments", [], |r| r.get(0))
        .unwrap();
    let alloc_count: i64 = conn
        .query_row("SELECT count(*) FROM allocation_sites", [], |r| r.get(0))
        .unwrap();
    let class_ann_count: i64 = conn
        .query_row("SELECT count(*) FROM class_annotations", [], |r| r.get(0))
        .unwrap();
    let field_ann_count: i64 = conn
        .query_row("SELECT count(*) FROM field_annotations", [], |r| r.get(0))
        .unwrap();
    let call_site_count: i64 = conn
        .query_row("SELECT count(*) FROM call_sites", [], |r| r.get(0))
        .unwrap();
    println!("DEBUG DB counts: source_assignments={}, allocation_sites={}, class_annotations={}, field_annotations={}, call_sites={}", sa_count, alloc_count, class_ann_count, field_ann_count, call_site_count);

    writeln!(debug_out, "=== CALL SITES ===").unwrap();
    let mut stmt = conn
        .prepare(
            "SELECT call_id, method_fqn, receiver, method_name, lhs, static_callee FROM call_sites",
        )
        .unwrap();
    let rows = stmt
        .query_map([], |row| {
            let call_id: String = row.get(0)?;
            let method_fqn: String = row.get(1)?;
            let receiver: Option<String> = row.get(2)?;
            let method_name: String = row.get(3)?;
            let lhs: Option<String> = row.get(4)?;
            let static_callee: Option<String> = row.get(5)?;
            Ok((
                call_id,
                method_fqn,
                receiver,
                method_name,
                lhs,
                static_callee,
            ))
        })
        .unwrap();
    for r in rows.flatten() {
        writeln!(
            debug_out,
            "CALL SITE: id={} | method={} | rec={:?} | name={} | lhs={:?} | static={:?}",
            r.0, r.1, r.2, r.3, r.4, r.5
        )
        .unwrap();
    }

    writeln!(debug_out, "=== ALL CALL EDGES ===").unwrap();
    let mut stmt = conn
        .prepare("SELECT caller, callee, is_virtual FROM call_edges")
        .unwrap();
    let rows = stmt
        .query_map([], |row| {
            let caller: String = row.get(0)?;
            let callee: String = row.get(1)?;
            let is_virtual: i32 = row.get(2)?;
            Ok((caller, callee, is_virtual))
        })
        .unwrap();
    for r in rows.flatten() {
        writeln!(debug_out, "EDGE: {} -> {} (virt={})", r.0, r.1, r.2).unwrap();
    }

    let mut stmt = conn.prepare("SELECT lhs, rhs, assignment_type, method_fqn FROM source_assignments WHERE lhs LIKE '%Spring%' OR rhs LIKE '%Spring%'").unwrap();
    let rows = stmt
        .query_map([], |row| {
            let lhs: String = row.get(0)?;
            let rhs: String = row.get(1)?;
            let assignment_type: String = row.get(2)?;
            let method_fqn: String = row.get(3)?;
            Ok((lhs, rhs, assignment_type, method_fqn))
        })
        .unwrap();
    for r in rows.flatten() {
        println!(
            "DEBUG Spring assignment: {} = {}({}) in method {}",
            r.0, r.1, r.2, r.3
        );
    }

    let mut stmt = conn
        .prepare(
            "SELECT callee FROM call_edges \
         WHERE caller = 'com.example.medium.service.impl.OrderServiceImpl.createOrder'",
        )
        .unwrap();

    let callee_rows = stmt
        .query_map([], |row| {
            let callee: String = row.get(0)?;
            Ok(callee)
        })
        .unwrap();

    let mut callees = std::collections::HashSet::new();
    for c in callee_rows.flatten() {
        callees.insert(c);
    }

    println!("OrderServiceImpl.createOrder callees: {:?}", callees);

    assert!(
        callees.contains("com.example.medium.service.impl.UserServiceImpl.findById"),
        "OrderServiceImpl.createOrder should call UserServiceImpl.findById (concrete edge)"
    );
    assert!(
        callees.contains("com.example.medium.service.impl.ProductServiceImpl.findById"),
        "OrderServiceImpl.createOrder should call ProductServiceImpl.findById (concrete edge)"
    );
    assert!(
        callees.contains("com.example.medium.service.impl.ProductServiceImpl.updateStock"),
        "OrderServiceImpl.createOrder should call ProductServiceImpl.updateStock (concrete edge)"
    );

    // Clean up
    manager.delete_workspace(&ws.id);
    if db_path.exists() {
        std::fs::remove_file(&db_path).ok();
    }
}
