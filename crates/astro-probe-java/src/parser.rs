use crate::{JavaError, Result};
use astro_probe_core::facts::{
    AllocationFact, AssignmentFact, CallArgumentFact, CallSiteFact, ClassAnnotationFact, ClassFact,
    Fact, FieldAnnotationFact, HierarchyFact, LibraryClassFact, MethodAnnotationFact, MethodFact,
    ParameterAnnotationFact,
};
use astro_probe_core::traits::SourceParser;
use std::collections::HashMap;
use std::path::Path;

pub struct JavaParser;

impl JavaParser {
    pub fn new() -> Self {
        Self
    }
}

impl Default for JavaParser {
    fn default() -> Self {
        Self::new()
    }
}

impl SourceParser for JavaParser {
    type Error = JavaError;

    fn parse_project(&self, project_path: &Path) -> std::result::Result<Vec<Fact>, Self::Error> {
        let temp_conn = rusqlite::Connection::open_in_memory()?;
        astro_probe_db::init_db(&temp_conn)?;
        self.parse_and_populate(project_path, &temp_conn)?;

        let mut facts = Vec::new();

        // 1. Classes
        let mut stmt = temp_conn.prepare("SELECT fqn, kind FROM classes")?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            facts.push(Fact::Class(ClassFact {
                fqn: row.get(0)?,
                kind: row.get(1)?,
            }));
        }

        // 2. Class Hierarchy
        let mut stmt = temp_conn.prepare("SELECT class_fqn, parent_fqn FROM class_hierarchy")?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            facts.push(Fact::Hierarchy(HierarchyFact {
                class_fqn: row.get(0)?,
                parent_fqn: row.get(1)?,
            }));
        }

        // 3. Method Declarations
        let mut stmt = temp_conn.prepare(
            "SELECT method_fqn, class_fqn, method_name, params FROM method_declarations",
        )?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            facts.push(Fact::Method(MethodFact {
                method_fqn: row.get(0)?,
                class_fqn: row.get(1)?,
                method_name: row.get(2)?,
                params: row.get(3)?,
            }));
        }

        // 4. Allocation Sites
        let mut stmt =
            temp_conn.prepare("SELECT alloc_id, class_fqn, method_fqn FROM allocation_sites")?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            facts.push(Fact::Allocation(AllocationFact {
                alloc_id: row.get(0)?,
                class_fqn: row.get(1)?,
                method_fqn: row.get(2)?,
            }));
        }

        // 5. Source Assignments
        let mut stmt = temp_conn
            .prepare("SELECT lhs, rhs, assignment_type, method_fqn FROM source_assignments")?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            facts.push(Fact::Assignment(AssignmentFact {
                lhs: row.get(0)?,
                rhs: row.get(1)?,
                assignment_type: row.get(2)?,
                method_fqn: row.get(3)?,
            }));
        }

        // 6. Call Sites
        let mut stmt = temp_conn.prepare(
            "SELECT call_id, method_fqn, receiver, method_name, lhs, static_callee FROM call_sites",
        )?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            facts.push(Fact::CallSite(CallSiteFact {
                call_id: row.get(0)?,
                method_fqn: row.get(1)?,
                receiver: row.get(2)?,
                method_name: row.get(3)?,
                lhs: row.get(4)?,
                static_callee: row.get(5)?,
            }));
        }

        // 7. Call Arguments
        let mut stmt = temp_conn
            .prepare("SELECT call_id, arg_index, arg_var, arg_type FROM call_arguments")?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            facts.push(Fact::CallArgument(CallArgumentFact {
                call_id: row.get(0)?,
                arg_index: row.get(1)?,
                arg_var: row.get(2)?,
                arg_type: row.get(3)?,
            }));
        }

        // 8. Class Annotations
        let mut stmt =
            temp_conn.prepare("SELECT class_fqn, annotation_name FROM class_annotations")?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            facts.push(Fact::ClassAnnotation(ClassAnnotationFact {
                class_fqn: row.get(0)?,
                annotation_name: row.get(1)?,
            }));
        }

        // 9. Field Annotations
        let mut stmt = temp_conn
            .prepare("SELECT class_fqn, field_name, annotation_name FROM field_annotations")?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            facts.push(Fact::FieldAnnotation(FieldAnnotationFact {
                class_fqn: row.get(0)?,
                field_name: row.get(1)?,
                annotation_name: row.get(2)?,
            }));
        }

        // 10. Method Annotations
        let mut stmt =
            temp_conn.prepare("SELECT method_fqn, annotation_name FROM method_annotations")?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            facts.push(Fact::MethodAnnotation(MethodAnnotationFact {
                method_fqn: row.get(0)?,
                annotation_name: row.get(1)?,
            }));
        }

        // 11. Parameter Annotations
        let mut stmt = temp_conn.prepare(
            "SELECT method_fqn, parameter_name, annotation_name FROM parameter_annotations",
        )?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            facts.push(Fact::ParameterAnnotation(ParameterAnnotationFact {
                method_fqn: row.get(0)?,
                parameter_name: row.get(1)?,
                annotation_name: row.get(2)?,
            }));
        }

        // 12. Library Classes
        let mut stmt = temp_conn.prepare("SELECT fqn FROM library_classes")?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            facts.push(Fact::LibraryClass(LibraryClassFact { fqn: row.get(0)? }));
        }

        Ok(facts)
    }
}

#[derive(Debug, Clone)]
struct JavaClassInfo {
    package_name: String,
    imports: Vec<String>,
    class_name: String,
    fields: Vec<(String, String)>, // (name, type)
    methods: Vec<JavaMethodInfo>,
    parents: Vec<String>,
    is_interface: bool,
    annotations: Vec<String>,
}

#[derive(Debug, Clone)]
struct JavaMethodInfo {
    method_name: String,
    signature: String, // list of parameter types (comma separated)
    body: String,
    is_constructor: bool,
    annotations: Vec<String>,
    parameter_annotations: Vec<(String, String)>, // (param_name, annotation)
}

#[derive(Debug, Clone)]
struct MethodCallInfo {
    receiver: Option<String>,
    method_name: String,
}

