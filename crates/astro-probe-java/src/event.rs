use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use rusqlite::Connection;
use astro_probe_core::cg::{
    PointsToSolverExtension, ExtensionContext, CallSiteInfo, CoreError
};

fn strip_signature(method_fqn: &str) -> &str {
    if let Some(idx) = method_fqn.find('(') {
        &method_fqn[..idx]
    } else {
        method_fqn
    }
}

fn get_first_param_type(method_fqn: &str) -> Option<String> {
    let start = method_fqn.find('(')?;
    let end = method_fqn.rfind(')')?;
    if start >= end {
        return None;
    }
    let params_str = &method_fqn[start + 1..end];
    params_str.split(',').next().map(|s| s.trim().to_string())
}

fn get_declared_type(context: &ExtensionContext, var_name: &str) -> Option<String> {
    if let Some(idx) = var_name.find('#') {
        let prefix = &var_name[..idx];
        let suffix = &var_name[idx + 1..];

        // 1. Check parameter_annotations
        if let Ok(mut stmt) = context.conn.prepare(
            "SELECT annotation_name FROM parameter_annotations WHERE method_fqn = ?1 AND parameter_name = ?2 AND annotation_name LIKE 'FieldType:%'"
        ) {
            if let Ok(mut rows) = stmt.query([prefix, suffix]) {
                if let Ok(Some(row)) = rows.next() {
                    if let Ok(ann) = row.get::<_, String>(0) {
                        if let Some(ty) = ann.strip_prefix("FieldType:") {
                            return Some(ty.to_string());
                        }
                    }
                }
            }
        }

        // 2. Check field_annotations
        if let Ok(mut stmt) = context.conn.prepare(
            "SELECT annotation_name FROM field_annotations WHERE class_fqn = ?1 AND field_name = ?2 AND annotation_name LIKE 'FieldType:%'"
        ) {
            if let Ok(mut rows) = stmt.query([prefix, suffix]) {
                if let Ok(Some(row)) = rows.next() {
                    if let Ok(ann) = row.get::<_, String>(0) {
                        if let Some(ty) = ann.strip_prefix("FieldType:") {
                            return Some(ty.to_string());
                        }
                    }
                }
            }
        }
    }
    None
}

fn get_runnable_implementations(context: &ExtensionContext, base_type: &str) -> Vec<String> {
    let mut impls = Vec::new();

    if context.decl_by_class_name.contains_key(&(base_type.to_string(), "run".to_string())) {
        impls.push(base_type.to_string());
    }

    for (class_fqn, _) in context.ancestors_map.iter() {
        if class_fqn != base_type
            && context.is_subtype(class_fqn, base_type)
            && context.decl_by_class_name.contains_key(&(class_fqn.clone(), "run".to_string()))
        {
            impls.push(class_fqn.clone());
        }
    }

    impls
}

pub struct SpringEventLineageExtension {
    listeners: Mutex<Option<Vec<(String, String)>>>,
}

impl SpringEventLineageExtension {
    pub fn new() -> Self {
        Self {
            listeners: Mutex::new(None),
        }
    }

    fn get_listeners(&self, conn: &Connection) -> Result<Vec<(String, String)>, rusqlite::Error> {
        let mut cache = self.listeners.lock().unwrap();
        if let Some(ref list) = *cache {
            return Ok(list.clone());
        }

        let mut list = Vec::new();

        let mut stmt = conn.prepare(
            "SELECT DISTINCT method_fqn FROM method_annotations \
             WHERE annotation_name = 'EventListener' OR annotation_name LIKE 'EventListener:%'"
        )?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            let method_fqn: String = row.get(0)?;
            if let Some(first_param_type) = get_first_param_type(&method_fqn) {
                if !first_param_type.is_empty() {
                    list.push((method_fqn, first_param_type));
                }
            }
        }

        let mut stmt2 = conn.prepare(
            "SELECT DISTINCT class_fqn FROM class_hierarchy \
             WHERE parent_fqn LIKE '%ApplicationListener%'"
        )?;
        let mut m_stmt = conn.prepare(
            "SELECT method_fqn FROM method_declarations WHERE class_fqn = ?1 AND method_name = 'onApplicationEvent'"
        )?;
        let mut rows2 = stmt2.query([])?;
        while let Some(row) = rows2.next()? {
            let class_fqn: String = row.get(0)?;
            let mut m_rows = m_stmt.query([&class_fqn])?;
            while let Some(m_row) = m_rows.next()? {
                let method_fqn: String = m_row.get(0)?;
                if let Some(first_param_type) = get_first_param_type(&method_fqn) {
                    if !first_param_type.is_empty() {
                        list.push((method_fqn, first_param_type));
                    }
                }
            }
        }

        *cache = Some(list.clone());
        Ok(list)
    }
}

