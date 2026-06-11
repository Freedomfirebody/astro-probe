use std::collections::{HashMap, HashSet};
use rusqlite::Connection;
use anyhow::Result;

pub struct DependencyInjectionAnalyzer;

impl DependencyInjectionAnalyzer {
    pub fn new() -> Self {
        Self
    }

    pub fn analyze(&self, conn: &Connection) -> Result<()> {
        // Step 1: Query all class annotations to find Spring beans.
        let mut stmt = conn.prepare(
            "SELECT class_fqn, annotation_name FROM class_annotations \
             WHERE annotation_name IN ('Component', 'Service', 'Repository', 'RestController', 'Controller') \
                OR annotation_name LIKE 'Component:%' \
                OR annotation_name LIKE 'Service:%' \
                OR annotation_name LIKE 'Repository:%' \
                OR annotation_name LIKE 'RestController:%' \
                OR annotation_name LIKE 'Controller:%'"
        )?;
        
        let mut beans = Vec::new(); // vector of (class_fqn, bean_name)
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            let class_fqn: String = row.get(0)?;
            let ann_name: String = row.get(1)?;
            let bean_name = if let Some(colon_idx) = ann_name.find(':') {
                ann_name[colon_idx + 1..].to_string()
            } else {
                // Decapitalize simple class name
                let simple_name = if let Some(dot_idx) = class_fqn.rfind('.') {
                    &class_fqn[dot_idx + 1..]
                } else {
                    &class_fqn
                };
                let mut chars = simple_name.chars();
                match chars.next() {
                    None => String::new(),
                    Some(c) => c.to_lowercase().collect::<String>() + chars.as_str(),
                }
            };
            beans.push((class_fqn, bean_name));
        }

        // De-duplicate beans by class_fqn
        let mut seen_beans = HashSet::new();
        beans.retain(|(fqn, _)| seen_beans.insert(fqn.clone()));

        // Step 2: Register bean allocations and map `this` pointers
        for (class_fqn, _) in &beans {
            let alloc_id = format!("SpringBeanAlloc:{}", class_fqn);
            conn.execute(
                "INSERT OR IGNORE INTO allocation_sites (alloc_id, class_fqn, method_fqn) VALUES (?1, ?2, 'SpringDI')",
                [&alloc_id, class_fqn],
            )?;

            // Assign this bean to M#this for all methods M in class_fqn
            let mut m_stmt = conn.prepare("SELECT method_fqn FROM method_declarations WHERE class_fqn = ?1")?;
            let mut m_rows = m_stmt.query([class_fqn])?;
            while let Some(row) = m_rows.next()? {
                let method_fqn: String = row.get(0)?;
                let this_node = format!("{}#this", method_fqn);
                conn.execute(
                    "INSERT INTO source_assignments (lhs, rhs, assignment_type, method_fqn) VALUES (?1, ?2, 'ALLOC', 'SpringDI')",
                    [&this_node, &alloc_id],
                )?;
            }
        }

        // Build class inheritance cache for sub-typing
        let mut hierarchy_stmt = conn.prepare("SELECT class_fqn, parent_fqn FROM class_hierarchy")?;
        let mut h_rows = hierarchy_stmt.query([])?;
        let mut parent_map: HashMap<String, Vec<String>> = HashMap::new();
        while let Some(row) = h_rows.next()? {
            let child: String = row.get(0)?;
            let parent: String = row.get(1)?;
            parent_map.entry(child).or_default().push(parent);
        }

        // Helper closure to get all parents of a class (transitive closure)
        let get_transitive_parents = |start_class: &str| -> HashSet<String> {
            let mut visited = HashSet::new();
            let mut queue = vec![start_class.to_string()];
            while let Some(current) = queue.pop() {
                if visited.insert(current.clone()) {
                    if let Some(parents) = parent_map.get(&current) {
                        for p in parents {
                            if !visited.contains(p) {
                                queue.push(p.clone());
                            }
                        }
                    }
                }
            }
            visited
        };