impl JavaParser {
    pub fn parse_and_populate<P: AsRef<Path>>(
        &self,
        project_path: P,
        conn: &rusqlite::Connection,
    ) -> Result<()> {
        let files = find_java_files(project_path.as_ref());
        println!(
            "parse_and_populate: project_path = {:?}, files found = {}",
            project_path.as_ref(),
            files.len()
        );

        // 1. Compute hashes for current files on disk
        let mut on_disk_hashes = HashMap::new();
        for file in &files {
            let path_str = file.to_string_lossy().to_string();
            if let Ok(hash) = compute_file_hash(file) {
                on_disk_hashes.insert(path_str, hash);
            }
        }

        // 2. Load existing file hashes from the database
        let mut stored_hashes = HashMap::new();
        let table_exists: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='file_hashes')",
            [],
            |row| row.get(0),
        ).unwrap_or(false);

        if table_exists {
            let mut stmt = conn.prepare("SELECT file_path, hash FROM file_hashes")?;
            let mut rows = stmt.query([])?;
            while let Some(row) = rows.next()? {
                let path: String = row.get(0)?;
                let hash: String = row.get(1)?;
                stored_hashes.insert(path, hash);
            }
        }

        // 3. Identify dirty, new, and deleted files
        let mut dirty_files = Vec::new();
        let mut new_files = Vec::new();
        let mut deleted_files = Vec::new();

        for (path_str, hash) in &on_disk_hashes {
            if let Some(stored_hash) = stored_hashes.get(path_str) {
                if stored_hash != hash {
                    dirty_files.push(path_str.clone());
                }
            } else {
                new_files.push(path_str.clone());
            }
        }

        for path_str in stored_hashes.keys() {
            if !on_disk_hashes.contains_key(path_str) {
                deleted_files.push(path_str.clone());
            }
        }

        // 4. For each dirty or deleted file, retrieve classes previously defined in them
        let mut classes_to_purge = std::collections::HashSet::new();
        if table_exists {
            let has_metadata: bool = conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='file_facts_metadata')",
                [],
                |row| row.get(0),
            ).unwrap_or(false);

            if has_metadata {
                for file_path in dirty_files.iter().chain(deleted_files.iter()) {
                    let mut stmt = conn.prepare(
                        "SELECT class_fqn FROM file_facts_metadata WHERE file_path = ?1",
                    )?;
                    let mut rows = stmt.query([file_path])?;
                    while let Some(row) = rows.next()? {
                        let fqn: String = row.get(0)?;
                        classes_to_purge.insert(fqn);
                    }
                }
            }
        }

        // 5. Purge old facts from the database for these classes and their methods
        if !classes_to_purge.is_empty() {
            conn.execute("BEGIN IMMEDIATE TRANSACTION;", [])?;
            let purge_res = (|| -> Result<()> {
                // Find all methods for classes to purge
                let mut methods_to_purge = std::collections::HashSet::new();
                for class_fqn in &classes_to_purge {
                    let mut stmt = conn.prepare(
                        "SELECT method_fqn FROM method_declarations WHERE class_fqn = ?1",
                    )?;
                    let mut rows = stmt.query([class_fqn])?;
                    while let Some(row) = rows.next()? {
                        let m_fqn: String = row.get(0)?;
                        methods_to_purge.insert(m_fqn);
                    }
                }

                // Delete from all tables as per rules
                for class_fqn in &classes_to_purge {
                    let class_prefix = format!("{}.", class_fqn);
                    let class_prefix_like = format!("{}%", class_prefix);

                    conn.execute("DELETE FROM classes WHERE fqn = ?1", [class_fqn])?;
                    conn.execute(
                        "DELETE FROM class_hierarchy WHERE class_fqn = ?1 OR parent_fqn = ?1",
                        [class_fqn],
                    )?;
                    conn.execute(
                        "DELETE FROM method_declarations WHERE class_fqn = ?1",
                        [class_fqn],
                    )?;
                    conn.execute(
                        "DELETE FROM allocation_sites WHERE class_fqn = ?1",
                        [class_fqn],
                    )?;

                    // Delete from source_assignments, call_sites, call_arguments
                    // Locate call_ids for call sites of methods to delete
                    let mut call_ids = Vec::new();
                    {
                        let mut stmt = conn.prepare("SELECT call_id FROM call_sites WHERE method_fqn = ?1 OR method_fqn LIKE ?2")?;
                        let mut rows = stmt.query([class_fqn, &class_prefix_like])?;
                        while let Some(row) = rows.next()? {
                            let cid: String = row.get(0)?;
                            call_ids.push(cid);
                        }
                    }
                    for cid in call_ids {
                        conn.execute("DELETE FROM call_arguments WHERE call_id = ?1", [&cid])?;
                    }

                    conn.execute("DELETE FROM source_assignments WHERE method_fqn = ?1 OR method_fqn LIKE ?2", [class_fqn, &class_prefix_like])?;
                    conn.execute(
                        "DELETE FROM call_sites WHERE method_fqn = ?1 OR method_fqn LIKE ?2",
                        [class_fqn, &class_prefix_like],
                    )?;
                    conn.execute(
                        "DELETE FROM class_annotations WHERE class_fqn = ?1",
                        [class_fqn],
                    )?;
                    conn.execute(
                        "DELETE FROM field_annotations WHERE class_fqn = ?1",
                        [class_fqn],
                    )?;
                    conn.execute("DELETE FROM method_annotations WHERE method_fqn = ?1 OR method_fqn LIKE ?2", [class_fqn, &class_prefix_like])?;
                    conn.execute("DELETE FROM parameter_annotations WHERE method_fqn = ?1 OR method_fqn LIKE ?2", [class_fqn, &class_prefix_like])?;
                }

                for file_path in dirty_files.iter().chain(deleted_files.iter()) {
                    conn.execute(
                        "DELETE FROM file_facts_metadata WHERE file_path = ?1",
                        [file_path],
                    )?;
                    conn.execute("DELETE FROM file_hashes WHERE file_path = ?1", [file_path])?;
                }

                Ok(())
            })();

            if purge_res.is_ok() {
                conn.execute("COMMIT;", [])?;
            } else {
                let _ = conn.execute("ROLLBACK;", []);
                return purge_res;
            }
        }

        // 6. Pre-populate Type Resolution
        let mut workspace_classes = HashMap::new();
        let has_classes_table = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='classes')",
                [],
                |row| row.get(0),
            )
            .unwrap_or(false);

        if has_classes_table {
            let mut stmt = conn.prepare("SELECT fqn FROM classes")?;
            let mut rows = stmt.query([])?;
            while let Some(row) = rows.next()? {
                let fqn: String = row.get(0)?;
                if !classes_to_purge.contains(&fqn) {
                    let simple_name = if let Some(idx) = fqn.rfind('.') {
                        &fqn[idx + 1..]
                    } else {
                        &fqn
                    };
                    workspace_classes.insert(simple_name.to_string(), fqn.clone());
                    if let Some(idx) = simple_name.rfind('$') {
                        let inner_simple = &simple_name[idx + 1..];
                        workspace_classes.insert(inner_simple.to_string(), fqn.clone());
                    }
                }
            }
        }

        // 7. Parse only the dirty and new files
        let mut classes = Vec::new();
        let files_to_parse: Vec<String> = dirty_files
            .iter()
            .chain(new_files.iter())
            .cloned()
            .collect();
        let mut file_to_classes: HashMap<String, Vec<String>> = HashMap::new();

        for file_path_str in &files_to_parse {
            let file_path = Path::new(file_path_str);
            if let Ok(content) = std::fs::read_to_string(file_path) {
                let stripped = strip_comments(&content);
                let (pkg, imports, name, kind, body, parents) =
                    parse_package_and_imports(&stripped);
                println!(
                    "file: {:?}, name = '{}', pkg = '{}', body_len = {}",
                    file_path,
                    name,
                    pkg,
                    body.len()
                );
                if !name.is_empty() {
                    let chars: Vec<char> = stripped.chars().collect();
                    let start_len = classes.len();
                    collect_classes_recursive(&chars, "", &pkg, &imports, &mut classes);

                    let mut file_classes = Vec::new();
                    for class in &classes[start_len..] {
                        let fqn = if class.package_name.is_empty() {
                            class.class_name.clone()
                        } else {
                            format!("{}.{}", class.package_name, class.class_name)
                        };
                        file_classes.push(fqn);
                    }
                    file_to_classes.insert(file_path_str.clone(), file_classes);
                }
            }
        }

        // Update workspace classes mapping with newly parsed ones
        for class in &classes {
            let fqn = if class.package_name.is_empty() {
                class.class_name.clone()
            } else {
                format!("{}.{}", class.package_name, class.class_name)
            };
            workspace_classes.insert(class.class_name.clone(), fqn.clone());
            if let Some(idx) = class.class_name.rfind('$') {
                let simple = &class.class_name[idx + 1..];
                workspace_classes.insert(simple.to_string(), fqn);
            }
        }

        // 8. Store populated facts in the database
        conn.execute("BEGIN IMMEDIATE TRANSACTION;", [])?;
        let populate_res = (|| -> Result<()> {
            for class in &classes {
                let fqn = if class.package_name.is_empty() {
                    class.class_name.clone()
                } else {
                    format!("{}.{}", class.package_name, class.class_name)
                };
                let kind = if class.is_interface {
                    "interface"
                } else {
                    "class"
                };

                conn.execute(
                    "INSERT OR REPLACE INTO classes (fqn, kind) VALUES (?1, ?2)",
                    [&fqn, kind],
                )?;

                for parent in &class.parents {
                    conn.execute(
                        "INSERT OR REPLACE INTO class_hierarchy (class_fqn, parent_fqn) VALUES (?1, ?2)",
                        [&fqn, parent],
                    )?;
                }

                for ann in &class.annotations {
                    conn.execute(
                        "INSERT OR REPLACE INTO class_annotations (class_fqn, annotation_name) VALUES (?1, ?2)",
                        [&fqn, ann],
                    )?;
                }

                // Add fields to field_annotations as FieldType:TYPE
                let mut fields_map = HashMap::new();
                for (fname, ftype) in &class.fields {
                    fields_map.insert(fname.clone(), ftype.clone());
                    let field_type_ann = format!("FieldType:{}", ftype);
                    conn.execute(
                        "INSERT OR REPLACE INTO field_annotations (class_fqn, field_name, annotation_name) VALUES (?1, ?2, ?3)",
                        [&fqn, fname, &field_type_ann],
                    )?;
                }

                // Field annotations
                let _class_body_stripped = strip_comments(&class.class_name);

                let mut alloc_counter = 0;

                for method in &class.methods {
                    let param_types = split_parameters(&method.signature);
                    let clean_param_types: Vec<String> = param_types
                        .iter()
                        .map(|t| {
                            let extracted_type = if let Some((_, ty)) = extract_type_and_name(t) {
                                ty
                            } else {
                                t.clone()
                            };
                            resolve_type_fqn(&extracted_type, class, &workspace_classes)
                        })
                        .collect();

                    let param_names =
                        extract_parameters_ordered(&format!("({})", method.signature));

                    let params_signature = clean_param_types.join(",");
                    let method_fqn =
                        format!("{}.{}({})", fqn, method.method_name, params_signature);

                    let param_names_str = param_names.join(",");
                    conn.execute(
                        "INSERT OR REPLACE INTO method_declarations (method_fqn, class_fqn, method_name, params) VALUES (?1, ?2, ?3, ?4)",
                        [&method_fqn, &fqn, &method.method_name, &param_names_str],
                    )?;

                    for ann in &method.annotations {
                        conn.execute(
                            "INSERT OR REPLACE INTO method_annotations (method_fqn, annotation_name) VALUES (?1, ?2)",
                            [&method_fqn, ann],
                        )?;
                    }

                    for (p_name, p_ann) in &method.parameter_annotations {
                        conn.execute(
                            "INSERT OR REPLACE INTO parameter_annotations (method_fqn, parameter_name, annotation_name) VALUES (?1, ?2, ?3)",
                            [&method_fqn, p_name, p_ann],
                        )?;
                    }

                    let mut local_vars = extract_local_variables(&method.body);
                    let method_params = extract_parameters(&format!("({})", method.signature));
                    for (p_name, p_type) in method_params {
                        local_vars.insert(p_name, p_type);
                    }

                    for (idx, p_name) in param_names.iter().enumerate() {
                        let param_node = format!("{}#{}", method_fqn, p_name);
                        let pos_node = format!("{}#p{}", method_fqn, idx);
                        conn.execute(
                            "INSERT INTO source_assignments (lhs, rhs, assignment_type, method_fqn) VALUES (?1, ?2, 'COPY', ?3)",
                            [&param_node, &pos_node, &method_fqn],
                        )?;
                    }

                    let statements = split_statements(&method.body);
                    for stmt in statements {
                        let preprocessed = preprocess_statement(
                            &stmt,
                            &method_fqn,
                            &mut alloc_counter,
                            conn,
                            class,
                            &workspace_classes,
                            &mut local_vars,
                        );
                        for prep_stmt in preprocessed {
                            if let Err(e) = process_rhs_expression_wrapper(
                                &prep_stmt,
                                &method_fqn,
                                &mut alloc_counter,
                                conn,
                                class,
                                &workspace_classes,
                                &local_vars,
                                &fields_map,
                            ) {
                                tracing::debug!(
                                    "Failed parsing statement: {} error: {:?}",
                                    prep_stmt,
                                    e
                                );
                            }
                        }
                    }
                }
            }

            // 9. Update file_hashes and file_facts_metadata for the newly parsed files
            for file_path in &files_to_parse {
                if let Some(hash) = on_disk_hashes.get(file_path) {
                    conn.execute(
                        "INSERT OR REPLACE INTO file_hashes (file_path, hash) VALUES (?1, ?2)",
                        [file_path, hash],
                    )?;
                }

                if let Some(classes_in_file) = file_to_classes.get(file_path) {
                    for class_fqn in classes_in_file {
                        conn.execute(
                            "INSERT OR REPLACE INTO file_facts_metadata (file_path, class_fqn) VALUES (?1, ?2)",
                            [file_path, class_fqn],
                        )?;
                    }
                }
            }

            Ok(())
        })();

        if populate_res.is_ok() {
            conn.execute("COMMIT;", [])?;
            Ok(())
        } else {
            let _ = conn.execute("ROLLBACK;", []);
            populate_res
        }
    }
}

fn process_rhs_expression_wrapper(
    stmt: &str,
    caller_fqn: &str,
    alloc_counter: &mut usize,
    conn: &rusqlite::Connection,
    class: &JavaClassInfo,
    workspace_classes: &HashMap<String, String>,
    local_vars: &HashMap<String, String>,
    fields_map: &HashMap<String, String>,
) -> Result<()> {
    if let Some((lhs, rhs)) = split_assignment(stmt) {
        process_rhs_expression(
            &lhs,
            &rhs,
            caller_fqn,
            alloc_counter,
            conn,
            class,
            workspace_classes,
            local_vars,
            fields_map,
        )?;
    } else if starts_with_keyword(stmt, "return") {
        let expr = stmt.strip_prefix("return").unwrap().trim();
        if !expr.is_empty() {
            let ret_var = format!("{}#return", caller_fqn);
            if expr.contains('(') {
                // Call site directly inside return statement
                process_rhs_expression(
                    &ret_var,
                    expr,
                    caller_fqn,
                    alloc_counter,
                    conn,
                    class,
                    workspace_classes,
                    local_vars,
                    fields_map,
                )?;
            } else {
                let simple = resolve_to_simple_var(
                    expr,
                    caller_fqn,
                    alloc_counter,
                    conn,
                    class,
                    workspace_classes,
                    local_vars,
                    fields_map,
                )?;
                conn.execute(
                    "INSERT INTO source_assignments (lhs, rhs, assignment_type, method_fqn) VALUES (?1, ?2, 'COPY', ?3)",
                    [&ret_var, &simple, caller_fqn],
                )?;
            }
        }
    } else if expr_looks_like_call(stmt) {
        // standalone call expression (e.g. doSomething();)
        let dummy_lhs = format!("temp_void_call_{}", alloc_counter);
        *alloc_counter += 1;
        process_rhs_expression(
            &dummy_lhs,
            stmt,
            caller_fqn,
            alloc_counter,
            conn,
            class,
            workspace_classes,
            local_vars,
            fields_map,
        )?;
    }
    Ok(())
}

