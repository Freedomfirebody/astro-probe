use std::collections::{HashMap, HashSet};
use rusqlite::Connection;
use anyhow::Result;

pub struct CallGraphAnalyzer;

impl CallGraphAnalyzer {
    pub fn new() -> Self {
        Self
    }

    pub fn analyze(&self, conn: &Connection) -> Result<()> {
        let mut solver = PointsToSolver::new();
        solver.solve(conn)?;
        Ok(())
    }
}

impl Default for CallGraphAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AllocSite {
    pub id: String,
    pub type_name: String,
}

#[derive(Clone)]
pub struct CallSiteInfo {
    pub call_id: String,
    pub method_fqn: String,
    pub receiver: Option<String>,
    pub method_name: String,
    pub lhs: Option<String>,
    pub arguments: Vec<String>,
    pub argument_types: Vec<String>,
    pub static_callee: Option<String>,
}

pub struct PointsToSolver {
    pub hierarchy: HashMap<String, Vec<String>>,
    pub class_kinds: HashMap<String, String>,
    pub method_declarations: HashMap<String, (String, String, Vec<String>)>,
    pub pts: HashMap<String, HashSet<AllocSite>>,
    pub copy_edges: HashMap<String, HashSet<String>>,
    pub field_writes: HashMap<String, Vec<(String, String)>>,
    pub field_reads: HashMap<String, Vec<(String, String)>>,
    pub call_sites: Vec<CallSiteInfo>,
    pub resolved_calls: HashMap<String, HashSet<String>>,
    pub worklist: Vec<String>,
    pub library_classes: HashSet<String>,
}

impl PointsToSolver {
    pub fn new() -> Self {
        Self {
            hierarchy: HashMap::new(),
            class_kinds: HashMap::new(),
            method_declarations: HashMap::new(),
            pts: HashMap::new(),
            copy_edges: HashMap::new(),
            field_writes: HashMap::new(),
            field_reads: HashMap::new(),
            call_sites: Vec::new(),
            resolved_calls: HashMap::new(),
            worklist: Vec::new(),
            library_classes: HashSet::new(),
        }
    }

