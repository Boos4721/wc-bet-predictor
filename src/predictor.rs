use crate::domain::{Bet, BetStatus, Match, Outcome, PlayOption};
use crate::ledger::Stats;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ApiProtocol { Anthropic, OpenAI }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Play { HAD, HHAD, CRS, TTG, HAFU, HT }

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

/// 给定玩法在该场比赛中的市场选项集合(label=中文,odds=小数赔率)。
/// 返回 Some 表示该玩法有真实盘口可锚定;None 表示让模型自行构造选项空间。
pub fn play_options(m: &Match, play: Play) -> Option<Vec<PlayOption>> {
    match play {
        Play::HAD => Some(vec![
            PlayOption { label: "主胜".into(), odds: m.odds.home },
            PlayOption { label: "平局".into(), odds: m.odds.draw },
            PlayOption { label: "客胜".into(), odds: m.odds.away },
        ]),
        Play::HHAD => m.hhad_odds.as_ref().map(|o| vec![
            PlayOption { label: "主胜".into(), odds: o.home },
            PlayOption { label: "平局".into(), odds: o.draw },
            PlayOption { label: "客胜".into(), odds: o.away },
        ]),
        Play::CRS => m.pm_score.clone(),
        Play::HT => m.pm_halftime.clone(),
        Play::TTG | Play::HAFU => None,
    }
}

/// 把 pick_label 映射到 Outcome(仅 HAD/HHAD 适用)。
fn outcome_from_label(label: &str) -> Option<Outcome> {
    match label {
        "主胜" => Some(Outcome::Home),
        "平局" => Some(Outcome::Draw),
        "客胜" => Some(Outcome::Away),
        _ => None,
    }
}

/// 把选项集合连同隐含概率(1/odds 归一)拼成提示文本片段。
fn options_block(opts: &[PlayOption]) -> String {
    let inv: Vec<f64> = opts.iter().map(|o| if o.odds > 0.0 { 1.0 / o.odds } else { 0.0 }).collect();
    let sum: f64 = inv.iter().sum();
    let mut s = String::from("市场可选结果(含隐含概率):");
    for (o, p) in opts.iter().zip(inv.iter()) {
        let implied = if sum > 0.0 { p / sum } else { 0.0 };
        s.push_str(&format!("[{} 赔{:.2} 隐含{:.1}%] ", o.label, o.odds, implied * 100.0));
    }
    s
}

fn play_outcome_rule(play: Play, line: Option<i32>, opts: Option<&[PlayOption]>) -> String {
    if let Some(opts) = opts {
        let labels: Vec<&str> = opts.iter().map(|o| o.label.as_str()).collect();
        let need_pick = matches!(play, Play::HAD | Play::HHAD);
        let pick_rule = if need_pick {
            "pick 用 Home/Draw/Away 对应你推荐的项;pick_label 用对应中文(主胜/平局/客胜)。"
        } else {
            "pick=null;pick_label 为你推荐的结果(必须是上面给定 label 之一)。"
        };
        let line_hint = if play == Play::HHAD {
            format!("玩法=让球胜平负,让球数={}(主队让/受球)。",
                line.map(|l| l.to_string()).unwrap_or_else(|| "未知".into()))
        } else { String::new() };
        return format!(
            "{}options 必须严格使用以下给定 label(逐项给出你自己的概率 prob,可不必和为1,因含抽水/其他比分):{}。{}",
            line_hint, labels.join("、"), pick_rule);
    }
    // 自由玩法(体彩 CRS/TTG/HAFU):模型自行构造选项空间。
    match play {
        Play::CRS => "玩法=正确比分。给出最可能的 6~8 个比分(label 如 \"2:1\"),prob 为各自概率(不必和为1,取主要比分即可)。pick=null。pick_label=最可能比分。".to_string(),
        Play::TTG => "玩法=总进球数。options 覆盖 0,1,2,3,4,5,6,7+(label 如 \"0球\"…\"7+球\"),prob 和≈1。pick=null。pick_label=最可能档。".to_string(),
        Play::HAFU => "玩法=半全场(半场/全场结果组合)。options 为 9 种组合,label 用 胜/胜、胜/平、胜/负、平/胜、平/平、平/负、负/胜、负/平、负/负,prob 和≈1。pick=null。pick_label=最可能组合。".to_string(),
        Play::HT => "玩法=半场胜平负。options 恰好三项,label 用 半场主胜/半场平局/半场客胜,prob 和≈1。pick=null。pick_label=最可能项。".to_string(),
        Play::HAD => format!("玩法=胜平负。options 恰好三项,label 用 主胜/平局/客胜,prob 三者之和≈1。pick 用 Home/Draw/Away 对应最高项。"),
        Play::HHAD => format!("玩法=让球胜平负,让球数={}(主队让/受球)。在该让球盘口下给出 主胜/平局/客胜 三项概率(和≈1),pick 用 Home/Draw/Away。",
            line.map(|l| l.to_string()).unwrap_or_else(|| "未知".into())),
    }
}