fn expr_looks_like_call(expr: &str) -> bool {
    let expr = expr.trim();
    expr.contains('(') && expr.ends_with(')')
}

fn find_java_files(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                files.extend(find_java_files(&path));
            } else if path.extension().and_then(|s| s.to_str()) == Some("java") {
                files.push(path);
            }
        }
    }
    files
}

fn strip_comments(code: &str) -> String {
    let mut result = String::new();
    let chars: Vec<char> = code.chars().collect();
    let mut i = 0;
    let mut in_line_comment = false;
    let mut in_block_comment = false;
    let mut in_string = false;
    let mut in_char = false;
    let mut in_escape = false;

    while i < chars.len() {
        let c = chars[i];
        if in_line_comment {
            if c == '\n' {
                in_line_comment = false;
                result.push('\n');
            }
        } else if in_block_comment {
            if c == '*' && i + 1 < chars.len() && chars[i + 1] == '/' {
                in_block_comment = false;
                i += 1;
            }
        } else if in_string {
            if in_escape {
                in_escape = false;
            } else if c == '\\' {
                in_escape = true;
            } else if c == '"' {
                in_string = false;
            }
            // Replace characters in strings to normalize literals, but keep length or just clear
        } else if in_char {
            if in_escape {
                in_escape = false;
            } else if c == '\\' {
                in_escape = true;
            } else if c == '\'' {
                in_char = false;
            }
        } else {
            if c == '/' && i + 1 < chars.len() && chars[i + 1] == '/' {
                in_line_comment = true;
                i += 1;
            } else if c == '/' && i + 1 < chars.len() && chars[i + 1] == '*' {
                in_block_comment = true;
                i += 1;
            } else if c == '"' {
                in_string = true;
                in_escape = false;
                result.push('"');
                result.push('"');
            } else if c == '\'' {
                in_char = true;
                in_escape = false;
                result.push('\'');
                result.push('\'');
            } else {
                result.push(c);
            }
        }
        i += 1;
    }
    result
}

fn parse_package_and_imports(
    code: &str,
) -> (String, Vec<String>, String, String, String, Vec<String>) {
    let mut pkg = String::new();
    let mut imports = Vec::new();
    let mut class_name = String::new();
    let mut class_kind = String::new();
    let mut class_body = String::new();
    let mut parents = Vec::new();

    let statements = split_statements(code);
    for stmt in &statements {
        let trimmed = stmt.trim();
        if trimmed.starts_with("package ") {
            pkg = trimmed["package ".len()..]
                .replace(';', "")
                .trim()
                .to_string();
        } else if trimmed.starts_with("import ") {
            let imp = trimmed["import ".len()..]
                .replace(';', "")
                .trim()
                .to_string();
            // Ignore static imports wildcard or sub imports
            imports.push(imp);
        }
    }

    // Extract class body using scan
    let chars: Vec<char> = code.chars().collect();
    let dc = scan_direct_classes(&chars);
    if let Some(first) = dc.first() {
        class_name = first.name.clone();
        class_kind = first.kind.clone();
        class_body = first.body_chars.iter().collect();
        parents = parse_inheritance(&first.header);
    }

    let resolved_parents = parents
        .iter()
        .map(|p| resolve_parent_fqn(p, &pkg, &imports))
        .collect();

    (
        pkg,
        imports,
        class_name,
        class_kind,
        class_body,
        resolved_parents,
    )
}

fn resolve_parent_fqn(parent: &str, pkg: &str, imports: &[String]) -> String {
    let clean = parent.trim();
    for imp in imports {
        if imp.ends_with(&format!(".{}", clean)) {
            return imp.clone();
        }
    }
    if pkg.is_empty() {
        clean.to_string()
    } else {
        format!("{}.{}", pkg, clean)
    }
}

fn parse_fields_annotations_from_body(_body: &str) -> HashMap<String, Vec<String>> {
    HashMap::new()
}

fn parse_class_body(
    package_name: &str,
    imports: &[String],
    class_fqn: &str,
    body: &str,
) -> (Vec<(String, String)>, Vec<JavaMethodInfo>) {
    let mut fields = Vec::new();
    let mut methods = Vec::new();

    let chars: Vec<char> = body.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_whitespace() {
            i += 1;
            continue;
        }

        // Check for annotations at field/method declaration level
        let annotations = extract_and_strip_annotations_at(&chars, &mut i);

        // Find next semicolon or brace
        let mut sem_idx = None;
        let mut brace_idx = None;
        for j in i..chars.len() {
            if chars[j] == ';' {
                sem_idx = Some(j);
                break;
            }
            if chars[j] == '{' {
                brace_idx = Some(j);
                break;
            }
        }

        match (sem_idx, brace_idx) {
            (Some(s), None) => {
                let decl: String = chars[i..s].iter().collect();
                if let Some((name, ty)) = extract_field_info(&decl) {
                    fields.push((name, ty));
                }
                i = s + 1;
            }
            (Some(s), Some(b)) => {
                if s < b {
                    // Field declaration
                    let decl: String = chars[i..s].iter().collect();
                    if let Some((name, ty)) = extract_field_info(&decl) {
                        fields.push((name, ty));
                    }
                    i = s + 1;
                } else {
                    // Method or class declaration
                    if let Some(matching_brace) = find_matching_brace(&chars, b) {
                        let header: String = chars[i..b].iter().collect();
                        let method_body: String = chars[b + 1..matching_brace].iter().collect();

                        if let Some(m_name) = extract_method_name(&header) {
                            let is_constructor = m_name == class_fqn
                                || (class_fqn.contains('$')
                                    && class_fqn.ends_with(&format!("${}", m_name)));
                            let clean_m_name = if is_constructor {
                                "<init>".to_string()
                            } else {
                                m_name
                            };

                            let signature = if let (Some(sp), Some(ep)) =
                                (header.find('('), header.rfind(')'))
                            {
                                if sp < ep {
                                    header[sp + 1..ep].to_string()
                                } else {
                                    String::new()
                                }
                            } else {
                                String::new()
                            };

                            // Parameter annotations
                            let mut param_annotations = Vec::new();
                            let params_list = split_parameters(&signature);
                            for p in params_list {
                                let param_anns = extract_annotations_from_string(&p);
                                if let Some((p_name, p_type)) = extract_type_and_name(&p) {
                                    for pa in param_anns {
                                        param_annotations.push((p_name.clone(), pa));
                                    }
                                    // FieldType annotation for DI resolution
                                    let clean_type = strip_generics(&p_type);
                                    let resolved_pt = resolve_type_fqn_simple(
                                        &clean_type,
                                        package_name,
                                        imports,
                                        class_fqn,
                                    );
                                    param_annotations.push((
                                        p_name.clone(),
                                        format!("FieldType:{}", resolved_pt),
                                    ));
                                }
                            }

                            methods.push(JavaMethodInfo {
                                method_name: clean_m_name,
                                signature,
                                body: method_body,
                                is_constructor,
                                annotations,
                                parameter_annotations: param_annotations,
                            });
                        }
                        i = matching_brace + 1;
                    } else {
                        i = b + 1;
                    }
                }
            }
            (None, Some(b)) => {
                // Method or block without semicolon
                if let Some(matching_brace) = find_matching_brace(&chars, b) {
                    let header: String = chars[i..b].iter().collect();
                    let method_body: String = chars[b + 1..matching_brace].iter().collect();
                    if let Some(m_name) = extract_method_name(&header) {
                        let is_constructor = m_name == class_fqn
                            || (class_fqn.contains('$')
                                && class_fqn.ends_with(&format!("${}", m_name)));
                        let clean_m_name = if is_constructor {
                            "<init>".to_string()
                        } else {
                            m_name
                        };
                        let signature =
                            if let (Some(sp), Some(ep)) = (header.find('('), header.rfind(')')) {
                                if sp < ep {
                                    header[sp + 1..ep].to_string()
                                } else {
                                    String::new()
                                }
                            } else {
                                String::new()
                            };

                        methods.push(JavaMethodInfo {
                            method_name: clean_m_name,
                            signature,
                            body: method_body,
                            is_constructor,
                            annotations,
                            parameter_annotations: Vec::new(),
                        });
                    }
                    i = matching_brace + 1;
                } else {
                    i = b + 1;
                }
            }
            (None, None) => {
                break;
            }
        }
    }

    (fields, methods)
}

fn resolve_type_fqn_simple(
    type_name: &str,
    package_name: &str,
    imports: &[String],
    class_fqn: &str,
) -> String {
    for imp in imports {
        if imp.ends_with(&format!(".{}", type_name)) {
            return imp.clone();
        }
    }
    if package_name.is_empty() {
        type_name.to_string()
    } else {
        format!("{}.{}", package_name, type_name)
    }
}

fn extract_method_name(header: &str) -> Option<String> {
    let idx = header.find('(')?;
    let before_paren = &header[..idx].trim();
    let name = before_paren.split_whitespace().last()?;
    if is_filtered_keyword(name) {
        return None;
    }
    Some(name.to_string())
}

fn extract_type_and_name(decl: &str) -> Option<(String, String)> {
    let decl = decl.trim();
    if decl.is_empty() {
        return None;
    }

    let chars: Vec<char> = decl.chars().collect();
    let mut j = chars.len();

    // Find the end of the identifier name
    while j > 0 && !chars[j - 1].is_alphanumeric() && chars[j - 1] != '_' && chars[j - 1] != '$' {
        j -= 1;
    }
    let name_end = j;

    // Find the start of the identifier name
    while j > 0 && (chars[j - 1].is_alphanumeric() || chars[j - 1] == '_' || chars[j - 1] == '$') {
        j -= 1;
    }
    let name_start = j;

    if name_start == name_end {
        return None;
    }

    let name: String = chars[name_start..name_end].iter().collect();
    let prefix = chars[..name_start].iter().collect::<String>();
    let prefix_trimmed = prefix.trim();
    if prefix_trimmed.is_empty() {
        return None;
    }

    // Scan type backwards tracking generic and array brackets
    let prefix_chars: Vec<char> = prefix_trimmed.chars().collect();
    let mut k = prefix_chars.len();
    let mut bracket_depth: i32 = 0;
    let mut square_bracket_depth: i32 = 0;

    while k > 0 {
        let c = prefix_chars[k - 1];
        if c == '>' {
            bracket_depth += 1;
            k -= 1;
        } else if c == '<' {
            bracket_depth = bracket_depth.saturating_sub(1);
            k -= 1;
        } else if c == ']' {
            square_bracket_depth += 1;
            k -= 1;
        } else if c == '[' {
            square_bracket_depth = square_bracket_depth.saturating_sub(1);
            k -= 1;
        } else if c.is_whitespace() && bracket_depth == 0 && square_bracket_depth == 0 {
            break;
        } else {
            k -= 1;
        }
    }

    let ty: String = prefix_chars[k..].iter().collect();
    let ty = ty.trim().to_string();
    if ty.is_empty() {
        return None;
    }

    Some((name, ty))
}

