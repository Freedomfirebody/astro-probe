use std::sync::Arc;
use clap::Parser;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};
use astro_probe::kernel::WorkspaceManager;
use astro_probe::api::{create_router, AppState};
use astro_probe::mcp::McpServer;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(short, long, default_value = "both")]
    mode: String, // "http", "mcp", "both"

    #[arg(short, long)]
    port: Option<u16>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Force logs to stderr so stdout remains clean for MCP JSON-RPC
    tracing_subscriber::registry()
        .with(fmt::layer().with_writer(std::io::stderr))
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .init();

    let args = Args::parse();

    let port = args.port
        .or_else(|| std::env::var("ASTRO_PROBE_PORT").ok().and_then(|v| v.parse().ok()))
        .or_else(|| std::env::var("PORT").ok().and_then(|v| v.parse().ok()))
        .unwrap_or(8080);

    let manager = Arc::new(WorkspaceManager::new());

    match args.mode.as_str() {
        "http" => {
            run_http_server(manager, port).await?;
        }
        "mcp" => {
            run_mcp_server(manager).await?;
        }
        "both" => {
            let http_manager = manager.clone();
            let http_handle = tokio::spawn(async move {
                if let Err(e) = run_http_server(http_manager, port).await {
                    eprintln!("HTTP server error: {}", e);
                }
            });

            let mcp_handle = tokio::spawn(async move {
                if let Err(e) = run_mcp_server(manager).await {
                    eprintln!("MCP server error: {}", e);
                }
            });

            tokio::select! {
                res = http_handle => {
                    if let Err(e) = res {
                        eprintln!("HTTP server task panicked: {}", e);
                    }
                }
                res = mcp_handle => {
                    if let Err(e) = res {
                        eprintln!("MCP server task panicked: {}", e);
                    }
                }
            }
        }
        _ => {
            anyhow::bail!("Invalid mode: {}", args.mode);
        }
    }

    Ok(())
}

async fn run_http_server(manager: Arc<WorkspaceManager>, port: u16) -> anyhow::Result<()> {
    let state = AppState { manager };
    let router = create_router(state);
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("HTTP server listening on {}", addr);
    axum::serve(listener, router).await?;
    Ok(())
}

async fn run_mcp_server(manager: Arc<WorkspaceManager>) -> anyhow::Result<()> {
    let server = McpServer::new(manager);
    server.run().await?;
    Ok(())
}