impl Default for SpringEventLineageExtension {
    fn default() -> Self {
        Self::new()
    }
}

impl PointsToSolverExtension for SpringEventLineageExtension {
    fn matches_call_site(&self, call: &CallSiteInfo) -> bool {
        call.method_name == "publishEvent" || call.static_callee.is_some_and(|sc| sc.contains("publishEvent"))
    }

    fn handle_call(
        &self,
        context: &mut ExtensionContext,
        call: &CallSiteInfo,
        _resolved_targets: &HashSet<(String, bool)>,
    ) -> Result<Option<bool>, CoreError> {
        let is_publish = call.method_name == "publishEvent"
            || call.static_callee.is_some_and(|sc| sc.contains("publishEvent"));

        if !is_publish {
            return Ok(None);
        }

        let mut changed = false;

        if let Some(args) = context.call_args.get(call.call_id) {
            if let Some((_, arg_var, _)) = args.iter().find(|(idx, _, _)| *idx == 0) {
                if let Some(arg_pts) = context.pts.get(arg_var) {
                    let arg_pts_clone = arg_pts.clone();
                    let listeners = self.get_listeners(context.conn).map_err(CoreError::Sqlite)?;

                    for event_alloc in &arg_pts_clone {
                        if let Some(event_type) = context.alloc_types.get(event_alloc) {
                            for (listener_method, event_listener_type) in &listeners {
                                if context.is_subtype(event_type, event_listener_type) {
                                    if context.call_edges_discovered.insert((
                                        strip_signature(call.caller_method_fqn).to_string(),
                                        strip_signature(listener_method).to_string(),
                                        true,
                                    )) {
                                        changed = true;
                                    }

                                    let p0_var = format!("{}#p0", listener_method);
                                    let p0_set = context.pts.entry(p0_var).or_default();
                                    if p0_set.insert(event_alloc.clone()) {
                                        changed = true;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(Some(changed))
    }
}

pub struct AsyncExecutionExtension;

impl AsyncExecutionExtension {
    pub fn new() -> Self {
        Self
    }
}

impl Default for AsyncExecutionExtension {
    fn default() -> Self {
        Self::new()
    }
}

impl PointsToSolverExtension for AsyncExecutionExtension {
    fn matches_call_site(&self, call: &CallSiteInfo) -> bool {
        call.method_name == "start" || call.method_name == "execute" || call.method_name == "submit"
    }

    fn handle_call(
        &self,
        context: &mut ExtensionContext,
        call: &CallSiteInfo,
        _resolved_targets: &HashSet<(String, bool)>,
    ) -> Result<Option<bool>, CoreError> {
        let mut changed = false;

        if call.method_name == "start" {
            if let Some(receiver_var) = call.receiver {
                if let Some(rec_pts) = context.pts.get(receiver_var) {
                    let rec_pts_clone = rec_pts.clone();
                    for alloc in rec_pts_clone {
                        if let Some(alloc_type) = context.alloc_types.get(&alloc) {
                            if alloc_type == "java.lang.Thread" || context.is_subtype(alloc_type, "java.lang.Thread") {
                                if let Some(methods) = context.decl_by_class_name.get(&(alloc_type.clone(), "run".to_string())) {
                                    for run_method in methods {
                                        if context.call_edges_discovered.insert((
                                            strip_signature(call.caller_method_fqn).to_string(),
                                            strip_signature(run_method).to_string(),
                                            true,
                                        )) {
                                            changed = true;
                                        }

                                        let this_var = format!("{}#this", run_method);
                                        let this_set = context.pts.entry(this_var).or_default();
                                        if this_set.insert(alloc.clone()) {
                                            changed = true;
                                        }
                                    }
                                }

                                // Constructor tracing for Runnable passed to Thread
                                let mut stmt = context.conn.prepare(
                                    "SELECT cs.receiver, ca.arg_var \
                                     FROM call_sites cs \
                                     JOIN call_arguments ca ON cs.call_id = ca.call_id \
                                     WHERE cs.method_fqn = ?1 AND cs.method_name = '<init>' AND ca.arg_index = 0"
                                )?;
                                let mut rows = stmt.query([call.caller_method_fqn])?;
                                while let Some(row) = rows.next()? {
                                    let constructor_receiver: Option<String> = row.get(0)?;
                                    let arg_var: String = row.get(1)?;

                                    let matches = if let Some(ref cr) = constructor_receiver {
                                        if let Some(cr_pts) = context.pts.get(cr) {
                                            cr_pts.contains(&alloc)
                                        } else {
                                            false
                                        }
                                    } else {
                                        false
                                    };

                                    if matches && !arg_var.is_empty() {
                                        let mut runnable_types = HashSet::new();

                                        // 1. Try points-to sets first
                                        if let Some(runnable_pts) = context.pts.get(&arg_var) {
                                            for r_alloc in runnable_pts {
                                                if let Some(r_type) = context.alloc_types.get(r_alloc) {
                                                    if r_type == "java.lang.Runnable" || context.is_subtype(r_type, "java.lang.Runnable") {
                                                        runnable_types.insert(r_type.clone());
                                                    }
                                                }
                                            }
                                        }

                                        // 2. Fall back to declared type
                                        if runnable_types.is_empty() {
                                            if let Some(declared_type) = get_declared_type(context, &arg_var) {
                                                if declared_type == "java.lang.Runnable" || context.is_subtype(&declared_type, "java.lang.Runnable") {
                                                    let impls = get_runnable_implementations(context, &declared_type);
                                                    for impl_class in impls {
                                                        runnable_types.insert(impl_class);
                                                    }
                                                }
                                            }
                                        }
                                        for r_type in runnable_types {
                                            if let Some(run_methods) = context.decl_by_class_name.get(&(r_type.clone(), "run".to_string())) {
                                                for run_method in run_methods {
                                                    if context.call_edges_discovered.insert((
                                                        strip_signature(call.caller_method_fqn).to_string(),
                                                        strip_signature(run_method).to_string(),
                                                        true,
                                                    )) {
                                                        changed = true;
                                                    }

                                                    // Propagate this pointer if we have concrete allocations
                                                    if let Some(runnable_pts) = context.pts.get(&arg_var).cloned() {
                                                        for r_alloc in runnable_pts {
                                                            if let Some(act) = context.alloc_types.get(&r_alloc) {
                                                                if act == &r_type {
                                                                    let this_var = format!("{}#this", run_method);
                                                                    let this_set = context.pts.entry(this_var).or_default();
                                                                    if this_set.insert(r_alloc.clone()) {
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
                            }
                        }
                    }
                }
            }
        }

        if call.method_name == "execute" || call.method_name == "submit" {
            if let Some(args) = context.call_args.get(call.call_id) {
                if let Some((_, arg_var, _)) = args.iter().find(|(idx, _, _)| *idx == 0) {
                    let mut target_types = HashSet::new();

                    // 1. Try points-to sets first
                    if let Some(arg_pts) = context.pts.get(arg_var) {
                        for alloc in arg_pts {
                            if let Some(alloc_type) = context.alloc_types.get(alloc) {
                                if alloc_type == "java.lang.Runnable" || context.is_subtype(alloc_type, "java.lang.Runnable") {
                                    target_types.insert(alloc_type.clone());
                                }
                            }
                        }
                    }

                    // 2. If no target types found via points-to set, fall back to declared type
                    if target_types.is_empty() {
                        if let Some(declared_type) = get_declared_type(context, arg_var) {
                            if declared_type == "java.lang.Runnable" || context.is_subtype(&declared_type, "java.lang.Runnable") {
                                for impl_class in get_runnable_implementations(context, &declared_type) {
                                    target_types.insert(impl_class);
                                }
                            }
                        }
                    }

                    for alloc_type in target_types {
                        if let Some(methods) = context.decl_by_class_name.get(&(alloc_type.clone(), "run".to_string())) {
                            for run_method in methods {
                                if context.call_edges_discovered.insert((
                                    strip_signature(call.caller_method_fqn).to_string(),
                                    strip_signature(run_method).to_string(),
                                    true,
                                )) {
                                    changed = true;
                                }

                                // Propagate this pointer if we have concrete allocations
                                if let Some(arg_pts) = context.pts.get(arg_var).cloned() {
                                    for alloc in arg_pts {
                                        if let Some(act) = context.alloc_types.get(&alloc) {
                                            if act == &alloc_type {
                                                let this_var = format!("{}#this", run_method);
                                                let this_set = context.pts.entry(this_var).or_default();
                                                if this_set.insert(alloc.clone()) {
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
        }

        if changed {
            Ok(Some(true))
        } else {
            Ok(None)
        }
    }
}

fn get_class_name(method_fqn: &str) -> &str {
    let stripped = strip_signature(method_fqn);
    if let Some(last_dot) = stripped.rfind('.') {
        &stripped[..last_dot]
    } else {
        stripped
    }
}

fn matches_within(method_fqn: &str, pattern: &str) -> bool {
    let pattern = pattern.trim();
    let method_stripped = strip_signature(method_fqn);
    if pattern.ends_with("..*") {
        let prefix = &pattern[..pattern.len() - 3];
        method_stripped == prefix || method_stripped.starts_with(&format!("{}.", prefix))
    } else if pattern.ends_with(".*") {
        let prefix = &pattern[..pattern.len() - 2];
        method_stripped == prefix || method_stripped.starts_with(&format!("{}.", prefix))
    } else if pattern.ends_with('*') {
        let prefix = &pattern[..pattern.len() - 1];
        method_stripped == prefix || method_stripped.starts_with(&format!("{}.", prefix))
    } else {
        if method_stripped == pattern {
            return true;
        }
        if let Some(last_dot) = method_stripped.rfind('.') {
            let class_fqn = &method_stripped[..last_dot];
            class_fqn == pattern
        } else {
            false
        }
    }
}

fn matches_pointcut(method_fqn: &str, expr: &str) -> bool {
    let expr = expr.trim();
    if expr.contains("||") {
        return expr.split("||").any(|part| matches_pointcut(method_fqn, part));
    }
    if expr.starts_with("within(") && expr.ends_with(')') {
        let pattern = &expr[7..expr.len() - 1];
        matches_within(method_fqn, pattern)
    } else if expr.starts_with("execution(") && expr.ends_with(')') {
        let inside = &expr[10..expr.len() - 1].trim();
        if let Some(last_part) = inside.split_whitespace().last() {
            let pattern = if let Some(paren_idx) = last_part.find('(') {
                &last_part[..paren_idx]
            } else {
                last_part
            };
            matches_within(method_fqn, pattern)
        } else {
            false
        }
    } else {
        false
    }
}

pub struct SpringAopPointcutExtension {
    advices: Mutex<Option<Vec<AdviceInfo>>>,
}

#[derive(Debug, Clone)]
struct AdviceInfo {
    advice_method_fqn: String,
    pointcut_expr: String,
}

impl SpringAopPointcutExtension {
    pub fn new() -> Self {
        Self {
            advices: Mutex::new(None),
        }
    }

    fn get_advices(&self, conn: &Connection) -> Result<Vec<AdviceInfo>, rusqlite::Error> {
        let mut cache = self.advices.lock().unwrap();
        if let Some(ref list) = *cache {
            return Ok(list.clone());
        }

        let mut list = Vec::new();

        let mut stmt = conn.prepare(
            "SELECT DISTINCT method_fqn, annotation_name FROM method_annotations \
             WHERE annotation_name LIKE 'Before:%' \
                OR annotation_name LIKE 'After:%' \
                OR annotation_name LIKE 'Around:%' \
                OR annotation_name LIKE 'AfterThrowing:%' \
                OR annotation_name LIKE 'AfterReturning:%'"
        )?;
        let mut pc_stmt = conn.prepare(
            "SELECT annotation_name FROM method_annotations \
             WHERE method_fqn LIKE ?1 AND annotation_name LIKE 'Pointcut:%'"
        )?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            let advice_method_fqn: String = row.get(0)?;
            let annotation_name: String = row.get(1)?;

            let colon_idx = annotation_name.find(':').unwrap();
            let raw_expr = annotation_name[colon_idx + 1..].trim().to_string();

            let resolved_expr = if raw_expr.ends_with("()")
                && !raw_expr.starts_with("within")
                && !raw_expr.starts_with("execution")
            {
                let pointcut_method_name = raw_expr[..raw_expr.len() - 2].trim();
                let class_name = get_class_name(&advice_method_fqn);
                let target_method_prefix = format!("{}.{}(", class_name, pointcut_method_name);

                let mut pc_rows = pc_stmt.query([format!("{}%", target_method_prefix)])?;
                if let Some(pc_row) = pc_rows.next()? {
                    let pc_ann: String = pc_row.get(0)?;
                    let pc_colon = pc_ann.find(':').unwrap();
                    pc_ann[pc_colon + 1..].trim().to_string()
                } else {
                    raw_expr
                }
            } else {
                raw_expr
            };

            list.push(AdviceInfo {
                advice_method_fqn,
                pointcut_expr: resolved_expr,
            });
        }

        *cache = Some(list.clone());
        Ok(list)
    }
}

impl Default for SpringAopPointcutExtension {
    fn default() -> Self {
        Self::new()
    }
}

impl PointsToSolverExtension for SpringAopPointcutExtension {
    fn needs_points_to(&self) -> bool {
        false
    }

    fn handle_call(
        &self,
        context: &mut ExtensionContext,
        call: &CallSiteInfo,
        resolved_targets: &HashSet<(String, bool)>,
    ) -> Result<Option<bool>, CoreError> {
        let advices = self.get_advices(context.conn).map_err(CoreError::Sqlite)?;
        if advices.is_empty() {
            return Ok(None);
        }

        for (target_fqn, _is_virt) in resolved_targets {
            for advice in &advices {
                if matches_pointcut(target_fqn, &advice.pointcut_expr) {
                    let caller = strip_signature(call.caller_method_fqn).to_string();
                    let advice_callee = strip_signature(&advice.advice_method_fqn).to_string();
                    context.call_edges_discovered.insert((caller, advice_callee, true));
                }
            }
        }

        Ok(None)
    }
}
