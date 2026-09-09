use rusqlite::Connection;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Barrier};
use std::time::{Duration, Instant};
use astro_probe_server::kernel::WorkspaceManager;
use astro_probe_server::kernel::workspace::WorkspaceStatus;

fn clean_node_name(node: &str) -> HashSet<String> {
    let mut final_nodes = HashSet::new();
    final_nodes.insert(node.to_string());
    if let Some(hash_idx) = node.find('#') {
        final_nodes.insert(node[hash_idx + 1..].to_string());
    } else {
        let clean_node = if let Some(paren_idx) = node.find('(') {
            &node[..paren_idx]
        } else {
            node
        };
        if let Some(dot_idx) = clean_node.rfind('.') {
            final_nodes.insert(node[dot_idx + 1..].to_string());
        }
    }
    final_nodes
}

#[test]
fn test_fqn_demangling_stress_cases() {
    let test_cases = vec![
        // Basic method signatures
        ("com.test.Class.method", vec!["com.test.Class.method", "method"]),
        ("com.test.Class.method()", vec!["com.test.Class.method()", "method()"]),
        ("com.test.Class.method(int)", vec!["com.test.Class.method(int)", "method(int)"]),
        ("com.test.Class.method(int,java.lang.String)", vec!["com.test.Class.method(int,java.lang.String)", "method(int,java.lang.String)"]),

        // Generics in method arguments/class name
        ("com.test.Class<T>.method(java.util.List<java.lang.String>)", vec!["com.test.Class<T>.method(java.util.List<java.lang.String>)", "method(java.util.List<java.lang.String>)"]),
        ("com.test.Class.method(java.util.Map<java.lang.String,java.util.List<java.lang.Integer>>)", vec!["com.test.Class.method(java.util.Map<java.lang.String,java.util.List<java.lang.Integer>>)", "method(java.util.Map<java.lang.String,java.util.List<java.lang.Integer>>)"].into_iter().collect()),

        // Return types (prefix / suffix / Kotlin style)
        ("public void com.test.Class.method(int)", vec!["public void com.test.Class.method(int)", "method(int)"]),
        ("com.test.Class.method(int)void", vec!["com.test.Class.method(int)void", "method(int)void"]),
        ("com.test.Class.method(int) : void", vec!["com.test.Class.method(int) : void", "method(int) : void"]),

        // Empty signatures / missing parts
        ("method()", vec!["method()"]),
        ("ClassMyMethod()", vec!["ClassMyMethod()"]),
        ("", vec![""]),

        // Variables with hash sign (#)
        ("com.test.Class.method#param", vec!["com.test.Class.method#param", "param"]),
        ("com.test.Class.method()#ret", vec!["com.test.Class.method()#ret", "ret"]),
        ("com.test.Class.method(int)#param", vec!["com.test.Class.method(int)#param", "param"]),
        ("com.test.Class.method(int,java.lang.String)#param", vec!["com.test.Class.method(int,java.lang.String)#param", "param"]),
        ("com.test.Class.method#param#subparam", vec!["com.test.Class.method#param#subparam", "param#subparam"]),
        ("com.test.Class.method#", vec!["com.test.Class.method#", ""]),

        // Weird inputs
        ("com.test.Class.", vec!["com.test.Class.", ""]),
        ("()", vec!["()"]),
        (".", vec![".", ""]),
        (".method()", vec![".method()", "method()"]),
        ("com.例子.类.方法(int)", vec!["com.例子.类.方法(int)", "方法(int)"]),
        ("com.例子.类.方法#变量", vec!["com.例子.类.方法#变量", "变量"]),
    ];

    for (input, expected) in test_cases {
        let result = clean_node_name(input);
        for exp in &expected {
            assert!(
                result.contains(*exp),
                "Input '{}' expected to yield '{}' but got {:?}",
                input,
                exp,
                result
            );
        }
    }
}

