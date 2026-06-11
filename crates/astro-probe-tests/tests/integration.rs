use std::path::Path;
use astro_probe_server::kernel::WorkspaceManager;

#[tokio::test]
async fn test_end_to_end_simple_spring() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let project_path = manifest_dir
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("test-samples")
        .join("simple-spring");

    assert!(project_path.exists(), "simple-spring test-sample path must exist");

    let manager = WorkspaceManager::new();
    let ws = manager
        .create_workspace("simple-spring-test".to_string(), project_path.to_string_lossy().to_string())
        .expect("Failed to create workspace");

    assert_eq!(ws.name, "simple-spring-test");
    
    let pool = manager
        .get_db_pool_and_touch(&ws.id)
        .expect("Failed to get DB pool");

    let conn = pool.get().expect("Failed to get connection");

    // Query counts to see what was parsed
    let class_count: i64 = conn.query_row("SELECT count(*) FROM classes", [], |r| r.get(0)).unwrap();
    let method_count: i64 = conn.query_row("SELECT count(*) FROM method_declarations", [], |r| r.get(0)).unwrap();
    let class_ann_count: i64 = conn.query_row("SELECT count(*) FROM class_annotations", [], |r| r.get(0)).unwrap();
    let field_ann_count: i64 = conn.query_row("SELECT count(*) FROM field_annotations", [], |r| r.get(0)).unwrap();
    println!("Parsed classes: {}, methods: {}, class_annotations: {}, field_annotations: {}", class_count, method_count, class_ann_count, field_ann_count);

    assert!(field_ann_count > 0, "Should have parsed some field annotations");

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
