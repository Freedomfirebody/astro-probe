use astro_probe_server::kernel::WorkspaceManager;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

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
    let mut local_conn = rusqlite::Connection::open_in_memory().unwrap();
    astro_probe_db::init_db(&local_conn).unwrap();

    let mut global_conn = rusqlite::Connection::open(&env.temp_path).unwrap();

    astro_probe_java::jar::init_global_db(&mut global_conn).unwrap();

    let jar_hash = "mock_jar_hash_123";

    global_conn.execute(
        "INSERT INTO cached_method_summaries (jar_hash, method_fqn, param_index) VALUES (?1, ?2, ?3)",
        [jar_hash, "com.test.Identity.f(java.lang.Object)", "0"],
    ).unwrap();

    astro_probe_java::jar::copy_jar_facts_to_local(&mut local_conn, jar_hash).unwrap();

    let count: i64 = local_conn.query_row(
        "SELECT count(*) FROM method_summaries WHERE method_fqn = 'com.test.Identity.f(java.lang.Object)' AND param_index = 0",
        [],
        |r| r.get(0)
    ).unwrap();
    assert_eq!(count, 1);
}

struct TestSupernodeExtension;
impl astro_probe_core::cg::PointsToSolverExtension for TestSupernodeExtension {
    fn is_supernode(&self, target: &str) -> bool {
        target.contains("java.lang.Object.toString")
            || target.contains("java.lang.StringBuilder.toString")
            || target.contains("java.lang.StringBuffer.toString")
    }
}