        // Step 3: Autowire fields
        let mut field_stmt = conn.prepare(
            "SELECT class_fqn, field_name FROM field_annotations WHERE annotation_name IN ('Autowired', 'Resource')"
        )?;
        let mut f_rows = field_stmt.query([])?;
        let mut autowired_fields = Vec::new();
        while let Some(row) = f_rows.next()? {
            let class_fqn: String = row.get(0)?;
            let field_name: String = row.get(1)?;
            autowired_fields.push((class_fqn, field_name));
        }

        for (owner_class, field_name) in autowired_fields {
            // Find field type
            let mut type_stmt = conn.prepare(
                "SELECT annotation_name FROM field_annotations \
                 WHERE class_fqn = ?1 AND field_name = ?2 AND annotation_name LIKE 'FieldType:%' LIMIT 1"
            )?;
            let field_type_ann: Option<String> = type_stmt.query_row([&owner_class, &field_name], |r| r.get(0)).ok();
            let field_type = match field_type_ann {
                Some(ann) => ann["FieldType:".len()..].to_string(),
                None => continue,
            };

            // Check for @Qualifier annotation on this field
            let mut qual_stmt = conn.prepare(
                "SELECT annotation_name FROM field_annotations \
                 WHERE class_fqn = ?1 AND field_name = ?2 AND annotation_name LIKE 'Qualifier:%' LIMIT 1"
            )?;
            let qualifier_val: Option<String> = qual_stmt.query_row([&owner_class, &field_name], |r| r.get(0))
                .ok()
                .map(|ann: String| ann["Qualifier:".len()..].to_string());

            // Find candidates matching field type (either direct class or subclass/implementer)
            let mut candidates = Vec::new();
            for (bean_class, bean_name) in &beans {
                let is_match = bean_class == &field_type || get_transitive_parents(bean_class).contains(&field_type);
                if is_match {
                    candidates.push((bean_class.clone(), bean_name.clone()));
                }
            }

            // Apply qualifier filtering
            if let Some(ref q) = qualifier_val {
                candidates.retain(|(c, n)| n == q || c == q || c.ends_with(&format!(".{}", q)));
            } else if candidates.len() > 1 {
                // Fallback: match by name
                if let Some(exact_match) = candidates.iter().find(|(_, n)| n == &field_name) {
                    candidates = vec![exact_match.clone()];
                }
            }

            // Inject the matching beans
            for (bean_class, _) in candidates {
                let field_node = format!("SpringBeanAlloc:{}.{}", owner_class, field_name);
                let bean_alloc = format!("SpringBeanAlloc:{}", bean_class);
                conn.execute(
                    "INSERT INTO source_assignments (lhs, rhs, assignment_type, method_fqn) VALUES (?1, ?2, 'ALLOC', 'SpringDI')",
                    [&field_node, &bean_alloc],
                )?;
            }
        }

        // Step 4: Autowire constructor / method parameters
        let mut m_decl_stmt = conn.prepare(
            "SELECT method_fqn, class_fqn, method_name, params FROM method_declarations"
        )?;
        let mut m_rows = m_decl_stmt.query([])?;
        let mut autowired_methods = Vec::new();
        while let Some(row) = m_rows.next()? {
            let method_fqn: String = row.get(0)?;
            let class_fqn: String = row.get(1)?;
            let method_name: String = row.get(2)?;
            let params: String = row.get(3)?;
            
            // Check if class is a Spring bean
            let is_bean = beans.iter().any(|(bfqn, _)| bfqn == &class_fqn);
            if !is_bean {
                continue;
            }

            // Check if method is annotated with @Autowired
            let mut is_autowired: bool = conn.query_row(
                "SELECT count(*) FROM method_annotations WHERE method_fqn = ?1 AND annotation_name IN ('Autowired', 'Resource')",
                [&method_fqn],
                |r| r.get::<_, i64>(0)
            )? > 0;

            // Also implicitly autowire constructor if it has parameters and class is a Spring bean
            if !is_autowired && method_name == "<init>" && !params.trim().is_empty() {
                is_autowired = true;
            }

            if is_autowired {
                autowired_methods.push((method_fqn, params));
            }
        }