    pub fn solve(&mut self, conn: &Connection) -> Result<()> {
        // Load facts from database
        self.initialize_facts(conn)?;

        // Run fixed-point solver
        while let Some(u) = self.worklist.pop() {
            let pts_u = match self.pts.get(&u) {
                Some(s) if !s.is_empty() => s.clone(),
                _ => continue,
            };

            // 1. Copy Propagation: For each edge u -> v (pt(u) <= pt(v))
            if let Some(targets) = self.copy_edges.get(&u) {
                for v in targets.clone() {
                    if self.propagate(&pts_u, &v) {
                        self.worklist.push(v);
                    }
                }
            }

            // 2. Field Writes (Store): For u.f = y
            if let Some(writes) = self.field_writes.get(&u) {
                for (field_name, source) in writes.clone() {
                    for o_i in &pts_u {
                        let field_node = format!("{}.{}", o_i.id, field_name);
                        self.add_copy_edge_and_propagate(source.clone(), field_node);
                    }
                }
            }

            // 3. Field Reads (Load): For y = u.f
            if let Some(reads) = self.field_reads.get(&u) {
                for (field_name, dest) in reads.clone() {
                    for o_i in &pts_u {
                        let field_node = format!("{}.{}", o_i.id, field_name);
                        self.add_copy_edge_and_propagate(field_node, dest.clone());
                    }
                }
            }

            // 4. Dynamic Method Dispatch Resolution
            let call_sites = self.call_sites.clone();
            for call_info in &call_sites {
                if let Some(ref rec) = call_info.receiver {
                    if *rec == u {
                        for o_i in &pts_u {
                            if let Some(callee_fqn) = self.resolve_dispatch(&o_i.type_name, &call_info.method_name, &call_info.argument_types) {
                                // 4a. Bind receiver to `this` (Parameter 0)
                                let this_node = format!("{}#this", callee_fqn);
                                let mut o_i_set = HashSet::new();
                                o_i_set.insert(o_i.clone());
                                if self.propagate(&o_i_set, &this_node) {
                                    self.worklist.push(this_node);
                                }

                                if self.resolved_calls.entry(call_info.call_id.clone()).or_default().insert(callee_fqn.clone()) {
                                    // 4b. Bind arguments to parameters
                                    if let Some((_, _, callee_params)) = self.method_declarations.get(&callee_fqn).cloned() {
                                        for (i, arg) in call_info.arguments.iter().enumerate() {
                                            if i < callee_params.len() {
                                                let param_node = format!("{}#{}", callee_fqn, callee_params[i]);
                                                self.add_copy_edge_and_propagate(arg.clone(), param_node);
                                            }
                                            let pos_node = format!("{}#p{}", callee_fqn, i);
                                            self.add_copy_edge_and_propagate(arg.clone(), pos_node);
                                        }
                                    } else {
                                        for (i, arg) in call_info.arguments.iter().enumerate() {
                                            let pos_node = format!("{}#p{}", callee_fqn, i);
                                            self.add_copy_edge_and_propagate(arg.clone(), pos_node);
                                        }
                                    }

                                    // 4c. Bind return value to LHS
                                    if let Some(ref lhs) = call_info.lhs {
                                        let return_node = format!("{}#return", callee_fqn);
                                        self.add_copy_edge_and_propagate(return_node, lhs.clone());
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // 5. Reflection propagation
            self.propagate_reflection(&u, &pts_u);
        }

        // Save resolved calls and points-to sets back to SQLite
        self.persist_results(conn)?;

        Ok(())
    }

    fn initialize_facts(&mut self, conn: &Connection) -> Result<()> {
        let mut stmt = conn.prepare("SELECT fqn FROM library_classes")?;
        let library_class_rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        for row in library_class_rows {
            self.library_classes.insert(row?);
        }

        // Load class hierarchy
        let mut stmt = conn.prepare("SELECT class_fqn, parent_fqn FROM class_hierarchy")?;
        let hierarchy_rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        for row in hierarchy_rows {
            let (class_fqn, parent_fqn) = row?;
            let parents = self.hierarchy.entry(class_fqn).or_default();
            if !parents.contains(&parent_fqn) {
                parents.push(parent_fqn);
            }
        }

        for parents in self.hierarchy.values_mut() {
            parents.sort();
        }

        // Load class kinds
        let mut stmt = conn.prepare("SELECT fqn, kind FROM classes")?;
        let class_rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        for row in class_rows {
            let (fqn, kind) = row?;
            self.class_kinds.insert(fqn, kind);
        }

        // Load method declarations
        let mut stmt = conn.prepare("SELECT method_fqn, class_fqn, method_name, params FROM method_declarations")?;
        let method_rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?;
        for row in method_rows {
            let (method_fqn, class_fqn, method_name, params_str) = row?;
            let params = if params_str.is_empty() {
                Vec::new()
            } else {
                params_str.split(',').map(|s| s.to_string()).collect()
            };
            self.method_declarations.insert(method_fqn, (class_fqn, method_name, params));
        }

        // Load allocation sites
        let mut stmt = conn.prepare("SELECT alloc_id, class_fqn FROM allocation_sites")?;
        let alloc_rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        let mut alloc_map = HashMap::new();
        for row in alloc_rows {
            let (alloc_id, class_fqn) = row?;
            alloc_map.insert(alloc_id, class_fqn);
        }

        // Load source assignments
        let mut stmt = conn.prepare("SELECT lhs, rhs, assignment_type FROM source_assignments")?;
        let assignment_rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?))
        })?;
        for row in assignment_rows {
            let (lhs, rhs, assignment_type) = row?;
            match assignment_type.as_str() {
                "ALLOC" => {
                    if let Some(class_fqn) = alloc_map.get(&rhs) {
                        let alloc_site = AllocSite {
                            id: rhs.clone(),
                            type_name: class_fqn.clone(),
                        };
                        self.pts.entry(lhs.clone()).or_default().insert(alloc_site);
                    }
                }
                "COPY" => {
                    self.copy_edges.entry(rhs).or_default().insert(lhs);
                }
                "FIELD_READ" => {
                    if let Some(hash_idx) = rhs.rfind('#') {
                        let left = &rhs[..hash_idx];
                        let right = &rhs[hash_idx + 1..];
                        if let Some(dot_idx) = right.find('.') {
                            let base = &right[..dot_idx];
                            let field_name = &right[dot_idx + 1..];
                            let base_var = format!("{}#{}", left, base);
                            self.field_reads.entry(base_var).or_default().push((field_name.to_string(), lhs));
                        }
                    }
                }
                "FIELD_WRITE" => {
                    if let Some(hash_idx) = lhs.rfind('#') {
                        let left = &lhs[..hash_idx];
                        let right = &lhs[hash_idx + 1..];
                        if let Some(dot_idx) = right.find('.') {
                            let base = &right[..dot_idx];
                            let field_name = &right[dot_idx + 1..];
                            let base_var = format!("{}#{}", left, base);
                            self.field_writes.entry(base_var).or_default().push((field_name.to_string(), rhs));
                        }
                    }
                }
                _ => {}
            }
        }

        // Initialize worklist with all nodes having initial allocations
        for u in self.pts.keys() {
            self.worklist.push(u.clone());
        }

        // Load call sites
        let mut stmt = conn.prepare("SELECT call_id, method_fqn, receiver, method_name, lhs, static_callee FROM call_sites")?;
        let call_rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, Option<String>>(5)?,
            ))
        })?;
        let mut call_sites = Vec::new();
        for row in call_rows {
            let (call_id, method_fqn, receiver, method_name, lhs, static_callee) = row?;
            
            let mut arg_stmt = conn.prepare("SELECT arg_var, arg_type FROM call_arguments WHERE call_id = ?1 ORDER BY arg_index ASC")?;
            let arg_rows = arg_stmt.query_map([&call_id], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?))
            })?;
            let mut arguments = Vec::new();
            let mut argument_types = Vec::new();
            for arg_res in arg_rows {
                let (arg_var, arg_type) = arg_res?;
                arguments.push(arg_var);
                argument_types.push(arg_type.unwrap_or_else(|| "java.lang.Object".to_string()));
            }

            call_sites.push(CallSiteInfo {
                call_id,
                method_fqn,
                receiver,
                method_name,
                lhs,
                arguments,
                argument_types,
                static_callee,
            });
        }
        self.call_sites = call_sites;

        Ok(())
    }
}

fn parse_param_types(method_fqn: &str) -> Vec<String> {
    if let Some(start) = method_fqn.find('(') {
        if let Some(end) = method_fqn.find(')') {
            let inner = &method_fqn[start + 1..end];
            if inner.is_empty() {
                return Vec::new();
            }
            return inner.split(',').map(|s| s.trim().to_string()).collect();
        }
    }
    Vec::new()
}

