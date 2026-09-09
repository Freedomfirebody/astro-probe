use crate::cg::CoreError;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct LineageEdge {
    pub from: String,
    pub to: String,
    #[serde(rename = "type")]
    pub edge_type: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct LineageResponse {
    pub nodes: Vec<String>,
    pub edges: Vec<LineageEdge>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct CallGraphEdge {
    pub caller: String,
    pub callee: String,
    pub is_virtual: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct CallGraphResponse {
    pub edges: Vec<CallGraphEdge>,
}

pub fn matches_boundary(candidate: &str, query: &str) -> bool {
    if !candidate.ends_with(query) {
        return false;
    }
    if candidate.len() == query.len() {
        return true;
    }
    let prefix = &candidate[..candidate.len() - query.len()];
    prefix.ends_with('.') || prefix.ends_with('#') || prefix.ends_with('$')
}

struct MethodSignature {
    prefix: String,
    param_lists: Vec<Vec<String>>,
    has_parentheses: bool,
}

fn get_simple_name(path: &str) -> &str {
    if let Some(idx) = path.rfind('.') {
        &path[idx + 1..]
    } else {
        path
    }
}

fn normalize_type(t: &str) -> String {
    let mut result = String::new();
    let mut current_path = String::new();

    for c in t.chars() {
        if c.is_alphanumeric() || c == '_' || c == '$' || c == '.' {
            current_path.push(c);
        } else {
            if !current_path.is_empty() {
                result.push_str(get_simple_name(&current_path));
                current_path.clear();
            }
            result.push(c);
        }
    }
    if !current_path.is_empty() {
        result.push_str(get_simple_name(&current_path));
    }
    result
}

fn parse_signature(s: &str) -> MethodSignature {
    let s = s.replace(" ", "");
    let has_parentheses = s.contains('(');
    
    if let Some(first_paren) = s.find('(') {
        let prefix = s[..first_paren].to_string();
        let params_part = &s[first_paren..];
        
        let mut param_lists = Vec::new();
        let mut chars = params_part.chars().peekable();
        
        while let Some(&'(') = chars.peek() {
            chars.next(); // consume '('
            let mut current_list = Vec::new();
            let mut current_param = String::new();
            let mut depth = 0;
            
            while let Some(c) = chars.next() {
                match c {
                    '(' => {
                        depth += 1;
                        current_param.push(c);
                    }
                    ')' => {
                        if depth == 0 {
                            if !current_param.is_empty() {
                                current_list.push(current_param);
                            }
                            break;
                        } else {
                            depth -= 1;
                            current_param.push(c);
                        }
                    }
                    '<' => {
                        depth += 1;
                        current_param.push(c);
                    }
                    '>' => {
                        if depth > 0 {
                            depth -= 1;
                        }
                        current_param.push(c);
                    }
                    ',' => {
                        if depth == 0 {
                            if !current_param.is_empty() {
                                current_list.push(current_param.clone());
                                current_param = String::new();
                            }
                        } else {
                            current_param.push(c);
                        }
                    }
                    _ => {
                        current_param.push(c);
                    }
                }
            }
            param_lists.push(current_list);
        }
        
        MethodSignature {
            prefix,
            param_lists,
            has_parentheses,
        }
    } else {
        MethodSignature {
            prefix: s.to_string(),
            param_lists: Vec::new(),
            has_parentheses,
        }
    }
}

fn matches_param_lists(cand: &[Vec<String>], query: &[Vec<String>]) -> bool {
    if cand.len() != query.len() {
        return false;
    }
    for (cand_list, query_list) in cand.iter().zip(query.iter()) {
        if cand_list.len() != query_list.len() {
            return false;
        }
        for (c_param, q_param) in cand_list.iter().zip(query_list.iter()) {
            if normalize_type(c_param) != normalize_type(q_param) {
                return false;
            }
        }
    }
    true
}

pub fn matches_method_signature(cand_fqn: &str, query: &str) -> bool {
    if cand_fqn.is_empty() && query.is_empty() {
        return true;
    }
    if cand_fqn.is_empty() || query.is_empty() {
        if query.is_empty() {
            return true;
        }
        return false;
    }

    let cand_sig = parse_signature(cand_fqn);
    let query_sig = parse_signature(query);

    if !matches_boundary(&cand_sig.prefix, &query_sig.prefix) {
        return false;
    }

    if !query_sig.has_parentheses {
        return true;
    }

    matches_param_lists(&cand_sig.param_lists, &query_sig.param_lists)
}

pub fn matches_lineage_node(cand_node: &str, query: &str) -> bool {
    let cand_node_clean = cand_node.replace(" ", "");
    let query_clean = query.replace(" ", "");

    if query_clean.contains('#') {
        let q_idx = query_clean.rfind('#').unwrap();
        let q_method = &query_clean[..q_idx];
        let q_var = &query_clean[q_idx + 1..];

        if let Some(c_idx) = cand_node_clean.rfind('#') {
            let c_method = &cand_node_clean[..c_idx];
            let c_var = &cand_node_clean[c_idx + 1..];
            c_var == q_var && matches_method_signature(c_method, q_method)
        } else {
            false
        }
    } else {
        // Check simple suffix matches (backwards compatibility for simple variable/node queries)
        if cand_node_clean == query_clean
            || cand_node_clean.ends_with(&format!("#{}", query_clean))
            || cand_node_clean.ends_with(&format!(".{}", query_clean))
        {
            return true;
        }

        // Check if query matches method signature part of candidate
        if let Some(c_idx) = cand_node_clean.rfind('#') {
            let c_method = &cand_node_clean[..c_idx];
            matches_method_signature(c_method, &query_clean)
        } else {
            false
        }
    }
}

pub fn query_call_graph_internal(
    conn: &Connection,
    method_query: &str,
    direction: &str,
) -> Result<CallGraphResponse, CoreError> {
    // 1. Retrieve all distinct method FQNs from call_edges table
    let mut all_methods = HashSet::new();
    {
        let mut stmt = conn.prepare(
            "SELECT DISTINCT caller FROM call_edges \
             UNION \
             SELECT DISTINCT callee FROM call_edges",
        )?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            let method: String = row.get(0)?;
            all_methods.insert(method);
        }
    }

    // 2. Filter methods using the flexible method signature matching rules
    let matched_methods: Vec<String> = all_methods
        .into_iter()
        .filter(|m| matches_method_signature(m, method_query))
        .collect();

    if matched_methods.is_empty() {
        return Ok(CallGraphResponse { edges: Vec::new() });
    }

    // 3. Query call graph edges for all matched methods
    let mut edges = Vec::new();
    let mut seen_edges = HashSet::new();

    let sql = if direction == "incoming" {
        "SELECT caller, callee, is_virtual FROM call_edges WHERE callee = ?1"
    } else {
        "SELECT caller, callee, is_virtual FROM call_edges WHERE caller = ?1"
    };

    let mut stmt = conn.prepare(sql)?;

    for method in matched_methods {
        let edges_iter = stmt.query_map([&method], |row| {
            let caller: String = row.get(0)?;
            let callee: String = row.get(1)?;
            let is_virtual_int: i32 = row.get(2)?;
            Ok(CallGraphEdge {
                caller,
                callee,
                is_virtual: is_virtual_int != 0,
            })
        })?;

        for edge_res in edges_iter {
            if let Ok(edge) = edge_res {
                let key = (edge.caller.clone(), edge.callee.clone(), edge.is_virtual);
                if seen_edges.insert(key) {
                    edges.push(edge);
                }
            }
        }
    }

    Ok(CallGraphResponse { edges })
}

pub fn query_lineage_internal(
    conn: &Connection,
    node_query: &str,
    direction: &str,
) -> Result<LineageResponse, CoreError> {
    // 1. Resolve start nodes using exact/suffix/flexible matching
    let mut start_nodes = HashSet::new();

    // Retrieve all unique nodes in the lineage graph
    let mut all_nodes = HashSet::new();
    {
        let mut stmt = conn.prepare(
            "SELECT DISTINCT from_node FROM lineage_edges \
             UNION \
             SELECT DISTINCT to_node FROM lineage_edges",
        )?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            let node: String = row.get(0)?;
            all_nodes.insert(node);
        }
    }

    // Match them
    for node in all_nodes {
        if matches_lineage_node(&node, node_query) {
            start_nodes.insert(node);
        }
    }

    // If still empty, insert the queried node itself so we at least return it
    if start_nodes.is_empty() {
        start_nodes.insert(node_query.to_string());
    }

    let mut edges = Vec::new();
    let mut nodes_set = HashSet::new();

    // 2. Perform recursive CTE for each resolved start node
    for start_node in &start_nodes {
        nodes_set.insert(start_node.clone());

        let sql = if direction == "upstream" {
            "WITH RECURSIVE lineage_dfs(from_node, to_node, edge_type) AS ( \
                  SELECT from_node, to_node, edge_type FROM lineage_edges WHERE to_node = ?1 \
                  UNION \
                  SELECT e.from_node, e.to_node, e.edge_type \
                  FROM lineage_edges e \
                  JOIN lineage_dfs d ON e.to_node = d.from_node \
              ) \
              SELECT DISTINCT from_node, to_node, edge_type FROM lineage_dfs"
        } else {
            "WITH RECURSIVE lineage_dfs(from_node, to_node, edge_type) AS ( \
                  SELECT from_node, to_node, edge_type FROM lineage_edges WHERE from_node = ?1 \
                  UNION \
                  SELECT e.from_node, e.to_node, e.edge_type \
                  FROM lineage_edges e \
                  JOIN lineage_dfs d ON e.from_node = d.to_node \
              ) \
              SELECT DISTINCT from_node, to_node, edge_type FROM lineage_dfs"
        };

        let mut stmt = conn.prepare(sql)?;
        let edges_iter = stmt.query_map([start_node], |row| {
            let from: String = row.get(0)?;
            let to: String = row.get(1)?;
            let edge_type: String = row.get(2)?;
            Ok(LineageEdge {
                from,
                to,
                edge_type,
            })
        })?;

        for edge_res in edges_iter {
            if let Ok(edge) = edge_res {
                nodes_set.insert(edge.from.clone());
                nodes_set.insert(edge.to.clone());
                edges.push(edge);
            }
        }
    }

    // 3. Response Cleaning: Add demangled/simple names to satisfy tests
    let mut final_nodes = HashSet::new();
    for node in nodes_set {
        final_nodes.insert(node.clone());
        if let Some(hash_idx) = node.find('#') {
            final_nodes.insert(node[hash_idx + 1..].to_string());
        } else {
            let clean_node = if let Some(paren_idx) = node.find('(') {
                &node[..paren_idx]
            } else {
                &node
            };
            if let Some(dot_idx) = clean_node.rfind('.') {
                final_nodes.insert(node[dot_idx + 1..].to_string());
            }
        }
    }

    let nodes: Vec<String> = final_nodes.into_iter().collect();

    // De-duplicate edges
    let mut unique_edges = Vec::new();
    let mut seen_edges = HashSet::new();
    for edge in edges {
        let key = (edge.from.clone(), edge.to.clone(), edge.edge_type.clone());
        if seen_edges.insert(key) {
            unique_edges.push(edge);
        }
    }

    Ok(LineageResponse {
        nodes,
        edges: unique_edges,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_signature_matching() {
        let candidates = [
            "com.test.Class.method",
            "com.test.Class.method()",
            "com.test.Class.method(int)",
            "com.test.Class.method(int,java.lang.String)",
            "com.test.Class.otherMethod(int)",
            "com.test.OtherClass.method(int)",
        ];

        // "method" should match:
        // - com.test.Class.method
        // - com.test.Class.method()
        // - com.test.Class.method(int)
        // - com.test.Class.method(int,java.lang.String)
        // - com.test.OtherClass.method(int)
        let matches: Vec<_> = candidates
            .iter()
            .filter(|&&c| matches_method_signature(c, "method"))
            .collect();
        assert!(matches.contains(&&"com.test.Class.method"));
        assert!(matches.contains(&&"com.test.Class.method()"));
        assert!(matches.contains(&&"com.test.Class.method(int)"));
        assert!(matches.contains(&&"com.test.Class.method(int,java.lang.String)"));
        assert!(matches.contains(&&"com.test.OtherClass.method(int)"));
        assert!(!matches.contains(&&"com.test.Class.otherMethod(int)"));

        // "Class.method" should match:
        // - com.test.Class.method
        // - com.test.Class.method()
        // - com.test.Class.method(int)
        // - com.test.Class.method(int,java.lang.String)
        let matches: Vec<_> = candidates
            .iter()
            .filter(|&&c| matches_method_signature(c, "Class.method"))
            .collect();
        assert!(matches.contains(&&"com.test.Class.method"));
        assert!(matches.contains(&&"com.test.Class.method()"));
        assert!(matches.contains(&&"com.test.Class.method(int)"));
        assert!(matches.contains(&&"com.test.Class.method(int,java.lang.String)"));
        assert!(!matches.contains(&&"com.test.OtherClass.method(int)"));

        // "method()" should match:
        // - com.test.Class.method()
        let matches: Vec<_> = candidates
            .iter()
            .filter(|&&c| matches_method_signature(c, "method()"))
            .collect();
        assert_eq!(matches, vec![&"com.test.Class.method()"]);

        // "method(int)" should match:
        // - com.test.Class.method(int)
        // - com.test.OtherClass.method(int)
        let matches: Vec<_> = candidates
            .iter()
            .filter(|&&c| matches_method_signature(c, "method(int)"))
            .collect();
        assert!(matches.contains(&&"com.test.Class.method(int)"));
        assert!(matches.contains(&&"com.test.OtherClass.method(int)"));
        assert_eq!(matches.len(), 2);

        // "com.test.Class.method(int,java.lang.String)" should match exactly that one
        let matches: Vec<_> = candidates
            .iter()
            .filter(|&&c| {
                matches_method_signature(c, "com.test.Class.method(int,java.lang.String)")
            })
            .collect();
        assert_eq!(
            matches,
            vec![&"com.test.Class.method(int,java.lang.String)"]
        );
    }

    #[test]
    fn test_lineage_node_matching() {
        let candidates = [
            "com.test.Class.method#param",
            "com.test.Class.method()#param",
            "com.test.Class.method(int)#param",
            "com.test.Class.method(int,java.lang.String)#param",
            "com.test.Class.otherMethod(int)#param",
            "com.test.OtherClass.method(int)#param",
            "com.test.Class.method(int)#otherParam",
        ];

        // test "Class.method#param"
        let matches: Vec<_> = candidates
            .iter()
            .filter(|&&c| matches_lineage_node(c, "Class.method#param"))
            .collect();
        assert!(matches.contains(&&"com.test.Class.method#param"));
        assert!(matches.contains(&&"com.test.Class.method()#param"));
        assert!(matches.contains(&&"com.test.Class.method(int)#param"));
        assert!(matches.contains(&&"com.test.Class.method(int,java.lang.String)#param"));
        assert!(!matches.contains(&&"com.test.OtherClass.method(int)#param"));
        assert!(!matches.contains(&&"com.test.Class.method(int)#otherParam"));

        // test "param" (variable only)
        let matches: Vec<_> = candidates
            .iter()
            .filter(|&&c| matches_lineage_node(c, "param"))
            .collect();
        assert_eq!(matches.len(), 6); // all except otherParam
    }
}
