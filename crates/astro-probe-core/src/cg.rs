use rusqlite::Connection;
use std::collections::{HashMap, HashSet};

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
        conn.execute("BEGIN IMMEDIATE TRANSACTION;", [])?;
        let res = (|| -> Result<(), CoreError> {
            // Step 1: Initialize local points-to sets from direct allocation assignments
            conn.execute("DELETE FROM points_to_sets", [])?;

            let mut pts: HashMap<String, HashSet<String>> = HashMap::new();

            // Load direct allocations: lhs = ALLOC(rhs)
            let mut alloc_stmt = conn.prepare(
                "SELECT lhs, rhs FROM source_assignments WHERE assignment_type = 'ALLOC'",
            )?;
            let mut alloc_rows = alloc_stmt.query([])?;
            while let Some(row) = alloc_rows.next()? {
                let lhs: String = row.get(0)?;
                let rhs: String = row.get(1)?;
                if lhs.contains("Spring") || rhs.contains("Spring") {
                    println!("DEBUG [alloc]: lhs={}, rhs={}", lhs, rhs);
                }
                pts.entry(lhs).or_default().insert(rhs);
            }
            drop(alloc_rows);
            drop(alloc_stmt);

            // Load copy assignments: lhs = COPY(rhs)
            let mut copy_stmt = conn.prepare(
                "SELECT lhs, rhs FROM source_assignments WHERE assignment_type = 'COPY'",
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
                "SELECT lhs, rhs FROM source_assignments WHERE assignment_type = 'FIELD_READ'",
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
                "SELECT lhs, rhs FROM source_assignments WHERE assignment_type = 'FIELD_WRITE'",
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
            let mut hier_stmt =
                conn.prepare("SELECT class_fqn, parent_fqn FROM class_hierarchy")?;
            let mut hier_rows = hier_stmt.query([])?;
            let mut parent_map: HashMap<String, Vec<String>> = HashMap::new();
            while let Some(row) = hier_rows.next()? {
                let child: String = row.get(0)?;
                let parent: String = row.get(1)?;
                parent_map.entry(child).or_default().push(parent);
            }
            drop(hier_rows);
            drop(hier_stmt);

            // Load all declared methods for dynamic dispatch resolution
            let mut decl_stmt = conn.prepare(
                "SELECT method_fqn, class_fqn, method_name, params FROM method_declarations",
            )?;
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

            // Precompute ancestor distances to speed up subtype check and resolve virtual calls
            let mut ancestors_map: HashMap<String, HashMap<String, usize>> = HashMap::new();
            let mut all_classes = HashSet::new();
            for (child, parents) in &parent_map {
                all_classes.insert(child.clone());
                for p in parents {
                    all_classes.insert(p.clone());
                }
            }
            for (_, class_fqn, _, _) in &declarations {
                all_classes.insert(class_fqn.clone());
            }

            for class in &all_classes {
                let mut distances = HashMap::new();
                distances.insert(class.clone(), 0);
                let mut queue = std::collections::VecDeque::new();
                queue.push_back((class.clone(), 0));
                let mut visited = HashSet::new();
                while let Some((curr, d)) = queue.pop_front() {
                    if visited.insert(curr.clone()) {
                        if let Some(parents) = parent_map.get(&curr) {
                            for p in parents {
                                let entry = distances.entry(p.clone()).or_insert(usize::MAX);
                                if d + 1 < *entry {
                                    *entry = d + 1;
                                    queue.push_back((p.clone(), d + 1));
                                }
                            }
                        }
                    }
                }
                ancestors_map.insert(class.clone(), distances);
            }

            // Index declarations for fast lookups
            let mut decl_by_name_params: HashMap<(String, String), Vec<(String, String)>> =
                HashMap::new();
            let mut decl_by_class_name: HashMap<(String, String), Vec<String>> = HashMap::new();
            for (method_fqn, class_fqn, name, params) in &declarations {
                decl_by_name_params
                    .entry((name.clone(), params.clone()))
                    .or_default()
                    .push((method_fqn.clone(), class_fqn.clone()));
                decl_by_class_name
                    .entry((class_fqn.clone(), name.clone()))
                    .or_default()
                    .push(method_fqn.clone());
            }

            // Helper to check if child is subtype of parent
            let is_subtype = |child: &str, parent: &str| -> bool {
                if child == parent {
                    return true;
                }
                if let Some(distances) = ancestors_map.get(child) {
                    distances.contains_key(parent)
                } else {
                    false
                }
            };

            // Helper to get parameter count from method signature FQN
            let get_param_count = |fqn: &str| -> usize {
                if let Some(start) = fqn.find('(') {
                    if let Some(end) = fqn.rfind(')') {
                        let content = fqn[start + 1..end].trim();
                        if content.is_empty() {
                            return 0;
                        }
                        return content.split(',').count();
                    }
                }
                0
            };

            // Helper to resolve virtual call
            let resolve_virtual_call =
                |alloc_type: &str, method_name: &str, params_sig: &str| -> Option<String> {
                    let mut best_target = None;
                    let mut best_distance = usize::MAX;

                    // 1. Try exact parameter signature match first
                    if let Some(list) =
                        decl_by_name_params.get(&(method_name.to_string(), params_sig.to_string()))
                    {
                        for (method_fqn, class_fqn) in list {
                            if is_subtype(alloc_type, class_fqn) {
                                if let Some(distances) = ancestors_map.get(alloc_type) {
                                    if let Some(&d) = distances.get(class_fqn) {
                                        if d < best_distance {
                                            best_distance = d;
                                            best_target = Some(method_fqn.clone());
                                        }
                                    }
                                }
                            }
                        }
                    }

                    // 2. If exact match failed, fall back to parameter count match in class hierarchy
                    if best_target.is_none() {
                        let call_param_count = if params_sig.trim().is_empty() {
                            0
                        } else {
                            params_sig.split(',').count()
                        };

                        if let Some(distances) = ancestors_map.get(alloc_type) {
                            for (ancestor, &d) in distances {
                                if d < best_distance {
                                    if let Some(methods) = decl_by_class_name
                                        .get(&(ancestor.clone(), method_name.to_string()))
                                    {
                                        for m_fqn in methods {
                                            if get_param_count(m_fqn) == call_param_count {
                                                best_distance = d;
                                                best_target = Some(m_fqn.clone());
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }

                    best_target
                };

            // Setup dynamic dispatch mapping
            // call_id -> (receiver_var, method_name, static_callee, return_var_opt)
            let mut call_sites = Vec::new();
            let mut call_args: HashMap<String, Vec<(u32, String, String)>> = HashMap::new();

            let mut call_stmt = conn.prepare(
                "SELECT call_id, method_fqn, receiver, method_name, lhs, static_callee FROM call_sites"
            )?;
            let mut call_rows = call_stmt.query([])?;
            while let Some(row) = call_rows.next()? {
                let call_id: String = row.get(0)?;
                let method_fqn: String = row.get(1)?;
                let receiver: Option<String> = row.get(2)?;
                let method_name: String = row.get(3)?;
                let lhs: Option<String> = row.get(4)?;
                let static_callee: Option<String> = row.get(5)?;
                call_sites.push((
                    call_id,
                    method_fqn,
                    receiver,
                    method_name,
                    lhs,
                    static_callee,
                ));
            }
            drop(call_rows);
            drop(call_stmt);

            let mut arg_stmt = conn.prepare(
                "SELECT call_id, arg_index AS parameter_idx, arg_var AS argument_var, arg_type AS parameter_type FROM call_arguments"
            )?;
            let mut arg_rows = arg_stmt.query([])?;
            while let Some(row) = arg_rows.next()? {
                let call_id: String = row.get(0)?;
                let parameter_idx: u32 = row.get(1)?;
                let argument_var: String = row.get(2)?;
                let parameter_type: String = row.get(3)?;
                call_args.entry(call_id).or_default().push((
                    parameter_idx,
                    argument_var,
                    parameter_type,
                ));
            }
            drop(arg_rows);
            drop(arg_stmt);

            // Populate allocation types
            let mut alloc_types = HashMap::new();
            let mut alloc_stmt =
                conn.prepare("SELECT alloc_id, class_fqn FROM allocation_sites")?;
            let mut alloc_rows = alloc_stmt.query([])?;
            while let Some(row) = alloc_rows.next()? {
                let alloc_id: String = row.get(0)?;
                let class_fqn: String = row.get(1)?;
                alloc_types.insert(alloc_id, class_fqn);
            }
            drop(alloc_rows);
            drop(alloc_stmt);

            // Load method summaries
            let mut summaries: HashMap<String, Vec<u32>> = HashMap::new();
            let mut summary_stmt =
                conn.prepare("SELECT method_fqn, param_index FROM method_summaries")?;
            let mut summary_rows = summary_stmt.query([])?;
            while let Some(row) = summary_rows.next()? {
                let method_fqn: String = row.get(0)?;
                let param_index: u32 = row.get(1)?;
                summaries.entry(method_fqn).or_default().push(param_index);
            }
            drop(summary_rows);
            drop(summary_stmt);

            // Load supernodes (in-degree > 500)
            let mut supernodes = HashSet::new();
            let mut supernode_stmt = conn.prepare(
                "SELECT static_callee, COUNT(*) FROM call_sites WHERE static_callee IS NOT NULL GROUP BY static_callee HAVING COUNT(*) > 500"
            )?;
            let mut supernode_rows = supernode_stmt.query([])?;
            while let Some(row) = supernode_rows.next()? {
                let static_callee: String = row.get(0)?;
                supernodes.insert(static_callee);
            }
            drop(supernode_rows);
            drop(supernode_stmt);

            let is_supernode = |target: &str| -> bool {
                supernodes.contains(target)
                    || target.contains("java.lang.Object.toString")
                    || target.contains("java.lang.StringBuilder.toString")
                    || target.contains("java.lang.StringBuffer.toString")
            };

            let mut allocs_by_class: HashMap<String, Vec<String>> = HashMap::new();
            for (alloc_id, class_fqn) in &alloc_types {
                allocs_by_class
                    .entry(class_fqn.clone())
                    .or_default()
                    .push(alloc_id.clone());
            }

            // Andersen's Point-to solver propagation loop
            let mut changed = true;
            let mut instance_fields: HashMap<(String, String), HashSet<String>> = HashMap::new();
            let mut call_edges_discovered: HashSet<(String, String, bool)> = HashSet::new();

            while changed {
                changed = false;

                // 1. Process copy assignments: lhs = COPY(rhs)
                for (lhs, rhs) in &copies {
                    let rhs_pts_clone = if let Some(rhs_set) = pts.get(rhs) {
                        Some(rhs_set.clone())
                    } else {
                        None
                    };
                    if let Some(rhs_set) = rhs_pts_clone {
                        let lhs_set = pts.entry(lhs.clone()).or_default();
                        for val in rhs_set {
                            if lhs_set.insert(val) {
                                changed = true;
                            }
                        }
                    }
                }

                // 2. Process field writes: base_var#field = rhs_var
                // In Andersen's: for each allocation 'a' in base_var's pts, copy rhs_var's pts to (a, field)
                for (lhs_field_expr, rhs) in &field_writes {
                    // lhs_field_expr format is base_var.field where base_var contains a '#' (e.g., caller_fqn#this.field_name)
                    if let Some(hash_idx) = lhs_field_expr.find('#') {
                        let suffix = &lhs_field_expr[hash_idx + 1..];
                        if let Some(dot_idx) = suffix.find('.') {
                            let base_var = &lhs_field_expr[..hash_idx + 1 + dot_idx];
                            let field = &suffix[dot_idx + 1..];

                            if let Some(base_pts) = pts.get(base_var) {
                                let base_pts_clone = base_pts.clone();
                                if let Some(rhs_set) = pts.get(rhs) {
                                    let rhs_clone = rhs_set.clone();
                                    for alloc in base_pts_clone {
                                        let field_set = instance_fields
                                            .entry((alloc, field.to_string()))
                                            .or_default();
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
                }

                // 3. Process field reads: lhs = base_var.field
                // In Andersen's: for each allocation 'a' in base_var's pts, copy (a, field)'s pts to lhs
                for (lhs, rhs_field_expr) in &field_reads {
                    if let Some(hash_idx) = rhs_field_expr.find('#') {
                        let suffix = &rhs_field_expr[hash_idx + 1..];
                        if let Some(dot_idx) = suffix.find('.') {
                            let base_var = &rhs_field_expr[..hash_idx + 1 + dot_idx];
                            let field = &suffix[dot_idx + 1..];

                            let base_pts_clone = if let Some(base_pts) = pts.get(base_var) {
                                Some(base_pts.clone())
                            } else {
                                None
                            };
                            if let Some(base_pts) = base_pts_clone {
                                for alloc in base_pts {
                                    if let Some(field_set) =
                                        instance_fields.get(&(alloc, field.to_string()))
                                    {
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
                }

                // 3b. Map Spring DI bean allocation facts to points-to propagation
                let spring_keys: Vec<_> = pts.keys().filter(|k| k.contains("Spring")).collect();
                println!("DEBUG [3b]: spring_keys in pts={:?}", spring_keys);
                for (key, key_pts) in &pts {
                    if key.starts_with("SpringBeanAlloc:") {
                        let stripped = &key["SpringBeanAlloc:".len()..];
                        if let Some(dot_idx) = stripped.rfind('.') {
                            let class_fqn = &stripped[..dot_idx];
                            let field_name = &stripped[dot_idx + 1..];
                            println!(
                                "DEBUG [3b]: key={}, stripped={}, class_fqn={}, field_name={}",
                                key, stripped, class_fqn, field_name
                            );
                            if let Some(alloc_ids) = allocs_by_class.get(class_fqn) {
                                println!(
                                    "DEBUG [3b]: Found alloc_ids={:?} for class_fqn={}",
                                    alloc_ids, class_fqn
                                );
                                let key_pts_clone = key_pts.clone();
                                for alloc_id in alloc_ids {
                                    let field_set = instance_fields
                                        .entry((alloc_id.clone(), field_name.to_string()))
                                        .or_default();
                                    for val in &key_pts_clone {
                                        if field_set.insert(val.clone()) {
                                            println!(
                                                "DEBUG [3b]: Inserted {} into ({}, {})",
                                                val, alloc_id, field_name
                                            );
                                            changed = true;
                                        }
                                    }
                                }
                            } else {
                                println!("DEBUG [3b]: NO alloc_ids found for class_fqn={} in allocs_by_class keys: {:?}", class_fqn, allocs_by_class.keys().collect::<Vec<_>>());
                            }
                        }
                    }
                }

                // 4. Resolve call sites virtually & statically
                for (call_id, caller_method_fqn, receiver, method_name, lhs, static_callee) in
                    &call_sites
                {
                    // Resolve target methods
                    let mut resolved_targets = HashSet::new(); // set of (target_method_fqn, is_virtual)

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

                                if let Some(target) =
                                    resolve_virtual_call(alloc_type, method_name, &params_sig)
                                {
                                    resolved_targets.insert((target, true));
                                }
                            }
                        }
                    } else if let Some(ref sc) = static_callee {
                        resolved_targets.insert((sc.clone(), false));
                    }

                    // Reflection propagation: handle Class.forName, getMethod, invoke
                    // If caller_method_fqn calls Class.forName(className)
                    if method_name == "forName"
                        && static_callee.as_deref()
                            == Some("java.lang.Class.forName(java.lang.String)")
                    {
                        if let Some(args) = call_args.get(call_id) {
                            if let Some((_, arg_var, _)) = args.iter().find(|(idx, _, _)| *idx == 0)
                            {
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
                                                let lhs_set =
                                                    pts.entry(return_var.clone()).or_default();
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
                                                            // Find all matching method declarations in class_name
                                                            if let Some(list) = decl_by_class_name.get(&(class_name.to_string(), m_name.to_string())) {
                                                                for m_fqn in list {
                                                                    method_allocs.push(format!("MethodAlloc:{}", m_fqn));
                                                                }
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        if !method_allocs.is_empty() {
                            if let Some(ref return_var) = lhs {
                                let lhs_set = pts.entry(return_var.clone()).or_default();
                                for ma in method_allocs {
                                    if lhs_set.insert(ma) {
                                        changed = true;
                                    }
                                }
                            }
                        }
                    }

                    // If caller_method_fqn calls Method.invoke(obj, args)
                    if method_name == "invoke" && static_callee.as_deref() == Some("java.lang.reflect.Method.invoke(java.lang.Object,java.lang.Object[])") {
                        if let Some(ref rec_var) = receiver {
                            if let Some(rec_pts) = pts.get(rec_var).cloned() {
                                for method_alloc in rec_pts {
                                    if method_alloc.starts_with("MethodAlloc:") {
                                        let target = &method_alloc["MethodAlloc:".len()..];
                                        let mut call_obj = None;
                                        if let Some(args) = call_args.get(call_id) {
                                            if let Some((_, obj_var, _)) = args.iter().find(|(idx, _, _)| *idx == 0) {
                                                call_obj = Some(obj_var.clone());
                                            }
                                        }

                                        // Dynamic dispatch target on reflection base object
                                        let mut resolved_targets_refl = HashSet::new();
                                        if let Some(ref co) = call_obj {
                                            if let Some(co_pts) = pts.get(co).cloned() {
                                                for alloc in co_pts {
                                                    if let Some(alloc_type) = alloc_types.get(&alloc) {
                                                        if let Some(idx) = target.rfind('.') {
                                                            let m_name = &target[idx + 1..];
                                                            let params_sig = if let Some(start) = target.find('(') {
                                                                if let Some(end) = target.rfind(')') {
                                                                    target[start + 1..end].to_string()
                                                                } else {
                                                                    String::new()
                                                                }
                                                            } else {
                                                                String::new()
                                                            };
                                                            if let Some(refl_target) = resolve_virtual_call(alloc_type, m_name, &params_sig) {
                                                                resolved_targets_refl.insert((refl_target, true));
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                        if resolved_targets_refl.is_empty() {
                                            resolved_targets_refl.insert((target.to_string(), false));
                                        }

                                        for (refl_target, is_virt) in resolved_targets_refl {
                                            call_edges_discovered.insert((
                                                strip_signature(caller_method_fqn).to_string(),
                                                strip_signature(&refl_target).to_string(),
                                                is_virt
                                            ));

                                            if is_supernode(&refl_target) {
                                                if let Some(ref return_var) = lhs {
                                                    let supernode_alloc = format!("SupernodeReturn:{}", refl_target);
                                                    let return_set = pts.entry(return_var.clone()).or_default();
                                                    if return_set.insert(supernode_alloc) {
                                                        changed = true;
                                                    }
                                                }
                                                continue;
                                            }

                                            if let Some(_param_indices) = summaries.get(&refl_target) {
                                                if let Some(ref return_var) = lhs {
                                                    if let Some(args) = call_args.get(call_id) {
                                                        if let Some((_, args_array_var, _)) = args.iter().find(|(idx, _, _)| *idx == 1) {
                                                            if let Some(arr_pts) = pts.get(args_array_var) {
                                                                let arr_pts_clone = arr_pts.clone();
                                                                let return_set = pts.entry(return_var.clone()).or_default();
                                                                for val in arr_pts_clone {
                                                                    if return_set.insert(val) {
                                                                        changed = true;
                                                                    }
                                                                }
                                                            }
                                                        }
                                                    }
                                                }
                                                continue;
                                            }

                                            // Copy reflection args to target params
                                            if let Some(args) = call_args.get(call_id) {
                                                if let Some((_, args_array_var, _)) = args.iter().find(|(idx, _, _)| *idx == 1) {
                                                    if let Some(arr_pts) = pts.get(args_array_var) {
                                                        // Array points-to set propagates to target parameter
                                                        let arr_pts_clone = arr_pts.clone();
                                                        let param_var = format!("{}#p0", refl_target);
                                                        let param_set = pts.entry(param_var).or_default();
                                                        for val in arr_pts_clone {
                                                            if param_set.insert(val) {
                                                                changed = true;
                                                            }
                                                        }
                                                    }
                                                }
                                            }

                                            // Copy target return to refl invoke LHS
                                            if let Some(ref return_var) = lhs {
                                                let target_return = format!("{}#return", refl_target);
                                                if let Some(ret_pts) = pts.get(&target_return) {
                                                    let ret_pts_clone = ret_pts.clone();
                                                    let return_set = pts.entry(return_var.clone()).or_default();
                                                    for val in ret_pts_clone {
                                                        if return_set.insert(val) {
                                                            changed = true;
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }

                    // Propagate values into resolved targets
                    for (target, is_virt) in resolved_targets {
                        call_edges_discovered.insert((
                            strip_signature(caller_method_fqn).to_string(),
                            strip_signature(&target).to_string(),
                            is_virt,
                        ));

                        if is_supernode(&target) {
                            if let Some(ref return_var) = lhs {
                                let supernode_alloc = format!("SupernodeReturn:{}", target);
                                let return_set = pts.entry(return_var.clone()).or_default();
                                if return_set.insert(supernode_alloc) {
                                    changed = true;
                                }
                            }
                            continue;
                        }

                        if let Some(param_indices) = summaries.get(&target) {
                            if let Some(ref return_var) = lhs {
                                if let Some(args) = call_args.get(call_id) {
                                    for &param_idx in param_indices {
                                        if let Some((_, arg_var, _)) =
                                            args.iter().find(|(idx, _, _)| *idx == param_idx)
                                        {
                                            if let Some(arg_pts) = pts.get(arg_var) {
                                                let arg_pts_clone = arg_pts.clone();
                                                let return_set =
                                                    pts.entry(return_var.clone()).or_default();
                                                for val in arg_pts_clone {
                                                    if return_set.insert(val) {
                                                        changed = true;
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                            continue;
                        }

                        // Special logic: Spring DI parameter propagation
                        if target == "SpringDI" {
                            // Find target parameter names
                            let mut params = Vec::new();
                            if let Some(args) = call_args.get(call_id) {
                                for (idx, _, _) in args {
                                    params.push(idx);
                                }
                            }
                            if let Some(ref return_var) = lhs {
                                if let Some(args) = call_args.get(call_id) {
                                    for &param_idx in params {
                                        if let Some((_, arg_var, _)) =
                                            args.iter().find(|(idx, _, _)| *idx == param_idx)
                                        {
                                            if let Some(arg_pts) = pts.get(arg_var) {
                                                let arg_pts_clone = arg_pts.clone();
                                                let return_set =
                                                    pts.entry(return_var.clone()).or_default();
                                                for val in arg_pts_clone {
                                                    if return_set.insert(val) {
                                                        changed = true;
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                            continue;
                        }

                        // Standard propagation
                        let mut propagations = Vec::new();
                        if let Some(args) = call_args.get(call_id) {
                            for (idx, arg_var, _) in args {
                                if let Some(arg_pts) = pts.get(arg_var) {
                                    propagations
                                        .push((format!("{}#p{}", target, idx), arg_pts.clone()));
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
            let mut pts_to_insert = Vec::new();
            for (var, allocs) in &pts {
                for alloc in allocs {
                    pts_to_insert.push((var, alloc));
                }
            }
            pts_to_insert.sort_unstable_by(|a, b| a.0.cmp(b.0).then_with(|| a.1.cmp(b.1)));

            const PTS_CHUNK_SIZE: usize = 450;
            let mut query_pts_full = String::from(
                "INSERT OR REPLACE INTO points_to_sets (variable_fqn, alloc_id) VALUES ",
            );
            for i in 0..PTS_CHUNK_SIZE {
                if i > 0 {
                    query_pts_full.push_str(", ");
                }
                query_pts_full.push_str(&format!("(?{}, ?{})", i * 2 + 1, i * 2 + 2));
            }
            let mut stmt_pts_full = conn.prepare(&query_pts_full)?;

            for chunk in pts_to_insert.chunks(PTS_CHUNK_SIZE) {
                if chunk.len() == PTS_CHUNK_SIZE {
                    let mut params: [&dyn rusqlite::ToSql; PTS_CHUNK_SIZE * 2] =
                        [&"" as &dyn rusqlite::ToSql; PTS_CHUNK_SIZE * 2];
                    for (i, item) in chunk.iter().enumerate() {
                        params[i * 2] = item.0;
                        params[i * 2 + 1] = item.1;
                    }
                    stmt_pts_full.execute(&params[..])?;
                } else {
                    let mut query_pts_last = String::from(
                        "INSERT OR REPLACE INTO points_to_sets (variable_fqn, alloc_id) VALUES ",
                    );
                    for i in 0..chunk.len() {
                        if i > 0 {
                            query_pts_last.push_str(", ");
                        }
                        query_pts_last.push_str(&format!("(?{}, ?{})", i * 2 + 1, i * 2 + 2));
                    }
                    let mut stmt_pts_last = conn.prepare(&query_pts_last)?;
                    let mut params: Vec<&dyn rusqlite::ToSql> = Vec::with_capacity(chunk.len() * 2);
                    for item in chunk {
                        params.push(item.0);
                        params.push(item.1);
                    }
                    stmt_pts_last.execute(rusqlite::params_from_iter(params))?;
                }
            }

            // Clear existing call edges and persist newly resolved call edges
            conn.execute("DELETE FROM call_edges", [])?;
            let mut edges_to_insert = Vec::new();
            for (caller, callee, is_virt) in call_edges_discovered {
                let is_virt_int = if is_virt { 1 } else { 0 };
                edges_to_insert.push((caller, callee, is_virt_int));
            }
            edges_to_insert.sort_unstable_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));

            const EDGE_CHUNK_SIZE: usize = 300;
            let mut query_edge_full = String::from(
                "INSERT OR IGNORE INTO call_edges (caller, callee, is_virtual) VALUES ",
            );
            for i in 0..EDGE_CHUNK_SIZE {
                if i > 0 {
                    query_edge_full.push_str(", ");
                }
                query_edge_full.push_str(&format!(
                    "(?{}, ?{}, ?{})",
                    i * 3 + 1,
                    i * 3 + 2,
                    i * 3 + 3
                ));
            }
            let mut stmt_edge_full = conn.prepare(&query_edge_full)?;

            for chunk in edges_to_insert.chunks(EDGE_CHUNK_SIZE) {
                if chunk.len() == EDGE_CHUNK_SIZE {
                    let mut params: [&dyn rusqlite::ToSql; EDGE_CHUNK_SIZE * 3] =
                        [&"" as &dyn rusqlite::ToSql; EDGE_CHUNK_SIZE * 3];
                    for (i, item) in chunk.iter().enumerate() {
                        params[i * 3] = &item.0;
                        params[i * 3 + 1] = &item.1;
                        params[i * 3 + 2] = &item.2;
                    }
                    stmt_edge_full.execute(&params[..])?;
                } else {
                    let mut query_edge_last = String::from(
                        "INSERT OR IGNORE INTO call_edges (caller, callee, is_virtual) VALUES ",
                    );
                    for i in 0..chunk.len() {
                        if i > 0 {
                            query_edge_last.push_str(", ");
                        }
                        query_edge_last.push_str(&format!(
                            "(?{}, ?{}, ?{})",
                            i * 3 + 1,
                            i * 3 + 2,
                            i * 3 + 3
                        ));
                    }
                    let mut stmt_edge_last = conn.prepare(&query_edge_last)?;
                    let mut params: Vec<&dyn rusqlite::ToSql> = Vec::with_capacity(chunk.len() * 3);
                    for item in chunk {
                        params.push(&item.0);
                        params.push(&item.1);
                        params.push(&item.2);
                    }
                    stmt_edge_last.execute(rusqlite::params_from_iter(params))?;
                }
            }

            Ok(())
        })();
        match res {
            Ok(_) => {
                conn.execute("COMMIT;", [])?;
                Ok(())
            }
            Err(e) => {
                let _ = conn.execute("ROLLBACK;", []);
                Err(e)
            }
        }
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
        conn.execute(
            "INSERT INTO classes (fqn, kind) VALUES ('com.test.Base', 'class')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO classes (fqn, kind) VALUES ('com.test.SubA', 'class')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO classes (fqn, kind) VALUES ('com.test.SubB', 'class')",
            [],
        )
        .unwrap();

        conn.execute("INSERT INTO class_hierarchy (class_fqn, parent_fqn) VALUES ('com.test.SubA', 'com.test.Base')", []).unwrap();
        conn.execute("INSERT INTO class_hierarchy (class_fqn, parent_fqn) VALUES ('com.test.SubB', 'com.test.Base')", []).unwrap();

        // Setup method declarations
        conn.execute(
            "INSERT INTO method_declarations (method_fqn, class_fqn, method_name, params) \
             VALUES ('com.test.Base.foo()', 'com.test.Base', 'foo', '')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO method_declarations (method_fqn, class_fqn, method_name, params) \
             VALUES ('com.test.SubA.foo()', 'com.test.SubA', 'foo', '')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO method_declarations (method_fqn, class_fqn, method_name, params) \
             VALUES ('com.test.SubB.foo()', 'com.test.SubB', 'foo', '')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO method_declarations (method_fqn, class_fqn, method_name, params) \
             VALUES ('com.test.Main.run()', 'com.test.Main', 'run', '')",
            [],
        )
        .unwrap();

        // Allocations in Main.run()
        conn.execute(
            "INSERT INTO allocation_sites (alloc_id, class_fqn, method_fqn) \
             VALUES ('AllocSubA', 'com.test.SubA', 'com.test.Main.run()')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO allocation_sites (alloc_id, class_fqn, method_fqn) \
             VALUES ('AllocSubB', 'com.test.SubB', 'com.test.Main.run()')",
            [],
        )
        .unwrap();

        // source_assignments representing:
        // x = new SubA();  -> com.test.Main.run()#x = ALLOC(AllocSubA)
        // y = new SubB();  -> com.test.Main.run()#y = ALLOC(AllocSubB)
        // z = x;           -> com.test.Main.run()#z = COPY(com.test.Main.run()#x)
        conn.execute(
            "INSERT INTO source_assignments (lhs, rhs, assignment_type, method_fqn) \
             VALUES ('com.test.Main.run()#x', 'AllocSubA', 'ALLOC', 'com.test.Main.run()')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO source_assignments (lhs, rhs, assignment_type, method_fqn) \
             VALUES ('com.test.Main.run()#y', 'AllocSubB', 'ALLOC', 'com.test.Main.run()')",
            [],
        )
        .unwrap();
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