fn box_type(t: &str) -> &str {
    match t {
        "int" => "java.lang.Integer",
        "double" => "java.lang.Double",
        "float" => "java.lang.Float",
        "long" => "java.lang.Long",
        "short" => "java.lang.Short",
        "byte" => "java.lang.Byte",
        "char" => "java.lang.Character",
        "boolean" => "java.lang.Boolean",
        other => other,
    }
}

fn is_subtype(sub: &str, super_type: &str, hierarchy: &HashMap<String, Vec<String>>) -> bool {
    let sub = box_type(sub);
    let super_type = box_type(super_type);
    
    if sub == super_type {
        return true;
    }
    if super_type == "java.lang.Object" {
        return true;
    }
    let mut queue = vec![sub.to_string()];
    let mut visited = HashSet::new();
    while let Some(curr) = queue.pop() {
        if curr == super_type {
            return true;
        }
        if !visited.insert(curr.clone()) {
            continue;
        }
        if let Some(parents) = hierarchy.get(&curr) {
            for parent in parents {
                queue.push(parent.clone());
            }
        }
    }
    false
}

impl PointsToSolver {
    fn resolve_dispatch(&self, class_fqn: &str, method_name: &str, arg_types: &[String]) -> Option<String> {
        let mut class_queue = std::collections::VecDeque::new();
        let mut interface_queue = std::collections::VecDeque::new();
        class_queue.push_back(class_fqn.to_string());
        
        let mut visited = HashSet::new();

        // 1. Process classes
        while let Some(current) = class_queue.pop_front() {
            if !visited.insert(current.clone()) {
                continue;
            }

            let mut candidates = Vec::new();
            for (method_fqn, (decl_class, decl_name, _)) in &self.method_declarations {
                if decl_class == &current && decl_name == method_name {
                    let param_types = parse_param_types(method_fqn);
                    if param_types.len() == arg_types.len() {
                        let mut compatible = true;
                        for (arg_type, param_type) in arg_types.iter().zip(param_types.iter()) {
                            if arg_type != "java.lang.Object" && !is_subtype(arg_type, param_type, &self.hierarchy) {
                                compatible = false;
                                break;
                            }
                        }
                        if compatible {
                            candidates.push((method_fqn.clone(), param_types));
                        }
                    }
                }
            }

            if !candidates.is_empty() {
                if candidates.len() == 1 {
                    return Some(candidates[0].0.clone());
                }
                let mut best_idx = 0;
                for i in 1..candidates.len() {
                    let mut better = true;
                    for j in 0..arg_types.len() {
                        let type_best = &candidates[best_idx].1[j];
                        let type_curr = &candidates[i].1[j];
                        if !is_subtype(type_curr, type_best, &self.hierarchy) {
                            better = false;
                            break;
                        }
                    }
                    if better {
                        best_idx = i;
                    }
                }
                return Some(candidates[best_idx].0.clone());
            }

            if let Some(parents) = self.hierarchy.get(&current) {
                for parent in parents {
                    let kind = self.class_kinds.get(parent).map(|s| s.as_str()).unwrap_or("class");
                    if kind == "interface" {
                        interface_queue.push_back(parent.clone());
                    } else {
                        class_queue.push_back(parent.clone());
                    }
                }
            }
        }

        // 2. Process interfaces
        while let Some(current) = interface_queue.pop_front() {
            if !visited.insert(current.clone()) {
                continue;
            }

            let mut candidates = Vec::new();
            for (method_fqn, (decl_class, decl_name, _)) in &self.method_declarations {
                if decl_class == &current && decl_name == method_name {
                    let param_types = parse_param_types(method_fqn);
                    if param_types.len() == arg_types.len() {
                        let mut compatible = true;
                        for (arg_type, param_type) in arg_types.iter().zip(param_types.iter()) {
                            if arg_type != "java.lang.Object" && !is_subtype(arg_type, param_type, &self.hierarchy) {
                                compatible = false;
                                break;
                            }
                        }
                        if compatible {
                            candidates.push((method_fqn.clone(), param_types));
                        }
                    }
                }
            }

            if !candidates.is_empty() {
                if candidates.len() == 1 {
                    return Some(candidates[0].0.clone());
                }
                let mut best_idx = 0;
                for i in 1..candidates.len() {
                    let mut better = true;
                    for j in 0..arg_types.len() {
                        let type_best = &candidates[best_idx].1[j];
                        let type_curr = &candidates[i].1[j];
                        if !is_subtype(type_curr, type_best, &self.hierarchy) {
                            better = false;
                            break;
                        }
                    }
                    if better {
                        best_idx = i;
                    }
                }
                return Some(candidates[best_idx].0.clone());
            }

            if let Some(parents) = self.hierarchy.get(&current) {
                for parent in parents {
                    interface_queue.push_back(parent.clone());
                }
            }
        }

        // Fallback
        let mut class_queue = std::collections::VecDeque::new();
        let mut interface_queue = std::collections::VecDeque::new();
        class_queue.push_back(class_fqn.to_string());
        visited.clear();

        while let Some(current) = class_queue.pop_front() {
            if !visited.insert(current.clone()) {
                continue;
            }
            for (method_fqn, (decl_class, decl_name, _)) in &self.method_declarations {
                if decl_class == &current && decl_name == method_name {
                    return Some(method_fqn.clone());
                }
            }
            if let Some(parents) = self.hierarchy.get(&current) {
                for parent in parents {
                    let kind = self.class_kinds.get(parent).map(|s| s.as_str()).unwrap_or("class");
                    if kind == "interface" {
                        interface_queue.push_back(parent.clone());
                    } else {
                        class_queue.push_back(parent.clone());
                    }
                }
            }
        }

        while let Some(current) = interface_queue.pop_front() {
            if !visited.insert(current.clone()) {
                continue;
            }
            for (method_fqn, (decl_class, decl_name, _)) in &self.method_declarations {
                if decl_class == &current && decl_name == method_name {
                    return Some(method_fqn.clone());
                }
            }
            if let Some(parents) = self.hierarchy.get(&current) {
                for parent in parents {
                    interface_queue.push_back(parent.clone());
                }
            }
        }

        None
    }

