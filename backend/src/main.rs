mod domain;
mod source;
mod polymarket;
mod sporttery;
mod cache;
mod ledger;
mod predictor;
mod config;
mod api;
mod static_assets;

use api::AppState;
use ledger::Store;
use std::sync::{Arc, Mutex};
use tower_http::cors::CorsLayer;

#[tokio::main]
async fn main() {
    let store = Store::open("ledger.db").expect("open db");
    let cfg_path = config::default_path();
    let cfg = config::load(&cfg_path).unwrap_or(None);

    let poly = std::sync::Arc::new(cache::MatchCache::new("poly_cache.json"));
    poly.load_disk();
    let sporttery = std::sync::Arc::new(cache::MatchCache::new("sporttery_cache.json"));
    sporttery.load_disk();
    {
        let p = poly.clone();
        tokio::spawn(async move {
            loop {
                match polymarket::fetch_and_map().await {
                    Ok(ms) => p.store(ms),
                    Err(e) => eprintln!("polymarket refresh failed: {e}"),
                }
                tokio::time::sleep(std::time::Duration::from_secs(15 * 60)).await;
            }
        });
    }
    {
        let s = sporttery.clone();
        tokio::spawn(async move {
            loop {
                match sporttery::fetch_and_map().await {
                    Ok(ms) => s.store(ms),
                    Err(e) => eprintln!("sporttery refresh failed: {e}"),
                }
                tokio::time::sleep(std::time::Duration::from_secs(15 * 60)).await;
            }
        });
    }

    let state = AppState {
        store: Arc::new(store),
        cfg: Arc::new(Mutex::new(cfg)),
        cfg_path,
        poly,
        sporttery,
    };

    let app = api::router(state)
        .fallback(static_assets::static_handler)
        .layer(CorsLayer::permissive());

    let listener = tokio::net::TcpListener::bind("127.0.0.1:8787").await.unwrap();
    println!("listening on http://127.0.0.1:8787");
    axum::serve(listener, app).await.unwrap();
}