fn extract_field_info(decl: &str) -> Option<(String, String)> {
    let left = decl.split('=').next()?.trim();
    if let Some((name, ty)) = extract_type_and_name(left) {
        if is_filtered_keyword(&name) {
            return None;
        }
        if is_filtered_keyword(&ty) && !is_primitive_type(&ty) {
            return None;
        }
        Some((name, ty))
    } else {
        None
    }
}

fn split_statements(body: &str) -> Vec<String> {
    let mut statements = Vec::new();
    let mut current = String::new();
    let mut in_string = false;
    let mut in_char = false;
    let mut in_escape = false;
    let mut paren_depth: i32 = 0;
    let mut brace_depth: i32 = 0;

    for c in body.chars() {
        if in_string {
            current.push(c);
            if in_escape {
                in_escape = false;
            } else if c == '\\' {
                in_escape = true;
            } else if c == '"' {
                in_string = false;
            }
        } else if in_char {
            current.push(c);
            if in_escape {
                in_escape = false;
            } else if c == '\\' {
                in_escape = true;
            } else if c == '\'' {
                in_char = false;
            }
        } else {
            if c == '(' {
                paren_depth += 1;
                current.push(c);
            } else if c == ')' {
                paren_depth = paren_depth.saturating_sub(1);
                current.push(c);
            } else if c == '{' {
                brace_depth += 1;
                let trimmed = current.trim();
                if !trimmed.is_empty() {
                    statements.push(trimmed.to_string());
                }
                current.clear();
            } else if c == '}' {
                brace_depth = brace_depth.saturating_sub(1);
                let trimmed = current.trim();
                if !trimmed.is_empty() {
                    statements.push(trimmed.to_string());
                }
                current.clear();
            } else if c == ';' && paren_depth == 0 {
                statements.push(current.trim().to_string());
                current.clear();
            } else {
                current.push(c);
                if c == '"' {
                    in_string = true;
                    in_escape = false;
                } else if c == '\'' {
                    in_char = true;
                    in_escape = false;
                }
            }
        }
    }
    if !current.trim().is_empty() {
        statements.push(current.trim().to_string());
    }
    statements
}

fn starts_with_keyword(s: &str, keyword: &str) -> bool {
    if s == keyword {
        return true;
    }
    if s.starts_with(keyword) {
        if let Some(c) = s.chars().nth(keyword.len()) {
            return !c.is_alphanumeric() && c != '_';
        }
    }
    false
}

fn find_matching_paren(s: &str) -> Option<usize> {
    let chars: Vec<char> = s.chars().collect();
    if chars.is_empty() || chars[0] != '(' {
        return None;
    }
    let mut depth = 0;
    for (i, &c) in chars.iter().enumerate() {
        if c == '(' {
            depth += 1;
        } else if c == ')' {
            depth -= 1;
            if depth == 0 {
                return Some(i);
            }
        }
    }
    None
}

fn clean_statement(s: &str) -> String {
    let cleaned = s.replace('{', " ").replace('}', " ");
    let mut trimmed = cleaned.trim().to_string();

    loop {
        let prev = trimmed.clone();
        if trimmed.starts_with("else") {
            trimmed = trimmed["else".len()..].trim().to_string();
        }
        if trimmed.starts_with("try") {
            trimmed = trimmed["try".len()..].trim().to_string();
        }
        if trimmed.starts_with("finally") {
            trimmed = trimmed["finally".len()..].trim().to_string();
        }
        if trimmed.starts_with("if") {
            let rem = trimmed["if".len()..].trim();
            if rem.starts_with('(') {
                if let Some(matching_paren) = find_matching_paren(rem) {
                    trimmed = rem[matching_paren + 1..].trim().to_string();
                }
            }
        }
        if trimmed.starts_with("while") {
            let rem = trimmed["while".len()..].trim();
            if rem.starts_with('(') {
                if let Some(matching_paren) = find_matching_paren(rem) {
                    trimmed = rem[matching_paren + 1..].trim().to_string();
                }
            }
        }
        if trimmed.starts_with("catch") {
            let rem = trimmed["catch".len()..].trim();
            if rem.starts_with('(') {
                if let Some(matching_paren) = find_matching_paren(rem) {
                    trimmed = rem[matching_paren + 1..].trim().to_string();
                }
            }
        }
        if trimmed.starts_with("for") {
            let rem = trimmed["for".len()..].trim();
            if rem.starts_with('(') {
                if let Some(matching_paren) = find_matching_paren(rem) {
                    trimmed = rem[matching_paren + 1..].trim().to_string();
                }
            }
        }
        if trimmed == prev {
            break;
        }
    }
    trimmed
}

fn extract_local_variables(body: &str) -> HashMap<String, String> {
    let mut vars = HashMap::new();
    let statements = split_statements(body);
    for stmt in statements {
        let cleaned = clean_statement(&stmt);
        let mut part = cleaned.trim();
        if part.is_empty() {
            continue;
        }
        if let Some(eq_idx) = part.find('=') {
            part = part[..eq_idx].trim();
        }

        if starts_with_keyword(part, "return")
            || starts_with_keyword(part, "throw")
            || starts_with_keyword(part, "assert")
            || starts_with_keyword(part, "import")
            || starts_with_keyword(part, "package")
            || part.contains('(')
            || part.contains(')')
            || part.contains('.')
        {
            continue;
        }

        if let Some((name, ty)) = extract_type_and_name(part) {
            if !is_filtered_keyword(&name) && (!is_filtered_keyword(&ty) || is_primitive_type(&ty))
            {
                vars.insert(name, ty);
            }
        }
    }
    vars
}

fn split_parameters(params_str: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut bracket_depth: i32 = 0;
    let mut paren_depth: i32 = 0;
    for c in params_str.chars() {
        if c == '<' {
            bracket_depth += 1;
            current.push(c);
        } else if c == '>' {
            bracket_depth = bracket_depth.saturating_sub(1);
            current.push(c);
        } else if c == '(' {
            paren_depth += 1;
            current.push(c);
        } else if c == ')' {
            paren_depth = paren_depth.saturating_sub(1);
            current.push(c);
        } else if c == ',' && bracket_depth == 0 && paren_depth == 0 {
            parts.push(current.trim().to_string());
            current.clear();
        } else {
            current.push(c);
        }
    }
    if !current.trim().is_empty() {
        parts.push(current.trim().to_string());
    }
    parts
}

fn extract_parameters(header: &str) -> HashMap<String, String> {
    let mut vars = HashMap::new();
    if let Some(start) = header.find('(') {
        if let Some(end) = header.rfind(')') {
            if start < end {
                let params_str = &header[start + 1..end];
                let params = split_parameters(params_str);
                for param in params {
                    if let Some((name, ty)) = extract_type_and_name(&param) {
                        if !is_filtered_keyword(&name)
                            && (!is_filtered_keyword(&ty) || is_primitive_type(&ty))
                        {
                            vars.insert(name, ty);
                        }
                    }
                }
            }
        }
    }
    vars
}

fn get_qualifier_before(chars: &[char], start_idx: usize) -> String {
    let mut j = start_idx;
    while j > 0 && chars[j - 1].is_whitespace() {
        j -= 1;
    }
    let mut qualifier_chars = Vec::new();
    let mut paren_depth: i32 = 0;
    let mut brace_depth: i32 = 0;

    while j > 0 {
        let c = chars[j - 1];
        if paren_depth > 0 {
            if c == ')' {
                paren_depth += 1;
            } else if c == '(' {
                paren_depth -= 1;
            }
            j -= 1;
        } else if brace_depth > 0 {
            if c == ']' {
                brace_depth += 1;
            } else if c == '[' {
                brace_depth -= 1;
            }
            j -= 1;
        } else {
            if c == ')' {
                paren_depth = 1;
                j -= 1;
            } else if c == ']' {
                brace_depth = 1;
                j -= 1;
            } else if c.is_alphanumeric() || c == '_' || c == '.' {
                qualifier_chars.push(c);
                j -= 1;
            } else if c.is_whitespace() {
                let mut temp_j = j - 1;
                while temp_j > 0 && chars[temp_j - 1].is_whitespace() {
                    temp_j -= 1;
                }
                if temp_j >= 3
                    && chars[temp_j - 1] == 'w'
                    && chars[temp_j - 2] == 'e'
                    && chars[temp_j - 3] == 'n'
                    && (temp_j == 3
                        || !chars[temp_j - 4].is_alphanumeric() && chars[temp_j - 4] != '_')
                {
                    qualifier_chars.push(' ');
                    qualifier_chars.push('w');
                    qualifier_chars.push('e');
                    qualifier_chars.push('n');
                    j = temp_j - 3;
                } else {
                    break;
                }
            } else {
                break;
            }
        }
    }
    qualifier_chars.into_iter().rev().collect::<String>()
}

fn is_filtered_keyword(name: &str) -> bool {
    let keywords = [
        "abstract",
        "assert",
        "boolean",
        "break",
        "byte",
        "case",
        "catch",
        "char",
        "class",
        "const",
        "continue",
        "default",
        "do",
        "double",
        "else",
        "enum",
        "extends",
        "final",
        "finally",
        "float",
        "for",
        "goto",
        "if",
        "implements",
        "import",
        "instanceof",
        "int",
        "interface",
        "long",
        "native",
        "new",
        "package",
        "private",
        "protected",
        "public",
        "return",
        "short",
        "static",
        "strictfp",
        "super",
        "switch",
        "synchronized",
        "this",
        "throw",
        "throws",
        "transient",
        "try",
        "void",
        "volatile",
        "while",
        "record",
        "yield",
        "sealed",
        "non-sealed",
        "permits",
    ];
    keywords.contains(&name) || name.starts_with("new ")
}

fn is_primitive_type(s: &str) -> bool {
    let primitives = [
        "boolean", "byte", "char", "double", "float", "int", "long", "short", "void",
    ];
    primitives.contains(&s)
}

