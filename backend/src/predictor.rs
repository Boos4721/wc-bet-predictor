use crate::domain::{Match, Outcome};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ApiProtocol { Anthropic, OpenAI }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Play { HAD, HHAD, CRS, TTG, HAFU }

impl Default for Play {
    fn default() -> Self { Play::HAD }
}

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

pub fn extract_text(p: ApiProtocol, resp: &Value) -> Result<String, PredictError> {
    let t = match p {
        ApiProtocol::Anthropic => resp["content"][0]["text"].as_str(),
        ApiProtocol::OpenAI => resp["choices"][0]["message"]["content"].as_str(),
    };
    t.map(|s| s.to_string())
        .ok_or_else(|| PredictError::Parse("响应缺少文本字段".into()))
}

pub fn now_iso() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
    format!("@{secs}") // 简化:Unix 秒,前缀标记;前端只作展示
}

// ===== 玩法-aware 预测(HAD/HHAD/CRS/TTG/HAFU,统一 schema) =====

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PredOption { pub label: String, pub prob: f64 }

#[derive(Debug, Clone, Serialize)]
pub struct PlayPrediction {
    pub play: Play,
    pub match_id: String,
    pub options: Vec<PredOption>,
    pub pick: Option<Outcome>,
    pub pick_label: String,
    pub confidence: f32,
    pub rationale: String,
    pub model: String,
    pub created_at: String,
    pub pick_odds: Option<f64>,
}

const PLAY_BASE_PROMPT: &str = "你是足球竞彩分析助手。只输出严格 JSON,不要任何解释或代码块。\
格式:{\"options\":[{\"label\":\"中文结果\",\"prob\":0到1}],\"pick\":\"Home|Draw|Away 或 null\",\
\"pick_label\":\"推荐结果中文\",\"confidence\":0到1,\"rationale\":\"中文理由\"}。";

fn play_outcome_rule(play: Play, line: Option<i32>) -> String {
    match play {
        Play::HAD => "玩法=胜平负。options 恰好三项,label 用 主胜/平局/客胜,prob 三者之和≈1。pick 用 Home/Draw/Away 对应最高项。".to_string(),
        Play::HHAD => format!("玩法=让球胜平负,让球数={}(主队让/受球)。在该让球盘口下给出 主胜/平局/客胜 三项概率(和≈1),pick 用 Home/Draw/Away。",
            line.map(|l| l.to_string()).unwrap_or_else(|| "未知".into())),
        Play::CRS => "玩法=正确比分。给出最可能的 6~8 个比分(label 如 \"2:1\"),prob 为各自概率(不必和为1,取主要比分即可)。pick=null。pick_label=最可能比分。".to_string(),
        Play::TTG => "玩法=总进球数。options 覆盖 0,1,2,3,4,5,6,7+(label 如 \"0球\"…\"7+球\"),prob 和≈1。pick=null。pick_label=最可能档。".to_string(),
        Play::HAFU => "玩法=半全场(半场/全场结果组合)。options 为 9 种组合,label 用 胜/胜、胜/平、胜/负、平/胜、平/平、平/负、负/胜、负/平、负/负,prob 和≈1。pick=null。pick_label=最可能组合。".to_string(),
    }
}

fn play_system_prompt(play: Play, line: Option<i32>) -> String {
    format!("{PLAY_BASE_PROMPT}{}", play_outcome_rule(play, line))
}

fn play_user_prompt(m: &Match, play: Play) -> String {
    let mut s = format!("赛事:{} {} vs {}(开赛 {})。胜平负赔率 主 {} / 平 {} / 客 {}。",
        m.league, m.home, m.away, m.kickoff, m.odds.home, m.odds.draw, m.odds.away);
    if play == Play::HHAD {
        if let Some(line) = m.hhad_line {
            s.push_str(&format!("让球数 {line}。"));
        }
        if let Some(o) = &m.hhad_odds {
            s.push_str(&format!("让球赔率 主 {} / 平 {} / 客 {}。", o.home, o.draw, o.away));
        }
    }
    s.push_str("请按上述玩法给出概率、推荐与中文理由。");
    s
}

pub fn build_play_body(cfg: &ApiConfig, m: &Match, play: Play) -> Value {
    let system = play_system_prompt(play, m.hhad_line);
    let user = play_user_prompt(m, play);
    match cfg.protocol {
        ApiProtocol::Anthropic => json!({
            "model": cfg.model, "max_tokens": 1024, "system": system,
            "messages": [{"role":"user","content": user}]
        }),
        ApiProtocol::OpenAI => json!({
            "model": cfg.model,
            "messages": [
                {"role":"system","content": system},
                {"role":"user","content": user}
            ]
        }),
    }
}

#[derive(Deserialize)]
struct RawPlay {
    options: Vec<PredOption>,
    #[serde(default)]
    pick: Option<String>,
    #[serde(default)]
    pick_label: String,
    confidence: f32,
    rationale: String,
}