        let parse_param_types = |method_fqn: &str| -> Vec<String> {
            if let Some(start) = method_fqn.find('(') {
                if let Some(end) = method_fqn.rfind(')') {
                    let content = &method_fqn[start + 1..end];
                    if content.trim().is_empty() {
                        return Vec::new();
                    }
                    return content.split(',').map(|s| s.trim().to_string()).collect();
                }
            }
            Vec::new()
        };

        for (method_fqn, params_str) in autowired_methods {
            let param_names: Vec<String> = params_str.split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            let param_types = parse_param_types(&method_fqn);
            
            for (i, p_name) in param_names.iter().enumerate() {
                let p_type = match param_types.get(i) {
                    Some(t) => t,
                    None => continue,
                };

                // Find candidate beans matching parameter type
                let mut candidates = Vec::new();
                for (bean_class, bean_name) in &beans {
                    let is_match = bean_class == p_type || get_transitive_parents(bean_class).contains(p_type);
                    if is_match {
                        candidates.push((bean_class.clone(), bean_name.clone()));
                    }
                }

                // Check for @Qualifier annotation on this parameter
                let mut param_qual_stmt = conn.prepare(
                    "SELECT annotation_name FROM parameter_annotations \
                     WHERE method_fqn = ?1 AND parameter_name = ?2 AND annotation_name LIKE 'Qualifier:%' LIMIT 1"
                )?;
                let qualifier_val: Option<String> = param_qual_stmt.query_row([&method_fqn, p_name], |r| r.get(0))
                    .ok()
                    .map(|ann: String| ann["Qualifier:".len()..].to_string());

                if let Some(ref q) = qualifier_val {
                    candidates.retain(|(c, n)| n == q || c == q || c.ends_with(&format!(".{}", q)));
                } else if candidates.len() > 1 {
                    // Match by name
                    if let Some(exact_match) = candidates.iter().find(|(_, n)| n == p_name) {
                        candidates = vec![exact_match.clone()];
                    }
                }

                // Inject the matching beans
                for (bean_class, _) in candidates {
                    let lhs_node = format!("{}#{}", method_fqn, p_name);
                    let bean_alloc = format!("SpringBeanAlloc:{}", bean_class);
                    conn.execute(
                        "INSERT INTO source_assignments (lhs, rhs, assignment_type, method_fqn) VALUES (?1, ?2, 'ALLOC', 'SpringDI')",
                        [&lhs_node, &bean_alloc],
                    )?;
                }
            }
        }

        // Step 5: Value Property Injection
        let mut val_stmt = conn.prepare(
            "SELECT class_fqn, field_name, annotation_name FROM field_annotations WHERE annotation_name LIKE 'Value:%'"
        )?;
        let mut v_rows = val_stmt.query([])?;
        while let Some(row) = v_rows.next()? {
            let owner_class: String = row.get(0)?;
            let field_name: String = row.get(1)?;
            let ann_name: String = row.get(2)?;
            let raw_val = ann_name["Value:".len()..].to_string();
            let mut val = raw_val.trim().to_string();
            if val.starts_with("${") && val.ends_with('}') {
                val = val[2..val.len() - 1].to_string();
            }
            if val.starts_with('"') && val.ends_with('"') {
                val = val[1..val.len() - 1].to_string();
            }

            let str_alloc_id = format!("StringAlloc:{}", val);
            // Register string allocation site
            conn.execute(
                "INSERT OR IGNORE INTO allocation_sites (alloc_id, class_fqn, method_fqn) VALUES (?1, 'java.lang.String', 'SpringDI')",
                [&str_alloc_id],
            )?;

            // Assign to field node
            let field_node = format!("SpringBeanAlloc:{}.{}", owner_class, field_name);
            conn.execute(
                "INSERT INTO source_assignments (lhs, rhs, assignment_type, method_fqn) VALUES (?1, ?2, 'ALLOC', 'SpringDI')",
                [&field_node, &str_alloc_id],
            )?;
        }

        Ok(())
    }
}

impl Default for DependencyInjectionAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}