fn resolve_type_fqn(
    type_name: &str,
    class: &JavaClassInfo,
    workspace_classes: &HashMap<String, String>,
) -> String {
    let clean_type_name = if let Some(idx) = type_name.find('<') {
        &type_name[..idx]
    } else {
        type_name
    };

    for imp in &class.imports {
        if imp.ends_with(&format!(".{}", clean_type_name)) {
            return imp.clone();
        }
    }

    let mut parts: Vec<&str> = class.class_name.split('$').collect();
    while !parts.is_empty() {
        let prefix = parts.join("$");
        let candidate = format!("{}${}", prefix, clean_type_name);
        if let Some(fqn) = workspace_classes.get(&candidate) {
            return fqn.clone();
        }
        parts.pop();
    }

    if let Some(fqn) = workspace_classes.get(clean_type_name) {
        return fqn.clone();
    }
    if class.package_name.is_empty() {
        clean_type_name.to_string()
    } else {
        format!("{}.{}", class.package_name, clean_type_name)
    }
}

fn is_workspace_class(
    name: &str,
    class: &JavaClassInfo,
    workspace_classes: &HashMap<String, String>,
) -> bool {
    get_workspace_class_fqn(name, class, workspace_classes).is_some()
}

fn get_workspace_class_fqn(
    name: &str,
    class: &JavaClassInfo,
    workspace_classes: &HashMap<String, String>,
) -> Option<String> {
    let clean_name = if let Some(idx) = name.find('<') {
        &name[..idx]
    } else {
        name
    };

    let mut parts: Vec<&str> = class.class_name.split('$').collect();
    while !parts.is_empty() {
        let prefix = parts.join("$");
        let candidate = format!("{}${}", prefix, clean_name);
        if let Some(fqn) = workspace_classes.get(&candidate) {
            return Some(fqn.clone());
        }
        parts.pop();
    }

    if let Some(fqn) = workspace_classes.get(clean_name) {
        return Some(fqn.clone());
    }

    None
}

fn strip_generics(s: &str) -> String {
    let mut result = String::new();
    let mut depth: i32 = 0;
    for c in s.chars() {
        if c == '<' {
            depth += 1;
        } else if c == '>' {
            depth = depth.saturating_sub(1);
        } else if depth == 0 {
            result.push(c);
        }
    }
    result
}

fn parse_inheritance(header: &str) -> Vec<String> {
    let clean = strip_generics(header).replace(',', " ");
    let tokens: Vec<&str> = clean.split_whitespace().collect();
    let mut parents = Vec::new();

    let mut mode = None;
    for &token in &tokens {
        if token == "extends" {
            mode = Some("extends");
        } else if token == "implements" {
            mode = Some("implements");
        } else if token == "class" || token == "interface" {
            mode = None;
        } else if let Some(_) = mode {
            parents.push(token.to_string());
        }
    }
    parents
}

fn find_assignment_eq(s: &str) -> Option<usize> {
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '=' {
            if i + 1 < chars.len() && chars[i + 1] == '=' {
                i += 2;
                continue;
            }
            if i > 0 {
                let prev = chars[i - 1];
                if prev == '!'
                    || prev == '+'
                    || prev == '-'
                    || prev == '*'
                    || prev == '/'
                    || prev == '<'
                    || prev == '>'
                {
                    i += 1;
                    continue;
                }
            }
            return Some(i);
        }
        i += 1;
    }
    None
}

fn parse_call_expr(expr: &str) -> Option<(Option<String>, String, Vec<String>)> {
    let idx = expr.find('(')?;
    let before_paren = expr[..idx].trim();
    let after_paren = expr[idx + 1..].trim();
    let end_paren = after_paren.rfind(')')?;
    let args_str = &after_paren[..end_paren];

    let before_paren_tokens: Vec<&str> = before_paren.split_whitespace().collect();
    let last_token = before_paren_tokens.last()?;

    let (receiver, method_name) = if let Some(dot_idx) = last_token.rfind('.') {
        let rec = &last_token[..dot_idx];
        let name = &last_token[dot_idx + 1..];
        (Some(rec.to_string()), name.to_string())
    } else {
        (None, last_token.to_string())
    };

    if is_filtered_keyword(&method_name) {
        return None;
    }

    let args = split_parameters(args_str)
        .into_iter()
        .map(|a| a.trim().to_string())
        .filter(|a| !a.is_empty())
        .collect();

    Some((receiver, method_name, args))
}

fn extract_parameters_ordered(header: &str) -> Vec<String> {
    let mut names = Vec::new();
    if let Some(start) = header.find('(') {
        if let Some(end) = header.rfind(')') {
            if start < end {
                let params_str = &header[start + 1..end];
                let params = split_parameters(params_str);
                for param in params {
                    if let Some((name, _ty)) = extract_type_and_name(&param) {
                        if !is_filtered_keyword(&name) {
                            names.push(name);
                        }
                    }
                }
            }
        }
    }
    names
}

fn extract_parameter_types_and_names(header: &str) -> Vec<(String, String)> {
    let mut result = Vec::new();
    if let Some(start) = header.find('(') {
        if let Some(end) = header.rfind(')') {
            if start < end {
                let params_str = &header[start + 1..end];
                let params = split_parameters(params_str);
                for param in params {
                    if let Some((name, ty)) = extract_type_and_name(&param) {
                        if !is_filtered_keyword(&name) {
                            result.push((ty, name));
                        }
                    }
                }
            }
        }
    }
    result
}

fn process_rhs_expression(
    lhs_raw: &str,
    rhs_part: &str,
    caller_fqn: &str,
    alloc_counter: &mut usize,
    conn: &rusqlite::Connection,
    class: &JavaClassInfo,
    workspace_classes: &HashMap<String, String>,
    local_vars: &HashMap<String, String>,
    fields_map: &HashMap<String, String>,
) -> Result<()> {
    let rhs_part = rhs_part.trim();
    if rhs_part.starts_with("StringAlloc:") || rhs_part.starts_with("ReflectClassAlloc:") {
        let lhs_var = format!("{}#{}", caller_fqn, lhs_raw);
        conn.execute(
            "INSERT INTO source_assignments (lhs, rhs, assignment_type, method_fqn) VALUES (?1, ?2, 'ALLOC', ?3)",
            [lhs_var.as_str(), rhs_part, caller_fqn],
        )?;
        return Ok(());
    }
    if rhs_part.contains("new ") {
        let mut type_name = String::new();
        if let Some(new_idx) = rhs_part.find("new ") {
            let after_new = rhs_part[new_idx + 4..].trim();
            for c in after_new.chars() {
                if c.is_alphanumeric() || c == '_' || c == '<' || c == '>' || c == '[' || c == ']' {
                    type_name.push(c);
                } else {
                    break;
                }
            }
        }
        let clean_type = strip_generics(&type_name);
        let resolved_type = resolve_type_fqn(&clean_type, class, workspace_classes);

        let alloc_id = format!("{}:alloc_{}", caller_fqn, alloc_counter);
        *alloc_counter += 1;

        conn.execute(
            "INSERT OR REPLACE INTO allocation_sites (alloc_id, class_fqn, method_fqn) VALUES (?1, ?2, ?3)",
            [alloc_id.as_str(), resolved_type.as_str(), caller_fqn],
        )?;

        let temp_var = format!("temp_alloc_{}", alloc_counter);
        *alloc_counter += 1;
        let temp_var_fqn = format!("{}#{}", caller_fqn, temp_var);

        conn.execute(
            "INSERT INTO source_assignments (lhs, rhs, assignment_type, method_fqn) VALUES (?1, ?2, 'ALLOC', ?3)",
            [temp_var_fqn.as_str(), alloc_id.as_str(), caller_fqn],
        )?;

        handle_field_write(
            lhs_raw,
            &temp_var_fqn,
            caller_fqn,
            alloc_counter,
            conn,
            class,
            workspace_classes,
            local_vars,
            fields_map,
        )?;
    } else if let Some((receiver, method_name, args)) = parse_call_expr(rhs_part) {
        let resolved_receiver = match receiver {
            Some(ref rec) => {
                if rec == "System.out" || rec == "System.err" {
                    None
                } else if is_workspace_class(rec, class, workspace_classes) {
                    None
                } else {
                    let rec_simple = resolve_to_simple_var(
                        rec,
                        caller_fqn,
                        alloc_counter,
                        conn,
                        class,
                        workspace_classes,
                        local_vars,
                        fields_map,
                    )?;
                    Some(rec_simple)
                }
            }
            None => Some(format!("{}#this", caller_fqn)),
        };

        let current_class_fqn = if class.package_name.is_empty() {
            class.class_name.clone()
        } else {
            format!("{}.{}", class.package_name, class.class_name)
        };

        let receiver_type = match receiver {
            None => current_class_fqn.to_string(),
            Some(ref rec) => {
                if rec == "System.out" || rec == "System.err" {
                    "java.io.PrintStream".to_string()
                } else if let Some(local_type) = local_vars.get(rec) {
                    resolve_type_fqn(local_type, class, workspace_classes)
                } else if let Some(field_type) = fields_map.get(rec) {
                    resolve_type_fqn(field_type, class, workspace_classes)
                } else if let Some(fqn) = get_workspace_class_fqn(rec, class, workspace_classes) {
                    fqn.clone()
                } else {
                    if class.package_name.is_empty() {
                        rec.clone()
                    } else {
                        format!("{}.{}", class.package_name, rec)
                    }
                }
            }
        };

        let mut arg_simple_vars = Vec::new();
        let mut arg_types = Vec::new();
        for arg in &args {
            let ty = resolve_expression_type(arg, class, workspace_classes, local_vars, fields_map);
            arg_types.push(ty);
            let arg_simple = resolve_to_simple_var(
                arg,
                caller_fqn,
                alloc_counter,
                conn,
                class,
                workspace_classes,
                local_vars,
                fields_map,
            )?;
            arg_simple_vars.push(arg_simple);
        }

        let static_callee = format!("{}.{}({})", receiver_type, method_name, arg_types.join(","));

        let call_id = format!("{}:call_{}", caller_fqn, alloc_counter);
        *alloc_counter += 1;

        let temp_lhs = format!("temp_call_lhs_{}", alloc_counter);
        *alloc_counter += 1;
        let temp_lhs_fqn = format!("{}#{}", caller_fqn, temp_lhs);

        conn.execute(
            "INSERT OR REPLACE INTO call_sites (call_id, method_fqn, receiver, method_name, lhs, static_callee) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            [
                Some(call_id.as_str()),
                Some(caller_fqn),
                resolved_receiver.as_deref(),
                Some(method_name.as_str()),
                Some(temp_lhs_fqn.as_str()),
                Some(static_callee.as_str()),
            ],
        )?;

        for (i, arg_simple) in arg_simple_vars.iter().enumerate() {
            let index_str = i.to_string();
            conn.execute(
                "INSERT OR REPLACE INTO call_arguments (call_id, arg_index, arg_var, arg_type) VALUES (?1, ?2, ?3, ?4)",
                [Some(call_id.as_str()), Some(index_str.as_str()), Some(arg_simple.as_str()), Some(arg_types[i].as_str())],
            )?;
        }

        handle_field_write(
            lhs_raw,
            &temp_lhs_fqn,
            caller_fqn,
            alloc_counter,
            conn,
            class,
            workspace_classes,
            local_vars,
            fields_map,
        )?;
    } else {
        let rhs_simple_var = resolve_to_simple_var(
            rhs_part,
            caller_fqn,
            alloc_counter,
            conn,
            class,
            workspace_classes,
            local_vars,
            fields_map,
        )?;
        handle_field_write(
            lhs_raw,
            &rhs_simple_var,
            caller_fqn,
            alloc_counter,
            conn,
            class,
            workspace_classes,
            local_vars,
            fields_map,
        )?;
    }
    Ok(())
}