    fn propagate(&mut self, source: &HashSet<AllocSite>, dest: &str) -> bool {
        let dest_set = self.pts.entry(dest.to_string()).or_default();
        let mut changed = false;
        for item in source {
            if dest_set.insert(item.clone()) {
                changed = true;
            }
        }
        changed
    }

    fn add_copy_edge_and_propagate(&mut self, from: String, to: String) {
        let added = self.copy_edges.entry(from.clone()).or_default().insert(to.clone());
        if added {
            let pts_from = self.pts.get(&from).cloned().unwrap_or_default();
            if self.propagate(&pts_from, &to) {
                self.worklist.push(to);
            }
        }
    }

    fn persist_results(&self, conn: &Connection) -> Result<()> {
        // 1. Run reachability analysis using stripped signatures
        let mut reachable_methods = HashSet::new();
        let mut reach_queue = std::collections::VecDeque::new();

        // Check if there is any main method in non-library methods
        let has_main = self.method_declarations.keys().any(|method_fqn| {
            let is_library = {
                let method_name_and_class = if let Some(sig_idx) = method_fqn.find('(') {
                    &method_fqn[..sig_idx]
                } else {
                    method_fqn
                };
                let class_name = if let Some(dot_idx) = method_name_and_class.rfind('.') {
                    &method_name_and_class[..dot_idx]
                } else {
                    method_name_and_class
                };
                self.library_classes.contains(class_name)
                    || class_name.starts_with("java.")
                    || class_name.starts_with("javax.")
                    || class_name.starts_with("sun.")
            };
            !is_library && (method_fqn.contains(".main(") || method_fqn.starts_with("Main.main"))
        });
        
        for method_fqn in self.method_declarations.keys() {
            let stripped_method_fqn = strip_signature(method_fqn).to_string();
            
            let is_library = {
                let method_name_and_class = if let Some(sig_idx) = method_fqn.find('(') {
                    &method_fqn[..sig_idx]
                } else {
                    method_fqn
                };
                let class_name = if let Some(dot_idx) = method_name_and_class.rfind('.') {
                    &method_name_and_class[..dot_idx]
                } else {
                    method_name_and_class
                };
                self.library_classes.contains(class_name)
                    || class_name.starts_with("java.")
                    || class_name.starts_with("javax.")
                    || class_name.starts_with("sun.")
            };

            if is_library {
                reachable_methods.insert(stripped_method_fqn.clone());
                reach_queue.push_back(stripped_method_fqn.clone());
            } else {
                if has_main {
                    if method_fqn.contains(".main(") || method_fqn.starts_with("Main.main") {
                        reachable_methods.insert(stripped_method_fqn.clone());
                        reach_queue.push_back(stripped_method_fqn.clone());
                    }
                } else {
                    reachable_methods.insert(stripped_method_fqn.clone());
                    reach_queue.push_back(stripped_method_fqn.clone());
                }
            }
        }
        
        if reachable_methods.is_empty() {
            for method_fqn in self.method_declarations.keys() {
                let stripped_method_fqn = strip_signature(method_fqn).to_string();
                reachable_methods.insert(stripped_method_fqn.clone());
                reach_queue.push_back(stripped_method_fqn.clone());
            }
        }

        while let Some(curr) = reach_queue.pop_front() {
            for call_info in &self.call_sites {
                let caller_stripped = strip_signature(&call_info.method_fqn);
                if caller_stripped == curr {
                    if call_info.receiver.is_some() {
                        if let Some(callees) = self.resolved_calls.get(&call_info.call_id) {
                            for callee in callees {
                                let callee_stripped = strip_signature(callee).to_string();
                                if reachable_methods.insert(callee_stripped.clone()) {
                                    reach_queue.push_back(callee_stripped);
                                }
                            }
                        }
                    } else {
                        if let Some(ref static_callee) = call_info.static_callee {
                            let mut found_callee = None;
                            if self.method_declarations.contains_key(static_callee) {
                                found_callee = Some(static_callee.clone());
                            } else {
                                let stripped = strip_signature(static_callee);
                                for decl in self.method_declarations.keys() {
                                    if strip_signature(decl) == stripped {
                                        found_callee = Some(decl.clone());
                                        break;
                                    }
                                }
                            }
                            if let Some(callee) = found_callee {
                                let callee_stripped = strip_signature(&callee).to_string();
                                if reachable_methods.insert(callee_stripped.clone()) {
                                    reach_queue.push_back(callee_stripped);
                                }
                            }
                        }
                    }
                }
            }
        }

        // 2. Delete all is_virtual = 1 call edges (we will insert new ones)
        let mut stmt_del = conn.prepare("DELETE FROM call_edges WHERE is_virtual = 1")?;
        stmt_del.execute([])?;

        // 3. Delete all call edges originating from unreachable methods
        let mut del_unreachable_stmt = conn.prepare(
            "DELETE FROM call_edges WHERE caller = ?1"
        )?;
        for method_fqn in self.method_declarations.keys() {
            let caller_stripped = strip_signature(method_fqn);
            if !reachable_methods.contains(caller_stripped) {
                del_unreachable_stmt.execute([caller_stripped])?;
            }
        }

        // 4. Insert resolved virtual call edges
        let mut call_edge_stmt = conn.prepare(
            "INSERT OR REPLACE INTO call_edges (caller, callee, is_virtual) VALUES (?1, ?2, 1)"
        )?;
        for (call_id, callees) in &self.resolved_calls {
            if let Some(call_info) = self.call_sites.iter().find(|c| c.call_id == *call_id) {
                let caller_stripped = strip_signature(&call_info.method_fqn);
                if reachable_methods.contains(caller_stripped) {
                    for callee in callees {
                        let callee_stripped = strip_signature(callee);
                        call_edge_stmt.execute([caller_stripped, callee_stripped])?;

                        // If it's a dynamic reflection invoke, also insert an edge with Method.invoke as caller
                        if call_info.method_name == "invoke" || call_info.static_callee.as_ref().map(|s| s.contains("Method.invoke")).unwrap_or(false) {
                            call_edge_stmt.execute(["java.lang.reflect.Method.invoke", callee_stripped])?;
                        }
                    }
                }
            }
        }

        // 5. Prune naive/interface call edges for virtual call sites of reachable methods
        let mut naive_del_stmt = conn.prepare(
            "DELETE FROM call_edges WHERE caller = ?1 AND callee = ?2 AND is_virtual = 0"
        )?;
        for call_info in &self.call_sites {
            let caller_stripped = strip_signature(&call_info.method_fqn);
            if reachable_methods.contains(caller_stripped) {
                if call_info.receiver.is_some() {
                    if let Some(ref static_callee) = call_info.static_callee {
                        let callee_stripped = strip_signature(static_callee);
                        if let Some(dot_idx) = callee_stripped.rfind('.') {
                            let callee_class = &callee_stripped[..dot_idx];
                            let method_name = &callee_stripped[dot_idx + 1..];

                            let mut queue = vec![callee_class.to_string()];
                            let mut visited = HashSet::new();
                            while let Some(curr_class) = queue.pop() {
                                if !visited.insert(curr_class.clone()) {
                                    continue;
                                }
                                let is_interface = self.class_kinds.get(&curr_class).map(|s| s.as_str()) == Some("interface");
                                if !is_interface {
                                    let naive_callee = format!("{}.{}", curr_class, method_name);
                                    let has_unresolved_site = self.call_sites.iter().any(|cs| {
                                        strip_signature(&cs.method_fqn) == caller_stripped
                                            && cs.method_name == method_name
                                            && cs.receiver.is_some()
                                            && self.resolved_calls.get(&cs.call_id).map_or(true, |set| set.is_empty())
                                    });
                                    if !has_unresolved_site {
                                        naive_del_stmt.execute([caller_stripped, naive_callee.as_str()])?;
                                    }
                                }

                                if let Some(parents) = self.hierarchy.get(&curr_class) {
                                    for parent in parents {
                                        queue.push(parent.clone());
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // 6. Persist points-to sets
        let mut stmt_del_pts = conn.prepare("DELETE FROM points_to_sets")?;
        stmt_del_pts.execute([])?;

        let mut pts_stmt = conn.prepare(
            "INSERT OR REPLACE INTO points_to_sets (variable_fqn, alloc_id) VALUES (?1, ?2)"
        )?;
        for (var, allocs) in &self.pts {
            for alloc in allocs {
                pts_stmt.execute([var.as_str(), alloc.id.as_str()])?;
            }
        }

        Ok(())
    }

    fn propagate_reflection(&mut self, u: &str, pts_u: &HashSet<AllocSite>) {
        let call_sites = self.call_sites.clone();
        for call_info in &call_sites {
            // 1. Class.forName
            if call_info.method_name == "forName" || call_info.static_callee.as_ref().map(|s| s.contains("Class.forName")).unwrap_or(false) {
                if call_info.arguments.get(0).map(|s| s.as_str()) == Some(u) {
                    for o in pts_u {
                        if o.id.starts_with("StringAlloc:") {
                            let class_name = &o.id["StringAlloc:".len()..];
                            let class_alloc = format!("ReflectClassAlloc:{}", class_name);
                            if let Some(ref lhs) = call_info.lhs {
                                let alloc_site = AllocSite {
                                    id: class_alloc,
                                    type_name: "java.lang.Class".to_string(),
                                };
                                let mut set = HashSet::new();
                                set.insert(alloc_site);
                                if self.propagate(&set, lhs) {
                                    self.worklist.push(lhs.clone());
                                }
                            }
                        }
                    }
                }
            }

            // 2. Class.getDeclaredMethod / getMethod
            if call_info.method_name == "getDeclaredMethod" || call_info.method_name == "getMethod" {
                let is_rec = call_info.receiver.as_ref().map(|s| s.as_str()) == Some(u);
                let is_arg = call_info.arguments.get(0).map(|s| s.as_str()) == Some(u);
                if is_rec || is_arg {
                    if let Some(ref rec) = call_info.receiver {
                        if let Some(arg) = call_info.arguments.get(0) {
                            let pts_rec = self.pts.get(rec).cloned().unwrap_or_default();
                            let pts_arg = self.pts.get(arg).cloned().unwrap_or_default();
                            
                            for o_rec in &pts_rec {
                                if o_rec.id.starts_with("ReflectClassAlloc:") {
                                    let class_name = &o_rec.id["ReflectClassAlloc:".len()..];
                                    for o_arg in &pts_arg {
                                        if o_arg.id.starts_with("StringAlloc:") {
                                            let method_name = &o_arg.id["StringAlloc:".len()..];
                                            
                                            // Find all matching methods
                                            let mut matched_methods = Vec::new();
                                            for method_fqn in self.method_declarations.keys() {
                                                if let Some((decl_class, decl_name, _)) = self.method_declarations.get(method_fqn) {
                                                    if decl_class == class_name && decl_name == method_name {
                                                        matched_methods.push(method_fqn.clone());
                                                    }
                                                }
                                            }

                                            // If no exact method matches in decls, construct FQN anyway
                                            if matched_methods.is_empty() {
                                                matched_methods.push(format!("{}.{}()", class_name, method_name));
                                            }

                                            for method_fqn in matched_methods {
                                                let method_alloc = format!("ReflectMethodAlloc:{}", method_fqn);
                                                if let Some(ref lhs) = call_info.lhs {
                                                    let alloc_site = AllocSite {
                                                        id: method_alloc,
                                                        type_name: "java.lang.reflect.Method".to_string(),
                                                    };
                                                    let mut set = HashSet::new();
                                                    set.insert(alloc_site);
                                                    if self.propagate(&set, lhs) {
                                                        self.worklist.push(lhs.clone());
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

            // 3. Class.getDeclaredField / getField
            if call_info.method_name == "getDeclaredField" || call_info.method_name == "getField" {
                let is_rec = call_info.receiver.as_ref().map(|s| s.as_str()) == Some(u);
                let is_arg = call_info.arguments.get(0).map(|s| s.as_str()) == Some(u);
                if is_rec || is_arg {
                    if let Some(ref rec) = call_info.receiver {
                        if let Some(arg) = call_info.arguments.get(0) {
                            let pts_rec = self.pts.get(rec).cloned().unwrap_or_default();
                            let pts_arg = self.pts.get(arg).cloned().unwrap_or_default();

                            for o_rec in &pts_rec {
                                if o_rec.id.starts_with("ReflectClassAlloc:") {
                                    let class_name = &o_rec.id["ReflectClassAlloc:".len()..];
                                    for o_arg in &pts_arg {
                                        if o_arg.id.starts_with("StringAlloc:") {
                                            let field_name = &o_arg.id["StringAlloc:".len()..];
                                            let field_alloc = format!("ReflectFieldAlloc:{}.{}", class_name, field_name);
                                            if let Some(ref lhs) = call_info.lhs {
                                                let alloc_site = AllocSite {
                                                    id: field_alloc,
                                                    type_name: "java.lang.reflect.Field".to_string(),
                                                };
                                                let mut set = HashSet::new();
                                                set.insert(alloc_site);
                                                if self.propagate(&set, lhs) {
                                                    self.worklist.push(lhs.clone());
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

            // 4. Method.invoke
            if call_info.method_name == "invoke" {
                if call_info.receiver.as_ref().map(|s| s.as_str()) == Some(u) {
                    for o in pts_u {
                        if o.id.starts_with("ReflectMethodAlloc:") {
                            let method_fqn = &o.id["ReflectMethodAlloc:".len()..];
                            
                            // 4a. Add call edge to resolved_calls
                            if self.resolved_calls.entry(call_info.call_id.clone()).or_default().insert(method_fqn.to_string()) {
                                // 4b. Bind receiver (first argument of invoke) to `this`
                                if let Some(obj_arg) = call_info.arguments.get(0) {
                                    let this_node = format!("{}#this", method_fqn);
                                    self.add_copy_edge_and_propagate(obj_arg.clone(), this_node);
                                }

                                // 4c. Bind other arguments of invoke to parameters of method_fqn
                                if let Some((_, _, callee_params)) = self.method_declarations.get(method_fqn).cloned() {
                                    for i in 0..callee_params.len() {
                                        if let Some(arg_var) = call_info.arguments.get(i + 1) {
                                            let param_node = format!("{}#{}", method_fqn, callee_params[i]);
                                            self.add_copy_edge_and_propagate(arg_var.clone(), param_node);
                                            
                                            let pos_node = format!("{}#p{}", method_fqn, i);
                                            self.add_copy_edge_and_propagate(arg_var.clone(), pos_node);
                                        }
                                    }
                                } else {
                                    for i in 1..call_info.arguments.len() {
                                        if let Some(arg_var) = call_info.arguments.get(i) {
                                            let pos_node = format!("{}#p{}", method_fqn, i - 1);
                                            self.add_copy_edge_and_propagate(arg_var.clone(), pos_node);
                                        }
                                    }
                                }

                                // 4d. Bind return to LHS
                                if let Some(ref lhs) = call_info.lhs {
                                    let return_node = format!("{}#return", method_fqn);
                                    self.add_copy_edge_and_propagate(return_node, lhs.clone());
                                }
                            }
                        }
                    }
                }
            }

            // 5. Field.get
            if call_info.method_name == "get" {
                if call_info.receiver.as_ref().map(|s| s.as_str()) == Some(u) {
                    for o in pts_u {
                        if o.id.starts_with("ReflectFieldAlloc:") {
                            let field_fqn = &o.id["ReflectFieldAlloc:".len()..];
                            if let Some(obj_arg) = call_info.arguments.get(0) {
                                if let Some(dot_idx) = field_fqn.rfind('.') {
                                    let field_name = &field_fqn[dot_idx + 1..];
                                    let obj_pts = self.pts.get(obj_arg).cloned().unwrap_or_default();
                                    for obj_o in &obj_pts {
                                        let field_node = format!("{}.{}", obj_o.id, field_name);
                                        if let Some(ref lhs) = call_info.lhs {
                                            self.add_copy_edge_and_propagate(field_node, lhs.clone());
                                        }
                                    }
                                }
                            }
                        }
                    }
                } else if call_info.arguments.get(0).map(|s| s.as_str()) == Some(u) {
                    if let Some(ref rec) = call_info.receiver {
                        let pts_rec = self.pts.get(rec).cloned().unwrap_or_default();
                        for o_rec in &pts_rec {
                            if o_rec.id.starts_with("ReflectFieldAlloc:") {
                                let field_fqn = &o_rec.id["ReflectFieldAlloc:".len()..];
                                if let Some(dot_idx) = field_fqn.rfind('.') {
                                    let field_name = &field_fqn[dot_idx + 1..];
                                    for obj_o in pts_u {
                                        let field_node = format!("{}.{}", obj_o.id, field_name);
                                        if let Some(ref lhs) = call_info.lhs {
                                            self.add_copy_edge_and_propagate(field_node, lhs.clone());
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // 6. Field.set
            if call_info.method_name == "set" {
                if call_info.receiver.as_ref().map(|s| s.as_str()) == Some(u) {
                    for o in pts_u {
                        if o.id.starts_with("ReflectFieldAlloc:") {
                            let field_fqn = &o.id["ReflectFieldAlloc:".len()..];
                            if let Some(obj_arg) = call_info.arguments.get(0) {
                                if let Some(val_arg) = call_info.arguments.get(1) {
                                    if let Some(dot_idx) = field_fqn.rfind('.') {
                                        let field_name = &field_fqn[dot_idx + 1..];
                                        let obj_pts = self.pts.get(obj_arg).cloned().unwrap_or_default();
                                        for obj_o in &obj_pts {
                                            let field_node = format!("{}.{}", obj_o.id, field_name);
                                            self.add_copy_edge_and_propagate(val_arg.clone(), field_node);
                                        }
                                    }
                                }
                            }
                        }
                    }
                } else if call_info.arguments.get(0).map(|s| s.as_str()) == Some(u) {
                    if let Some(ref rec) = call_info.receiver {
                        if let Some(val_arg) = call_info.arguments.get(1) {
                            let pts_rec = self.pts.get(rec).cloned().unwrap_or_default();
                            for o_rec in &pts_rec {
                                if o_rec.id.starts_with("ReflectFieldAlloc:") {
                                    let field_fqn = &o_rec.id["ReflectFieldAlloc:".len()..];
                                    if let Some(dot_idx) = field_fqn.rfind('.') {
                                        let field_name = &field_fqn[dot_idx + 1..];
                                        for obj_o in pts_u {
                                            let field_node = format!("{}.{}", obj_o.id, field_name);
                                            self.add_copy_edge_and_propagate(val_arg.clone(), field_node);
                                        }
                                    }
                                }
                            }
                        }
                    }
                } else if call_info.arguments.get(1).map(|s| s.as_str()) == Some(u) {
                    if let Some(ref rec) = call_info.receiver {
                        if let Some(obj_arg) = call_info.arguments.get(0) {
                            let pts_rec = self.pts.get(rec).cloned().unwrap_or_default();
                            let pts_obj = self.pts.get(obj_arg).cloned().unwrap_or_default();
                            for o_rec in &pts_rec {
                                if o_rec.id.starts_with("ReflectFieldAlloc:") {
                                    let field_fqn = &o_rec.id["ReflectFieldAlloc:".len()..];
                                    if let Some(dot_idx) = field_fqn.rfind('.') {
                                        let field_name = &field_fqn[dot_idx + 1..];
                                        for obj_o in &pts_obj {
                                            let field_node = format!("{}.{}", obj_o.id, field_name);
                                            self.add_copy_edge_and_propagate(u.to_string(), field_node);
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
    use rusqlite::Connection;

    fn setup_test_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::init_db(&conn).unwrap();
        conn
    }

    #[test]
    fn test_polymorphic_dispatch_and_pruning() {
        let conn = setup_test_db();
        conn.execute("INSERT INTO method_declarations VALUES ('Main.main', 'Main', 'main', '')", []).unwrap();
        
        // Class hierarchy facts
        conn.execute("INSERT INTO classes VALUES ('com.example.Shape', 'interface')", []).unwrap();
        conn.execute("INSERT INTO classes VALUES ('com.example.Circle', 'class')", []).unwrap();
        conn.execute("INSERT INTO classes VALUES ('com.example.Square', 'class')", []).unwrap();
        conn.execute("INSERT INTO class_hierarchy VALUES ('com.example.Circle', 'com.example.Shape')", []).unwrap();
        conn.execute("INSERT INTO class_hierarchy VALUES ('com.example.Square', 'com.example.Shape')", []).unwrap();
        
        // Method declarations
        conn.execute("INSERT INTO method_declarations VALUES ('com.example.Shape.draw', 'com.example.Shape', 'draw', '')", []).unwrap();
        conn.execute("INSERT INTO method_declarations VALUES ('com.example.Circle.draw', 'com.example.Circle', 'draw', '')", []).unwrap();
        conn.execute("INSERT INTO method_declarations VALUES ('com.example.Square.draw', 'com.example.Square', 'draw', '')", []).unwrap();
        
        // Main allocations and assignments
        // Shape shape = new Circle();
        conn.execute("INSERT INTO allocation_sites VALUES ('Main.main:alloc_1', 'com.example.Circle', 'Main.main')", []).unwrap();
        conn.execute("INSERT INTO source_assignments (lhs, rhs, assignment_type, method_fqn) VALUES ('Main.main#shape', 'Main.main:alloc_1', 'ALLOC', 'Main.main')", []).unwrap();
        
        // Method call: shape.draw();
        conn.execute("INSERT INTO call_sites (call_id, method_fqn, receiver, method_name, lhs) VALUES ('Main.main:call_1', 'Main.main', 'Main.main#shape', 'draw', NULL)", []).unwrap();
        
        // Solve
        let analyzer = CallGraphAnalyzer::new();
        analyzer.analyze(&conn).unwrap();
        
        // Assert resolved edges
        let has_circle_draw: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM call_edges WHERE caller='Main.main' AND callee='com.example.Circle.draw' AND is_virtual=1)",
            [],
            |r| r.get(0)
        ).unwrap();
        assert!(has_circle_draw, "Circle.draw should be resolved");

        let has_square_draw: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM call_edges WHERE caller='Main.main' AND callee='com.example.Square.draw')",
            [],
            |r| r.get(0)
        ).unwrap();
        assert!(!has_square_draw, "Square.draw should be pruned (not instantiated)");
    }

    #[test]
    fn test_overloaded_method_dispatch() {
        let conn = setup_test_db();
        conn.execute("INSERT INTO method_declarations VALUES ('Main.main', 'Main', 'main', '')", []).unwrap();
        
        // Class hierarchy
        conn.execute("INSERT INTO classes VALUES ('com.example.Calculator', 'class')", []).unwrap();
        
        // Method declarations (overloaded)
        conn.execute("INSERT INTO method_declarations VALUES ('com.example.Calculator.add(int)', 'com.example.Calculator', 'add', 'a')", []).unwrap();
        conn.execute("INSERT INTO method_declarations VALUES ('com.example.Calculator.add(int,int)', 'com.example.Calculator', 'add', 'a,b')", []).unwrap();
        
        // Main allocations and assignments
        conn.execute("INSERT INTO allocation_sites VALUES ('Main.main:alloc_1', 'com.example.Calculator', 'Main.main')", []).unwrap();
        conn.execute("INSERT INTO source_assignments (lhs, rhs, assignment_type, method_fqn) VALUES ('Main.main#calc', 'Main.main:alloc_1', 'ALLOC', 'Main.main')", []).unwrap();
        
        // Allocation for argument x and y
        conn.execute("INSERT INTO allocation_sites VALUES ('Main.main:alloc_x', 'java.lang.Integer', 'Main.main')", []).unwrap();
        conn.execute("INSERT INTO source_assignments (lhs, rhs, assignment_type, method_fqn) VALUES ('Main.main#x', 'Main.main:alloc_x', 'ALLOC', 'Main.main')", []).unwrap();

        conn.execute("INSERT INTO allocation_sites VALUES ('Main.main:alloc_y', 'java.lang.Integer', 'Main.main')", []).unwrap();
        conn.execute("INSERT INTO source_assignments (lhs, rhs, assignment_type, method_fqn) VALUES ('Main.main#y', 'Main.main:alloc_y', 'ALLOC', 'Main.main')", []).unwrap();

        // Method call 1: calc.add(x); (1 argument)
        conn.execute("INSERT INTO call_sites (call_id, method_fqn, receiver, method_name, lhs) VALUES ('Main.main:call_1', 'Main.main', 'Main.main#calc', 'add', NULL)", []).unwrap();
        conn.execute("INSERT INTO call_arguments (call_id, arg_index, arg_var) VALUES ('Main.main:call_1', 0, 'Main.main#x')", []).unwrap();

        // Method call 2: calc.add(x, y); (2 arguments)
        conn.execute("INSERT INTO call_sites (call_id, method_fqn, receiver, method_name, lhs) VALUES ('Main.main:call_2', 'Main.main', 'Main.main#calc', 'add', NULL)", []).unwrap();
        conn.execute("INSERT INTO call_arguments (call_id, arg_index, arg_var) VALUES ('Main.main:call_2', 0, 'Main.main#x')", []).unwrap();
        conn.execute("INSERT INTO call_arguments (call_id, arg_index, arg_var) VALUES ('Main.main:call_2', 1, 'Main.main#y')", []).unwrap();
        
        // Solve
        let analyzer = CallGraphAnalyzer::new();
        analyzer.analyze(&conn).unwrap();
        
        // Assert resolved edges
        let has_add: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM call_edges WHERE caller='Main.main' AND callee='com.example.Calculator.add' AND is_virtual=1)",
            [],
            |r| r.get(0)
        ).unwrap();
        assert!(has_add, "Calculator.add should be resolved");

        // Verify parameter binding:
        // 'com.example.Calculator.add(int)#a' should receive Main.main:alloc_x
        let has_param_1a: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM points_to_sets WHERE variable_fqn='com.example.Calculator.add(int)#a' AND alloc_id='Main.main:alloc_x')",
            [],
            |r| r.get(0)
        ).unwrap();
        assert!(has_param_1a, "Calculator.add(int)#a should point to alloc_x");

        // 'com.example.Calculator.add(int,int)#b' should receive Main.main:alloc_y
        let has_param_2b: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM points_to_sets WHERE variable_fqn='com.example.Calculator.add(int,int)#b' AND alloc_id='Main.main:alloc_y')",
            [],
            |r| r.get(0)
        ).unwrap();
        assert!(has_param_2b, "Calculator.add(int,int)#b should point to alloc_y");
    }
}