#[test]
fn test_supernode_detection_and_bypass() {
    let mut conn = rusqlite::Connection::open_in_memory().unwrap();
    astro_probe_db::init_db(&conn).unwrap();

    conn.execute(
        "INSERT INTO call_sites (call_id, method_fqn, receiver, method_name, lhs, static_callee) \
         VALUES ('call_1', 'com.test.Main.run()', NULL, 'toString', 'com.test.Main.run()#x', 'java.lang.StringBuilder.toString()')",
        []
    ).unwrap();

    let solver = astro_probe_core::cg::PointsToSolver::new();
    let ext = TestSupernodeExtension;
    solver.solve(&mut conn, &[&ext]).unwrap();

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
    let mut conn = rusqlite::Connection::open_in_memory().unwrap();
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
    solver.solve(&mut conn, &[]).unwrap();

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

#[tokio::test]
async fn test_milestone_3_features() {
    let _lock = lock_test_env();
    let _env = EnvGuard::new("m3_features");
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let target_dir = manifest_dir
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("target");

    let test_proj_dir = target_dir.join(format!("test_m3_proj_{}", uuid::Uuid::new_v4()));
    let src_dir = test_proj_dir
        .join("src")
        .join("main")
        .join("java")
        .join("com")
        .join("test");
    std::fs::create_dir_all(&src_dir).unwrap();

    // 1. Create properties files
    let props_content = "spring.profiles.active=dev\nserver.port=9090";
    std::fs::write(test_proj_dir.join("application.properties"), props_content).unwrap();

    let yaml_content = r#"
app:
  name: my-cool-app
  desc: "development mode"
"#;
    std::fs::write(test_proj_dir.join("application-dev.yml"), yaml_content).unwrap();

    // 2. Create MyService.java
    let service_code = r#"
package com.test;
import org.springframework.beans.factory.annotation.Value;
import org.springframework.stereotype.Service;

@Service
public class MyService {
    @Value("${server.port:8080}")
    private String port;

    @Value("${app.name}")
    private String appName;

    @Value("${app.missing:default-val}")
    private String missing;

    @Value("literal-value")
    private String literal;

    public MyService(@Value("${app.desc}") String desc) {
    }
}
"#;
    std::fs::write(src_dir.join("MyService.java"), service_code).unwrap();

    // 3. Create MyController.java
    let controller_code = r#"
package com.test;
import org.springframework.web.bind.annotation.*;

@RestController
@RequestMapping("/api/v1")
public class MyController {
    @GetMapping("/users")
    public String listUsers() {
        return "users";
    }

    @PostMapping(value = { "/users/add", "/users/create" })
    public String createUser() {
        return "created";
    }
}
"#;
    std::fs::write(src_dir.join("MyController.java"), controller_code).unwrap();

    // 4. Create Event files
    let event_code = r#"
package com.test;
public class MyEvent {
    private String data;
    public MyEvent(Object src, String data) {
        this.data = data;
    }
    public String getData() {
        return this.data;
    }
}
"#;
    std::fs::write(src_dir.join("MyEvent.java"), event_code).unwrap();

    let publisher_code = r#"
package com.test;
import org.springframework.context.ApplicationEventPublisher;
import org.springframework.stereotype.Component;

@Component
public class MyPublisher {
    private final ApplicationEventPublisher publisher;
    public MyPublisher(ApplicationEventPublisher publisher) {
        this.publisher = publisher;
    }
    public void publish(String val) {
        MyEvent event = new MyEvent(this, val);
        publisher.publishEvent(event);
    }
}
"#;
    std::fs::write(src_dir.join("MyPublisher.java"), publisher_code).unwrap();

    let listener_code = r#"
package com.test;
import org.springframework.context.event.EventListener;
import org.springframework.stereotype.Component;

@Component
public class MyListener {
    @EventListener
    public void onEvent(MyEvent event) {
        String data = event.getData();
    }
}
"#;
    std::fs::write(src_dir.join("MyListener.java"), listener_code).unwrap();

    // 5. Create Runnable & Thread/Executor caller code
    let thread_code = r#"
package com.test;
import java.util.concurrent.Executor;

public class MyThreadCaller {
    public void startThread(MyRunnable runnable) {
        Thread t = new Thread(runnable);
        t.start();
    }
    public void runExecutor(Executor exec, MyRunnable runnable) {
        exec.execute(runnable);
    }
}
"#;
    std::fs::write(src_dir.join("MyThreadCaller.java"), thread_code).unwrap();

    let runnable_code = r#"
package com.test;
public class MyRunnable implements Runnable {
    @Override
    public void run() {
        String x = "run-method";
    }
}
"#;
    std::fs::write(src_dir.join("MyRunnable.java"), runnable_code).unwrap();

    // 6. Run workspace manager to parse and analyze
    let manager = WorkspaceManager::new();
    let ws = manager
        .create_workspace(
            "m3-test".to_string(),
            test_proj_dir.to_string_lossy().to_string(),
        )
        .expect("Failed to create workspace");

    let pool = manager.get_db_pool_and_touch(&ws.id).unwrap();
    let conn = pool.get().unwrap();

    // -- Assert Properties Resolution --
    let count: i64 = conn
        .query_row("SELECT count(*) FROM resolved_properties", [], |r| r.get(0))
        .unwrap();
    assert!(count >= 3);

    let dev_name: String = conn
        .query_row(
            "SELECT value FROM resolved_properties WHERE key = 'app.name'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(dev_name, "my-cool-app");

    let port_val: String = conn
        .query_row(
            "SELECT value FROM resolved_properties WHERE key = 'server.port'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(port_val, "9090");

    {
        let mut st_h = conn.prepare("SELECT * FROM class_hierarchy").unwrap();
        let rows_h = st_h
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0).unwrap(),
                    r.get::<_, String>(1).unwrap(),
                ))
            })
            .unwrap();
        for r in rows_h.flatten() {
            println!("CLASS HIERARCHY: {:?}", r);
        }

        let mut st = conn.prepare("SELECT * FROM class_annotations").unwrap();
        let rows = st
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0).unwrap(),
                    r.get::<_, String>(1).unwrap(),
                ))
            })
            .unwrap();
        for r in rows.flatten() {
            println!("CLASS ANN: {:?}", r);
        }

        let mut st2 = conn.prepare("SELECT * FROM field_annotations").unwrap();
        let rows2 = st2
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0).unwrap(),
                    r.get::<_, String>(1).unwrap(),
                    r.get::<_, String>(2).unwrap(),
                ))
            })
            .unwrap();
        for r in rows2.flatten() {
            println!("FIELD ANN: {:?}", r);
        }
    }

    let port_alloc: String = conn.query_row(
        "SELECT rhs FROM source_assignments WHERE lhs = 'SpringFieldAlloc:com.test.MyService.port' OR lhs = 'SpringBeanAlloc:com.test.MyService.port'", [], |r| r.get(0)
    ).unwrap();
    assert_eq!(port_alloc, "StringAlloc:9090");

    // appName field resolved to "my-cool-app"
    let app_name_alloc: String = conn.query_row(
        "SELECT rhs FROM source_assignments WHERE lhs = 'SpringFieldAlloc:com.test.MyService.appName' OR lhs = 'SpringBeanAlloc:com.test.MyService.appName'", [], |r| r.get(0)
    ).unwrap();
    assert_eq!(app_name_alloc, "StringAlloc:my-cool-app");

    // missing field resolved to "default-val"
    let missing_alloc: String = conn.query_row(
        "SELECT rhs FROM source_assignments WHERE lhs = 'SpringFieldAlloc:com.test.MyService.missing' OR lhs = 'SpringBeanAlloc:com.test.MyService.missing'", [], |r| r.get(0)
    ).unwrap();
    assert_eq!(missing_alloc, "StringAlloc:default-val");

    // literal field resolved to "literal-value"
    let literal_alloc: String = conn.query_row(
        "SELECT rhs FROM source_assignments WHERE lhs = 'SpringFieldAlloc:com.test.MyService.literal' OR lhs = 'SpringBeanAlloc:com.test.MyService.literal'", [], |r| r.get(0)
    ).unwrap();
    assert_eq!(literal_alloc, "StringAlloc:literal-value");

    // constructor parameter desc resolved to "development mode"
    let desc_param_exists: i64 = conn.query_row(
        "SELECT count(*) FROM source_assignments WHERE lhs = 'com.test.MyService.<init>(java.lang.String)#desc' AND rhs = 'StringAlloc:development mode'",
        [], |r| r.get(0)
    ).unwrap();
    assert_eq!(desc_param_exists, 1);

    // -- Assert Spring MVC Route Mapping --
    let get_path: String = conn
        .query_row(
            "SELECT path FROM web_routes WHERE http_method = 'GET'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(get_path, "/api/v1/users");

    let post_paths_count: i64 = conn
        .query_row(
            "SELECT count(*) FROM web_routes WHERE http_method = 'POST'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(post_paths_count, 2);

    let paths: Vec<String> = {
        let mut stmt = conn
            .prepare("SELECT path FROM web_routes WHERE http_method = 'POST' ORDER BY path")
            .unwrap();
        stmt.query_map([], |r| r.get(0))
            .unwrap()
            .flatten()
            .collect()
    };
    assert_eq!(paths, vec!["/api/v1/users/add", "/api/v1/users/create"]);

    // -- Assert Event Listener Propagation --
    // Assert call edge exists from publish to listener onEvent method
    let event_edge_exists: i64 = conn.query_row(
        "SELECT count(*) FROM call_edges WHERE caller = 'com.test.MyPublisher.publish' AND callee = 'com.test.MyListener.onEvent'",
        [], |r| r.get(0)
    ).unwrap();
    assert_eq!(event_edge_exists, 1);

    // -- Assert Async Execution Tracing --
    // startThread -> MyRunnable.run
    let thread_edge_exists: i64 = conn.query_row(
        "SELECT count(*) FROM call_edges WHERE caller = 'com.test.MyThreadCaller.startThread' AND callee = 'com.test.MyRunnable.run'",
        [], |r| r.get(0)
    ).unwrap();
    assert_eq!(thread_edge_exists, 1);

    // runExecutor -> MyRunnable.run
    let exec_edge_exists: i64 = conn.query_row(
        "SELECT count(*) FROM call_edges WHERE caller = 'com.test.MyThreadCaller.runExecutor' AND callee = 'com.test.MyRunnable.run'",
        [], |r| r.get(0)
    ).unwrap();
    assert_eq!(exec_edge_exists, 1);

    // Cleanup
    drop(conn);
    drop(pool);
    manager.delete_workspace(&ws.id);
    std::fs::remove_dir_all(&test_proj_dir).ok();
}

#[tokio::test]
async fn test_spring_aop_pointcut_resolution() {
    let _lock = lock_test_env();
    let _env = EnvGuard::new("spring_aop");
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let target_dir = manifest_dir
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("target");

    let test_proj_dir = target_dir.join(format!("test_aop_proj_{}", uuid::Uuid::new_v4()));
    let src_dir = test_proj_dir
        .join("src")
        .join("main")
        .join("java")
        .join("com")
        .join("test");
    std::fs::create_dir_all(&src_dir).unwrap();

    // 1. Create MyService.java
    let service_code = r#"
package com.test;
public class MyService {
    public void doSomething() {}
    public void doOtherThing() {}
}
"#;
    std::fs::write(src_dir.join("MyService.java"), service_code).unwrap();

    // 2. Create MyAspect.java
    let aspect_code = r#"
package com.test;
import org.aspectj.lang.annotation.Aspect;
import org.aspectj.lang.annotation.Before;
import org.aspectj.lang.annotation.Pointcut;

@Aspect
public class MyAspect {
    @Pointcut("within(com.test..*)")
    public void myPointcut() {}

    @Before("myPointcut()")
    public void beforeAdvice() {}

    @Before("execution(* com.test.MyService.doOtherThing(..))")
    public void beforeOtherAdvice() {}
}
"#;
    std::fs::write(src_dir.join("MyAspect.java"), aspect_code).unwrap();

    // 3. Create MyController.java
    let controller_code = r#"
package com.test;
public class MyController {
    public void trigger() {
        MyService service = new MyService();
        service.doSomething();
        service.doOtherThing();
    }
}
"#;
    std::fs::write(src_dir.join("MyController.java"), controller_code).unwrap();

    // 4. Run workspace manager to parse and analyze
    let manager = WorkspaceManager::new();
    let ws = manager
        .create_workspace(
            "aop-test".to_string(),
            test_proj_dir.to_string_lossy().to_string(),
        )
        .expect("Failed to create workspace");

    let pool = manager.get_db_pool_and_touch(&ws.id).unwrap();
    let conn = pool.get().unwrap();

    // Print all discovered method annotations for debugging
    {
        let mut stmt = conn
            .prepare("SELECT method_fqn, annotation_name FROM method_annotations")
            .unwrap();
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0).unwrap(),
                    r.get::<_, String>(1).unwrap(),
                ))
            })
            .unwrap();
        for r in rows.flatten() {
            println!("METHOD ANN: {:?}", r);
        }
    }

    // Verify call edges to the advices
    let advice1_exists: i64 = conn.query_row(
        "SELECT count(*) FROM call_edges WHERE caller = 'com.test.MyController.trigger' AND callee = 'com.test.MyAspect.beforeAdvice'",
        [], |r| r.get(0)
    ).unwrap();
    assert_eq!(advice1_exists, 2);

    let advice2_exists: i64 = conn.query_row(
        "SELECT count(*) FROM call_edges WHERE caller = 'com.test.MyController.trigger' AND callee = 'com.test.MyAspect.beforeOtherAdvice'",
        [], |r| r.get(0)
    ).unwrap();
    assert_eq!(advice2_exists, 1);

    // Cleanup
    drop(conn);
    drop(pool);
    manager.delete_workspace(&ws.id);
    std::fs::remove_dir_all(&test_proj_dir).ok();
}

