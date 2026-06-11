use std::sync::Arc;
use tokio::io::{self, AsyncBufReadExt, AsyncWriteExt, BufReader};
use serde_json::{json, Value};
use crate::kernel::WorkspaceManager;
use crate::mcp::schema::{JsonRpcRequest, JsonRpcResponse};

pub struct McpServer {
    manager: Arc<WorkspaceManager>,
}

impl McpServer {
    pub fn new(manager: Arc<WorkspaceManager>) -> Self {
        Self { manager }
    }

    pub async fn run(&self) -> io::Result<()> {
        let stdin = io::stdin();
        let mut reader = BufReader::new(stdin).lines();
        let mut stdout = io::stdout();

        while let Some(line) = reader.next_line().await? {
            if let Some(response) = self.handle_line(&line).await {
                let response_str = serde_json::to_string(&response).unwrap();
                stdout.write_all(response_str.as_bytes()).await?;
                stdout.write_all(b"\n").await?;
                stdout.flush().await?;
            }
        }

        Ok(())
    }

    async fn handle_line(&self, line: &str) -> Option<JsonRpcResponse> {
        let req: JsonRpcRequest = match serde_json::from_str(line) {
            Ok(r) => r,
            Err(e) => {
                return Some(JsonRpcResponse::error(None, -32700, &format!("Parse error: {}", e)));
            }
        };

        let is_notification = req.id.is_none();

        let method = req.method.as_str();
        let resp = match method {
            "initialize" => {
                let result = json!({
                    "protocolVersion": "2024-11-05",
                    "capabilities": {
                        "tools": {}
                    },
                    "serverInfo": {
                        "name": "astro-probe",
                        "version": "0.1.0"
                    }
                });
                JsonRpcResponse::success(req.id.clone(), result)
            }
            "tools/list" => {
                let result = json!({
                    "tools": [
                        {
                            "name": "workspace_create",
                            "description": "Create a new code analysis workspace",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "name": { "type": "string" },
                                    "project_path": { "type": "string" }
                                },
                                "required": ["name", "project_path"]
                            }
                        },
                        {
                            "name": "workspace_list",
                            "description": "List all registered workspaces",
                            "inputSchema": {
                                "type": "object",
                                "properties": {}
                            }
                        },
                        {
                            "name": "workspace_delete",
                            "description": "Delete a workspace by ID",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "id": { "type": "string" }
                                },
                                "required": ["id"]
                            }
                        },
                        {
                            "name": "workspace_start",
                            "description": "Start/load a workspace by ID",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "id": { "type": "string" }
                                },
                                "required": ["id"]
                            }
                        },
                        {
                            "name": "workspace_stop",
                            "description": "Stop/unload a workspace by ID",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "id": { "type": "string" }
                                },
                                "required": ["id"]
                            }
                        },
                        {
                            "name": "query_call_graph",
                            "description": "Query call graph edges for a method in a workspace",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "workspace_id": { "type": "string" },
                                    "method": { "type": "string" },
                                    "direction": { "type": "string", "enum": ["incoming", "outgoing"] }
                                },
                                "required": ["workspace_id", "method", "direction"]
                            }
                        },
                        {
                            "name": "query_lineage",
                            "description": "Query variable data flow/call lineage for a node in a workspace",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "workspace_id": { "type": "string" },
                                    "node": { "type": "string" },
                                    "direction": { "type": "string", "enum": ["upstream", "downstream"] }
                                },
                                "required": ["workspace_id", "node", "direction"]
                            }
                        }
                    ]
                });
                JsonRpcResponse::success(req.id.clone(), result)
            }
            "tools/call" => {
                let params = match req.params {
                    Some(ref p) => p,
                    None => return Some(JsonRpcResponse::error(req.id.clone(), -32602, "Missing parameters")),
                };
                let tool_name = match params.get("name").and_then(|v| v.as_str()) {
                    Some(n) => n,
                    None => return Some(JsonRpcResponse::error(req.id.clone(), -32602, "Missing tool name")),
                };
                let arguments = params.get("arguments").cloned().unwrap_or(json!({}));

                match self.handle_tool_call(tool_name, arguments).await {
                    Ok(result) => JsonRpcResponse::success(req.id.clone(), result),
                    Err(err_msg) => JsonRpcResponse::error(req.id.clone(), -32603, &err_msg),
                }
            }
            _ => {
                JsonRpcResponse::error(req.id.clone(), -32601, &format!("Method not found: {}", method))
            }
        };

        if is_notification {
            None
        } else {
            Some(resp)
        }
    }

    async fn handle_tool_call(&self, name: &str, args: Value) -> Result<Value, String> {
        match name {
            "workspace_create" => {
                let name_param = args.get("name").and_then(|v| v.as_str())
                    .ok_or_else(|| "Missing required parameter: name".to_string())?;
                let path_param = args.get("project_path").and_then(|v| v.as_str())
                    .ok_or_else(|| "Missing required parameter: project_path".to_string())?;

                match self.manager.create_workspace(name_param.to_string(), path_param.to_string()) {
                    Ok(ws) => Ok(json!({
                        "content": [
                            {
                                "type": "text",
                                "text": serde_json::to_string(&ws).unwrap()
                            }
                        ]
                    })),
                    Err(e) => Err(e.to_string()),
                }
            }
            "workspace_list" => {
                let workspaces = self.manager.list_workspaces();
                Ok(json!({
                    "content": [
                        {
                            "type": "text",
                            "text": serde_json::to_string(&workspaces).unwrap()
                        }
                    ]
                }))
            }
            "workspace_delete" => {
                let id_param = args.get("id").and_then(|v| v.as_str())
                    .ok_or_else(|| "Missing required parameter: id".to_string())?;

                let success = self.manager.delete_workspace(id_param);
                Ok(json!({
                    "content": [
                        {
                            "type": "text",
                            "text": json!({ "success": success }).to_string()
                        }
                    ]
                }))
            }
            "workspace_start" => {
                let id_param = args.get("id").and_then(|v| v.as_str())
                    .ok_or_else(|| "Missing required parameter: id".to_string())?;

                match self.manager.start_workspace(id_param) {
                    Some(ws) => Ok(json!({
                        "content": [
                            {
                                "type": "text",
                                "text": serde_json::to_string(&ws).unwrap()
                            }
                        ]
                    })),
                    None => Err("Workspace not found".to_string()),
                }
            }
            "workspace_stop" => {
                let id_param = args.get("id").and_then(|v| v.as_str())
                    .ok_or_else(|| "Missing required parameter: id".to_string())?;

                match self.manager.stop_workspace(id_param) {
                    Some(ws) => Ok(json!({
                        "content": [
                            {
                                "type": "text",
                                "text": serde_json::to_string(&ws).unwrap()
                            }
                        ]
                    })),
                    None => Err("Workspace not found".to_string()),
                }
            }
            "query_call_graph" => {
                let workspace_id = args.get("workspace_id").and_then(|v| v.as_str())
                    .ok_or_else(|| "Missing required parameter: workspace_id".to_string())?;
                let method = args.get("method").and_then(|v| v.as_str())
                    .ok_or_else(|| "Missing required parameter: method".to_string())?;
                let direction = args.get("direction").and_then(|v| v.as_str())
                    .ok_or_else(|| "Missing required parameter: direction".to_string())?;

                let pool = self.manager.get_db_pool_and_touch(workspace_id)
                    .ok_or_else(|| "Workspace not found".to_string())?;

                let conn = pool.get()
                    .map_err(|e| format!("Failed to get DB connection: {}", e))?;

                let sql = if direction == "incoming" {
                    "SELECT caller, callee, is_virtual FROM call_edges WHERE callee = ?1"
                } else {
                    "SELECT caller, callee, is_virtual FROM call_edges WHERE caller = ?1"
                };

                let mut stmt = conn.prepare(sql)
                    .map_err(|e| format!("Failed to prepare SQL: {}", e))?;

                let edges_iter = stmt.query_map([method], |row| {
                    let caller: String = row.get(0)?;
                    let callee: String = row.get(1)?;
                    let is_virtual_int: i32 = row.get(2)?;
                    Ok(json!({
                        "caller": caller,
                        "callee": callee,
                        "is_virtual": is_virtual_int != 0
                    }))
                }).map_err(|e| format!("Failed to execute SQL: {}", e))?;

                let mut edges = Vec::new();
                for edge_res in edges_iter {
                    if let Ok(edge) = edge_res {
                        edges.push(edge);
                    }
                }

                let response = json!({
                    "edges": edges
                });

                Ok(json!({
                    "content": [
                        {
                            "type": "text",
                            "text": response.to_string()
                        }
                    ]
                }))
            }
            "query_lineage" => {
                let workspace_id = args.get("workspace_id").and_then(|v| v.as_str())
                    .ok_or_else(|| "Missing required parameter: workspace_id".to_string())?;
                let node = args.get("node").and_then(|v| v.as_str())
                    .ok_or_else(|| "Missing required parameter: node".to_string())?;
                let direction = args.get("direction").and_then(|v| v.as_str())
                    .ok_or_else(|| "Missing required parameter: direction".to_string())?;

                let pool = self.manager.get_db_pool_and_touch(workspace_id)
                    .ok_or_else(|| "Workspace not found".to_string())?;

                let conn = pool.get()
                    .map_err(|e| format!("Failed to get DB connection: {}", e))?;

                match crate::api::handlers::query_lineage_internal(&conn, node, direction) {
                    Ok(resp) => {
                        Ok(json!({
                            "content": [
                                {
                                    "type": "text",
                                    "text": serde_json::to_string(&resp).unwrap()
                                }
                            ]
                        }))
                    }
                    Err(e) => Err(format!("Lineage query failed: {}", e)),
                }
            }
            _ => Err(format!("Unknown tool: {}", name)),
        }
    }
}
