mod domain;
mod source;
mod ledger;
mod predictor;
mod config;
mod api;

use api::AppState;
use ledger::Store;
use std::sync::{Arc, Mutex};
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;

#[tokio::main]
async fn main() {
    let store = Store::open("ledger.db").expect("open db");
    let cfg_path = config::default_path();
    let cfg = config::load(&cfg_path).unwrap_or(None);

    let state = AppState {
        store: Arc::new(store),
        cfg: Arc::new(Mutex::new(cfg)),
        cfg_path,
    };

    let app = api::router(state)
        .nest_service("/", ServeDir::new("../frontend/dist"))
        .layer(CorsLayer::permissive());

    let listener = tokio::net::TcpListener::bind("127.0.0.1:8787").await.unwrap();
    println!("listening on http://127.0.0.1:8787");
    axum::serve(listener, app).await.unwrap();
}