#[tokio::test]
async fn test_maven_dependency_resolution() {
    let _lock = lock_test_env();
    let _env = EnvGuard::new("maven_test");

    let test_dir = std::env::temp_dir().join(format!("maven_test_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&test_dir).unwrap();

    // Setup dummy .m2 repo
    let m2_dir = test_dir
        .join(".m2")
        .join("repository")
        .join("org")
        .join("example")
        .join("dummy")
        .join("1.0");
    std::fs::create_dir_all(&m2_dir).unwrap();
    let dummy_jar_path = m2_dir.join("dummy-1.0.jar");

    // Create valid zip for dummy.jar
    {
        let file = std::fs::File::create(&dummy_jar_path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file("META-INF/MANIFEST.MF", options).unwrap();
        zip.write_all(b"Manifest-Version: 1.0\n").unwrap();
        zip.finish().unwrap();
    }

    // Write pom.xml using property interpolation
    let pom_code = r#"<project>
    <modelVersion>4.0.0</modelVersion>
    <groupId>com.test</groupId>
    <artifactId>test-app</artifactId>
    <version>1.0.0</version>
    <dependencies>
        <dependency>
            <groupId>org.example</groupId>
            <artifactId>dummy</artifactId>
            <version>${dummy.version}</version>
        </dependency>
    </dependencies>
    <properties>
        <dummy.version>1.0</dummy.version>
    </properties>
</project>"#;
    std::fs::write(test_dir.join("pom.xml"), pom_code).unwrap();

    // Set USERPROFILE to redirect maven .m2 resolution
    let original_userprofile = std::env::var("USERPROFILE").ok();
    let original_home = std::env::var("HOME").ok();
    std::env::set_var("USERPROFILE", test_dir.to_str().unwrap());
    std::env::set_var("HOME", test_dir.to_str().unwrap());

    // Run workspace manager
    let manager = WorkspaceManager::new();
    let ws = manager.create_workspace(
        "maven-workspace".to_string(),
        test_dir.to_string_lossy().to_string(),
    );

    // Restore env
    if let Some(ref val) = original_userprofile {
        std::env::set_var("USERPROFILE", val);
    } else {
        std::env::remove_var("USERPROFILE");
    }
    if let Some(ref val) = original_home {
        std::env::set_var("HOME", val);
    } else {
        std::env::remove_var("HOME");
    }

    let ws = ws.expect("Failed to create workspace");
    manager.delete_workspace(&ws.id);
    std::fs::remove_dir_all(&test_dir).ok();
}

#[tokio::test]
async fn test_1cfa_strategy_pattern() {
    let _lock = lock_test_env();
    let _env = EnvGuard::new("strategy_test");

    let test_proj_dir =
        std::env::temp_dir().join(format!("strategy_proj_{}", uuid::Uuid::new_v4()));
    let src_dir = test_proj_dir
        .join("src")
        .join("main")
        .join("java")
        .join("com")
        .join("strategy");
    std::fs::create_dir_all(&src_dir).unwrap();

    let strategy_code = r#"
package com.strategy;
public interface Strategy {
    void execute();
}
"#;
    std::fs::write(src_dir.join("Strategy.java"), strategy_code).unwrap();

    let concrete_a_code = r#"
package com.strategy;
public class ConcreteA implements Strategy {
    public void execute() {}
}
"#;
    std::fs::write(src_dir.join("ConcreteA.java"), concrete_a_code).unwrap();

    let concrete_b_code = r#"
package com.strategy;
public class ConcreteB implements Strategy {
    public void execute() {}
}
"#;
    std::fs::write(src_dir.join("ConcreteB.java"), concrete_b_code).unwrap();

    let context_code = r#"
package com.strategy;
public class Context {
    private Strategy strategy;
    public Context(Strategy s) {
        this.strategy = s;
    }
    public void run() {
        this.strategy.execute();
    }
}
"#;
    std::fs::write(src_dir.join("Context.java"), context_code).unwrap();

    let client_code = r#"
package com.strategy;
public class Client {
    public void main() {
        ConcreteA a = new ConcreteA();
        Context ctx1 = new Context(a);
        ConcreteB b = new ConcreteB();
        Context ctx2 = new Context(b);
        ctx1.run();
        ctx2.run();
    }
}
"#;
    std::fs::write(src_dir.join("Client.java"), client_code).unwrap();

    let manager = WorkspaceManager::new();
    let ws = manager
        .create_workspace(
            "strategy-test".to_string(),
            test_proj_dir.to_string_lossy().to_string(),
        )
        .expect("Failed to create workspace");

    let pool = manager.get_db_pool_and_touch(&ws.id).unwrap();
    let conn = pool.get().unwrap();

    {
        let mut stmt = conn
            .prepare(
                "SELECT caller, callee, caller_context, callee_context, is_virtual FROM call_edges",
            )
            .unwrap();
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0).unwrap(),
                    r.get::<_, String>(1).unwrap(),
                    r.get::<_, String>(2).unwrap(),
                    r.get::<_, String>(3).unwrap(),
                    r.get::<_, i32>(4).unwrap(),
                ))
            })
            .unwrap();
        println!("--- ALL CALL EDGES ---");
        for r in rows.flatten() {
            println!("EDGE: {:?}", r);
        }
    }
    {
        let mut stmt = conn
            .prepare("SELECT variable_fqn, alloc_id, context, alloc_context FROM points_to_sets")
            .unwrap();
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0).unwrap(),
                    r.get::<_, String>(1).unwrap(),
                    r.get::<_, String>(2).unwrap(),
                    r.get::<_, String>(3).unwrap(),
                ))
            })
            .unwrap();
        println!("--- ALL POINTS-TO SETS ---");
        for r in rows.flatten() {
            println!("PTS: {:?}", r);
        }
    }

    let edges = {
        let mut stmt = conn.prepare(
            "SELECT caller_context, caller, callee_context, callee FROM call_edges \
             WHERE caller = 'com.strategy.Context.run' AND callee LIKE 'com.strategy.Concrete%.execute'"
        ).unwrap();
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0).unwrap(),
                    r.get::<_, String>(1).unwrap(),
                    r.get::<_, String>(2).unwrap(),
                    r.get::<_, String>(3).unwrap(),
                ))
            })
            .unwrap();

        let mut edges = Vec::new();
        for r in rows.flatten() {
            edges.push(r);
        }
        edges
    };

    assert_eq!(edges.len(), 2, "Should have exactly 2 edges under 1-CFA");

    // First edge: Context.run -> ConcreteA.execute or ConcreteB.execute
    // Second edge: Context.run -> ConcreteB.execute or ConcreteA.execute
    // But they must have different caller contexts!
    let ctxs: std::collections::HashSet<String> = edges.iter().map(|e| e.0.clone()).collect();
    assert_eq!(
        ctxs.len(),
        2,
        "Should have 2 distinct caller contexts for the calls to Strategy.execute"
    );

    drop(conn);
    drop(pool);
    manager.delete_workspace(&ws.id);
    std::fs::remove_dir_all(&test_proj_dir).ok();
}

