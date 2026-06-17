use crate::config;
use crate::domain::Outcome;
use crate::ledger::Store;
use crate::predictor::{self, ApiConfig};
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
    pub poly: std::sync::Arc<crate::cache::MatchCache>,
    pub sporttery: std::sync::Arc<crate::cache::MatchCache>,
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/matches", post(parse_matches))
        .route("/api/matches/polymarket", get(polymarket_matches))
        .route("/api/matches/polymarket/dates", get(polymarket_dates))
        .route("/api/matches/polymarket/status", get(polymarket_status))
        .route("/api/matches/sporttery", get(sporttery_matches))
        .route("/api/matches/sporttery/dates", get(sporttery_dates))
        .route("/api/matches/sporttery/status", get(sporttery_status))
        .route("/api/predict", post(predict))
        .route("/api/chat", post(chat))
        .route("/api/bets", get(list_bets).post(place_bet))
        .route("/api/settle", post(settle))
        .route("/api/tickets", get(list_tickets).post(place_ticket))
        .route("/api/tickets/settle", post(settle_ticket))
        .route("/api/ledger/clear", post(clear_ledger))
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

#[derive(Deserialize)]
struct PolyQuery { date: Option<String>, limit: Option<usize> }

async fn polymarket_matches(State(s): State<AppState>, Query(q): Query<PolyQuery>) -> impl IntoResponse {
    let limit = q.limit.unwrap_or(40).min(200);
    let mut ms = s.poly.snapshot();
    if let Some(d) = q.date.as_deref() { ms.retain(|m| m.kickoff == d); }
    ms.truncate(limit);
    (StatusCode::OK, Json(json!(ms))).into_response()
}

async fn polymarket_dates(State(s): State<AppState>) -> impl IntoResponse {
    (StatusCode::OK, Json(json!(crate::cache::available_dates(&s.poly.snapshot())))).into_response()
}

async fn polymarket_status(State(s): State<AppState>) -> impl IntoResponse {
    (StatusCode::OK, Json(json!({ "updated_at": s.poly.updated(), "count": s.poly.len() }))).into_response()
}

async fn sporttery_matches(State(s): State<AppState>, Query(q): Query<PolyQuery>) -> impl IntoResponse {
    let limit = q.limit.unwrap_or(40).min(200);
    let mut ms = s.sporttery.snapshot();
    if let Some(d) = q.date.as_deref() { ms.retain(|m| m.kickoff == d); }
    ms.truncate(limit);
    (StatusCode::OK, Json(json!(ms))).into_response()
}

async fn sporttery_dates(State(s): State<AppState>) -> impl IntoResponse {
    (StatusCode::OK, Json(json!(crate::cache::available_dates(&s.sporttery.snapshot())))).into_response()
}

async fn sporttery_status(State(s): State<AppState>) -> impl IntoResponse {
    (StatusCode::OK, Json(json!({ "updated_at": s.sporttery.updated(), "count": s.sporttery.len() }))).into_response()
}

#[derive(Deserialize)]
struct PredictIn {
    #[serde(rename = "match")]
    m: crate::domain::Match,
    #[serde(default)]
    play: crate::predictor::Play,
    #[serde(default)]
    cfg: Option<ApiConfig>,
}

async fn predict(State(s): State<AppState>, Json(b): Json<PredictIn>) -> impl IntoResponse {
    // 优先用请求内联的配置(浏览器存储),回退到服务端内存配置。
    let cfg = b.cfg.or_else(|| s.cfg.lock().unwrap().clone());
    let Some(cfg) = cfg else {
        return err(StatusCode::BAD_REQUEST, "未配置 AI,请先在配置区填写").into_response();
    };
    match predictor::predict_play(&cfg, &b.m, b.play).await {
        Ok(p) => (StatusCode::OK, Json(json!(p))).into_response(),
        Err(e) => err(StatusCode::BAD_GATEWAY, e.to_string()).into_response(),
    }
}

#[derive(Deserialize)]
struct ChatIn {
    #[serde(default)] messages: Vec<crate::predictor::ChatMsg>,
    #[serde(default)] matches: Vec<crate::domain::Match>,
    #[serde(default)] cfg: Option<ApiConfig>,
}

async fn chat(State(s): State<AppState>, Json(b): Json<ChatIn>) -> impl IntoResponse {
    let cfg = b.cfg.or_else(|| s.cfg.lock().unwrap().clone());
    let Some(cfg) = cfg else {
        return err(StatusCode::BAD_REQUEST, "未配置 AI,请先在配置区填写").into_response();
    };
    if b.messages.is_empty() {
        return err(StatusCode::BAD_REQUEST, "消息为空").into_response();
    }
    let ctx = crate::predictor::matches_context(&b.matches);
    let system = format!(
        "你是足球竞彩分析助手,用简洁中文回答。可基于赔率给出分析与建议,但要提醒用户理性、量力而行,不承诺胜率。\n{ctx}");
    match crate::predictor::chat(&cfg, &system, &b.messages).await {
        Ok(reply) => (StatusCode::OK, Json(json!({"reply": reply}))).into_response(),
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

#[derive(Deserialize)]
struct TicketIn { legs: serde_json::Value, ways: serde_json::Value, multiplier: i64, bet_count: i64, stake: f64, max_return: f64 }

async fn place_ticket(State(s): State<AppState>, Json(b): Json<TicketIn>) -> impl IntoResponse {
    let now = predictor::now_iso();
    let legs = b.legs.to_string();
    let ways = b.ways.to_string();
    match s.store.insert_ticket(&legs, &ways, b.multiplier, b.bet_count, b.stake, b.max_return, &now) {
        Ok(id) => (StatusCode::OK, Json(json!({"id": id}))).into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

async fn list_tickets(State(s): State<AppState>) -> impl IntoResponse {
    match s.store.list_tickets() {
        Ok(t) => (StatusCode::OK, Json(json!(t))).into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

#[derive(Deserialize)]
struct SettleTicketIn { id: i64, payout: f64 }

async fn settle_ticket(State(s): State<AppState>, Json(b): Json<SettleTicketIn>) -> impl IntoResponse {
    let now = predictor::now_iso();
    match s.store.settle_ticket(b.id, b.payout, &now) {
        Ok(()) => (StatusCode::OK, Json(json!({"ok": true}))).into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

async fn clear_ledger(State(s): State<AppState>) -> impl IntoResponse {
    match s.store.clear_all() {
        Ok(()) => (StatusCode::OK, Json(json!({"ok": true}))).into_response(),
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
