use crate::domain::{Match, Outcome, Prediction, Probs};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ApiProtocol { Anthropic, OpenAI }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiConfig {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub protocol: ApiProtocol,
}

#[derive(Debug, thiserror::Error)]
pub enum PredictError {
    #[error("HTTP 错误:{0}")]
    Http(String),
    #[error("响应无法解析:{0}")]
    Parse(String),
    #[error("预测不合法:{0}")]
    Invalid(String),
}

const SYSTEM_PROMPT: &str = "你是足球竞彩分析助手。只输出严格 JSON,不要任何解释或代码块标记。\
格式:{\"probs\":{\"home\":数,\"draw\":数,\"away\":数},\"pick\":\"Home|Draw|Away\",\
\"confidence\":0到1的数,\"rationale\":\"中文理由\"}。probs 三者之和必须约等于 1。";

fn user_prompt(m: &Match) -> String {
    format!("赛事:{} {} vs {}(开赛 {})。胜平负赔率 主 {} / 平 {} / 客 {}{}。\
        请给出胜平负概率、推荐与中文理由。",
        m.league, m.home, m.away, m.kickoff,
        m.odds.home, m.odds.draw, m.odds.away,
        m.handicap.map(|h| format!(",让球 {h}")).unwrap_or_default())
}

pub fn build_body(cfg: &ApiConfig, m: &Match) -> Value {
    let user = user_prompt(m);
    match cfg.protocol {
        ApiProtocol::Anthropic => json!({
            "model": cfg.model, "max_tokens": 1024, "system": SYSTEM_PROMPT,
            "messages": [{"role":"user","content": user}]
        }),
        ApiProtocol::OpenAI => json!({
            "model": cfg.model,
            "messages": [
                {"role":"system","content": SYSTEM_PROMPT},
                {"role":"user","content": user}
            ]
        }),
    }
}

pub fn extract_text(p: ApiProtocol, resp: &Value) -> Result<String, PredictError> {
    let t = match p {
        ApiProtocol::Anthropic => resp["content"][0]["text"].as_str(),
        ApiProtocol::OpenAI => resp["choices"][0]["message"]["content"].as_str(),
    };
    t.map(|s| s.to_string())
        .ok_or_else(|| PredictError::Parse("响应缺少文本字段".into()))
}

#[derive(Deserialize)]
struct RawPred { probs: Probs, pick: Outcome, confidence: f32, rationale: String }

pub fn parse_prediction(match_id: &str, model: &str, text: &str) -> Result<Prediction, PredictError> {
    let cleaned = text.trim()
        .trim_start_matches("```json").trim_start_matches("```")
        .trim_end_matches("```").trim();
    let raw: RawPred = serde_json::from_str(cleaned)
        .map_err(|e| PredictError::Parse(e.to_string()))?;
    let sum = raw.probs.home + raw.probs.draw + raw.probs.away;
    if (sum - 1.0).abs() > 0.02 {
        return Err(PredictError::Invalid(format!("概率和={sum:.3} 偏离 1")));
    }
    Ok(Prediction {
        match_id: match_id.into(),
        probs: raw.probs, pick: raw.pick,
        confidence: raw.confidence, rationale: raw.rationale,
        model: model.into(),
        created_at: now_iso(),
    })
}

pub fn now_iso() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
    format!("@{secs}") // 简化:Unix 秒,前缀标记;前端只作展示
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{Match, Odds, Outcome};

    fn sample() -> Match {
        Match { id: "周日001".into(), league: "世界杯".into(),
            home: "A".into(), away: "B".into(), kickoff: "2026-06-20T19:00:00".into(),
            odds: Odds { home: 2.1, draw: 3.2, away: 3.5 }, handicap: None }
    }

    #[test]
    fn anthropic_body_shape() {
        let cfg = ApiConfig { base_url: "https://x".into(), api_key: "k".into(),
            model: "claude-fable-5".into(), protocol: ApiProtocol::Anthropic };
        let b = build_body(&cfg, &sample());
        assert_eq!(b["model"], "claude-fable-5");
        assert!(b["max_tokens"].is_number());
        assert_eq!(b["messages"][0]["role"], "user");
        assert!(b["system"].is_string());
    }

    #[test]
    fn openai_body_shape() {
        let cfg = ApiConfig { base_url: "https://x".into(), api_key: "k".into(),
            model: "gpt-4o".into(), protocol: ApiProtocol::OpenAI };
        let b = build_body(&cfg, &sample());
        assert_eq!(b["model"], "gpt-4o");
        assert_eq!(b["messages"][0]["role"], "system");
        assert_eq!(b["messages"][1]["role"], "user");
    }

    #[test]
    fn extract_anthropic_text() {
        let resp = serde_json::json!({"content":[{"type":"text","text":"hello"}]});
        assert_eq!(extract_text(ApiProtocol::Anthropic, &resp).unwrap(), "hello");
    }

    #[test]
    fn extract_openai_text() {
        let resp = serde_json::json!({"choices":[{"message":{"content":"hi"}}]});
        assert_eq!(extract_text(ApiProtocol::OpenAI, &resp).unwrap(), "hi");
    }

    #[test]
    fn parse_valid_prediction() {
        let txt = r#"{"probs":{"home":0.5,"draw":0.3,"away":0.2},
            "pick":"Home","confidence":0.7,"rationale":"主队状态好"}"#;
        let p = parse_prediction("周日001", "m", txt).unwrap();
        assert_eq!(p.pick, Outcome::Home);
        assert!((p.probs.home - 0.5).abs() < 1e-9);
    }

    #[test]
    fn parse_prediction_strips_codefence() {
        let txt = "```json\n{\"probs\":{\"home\":0.5,\"draw\":0.3,\"away\":0.2},\"pick\":\"Home\",\"confidence\":0.7,\"rationale\":\"x\"}\n```";
        assert!(parse_prediction("m", "m", txt).is_ok());
    }

    #[test]
    fn parse_rejects_bad_prob_sum() {
        let txt = r#"{"probs":{"home":0.9,"draw":0.9,"away":0.9},
            "pick":"Home","confidence":0.7,"rationale":"x"}"#;
        assert!(parse_prediction("m", "m", txt).is_err());
    }
}