#[tokio::test]
async fn test_collection_propagation_list() {
    let _lock = lock_test_env();
    let _env = EnvGuard::new("list_test");

    let test_proj_dir = std::env::temp_dir().join(format!("list_proj_{}", uuid::Uuid::new_v4()));
    let src_dir = test_proj_dir
        .join("src")
        .join("main")
        .join("java")
        .join("com")
        .join("coll");
    std::fs::create_dir_all(&src_dir).unwrap();

    let item_code = r#"
package com.coll;
public class ItemA {}
"#;
    std::fs::write(src_dir.join("ItemA.java"), item_code).unwrap();

    let list_test_code = r#"
package com.coll;
import java.util.ArrayList;
import java.util.List;
public class ListTest {
    public void run() {
        List list = new ArrayList();
        ItemA item = new ItemA();
        list.add(item);
        Object res = list.get(0);
    }
}
"#;
    std::fs::write(src_dir.join("ListTest.java"), list_test_code).unwrap();

    let manager = WorkspaceManager::new();
    let ws = manager
        .create_workspace(
            "list-test".to_string(),
            test_proj_dir.to_string_lossy().to_string(),
        )
        .expect("Failed to create workspace");

    let pool = manager.get_db_pool_and_touch(&ws.id).unwrap();
    let conn = pool.get().unwrap();

    // Verify points-to set of res contains ItemA allocation

    let res_points_to_item_a: i64 = conn
        .query_row(
            "SELECT count(*) FROM points_to_sets p \
         JOIN allocation_sites a ON p.alloc_id = a.alloc_id \
         WHERE p.variable_fqn = 'com.coll.ListTest.run()#res' \
           AND a.class_fqn = 'com.coll.ItemA'",
            [],
            |r| r.get(0),
        )
        .unwrap();

    assert!(
        res_points_to_item_a >= 1,
        "Variable res should point to ItemA allocation via list.[element]"
    );

    drop(conn);
    drop(pool);
    manager.delete_workspace(&ws.id);
    std::fs::remove_dir_all(&test_proj_dir).ok();
}

