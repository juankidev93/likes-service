use axum::{routing::get, Json, Router};
use serde_json::json;
use std::net::SocketAddr;

#[tokio::main]
async fn main() {
    let app = Router::new().route("/health/live", get(live_health));
    let address = SocketAddr::from(([127, 0, 0, 1], 3000));

    let listener = tokio::net::TcpListener::bind(address)
        .await
        .expect("failed to bind TCP listener");

    axum::serve(listener, app)
        .await
        .expect("HTTP server failed");
}

async fn live_health() -> Json<serde_json::Value> {
    Json(json!({ "status": "ok" }))
}
