use crate::api::handlers::{
    create_workspace, delete_workspace, list_workspaces, query_call_graph, query_lineage,
    query_routes, start_workspace, stop_workspace, AppState,
};
use axum::{
    routing::{delete, get, post},
    Router,
};

pub fn create_router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(|| async { "OK" }))
        .route(
            "/api/workspaces",
            post(create_workspace).get(list_workspaces),
        )
        .route("/api/workspaces/:id", delete(delete_workspace))
        .route("/api/workspaces/:id/start", post(start_workspace))
        .route("/api/workspaces/:id/stop", post(stop_workspace))
        .route("/api/workspaces/:id/call-graph", get(query_call_graph))
        .route("/api/workspaces/:id/lineage", get(query_lineage))
        .route("/api/workspaces/:id/routes", get(query_routes))
        .with_state(state)
}
