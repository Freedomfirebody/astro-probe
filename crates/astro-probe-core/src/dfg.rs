use std::collections::{HashMap, HashSet};
use rusqlite::Connection;
use crate::cg::CoreError;

pub struct DfgAnalyzer;

impl DfgAnalyzer {
    pub fn new() -> Self {
        Self
    }

    pub fn analyze(&self, conn: &Connection) -> Result<(), CoreError> {
        conn.execute("DELETE FROM lineage_edges", [])?;

        // 1. Intra-procedural assignment copying
        let mut assign_stmt = conn.prepare(
            "SELECT lhs, rhs, assignment_type FROM source_assignments"
        )?;
        let mut assign_rows = assign_stmt.query([])?;
        let mut edges = Vec::new();
        while let Some(row) = assign_rows.next()? {
            let lhs: String = row.get(0)?;
            let rhs: String = row.get(1)?;
            let assign_type: String = row.get(2)?;
            
            // In lineage dataflow, values flow from rhs to lhs
            edges.push((rhs, lhs, assign_type));
        }
        drop(assign_rows);
        drop(assign_stmt);

        // 2. Inter-procedural call arguments mapping
        // Fetch all call arguments
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

        // For each call site:
        // Identify caller stripped signature, get resolved targets from call_edges
        let mut cs_stmt = conn.prepare(
            "SELECT call_id, method_fqn, receiver, method_name, lhs, static_callee FROM call_sites"
        )?;
        let mut cs_rows = cs_stmt.query([])?;
        let mut call_sites = Vec::new();
        while let Some(row) = cs_rows.next()? {
            let call_id: String = row.get(0)?;
            let caller_fqn: String = row.get(1)?;
            let receiver: Option<String> = row.get(2)?;
            let method_name: String = row.get(3)?;
            let lhs: Option<String> = row.get(4)?;
            let static_callee: Option<String> = row.get(5)?;
            call_sites.push((call_id, caller_fqn, receiver, method_name, lhs, static_callee));
        }
        drop(cs_rows);
        drop(cs_stmt);

        // Map call_edges for quick lookup
        let mut ce_stmt = conn.prepare("SELECT caller, callee, is_virtual FROM call_edges")?;
        let mut ce_rows = ce_stmt.query([])?;
        let mut call_edges_map: HashMap<String, Vec<(String, bool)>> = HashMap::new();
        while let Some(row) = ce_rows.next()? {
            let caller: String = row.get(0)?;
            let callee: String = row.get(1)?;
            let is_virt_int: i32 = row.get(2)?;
            call_edges_map.entry(caller).or_default().push((callee, is_virt_int != 0));
        }
        drop(ce_rows);
        drop(ce_stmt);

        // Map method declarations to get their full signatures from stripped callee names
        let mut decl_stmt = conn.prepare("SELECT method_fqn, class_fqn, method_name, params FROM method_declarations")?;
        let mut decl_rows = decl_stmt.query([])?;
        let mut decls_by_stripped: HashMap<String, Vec<String>> = HashMap::new();
        while let Some(row) = decl_rows.next()? {
            let method_fqn: String = row.get(0)?;
            let stripped = strip_signature(&method_fqn).to_string();
            decls_by_stripped.entry(stripped).or_default().push(method_fqn);
        }
        drop(decl_rows);
        drop(decl_stmt);

        for (call_id, caller_fqn, receiver, method_name, lhs, static_callee) in call_sites {
            let caller_stripped = strip_signature(&caller_fqn);
            
            // Find resolved target method FQNs
            let mut resolved_targets = HashSet::new();
            if let Some(targets) = call_edges_map.get(caller_stripped) {
                for (target_stripped, _) in targets {
                    // Match the stripped target to its full declarations
                    // Check if name aligns
                    if let Some(fqns) = decls_by_stripped.get(target_stripped) {
                        for fqn in fqns {
                            if fqn.contains(&format!(".{}(", method_name)) {
                                resolved_targets.insert(fqn.clone());
                            }
                        }
                    }
                }
            }

            // Fallback to static_callee if no dynamic dispatch target resolved
            if resolved_targets.is_empty() {
                if let Some(ref sc) = static_callee {
                    resolved_targets.insert(sc.clone());
                }
            }

            for target in resolved_targets {
                // Propagate arguments to parameters: arg_var -> target#pX
                if let Some(args) = call_args.get(&call_id) {
                    for (idx, arg_var, _) in args {
                        let target_param = format!("{}#p{}", target, idx);
                        edges.push((arg_var.clone(), target_param, "PASS_ARG".to_string()));
                    }
                }

                // Propagate receiver to receiver/this parameter of callee
                if let Some(ref rec) = receiver {
                    let target_this = format!("{}#this", target);
                    edges.push((rec.clone(), target_this, "PASS_REC".to_string()));
                }

                // Propagate return value back to call site LHS: target#return -> call_site_lhs
                if let Some(ref return_var) = lhs {
                    let target_return = format!("{}#return", target);
                    edges.push((target_return, return_var.clone(), "PASS_RET".to_string()));
                }
            }
        }

        // 3. Persist lineage edges
        let mut insert_edge = conn.prepare(
            "INSERT OR IGNORE INTO lineage_edges (from_node, to_node, edge_type) VALUES (?1, ?2, ?3)"
        )?;
        for (from, to, edge_type) in edges {
            insert_edge.execute([&from, &to, &edge_type])?;
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
    fn test_dfg_lineage_generation() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        init_db(&conn).unwrap();

        // Setup call graph and source assignments representing a method call
        conn.execute(
            "INSERT INTO method_declarations (method_fqn, class_fqn, method_name, params) \
             VALUES ('com.test.Service.process(java.lang.String)', 'com.test.Service', 'process', 'data')",
            []
        ).unwrap();
        conn.execute(
            "INSERT INTO method_declarations (method_fqn, class_fqn, method_name, params) \
             VALUES ('com.test.Client.main()', 'com.test.Client', 'main', '')",
            []
        ).unwrap();

        // Inside Client.main():
        // input = "sensitivedata";
        // output = service.process(input);
        conn.execute(
            "INSERT INTO allocation_sites (alloc_id, class_fqn, method_fqn) \
             VALUES ('StringSens', 'java.lang.String', 'com.test.Client.main()')",
            []
        ).unwrap();
        conn.execute(
            "INSERT INTO source_assignments (lhs, rhs, assignment_type, method_fqn) \
             VALUES ('com.test.Client.main()#input', 'StringSens', 'ALLOC', 'com.test.Client.main()')",
            []
        ).unwrap();

        // Call Site
        conn.execute(
            "INSERT INTO call_sites (call_id, method_fqn, receiver, method_name, lhs, static_callee) \
             VALUES ('call_2', 'com.test.Client.main()', 'com.test.Client.main()#service', 'process', 'com.test.Client.main()#output', 'com.test.Service.process(java.lang.String)')",
            []
        ).unwrap();

        // Call Argument
        conn.execute(
            "INSERT INTO call_arguments (call_id, arg_index, arg_var, arg_type) \
             VALUES ('call_2', 0, 'com.test.Client.main()#input', 'java.lang.String')",
            []
        ).unwrap();

        // Call Edge
        conn.execute(
            "INSERT INTO call_edges (caller, callee, is_virtual) \
             VALUES ('com.test.Client.main', 'com.test.Service.process', 0)",
            []
        ).unwrap();

        // Inner processing inside Service.process():
        // return data;  -> Service.process()#return = COPY(Service.process()#data)
        conn.execute(
            "INSERT OR REPLACE INTO source_assignments (lhs, rhs, assignment_type, method_fqn) \
             VALUES ('com.test.Service.process(java.lang.String)#data', 'com.test.Service.process(java.lang.String)#p0', 'COPY', 'com.test.Service.process(java.lang.String)')",
            []
        ).unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO source_assignments (lhs, rhs, assignment_type, method_fqn) \
             VALUES ('com.test.Service.process(java.lang.String)#return', 'com.test.Service.process(java.lang.String)#data', 'COPY', 'com.test.Service.process(java.lang.String)')",
            []
        ).unwrap();

        let dfg = DfgAnalyzer::new();
        dfg.analyze(&conn).unwrap();

        // Verify parameter passing lineage edge
        let count_param_edge: i64 = conn.query_row(
            "SELECT count(*) FROM lineage_edges WHERE from_node = 'com.test.Client.main()#input' AND to_node = 'com.test.Service.process(java.lang.String)#p0' AND edge_type = 'PASS_ARG'",
            [],
            |r| r.get(0)
        ).unwrap();
        assert_eq!(count_param_edge, 1);

        // Verify return value lineage edge
        let count_ret_edge: i64 = conn.query_row(
            "SELECT count(*) FROM lineage_edges WHERE from_node = 'com.test.Service.process(java.lang.String)#return' AND to_node = 'com.test.Client.main()#output' AND edge_type = 'PASS_RET'",
            [],
            |r| r.get(0)
        ).unwrap();
        assert_eq!(count_ret_edge, 1);
    }
}
