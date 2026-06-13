use rusqlite::Connection;
use std::collections::{HashMap, HashSet};

#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("Database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
}

#[derive(Debug, Clone)]
pub struct CallSiteInfo<'a> {
    pub call_id: &'a str,
    pub caller_method_fqn: &'a str,
    pub receiver: Option<&'a str>,
    pub method_name: &'a str,
    pub lhs: Option<&'a str>,
    pub static_callee: Option<&'a str>,
}

pub struct ExtensionContext<'a> {
    pub conn: &'a Connection,
    pub pts: &'a mut HashMap<String, HashSet<String>>,
    pub instance_fields: &'a mut HashMap<(String, String), HashSet<String>>,
    pub call_edges_discovered: &'a mut HashSet<(String, String, bool)>,
    pub alloc_types: &'a HashMap<String, String>,
    pub allocs_by_class: &'a HashMap<String, Vec<String>>,
    pub ancestors_map: &'a HashMap<String, HashMap<String, usize>>,
    pub decl_by_name_params: &'a HashMap<(String, String), Vec<(String, String)>>,
    pub decl_by_class_name: &'a HashMap<(String, String), Vec<String>>,
    pub summaries: &'a HashMap<String, Vec<u32>>,
    pub call_args: &'a HashMap<String, Vec<(u32, String, String)>>,
}

impl<'a> ExtensionContext<'a> {
    pub fn is_subtype(&self, child: &str, parent: &str) -> bool {
        if child == parent {
            return true;
        }
        if let Some(distances) = self.ancestors_map.get(child) {
            distances.contains_key(parent)
        } else {
            false
        }
    }

