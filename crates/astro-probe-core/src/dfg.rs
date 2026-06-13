use crate::cg::CoreError;
use rusqlite::Connection;
use std::collections::{HashMap, HashSet};

pub struct DfgAnalyzer;

impl DfgAnalyzer {
    pub fn new() -> Self {
        Self
    }

    pub fn analyze(&self, conn: &Connection) -> Result<(), CoreError> {
        let t_total = std::time::Instant::now();
        conn.execute("BEGIN IMMEDIATE TRANSACTION;", [])?;

        let res = (|| -> Result<(), CoreError> {
            let t_delete = std::time::Instant::now();
            conn.execute("DELETE FROM lineage_edges", [])?;
            println!(
                "DFG: DELETE FROM lineage_edges took {:?}",
                t_delete.elapsed()
            );

            let t_assign = std::time::Instant::now();
            // 1. Intra-procedural assignment copying
            let mut assign_stmt =
                conn.prepare("SELECT lhs, rhs, assignment_type FROM source_assignments")?;
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
            println!("DFG: Fetching assignments took {:?}", t_assign.elapsed());

            let t_calls = std::time::Instant::now();
            // 2. Inter-procedural call arguments mapping
            // Fetch all call arguments
            let mut args_stmt =
                conn.prepare("SELECT call_id, arg_index, arg_var, arg_type FROM call_arguments")?;
            let mut args_rows = args_stmt.query([])?;
            let mut call_args: HashMap<String, Vec<(usize, String, Option<String>)>> =
                HashMap::new();
            while let Some(row) = args_rows.next()? {
                let call_id: String = row.get(0)?;
                let idx: usize = row.get(1)?;
                let arg_var: String = row.get(2)?;
                let arg_type: Option<String> = row.get(3)?;
                call_args
                    .entry(call_id)
                    .or_default()
                    .push((idx, arg_var, arg_type));
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
                call_sites.push((
                    call_id,
                    caller_fqn,
                    receiver,
                    method_name,
                    lhs,
                    static_callee,
                ));
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
                call_edges_map
                    .entry(caller)
                    .or_default()
                    .push((callee, is_virt_int != 0));
            }
            drop(ce_rows);
            drop(ce_stmt);

            // Map method declarations to get their full signatures from stripped callee names
            let mut decl_stmt = conn.prepare(
                "SELECT method_fqn, class_fqn, method_name, params FROM method_declarations",
            )?;
            let mut decl_rows = decl_stmt.query([])?;
            let mut decls_by_stripped_and_name: HashMap<(String, String), Vec<String>> =
                HashMap::new();
            while let Some(row) = decl_rows.next()? {
                let method_fqn: String = row.get(0)?;
                let method_name: String = row.get(2)?;
                let stripped = strip_signature(&method_fqn).to_string();
                decls_by_stripped_and_name
                    .entry((stripped, method_name))
                    .or_default()
                    .push(method_fqn);
            }
            drop(decl_rows);
            drop(decl_stmt);
            println!(
                "DFG: Preparing call-site/decls structures took {:?}",
                t_calls.elapsed()
            );

            let t_resolve = std::time::Instant::now();
            for (call_id, caller_fqn, receiver, method_name, lhs, static_callee) in call_sites {
                let caller_stripped = strip_signature(&caller_fqn);

                // Find resolved target method FQNs
                let mut resolved_targets = HashSet::new();
                if let Some(targets) = call_edges_map.get(caller_stripped) {
                    for (target_stripped, _) in targets {
                        // Match the stripped target to its full declarations
                        if let Some(fqns) = decls_by_stripped_and_name
                            .get(&(target_stripped.clone(), method_name.clone()))
                        {
                            for fqn in fqns {
                                resolved_targets.insert(fqn.clone());
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
            println!(
                "DFG: Resolving call targets and constructing initial edges took {:?}",
                t_resolve.elapsed()
            );

            let t_build_graph = std::time::Instant::now();
            // 3. Perform transitive DFG graph reduction on lineage edges in memory
            let mut in_edges: HashMap<String, HashMap<String, String>> = HashMap::new();
            let mut out_edges: HashMap<String, HashMap<String, String>> = HashMap::new();

            for (from, to, edge_type) in edges {
                if from == to {
                    continue;
                }
                in_edges
                    .entry(to.clone())
                    .or_default()
                    .entry(from.clone())
                    .and_modify(|t| *t = merge_edge_types(t, &edge_type))
                    .or_insert(edge_type.clone());
                out_edges
                    .entry(from.clone())
                    .or_default()
                    .entry(to.clone())
                    .and_modify(|t| *t = merge_edge_types(t, &edge_type))
                    .or_insert(edge_type);
            }
            println!(
                "DFG: Building initial in-memory graph took {:?}",
                t_build_graph.elapsed()
            );

            let t_reduce = std::time::Instant::now();
            let mut worklist = Vec::new();
            for node in in_edges.keys() {
                if is_collapsible_node(node) {
                    if let Some(ins) = in_edges.get(node) {
                        if ins.len() == 1 {
                            if let Some(outs) = out_edges.get(node) {
                                if outs.len() == 1 {
                                    let u = ins.keys().next().unwrap();
                                    let w = outs.keys().next().unwrap();
                                    if u != w {
                                        worklist.push(node.clone());
                                    }
                                }
                            }
                        }
                    }
                }
            }

            while let Some(v) = worklist.pop() {
                let (u, w) = if let (Some(ins), Some(outs)) = (in_edges.get(&v), out_edges.get(&v))
                {
                    if ins.len() == 1 && outs.len() == 1 {
                        let u = ins.keys().next().unwrap().clone();
                        let w = outs.keys().next().unwrap().clone();
                        if u != w {
                            (u, w)
                        } else {
                            continue;
                        }
                    } else {
                        continue;
                    }
                } else {
                    continue;
                };

                let t1 = in_edges.get(&v).unwrap().get(&u).unwrap().clone();
                let t2 = out_edges.get(&v).unwrap().get(&w).unwrap().clone();

                // Remove v
                in_edges.remove(&v);
                out_edges.remove(&v);
                if let Some(outs) = out_edges.get_mut(&u) {
                    outs.remove(&v);
                }
                if let Some(ins) = in_edges.get_mut(&w) {
                    ins.remove(&v);
                }

                // Add merged u -> w
                let merged_t = merge_edge_types(&t1, &t2);
                out_edges
                    .entry(u.clone())
                    .or_default()
                    .entry(w.clone())
                    .and_modify(|t| *t = merge_edge_types(t, &merged_t))
                    .or_insert(merged_t.clone());
                in_edges
                    .entry(w.clone())
                    .or_default()
                    .entry(u.clone())
                    .and_modify(|t| *t = merge_edge_types(t, &merged_t))
                    .or_insert(merged_t);

                // Check if u and w might be new candidates
                if is_collapsible_node(&u) {
                    if let (Some(ins), Some(outs)) = (in_edges.get(&u), out_edges.get(&u)) {
                        if ins.len() == 1 && outs.len() == 1 {
                            let uu = ins.keys().next().unwrap();
                            let uw = outs.keys().next().unwrap();
                            if uu != uw {
                                worklist.push(u.clone());
                            }
                        }
                    }
                }
                if is_collapsible_node(&w) {
                    if let (Some(ins), Some(outs)) = (in_edges.get(&w), out_edges.get(&w)) {
                        if ins.len() == 1 && outs.len() == 1 {
                            let wu = ins.keys().next().unwrap();
                            let ww = outs.keys().next().unwrap();
                            if wu != ww {
                                worklist.push(w.clone());
                            }
                        }
                    }
                }
            }
            println!("DFG: Transitive reduction took {:?}", t_reduce.elapsed());

            let t_insert = std::time::Instant::now();
            // 4. Persist reduced lineage edges to database
            conn.execute("DROP INDEX IF EXISTS idx_lineage_from;", [])?;
            conn.execute("DROP INDEX IF EXISTS idx_lineage_to;", [])?;

            let mut edges_to_insert = Vec::new();
            for (from, tos) in out_edges {
                for (to, edge_type) in tos {
                    edges_to_insert.push((from.clone(), to, edge_type));
                }
            }
            edges_to_insert.sort_unstable_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));

            let mut insert_count = 0;
            const CHUNK_SIZE: usize = 300;

            let mut query_full =
                String::from("INSERT INTO lineage_edges (from_node, to_node, edge_type) VALUES ");
            for i in 0..CHUNK_SIZE {
                if i > 0 {
                    query_full.push_str(", ");
                }
                query_full.push_str(&format!("(?{}, ?{}, ?{})", i * 3 + 1, i * 3 + 2, i * 3 + 3));
            }
            let mut stmt_full = conn.prepare(&query_full)?;

            for chunk in edges_to_insert.chunks(CHUNK_SIZE) {
                if chunk.len() == CHUNK_SIZE {
                    let mut params: [&dyn rusqlite::ToSql; CHUNK_SIZE * 3] =
                        [&"" as &dyn rusqlite::ToSql; CHUNK_SIZE * 3];
                    for (i, (from, to, edge_type)) in chunk.iter().enumerate() {
                        params[i * 3] = from;
                        params[i * 3 + 1] = to;
                        params[i * 3 + 2] = edge_type;
                    }
                    stmt_full.execute(&params[..])?;
                } else {
                    let mut query_last = String::from(
                        "INSERT INTO lineage_edges (from_node, to_node, edge_type) VALUES ",
                    );
                    for i in 0..chunk.len() {
                        if i > 0 {
                            query_last.push_str(", ");
                        }
                        query_last.push_str(&format!(
                            "(?{}, ?{}, ?{})",
                            i * 3 + 1,
                            i * 3 + 2,
                            i * 3 + 3
                        ));
                    }
                    let mut stmt_last = conn.prepare(&query_last)?;
                    let mut params: Vec<&dyn rusqlite::ToSql> = Vec::with_capacity(chunk.len() * 3);
                    for (from, to, edge_type) in chunk {
                        params.push(from);
                        params.push(to);
                        params.push(edge_type);
                    }
                    stmt_last.execute(rusqlite::params_from_iter(params))?;
                }
                insert_count += chunk.len();
            }
            println!("DFG: Inserted {} rows", insert_count);

            let t_reindex = std::time::Instant::now();
            conn.execute(
                "CREATE INDEX IF NOT EXISTS idx_lineage_from ON lineage_edges(from_node);",
                [],
            )?;
            conn.execute(
                "CREATE INDEX IF NOT EXISTS idx_lineage_to ON lineage_edges(to_node);",
                [],
            )?;
            println!("DFG: Reindexing took {:?}", t_reindex.elapsed());
            println!("DFG: DB insert took {:?}", t_insert.elapsed());

            Ok(())
        })();

        match res {
            Ok(_) => {
                conn.execute("COMMIT;", [])?;
                println!("DFG: Total analysis took {:?}", t_total.elapsed());
                Ok(())
            }
            Err(e) => {
                let _ = conn.execute("ROLLBACK;", []);
                Err(e)
            }
        }
    }
}

fn is_collapsible_node(node: &str) -> bool {
    let prefixes = [
        "temp_void_call_",
        "temp_alloc_",
        "temp_call_lhs_",
        "temp_field_",
    ];
    for prefix in &prefixes {
        if node.starts_with(prefix) {
            return true;
        }
        if let Some(hash_idx) = node.rfind('#') {
            let var_name = &node[hash_idx + 1..];
            if var_name.starts_with(prefix) {
                return true;
            }
        }
    }
    false
}

fn edge_priority(t: &str) -> i32 {
    match t {
        "PASS_ARG" => 5,
        "PASS_RET" => 4,
        "PASS_REC" => 3,
        "FIELD_READ" => 2,
        "FIELD_WRITE" => 1,
        "COPY" => 0,
        _ => -1,
    }
}

fn merge_edge_types(t1: &str, t2: &str) -> String {
    if edge_priority(t1) >= edge_priority(t2) {
        t1.to_string()
    } else {
        t2.to_string()
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
            [],
        )
        .unwrap();

        // Inside Client.main():
        // input = "sensitivedata";
        // output = service.process(input);
        conn.execute(
            "INSERT INTO allocation_sites (alloc_id, class_fqn, method_fqn) \
             VALUES ('StringSens', 'java.lang.String', 'com.test.Client.main()')",
            [],
        )
        .unwrap();
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
            [],
        )
        .unwrap();

        // Call Edge
        conn.execute(
            "INSERT INTO call_edges (caller, callee, is_virtual) \
             VALUES ('com.test.Client.main', 'com.test.Service.process', 0)",
            [],
        )
        .unwrap();

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