fn play_system_prompt(play: Play, line: Option<i32>, opts: Option<&[PlayOption]>) -> String {
    format!("{PLAY_BASE_PROMPT}{}", play_outcome_rule(play, line, opts))
}

fn play_user_prompt(m: &Match, play: Play, opts: Option<&[PlayOption]>) -> String {
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
    if let Some(opts) = opts {
        s.push_str(&options_block(opts));
    }
    s.push_str("请按上述玩法给出概率、推荐与中文理由。");
    s
}

pub fn build_play_body(cfg: &ApiConfig, m: &Match, play: Play) -> Value {
    let opts = play_options(m, play);
    let system = play_system_prompt(play, m.hhad_line, opts.as_deref());
    let user = play_user_prompt(m, play, opts.as_deref());
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

/// 解析玩法预测(纯函数,无网络)。
/// `opts` = 该玩法的市场选项集合(HAD/HHAD/CRS-poly/HT-poly 为 Some,自由玩法为 None)。
/// 当 opts 为 Some:按 pick_label 在 opts 中查 pick_odds;HAD/HHAD 另由 label 推出 pick。
pub fn parse_play_prediction(play: Play, match_id: &str, model: &str, text: &str,
    opts: Option<&[PlayOption]>) -> Result<PlayPrediction, PredictError>
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

    let pick_label = if raw.pick_label.is_empty() {
        // 退回:取概率最高项的 label
        raw.options.iter()
            .max_by(|a, b| a.prob.partial_cmp(&b.prob).unwrap_or(std::cmp::Ordering::Equal))
            .map(|o| o.label.clone()).unwrap_or_default()
    } else {
        raw.pick_label.clone()
    };

    // 计算 pick:HAD/HHAD 由 pick_label 推出(回退到 raw.pick);其余为 None。
    let pick = if matches!(play, Play::HAD | Play::HHAD) {
        outcome_from_label(&pick_label).or_else(|| raw.pick.as_deref().and_then(parse_outcome))
    } else {
        None
    };

    // HAD/HHAD 是封闭三选一:严格校验。CRS/HT(option-backed)概率不强制和为1。
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

    // pick_odds:在 opts 中按 pick_label 精确匹配。
    let pick_odds = opts.and_then(|os| os.iter()
        .find(|o| o.label == pick_label)
        .map(|o| o.odds));

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
        pick_odds,
    })
}

async fn call_ai_play(cfg: &ApiConfig, m: &Match, play: Play) -> Result<PlayPrediction, PredictError> {
    let body = build_play_body(cfg, m, play);
    let v = post_ai(cfg, &body).await?;
    let text = extract_text(cfg.protocol, &v)?;
    let opts = play_options(m, play);
    parse_play_prediction(play, &m.id, &cfg.model, &text, opts.as_deref())
}

/// AI 接口的 URL(按协议)。
fn ai_url(cfg: &ApiConfig) -> String {
    match cfg.protocol {
        ApiProtocol::Anthropic => format!("{}/v1/messages", cfg.base_url.trim_end_matches('/')),
        ApiProtocol::OpenAI => format!("{}/chat/completions", cfg.base_url.trim_end_matches('/')),
    }
}

/// POST 一个请求体到 AI 接口,返回原始 JSON 响应(供对话/玩法/工具循环共用)。
pub async fn post_ai(cfg: &ApiConfig, body: &Value) -> Result<Value, PredictError> {
    let client = reqwest::Client::new();
    let url = ai_url(cfg);
    let mut req = client.post(&url).json(body);
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
    resp.json().await.map_err(|e| PredictError::Parse(e.to_string()))
}