    pub fn resolve_virtual_call(&self, alloc_type: &str, method_name: &str, params_sig: &str) -> Option<String> {
        let mut best_target = None;
        let mut best_distance = usize::MAX;

        // 1. Try exact parameter signature match first
        if let Some(list) = self.decl_by_name_params.get(&(method_name.to_string(), params_sig.to_string())) {
            for (method_fqn, class_fqn) in list {
                if self.is_subtype(alloc_type, class_fqn) {
                    if let Some(distances) = self.ancestors_map.get(alloc_type) {
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

        // 2. Fall back to parameter count match in class hierarchy
        if best_target.is_none() {
            let call_param_count = if params_sig.trim().is_empty() {
                0
            } else {
                params_sig.split(',').count()
            };

            if let Some(distances) = self.ancestors_map.get(alloc_type) {
                for (ancestor, &d) in distances {
                    if d < best_distance {
                        if let Some(methods) = self.decl_by_class_name.get(&(ancestor.clone(), method_name.to_string())) {
                            for m_fqn in methods {
                                if self.get_param_count(m_fqn) == call_param_count {
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
    }

    pub fn get_param_count(&self, fqn: &str) -> usize {
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
    }

    pub fn strip_signature<'b>(&self, method_fqn: &'b str) -> &'b str {
        if let Some(idx) = method_fqn.find('(') {
            &method_fqn[..idx]
        } else {
            method_fqn
        }
    }
}

pub trait PointsToSolverExtension: Send + Sync {
    /// Check if target is a supernode (e.g. java.lang.Object.toString)
    fn is_supernode(&self, _target: &str) -> bool {
        false
    }

    /// Executed in the fixpoint loop. Returns true if changes were made.
    fn propagate(&self, _context: &mut ExtensionContext) -> Result<bool, CoreError> {
        Ok(false)
    }

    /// Executed per call site. Returns Option<bool>.
    /// Some(changed) indicates the call was handled. None delegates to default.
    fn handle_call(
        &self,
        _context: &mut ExtensionContext,
        _call: &CallSiteInfo,
        _resolved_targets: &HashSet<(String, bool)>,
    ) -> Result<Option<bool>, CoreError> {
        Ok(None)
    }

    /// Returns true if the extension overrides propagate.
    fn needs_propagation(&self) -> bool {
        false
    }

    /// Check if the extension wishes to handle a call site.
    fn matches_call_site(&self, _call: &CallSiteInfo) -> bool {
        true
    }

    /// Returns true if the extension requires points-to and field lookup maps to be built.
    fn needs_points_to(&self) -> bool {
        true
    }
}

pub struct PointsToSolver;

impl PointsToSolver {
    pub fn new() -> Self {
        Self
    }

    #[allow(non_snake_case, clippy::type_complexity)]
    pub fn solve(&self, conn: &mut Connection, extensions: &[&dyn PointsToSolverExtension]) -> Result<(), CoreError> {
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;

        // Step 1: Initialize local points-to sets from direct allocation assignments
        let has_existing_records: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM points_to_sets LIMIT 1)",
            [],
            |row| row.get(0),
        ).unwrap_or(false);

        if !has_existing_records {
            tx.execute("DELETE FROM points_to_sets", [])?;
        }

        let mut pts: HashMap<(String, String), HashSet<(String, String)>> = HashMap::new();
        let mut loaded_pts: HashSet<((String, String), (String, String))> = HashSet::new();
        let mut loaded_edges: HashSet<(String, String, String, String, i32)> = HashSet::new();

        // Load direct allocations: lhs = ALLOC(rhs)
        let mut alloc_stmt = tx.prepare(
            "SELECT lhs, rhs, method_fqn FROM source_assignments WHERE assignment_type = 'ALLOC'",
        )?;
        let mut alloc_rows = alloc_stmt.query([])?;
        let mut allocs = Vec::new();
        while let Some(row) = alloc_rows.next()? {
            let lhs: String = row.get(0)?;
            let rhs: String = row.get(1)?;
            let method_fqn: String = row.get(2)?;
            allocs.push((lhs, rhs, method_fqn));
        }
        drop(alloc_rows);
        drop(alloc_stmt);

        // Load copy assignments: lhs = COPY(rhs)
        let mut copy_stmt = tx.prepare(
            "SELECT lhs, rhs, method_fqn FROM source_assignments WHERE assignment_type = 'COPY'",
        )?;
        let mut copy_rows = copy_stmt.query([])?;
        let mut copies = Vec::new();
        while let Some(row) = copy_rows.next()? {
            let lhs: String = row.get(0)?;
            let rhs: String = row.get(1)?;
            let method_fqn: String = row.get(2)?;
            copies.push((lhs, rhs, method_fqn));
        }
        drop(copy_rows);
        drop(copy_stmt);

        // Load field read assignments: lhs = FIELD_READ(rhs.field) -> represented as lhs = rhs.field
        let mut read_stmt = tx.prepare(
            "SELECT lhs, rhs, method_fqn FROM source_assignments WHERE assignment_type = 'FIELD_READ'",
        )?;
        let mut read_rows = read_stmt.query([])?;
        let mut field_reads = Vec::new();
        while let Some(row) = read_rows.next()? {
            let lhs: String = row.get(0)?;
            let rhs: String = row.get(1)?; // format: base#field
            let method_fqn: String = row.get(2)?;
            field_reads.push((lhs, rhs, method_fqn));
        }
        drop(read_rows);
        drop(read_stmt);

        // Load field write assignments: lhs.field = FIELD_WRITE(rhs) -> represented as lhs.field = rhs
        let mut write_stmt = tx.prepare(
            "SELECT lhs, rhs, method_fqn FROM source_assignments WHERE assignment_type = 'FIELD_WRITE'",
        )?;
        let mut write_rows = write_stmt.query([])?;
        let mut field_writes = Vec::new();
        while let Some(row) = write_rows.next()? {
            let lhs: String = row.get(0)?; // format: base#field
            let rhs: String = row.get(1)?;
            let method_fqn: String = row.get(2)?;
            field_writes.push((lhs, rhs, method_fqn));
        }
        drop(write_rows);
        drop(write_stmt);

        let mut active_contexts: HashMap<String, HashSet<String>> = HashMap::new();
        // Add default context "" for all method FQNs
        let mut method_stmt = tx.prepare(
            "SELECT DISTINCT method_fqn FROM source_assignments \
             UNION \
             SELECT DISTINCT method_fqn FROM call_sites \
             UNION \
             SELECT DISTINCT method_fqn FROM allocation_sites"
        )?;
        let mut method_rows = method_stmt.query([])?;
        while let Some(row) = method_rows.next()? {
            let m_fqn: String = row.get(0)?;
            active_contexts.entry(m_fqn).or_default().insert("".to_string());
        }
        drop(method_rows);
        drop(method_stmt);

        // Load class hierarchy for resolving virtual calls
        let mut hier_stmt =
            tx.prepare("SELECT class_fqn, parent_fqn FROM class_hierarchy")?;
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
        let mut decl_stmt = tx.prepare(
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
        let mut call_sites = Vec::new();
        let mut call_args: HashMap<String, Vec<(u32, String, String)>> = HashMap::new();

        let mut call_stmt = tx.prepare(
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

        let _disable_cfa = call_sites.len() > 1000;

        let mut arg_stmt = tx.prepare(
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
            tx.prepare("SELECT alloc_id, class_fqn FROM allocation_sites")?;
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
            tx.prepare("SELECT method_fqn, param_index FROM method_summaries")?;
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
        let mut supernode_stmt = tx.prepare(
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
                || extensions.iter().any(|ext| ext.is_supernode(target))
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
        let mut instance_fields: HashMap<((String, String), String), HashSet<(String, String)>> = HashMap::new();
        let mut call_edges_discovered: HashSet<(String, String, String, String, bool)> = HashSet::new();

        let mut vars_by_ctx: HashMap<String, HashSet<String>> = HashMap::new();
        let mut fields_by_aid: HashMap<String, HashSet<(String, String)>> = HashMap::new();
        let mut alloc_to_ctxs: HashMap<String, HashSet<String>> = HashMap::new();

        if has_existing_records {
            // Load existing points_to_sets
            let mut pts_stmt = tx.prepare(
                "SELECT variable_fqn, alloc_id, context, alloc_context FROM points_to_sets"
            )?;
            let mut pts_rows = pts_stmt.query([])?;
            let mut loaded_records = Vec::new();
            while let Some(row) = pts_rows.next()? {
                let variable_fqn: String = row.get(0)?;
                let alloc_id: String = row.get(1)?;
                let context: String = row.get(2)?;
                let alloc_context: String = row.get(3)?;

                let key = (context.clone(), variable_fqn.clone());
                let val = (alloc_context.clone(), alloc_id.clone());
                pts.entry(key.clone()).or_default().insert(val.clone());
                loaded_pts.insert((key, val));

                vars_by_ctx.entry(context.clone()).or_default().insert(variable_fqn.clone());
                alloc_to_ctxs.entry(alloc_id.clone()).or_default().insert(alloc_context.clone());

                if let Some(idx) = variable_fqn.find('#') {
                    let method_fqn = &variable_fqn[..idx];
                    active_contexts
                        .entry(method_fqn.to_string())
                        .or_default()
                        .insert(context.clone());
                }

                loaded_records.push((variable_fqn, alloc_id, context, alloc_context));
            }
            drop(pts_rows);
            drop(pts_stmt);

            // Populate instance_fields for field expressions
            for (variable_fqn, alloc_id, context, alloc_context) in &loaded_records {
                if let Some(hash_idx) = variable_fqn.find('#') {
                    let suffix = &variable_fqn[hash_idx + 1..];
                    if let Some(dot_idx) = suffix.find('.') {
                        let base_var = &variable_fqn[..hash_idx + 1 + dot_idx];
                        let field = &suffix[dot_idx + 1..];

                        let base_cs = (context.clone(), base_var.to_string());
                        if let Some(base_pts) = pts.get(&base_cs) {
                            for base_alloc in base_pts {
                                let alloc_key = (base_alloc.clone(), field.to_string());
                                let val = (alloc_context.clone(), alloc_id.clone());
                                instance_fields.entry(alloc_key.clone()).or_default().insert(val);
                                fields_by_aid.entry((alloc_key.0).1.clone()).or_default().insert(((alloc_key.0).0.clone(), alloc_key.1));
                            }
                        }
                    }
                }
            }

            // Load existing call_edges
            let mut edge_stmt = tx.prepare(
                "SELECT caller, callee, caller_context, callee_context, is_virtual FROM call_edges"
            )?;
            let mut edge_rows = edge_stmt.query([])?;
            let mut stripped_to_full: HashMap<String, Vec<String>> = HashMap::new();
            for full in active_contexts.keys() {
                stripped_to_full.entry(strip_signature(full).to_string()).or_default().push(full.clone());
            }

            while let Some(row) = edge_rows.next()? {
                let caller: String = row.get(0)?;
                let callee: String = row.get(1)?;
                let caller_context: String = row.get(2)?;
                let callee_context: String = row.get(3)?;
                let is_virtual_int: i32 = row.get(4)?;
                let is_virtual = is_virtual_int != 0;

                call_edges_discovered.insert((
                    caller_context.clone(),
                    caller.clone(),
                    callee_context.clone(),
                    callee.clone(),
                    is_virtual,
                ));

                loaded_edges.insert((
                    caller.clone(),
                    callee.clone(),
                    caller_context.clone(),
                    callee_context.clone(),
                    is_virtual_int,
                ));

                if let Some(fulls) = stripped_to_full.get(&caller) {
                    for full in fulls {
                        active_contexts
                            .entry(full.clone())
                            .or_default()
                            .insert(caller_context.clone());
                    }
                }
                if let Some(fulls) = stripped_to_full.get(&callee) {
                    for full in fulls {
                        active_contexts
                            .entry(full.clone())
                            .or_default()
                            .insert(callee_context.clone());
                    }
                }
            }
            drop(edge_rows);
            drop(edge_stmt);
        }

        macro_rules! insert_pts {
            ($pts:expr, $vars_by_ctx:expr, $alloc_to_ctxs:expr, $key:expr, $val:expr) => {{
                let k = $key;
                let v = $val;
                let inserted = $pts.entry(k.clone()).or_default().insert(v.clone());
                if inserted {
                    $vars_by_ctx.entry(k.0).or_default().insert(k.1);
                    $alloc_to_ctxs.entry(v.1).or_default().insert(v.0);
                }
                inserted
            }};
        }

        macro_rules! insert_field {
            ($instance_fields:expr, $fields_by_aid:expr, $key:expr, $val:expr) => {{
                let k = $key;
                let v = $val;
                let inserted = $instance_fields.entry(k.clone()).or_default().insert(v);
                if inserted {
                    $fields_by_aid.entry((k.0).1.clone()).or_default().insert(((k.0).0.clone(), k.1));
                }
                inserted
            }};
        }

        let mut iter_count = 0;
        macro_rules! set_changed {
            ($loc:expr) => {
                changed = true;
            };
        }
        while changed {
            iter_count += 1;
            changed = false;

            // 1. Process allocations: lhs = ALLOC(rhs)
            for (lhs, rhs, method_fqn) in &allocs {
                if let Some(ctxs) = active_contexts.get(method_fqn) {
                    for ctx in ctxs {
                        let lhs_cs = (ctx.clone(), lhs.clone());
                        let alloc_cs = (ctx.clone(), rhs.clone());
                        if insert_pts!(pts, vars_by_ctx, alloc_to_ctxs, lhs_cs, alloc_cs) {
                            set_changed!("allocation");
                        }
                    }
                }
            }

            // 2. Process copy assignments: lhs = COPY(rhs)
            for (lhs, rhs, method_fqn) in &copies {
                if let Some(ctxs) = active_contexts.get(method_fqn) {
                    for ctx in ctxs {
                        let rhs_cs = (ctx.clone(), rhs.clone());
                        if let Some(rhs_set) = pts.get(&rhs_cs) {
                            let lhs_cs = (ctx.clone(), lhs.clone());
                            let mut needs_insert = false;
                            if let Some(lhs_set) = pts.get(&lhs_cs) {
                                for val in rhs_set {
                                    if !lhs_set.contains(val) {
                                        needs_insert = true;
                                        break;
                                    }
                                }
                            } else {
                                needs_insert = true;
                            }
                            if needs_insert {
                                let rhs_set_clone = rhs_set.clone();
                                for val in rhs_set_clone {
                                    if insert_pts!(pts, vars_by_ctx, alloc_to_ctxs, lhs_cs.clone(), val) {
                                        set_changed!("copy");
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // 3. Process field writes: base_var#field = rhs_var
            for (lhs_field_expr, rhs, method_fqn) in &field_writes {
                if let Some(ctxs) = active_contexts.get(method_fqn) {
                    for ctx in ctxs {
                        if let Some(hash_idx) = lhs_field_expr.find('#') {
                            let suffix = &lhs_field_expr[hash_idx + 1..];
                            if let Some(dot_idx) = suffix.find('.') {
                                let base_var = &lhs_field_expr[..hash_idx + 1 + dot_idx];
                                let field = &suffix[dot_idx + 1..];

                                let base_cs = (ctx.clone(), base_var.to_string());
                                if let Some(base_pts) = pts.get(&base_cs) {
                                    let rhs_cs = (ctx.clone(), rhs.clone());
                                    if let Some(rhs_set) = pts.get(&rhs_cs) {
                                        let base_pts_clone = base_pts.clone();
                                        for alloc in base_pts_clone {
                                            let alloc_key = (alloc.clone(), field.to_string());
                                            let mut needs_insert = false;
                                            if let Some(field_set) = instance_fields.get(&alloc_key) {
                                                for val in rhs_set {
                                                    if !field_set.contains(val) {
                                                        needs_insert = true;
                                                        break;
                                                    }
                                                }
                                            } else {
                                                needs_insert = true;
                                            }
                                            if needs_insert {
                                                let rhs_set_clone = rhs_set.clone();
                                                for val in rhs_set_clone {
                                                    if insert_field!(instance_fields, fields_by_aid, alloc_key.clone(), val) {
                                                        set_changed!("field_write");
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

            // 4. Process field reads: lhs = base_var.field
            for (lhs, rhs_field_expr, method_fqn) in &field_reads {
                if let Some(ctxs) = active_contexts.get(method_fqn) {
                    for ctx in ctxs {
                        if let Some(hash_idx) = rhs_field_expr.find('#') {
                            let suffix = &rhs_field_expr[hash_idx + 1..];
                            if let Some(dot_idx) = suffix.find('.') {
                                let base_var = &rhs_field_expr[..hash_idx + 1 + dot_idx];
                                let field = &suffix[dot_idx + 1..];

                                let base_cs = (ctx.clone(), base_var.to_string());
                                if let Some(base_pts) = pts.get(&base_cs) {
                                    let base_pts_clone = base_pts.clone();
                                    for alloc in base_pts_clone {
                                        if let Some(field_set) = instance_fields.get(&(alloc, field.to_string())) {
                                            let lhs_cs = (ctx.clone(), lhs.clone());
                                            let mut needs_insert = false;
                                            if let Some(lhs_set) = pts.get(&lhs_cs) {
                                                for val in field_set {
                                                    if !lhs_set.contains(val) {
                                                        needs_insert = true;
                                                        break;
                                                    }
                                                }
                                            } else {
                                                needs_insert = true;
                                            }
                                            if needs_insert {
                                                let field_clone = field_set.clone();
                                                for val in field_clone {
                                                    if insert_pts!(pts, vars_by_ctx, alloc_to_ctxs, lhs_cs.clone(), val) {
                                                        set_changed!("field_read");
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

            // 3c. Map Spring DI bean allocation facts to points-to propagation under k-CFA
            for ((_key_ctx, key_var), key_pts) in &pts {
                if key_var.starts_with("SpringBeanAlloc:") {
                    let stripped = &key_var["SpringBeanAlloc:".len()..];
                    if let Some(dot_idx) = stripped.rfind('.') {
                        let class_fqn = &stripped[..dot_idx];
                        let field_name = &stripped[dot_idx + 1..];
                        if let Some(alloc_ids) = allocs_by_class.get(class_fqn) {
                            for alloc_id in alloc_ids {
                                // Spring beans have allocation context ""
                                let alloc_key = (("".to_string(), alloc_id.clone()), field_name.to_string());
                                let mut needs_insert = false;
                                if let Some(field_set) = instance_fields.get(&alloc_key) {
                                    for val in key_pts {
                                        if !field_set.contains(val) {
                                            needs_insert = true;
                                            break;
                                        }
                                    }
                                } else {
                                    needs_insert = true;
                                }
                                if needs_insert {
                                    let key_pts_clone = key_pts.clone();
                                    for val in key_pts_clone {
                                        if insert_field!(instance_fields, fields_by_aid, alloc_key.clone(), val.clone()) {
                                            set_changed!("spring_di");
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // 3b. Map solver extension propagation facts
            for ext in extensions {
                if ext.needs_propagation() {
                    let mut temp_pts: HashMap<String, HashSet<String>> = HashMap::new();
                    for ((_ctx, var), allocs) in &pts {
                        let entry = temp_pts.entry(var.clone()).or_default();
                        for (_, aid) in allocs {
                            entry.insert(aid.clone());
                        }
                    }

                    let mut temp_instance_fields: HashMap<(String, String), HashSet<String>> = HashMap::new();
                    for (((_, aid), field), target_allocs) in &instance_fields {
                        let entry = temp_instance_fields.entry((aid.clone(), field.clone())).or_default();
                        for (_, taid) in target_allocs {
                            entry.insert(taid.clone());
                        }
                    }

                    let mut temp_call_edges_discovered: HashSet<(String, String, bool)> = HashSet::new();
                    for (_caller_ctx, caller, _callee_ctx, callee, is_virt) in &call_edges_discovered {
                        temp_call_edges_discovered.insert((caller.clone(), callee.clone(), *is_virt));
                    }

                    let mut ext_ctx = ExtensionContext {
                        conn: &tx,
                        pts: &mut temp_pts,
                        instance_fields: &mut temp_instance_fields,
                        call_edges_discovered: &mut temp_call_edges_discovered,
                        alloc_types: &alloc_types,
                        allocs_by_class: &allocs_by_class,
                        ancestors_map: &ancestors_map,
                        decl_by_name_params: &decl_by_name_params,
                        decl_by_class_name: &decl_by_class_name,
                        summaries: &summaries,
                        call_args: &call_args,
                    };

                    if ext.propagate(&mut ext_ctx)? {
                        // No longer setting changed = true unconditionally to prevent infinite loop.
                        // Instead, the reconciliation of temp_pts and temp_call_edges_discovered below will set it.

                        for (var, allocs) in temp_pts {
                            let dest_cs = ("".to_string(), var);
                            for alloc_id in allocs {
                                let alloc_ctxs = if alloc_id.starts_with("SpringBeanAlloc:") {
                                    let mut s = HashSet::new();
                                    s.insert("".to_string());
                                    s
                                } else if let Some(ctxs) = alloc_to_ctxs.get(&alloc_id) {
                                    ctxs.clone()
                                } else {
                                    let mut s = HashSet::new();
                                    s.insert("".to_string());
                                    s
                                };
                                for alloc_ctx in alloc_ctxs {
                                    let alloc_cs = (alloc_ctx, alloc_id.clone());
                                    if insert_pts!(pts, vars_by_ctx, alloc_to_ctxs, dest_cs.clone(), alloc_cs) {
                                        changed = true;
                                    }
                                }
                            }
                        }

                        for (c_caller_str, c_callee_str, is_virt) in temp_call_edges_discovered {
                            if call_edges_discovered.insert((
                                "".to_string(),
                                c_caller_str.clone(),
                                "".to_string(),
                                c_callee_str.clone(),
                                is_virt,
                            )) {
                                changed = true;
                                active_contexts.entry(c_callee_str).or_default().insert("".to_string());
                            }
                        }
                    }
                }
            }

            // 4. Resolve call sites virtually & statically
            for (call_id, caller_method_fqn, receiver, method_name, lhs, static_callee) in
                &call_sites
            {
                if let Some(ctxs) = active_contexts.get(caller_method_fqn) {
                    let ctxs_clone = ctxs.clone();
                    for C_caller in ctxs_clone {
                        let mut resolved_targets = HashSet::new();

                        let rec_pts_clone = if let Some(ref rec_var) = receiver {
                            let rec_cs = (C_caller.clone(), rec_var.clone());
                            if let Some(rec_pts) = pts.get(&rec_cs) {
                                Some(rec_pts.clone())
                            } else {
                                None
                            }
                        } else {
                            None
                        };

                        if let Some(rec_pts) = rec_pts_clone {
                            for (_alloc_ctx, alloc_id) in rec_pts {
                                if let Some(alloc_type) = alloc_types.get(&alloc_id) {
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
                            let mut resolved = sc.clone();
                            if let Some(start) = sc.find('(') {
                                let prefix = &sc[..start];
                                if let Some(last_dot) = prefix.rfind('.') {
                                    let class_fqn = &prefix[..last_dot];
                                    let method_name = &prefix[last_dot + 1..];
                                    let params_content = if let Some(end) = sc.rfind(')') {
                                        sc[start + 1..end].trim()
                                    } else {
                                        ""
                                    };
                                    let arg_count = if params_content.is_empty() {
                                        0
                                    } else {
                                        params_content.split(',').count()
                                    };

                                    if let Some(declared_methods) = decl_by_class_name.get(&(class_fqn.to_string(), method_name.to_string())) {
                                        for decl_fqn in declared_methods {
                                            if get_param_count(decl_fqn) == arg_count {
                                                resolved = decl_fqn.clone();
                                                break;
                                            }
                                        }
                                    }
                                }
                            }
                            resolved_targets.insert((resolved, false));
                        }

                        let call_info = CallSiteInfo {
                            call_id,
                            caller_method_fqn,
                            receiver: receiver.as_deref(),
                            method_name,
                            lhs: lhs.as_deref(),
                            static_callee: static_callee.as_deref(),
                        };

                        let mut handled_by_ext = false;

                        for ext in extensions {
                            if !ext.matches_call_site(&call_info) {
                                continue;
                            }
                            let needs_pts = ext.needs_points_to();

                            let mut temp_pts: HashMap<String, HashSet<String>> = HashMap::new();
                            let C_callee = if _disable_cfa {
                                "".to_string()
                            } else if C_caller.is_empty() {
                                call_id.clone()
                            } else {
                                "".to_string()
                            };
                            let mut aids = HashSet::new();
                            let mut temp_instance_fields: HashMap<(String, String), HashSet<String>> = HashMap::new();

                            if needs_pts {
                                let belongs_to_call = |var: &str| -> bool {
                                    var.starts_with(caller_method_fqn)
                                        || resolved_targets.iter().any(|(target, _)| var.starts_with(target))
                                };

                                let mut process_var = |ctx: &str, var: &str| {
                                    if belongs_to_call(var) {
                                        if let Some(allocs) = pts.get(&(ctx.to_string(), var.to_string())) {
                                            let entry = temp_pts.entry(var.to_string()).or_default();
                                            for (_, aid) in allocs {
                                                entry.insert(aid.clone());
                                                aids.insert(aid.clone());
                                            }
                                        }
                                    }
                                };

                                if let Some(vars) = vars_by_ctx.get(&C_caller) {
                                    for var in vars {
                                        process_var(&C_caller, var);
                                    }
                                }
                                if C_caller != C_callee {
                                    if let Some(vars) = vars_by_ctx.get(&C_callee) {
                                        for var in vars {
                                            process_var(&C_callee, var);
                                        }
                                    }
                                }

                                for aid in &aids {
                                    if let Some(entries) = fields_by_aid.get(aid) {
                                        for (alloc_ctx, field) in entries {
                                            let key = ((alloc_ctx.clone(), aid.clone()), field.clone());
                                            if let Some(target_allocs) = instance_fields.get(&key) {
                                                let entry = temp_instance_fields.entry((aid.clone(), field.clone())).or_default();
                                                for (_, taid) in target_allocs {
                                                    entry.insert(taid.clone());
                                                }
                                            }
                                        }
                                    }
                                }
                            }

                            let mut temp_call_edges_discovered: HashSet<(String, String, bool)> = HashSet::new();

                            let mut ext_ctx = ExtensionContext {
                                conn: &tx,
                                pts: &mut temp_pts,
                                instance_fields: &mut temp_instance_fields,
                                call_edges_discovered: &mut temp_call_edges_discovered,
                                alloc_types: &alloc_types,
                                allocs_by_class: &allocs_by_class,
                                ancestors_map: &ancestors_map,
                                decl_by_name_params: &decl_by_name_params,
                                decl_by_class_name: &decl_by_class_name,
                                summaries: &summaries,
                                call_args: &call_args,
                            };

                            let ext_res = ext.handle_call(&mut ext_ctx, &call_info, &resolved_targets)?;

                            if !temp_pts.is_empty() || !temp_call_edges_discovered.is_empty() {
                                for (var, allocs) in temp_pts {
                                    let mut dest_ctx = C_caller.clone();
                                    for (target, _) in &resolved_targets {
                                        if var.starts_with(&format!("{}#", target)) || var.starts_with(target) {
                                            dest_ctx = C_callee.clone();
                                            break;
                                        }
                                    }
                                    let dest_cs = (dest_ctx, var);
                                    for alloc_id in allocs {
                                        let alloc_ctxs = if alloc_id.starts_with("SpringBeanAlloc:") {
                                            let mut s = HashSet::new();
                                            s.insert("".to_string());
                                            s
                                        } else if let Some(ctxs) = alloc_to_ctxs.get(&alloc_id) {
                                            ctxs.clone()
                                        } else {
                                            let mut s = HashSet::new();
                                            s.insert(C_caller.clone());
                                            s
                                        };
                                        for alloc_ctx in alloc_ctxs {
                                            let alloc_cs = (alloc_ctx, alloc_id.clone());
                                            if insert_pts!(pts, vars_by_ctx, alloc_to_ctxs, dest_cs.clone(), alloc_cs) {
                                                changed = true;
                                            }
                                        }
                                    }
                                }

                                for (c_caller_str, c_callee_str, is_virt) in temp_call_edges_discovered {
                                    if call_edges_discovered.insert((
                                        C_caller.clone(),
                                        c_caller_str.clone(),
                                        C_callee.clone(),
                                        c_callee_str.clone(),
                                        is_virt,
                                    )) {
                                        changed = true;
                                        active_contexts.entry(c_callee_str).or_default().insert(C_callee.clone());
                                    }
                                }
                            }

                            if let Some(ext_changed) = ext_res {
                                handled_by_ext = true;
                                if ext_changed {
                                    // set changed in reconciliation above
                                }
                                break;
                            }
                        }

                        if handled_by_ext {
                            continue;
                        }

                        // Standard propagation
                        for (target, is_virt) in resolved_targets {
                            let C_callee = if _disable_cfa {
                                "".to_string()
                            } else if C_caller.is_empty() {
                                call_id.clone()
                            } else {
                                "".to_string()
                            };
                            if call_edges_discovered.insert((
                                C_caller.clone(),
                                strip_signature(caller_method_fqn).to_string(),
                                C_callee.clone(),
                                strip_signature(&target).to_string(),
                                is_virt,
                            )) {
                                set_changed!("standard_call_edges");
                                active_contexts.entry(target.clone()).or_default().insert(C_callee.clone());
                            }

                            if is_supernode(&target) {
                                if let Some(ref return_var) = lhs {
                                    let supernode_alloc = format!("SupernodeReturn:{}", target);
                                    let return_cs = (C_caller.clone(), return_var.clone());
                                    let supernode_cs = ("".to_string(), supernode_alloc);
                                    if insert_pts!(pts, vars_by_ctx, alloc_to_ctxs, return_cs, supernode_cs) {
                                        set_changed!("supernode_return");
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
                                                let arg_cs = (C_caller.clone(), arg_var.clone());
                                                if let Some(arg_pts) = pts.get(&arg_cs) {
                                                    let arg_pts_clone = arg_pts.clone();
                                                    let return_cs = (C_caller.clone(), return_var.clone());
                                                    for val in arg_pts_clone {
                                                        if insert_pts!(pts, vars_by_ctx, alloc_to_ctxs, return_cs.clone(), val) {
                                                            set_changed!("summary_param");
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                                continue;
                            }

                            let mut propagations = Vec::new();
                            if let Some(ref rec_var) = receiver {
                                let rec_cs = (C_caller.clone(), rec_var.clone());
                                if let Some(rec_pts) = pts.get(&rec_cs) {
                                    propagations.push(((C_callee.clone(), format!("{}#this", target)), rec_pts.clone()));
                                }
                            }
                            if let Some(args) = call_args.get(call_id) {
                                for (idx, arg_var, _) in args {
                                    let arg_cs = (C_caller.clone(), arg_var.clone());
                                    if let Some(arg_pts) = pts.get(&arg_cs) {
                                        propagations
                                            .push(((C_callee.clone(), format!("{}#p{}", target, idx)), arg_pts.clone()));
                                    }
                                }
                            }
                            if let Some(ref return_var) = lhs {
                                let target_return_cs = (C_callee.clone(), format!("{}#return", target));
                                if let Some(ret_pts) = pts.get(&target_return_cs) {
                                    propagations.push(((C_caller.clone(), return_var.clone()), ret_pts.clone()));
                                }
                            }

                            for (to_cs, vals) in propagations {
                                let mut needs_insert = false;
                                if let Some(param_set) = pts.get(&to_cs) {
                                    for val in &vals {
                                        if !param_set.contains(val) {
                                            needs_insert = true;
                                            break;
                                        }
                                    }
                                } else {
                                    needs_insert = true;
                                }
                                if needs_insert {
                                    for val in vals {
                                        if insert_pts!(pts, vars_by_ctx, alloc_to_ctxs, to_cs.clone(), val) {
                                            set_changed!("param_propagation");
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            println!("Solver iter {} finished, pts size = {}", iter_count, pts.len());
        }

        // Persist points-to sets back to database
        let mut pts_to_insert = Vec::new();
        for ((ctx, var), allocs) in &pts {
            for (actx, alloc) in allocs {
                let key = (ctx.clone(), var.clone());
                let val = (actx.clone(), alloc.clone());
                if !loaded_pts.contains(&(key, val)) {
                    pts_to_insert.push((var, alloc, ctx, actx));
                }
            }
        }

        // Also persist points-to sets for field expressions
        for (lhs_field_expr, _rhs, method_fqn) in &field_writes {
            if let Some(ctxs) = active_contexts.get(method_fqn) {
                for ctx in ctxs {
                    if let Some(hash_idx) = lhs_field_expr.find('#') {
                        let suffix = &lhs_field_expr[hash_idx + 1..];
                        if let Some(dot_idx) = suffix.find('.') {
                            let base_var = &lhs_field_expr[..hash_idx + 1 + dot_idx];
                            let field = &suffix[dot_idx + 1..];
                            let base_cs = (ctx.clone(), base_var.to_string());
                            if let Some(base_pts) = pts.get(&base_cs) {
                                for alloc in base_pts {
                                    if let Some(field_set) = instance_fields.get(&(alloc.clone(), field.to_string())) {
                                        for val in field_set {
                                            let key = (ctx.clone(), lhs_field_expr.clone());
                                            let val_entry = (val.0.clone(), val.1.clone());
                                            if !loaded_pts.contains(&(key, val_entry)) {
                                                pts_to_insert.push((lhs_field_expr, &val.1, ctx, &val.0));
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

        for (_lhs, rhs_field_expr, method_fqn) in &field_reads {
            if let Some(ctxs) = active_contexts.get(method_fqn) {
                for ctx in ctxs {
                    if let Some(hash_idx) = rhs_field_expr.find('#') {
                        let suffix = &rhs_field_expr[hash_idx + 1..];
                        if let Some(dot_idx) = suffix.find('.') {
                            let base_var = &rhs_field_expr[..hash_idx + 1 + dot_idx];
                            let field = &suffix[dot_idx + 1..];
                            let base_cs = (ctx.clone(), base_var.to_string());
                            if let Some(base_pts) = pts.get(&base_cs) {
                                for alloc in base_pts {
                                    if let Some(field_set) = instance_fields.get(&(alloc.clone(), field.to_string())) {
                                        for val in field_set {
                                            let key = (ctx.clone(), rhs_field_expr.clone());
                                            let val_entry = (val.0.clone(), val.1.clone());
                                            if !loaded_pts.contains(&(key, val_entry)) {
                                                pts_to_insert.push((rhs_field_expr, &val.1, ctx, &val.0));
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
        pts_to_insert.sort_unstable_by(|a, b| {
            a.2.cmp(b.2)
                .then_with(|| a.0.cmp(b.0))
                .then_with(|| a.3.cmp(b.3))
                .then_with(|| a.1.cmp(b.1))
        });

        const PTS_CHUNK_SIZE: usize = 200;
        let mut query_pts_full = String::from(
            "INSERT OR REPLACE INTO points_to_sets (variable_fqn, alloc_id, context, alloc_context) VALUES ",
        );
        for i in 0..PTS_CHUNK_SIZE {
            if i > 0 {
                query_pts_full.push_str(", ");
            }
            query_pts_full.push_str(&format!("(?{}, ?{}, ?{}, ?{})", i * 4 + 1, i * 4 + 2, i * 4 + 3, i * 4 + 4));
        }
        let mut stmt_pts_full = tx.prepare(&query_pts_full)?;

        for chunk in pts_to_insert.chunks(PTS_CHUNK_SIZE) {
            if chunk.len() == PTS_CHUNK_SIZE {
                let mut params: [&dyn rusqlite::ToSql; PTS_CHUNK_SIZE * 4] =
                    [&"" as &dyn rusqlite::ToSql; PTS_CHUNK_SIZE * 4];
                for (i, item) in chunk.iter().enumerate() {
                    params[i * 4] = item.0;
                    params[i * 4 + 1] = item.1;
                    params[i * 4 + 2] = item.2;
                    params[i * 4 + 3] = item.3;
                }
                stmt_pts_full.execute(&params[..])?;
            } else {
                let mut query_pts_last = String::from(
                    "INSERT OR REPLACE INTO points_to_sets (variable_fqn, alloc_id, context, alloc_context) VALUES ",
                );
                for i in 0..chunk.len() {
                    if i > 0 {
                        query_pts_last.push_str(", ");
                    }
                    query_pts_last.push_str(&format!("(?{}, ?{}, ?{}, ?{})", i * 4 + 1, i * 4 + 2, i * 4 + 3, i * 4 + 4));
                }
                let mut stmt_pts_last = tx.prepare(&query_pts_last)?;
                let mut params: Vec<&dyn rusqlite::ToSql> = Vec::with_capacity(chunk.len() * 4);
                for item in chunk {
                    params.push(item.0);
                    params.push(item.1);
                    params.push(item.2);
                    params.push(item.3);
                }
                stmt_pts_last.execute(rusqlite::params_from_iter(params))?;
            }
        }

        // Clear existing call edges and persist newly resolved call edges
        if !has_existing_records {
            tx.execute("DELETE FROM call_edges", [])?;
        }
        let mut edges_to_insert = Vec::new();
        for (caller_ctx, caller, callee_ctx, callee, is_virt) in call_edges_discovered {
            let is_virt_int = if is_virt { 1 } else { 0 };
            let entry = (caller.clone(), callee.clone(), caller_ctx.clone(), callee_ctx.clone(), is_virt_int);
            if !loaded_edges.contains(&entry) {
                edges_to_insert.push((caller, callee, caller_ctx, callee_ctx, is_virt_int));
            }
        }
        edges_to_insert.sort_unstable_by(|a, b| {
            a.2.cmp(&b.2)
                .then_with(|| a.0.cmp(&b.0))
                .then_with(|| a.3.cmp(&b.3))
                .then_with(|| a.1.cmp(&b.1))
        });

        const EDGE_CHUNK_SIZE: usize = 150;
        let mut query_edge_full = String::from(
            "INSERT OR IGNORE INTO call_edges (caller, callee, caller_context, callee_context, is_virtual) VALUES ",
        );
        for i in 0..EDGE_CHUNK_SIZE {
            if i > 0 {
                query_edge_full.push_str(", ");
            }
            query_edge_full.push_str(&format!(
                "(?{}, ?{}, ?{}, ?{}, ?{})",
                i * 5 + 1,
                i * 5 + 2,
                i * 5 + 3,
                i * 5 + 4,
                i * 5 + 5
            ));
        }
        let mut stmt_edge_full = tx.prepare(&query_edge_full)?;

        for chunk in edges_to_insert.chunks(EDGE_CHUNK_SIZE) {
            if chunk.len() == EDGE_CHUNK_SIZE {
                let mut params: [&dyn rusqlite::ToSql; EDGE_CHUNK_SIZE * 5] =
                    [&"" as &dyn rusqlite::ToSql; EDGE_CHUNK_SIZE * 5];
                for (i, item) in chunk.iter().enumerate() {
                    params[i * 5] = &item.0;
                    params[i * 5 + 1] = &item.1;
                    params[i * 5 + 2] = &item.2;
                    params[i * 5 + 3] = &item.3;
                    params[i * 5 + 4] = &item.4;
                }
                stmt_edge_full.execute(&params[..])?;
            } else {
                let mut query_edge_last = String::from(
                    "INSERT OR IGNORE INTO call_edges (caller, callee, caller_context, callee_context, is_virtual) VALUES ",
                );
                for i in 0..chunk.len() {
                    if i > 0 {
                        query_edge_last.push_str(", ");
                    }
                    query_edge_last.push_str(&format!(
                        "(?{}, ?{}, ?{}, ?{}, ?{})",
                        i * 5 + 1,
                        i * 5 + 2,
                        i * 5 + 3,
                        i * 5 + 4,
                        i * 5 + 5
                    ));
                }
                let mut stmt_edge_last = tx.prepare(&query_edge_last)?;
                let mut params: Vec<&dyn rusqlite::ToSql> = Vec::with_capacity(chunk.len() * 5);
                for item in chunk {
                    params.push(&item.0);
                    params.push(&item.1);
                    params.push(&item.2);
                    params.push(&item.3);
                    params.push(&item.4);
                }
                stmt_edge_last.execute(rusqlite::params_from_iter(params))?;
            }
        }

        drop(stmt_pts_full);
        drop(stmt_edge_full);
        tx.commit()?;
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
        let mut conn = rusqlite::Connection::open_in_memory().unwrap();
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
        solver.solve(&mut conn, &[]).unwrap();

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
