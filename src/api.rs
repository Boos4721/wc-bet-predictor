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

    // 系统提示:充实人设 + 注入赛事上下文 + 账本上下文。
    let stats = s.store.stats().unwrap_or(crate::ledger::Stats {
        settled: 0, hit_rate: 0.0, total_pnl: 0.0, roi: 0.0 });
    let bets = s.store.list_bets(None).unwrap_or_default();
    let ledger_ctx = crate::predictor::ledger_context(&stats, &bets);
    let matches_ctx = crate::predictor::matches_context(&b.matches);
    let system = format!("{PERSONA}\n{matches_ctx}\n{ledger_ctx}");

    match run_agent(&s, &cfg, &system, &b.messages).await {
        Ok((reply, steps)) => (StatusCode::OK, Json(json!({"reply": reply, "steps": steps}))).into_response(),
        Err(e) => err(StatusCode::BAD_GATEWAY, e.to_string()).into_response(),
    }
}

const PERSONA: &str = "你是「世界鸡预测」的足球竞彩分析助手。\n\
职责:基于赔率、隐含概率与已加载赛事,为用户分析赛事、解释玩法、给出有依据的倾向性建议。\n\
可用玩法:胜平负、让球胜平负、比分、总进球数、半全场、半场。\n\
风格:简洁中文,先结论后理由,涉及金额时务必提醒「均为虚拟、仅供复盘,请理性、量力而行」,绝不承诺胜率或保证盈利。\n\
当你需要数据(某天的赛事、某场的预测、账本战绩)时,调用提供的工具获取,不要编造赔率或比分。";

const MAX_STEPS: usize = 6;

/// 选择数据源缓存(默认 sporttery)。
fn pick_cache(s: &AppState, source: &str) -> std::sync::Arc<crate::cache::MatchCache> {
    if source == "polymarket" { s.poly.clone() } else { s.sporttery.clone() }
}

/// 执行一次工具调用,返回 (结果字符串, 步骤摘要)。
async fn exec_tool(s: &AppState, cfg: &ApiConfig, name: &str, args: &serde_json::Value) -> (String, String) {
    let source = args["source"].as_str().unwrap_or("sporttery");
    match name {
        "list_dates" => {
            let snap = pick_cache(s, source).snapshot();
            let dates = crate::cache::available_dates(&snap);
            let summary = format!("list_dates {source} → {} 个比赛日", dates.len());
            (json!(dates).to_string(), summary)
        }
        "get_matches" => {
            let mut snap = pick_cache(s, source).snapshot();
            if let Some(d) = args["date"].as_str() { snap.retain(|m| m.kickoff == d); }
            snap.truncate(40);
            let mini: Vec<serde_json::Value> = snap.iter().map(|m| json!({
                "id": m.id, "home": m.home, "away": m.away, "kickoff": m.kickoff,
                "odds": {"home": m.odds.home, "draw": m.odds.draw, "away": m.odds.away}
            })).collect();
            let summary = format!("get_matches {source} → {} 场", mini.len());
            (json!(mini).to_string(), summary)
        }
        "predict" => {
            let match_id = args["match_id"].as_str().unwrap_or_default();
            let play = crate::predictor::play_from_str(args["play"].as_str().unwrap_or("HAD"));
            let snap = pick_cache(s, source).snapshot();
            let Some(m) = snap.iter().find(|m| m.id == match_id) else {
                return ("未找到该赛事".into(), format!("predict {match_id} → 未找到"));
            };
            match crate::predictor::predict_play(cfg, m, play).await {
                Ok(p) => {
                    let summary = format!("predict {} → {} {:.0}%",
                        match_id, p.pick_label, (p.confidence as f64) * 100.0);
                    (json!(p).to_string(), summary)
                }
                Err(e) => (e.to_string(), format!("predict {match_id} → 失败")),
            }
        }
        "get_stats" => {
            let st = s.store.stats().unwrap_or(crate::ledger::Stats {
                settled: 0, hit_rate: 0.0, total_pnl: 0.0, roi: 0.0 });
            (json!(st).to_string(), "get_stats → 战绩".into())
        }
        "list_bets" => {
            let mut bets = s.store.list_bets(None).unwrap_or_default();
            bets.truncate(20);
            (json!(bets).to_string(), format!("list_bets → {} 笔", bets.len()))
        }
        other => (format!("未知工具:{other}"), format!("{other} → 未知")),
    }
}

/// 只读工具 agentic 循环。返回最终回复文本与步骤摘要。
async fn run_agent(s: &AppState, cfg: &ApiConfig, system: &str, msgs: &[crate::predictor::ChatMsg])
    -> Result<(String, Vec<serde_json::Value>), crate::predictor::PredictError>
{
    use crate::predictor as p;
    let mut convo = p::seed_messages(msgs);
    let mut steps: Vec<serde_json::Value> = Vec::new();

    for _ in 0..MAX_STEPS {
        let body = p::build_agent_body(cfg, system, &convo);
        let resp = p::post_ai(cfg, &body).await?;
        let calls = p::parse_tool_calls(cfg.protocol, &resp);
        if calls.is_empty() {
            return Ok((p::final_text(cfg.protocol, &resp), steps));
        }
        // 回填助手轮(含 tool_use/tool_calls)。
        push(&mut convo, p::assistant_turn(cfg.protocol, &resp));
        for c in &calls {
            let (result, summary) = exec_tool(s, cfg, &c.name, &c.args).await;
            steps.push(json!({"tool": c.name, "summary": summary}));
            push(&mut convo, p::tool_result_turn(cfg.protocol, &c.id, &result));
        }
    }

    // 步数耗尽:不带工具再问一次,强制出文本。
    let body = p::build_agent_body_no_tools(cfg, system, &convo);
    let resp = p::post_ai(cfg, &body).await?;
    Ok((p::final_text(cfg.protocol, &resp), steps))
}

fn push(convo: &mut serde_json::Value, turn: serde_json::Value) {
    if let Some(arr) = convo.as_array_mut() { arr.push(turn); }
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
