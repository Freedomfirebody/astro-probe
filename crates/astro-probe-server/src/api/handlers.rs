use crate::kernel::WorkspaceManager;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub manager: Arc<WorkspaceManager>,
}

#[derive(Debug, Deserialize)]
pub struct CreateWorkspaceRequest {
    pub name: String,
    pub project_path: String,
}

#[derive(Debug, Serialize)]
pub struct WorkspaceResponse {
    pub id: String,
    pub name: String,
    pub project_path: String,
    pub status: String,
}

#[derive(Debug, Serialize)]
pub struct WorkspaceListItem {
    pub id: String,
    pub name: String,
    pub status: String,
}

#[derive(Debug, Serialize)]
pub struct DeleteWorkspaceResponse {
    pub success: bool,
}

#[derive(Debug, Serialize)]
pub struct StartWorkspaceResponse {
    pub id: String,
    pub status: String,
}

#[derive(Debug, Serialize)]
pub struct StopWorkspaceResponse {
    pub id: String,
    pub status: String,
}

#[derive(Debug, Deserialize)]
pub struct CallGraphQueryParams {
    pub method: String,
    pub direction: String,
}

#[derive(Debug, Serialize)]
pub struct CallGraphEdge {
    pub caller: String,
    pub callee: String,
    pub is_virtual: bool,
}

#[derive(Debug, Serialize)]
pub struct CallGraphResponse {
    pub edges: Vec<CallGraphEdge>,
}

#[derive(Debug, Deserialize)]
pub struct LineageQueryParams {
    pub node: String,
    pub direction: String,
}

pub use astro_probe_core::query::{query_lineage_internal, LineageEdge, LineageResponse};

#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub error: String,
}

pub async fn create_workspace(
    State(state): State<AppState>,
    Json(payload): Json<CreateWorkspaceRequest>,
) -> impl IntoResponse {
    match state
        .manager
        .create_workspace(payload.name, payload.project_path)
    {
        Ok(ws) => {
            let resp = WorkspaceResponse {
                id: ws.id,
                name: ws.name,
                project_path: ws.project_path,
                status: ws.status.to_string(),
            };
            (StatusCode::CREATED, Json(resp)).into_response()
        }
        Err(e) => {
            let err_msg = e.to_string();
            let status = if err_msg.contains("NotFound")
                || err_msg.contains("Invalid")
                || err_msg.contains("empty")
            {
                StatusCode::BAD_REQUEST
            } else {
                StatusCode::INTERNAL_SERVER_ERROR
            };
            (status, Json(ErrorResponse { error: err_msg })).into_response()
        }
    }
}

pub async fn list_workspaces(State(state): State<AppState>) -> impl IntoResponse {
    let list = state.manager.list_workspaces();
    let resp: Vec<WorkspaceListItem> = list
        .into_iter()
        .map(|ws| WorkspaceListItem {
            id: ws.id,
            name: ws.name,
            status: ws.status.to_string(),
        })
        .collect();
    Json(resp)
}

pub async fn delete_workspace(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let success = state.manager.delete_workspace(&id);
    if success {
        Json(DeleteWorkspaceResponse { success: true }).into_response()
    } else {
        (
            StatusCode::NOT_FOUND,
            Json(DeleteWorkspaceResponse { success: false }),
        )
            .into_response()
    }
}

pub async fn start_workspace(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if let Some(ws) = state.manager.start_workspace(&id) {
        Json(StartWorkspaceResponse {
            id: ws.id,
            status: ws.status.to_string(),
        })
        .into_response()
    } else {
        StatusCode::NOT_FOUND.into_response()
    }
}

pub async fn stop_workspace(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if let Some(ws) = state.manager.stop_workspace(&id) {
        Json(StopWorkspaceResponse {
            id: ws.id,
            status: ws.status.to_string(),
        })
        .into_response()
    } else {
        StatusCode::NOT_FOUND.into_response()
    }
}

