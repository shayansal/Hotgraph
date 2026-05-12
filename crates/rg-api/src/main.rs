use std::net::SocketAddr;

use rg_api::{serve_with_graceful_shutdown, ApiState};
use tracing_subscriber::{fmt, EnvFilter};

#[tokio::main]
async fn main() {
    fmt()
        .json()
        .with_env_filter(EnvFilter::from_default_env())
        .init();
    let addr = std::env::var("RG_API_ADDR")
        .ok()
        .and_then(|value| value.parse::<SocketAddr>().ok())
        .unwrap_or_else(|| SocketAddr::from(([127, 0, 0, 1], 8080)));
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("bind API listener");
    let state = ApiState::from_env().expect("load API state from environment");

    serve_with_graceful_shutdown(listener, state, shutdown_signal())
        .await
        .expect("serve API");
}

async fn shutdown_signal() {
    if tokio::signal::ctrl_c().await.is_ok() {
        tracing::info!("graceful_shutdown_requested");
    }
}
