use crate::JavaError;
use astro_probe_core::traits::FrameworkAnalyzer;
use rusqlite::Connection;
use std::collections::HashMap;
use std::path::Path;

pub struct SpringMvcRouteAnalyzer;

impl SpringMvcRouteAnalyzer {
    pub fn new() -> Self {
        Self
    }
}

impl Default for SpringMvcRouteAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

fn parse_annotation_paths(ann_args: &str) -> Vec<String> {
    let mut args = ann_args.trim();
    if args.is_empty() {
        return vec!["".to_string()];
    }

    if args.starts_with("value") {
        if let Some(eq_idx) = args.find('=') {
            args = args[eq_idx + 1..].trim();
        }
    } else if args.starts_with("path") {
        if let Some(eq_idx) = args.find('=') {
            args = args[eq_idx + 1..].trim();
        }
    }

    let mut paths = Vec::new();
    let chars: Vec<char> = args.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '"' || chars[i] == '\'' {
            let quote = chars[i];
            i += 1;
            let mut path = String::new();
            while i < chars.len() && chars[i] != quote {
                path.push(chars[i]);
                i += 1;
            }
            paths.push(path);
        }
        i += 1;
    }

    if paths.is_empty() {
        let cleaned = args.replace(['{', '}', '"', '\''], "");
        if !cleaned.trim().is_empty() {
            paths.push(cleaned.trim().to_string());
        } else {
            paths.push("".to_string());
        }
    }

    paths
}

fn get_http_methods(ann_name: &str, ann_args: &str) -> Vec<String> {
    match ann_name {
        "GetMapping" => vec!["GET".to_string()],
        "PostMapping" => vec!["POST".to_string()],
        "PutMapping" => vec!["PUT".to_string()],
        "DeleteMapping" => vec!["DELETE".to_string()],
        "PatchMapping" => vec!["PATCH".to_string()],
        "RequestMapping" => {
            let mut methods = Vec::new();
            let upper = ann_args.to_uppercase();
            if upper.contains("REQUESTMETHOD.GET") {
                methods.push("GET".to_string());
            }
            if upper.contains("REQUESTMETHOD.POST") {
                methods.push("POST".to_string());
            }
            if upper.contains("REQUESTMETHOD.PUT") {
                methods.push("PUT".to_string());
            }
            if upper.contains("REQUESTMETHOD.DELETE") {
                methods.push("DELETE".to_string());
            }
            if upper.contains("REQUESTMETHOD.PATCH") {
                methods.push("PATCH".to_string());
            }
            if methods.is_empty() {
                methods.push("ANY".to_string());
            }
            methods
        }
        _ => vec!["ANY".to_string()],
    }
}

fn combine_paths(c: &str, m: &str) -> String {
    let mut c_clean = c.trim().to_string();
    let mut m_clean = m.trim().to_string();

    if !c_clean.is_empty() {
        if !c_clean.starts_with('/') {
            c_clean.insert(0, '/');
        }
        if c_clean.ends_with('/') && c_clean.len() > 1 {
            c_clean.pop();
        }
    }

    if !m_clean.is_empty() {
        if !m_clean.starts_with('/') {
            m_clean.insert(0, '/');
        }
        if m_clean.ends_with('/') && m_clean.len() > 1 {
            m_clean.pop();
        }
    }

    let mut full_path = format!("{}{}", c_clean, m_clean);

    while full_path.contains("//") {
        full_path = full_path.replace("//", "/");
    }

    if full_path.ends_with('/') && full_path.len() > 1 {
        full_path.pop();
    }

    if full_path.is_empty() {
        "/".to_string()
    } else {
        full_path
    }
}

impl FrameworkAnalyzer<Connection> for SpringMvcRouteAnalyzer {
    type Error = JavaError;