#[tokio::test]
async fn test_collection_propagation_map() {
    let _lock = lock_test_env();
    let _env = EnvGuard::new("map_test");

    let test_proj_dir = std::env::temp_dir().join(format!("map_proj_{}", uuid::Uuid::new_v4()));
    let src_dir = test_proj_dir
        .join("src")
        .join("main")
        .join("java")
        .join("com")
        .join("coll");
    std::fs::create_dir_all(&src_dir).unwrap();

    let key_code = r#"
package com.coll;
public class Key {}
"#;
    std::fs::write(src_dir.join("Key.java"), key_code).unwrap();

    let value_code = r#"
package com.coll;
public class Value {}
"#;
    std::fs::write(src_dir.join("Value.java"), value_code).unwrap();

    let map_test_code = r#"
package com.coll;
import java.util.HashMap;
import java.util.Map;
public class MapTest {
    public void run() {
        Map map = new HashMap();
        Key k = new Key();
        Value v = new Value();
        map.put(k, v);
        Object res = map.get(k);
    }
}
"#;
    std::fs::write(src_dir.join("MapTest.java"), map_test_code).unwrap();

    let manager = WorkspaceManager::new();
    let ws = manager
        .create_workspace(
            "map-test".to_string(),
            test_proj_dir.to_string_lossy().to_string(),
        )
        .expect("Failed to create workspace");

    let pool = manager.get_db_pool_and_touch(&ws.id).unwrap();
    let conn = pool.get().unwrap();

    // Verify points-to set of res contains Value allocation
    let res_points_to_value: i64 = conn
        .query_row(
            "SELECT count(*) FROM points_to_sets p \
         JOIN allocation_sites a ON p.alloc_id = a.alloc_id \
         WHERE p.variable_fqn = 'com.coll.MapTest.run()#res' \
           AND a.class_fqn = 'com.coll.Value'",
            [],
            |r| r.get(0),
        )
        .unwrap();

    assert!(
        res_points_to_value >= 1,
        "Variable res should point to Value allocation via map.[value]"
    );

    drop(conn);
    drop(pool);
    manager.delete_workspace(&ws.id);
    std::fs::remove_dir_all(&test_proj_dir).ok();
}