pub async fn query_call_graph(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(params): Query<CallGraphQueryParams>,
) -> impl IntoResponse {
    let pool = match state.manager.get_db_pool_and_touch(&id) {
        Some(p) => p,
        None => return (StatusCode::NOT_FOUND, "Workspace not found").into_response(),
    };

    let conn = match pool.get() {
        Ok(c) => c,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to get DB connection: {}", e),
            )
                .into_response()
        }
    };

    let sql = if params.direction == "incoming" {
        "SELECT caller, callee, is_virtual FROM call_edges WHERE callee = ?1"
    } else {
        "SELECT caller, callee, is_virtual FROM call_edges WHERE caller = ?1"
    };

    let mut stmt = match conn.prepare(sql) {
        Ok(s) => s,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to prepare SQL: {}", e),
            )
                .into_response()
        }
    };

    let edges_iter = match stmt.query_map([&params.method], |row| {
        let caller: String = row.get(0)?;
        let callee: String = row.get(1)?;
        let is_virtual_int: i32 = row.get(2)?;
        Ok(CallGraphEdge {
            caller,
            callee,
            is_virtual: is_virtual_int != 0,
        })
    }) {
        Ok(it) => it,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to execute SQL: {}", e),
            )
                .into_response()
        }
    };

    let mut edges = Vec::new();
    for edge in edges_iter.flatten() {
        edges.push(edge);
    }

    Json(CallGraphResponse { edges }).into_response()
}

pub async fn query_lineage(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(params): Query<LineageQueryParams>,
) -> impl IntoResponse {
    let pool = match state.manager.get_db_pool_and_touch(&id) {
        Some(p) => p,
        None => return (StatusCode::NOT_FOUND, "Workspace not found").into_response(),
    };

    let conn = match pool.get() {
        Ok(c) => c,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to get DB connection: {}", e),
            )
                .into_response()
        }
    };

    match astro_probe_core::query::query_lineage_internal(&conn, &params.node, &params.direction) {
        Ok(resp) => Json(resp).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Lineage query failed: {}", e),
        )
            .into_response(),
    }
}

#[derive(Debug, Deserialize)]
pub struct RoutesQueryParams {
    pub path: Option<String>,
    pub http_method: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct WebRoute {
    pub http_method: String,
    pub path: String,
    pub controller_method_fqn: String,
}

#[derive(Debug, Serialize)]
pub struct RoutesResponse {
    pub routes: Vec<WebRoute>,
}

pub async fn query_routes(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(params): Query<RoutesQueryParams>,
) -> impl IntoResponse {
    let pool = match state.manager.get_db_pool_and_touch(&id) {
        Some(p) => p,
        None => return (StatusCode::NOT_FOUND, "Workspace not found").into_response(),
    };

    let conn = match pool.get() {
        Ok(c) => c,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to get DB connection: {}", e),
            )
                .into_response()
        }
    };

    let mut sql = "SELECT http_method, path, controller_method_fqn FROM web_routes WHERE 1=1".to_string();
    let mut args: Vec<String> = Vec::new();

    if let Some(ref path) = params.path {
        sql.push_str(" AND path = ?");
        args.push(path.clone());
    }
    if let Some(ref method) = params.http_method {
        sql.push_str(" AND http_method = ?");
        args.push(method.clone());
    }

    let mut stmt = match conn.prepare(&sql) {
        Ok(s) => s,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to prepare SQL: {}", e),
            )
                .into_response()
        }
    };

    let params_ref: Vec<&dyn rusqlite::ToSql> = args.iter().map(|s| s as &dyn rusqlite::ToSql).collect();

    let routes_iter = match stmt.query_map(&*params_ref, |row| {
        let http_method: String = row.get(0)?;
        let path: String = row.get(1)?;
        let controller_method_fqn: String = row.get(2)?;
        Ok(WebRoute {
            http_method,
            path,
            controller_method_fqn,
        })
    }) {
        Ok(it) => it,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to execute SQL: {}", e),
            )
                .into_response()
        }
    };

    let mut routes = Vec::new();
    for r in routes_iter.flatten() {
        routes.push(r);
    }

    Json(RoutesResponse { routes }).into_response()
}