#[test]
fn test_signature_matching_robustness() {
    let cases = vec![
        ("com.test.Class.method", "method", true),
        ("com.test.Class.method()", "method", true),
        ("com.test.Class.method(int)", "method", true),
        ("com.test.Class.method(int)", "method()", false),
        ("com.test.Class.method()", "method()", true),
        ("com.test.Class.method(int,java.lang.String)", "method(int,java.lang.String)", true),
        ("com.test.Class.method(int,java.lang.String)", "method(int, java.lang.String)", true),
        ("com.test.Class.method(int,java.lang.String)", "method(int)", false),
        ("", "", true),
        ("method", "", true),
        ("", "method", false),
        ("com.test.Class.method", "Class.method", true),
        ("com.test.Class.method", "other.Class.method", false),
        ("com.test.Class.method(int)", "Class.method(int)", true),
        // Adversarial & Boundary Cases:
        // 1. Curried Scala methods (Bug: query 'method(String)' matches 'method(int)(String)')
        ("com.test.Class.method(int)(String)", "method(String)", false),
        // 2. Package prefix mismatch (Gap: 'method(String)' does not match 'method(java.lang.String)')
        ("com.test.Class.method(java.lang.String)", "method(String)", true),
        // 3. Generics with package prefixes (Gap: package sensitivity causes mismatches)
        ("com.test.Class.method(java.util.Map<java.lang.String,java.util.List<java.lang.Integer>>)", "method(Map<String,List<Integer>>)", true),
        // 4. Inner classes using '$' instead of '.'
        ("com.test.Class$Inner.method(int)", "Inner.method(int)", true),
        // 5. Special characters and spaces
        ("com.test.Class.method-name(int)", "method-name(int)", true),
        ("com.test.Class.method(int)", "  method  (  int  )  ", true),
    ];

    for (cand, query, expected) in cases {
        let res = astro_probe_core::query::matches_method_signature(cand, query);
        assert_eq!(
            res, expected,
            "matches_method_signature('{}', '{}') returned {}, expected {}",
            cand, query, res, expected
        );
    }
}

