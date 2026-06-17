use crate::config;
use crate::domain::{Match, Outcome};
use crate::ledger::Store;
use crate::predictor::{self, ApiConfig};
use crate::polymarket;
use crate::source::{MatchSource, PasteSource};
use axum::{extract::{Query, State}, http::StatusCode, response::IntoResponse, routing::{get, post}, Json, Router};
use serde::Deserialize;
use serde_json::json;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub struct AppState {
    pub store: Arc<Store>,
    pub cfg: Arc<Mutex<Option<ApiConfig>>>,
    pub cfg_path: String,
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/matches", post(parse_matches))
        .route("/api/matches/polymarket", get(polymarket_matches))
        .route("/api/predict", post(predict))
        .route("/api/bets", get(list_bets).post(place_bet))
        .route("/api/settle", post(settle))
        .route("/api/stats", get(stats))
        .route("/api/config", get(get_config).post(set_config))
        .with_state(state)
}

fn err(code: StatusCode, msg: impl Into<String>) -> (StatusCode, Json<serde_json::Value>) {
    (code, Json(json!({"error": msg.into()})))
}

#[derive(Deserialize)]
struct RawIn { raw: String }

async fn parse_matches(Json(b): Json<RawIn>) -> impl IntoResponse {
    match (PasteSource { raw: b.raw }).load() {
        Ok(ms) => (StatusCode::OK, Json(json!(ms))).into_response(),
        Err(e) => err(StatusCode::BAD_REQUEST, e.to_string()).into_response(),
    }
}

async fn polymarket_matches() -> impl IntoResponse {
    match polymarket::fetch_matches().await {
        Ok(ms) => (StatusCode::OK, Json(json!(ms))).into_response(),
        Err(e) => err(StatusCode::BAD_GATEWAY, e).into_response(),
    }
}

async fn predict(State(s): State<AppState>, Json(m): Json<Match>) -> impl IntoResponse {
    let cfg = { s.cfg.lock().unwrap().clone() };
    let Some(cfg) = cfg else {
        return err(StatusCode::BAD_REQUEST, "未配置 AI,请先在配置区填写").into_response();
    };
    match predictor::predict(&cfg, &m).await {
        Ok(p) => (StatusCode::OK, Json(json!(p))).into_response(),
        Err(e) => err(StatusCode::BAD_GATEWAY, e.to_string()).into_response(),
    }
}

#[derive(Deserialize)]
struct BetIn { match_id: String, pick: Outcome, stake: f64, odds: f64 }

async fn place_bet(State(s): State<AppState>, Json(b): Json<BetIn>) -> impl IntoResponse {
    let now = predictor::now_iso();
    match s.store.insert_bet(&b.match_id, b.pick, b.stake, b.odds, &now) {
        Ok(id) => (StatusCode::OK, Json(json!({"id": id}))).into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

#[derive(Deserialize)]
struct BetsQuery { status: Option<String> }

async fn list_bets(State(s): State<AppState>, Query(q): Query<BetsQuery>) -> impl IntoResponse {
    use crate::domain::BetStatus;
    let filter = match q.status.as_deref() {
        Some("Pending") => Some(BetStatus::Pending),
        Some("Won") => Some(BetStatus::Won),
        Some("Lost") => Some(BetStatus::Lost),
        _ => None,
    };
    match s.store.list_bets(filter) {
        Ok(b) => (StatusCode::OK, Json(json!(b))).into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

#[derive(Deserialize)]
struct SettleIn { bet_id: i64, actual_result: Outcome }

async fn settle(State(s): State<AppState>, Json(b): Json<SettleIn>) -> impl IntoResponse {
    let now = predictor::now_iso();
    match s.store.settle(b.bet_id, b.actual_result, &now) {
        Ok(st) => (StatusCode::OK, Json(json!(st))).into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

async fn stats(State(s): State<AppState>) -> impl IntoResponse {
    match s.store.stats() {
        Ok(st) => (StatusCode::OK, Json(json!(st))).into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

async fn get_config(State(s): State<AppState>) -> impl IntoResponse {
    let cfg = s.cfg.lock().unwrap().clone();
    match cfg {
        Some(c) => (StatusCode::OK, Json(json!({
            "base_url": c.base_url, "model": c.model,
            "protocol": c.protocol, "has_key": !c.api_key.is_empty()
        }))).into_response(),
        None => (StatusCode::OK, Json(json!({"has_key": false}))).into_response(),
    }
}

async fn set_config(State(s): State<AppState>, Json(c): Json<ApiConfig>) -> impl IntoResponse {
    if let Err(e) = config::save(&s.cfg_path, &c) {
        return err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response();
    }
    *s.cfg.lock().unwrap() = Some(c);
    (StatusCode::OK, Json(json!({"ok": true}))).into_response()
}
