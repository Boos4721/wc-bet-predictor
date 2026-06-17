use http_body_util::BodyExt;
use tower::ServiceExt;
use axum::http::{Request, StatusCode};
use axum::body::Body;
use std::sync::{Arc, Mutex};
use wc_bet_predictor::{api::{self, AppState}, ledger::Store};

fn app() -> axum::Router {
    let state = AppState {
        store: Arc::new(Store::open(":memory:").unwrap()),
        cfg: Arc::new(Mutex::new(None)),
        cfg_path: std::env::temp_dir().join("wcbp-test-cfg.json").to_string_lossy().into(),
        poly: std::sync::Arc::new(wc_bet_predictor::polymarket::PolyCache::new("/tmp/wcbp-test-poly.json")),
    };
    api::router(state)
}

async fn body_json(resp: axum::response::Response) -> serde_json::Value {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn full_ledger_flow() {
    let app = app();

    // 1. 解析赛事
    let resp = app.clone().oneshot(Request::post("/api/matches")
        .header("content-type","application/json")
        .body(Body::from(r#"{"raw":"周日001|世界杯|A|B|2026-06-20T19:00:00|2.1|3.2|3.5"}"#)).unwrap())
        .await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let ms = body_json(resp).await;
    assert_eq!(ms[0]["home"], "A");

    // 2. 下注
    let resp = app.clone().oneshot(Request::post("/api/bets")
        .header("content-type","application/json")
        .body(Body::from(r#"{"match_id":"周日001","pick":"Home","stake":100.0,"odds":2.1}"#)).unwrap())
        .await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bet_id = body_json(resp).await["id"].as_i64().unwrap();

    // 3. 结算(命中)
    let resp = app.clone().oneshot(Request::post("/api/settle")
        .header("content-type","application/json")
        .body(Body::from(format!(r#"{{"bet_id":{bet_id},"actual_result":"Home"}}"#))).unwrap())
        .await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let st = body_json(resp).await;
    assert!((st["pnl"].as_f64().unwrap() - 110.0).abs() < 1e-9);

    // 4. 统计
    let resp = app.clone().oneshot(Request::get("/api/stats").body(Body::empty()).unwrap())
        .await.unwrap();
    let stats = body_json(resp).await;
    assert_eq!(stats["settled"], 1);
    assert!((stats["total_pnl"].as_f64().unwrap() - 110.0).abs() < 1e-9);
}

#[tokio::test]
async fn predict_without_config_is_400() {
    let app = app();
    let resp = app.oneshot(Request::post("/api/predict")
        .header("content-type","application/json")
        .body(Body::from(r#"{"id":"m","league":"l","home":"A","away":"B",
            "kickoff":"t","odds":{"home":2.0,"draw":3.0,"away":3.0},"handicap":null}"#)).unwrap())
        .await.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}