fn parse_outcome(s: &str) -> Option<Outcome> {
    match s {
        "Home" => Some(Outcome::Home),
        "Draw" => Some(Outcome::Draw),
        "Away" => Some(Outcome::Away),
        _ => None,
    }
}

/// 解析玩法预测(纯函数,无网络)。caller 负责设置 pick_odds。
pub fn parse_play_prediction(play: Play, match_id: &str, model: &str, text: &str)
    -> Result<PlayPrediction, PredictError>
{
    let cleaned = text.trim()
        .trim_start_matches("```json").trim_start_matches("```")
        .trim_end_matches("```").trim();
    let raw: RawPlay = serde_json::from_str(cleaned)
        .map_err(|e| PredictError::Parse(e.to_string()))?;

    if raw.options.is_empty() {
        return Err(PredictError::Invalid("options 为空".into()));
    }
    for o in &raw.options {
        if !(0.0..=1.5).contains(&o.prob) {
            return Err(PredictError::Invalid(format!("prob={} 越界", o.prob)));
        }
    }
    let pick = raw.pick.as_deref().and_then(parse_outcome);

    // HAD/HHAD 是封闭三选一:严格校验。
    if matches!(play, Play::HAD | Play::HHAD) {
        if raw.options.len() != 3 {
            return Err(PredictError::Invalid(format!("胜平负应有 3 项,实得 {}", raw.options.len())));
        }
        let sum: f64 = raw.options.iter().map(|o| o.prob).sum();
        if (sum - 1.0).abs() > 0.08 {
            return Err(PredictError::Invalid(format!("概率和={sum:.3} 偏离 1")));
        }
        if pick.is_none() {
            return Err(PredictError::Invalid("胜平负 pick 不可为空".into()));
        }
    }

    let pick_label = if raw.pick_label.is_empty() {
        // 退回:取概率最高项的 label
        raw.options.iter()
            .max_by(|a, b| a.prob.partial_cmp(&b.prob).unwrap_or(std::cmp::Ordering::Equal))
            .map(|o| o.label.clone()).unwrap_or_default()
    } else {
        raw.pick_label.clone()
    };

    Ok(PlayPrediction {
        play,
        match_id: match_id.into(),
        options: raw.options,
        pick,
        pick_label,
        confidence: raw.confidence,
        rationale: raw.rationale,
        model: model.into(),
        created_at: now_iso(),
        pick_odds: None,
    })
}

/// pick 对应的玩法赔率:HAD 取 odds,HHAD 取 hhad_odds,其余 None。
fn pick_odds_for(m: &Match, play: Play, pick: Option<Outcome>) -> Option<f64> {
    let o = pick?;
    match play {
        Play::HAD => Some(match o {
            Outcome::Home => m.odds.home,
            Outcome::Draw => m.odds.draw,
            Outcome::Away => m.odds.away,
        }),
        Play::HHAD => m.hhad_odds.as_ref().map(|h| match o {
            Outcome::Home => h.home,
            Outcome::Draw => h.draw,
            Outcome::Away => h.away,
        }),
        _ => None,
    }
}

async fn call_ai_play(cfg: &ApiConfig, m: &Match, play: Play) -> Result<PlayPrediction, PredictError> {
    let body = build_play_body(cfg, m, play);
    let client = reqwest::Client::new();
    let url = match cfg.protocol {
        ApiProtocol::Anthropic => format!("{}/v1/messages", cfg.base_url.trim_end_matches('/')),
        ApiProtocol::OpenAI => format!("{}/chat/completions", cfg.base_url.trim_end_matches('/')),
    };
    let mut req = client.post(&url).json(&body);
    req = match cfg.protocol {
        ApiProtocol::Anthropic => req
            .header("x-api-key", &cfg.api_key)
            .header("anthropic-version", "2023-06-01"),
        ApiProtocol::OpenAI => req
            .header("authorization", format!("Bearer {}", cfg.api_key)),
    };
    let resp = req.send().await.map_err(|e| PredictError::Http(e.to_string()))?;
    if !resp.status().is_success() {
        let code = resp.status();
        let txt = resp.text().await.unwrap_or_default();
        return Err(PredictError::Http(format!("{code}: {txt}")));
    }
    let v: Value = resp.json().await.map_err(|e| PredictError::Parse(e.to_string()))?;
    let text = extract_text(cfg.protocol, &v)?;
    let mut p = parse_play_prediction(play, &m.id, &cfg.model, &text)?;
    p.pick_odds = pick_odds_for(m, play, p.pick);
    Ok(p)
}

