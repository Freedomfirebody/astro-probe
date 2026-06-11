use std::collections::{HashMap, HashSet};
use rusqlite::Connection;
use anyhow::Result;

pub struct DfgAnalyzer;

impl DfgAnalyzer {
    pub fn new() -> Self {
        Self
    }

    pub fn analyze(&self, conn: &Connection) -> Result<()> {
        // Clear old lineage edges
        conn.execute("DELETE FROM lineage_edges", [])?;

        // 1. Local Copy Assignments & ALLOC Assignments
        // For every record in source_assignments:
        // If type is 'COPY' or 'ALLOC':
        // from_node = rhs, to_node = lhs, edge_type = 'data'
        let mut stmt = conn.prepare("SELECT lhs, rhs, assignment_type FROM source_assignments")?;
        let mut rows = stmt.query([])?;
        let mut lineage_inserts = Vec::new();

        while let Some(row) = rows.next()? {
            let lhs: String = row.get(0)?;
            let rhs: String = row.get(1)?;
            let assignment_type: String = row.get(2)?;
            if assignment_type == "COPY" || assignment_type == "ALLOC" {
                lineage_inserts.push((rhs, lhs, "data".to_string()));
            }
        }

        // 2. Load facts for points-to and resolution
        // Load hierarchy
        let mut stmt = conn.prepare("SELECT class_fqn, parent_fqn FROM class_hierarchy")?;
        let mut h_rows = stmt.query([])?;
        let mut parent_map: HashMap<String, Vec<String>> = HashMap::new();
        while let Some(row) = h_rows.next()? {
            let child: String = row.get(0)?;
            let parent: String = row.get(1)?;
            parent_map.entry(child).or_default().push(parent);
        }

        // Load method declarations: method_fqn -> (class_fqn, method_name, params_list)
        let mut stmt = conn.prepare("SELECT method_fqn, class_fqn, method_name, params FROM method_declarations")?;
        let mut m_rows = stmt.query([])?;
        let mut method_decls = HashMap::new();
        while let Some(row) = m_rows.next()? {
            let method_fqn: String = row.get(0)?;
            let class_fqn: String = row.get(1)?;
            let method_name: String = row.get(2)?;
            let params_str: String = row.get(3)?;
            let params = if params_str.is_empty() {
                Vec::new()
            } else {
                params_str.split(',').map(|s| s.trim().to_string()).collect()
            };
            method_decls.insert(method_fqn, (class_fqn, method_name, params));
        }

        // Load points-to sets: var -> set of alloc_id
        let mut stmt = conn.prepare("SELECT variable_fqn, alloc_id FROM points_to_sets")?;
        let mut pts_rows = stmt.query([])?;
        let mut pts: HashMap<String, HashSet<String>> = HashMap::new();
        while let Some(row) = pts_rows.next()? {
            let var: String = row.get(0)?;
            let alloc_id: String = row.get(1)?;
            pts.entry(var).or_default().insert(alloc_id);
        }

        // Load allocation types: alloc_id -> class_fqn
        let mut stmt = conn.prepare("SELECT alloc_id, class_fqn FROM allocation_sites")?;
        let mut alloc_rows = stmt.query([])?;
        let mut alloc_types = HashMap::new();
        while let Some(row) = alloc_rows.next()? {
            let alloc_id: String = row.get(0)?;
            let class_fqn: String = row.get(1)?;
            alloc_types.insert(alloc_id, class_fqn);
        }

        // Load field writes and reads
        let mut stmt = conn.prepare("SELECT lhs, rhs, assignment_type FROM source_assignments")?;
        let mut assign_rows = stmt.query([])?;
        while let Some(row) = assign_rows.next()? {
            let lhs: String = row.get(0)?;
            let rhs: String = row.get(1)?;
            let assignment_type: String = row.get(2)?;

            if assignment_type == "FIELD_WRITE" {
                if let Some(dot_idx) = lhs.rfind('.') {
                    let base_var = &lhs[..dot_idx];
                    let field_name = &lhs[dot_idx + 1..];
                    if let Some(base_pts) = pts.get(base_var) {
                        for o_i in base_pts {
                            let field_node = format!("{}.{}", o_i, field_name);
                            lineage_inserts.push((rhs.clone(), field_node, "data".to_string()));
                        }
                    }
                }
            } else if assignment_type == "FIELD_READ" {
                if let Some(dot_idx) = rhs.rfind('.') {
                    let base_var = &rhs[..dot_idx];
                    let field_name = &rhs[dot_idx + 1..];
                    if let Some(base_pts) = pts.get(base_var) {
                        for o_i in base_pts {
                            let field_node = format!("{}.{}", o_i, field_name);
                            lineage_inserts.push((field_node, lhs.clone(), "data".to_string()));
                        }
                    }
                }
            }
        }

        // Helper to resolve virtual method dispatch on a class
        let resolve_method = |class_fqn: &str, method_name: &str, arg_count: usize| -> Option<String> {
            let mut visited = HashSet::new();
            let mut queue = vec![class_fqn.to_string()];
            while let Some(curr) = queue.pop() {
                if !visited.insert(curr.clone()) {
                    continue;
                }
                for (fqn, (decl_class, decl_name, params)) in &method_decls {
                    if decl_class == &curr && decl_name == method_name && params.len() == arg_count {
                        return Some(fqn.clone());
                    }
                }
                if let Some(parents) = parent_map.get(&curr) {
                    for p in parents {
                        queue.push(p.clone());
                    }
                }
            }
            None
        };

        // 3. Inter-procedural Call Mappings
        let mut stmt = conn.prepare("SELECT call_id, method_fqn, receiver, method_name, lhs, static_callee FROM call_sites")?;
        let mut call_rows = stmt.query([])?;
        let mut call_sites = Vec::new();
        while let Some(row) = call_rows.next()? {
            let call_id: String = row.get(0)?;
            let caller_method: String = row.get(1)?;
            let receiver: Option<String> = row.get(2)?;
            let method_name: String = row.get(3)?;
            let lhs: Option<String> = row.get(4)?;
            let static_callee: Option<String> = row.get(5)?;
            call_sites.push((call_id, caller_method, receiver, method_name, lhs, static_callee));
        }

        for (call_id, _caller_method, receiver, method_name, lhs, static_callee) in call_sites {
            let mut arg_stmt = conn.prepare("SELECT arg_index, arg_var FROM call_arguments WHERE call_id = ?1 ORDER BY arg_index")?;
            let mut arg_rows = arg_stmt.query([&call_id])?;
            let mut args = Vec::new();
            while let Some(row) = arg_rows.next()? {
                let index: usize = row.get(0)?;
                let var: String = row.get(1)?;
                args.push((index, var));
            }

            let mut resolved_targets = Vec::new();
            if let Some(ref static_f) = static_callee {
                resolved_targets.push(static_f.clone());
            } else if let Some(ref rec_var) = receiver {
                if let Some(rec_pts) = pts.get(rec_var) {
                    for alloc_id in rec_pts {
                        if let Some(class_fqn) = alloc_types.get(alloc_id) {
                            if let Some(target) = resolve_method(class_fqn, &method_name, args.len()) {
                                resolved_targets.push(target);
                            }
                        }
                    }
                }
            }

            let mut unique_targets = HashSet::new();
            resolved_targets.retain(|t| unique_targets.insert(t.clone()));

            for callee_method in unique_targets {
                if let Some((_, _, callee_params)) = method_decls.get(&callee_method) {
                    for (arg_idx, arg_var) in &args {
                        if *arg_idx < callee_params.len() {
                            let param_name = &callee_params[*arg_idx];
                            let param_node = format!("{}#{}", callee_method, param_name);
                            lineage_inserts.push((arg_var.clone(), param_node, "data".to_string()));
                            
                            let pos_node = format!("{}#p{}", callee_method, arg_idx);
                            lineage_inserts.push((arg_var.clone(), pos_node, "data".to_string()));
                        } else {
                            let pos_node = format!("{}#p{}", callee_method, arg_idx);
                            lineage_inserts.push((arg_var.clone(), pos_node, "data".to_string()));
                        }
                    }
                } else {
                    for (arg_idx, arg_var) in &args {
                        let pos_node = format!("{}#p{}", callee_method, arg_idx);
                        lineage_inserts.push((arg_var.clone(), pos_node, "data".to_string()));
                    }
                }

                if let Some(ref rec_var) = receiver {
                    let this_node = format!("{}#this", callee_method);
                    lineage_inserts.push((rec_var.clone(), this_node, "data".to_string()));
                }

                if let Some(ref lhs_var) = lhs {
                    let return_node = format!("{}#return", callee_method);
                    lineage_inserts.push((return_node, lhs_var.clone(), "data".to_string()));
                }
            }
        }

        let mut insert_stmt = conn.prepare(
            "INSERT OR IGNORE INTO lineage_edges (from_node, to_node, edge_type) VALUES (?1, ?2, ?3)"
        )?;
        for (from_n, to_n, edge_t) in lineage_inserts {
            if from_n != to_n {
                insert_stmt.execute([from_n, to_n, edge_t])?;
            }
        }

        Ok(())
    }
}

impl Default for DfgAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}