struct FoundClass {
    name: String,
    kind: String,
    header: String,
    body_chars: Vec<char>,
    start_idx: usize,
    end_idx: usize,
}

fn is_word_at(chars: &[char], idx: usize, word: &str) -> bool {
    let word_chars: Vec<char> = word.chars().collect();
    if idx + word_chars.len() > chars.len() {
        return false;
    }
    for (i, &wc) in word_chars.iter().enumerate() {
        if chars[idx + i] != wc {
            return false;
        }
    }
    if idx > 0 {
        let prev = chars[idx - 1];
        if prev.is_alphanumeric() || prev == '_' || prev == '$' || prev == '.' {
            return false;
        }
    }
    if idx + word_chars.len() < chars.len() {
        let next = chars[idx + word_chars.len()];
        if next.is_alphanumeric() || next == '_' || next == '$' {
            return false;
        }
    }
    true
}

fn find_matching_brace(chars: &[char], start_brace_idx: usize) -> Option<usize> {
    let mut depth = 1;
    let mut in_string = false;
    let mut in_char = false;
    let mut in_escape = false;
    let mut i = start_brace_idx + 1;
    while i < chars.len() {
        let c = chars[i];
        if in_string {
            if in_escape {
                in_escape = false;
            } else if c == '\\' {
                in_escape = true;
            } else if c == '"' {
                in_string = false;
            }
        } else if in_char {
            if in_escape {
                in_escape = false;
            } else if c == '\\' {
                in_escape = true;
            } else if c == '\'' {
                in_char = false;
            }
        } else {
            if c == '"' {
                in_string = true;
                in_escape = false;
            } else if c == '\'' {
                in_char = true;
                in_escape = false;
            } else if c == '{' {
                depth += 1;
            } else if c == '}' {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
        }
        i += 1;
    }
    None
}

fn parse_class_name(header: &str, _keyword_len: usize) -> String {
    if let Some(kw_idx) = header.find("class") {
        let after = &header[kw_idx + 5..];
        let mut name = String::new();
        for c in after.chars() {
            if c.is_alphanumeric() || c == '_' || c == '$' {
                name.push(c);
            } else if c.is_whitespace() {
                if !name.is_empty() {
                    break;
                }
            } else {
                break;
            }
        }
        return name;
    }
    if let Some(kw_idx) = header.find("interface") {
        let after = &header[kw_idx + 9..];
        let mut name = String::new();
        for c in after.chars() {
            if c.is_alphanumeric() || c == '_' || c == '$' {
                name.push(c);
            } else if c.is_whitespace() {
                if !name.is_empty() {
                    break;
                }
            } else {
                break;
            }
        }
        return name;
    }
    String::new()
}

fn scan_direct_classes(chars: &[char]) -> Vec<FoundClass> {
    let mut found = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let is_class = is_word_at(chars, i, "class");
        let is_interface = is_word_at(chars, i, "interface");
        if is_class || is_interface {
            let mut brace_idx = None;
            for j in i..chars.len() {
                if chars[j] == '{' {
                    brace_idx = Some(j);
                    break;
                }
            }
            if let Some(b_idx) = brace_idx {
                if let Some(matching_brace_idx) = find_matching_brace(chars, b_idx) {
                    let header: String = chars[i..b_idx].iter().collect();
                    let kind = if is_interface {
                        "interface".to_string()
                    } else {
                        "class".to_string()
                    };
                    let name = parse_class_name(&header, if is_interface { 10 } else { 6 });
                    if !name.is_empty() {
                        let body_chars = chars[b_idx + 1..matching_brace_idx].to_vec();
                        found.push(FoundClass {
                            name,
                            kind,
                            header,
                            body_chars,
                            start_idx: i,
                            end_idx: matching_brace_idx,
                        });
                    }
                    i = matching_brace_idx + 1;
                    continue;
                }
            }
        }
        i += 1;
    }
    found
}

fn collect_classes_recursive(
    chars: &[char],
    prefix_fqn: &str,
    package_name: &str,
    imports: &[String],
    out_classes: &mut Vec<JavaClassInfo>,
) {
    let direct_classes = scan_direct_classes(chars);
    for dc in direct_classes {
        let class_name = if prefix_fqn.is_empty() {
            dc.name.clone()
        } else {
            format!("{}${}", prefix_fqn, dc.name)
        };

        let mut inner_classes = Vec::new();
        collect_classes_recursive(
            &dc.body_chars,
            &class_name,
            package_name,
            imports,
            &mut inner_classes,
        );

        let mut cleaned_body_chars = dc.body_chars.clone();
        let direct_inners = scan_direct_classes(&dc.body_chars);
        for inner in direct_inners {
            for idx in inner.start_idx..=inner.end_idx {
                cleaned_body_chars[idx] = ' ';
            }
        }

        let cleaned_body: String = cleaned_body_chars.into_iter().collect();
        let (fields, methods) = parse_class_body(package_name, imports, &class_name, &cleaned_body);
        let parents = parse_inheritance(&dc.header);
        let resolved_parents = parents
            .iter()
            .map(|parent| resolve_parent_fqn(parent, package_name, imports))
            .collect();

        let preceding_text = get_preceding_text(chars, dc.start_idx);
        let class_annotations = extract_annotations_from_string(&preceding_text);

        out_classes.push(JavaClassInfo {
            package_name: package_name.to_string(),
            imports: imports.to_vec(),
            class_name,
            fields,
            methods,
            parents: resolved_parents,
            is_interface: dc.kind == "interface",
            annotations: class_annotations,
        });

        out_classes.extend(inner_classes);
    }
}

fn resolve_expression_type(
    expr: &str,
    class: &JavaClassInfo,
    workspace_classes: &HashMap<String, String>,
    local_vars: &HashMap<String, String>,
    fields_map: &HashMap<String, String>,
) -> String {
    let expr = expr.trim();
    if expr.starts_with('"') {
        return "java.lang.String".to_string();
    }
    if expr.starts_with('\'') {
        return "char".to_string();
    }
    if expr.parse::<i64>().is_ok() {
        return "int".to_string();
    }
    if expr.parse::<f64>().is_ok() {
        return "double".to_string();
    }
    if expr == "true" || expr == "false" {
        return "boolean".to_string();
    }
    if expr.starts_with("new ") {
        let mut type_name = String::new();
        let after_new = expr["new ".len()..].trim();
        for c in after_new.chars() {
            if c.is_alphanumeric() || c == '_' || c == '<' || c == '>' || c == '[' || c == ']' {
                type_name.push(c);
            } else {
                break;
            }
        }
        let clean_type = strip_generics(&type_name);
        return resolve_type_fqn(&clean_type, class, workspace_classes);
    }

    let var_name = if expr.starts_with("this.") {
        &expr["this.".len()..]
    } else {
        expr
    };

    if let Some(ty) = local_vars.get(var_name) {
        return resolve_type_fqn(ty, class, workspace_classes);
    }
    if let Some(ty) = fields_map.get(var_name) {
        return resolve_type_fqn(ty, class, workspace_classes);
    }

    "java.lang.Object".to_string()
}

fn resolve_to_simple_var(
    expr: &str,
    caller_fqn: &str,
    alloc_counter: &mut usize,
    conn: &rusqlite::Connection,
    class: &JavaClassInfo,
    workspace_classes: &HashMap<String, String>,
    local_vars: &HashMap<String, String>,
    fields_map: &HashMap<String, String>,
) -> Result<String> {
    let expr = expr.trim();
    if !expr.contains('.') {
        if local_vars.contains_key(expr) {
            return Ok(format!("{}#{}", caller_fqn, expr));
        } else if fields_map.contains_key(expr) {
            let temp_var = format!("temp_field_{}", alloc_counter);
            *alloc_counter += 1;
            let temp_var_fqn = format!("{}#{}", caller_fqn, temp_var);
            let rhs_field = format!("{}#this.{}", caller_fqn, expr);
            conn.execute(
                "INSERT INTO source_assignments (lhs, rhs, assignment_type, method_fqn) VALUES (?1, ?2, 'FIELD_READ', ?3)",
                [temp_var_fqn.as_str(), rhs_field.as_str(), caller_fqn],
            )?;
            return Ok(temp_var_fqn);
        } else {
            return Ok(format!("{}#{}", caller_fqn, expr));
        }
    }

    let parts: Vec<&str> = expr.split('.').collect();
    let mut base_var = if parts[0] == "this" {
        format!("{}#this", caller_fqn)
    } else if local_vars.contains_key(parts[0]) {
        format!("{}#{}", caller_fqn, parts[0])
    } else if fields_map.contains_key(parts[0]) {
        let temp_var = format!("temp_field_{}", alloc_counter);
        *alloc_counter += 1;
        let temp_var_fqn = format!("{}#{}", caller_fqn, temp_var);
        let rhs_field = format!("{}#this.{}", caller_fqn, parts[0]);
        conn.execute(
            "INSERT INTO source_assignments (lhs, rhs, assignment_type, method_fqn) VALUES (?1, ?2, 'FIELD_READ', ?3)",
            [temp_var_fqn.as_str(), rhs_field.as_str(), caller_fqn],
        )?;
        temp_var_fqn
    } else {
        format!("{}#{}", caller_fqn, parts[0])
    };

    for &part in &parts[1..] {
        let temp_var = format!("temp_field_{}", alloc_counter);
        *alloc_counter += 1;
        let temp_var_fqn = format!("{}#{}", caller_fqn, temp_var);
        let rhs_field = format!("{}.{}", base_var, part);
        conn.execute(
            "INSERT INTO source_assignments (lhs, rhs, assignment_type, method_fqn) VALUES (?1, ?2, 'FIELD_READ', ?3)",
            [temp_var_fqn.as_str(), rhs_field.as_str(), caller_fqn],
        )?;
        base_var = temp_var_fqn;
    }

    Ok(base_var)
}