/// 玩法预测,失败重试一次。
pub async fn predict_play(cfg: &ApiConfig, m: &Match, play: Play) -> Result<PlayPrediction, PredictError> {
    match call_ai_play(cfg, m, play).await {
        Ok(p) => Ok(p),
        Err(_) => call_ai_play(cfg, m, play).await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{Match, Odds, Outcome};

    fn sample() -> Match {
        Match { id: "周日001".into(), league: "世界杯".into(),
            home: "A".into(), away: "B".into(), kickoff: "2026-06-20T19:00:00".into(),
            odds: Odds { home: 2.1, draw: 3.2, away: 3.5 }, handicap: None,
            hhad_odds: None, hhad_line: None }
    }

    #[test]
    fn anthropic_body_shape() {
        let cfg = ApiConfig { base_url: "https://x".into(), api_key: "k".into(),
            model: "claude-fable-5".into(), protocol: ApiProtocol::Anthropic };
        let b = build_play_body(&cfg, &sample(), Play::HAD);
        assert_eq!(b["model"], "claude-fable-5");
        assert!(b["max_tokens"].is_number());
        assert_eq!(b["messages"][0]["role"], "user");
        assert!(b["system"].is_string());
    }

    #[test]
    fn openai_body_shape() {
        let cfg = ApiConfig { base_url: "https://x".into(), api_key: "k".into(),
            model: "gpt-4o".into(), protocol: ApiProtocol::OpenAI };
        let b = build_play_body(&cfg, &sample(), Play::HAD);
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
    fn parse_play_strips_codefence() {
        let txt = "```json\n{\"options\":[{\"label\":\"主胜\",\"prob\":0.5},{\"label\":\"平局\",\"prob\":0.3},{\"label\":\"客胜\",\"prob\":0.2}],\"pick\":\"Home\",\"pick_label\":\"主胜\",\"confidence\":0.5,\"rationale\":\"x\"}\n```";
        assert!(parse_play_prediction(Play::HAD, "m", "m", txt).is_ok());
    }

    #[test]
    fn parse_play_had_three_options() {
        let txt = r#"{"options":[{"label":"主胜","prob":0.52},{"label":"平局","prob":0.27},
            {"label":"客胜","prob":0.21}],"pick":"Home","pick_label":"主胜",
            "confidence":0.52,"rationale":"主队状态好"}"#;
        let p = parse_play_prediction(Play::HAD, "m1", "mdl", txt).unwrap();
        assert_eq!(p.play, Play::HAD);
        assert_eq!(p.options.len(), 3);
        assert_eq!(p.pick, Some(Outcome::Home));
        assert_eq!(p.pick_label, "主胜");
        assert!(p.pick_odds.is_none()); // caller 填充,parse 不填
    }

    #[test]
    fn parse_play_crs_six_options_pick_null() {
        let txt = r#"{"options":[{"label":"1:0","prob":0.18},{"label":"2:1","prob":0.16},
            {"label":"1:1","prob":0.14},{"label":"2:0","prob":0.12},{"label":"0:0","prob":0.10},
            {"label":"0:1","prob":0.08}],"pick":null,"pick_label":"1:0",
            "confidence":0.18,"rationale":"低进球预期"}"#;
        let p = parse_play_prediction(Play::CRS, "m2", "mdl", txt).unwrap();
        assert_eq!(p.play, Play::CRS);
        assert_eq!(p.options.len(), 6);
        assert!(p.pick.is_none());
        assert_eq!(p.pick_label, "1:0");
    }

    #[test]
    fn parse_play_had_rejects_bad_sum() {
        let txt = r#"{"options":[{"label":"主胜","prob":0.9},{"label":"平局","prob":0.9},
            {"label":"客胜","prob":0.9}],"pick":"Home","pick_label":"主胜",
            "confidence":0.5,"rationale":"x"}"#;
        assert!(parse_play_prediction(Play::HAD, "m", "m", txt).is_err());
    }

    #[test]
    fn parse_play_had_rejects_null_pick() {
        let txt = r#"{"options":[{"label":"主胜","prob":0.4},{"label":"平局","prob":0.3},
            {"label":"客胜","prob":0.3}],"pick":null,"pick_label":"主胜",
            "confidence":0.4,"rationale":"x"}"#;
        assert!(parse_play_prediction(Play::HAD, "m", "m", txt).is_err());
    }

    #[test]
    fn predict_play_defaults_to_had() {
        assert_eq!(Play::default(), Play::HAD);
    }

    #[test]
    fn hhad_body_includes_line_and_odds() {
        let cfg = ApiConfig { base_url: "https://x".into(), api_key: "k".into(),
            model: "m".into(), protocol: ApiProtocol::Anthropic };
        let mut m = sample();
        m.hhad_line = Some(-2);
        m.hhad_odds = Some(Odds { home: 2.54, draw: 4.00, away: 2.06 });
        let b = build_play_body(&cfg, &m, Play::HHAD);
        let user = b["messages"][0]["content"].as_str().unwrap();
        assert!(user.contains("让球数 -2"));
        assert!(user.contains("让球赔率"));
        let sys = b["system"].as_str().unwrap();
        assert!(sys.contains("让球数=-2"));
    }
}