/// 玩法预测,失败重试一次。
pub async fn predict_play(cfg: &ApiConfig, m: &Match, play: Play) -> Result<PlayPrediction, PredictError> {
    match call_ai_play(cfg, m, play).await {
        Ok(p) => Ok(p),
        Err(_) => call_ai_play(cfg, m, play).await,
    }
}

// ===== 对话(自由文本回复,不强制 JSON) =====

#[derive(serde::Deserialize, serde::Serialize, Clone)]
pub struct ChatMsg { pub role: String, pub content: String }

/// 把赛事列表压缩成上下文文本(供 system 提示注入)。
pub fn matches_context(ms: &[Match]) -> String {
    if ms.is_empty() { return "当前没有加载任何赛事。".into(); }
    let mut s = String::from("当前赛事列表(供参考):\n");
    for m in ms.iter().take(40) {
        s.push_str(&format!("- {} {} vs {}({}) 胜平负赔率 主{}/平{}/客{}\n",
            m.id, m.home, m.away, m.kickoff, m.odds.home, m.odds.draw, m.odds.away));
    }
    s
}

fn outcome_zh(o: Outcome) -> &'static str {
    match o { Outcome::Home => "主胜", Outcome::Draw => "平局", Outcome::Away => "客胜" }
}

fn status_zh(s: BetStatus) -> &'static str {
    match s { BetStatus::Pending => "待结算", BetStatus::Won => "命中", BetStatus::Lost => "未中" }
}

/// 把账本战绩与近期注单压缩成上下文文本(纯函数,供 system 提示注入)。
pub fn ledger_context(stats: &Stats, bets: &[Bet]) -> String {
    let mut s = format!(
        "账本战绩:已结算 {} 笔,命中率 {:.0}%,累计盈亏 {:.0},ROI {:.0}%。",
        stats.settled, stats.hit_rate * 100.0, stats.total_pnl, stats.roi * 100.0);
    if bets.is_empty() {
        s.push_str("近期注单:暂无。");
    } else {
        s.push_str("近期注单:");
        for b in bets.iter().take(10) {
            s.push_str(&format!("{} {} @{} {}元 [{}];",
                b.match_id, outcome_zh(b.pick), b.odds_at_bet, b.stake, status_zh(b.status)));
        }
    }
    s
}

// ===== 只读工具(function calling)agentic 循环 =====

/// 解析后的工具调用。
#[derive(Debug, Clone)]
pub struct ToolCall { pub id: String, pub name: String, pub args: Value }

/// 5 个只读工具的统一定义:名称、描述、JSON-Schema 参数(单一事实来源)。
fn tool_defs() -> Vec<(&'static str, &'static str, Value)> {
    let source_enum = json!({"type":"string","enum":["sporttery","polymarket"],"description":"数据源"});
    let play_enum = json!({"type":"string","enum":["HAD","HHAD","CRS","TTG","HAFU","HT"],
        "description":"玩法,默认 HAD(胜平负)"});
    vec![
        ("list_dates", "列出某数据源可用的比赛日及每日场次数",
            json!({"type":"object",
                "properties":{"source":source_enum},
                "required":["source"]})),
        ("get_matches", "获取某数据源(可按日期过滤)的赛事列表,含 id、队伍、开赛、赔率",
            json!({"type":"object",
                "properties":{"source":source_enum,"date":{"type":"string","description":"开赛日期,可选"}},
                "required":["source"]})),
        ("predict", "对某场比赛运行一个玩法的预测,返回推荐与各项概率",
            json!({"type":"object",
                "properties":{"source":source_enum,"match_id":{"type":"string","description":"赛事 id"},"play":play_enum},
                "required":["source","match_id"]})),
        ("get_stats", "查询账本战绩统计(已结算笔数、命中率、累计盈亏、ROI)",
            json!({"type":"object","properties":{}})),
        ("list_bets", "列出近期注单记录",
            json!({"type":"object","properties":{}})),
    ]
}

/// 工具规格数组(按协议包装)。Anthropic 与 OpenAI 名称/参数一致,仅外层结构不同。
pub fn tool_specs(p: ApiProtocol) -> Value {
    let defs = tool_defs();
    let arr: Vec<Value> = defs.into_iter().map(|(name, desc, schema)| match p {
        ApiProtocol::Anthropic => json!({
            "name": name, "description": desc, "input_schema": schema
        }),
        ApiProtocol::OpenAI => json!({
            "type": "function",
            "function": {"name": name, "description": desc, "parameters": schema}
        }),
    }).collect();
    Value::Array(arr)
}