fn handle_field_write(
    lhs_expr: &str,
    rhs_simple_var: &str,
    caller_fqn: &str,
    alloc_counter: &mut usize,
    conn: &rusqlite::Connection,
    class: &JavaClassInfo,
    workspace_classes: &HashMap<String, String>,
    local_vars: &HashMap<String, String>,
    fields_map: &HashMap<String, String>,
) -> Result<()> {
    let lhs_expr = lhs_expr.trim();
    if !lhs_expr.contains('.') {
        if fields_map.contains_key(lhs_expr) {
            let lhs_field = format!("{}#this.{}", caller_fqn, lhs_expr);
            conn.execute(
                "INSERT INTO source_assignments (lhs, rhs, assignment_type, method_fqn) VALUES (?1, ?2, 'FIELD_WRITE', ?3)",
                [lhs_field.as_str(), rhs_simple_var, caller_fqn],
            )?;
        } else {
            let lhs_var = format!("{}#{}", caller_fqn, lhs_expr);
            conn.execute(
                "INSERT INTO source_assignments (lhs, rhs, assignment_type, method_fqn) VALUES (?1, ?2, 'COPY', ?3)",
                [lhs_var.as_str(), rhs_simple_var, caller_fqn],
            )?;
        }
        return Ok(());
    }

    if let Some(dot_idx) = lhs_expr.rfind('.') {
        let base_expr = &lhs_expr[..dot_idx];
        let field_name = &lhs_expr[dot_idx + 1..];

        let base_simple_var = resolve_to_simple_var(
            base_expr,
            caller_fqn,
            alloc_counter,
            conn,
            class,
            workspace_classes,
            local_vars,
            fields_map,
        )?;

        let lhs_field = format!("{}.{}", base_simple_var, field_name);
        conn.execute(
            "INSERT INTO source_assignments (lhs, rhs, assignment_type, method_fqn) VALUES (?1, ?2, 'FIELD_WRITE', ?3)",
            [lhs_field.as_str(), rhs_simple_var, caller_fqn],
        )?;
    }

    Ok(())
}

pub fn strip_signature(method_fqn: &str) -> &str {
    if let Some(idx) = method_fqn.find('(') {
        &method_fqn[..idx]
    } else {
        method_fqn
    }
}

fn extract_and_strip_annotations_at(chars: &[char], i: &mut usize) -> Vec<String> {
    let mut annotations = Vec::new();
    while *i < chars.len() {
        // Skip whitespace
        while *i < chars.len() && chars[*i].is_whitespace() {
            *i += 1;
        }
        if *i < chars.len() && chars[*i] == '@' {
            let start = *i;
            *i += 1;
            let mut name = String::new();
            while *i < chars.len()
                && (chars[*i].is_alphanumeric()
                    || chars[*i] == '_'
                    || chars[*i] == '$'
                    || chars[*i] == '.')
            {
                name.push(chars[*i]);
                *i += 1;
            }
            if !name.is_empty() {
                let mut temp = *i;
                while temp < chars.len() && chars[temp].is_whitespace() {
                    temp += 1;
                }
                if temp < chars.len() && chars[temp] == '(' {
                    let mut depth = 1;
                    temp += 1;
                    while temp < chars.len() && depth > 0 {
                        if chars[temp] == '(' {
                            depth += 1;
                        } else if chars[temp] == ')' {
                            depth -= 1;
                        }
                        temp += 1;
                    }
                    *i = temp;
                } else {
                    *i = temp;
                }
                let annotation_text: String = chars[start..*i].iter().collect();
                annotations.push(annotation_text.trim().to_string());
            } else {
                break;
            }
        } else {
            break;
        }
    }
    annotations
}

pub fn extract_annotations_from_string(s: &str) -> Vec<String> {
    let mut annotations = Vec::new();
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '@' {
            i += 1;
            let mut name = String::new();
            while i < chars.len()
                && (chars[i].is_alphanumeric()
                    || chars[i] == '_'
                    || chars[i] == '$'
                    || chars[i] == '.')
            {
                name.push(chars[i]);
                i += 1;
            }
            if !name.is_empty() {
                let mut temp = i;
                while temp < chars.len() && chars[temp].is_whitespace() {
                    temp += 1;
                }
                let mut args = String::new();
                if temp < chars.len() && chars[temp] == '(' {
                    let mut depth = 1;
                    temp += 1;
                    while temp < chars.len() && depth > 0 {
                        if chars[temp] == '(' {
                            depth += 1;
                        } else if chars[temp] == ')' {
                            depth -= 1;
                        }
                        if depth > 0 {
                            args.push(chars[temp]);
                        }
                        temp += 1;
                    }
                    i = temp;
                } else {
                    i = temp;
                }
                let short_name = if let Some(dot_idx) = name.rfind('.') {
                    name[dot_idx + 1..].to_string()
                } else {
                    name
                };
                annotations.push(short_name.clone());

                if (short_name == "Qualifier"
                    || short_name == "Service"
                    || short_name == "Component"
                    || short_name == "Repository"
                    || short_name == "Controller"
                    || short_name == "RestController"
                    || short_name == "Value")
                    && !args.is_empty()
                {
                    let mut val = args.trim().to_string();
                    if val.starts_with("value") {
                        if let Some(eq_idx) = val.find('=') {
                            val = val[eq_idx + 1..].trim().to_string();
                        }
                    }
                    if val.starts_with('"') && val.ends_with('"') {
                        val = val[1..val.len() - 1].to_string();
                    }
                    if !val.is_empty() {
                        annotations.push(format!("{}:{}", short_name, val));
                    }
                }
            }
        } else {
            i += 1;
        }
    }
    annotations
}

pub fn get_preceding_text(chars: &[char], start_idx: usize) -> String {
    let mut idx = start_idx;
    while idx > 0 {
        let c = chars[idx - 1];
        if c == ';' || c == '}' || c == '{' {
            break;
        }
        idx -= 1;
    }
    chars[idx..start_idx].iter().collect()
}

fn split_assignment(stmt: &str) -> Option<(String, String)> {
    let chars: Vec<char> = stmt.chars().collect();
    let mut paren_depth: i32 = 0;
    let mut bracket_depth: i32 = 0;
    let mut in_string = false;
    let mut in_char = false;
    let mut in_escape = false;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if in_string {
            if in_escape {
                in_escape = false;
            } else if c == '\\' {
                in_escape = true;
            } else if c == '"' {
                in_string = false;
            }
        } else if in_char {
            if in_escape {
                in_escape = false;
            } else if c == '\\' {
                in_escape = true;
            } else if c == '\'' {
                in_char = false;
            }
        } else {
            if c == '"' {
                in_string = true;
                in_escape = false;
            } else if c == '\'' {
                in_char = true;
                in_escape = false;
            } else if c == '(' {
                paren_depth += 1;
            } else if c == ')' {
                paren_depth = paren_depth.saturating_sub(1);
            } else if c == '[' {
                bracket_depth += 1;
            } else if c == ']' {
                bracket_depth = bracket_depth.saturating_sub(1);
            } else if c == '=' && paren_depth == 0 && bracket_depth == 0 {
                if i + 1 < chars.len() && chars[i + 1] == '=' {
                    i += 1;
                } else if i > 0
                    && (chars[i - 1] == '+'
                        || chars[i - 1] == '-'
                        || chars[i - 1] == '*'
                        || chars[i - 1] == '/')
                {
                    // ignore
                } else {
                    let lhs: String = chars[..i].iter().collect();
                    let rhs: String = chars[i + 1..].iter().collect();
                    return Some((lhs.trim().to_string(), rhs.trim().to_string()));
                }
            }
        }
        i += 1;
    }
    None
}

fn split_top_level_dots(expr: &str) -> Vec<String> {
    let chars: Vec<char> = expr.chars().collect();
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut paren_depth: i32 = 0;
    let mut bracket_depth: i32 = 0;
    let mut in_string = false;
    let mut in_char = false;
    let mut in_escape = false;

    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if in_string {
            current.push(c);
            if in_escape {
                in_escape = false;
            } else if c == '\\' {
                in_escape = true;
            } else if c == '"' {
                in_string = false;
            }
        } else if in_char {
            current.push(c);
            if in_escape {
                in_escape = false;
            } else if c == '\\' {
                in_escape = true;
            } else if c == '\'' {
                in_char = false;
            }
        } else {
            if c == '"' {
                in_string = true;
                in_escape = false;
                current.push(c);
            } else if c == '\'' {
                in_char = true;
                in_escape = false;
                current.push(c);
            } else if c == '(' {
                paren_depth += 1;
                current.push(c);
            } else if c == ')' {
                paren_depth = paren_depth.saturating_sub(1);
                current.push(c);
            } else if c == '[' {
                bracket_depth += 1;
                current.push(c);
            } else if c == ']' {
                bracket_depth = bracket_depth.saturating_sub(1);
                current.push(c);
            } else if c == '.' && paren_depth == 0 && bracket_depth == 0 {
                parts.push(current.trim().to_string());
                current.clear();
            } else {
                current.push(c);
            }
        }
        i += 1;
    }
    if !current.trim().is_empty() {
        parts.push(current.trim().to_string());
    }
    parts
}

fn rewrite_symbolic_allocations(
    expr: &str,
    caller_fqn: &str,
    alloc_counter: &mut usize,
    new_stmts: &mut Vec<String>,
    conn: &rusqlite::Connection,
    class: &JavaClassInfo,
    workspace_classes: &HashMap<String, String>,
    local_vars: &mut HashMap<String, String>,
) -> String {
    let chars: Vec<char> = expr.chars().collect();
    let mut rewritten = String::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '"' {
            let mut val_content = String::new();
            let mut temp_i = i + 1;
            let mut escaped = false;
            while temp_i < chars.len() {
                let c = chars[temp_i];
                if escaped {
                    val_content.push(c);
                    escaped = false;
                } else if c == '\\' {
                    val_content.push(c);
                    escaped = true;
                } else if c == '"' {
                    break;
                } else {
                    val_content.push(c);
                }
                temp_i += 1;
            }
            i = temp_i + 1;

            let alloc_id = format!("StringAlloc:{}", val_content);
            let temp_var = format!("temp_str_alloc_{}", *alloc_counter);
            *alloc_counter += 1;

            let _ = conn.execute(
                "INSERT OR REPLACE INTO allocation_sites (alloc_id, class_fqn, method_fqn) VALUES (?1, 'java.lang.String', ?2)",
                [&alloc_id, caller_fqn],
            );
            new_stmts.push(format!("{} = {}", temp_var, alloc_id));
            local_vars.insert(temp_var.clone(), "java.lang.String".to_string());
            rewritten.push_str(&temp_var);
        } else {
            let mut matched_class_literal = false;
            if chars[i].is_alphanumeric() || chars[i] == '_' || chars[i] == '$' {
                let mut ident = String::new();
                let mut temp_i = i;
                while temp_i < chars.len()
                    && (chars[temp_i].is_alphanumeric()
                        || chars[temp_i] == '_'
                        || chars[temp_i] == '$'
                        || chars[temp_i] == '.')
                {
                    ident.push(chars[temp_i]);
                    temp_i += 1;
                }
                if ident.ends_with(".class") {
                    let class_part = ident.strip_suffix(".class").unwrap();
                    let resolved_class_fqn = resolve_type_fqn(class_part, class, workspace_classes);

                    let alloc_id = format!("ReflectClassAlloc:{}", resolved_class_fqn);
                    let temp_var = format!("temp_class_alloc_{}", *alloc_counter);
                    *alloc_counter += 1;

                    let _ = conn.execute(
                        "INSERT OR REPLACE INTO allocation_sites (alloc_id, class_fqn, method_fqn) VALUES (?1, 'java.lang.Class', ?2)",
                        [&alloc_id, caller_fqn],
                    );
                    new_stmts.push(format!("{} = {}", temp_var, alloc_id));
                    local_vars.insert(temp_var.clone(), "java.lang.Class".to_string());
                    rewritten.push_str(&temp_var);
                    i = temp_i;
                    matched_class_literal = true;
                }
            }
            if !matched_class_literal {
                rewritten.push(chars[i]);
                i += 1;
            }
        }
    }
    rewritten
}