    fn analyze(&self, conn: &mut Connection) -> std::result::Result<(), Self::Error> {
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        {
            let conn = &tx;
            conn.execute("DELETE FROM web_routes", [])?;

            let mut stmt = conn.prepare(
                "SELECT DISTINCT class_fqn FROM class_annotations \
                 WHERE annotation_name IN ('Controller', 'RestController') \
                    OR annotation_name LIKE 'Controller:%' \
                    OR annotation_name LIKE 'RestController:%'",
            )?;
            let mut controllers = Vec::new();
            let mut rows = stmt.query([])?;
            while let Some(row) = rows.next()? {
                let fqn: String = row.get(0)?;
                controllers.push(fqn);
            }

            for class_fqn in controllers {
                let mut class_anns = Vec::new();
                let mut class_ann_stmt = conn.prepare(
                    "SELECT annotation_name FROM class_annotations \
                     WHERE class_fqn = ?1 AND (annotation_name = 'RequestMapping' OR annotation_name LIKE 'RequestMapping:%')"
                )?;
                let mut ca_rows = class_ann_stmt.query([&class_fqn])?;
                while let Some(row) = ca_rows.next()? {
                    class_anns.push(row.get::<_, String>(0)?);
                }

                let has_args_class_ann =
                    class_anns.iter().any(|a| a.starts_with("RequestMapping:"));
                let mut class_paths = Vec::new();
                for ann in &class_anns {
                    if ann == "RequestMapping" {
                        if !has_args_class_ann {
                            class_paths.push("".to_string());
                        }
                    } else {
                        let args = &ann["RequestMapping:".len()..];
                        class_paths.extend(parse_annotation_paths(args));
                    }
                }
                if class_paths.is_empty() {
                    class_paths.push("".to_string());
                }

                let mut method_ann_stmt = conn.prepare(
                    "SELECT method_fqn, annotation_name FROM method_annotations \
                     WHERE method_fqn LIKE ?1 AND ( \
                        annotation_name = 'RequestMapping' OR annotation_name LIKE 'RequestMapping:%' OR \
                        annotation_name = 'GetMapping' OR annotation_name LIKE 'GetMapping:%' OR \
                        annotation_name = 'PostMapping' OR annotation_name LIKE 'PostMapping:%' OR \
                        annotation_name = 'PutMapping' OR annotation_name LIKE 'PutMapping:%' OR \
                        annotation_name = 'DeleteMapping' OR annotation_name LIKE 'DeleteMapping:%' OR \
                        annotation_name = 'PatchMapping' OR annotation_name LIKE 'PatchMapping:%' \
                     )"
                )?;

                let method_prefix = format!("{}.", class_fqn);
                let mut m_rows = method_ann_stmt.query([format!("{}%", method_prefix)])?;
                let mut method_to_anns: HashMap<String, Vec<String>> = HashMap::new();
                while let Some(row) = m_rows.next()? {
                    let method_fqn: String = row.get(0)?;
                    let ann_name: String = row.get(1)?;
                    method_to_anns.entry(method_fqn).or_default().push(ann_name);
                }

                for (method_fqn, anns) in method_to_anns {
                    let mut base_to_anns: HashMap<String, Vec<String>> = HashMap::new();
                    for ann in anns {
                        let base = if let Some(colon_idx) = ann.find(':') {
                            ann[..colon_idx].to_string()
                        } else {
                            ann.clone()
                        };
                        base_to_anns.entry(base).or_default().push(ann);
                    }

                    for (ann_base, base_anns) in base_to_anns {
                        let has_args = base_anns.iter().any(|a| a.contains(':'));
                        let mut m_paths = Vec::new();
                        let mut m_methods = Vec::new();

                        for ann in &base_anns {
                            let (base, args) = if let Some(colon_idx) = ann.find(':') {
                                (
                                    ann[..colon_idx].to_string(),
                                    ann[colon_idx + 1..].to_string(),
                                )
                            } else {
                                (ann.clone(), "".to_string())
                            };

                            if ann.contains(':') {
                                m_paths.extend(parse_annotation_paths(&args));
                                m_methods.extend(get_http_methods(&base, &args));
                            } else if !has_args {
                                m_paths.push("".to_string());
                                m_methods.extend(get_http_methods(&base, ""));
                            }
                        }

                        if m_paths.is_empty() {
                            m_paths.push("".to_string());
                        }
                        if m_methods.is_empty() {
                            m_methods.push("ANY".to_string());
                        }

                        m_methods.sort();
                        m_methods.dedup();

                        for c_path in &class_paths {
                            for m_path in &m_paths {
                                let full_path = combine_paths(c_path, m_path);
                                for http_method in &m_methods {
                                    conn.execute(
                                        "INSERT OR REPLACE INTO web_routes (http_method, path, controller_method_fqn) \
                                         VALUES (?1, ?2, ?3)",
                                        [http_method, &full_path, &method_fqn],
                                    )?;
                                }
                            }
                        }
                    }
                }
            }
        }
        tx.commit()?;
        Ok(())
    }
}