/// 构造带工具的对话请求体。`msgs_json` 为已按协议成形的运行中消息数组。
pub fn build_agent_body(cfg: &ApiConfig, system: &str, msgs_json: &Value) -> Value {
    let tools = tool_specs(cfg.protocol);
    match cfg.protocol {
        ApiProtocol::Anthropic => json!({
            "model": cfg.model, "max_tokens": 1024, "system": system,
            "messages": msgs_json, "tools": tools, "tool_choice": {"type": "auto"}
        }),
        ApiProtocol::OpenAI => {
            let mut full = vec![json!({"role":"system","content": system})];
            if let Some(arr) = msgs_json.as_array() { full.extend(arr.iter().cloned()); }
            json!({"model": cfg.model, "messages": full, "tools": tools, "tool_choice": "auto"})
        }
    }
}

/// 构造不带工具的对话请求体(用于步数耗尽后强制出文本)。
pub fn build_agent_body_no_tools(cfg: &ApiConfig, system: &str, msgs_json: &Value) -> Value {
    match cfg.protocol {
        ApiProtocol::Anthropic => json!({
            "model": cfg.model, "max_tokens": 1024, "system": system, "messages": msgs_json
        }),
        ApiProtocol::OpenAI => {
            let mut full = vec![json!({"role":"system","content": system})];
            if let Some(arr) = msgs_json.as_array() { full.extend(arr.iter().cloned()); }
            json!({"model": cfg.model, "messages": full})
        }
    }
}

/// 从响应中解析工具调用;仅有文本时返回空 vec(循环结束)。
pub fn parse_tool_calls(p: ApiProtocol, resp: &Value) -> Vec<ToolCall> {
    match p {
        ApiProtocol::Anthropic => resp["content"].as_array().map(|items| {
            items.iter().filter(|it| it["type"] == "tool_use").map(|it| ToolCall {
                id: it["id"].as_str().unwrap_or_default().to_string(),
                name: it["name"].as_str().unwrap_or_default().to_string(),
                args: it["input"].clone(),
            }).collect()
        }).unwrap_or_default(),
        ApiProtocol::OpenAI => resp["choices"][0]["message"]["tool_calls"].as_array().map(|items| {
            items.iter().map(|it| ToolCall {
                id: it["id"].as_str().unwrap_or_default().to_string(),
                name: it["function"]["name"].as_str().unwrap_or_default().to_string(),
                args: it["function"]["arguments"].as_str()
                    .and_then(|s| serde_json::from_str::<Value>(s).ok())
                    .unwrap_or_else(|| json!({})),
            }).collect()
        }).unwrap_or_default(),
    }
}

/// 提取最终文本;无文本(仅工具调用)时返回 ""。
pub fn final_text(p: ApiProtocol, resp: &Value) -> String {
    match p {
        ApiProtocol::Anthropic => resp["content"].as_array()
            .map(|items| items.iter()
                .filter_map(|it| if it["type"] == "text" { it["text"].as_str() } else { None })
                .collect::<Vec<_>>().join(""))
            .unwrap_or_default(),
        ApiProtocol::OpenAI => resp["choices"][0]["message"]["content"].as_str()
            .unwrap_or_default().to_string(),
    }
}

/// 助手轮(把模型上一次响应原样回填进消息数组)。
pub fn assistant_turn(p: ApiProtocol, resp: &Value) -> Value {
    match p {
        ApiProtocol::Anthropic => json!({"role":"assistant","content": resp["content"].clone()}),
        ApiProtocol::OpenAI => resp["choices"][0]["message"].clone(),
    }
}

/// 单个工具结果轮。Anthropic 用 user 轮包 tool_result;OpenAI 用 role=tool。
pub fn tool_result_turn(p: ApiProtocol, call_id: &str, content: &str) -> Value {
    match p {
        ApiProtocol::Anthropic => json!({"role":"user","content":[
            {"type":"tool_result","tool_use_id": call_id,"content": content}
        ]}),
        ApiProtocol::OpenAI => json!({"role":"tool","tool_call_id": call_id,"content": content}),
    }
}