#[tokio::test]
async fn test_callback_pattern_flow() {
    let _lock = lock_test_env();
    let _env = EnvGuard::new("callback_test");

    let test_proj_dir =
        std::env::temp_dir().join(format!("callback_proj_{}", uuid::Uuid::new_v4()));
    let src_dir = test_proj_dir
        .join("src")
        .join("main")
        .join("java")
        .join("com")
        .join("callback");
    std::fs::create_dir_all(&src_dir).unwrap();

    let callback_code = r#"
package com.callback;
public interface Callback {
    void call(Object data);
}
"#;
    std::fs::write(src_dir.join("Callback.java"), callback_code).unwrap();

    let my_callback_code = r#"
package com.callback;
public class MyCallback implements Callback {
    public Object received;
    public void call(Object data) {
        this.received = data;
    }
}
"#;
    std::fs::write(src_dir.join("MyCallback.java"), my_callback_code).unwrap();

    let data_code = r#"
package com.callback;
public class Data {}
"#;
    std::fs::write(src_dir.join("Data.java"), data_code).unwrap();

    let caller_code = r#"
package com.callback;
public class Caller {
    public void doWork(Callback cb) {
        Data d = new Data();
        cb.call(d);
    }
}
"#;
    std::fs::write(src_dir.join("Caller.java"), caller_code).unwrap();

    let client_code = r#"
package com.callback;
public class Client {
    public void main() {
        MyCallback cb = new MyCallback();
        Caller caller = new Caller();
        caller.doWork(cb);
    }
}
"#;
    std::fs::write(src_dir.join("Client.java"), client_code).unwrap();

    let manager = WorkspaceManager::new();
    let ws = manager
        .create_workspace(
            "callback-test".to_string(),
            test_proj_dir.to_string_lossy().to_string(),
        )
        .expect("Failed to create workspace");

    let pool = manager.get_db_pool_and_touch(&ws.id).unwrap();
    let conn = pool.get().unwrap();

    // Verify points-to propagation in MyCallback.call
    // MyCallback.received field should point to Data allocation

    let field_points_to_data: i64 = conn
        .query_row(
            "SELECT count(*) FROM points_to_sets p \
         JOIN allocation_sites a ON p.alloc_id = a.alloc_id \
         WHERE p.variable_fqn LIKE '%MyCallback%#this.received' \
           AND a.class_fqn = 'com.callback.Data'",
            [],
            |r| r.get(0),
        )
        .unwrap();

    assert!(
        field_points_to_data >= 1,
        "MyCallback.received should point to Data allocation via callback flow"
    );

    drop(conn);
    drop(pool);
    manager.delete_workspace(&ws.id);
    std::fs::remove_dir_all(&test_proj_dir).ok();
}