fn preprocess_statement(
    stmt: &str,
    caller_fqn: &str,
    alloc_counter: &mut usize,
    conn: &rusqlite::Connection,
    class: &JavaClassInfo,
    workspace_classes: &HashMap<String, String>,
    local_vars: &mut HashMap<String, String>,
) -> Vec<String> {
    let stmt_trimmed = stmt.trim();
    if stmt_trimmed.is_empty() {
        return Vec::new();
    }

    let mut new_stmts = Vec::new();

    let infer_type = |part: &str| -> String {
        let part_trimmed = part.trim();
        if part_trimmed.starts_with("forName(") {
            "java.lang.Class".to_string()
        } else if part_trimmed.starts_with("getDeclaredMethod(")
            || part_trimmed.starts_with("getMethod(")
        {
            "java.lang.reflect.Method".to_string()
        } else if part_trimmed.starts_with("getDeclaredField(")
            || part_trimmed.starts_with("getField(")
        {
            "java.lang.reflect.Field".to_string()
        } else if part_trimmed.starts_with("invoke(") {
            "java.lang.Object".to_string()
        } else {
            "java.lang.Object".to_string()
        }
    };

    if let Some((lhs, rhs)) = split_assignment(stmt_trimmed) {
        let rewritten_rhs = rewrite_symbolic_allocations(
            &rhs,
            caller_fqn,
            alloc_counter,
            &mut new_stmts,
            conn,
            class,
            workspace_classes,
            local_vars,
        );
        let parts = split_top_level_dots(&rewritten_rhs);
        if parts.len() > 1 && parts.iter().skip(1).any(|p| p.contains('(')) {
            let mut current_base = parts[0].clone();
            for i in 1..parts.len() {
                let part = &parts[i];
                let next_expr = format!("{}.{}", current_base, part);
                let temp_var = format!("temp_anorm_{}", *alloc_counter);
                *alloc_counter += 1;
                new_stmts.push(format!("{} = {}", temp_var, next_expr));
                let inf_ty = infer_type(part);
                local_vars.insert(temp_var.clone(), inf_ty);
                current_base = temp_var;
            }
            new_stmts.push(format!("{} = {}", lhs, current_base));
        } else {
            new_stmts.push(format!("{} = {}", lhs, rewritten_rhs));
        }
    } else if starts_with_keyword(stmt_trimmed, "return") {
        let expr = stmt_trimmed.strip_prefix("return").unwrap().trim();
        let rewritten_expr = rewrite_symbolic_allocations(
            expr,
            caller_fqn,
            alloc_counter,
            &mut new_stmts,
            conn,
            class,
            workspace_classes,
            local_vars,
        );
        let parts = split_top_level_dots(&rewritten_expr);
        if parts.len() > 1 && parts.iter().skip(1).any(|p| p.contains('(')) {
            let mut current_base = parts[0].clone();
            for i in 1..parts.len() {
                let part = &parts[i];
                let next_expr = format!("{}.{}", current_base, part);
                let temp_var = format!("temp_anorm_{}", *alloc_counter);
                *alloc_counter += 1;
                new_stmts.push(format!("{} = {}", temp_var, next_expr));
                let inf_ty = infer_type(part);
                local_vars.insert(temp_var.clone(), inf_ty);
                current_base = temp_var;
            }
            new_stmts.push(format!("return {}", current_base));
        } else {
            new_stmts.push(format!("return {}", rewritten_expr));
        }
    } else {
        let rewritten_expr = rewrite_symbolic_allocations(
            stmt_trimmed,
            caller_fqn,
            alloc_counter,
            &mut new_stmts,
            conn,
            class,
            workspace_classes,
            local_vars,
        );
        let parts = split_top_level_dots(&rewritten_expr);
        if parts.len() > 1 && parts.iter().skip(1).any(|p| p.contains('(')) {
            let mut current_base = parts[0].clone();
            for i in 1..parts.len() {
                let part = &parts[i];
                let next_expr = format!("{}.{}", current_base, part);
                let temp_var = format!("temp_anorm_{}", *alloc_counter);
                *alloc_counter += 1;
                new_stmts.push(format!("{} = {}", temp_var, next_expr));
                let inf_ty = infer_type(part);
                local_vars.insert(temp_var.clone(), inf_ty);
                current_base = temp_var;
            }
            new_stmts.push(current_base);
        } else {
            new_stmts.push(rewritten_expr);
        }
    }

    new_stmts
}

fn compute_file_hash(path: &Path) -> std::io::Result<String> {
    let content = std::fs::read(path)?;
    Ok(crate::jar::sha256_hash(&content))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_comments_and_literals() {
        let code = r#"
            // This is a comment
            String s = "hello";
            char c = 'a';
            /* block comment */
            String s2 = "nested \" quotes";
            char c2 = '\''; // escaped quote
        "#;
        let stripped = strip_comments(code);
        assert!(!stripped.contains("hello"));
        assert!(!stripped.contains("'a'"));
        assert!(!stripped.contains("block comment"));
        assert!(!stripped.contains("nested"));
        assert!(stripped.contains("String s = \"\";"));
        assert!(stripped.contains("char c = '';"));
        assert!(stripped.contains("String s2 = \"\";"));
        assert!(stripped.contains("char c2 = '';"));
    }

    #[test]
    fn test_extract_type_and_name() {
        // Simple case
        let (name, ty) = extract_type_and_name("int x").unwrap();
        assert_eq!(name, "x");
        assert_eq!(ty, "int");

        // Generic case
        let (name, ty) = extract_type_and_name("Map<String, List<Integer>> myMap").unwrap();
        assert_eq!(name, "myMap");
        assert_eq!(ty, "Map<String, List<Integer>>");

        // Array case
        let (name, ty) = extract_type_and_name("int[] arr").unwrap();
        assert_eq!(name, "arr");
        assert_eq!(ty, "int[]");

        // Modifier prefix case
        let (name, ty) = extract_type_and_name("public static final String name").unwrap();
        assert_eq!(name, "name");
        assert_eq!(ty, "String");

        // Annotations case
        let (name, ty) = extract_type_and_name("@Nullable List<String> list").unwrap();
        assert_eq!(name, "list");
        assert_eq!(ty, "List<String>");
    }

    #[test]
    fn test_is_filtered_keyword() {
        assert!(is_filtered_keyword("while"));
        assert!(is_filtered_keyword("try"));
        assert!(is_filtered_keyword("assert"));
        assert!(is_filtered_keyword("new"));
        assert!(is_filtered_keyword("new MyClass"));
        assert!(!is_filtered_keyword("myVariable"));
    }

    #[test]
    fn test_parse_package_and_imports() {
        let code = r#"
            package com.example;
            import java.util.List;
            import java.util.Map;
            public class MyClass<T> {
                private Map<String, List<Integer>> myMap;
            }
        "#;
        let (pkg, imports, name, kind, body, parents) = parse_package_and_imports(code);
        assert_eq!(pkg, "com.example");
        assert_eq!(imports, vec!["java.util.List", "java.util.Map"]);
        assert_eq!(name, "MyClass");
        assert_eq!(kind, "class");
        assert!(body.contains("private Map<String, List<Integer>> myMap;"));
        assert!(parents.is_empty());
    }

    #[test]
    fn test_is_primitive_type() {
        assert!(is_primitive_type("int"));
        assert!(is_primitive_type("boolean"));
        assert!(is_primitive_type("void"));
        assert!(!is_primitive_type("String"));
        assert!(!is_primitive_type("Map"));
    }

    #[test]
    fn test_primitive_variable_preservation() {
        let body = "int x = 5; public void method() {}";
        let vars = extract_local_variables(body);
        assert_eq!(vars.get("x"), Some(&"int".to_string()));
    }

    #[test]
    fn test_nested_generics_stress() {
        let (name, ty) =
            extract_type_and_name("Map<String, Map<Integer, List<String>>> myMap").unwrap();
        assert_eq!(name, "myMap");
        assert_eq!(ty, "Map<String, Map<Integer, List<String>>>");

        let (name2, ty2) = extract_type_and_name(
            "Map<List<Set<Map<String, Integer>>>, Map<String, String>> complexMap",
        )
        .unwrap();
        assert_eq!(name2, "complexMap");
        assert_eq!(
            ty2,
            "Map<List<Set<Map<String, Integer>>>, Map<String, String>>"
        );
    }

    #[test]
    fn test_string_literals_complex_structures() {
        let code = r#"
            String url = "http://example.com/api"; // some comment
            String sql = "SELECT * FROM users WHERE email = 'a@b.com' -- sql comment";
            String javaCode = "class FakeClass { int x; }";
        "#;
        let stripped = strip_comments(code);
        assert!(stripped.contains(r#"String url = "";"#));
        assert!(stripped.contains(r#"String sql = "";"#));
        assert!(stripped.contains(r#"String javaCode = "";"#));

        let (fields, methods) = parse_class_body("", &[], "TestClass", &stripped);
        let field_names: Vec<String> = fields.iter().map(|f| f.0.clone()).collect();
        assert!(field_names.contains(&"url".to_string()));
        assert!(field_names.contains(&"sql".to_string()));
        assert!(field_names.contains(&"javaCode".to_string()));
        assert_eq!(methods.len(), 0);
    }

    #[test]
    fn test_else_block_variable_bug() {
        let body = r#"
            if (cond) {
                int x = 1;
            } else {
                y = 5;
            }
        "#;
        let vars = extract_local_variables(body);
        assert!(vars.get("y").is_none());
        assert_eq!(vars.get("x"), Some(&"int".to_string()));
    }

    #[test]
    fn test_parameter_annotation_comma_bug() {
        let header = "public void myMethod(@Annotation(a=1, b=2) String myParam)";
        let vars = extract_parameters(header);
        assert!(!vars.contains_key("1"));
        assert_eq!(vars.get("myParam"), Some(&"String".to_string()));
    }

    #[test]
    fn test_extract_parameter_types_and_names() {
        let header = "public void myMethod(@Annotation(a=1, b=2) String myParam, int x, Map<String, Integer> map)";
        let params = extract_parameter_types_and_names(header);
        assert_eq!(params.len(), 3);
        assert_eq!(params[0], ("String".to_string(), "myParam".to_string()));
        assert_eq!(params[1], ("int".to_string(), "x".to_string()));
        assert_eq!(
            params[2],
            ("Map<String, Integer>".to_string(), "map".to_string())
        );
    }
}