/// 把 ChatMsg 列表映射成协议中立的消息数组(role 仅 user/assistant)。
pub fn seed_messages(msgs: &[ChatMsg]) -> Value {
    Value::Array(msgs.iter().map(|m| json!({
        "role": if m.role == "assistant" { "assistant" } else { "user" },
        "content": m.content
    })).collect())
}

/// 把工具名解析为 Play(默认 HAD)。
pub fn play_from_str(s: &str) -> Play {
    match s {
        "HHAD" => Play::HHAD, "CRS" => Play::CRS, "TTG" => Play::TTG,
        "HAFU" => Play::HAFU, "HT" => Play::HT, _ => Play::HAD,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{Match, Odds, Outcome, PlayOption};

    fn sample() -> Match {
        Match { id: "周日001".into(), league: "世界杯".into(),
            home: "A".into(), away: "B".into(), kickoff: "2026-06-20T19:00:00".into(),
            odds: Odds { home: 2.1, draw: 3.2, away: 3.5 }, handicap: None,
            hhad_odds: None, hhad_line: None, pm_score: None, pm_halftime: None }
    }

    fn had_opts() -> Vec<PlayOption> {
        vec![
            PlayOption { label: "主胜".into(), odds: 2.1 },
            PlayOption { label: "平局".into(), odds: 3.2 },
            PlayOption { label: "客胜".into(), odds: 3.5 },
        ]
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
        assert!(parse_play_prediction(Play::HAD, "m", "m", txt, Some(&had_opts())).is_ok());
    }

    #[test]
    fn parse_play_had_three_options() {
        let txt = r#"{"options":[{"label":"主胜","prob":0.52},{"label":"平局","prob":0.27},
            {"label":"客胜","prob":0.21}],"pick":"Home","pick_label":"主胜",
            "confidence":0.52,"rationale":"主队状态好"}"#;
        let p = parse_play_prediction(Play::HAD, "m1", "mdl", txt, Some(&had_opts())).unwrap();
        assert_eq!(p.play, Play::HAD);
        assert_eq!(p.options.len(), 3);
        assert_eq!(p.pick, Some(Outcome::Home));
        assert_eq!(p.pick_label, "主胜");
        // option-backed:pick_odds 由 pick_label 在 opts 中查得 = 主胜赔率
        assert_eq!(p.pick_odds, Some(2.1));
    }

    #[test]
    fn parse_play_crs_option_backed_pick_odds() {
        // 模型在给定比分 label 上给出自己的概率,pick_label 命中其中一项 → pick_odds=该项赔率
        let opts = vec![
            PlayOption { label: "英格兰 2 - 1 克罗地亚".into(), odds: 9.5 },
            PlayOption { label: "英格兰 1 - 0 克罗地亚".into(), odds: 7.4 },
            PlayOption { label: "其他比分".into(), odds: 5.1 },
        ];
        let txt = r#"{"options":[{"label":"英格兰 2 - 1 克罗地亚","prob":0.12},
            {"label":"英格兰 1 - 0 克罗地亚","prob":0.14},{"label":"其他比分","prob":0.20}],
            "pick":null,"pick_label":"英格兰 1 - 0 克罗地亚","confidence":0.3,"rationale":"低进球预期"}"#;
        let p = parse_play_prediction(Play::CRS, "m2", "mdl", txt, Some(&opts)).unwrap();
        assert_eq!(p.play, Play::CRS);
        assert_eq!(p.options.len(), 3);
        assert!(p.pick.is_none()); // 比分玩法无 Outcome
        assert_eq!(p.pick_label, "英格兰 1 - 0 克罗地亚");
        assert_eq!(p.pick_odds, Some(7.4));
    }

    #[test]
    fn parse_play_ht_three_options() {
        let opts = vec![
            PlayOption { label: "半场主胜".into(), odds: 2.0 },
            PlayOption { label: "半场平局".into(), odds: 2.9 },
            PlayOption { label: "半场客胜".into(), odds: 4.8 },
        ];
        let txt = r#"{"options":[{"label":"半场主胜","prob":0.42},{"label":"半场平局","prob":0.40},
            {"label":"半场客胜","prob":0.18}],"pick":null,"pick_label":"半场平局",
            "confidence":0.4,"rationale":"半场常平"}"#;
        let p = parse_play_prediction(Play::HT, "m3", "mdl", txt, Some(&opts)).unwrap();
        assert_eq!(p.play, Play::HT);
        assert_eq!(p.options.len(), 3);
        assert!(p.pick.is_none());
        assert_eq!(p.pick_label, "半场平局");
        assert_eq!(p.pick_odds, Some(2.9));
    }

    #[test]
    fn parse_play_crs_six_options_pick_null() {
        // 自由玩法(opts=None):模型自构造比分空间,pick_odds 留空
        let txt = r#"{"options":[{"label":"1:0","prob":0.18},{"label":"2:1","prob":0.16},
            {"label":"1:1","prob":0.14},{"label":"2:0","prob":0.12},{"label":"0:0","prob":0.10},
            {"label":"0:1","prob":0.08}],"pick":null,"pick_label":"1:0",
            "confidence":0.18,"rationale":"低进球预期"}"#;
        let p = parse_play_prediction(Play::CRS, "m2", "mdl", txt, None).unwrap();
        assert_eq!(p.play, Play::CRS);
        assert_eq!(p.options.len(), 6);
        assert!(p.pick.is_none());
        assert_eq!(p.pick_label, "1:0");
        assert!(p.pick_odds.is_none());
    }

    #[test]
    fn parse_play_had_rejects_bad_sum() {
        let txt = r#"{"options":[{"label":"主胜","prob":0.9},{"label":"平局","prob":0.9},
            {"label":"客胜","prob":0.9}],"pick":"Home","pick_label":"主胜",
            "confidence":0.5,"rationale":"x"}"#;
        assert!(parse_play_prediction(Play::HAD, "m", "m", txt, Some(&had_opts())).is_err());
    }

    #[test]
    fn parse_play_had_rejects_null_pick() {
        // pick=null 且 pick_label 不能映射到 主胜/平局/客胜 → 无法得出 pick → HAD 拒绝
        let bad = r#"{"options":[{"label":"未知1","prob":0.4},{"label":"未知2","prob":0.3},
            {"label":"未知3","prob":0.3}],"pick":null,"pick_label":"未知1",
            "confidence":0.4,"rationale":"x"}"#;
        assert!(parse_play_prediction(Play::HAD, "m", "m", bad, Some(&had_opts())).is_err());
    }

    #[test]
    fn predict_play_defaults_to_had() {
        assert_eq!(Play::default(), Play::HAD);
    }

    #[test]
    fn chat_body_and_context() {
        // matches_context:空返回提示串;非空含队名
        assert_eq!(matches_context(&[]), "当前没有加载任何赛事。");
        let ctx = matches_context(&[sample()]);
        assert!(ctx.contains("A vs B"));
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

    use crate::domain::{Bet, BetStatus};
    use crate::ledger::Stats;

    fn acfg() -> ApiConfig {
        ApiConfig { base_url: "https://x".into(), api_key: "k".into(),
            model: "m".into(), protocol: ApiProtocol::Anthropic }
    }
    fn ocfg() -> ApiConfig {
        ApiConfig { base_url: "https://x".into(), api_key: "k".into(),
            model: "m".into(), protocol: ApiProtocol::OpenAI }
    }

    #[test]
    fn ledger_context_formats_stats_and_bets() {
        let stats = Stats { settled: 5, hit_rate: 0.6, total_pnl: 120.0, roi: 0.24 };
        let bets = vec![Bet { id: 1, match_id: "周三021".into(), pick: Outcome::Home,
            stake: 100.0, odds_at_bet: 1.8, status: BetStatus::Won,
            created_at: "@1".into() }];
        let s = ledger_context(&stats, &bets);
        assert!(s.contains("已结算 5 笔"));
        assert!(s.contains("命中率 60%"));
        assert!(s.contains("周三021 主胜 @1.8 100元 [命中]"));
    }

    #[test]
    fn ledger_context_empty_bets() {
        let stats = Stats { settled: 0, hit_rate: 0.0, total_pnl: 0.0, roi: 0.0 };
        assert!(ledger_context(&stats, &[]).contains("近期注单:暂无"));
    }

    #[test]
    fn tool_specs_anthropic_has_five_tools() {
        let v = tool_specs(ApiProtocol::Anthropic);
        let arr = v.as_array().unwrap();
        assert_eq!(arr.len(), 5);
        assert_eq!(arr[0]["name"], "list_dates");
        assert!(arr[0]["input_schema"].is_object());
        let names: Vec<&str> = arr.iter().map(|t| t["name"].as_str().unwrap()).collect();
        for n in ["list_dates","get_matches","predict","get_stats","list_bets"] {
            assert!(names.contains(&n), "missing {n}");
        }
    }

    #[test]
    fn tool_specs_openai_has_five_function_tools() {
        let v = tool_specs(ApiProtocol::OpenAI);
        let arr = v.as_array().unwrap();
        assert_eq!(arr.len(), 5);
        assert_eq!(arr[0]["type"], "function");
        assert_eq!(arr[0]["function"]["name"], "list_dates");
        assert!(arr[0]["function"]["parameters"].is_object());
    }

    #[test]
    fn build_agent_body_includes_tools() {
        let msgs = seed_messages(&[ChatMsg { role: "user".into(), content: "hi".into() }]);
        let ab = build_agent_body(&acfg(), "sys", &msgs);
        assert!(ab["tools"].is_array());
        assert_eq!(ab["tools"].as_array().unwrap().len(), 5);
        assert_eq!(ab["tool_choice"]["type"], "auto");
        assert_eq!(ab["system"], "sys");

        let ob = build_agent_body(&ocfg(), "sys", &msgs);
        assert!(ob["tools"].is_array());
        assert_eq!(ob["tool_choice"], "auto");
        assert_eq!(ob["messages"][0]["role"], "system");
    }

    #[test]
    fn build_agent_body_no_tools_omits_tools() {
        let msgs = seed_messages(&[ChatMsg { role: "user".into(), content: "hi".into() }]);
        let ab = build_agent_body_no_tools(&acfg(), "sys", &msgs);
        assert!(ab["tools"].is_null());
    }

    #[test]
    fn parse_tool_calls_anthropic() {
        let resp = json!({"content":[
            {"type":"text","text":"让我查一下"},
            {"type":"tool_use","id":"tu_1","name":"get_matches","input":{"source":"sporttery"}}
        ]});
        let calls = parse_tool_calls(ApiProtocol::Anthropic, &resp);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].id, "tu_1");
        assert_eq!(calls[0].name, "get_matches");
        assert_eq!(calls[0].args["source"], "sporttery");
        // 纯文本响应 → 无工具调用
        let text_only = json!({"content":[{"type":"text","text":"答案"}]});
        assert!(parse_tool_calls(ApiProtocol::Anthropic, &text_only).is_empty());
    }

    #[test]
    fn parse_tool_calls_openai() {
        let resp = json!({"choices":[{"message":{"content":null,"tool_calls":[
            {"id":"call_1","type":"function","function":{"name":"predict","arguments":"{\"source\":\"sporttery\",\"match_id\":\"周三021\"}"}}
        ]}}]});
        let calls = parse_tool_calls(ApiProtocol::OpenAI, &resp);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].id, "call_1");
        assert_eq!(calls[0].name, "predict");
        assert_eq!(calls[0].args["match_id"], "周三021");
        let text_only = json!({"choices":[{"message":{"content":"答案"}}]});
        assert!(parse_tool_calls(ApiProtocol::OpenAI, &text_only).is_empty());
    }

    #[test]
    fn final_text_tolerates_tool_only() {
        let tool_only = json!({"content":[{"type":"tool_use","id":"x","name":"get_stats","input":{}}]});
        assert_eq!(final_text(ApiProtocol::Anthropic, &tool_only), "");
        let with_text = json!({"content":[{"type":"text","text":"结论"}]});
        assert_eq!(final_text(ApiProtocol::Anthropic, &with_text), "结论");
    }

    #[test]
    fn turn_builders_shape() {
        // Anthropic 工具结果 = user 轮 + tool_result
        let ar = tool_result_turn(ApiProtocol::Anthropic, "tu_1", "data");
        assert_eq!(ar["role"], "user");
        assert_eq!(ar["content"][0]["type"], "tool_result");
        assert_eq!(ar["content"][0]["tool_use_id"], "tu_1");
        // OpenAI 工具结果 = role=tool
        let or = tool_result_turn(ApiProtocol::OpenAI, "call_1", "data");
        assert_eq!(or["role"], "tool");
        assert_eq!(or["tool_call_id"], "call_1");
    }

    #[test]
    fn play_from_str_defaults_had() {
        assert_eq!(play_from_str("HHAD"), Play::HHAD);
        assert_eq!(play_from_str("xyz"), Play::HAD);
        assert_eq!(play_from_str("CRS"), Play::CRS);
    }
}