#[tokio::test]
async fn test_headless_auto_resolution_and_persistence() {
    let _lock = lock_test_env();
    let _env = EnvGuard::new("headless_test");

    let test_proj_dir =
        std::env::temp_dir().join(format!("headless_proj_{}", uuid::Uuid::new_v4()));
    let src_dir = test_proj_dir
        .join("src")
        .join("main")
        .join("java")
        .join("com")
        .join("headless");
    std::fs::create_dir_all(&src_dir).unwrap();

    let code = r#"
package com.headless;
public class App {
    public void run() {}
}
"#;
    std::fs::write(src_dir.join("App.java"), code).unwrap();

    let manager = WorkspaceManager::new();
    let proj_path = test_proj_dir.to_string_lossy().to_string();

    // 1. Verify get_or_create_workspace_id auto-creates workspace if it doesn't exist
    let ws_id = manager.get_or_create_workspace_id(None, Some(&proj_path)).unwrap();
    assert!(!ws_id.is_empty());

    // Verify it is registered and loaded
    let pool = manager.get_db_pool_and_touch(&ws_id);
    assert!(pool.is_some());

    // 2. Stop/Unload workspace (so status becomes Unloaded)
    let ws = manager.stop_workspace(&ws_id).expect("Failed to stop workspace");
    assert_eq!(ws.status, astro_probe_server::kernel::workspace::WorkspaceStatus::Unloaded);

    // 3. Verify get_db_pool_and_touch auto-wakes/loads an Unloaded workspace
    let pool2 = manager.get_db_pool_and_touch(&ws_id);
    assert!(pool2.is_some(), "Should automatically load pool from Unloaded state");

    // 4. Verify get_or_create_workspace_id resolves existing workspace by ID or by path
    let resolved_by_id = manager.get_or_create_workspace_id(Some(&ws_id), None).unwrap();
    assert_eq!(resolved_by_id, ws_id);

    let resolved_by_path = manager.get_or_create_workspace_id(None, Some(&proj_path)).unwrap();
    assert_eq!(resolved_by_path, ws_id);

    // Verify it resolves even when passed to workspace_id parameter directly (e.g. if agent passes path to workspace_id)
    let resolved_path_via_id_param = manager.get_or_create_workspace_id(Some(&proj_path), None).unwrap();
    assert_eq!(resolved_path_via_id_param, ws_id);

    // Cleanup
    manager.delete_workspace(&ws_id);
    std::fs::remove_dir_all(&test_proj_dir).ok();
}