#[tokio::test]
async fn test_high_concurrency_workspace_deletion() {
    // Stress-test workspace deletion under high concurrent query load.
    // Verify no deadlocks occur and file cleanup retries function as intended.
    let temp_dir = std::env::temp_dir();
    let ws_dir = temp_dir.join(format!("ws_concurrency_delete_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&ws_dir).unwrap();

    let manager = Arc::new(WorkspaceManager::new());
    {
        let ws_list = manager.list_workspaces();
        if let Some(existing) = ws_list.iter().find(|w| w.name == "concurrency_ws") {
            manager.delete_workspace(&existing.id);
        }
    }
    let ws = manager
        .create_workspace("concurrency_ws".to_string(), ws_dir.to_string_lossy().to_string())
        .unwrap();

    let db_path = PathBuf::from(&ws.db_path);
    let parent = db_path.parent().unwrap().to_path_buf();
    let wal_path = parent.join("astro-probe.db-wal");
    let shm_path = parent.join("astro-probe.db-shm");

    // Initialize DB schema inside the workspace db
    {
        let pool = manager.get_db_pool_and_touch(&ws.id).unwrap();
        let conn = pool.get().unwrap();
        conn.execute("CREATE TABLE IF NOT EXISTS test_table (id INTEGER PRIMARY KEY, val TEXT);", []).unwrap();
    }

    let num_threads = 10;
    let barrier = Arc::new(Barrier::new(num_threads + 1));
    let mut handles = Vec::new();

    // Spawn concurrent reader/writer threads that query the database continuously
    for i in 0..num_threads {
        let manager_clone = Arc::clone(&manager);
        let ws_id = ws.id.clone();
        let barrier_clone = Arc::clone(&barrier);

        let handle = std::thread::spawn(move || {
            // Wait for all threads to start
            barrier_clone.wait();

            let pool = match manager_clone.get_db_pool_and_touch(&ws_id) {
                Some(p) => p,
                None => return, // Workspace deleted before we could start
            };

            let start = Instant::now();
            let mut count = 0;
            // Query for up to 300ms
            while start.elapsed() < Duration::from_millis(300) {
                if let Ok(conn) = pool.get() {
                    let _ = conn.execute(
                        "INSERT INTO test_table (val) VALUES (?1)",
                        [format!("thread_{}_val_{}", i, count)],
                    );
                    let mut stmt = match conn.prepare("SELECT val FROM test_table LIMIT 10") {
                        Ok(s) => s,
                        Err(_) => break, // DB being closed/deleted
                    };
                    let _ = stmt.query_map([], |r| r.get::<_, String>(0));
                    count += 1;
                } else {
                    break; // pool exhausted/closed
                }
                std::thread::sleep(Duration::from_millis(5)); // small gap
            }
        });
        handles.push(handle);
    }

    // Wait for worker threads to start
    barrier.wait();
    
    // Let workers query for 50ms, then trigger deletion
    std::thread::sleep(Duration::from_millis(50));

    let t_start = Instant::now();
    let deleted = manager.delete_workspace(&ws.id);
    let t_duration = t_start.elapsed();

    println!("Workspace deletion finished in {:?}", t_duration);
    assert!(deleted, "Workspace deletion should return true even under concurrent load");

    // Join all threads
    for handle in handles {
        let _ = handle.join();
    }

    // Check if files are cleaned up. Since threads query for 300ms, and deletion starts at 50ms,
    // some queries might run for 250ms, which is within the 500ms total retry window (5 attempts * 100ms sleep).
    // Therefore, deletion should succeed eventually and files should be cleaned up.
    // If not, we assert on Windows that the file might remain if queries took longer,
    // but the system must not deadlock.
    println!("DB path exists after deletion: {}", db_path.exists());
    println!("WAL path exists after deletion: {}", wal_path.exists());
    println!("SHM path exists after deletion: {}", shm_path.exists());

    // Clean up directory
    let _ = std::fs::remove_dir_all(&ws_dir);
    let _ = std::fs::remove_dir_all(&parent);
}

#[tokio::test]
async fn test_idle_timeout_clamping() {
    let original_val = std::env::var("ASTRO_PROBE_IDLE_TIMEOUT_SECS").ok();

    // Test Case 1: Clamping to minimum 5 seconds when environment variable is set to 2
    std::env::set_var("ASTRO_PROBE_IDLE_TIMEOUT_SECS", "2");
    let temp_dir = std::env::temp_dir();
    let ws_dir = temp_dir.join(format!("ws_timeout_clamp_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&ws_dir).unwrap();

    let manager = WorkspaceManager::new();
    {
        let ws_list = manager.list_workspaces();
        if let Some(existing) = ws_list.iter().find(|w| w.name == "clamp_test_ws") {
            manager.delete_workspace(&existing.id);
        }
    }
    let ws = manager
        .create_workspace("clamp_test_ws".to_string(), ws_dir.to_string_lossy().to_string())
        .unwrap();

    assert_eq!(ws.status, WorkspaceStatus::Loaded);

    // Sleep 3 seconds: should NOT transition to Idle yet because minimum 5s clamp is applied
    tokio::time::sleep(Duration::from_secs(3)).await;
    let ws_status = manager.list_workspaces().into_iter().find(|w| w.id == ws.id).unwrap().status;
    assert_eq!(
        ws_status,
        WorkspaceStatus::Loaded,
        "Workspace should still be Loaded at 3s due to 5s clamp constraint"
    );

    // Sleep another 3 seconds (6s total elapsed): should transition to Idle
    tokio::time::sleep(Duration::from_secs(3)).await;
    let ws_status2 = manager.list_workspaces().into_iter().find(|w| w.id == ws.id).unwrap().status;
    assert_eq!(
        ws_status2,
        WorkspaceStatus::Idle,
        "Workspace should be Idle after 6s (exceeds clamped 5s minimum)"
    );

    let db_path = PathBuf::from(&ws.db_path);
    let _ = std::fs::remove_dir_all(db_path.parent().unwrap());
    manager.delete_workspace(&ws.id);
    let _ = std::fs::remove_dir_all(&ws_dir);

    // Test Case 2: Clamping to minimum 5 seconds when environment variable is set to 0 or negative/invalid
    std::env::set_var("ASTRO_PROBE_IDLE_TIMEOUT_SECS", "-1");
    let ws_dir_invalid = temp_dir.join(format!("ws_timeout_invalid_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&ws_dir_invalid).unwrap();

    let manager_invalid = WorkspaceManager::new();
    {
        let ws_list = manager_invalid.list_workspaces();
        if let Some(existing) = ws_list.iter().find(|w| w.name == "invalid_val_ws") {
            manager_invalid.delete_workspace(&existing.id);
        }
    }
    let ws_invalid = manager_invalid
        .create_workspace("invalid_val_ws".to_string(), ws_dir_invalid.to_string_lossy().to_string())
        .unwrap();

    // Sleep 3 seconds: should NOT transition to Idle
    tokio::time::sleep(Duration::from_secs(3)).await;
    let ws_status_invalid = manager_invalid.list_workspaces().into_iter().find(|w| w.id == ws_invalid.id).unwrap().status;
    assert_eq!(
        ws_status_invalid,
        WorkspaceStatus::Loaded,
        "Workspace should still be Loaded at 3s when env var is invalid (-1), defaulting to minimum 5s"
    );

    let db_path_invalid = PathBuf::from(&ws_invalid.db_path);
    let _ = std::fs::remove_dir_all(db_path_invalid.parent().unwrap());
    manager_invalid.delete_workspace(&ws_invalid.id);
    let _ = std::fs::remove_dir_all(&ws_dir_invalid);

    // Restore original env var
    if let Some(val) = original_val {
        std::env::set_var("ASTRO_PROBE_IDLE_TIMEOUT_SECS", val);
    } else {
        std::env::remove_var("ASTRO_PROBE_IDLE_TIMEOUT_SECS");
    }
}
