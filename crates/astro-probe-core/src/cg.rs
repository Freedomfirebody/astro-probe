use std::collections::{HashMap, HashSet};
use rusqlite::Connection;

#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("Database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
}

pub struct PointsToSolver;

impl PointsToSolver {
    pub fn new() -> Self {
        Self
    }

    pub fn solve(&self, conn: &Connection) -> Result<(), CoreError> {
        // Step 1: Initialize local points-to sets from direct allocation assignments
        conn.execute("DELETE FROM points_to_sets", [])?;

        let mut pts: HashMap<String, HashSet<String>> = HashMap::new();

        // Load direct allocations: lhs = ALLOC(rhs)
        let mut alloc_stmt = conn.prepare(
            "SELECT lhs, rhs FROM source_assignments WHERE assignment_type = 'ALLOC'"
        )?;
        let mut alloc_rows = alloc_stmt.query([])?;
        while let Some(row) = alloc_rows.next()? {
            let lhs: String = row.get(0)?;
            let rhs: String = row.get(1)?;
            pts.entry(lhs).or_default().insert(rhs);
        }
        drop(alloc_rows);
        drop(alloc_stmt);

        // Load copy assignments: lhs = COPY(rhs)
        let mut copy_stmt = conn.prepare(
            "SELECT lhs, rhs FROM source_assignments WHERE assignment_type = 'COPY'"
        )?;
        let mut copy_rows = copy_stmt.query([])?;
        let mut copies = Vec::new();
        while let Some(row) = copy_rows.next()? {
            let lhs: String = row.get(0)?;
            let rhs: String = row.get(1)?;
            copies.push((lhs, rhs));
        }
        drop(copy_rows);
        drop(copy_stmt);

        // Load field read assignments: lhs = FIELD_READ(rhs.field) -> represented as lhs = rhs.field
        let mut read_stmt = conn.prepare(
            "SELECT lhs, rhs FROM source_assignments WHERE assignment_type = 'FIELD_READ'"
        )?;
        let mut read_rows = read_stmt.query([])?;
        let mut field_reads = Vec::new();
        while let Some(row) = read_rows.next()? {
            let lhs: String = row.get(0)?;
            let rhs: String = row.get(1)?; // format: base#field
            field_reads.push((lhs, rhs));
        }
        drop(read_rows);
        drop(read_stmt);

        // Load field write assignments: lhs.field = FIELD_WRITE(rhs) -> represented as lhs.field = rhs
        let mut write_stmt = conn.prepare(
            "SELECT lhs, rhs FROM source_assignments WHERE assignment_type = 'FIELD_WRITE'"
        )?;
        let mut write_rows = write_stmt.query([])?;
        let mut field_writes = Vec::new();
        while let Some(row) = write_rows.next()? {
            let lhs: String = row.get(0)?; // format: base#field
            let rhs: String = row.get(1)?;
            field_writes.push((lhs, rhs));
        }
        drop(write_rows);
        drop(write_stmt);

        // Load class hierarchy for resolving virtual calls
        let mut hier_stmt = conn.prepare("SELECT class_fqn, parent_fqn FROM class_hierarchy")?;
        let mut hier_rows = hier_stmt.query([])?;
        let mut parent_map: HashMap<String, Vec<String>> = HashMap::new();
        while let Some(row) = hier_rows.next()? {
            let child: String = row.get(0)?;
            let parent: String = row.get(1)?;
            parent_map.entry(child).or_default().push(parent);
        }
        drop(hier_rows);
        drop(hier_stmt);

        // Helper to check subtype relation: resolves if sub is a subclass or implementer of sup (transitive)
        let is_subtype = |sub: &str, sup: &str| -> bool {
            if sub == sup {
                return true;
            }
            let mut visited = HashSet::new();
            let mut queue = vec![sub.to_string()];
            while let Some(curr) = queue.pop() {
                if curr == sup {
                    return true;
                }
                if visited.insert(curr.clone()) {
                    if let Some(parents) = parent_map.get(&curr) {
                        for p in parents {
                            if !visited.contains(p) {
                                queue.push(p.clone());
                            }
                        }
                    }
                }
            }
            false
        };

        // Cache of resolved target methods to speed up virtual calls
        // Map: (ReceiverType, MethodName, ParamsSignature) -> TargetMethodFQN
        let mut resolution_cache: HashMap<(String, String, String), Option<String>> = HashMap::new();

        // Load all declared methods for dynamic dispatch resolution
        let mut decl_stmt = conn.prepare("SELECT method_fqn, class_fqn, method_name, params FROM method_declarations")?;
        let mut decl_rows = decl_stmt.query([])?;
        let mut declarations = Vec::new();
        while let Some(row) = decl_rows.next()? {
            let method_fqn: String = row.get(0)?;
            let class_fqn: String = row.get(1)?;
            let method_name: String = row.get(2)?;
            let params: String = row.get(3)?;
            declarations.push((method_fqn, class_fqn, method_name, params));
        }
        drop(decl_rows);
        drop(decl_stmt);

        let mut resolve_virtual_call = |rec_type: &str, method_name: &str, params_sig: &str| -> Option<String> {
            let key = (rec_type.to_string(), method_name.to_string(), params_sig.to_string());
            if let Some(cached) = resolution_cache.get(&key) {
                return cached.clone();
            }

            // We must find the most specific method implementation in rec_type or its superclasses
            let mut best_target = None;
            let mut best_distance = usize::MAX;

            for (m_fqn, class_fqn, m_name, m_params) in &declarations {
                if m_name == method_name && m_params == params_sig {
                    // Check if class_fqn is a supertype of rec_type
                    if is_subtype(rec_type, class_fqn) {
                        // Find distance in hierarchy
                        let mut dist = 0;
                        let mut found = false;
                        let mut visited = HashSet::new();
                        let mut queue = vec![(rec_type.to_string(), 0)];
                        while !queue.is_empty() {
                            let (curr, d) = queue.remove(0);
                            if &curr == class_fqn {
                                dist = d;
                                found = true;
                                break;
                            }
                            if visited.insert(curr.clone()) {
                                if let Some(parents) = parent_map.get(&curr) {
                                    for p in parents {
                                        queue.push((p.clone(), d + 1));
                                    }
                                }
                            }
                        }

                        if found && dist < best_distance {
                            best_distance = dist;
                            best_target = Some(m_fqn.clone());
                        }
                    }
                }
            }

            resolution_cache.insert(key, best_target.clone());
            best_target
        };

        // Cache call sites
        let mut cs_stmt = conn.prepare(
            "SELECT call_id, method_fqn, receiver, method_name, lhs, static_callee FROM call_sites"
        )?;
        let mut cs_rows = cs_stmt.query([])?;
        let mut call_sites = Vec::new();
        while let Some(row) = cs_rows.next()? {
            let call_id: String = row.get(0)?;
            let caller_method_fqn: String = row.get(1)?;
            let receiver: Option<String> = row.get(2)?;
            let method_name: String = row.get(3)?;
            let lhs: Option<String> = row.get(4)?;
            let static_callee: Option<String> = row.get(5)?;
            call_sites.push((call_id, caller_method_fqn, receiver, method_name, lhs, static_callee));
        }
        drop(cs_rows);
        drop(cs_stmt);

        // Cache call arguments
        let mut args_stmt = conn.prepare(
            "SELECT call_id, arg_index, arg_var, arg_type FROM call_arguments"
        )?;
        let mut args_rows = args_stmt.query([])?;
        let mut call_args: HashMap<String, Vec<(usize, String, Option<String>)>> = HashMap::new();
        while let Some(row) = args_rows.next()? {
            let call_id: String = row.get(0)?;
            let idx: usize = row.get(1)?;
            let arg_var: String = row.get(2)?;
            let arg_type: Option<String> = row.get(3)?;
            call_args.entry(call_id).or_default().push((idx, arg_var, arg_type));
        }
        drop(args_rows);
        drop(args_stmt);

        // Allocation-type mappings: AllocID -> AllocType (i.e. Class FQN)
        let mut type_stmt = conn.prepare("SELECT alloc_id, class_fqn FROM allocation_sites")?;
        let mut type_rows = type_stmt.query([])?;
        let mut alloc_types = HashMap::new();
        while let Some(row) = type_rows.next()? {
            let aid: String = row.get(0)?;
            let c_fqn: String = row.get(1)?;
            alloc_types.insert(aid, c_fqn);
        }
        drop(type_rows);
        drop(type_stmt);

        // Instance field points-to sets: Map: (AllocID, FieldName) -> PointsToSet
        let mut instance_fields: HashMap<(String, String), HashSet<String>> = HashMap::new();

        // Track Call Edges to build call graph. Unique tuples of (caller, callee, is_virtual)
        let mut call_edges_discovered: HashSet<(String, String, bool)> = HashSet::new();

        // Fixed-point iteration loop
        let mut changed = true;
        let mut iterations = 0;
        while changed {
            changed = false;
            iterations += 1;
            if iterations > 1000 {
                // Safety bound to avoid infinite loop on unexpected structures
                break;
            }

            // 1. Process copy assignments: lhs = COPY(rhs)
            for (lhs, rhs) in &copies {
                let rhs_vals = if let Some(rhs_set) = pts.get(rhs) {
                    Some(rhs_set.clone())
                } else {
                    None
                };
                if let Some(rhs_clone) = rhs_vals {
                    let lhs_set = pts.entry(lhs.clone()).or_default();
                    for val in rhs_clone {
                        if lhs_set.insert(val) {
                            changed = true;
                        }
                    }
                }
            }

            // 2. Process field writes: base_var#field = rhs_var
            // In Andersen's: for each allocation 'a' in base_var's pts, copy rhs_var's pts to (a, field)
            for (lhs_field_expr, rhs) in &field_writes {
                // lhs_field_expr format is base#field
                if let Some(hash_idx) = lhs_field_expr.find('#') {
                    let base_var = &lhs_field_expr[..hash_idx];
                    let field = &lhs_field_expr[hash_idx + 1..];

                    if let Some(base_pts) = pts.get(base_var) {
                        let base_pts_clone = base_pts.clone();
                        if let Some(rhs_set) = pts.get(rhs) {
                            let rhs_clone = rhs_set.clone();
                            for alloc in base_pts_clone {
                                let field_set = instance_fields.entry((alloc, field.to_string())).or_default();
                                for val in &rhs_clone {
                                    if field_set.insert(val.clone()) {
                                        changed = true;
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // 3. Process field reads: lhs = base_var#field
            // In Andersen's: for each allocation 'a' in base_var's pts, copy (a, field)'s pts to lhs
            for (lhs, rhs_field_expr) in &field_reads {
                if let Some(hash_idx) = rhs_field_expr.find('#') {
                    let base_var = &rhs_field_expr[..hash_idx];
                    let field = &rhs_field_expr[hash_idx + 1..];

                    let base_pts_clone = if let Some(base_pts) = pts.get(base_var) {
                        Some(base_pts.clone())
                    } else {
                        None
                    };
                    if let Some(base_pts) = base_pts_clone {
                        for alloc in base_pts {
                            if let Some(field_set) = instance_fields.get(&(alloc, field.to_string())) {
                                let field_clone = field_set.clone();
                                let lhs_set = pts.entry(lhs.clone()).or_default();
                                for val in field_clone {
                                    if lhs_set.insert(val) {
                                        changed = true;
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // 4. Process call sites and dynamic dispatch propagation
            for (call_id, caller_method_fqn, receiver, method_name, lhs, static_callee) in &call_sites {
                // If static callee is known, propagate to it directly
                // Otherwise, resolve dynamically based on receiver points-to set
                let mut resolved_targets = HashSet::new();

                let rec_pts_clone = if let Some(ref rec_var) = receiver {
                    if let Some(rec_pts) = pts.get(rec_var) {
                        Some(rec_pts.clone())
                    } else {
                        None
                    }
                } else {
                    None
                };

                if let Some(rec_pts) = rec_pts_clone {
                    for alloc in rec_pts {
                        // Find type of this allocation
                        if let Some(alloc_type) = alloc_types.get(&alloc) {
                            // Extract parameter signature from static_callee if possible
                            let params_sig = if let Some(ref sc) = static_callee {
                                if let Some(start) = sc.find('(') {
                                    if let Some(end) = sc.rfind(')') {
                                        sc[start + 1..end].to_string()
                                    } else {
                                        String::new()
                                    }
                                } else {
                                    String::new()
                                }
                            } else {
                                String::new()
                            };

                            if let Some(target) = resolve_virtual_call(alloc_type, method_name, &params_sig) {
                                resolved_targets.insert((target, true));
                            }
                        }
                    }
                } else if let Some(ref sc) = static_callee {
                    resolved_targets.insert((sc.clone(), false));
                }

                // Reflection propagation: handle Class.forName, getMethod, invoke
                // If caller_method_fqn calls Class.forName(className)
                if method_name == "forName" && static_callee.as_deref() == Some("java.lang.Class.forName(java.lang.String)") {
                    if let Some(args) = call_args.get(call_id) {
                        if let Some((_, arg_var, _)) = args.iter().find(|(idx, _, _)| *idx == 0) {
                            let arg_pts_clone = if let Some(arg_pts) = pts.get(arg_var) {
                                Some(arg_pts.clone())
                            } else {
                                None
                            };
                            if let Some(arg_pts) = arg_pts_clone {
                                // Propagate ClassAlloc for matched classes
                                for alloc in arg_pts {
                                    if alloc.starts_with("StringAlloc:") {
                                        let class_name = &alloc["StringAlloc:".len()..];
                                        let class_alloc = format!("ClassAlloc:{}", class_name);
                                        if let Some(ref return_var) = lhs {
                                            let lhs_set = pts.entry(return_var.clone()).or_default();
                                            if lhs_set.insert(class_alloc) {
                                                changed = true;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                // If caller_method_fqn calls getMethod(methodName, ...)
                if (method_name == "getMethod" || method_name == "getDeclaredMethod")
                    && (static_callee.as_deref() == Some("java.lang.Class.getMethod(java.lang.String,java.lang.Class[])")
                        || static_callee.as_deref() == Some("java.lang.Class.getDeclaredMethod(java.lang.String,java.lang.Class[])"))
                {
                    let mut method_allocs = Vec::new();
                    if let Some(ref rec_var) = receiver {
                        if let Some(rec_pts) = pts.get(rec_var) {
                            for class_alloc in rec_pts {
                                if class_alloc.starts_with("ClassAlloc:") {
                                    let class_name = &class_alloc["ClassAlloc:".len()..];
                                    if let Some(args) = call_args.get(call_id) {
                                        if let Some((_, name_var, _)) = args.iter().find(|(idx, _, _)| *idx == 0) {
                                            if let Some(name_pts) = pts.get(name_var) {
                                                for name_alloc in name_pts {
                                                    if name_alloc.starts_with("StringAlloc:") {
                                                        let m_name = &name_alloc["StringAlloc:".len()..];
                                                        method_allocs.push(format!("MethodAlloc:{}#{}", class_name, m_name));
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    for method_alloc in method_allocs {
                        if let Some(ref return_var) = lhs {
                            let lhs_set = pts.entry(return_var.clone()).or_default();
                            if lhs_set.insert(method_alloc) {
                                changed = true;
                            }
                        }
                    }
                }

                // If caller_method_fqn calls Method.invoke(receiver, args...)
                if method_name == "invoke" && static_callee.as_deref() == Some("java.lang.reflect.Method.invoke(java.lang.Object,java.lang.Object[])") {
                    let mut method_targets = Vec::new();
                    if let Some(ref rec_var) = receiver {
                        if let Some(rec_pts) = pts.get(rec_var) {
                            for method_alloc in rec_pts {
                                if method_alloc.starts_with("MethodAlloc:") {
                                    // format: MethodAlloc:ClassName#MethodName
                                    let parts = &method_alloc["MethodAlloc:".len()..];
                                    if let Some(hash_idx) = parts.find('#') {
                                        let class_name = &parts[..hash_idx];
                                        let m_name = &parts[hash_idx + 1..];
                                        method_targets.push((class_name.to_string(), m_name.to_string()));
                                    }
                                }
                            }
                        }
                    }
                    for (class_name, m_name) in method_targets {
                        // Find all matching method declarations
                        for (m_fqn, class_fqn, decl_m_name, _) in &declarations {
                            if decl_m_name == &m_name && is_subtype(&class_name, class_fqn) {
                                resolved_targets.insert((m_fqn.clone(), true));
                            }
                        }
                    }
                }

                // Propagate parameters and return values for all resolved targets
                for (target, is_virt) in resolved_targets {
                    let caller_stripped = strip_signature(caller_method_fqn).to_string();
                    let target_stripped = strip_signature(&target).to_string();
                    call_edges_discovered.insert((caller_stripped, target_stripped, is_virt));

                    let mut propagations = Vec::new();
                    if let Some(args) = call_args.get(call_id) {
                        for (idx, arg_var, _) in args {
                            if let Some(arg_pts) = pts.get(arg_var) {
                                propagations.push((format!("{}#p{}", target, idx), arg_pts.clone()));
                            }
                        }
                    }
                    if let Some(ref return_var) = lhs {
                        let target_return = format!("{}#return", target);
                        if let Some(ret_pts) = pts.get(&target_return) {
                            propagations.push((return_var.clone(), ret_pts.clone()));
                        }
                    }

                    for (to_var, vals) in propagations {
                        let param_set = pts.entry(to_var).or_default();
                        for val in vals {
                            if param_set.insert(val) {
                                changed = true;
                            }
                        }
                    }
                }
            }
        }

        // Persist points-to sets back to database
        let mut insert_pts = conn.prepare(
            "INSERT OR REPLACE INTO points_to_sets (variable_fqn, alloc_id) VALUES (?1, ?2)"
        )?;
        for (var, allocs) in &pts {
            for alloc in allocs {
                insert_pts.execute([var, alloc])?;
            }
        }
        drop(insert_pts);

        // Clear existing call edges and persist newly resolved call edges
        conn.execute("DELETE FROM call_edges", [])?;
        let mut insert_edge = conn.prepare(
            "INSERT OR IGNORE INTO call_edges (caller, callee, is_virtual) VALUES (?1, ?2, ?3)"
        )?;
        for (caller, callee, is_virt) in call_edges_discovered {
            let is_virt_int = if is_virt { 1 } else { 0 };
            insert_edge.execute([&caller, &callee, &is_virt_int.to_string()])?;
        }
        drop(insert_edge);

        Ok(())
    }
}

fn strip_signature(method_fqn: &str) -> &str {
    if let Some(idx) = method_fqn.find('(') {
        &method_fqn[..idx]
    } else {
        method_fqn
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use astro_probe_db::init_db;

    #[test]
    fn test_andersen_points_to_analysis() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        init_db(&conn).unwrap();

        // Setup class structure
        conn.execute("INSERT INTO classes (fqn, kind) VALUES ('com.test.Base', 'class')", []).unwrap();
        conn.execute("INSERT INTO classes (fqn, kind) VALUES ('com.test.SubA', 'class')", []).unwrap();
        conn.execute("INSERT INTO classes (fqn, kind) VALUES ('com.test.SubB', 'class')", []).unwrap();
        
        conn.execute("INSERT INTO class_hierarchy (class_fqn, parent_fqn) VALUES ('com.test.SubA', 'com.test.Base')", []).unwrap();
        conn.execute("INSERT INTO class_hierarchy (class_fqn, parent_fqn) VALUES ('com.test.SubB', 'com.test.Base')", []).unwrap();

        // Setup method declarations
        conn.execute(
            "INSERT INTO method_declarations (method_fqn, class_fqn, method_name, params) \
             VALUES ('com.test.Base.foo()', 'com.test.Base', 'foo', '')",
            []
        ).unwrap();
        conn.execute(
            "INSERT INTO method_declarations (method_fqn, class_fqn, method_name, params) \
             VALUES ('com.test.SubA.foo()', 'com.test.SubA', 'foo', '')",
            []
        ).unwrap();
        conn.execute(
            "INSERT INTO method_declarations (method_fqn, class_fqn, method_name, params) \
             VALUES ('com.test.SubB.foo()', 'com.test.SubB', 'foo', '')",
            []
        ).unwrap();
        conn.execute(
            "INSERT INTO method_declarations (method_fqn, class_fqn, method_name, params) \
             VALUES ('com.test.Main.run()', 'com.test.Main', 'run', '')",
            []
        ).unwrap();

        // Allocations in Main.run()
        conn.execute(
            "INSERT INTO allocation_sites (alloc_id, class_fqn, method_fqn) \
             VALUES ('AllocSubA', 'com.test.SubA', 'com.test.Main.run()')",
            []
        ).unwrap();
        conn.execute(
            "INSERT INTO allocation_sites (alloc_id, class_fqn, method_fqn) \
             VALUES ('AllocSubB', 'com.test.SubB', 'com.test.Main.run()')",
            []
        ).unwrap();

        // source_assignments representing:
        // x = new SubA();  -> com.test.Main.run()#x = ALLOC(AllocSubA)
        // y = new SubB();  -> com.test.Main.run()#y = ALLOC(AllocSubB)
        // z = x;           -> com.test.Main.run()#z = COPY(com.test.Main.run()#x)
        conn.execute(
            "INSERT INTO source_assignments (lhs, rhs, assignment_type, method_fqn) \
             VALUES ('com.test.Main.run()#x', 'AllocSubA', 'ALLOC', 'com.test.Main.run()')",
            []
        ).unwrap();
        conn.execute(
            "INSERT INTO source_assignments (lhs, rhs, assignment_type, method_fqn) \
             VALUES ('com.test.Main.run()#y', 'AllocSubB', 'ALLOC', 'com.test.Main.run()')",
            []
        ).unwrap();
        conn.execute(
            "INSERT INTO source_assignments (lhs, rhs, assignment_type, method_fqn) \
             VALUES ('com.test.Main.run()#z', 'com.test.Main.run()#x', 'COPY', 'com.test.Main.run()')",
            []
        ).unwrap();

        // Call site representing: z.foo()
        conn.execute(
            "INSERT INTO call_sites (call_id, method_fqn, receiver, method_name, lhs, static_callee) \
             VALUES ('call_1', 'com.test.Main.run()', 'com.test.Main.run()#z', 'foo', NULL, 'com.test.Base.foo()')",
            []
        ).unwrap();

        let solver = PointsToSolver::new();
        solver.solve(&conn).unwrap();

        // Assert points-to set of z contains AllocSubA
        let count_z_alloc_sub_a: i64 = conn.query_row(
            "SELECT count(*) FROM points_to_sets WHERE variable_fqn = 'com.test.Main.run()#z' AND alloc_id = 'AllocSubA'",
            [],
            |r| r.get(0)
        ).unwrap();
        assert_eq!(count_z_alloc_sub_a, 1);

        // Assert points-to set of z does NOT contain AllocSubB
        let count_z_alloc_sub_b: i64 = conn.query_row(
            "SELECT count(*) FROM points_to_sets WHERE variable_fqn = 'com.test.Main.run()#z' AND alloc_id = 'AllocSubB'",
            [],
            |r| r.get(0)
        ).unwrap();
        assert_eq!(count_z_alloc_sub_b, 0);

        // Assert call edge com.test.Main.run -> com.test.SubA.foo exists, and is virtual
        let is_virt: i64 = conn.query_row(
            "SELECT is_virtual FROM call_edges WHERE caller = 'com.test.Main.run' AND callee = 'com.test.SubA.foo'",
            [],
            |r| r.get(0)
        ).unwrap();
        assert_eq!(is_virt, 1);
    }
}
