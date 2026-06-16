# 世界杯体彩预测 + 模拟下单 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 构建本地自用的竞彩足球预测工具:粘贴赛事数据 → 大模型(Claude/OpenAI 双格式)预测胜平负 → 虚拟账本记账与盈亏统计,本地 Web 界面驱动。

**Architecture:** Rust(axum + rusqlite)后端按模块拆分(domain/source/ledger/predictor/config/api),后端代发 AI 请求避开 CORS;Bun 原生 TS 前端四区块,通过 REST 调用后端。取数被隔离在 `MatchSource` trait 后,默认手动粘贴。金额全为虚拟,不绕任何站点防护。

**Tech Stack:** Rust 1.75+、axum 0.7、rusqlite 0.31(bundled)、serde/serde_json、reqwest(blocking 暂不用,用 async)、tokio;Bun + 原生 TypeScript + Vite。

---

## 约定

- 工作目录:`/Users/Boos/wc-bet-predictor`
- 时间戳统一用 ISO8601 字符串(`String`),避免引入 chrono,保持 KISS。
- 所有金额、赔率为 `f64`;概率为 `f64`,校验三者之和 ≈ 1(容差 0.02)。
- 每个任务结束即 commit。

---

## Task 1: 后端脚手架

**Files:**
- Create: `backend/Cargo.toml`
- Create: `backend/src/main.rs`

- [ ] **Step 1: 创建 Cargo.toml**

```toml
[package]
name = "wc-bet-predictor"
version = "0.1.0"
edition = "2021"

[dependencies]
axum = "0.7"
tokio = { version = "1", features = ["full"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
rusqlite = { version = "0.31", features = ["bundled"] }
reqwest = { version = "0.12", features = ["json"] }
tower-http = { version = "0.5", features = ["fs", "cors"] }
thiserror = "1"

[dev-dependencies]
tokio = { version = "1", features = ["full", "test-util"] }
```

- [ ] **Step 2: 创建最小 main.rs**

```rust
#[tokio::main]
async fn main() {
    println!("wc-bet-predictor starting...");
}
```

- [ ] **Step 3: 验证编译**

Run: `cd backend && cargo build`
Expected: 编译成功(下载依赖后 `Finished`)。

- [ ] **Step 4: Commit**

```bash
git add backend/Cargo.toml backend/src/main.rs
git commit -m "chore: backend scaffold"
```

## Task 2: 领域类型(domain.rs)

**Files:**
- Create: `backend/src/domain.rs`
- Modify: `backend/src/main.rs`(加 `mod domain;`)

- [ ] **Step 1: 写失败测试**

在 `backend/src/domain.rs` 末尾:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outcome_serde_roundtrip() {
        let o = Outcome::Draw;
        let s = serde_json::to_string(&o).unwrap();
        assert_eq!(s, "\"Draw\"");
        let back: Outcome = serde_json::from_str(&s).unwrap();
        assert_eq!(back, Outcome::Draw);
    }

    #[test]
    fn match_serializes_odds() {
        let m = Match {
            id: "周日001".into(), league: "世界杯".into(),
            home: "A".into(), away: "B".into(),
            kickoff: "2026-06-20T19:00:00".into(),
            odds: Odds { home: 2.1, draw: 3.2, away: 3.5 },
            handicap: None,
        };
        let v = serde_json::to_value(&m).unwrap();
        assert_eq!(v["odds"]["home"], 2.1);
        assert!(v["handicap"].is_null());
    }
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cd backend && cargo test domain`
Expected: FAIL(`cannot find type Outcome`/编译错误)。

- [ ] **Step 3: 实现类型**

在 `backend/src/domain.rs` 顶部:

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Outcome { Home, Draw, Away }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BetStatus { Pending, Won, Lost }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Odds { pub home: f64, pub draw: f64, pub away: f64 }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Probs { pub home: f64, pub draw: f64, pub away: f64 }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Match {
    pub id: String,
    pub league: String,
    pub home: String,
    pub away: String,
    pub kickoff: String,
    pub odds: Odds,
    pub handicap: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Prediction {
    pub match_id: String,
    pub probs: Probs,
    pub pick: Outcome,
    pub confidence: f32,
    pub rationale: String,
    pub model: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bet {
    pub id: i64,
    pub match_id: String,
    pub pick: Outcome,
    pub stake: f64,
    pub odds_at_bet: f64,
    pub status: BetStatus,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settlement {
    pub bet_id: i64,
    pub actual_result: Outcome,
    pub payout: f64,
    pub pnl: f64,
    pub settled_at: String,
}
```

在 `backend/src/main.rs` 顶部加:`mod domain;`

- [ ] **Step 4: 运行确认通过**

Run: `cd backend && cargo test domain`
Expected: PASS(2 passed)。

- [ ] **Step 5: Commit**

```bash
git add backend/src/domain.rs backend/src/main.rs
git commit -m "feat: domain types"
```

---

## Task 3: 粘贴解析器(source.rs)

支持两种粘贴格式:**JSON 数组**(官网接口/手动整理)与**简单文本行**(`场次|联赛|主|客|开赛|主赔|平赔|客赔[|让球]`)。

**Files:**
- Create: `backend/src/source.rs`
- Modify: `backend/src/main.rs`(加 `mod source;`)

- [ ] **Step 1: 写失败测试**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_json_array() {
        let raw = r#"[{"id":"周日001","league":"世界杯","home":"A","away":"B",
            "kickoff":"2026-06-20T19:00:00","odds":{"home":2.1,"draw":3.2,"away":3.5},"handicap":null}]"#;
        let ms = PasteSource { raw: raw.into() }.load().unwrap();
        assert_eq!(ms.len(), 1);
        assert_eq!(ms[0].home, "A");
    }

    #[test]
    fn parses_pipe_text() {
        let raw = "周日001|世界杯|A|B|2026-06-20T19:00:00|2.1|3.2|3.5";
        let ms = PasteSource { raw: raw.into() }.load().unwrap();
        assert_eq!(ms[0].odds.draw, 3.2);
        assert_eq!(ms[0].handicap, None);
    }

    #[test]
    fn pipe_text_with_handicap() {
        let raw = "周日002|世界杯|C|D|2026-06-20T22:00:00|1.8|3.4|4.0|-1";
        let ms = PasteSource { raw: raw.into() }.load().unwrap();
        assert_eq!(ms[0].handicap, Some(-1));
    }

    #[test]
    fn bad_line_reports_index() {
        let raw = "周日001|world|A"; // 字段不足
        let err = PasteSource { raw: raw.into() }.load().unwrap_err();
        assert!(format!("{err}").contains("第 1 行"));
    }
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cd backend && cargo test source`
Expected: FAIL(`cannot find PasteSource`)。

- [ ] **Step 3: 实现解析器**

```rust
use crate::domain::{Match, Odds};

#[derive(Debug, thiserror::Error)]
pub enum SourceError {
    #[error("解析失败:{0}")]
    Parse(String),
}

pub trait MatchSource {
    fn load(&self) -> Result<Vec<Match>, SourceError>;
}

pub struct PasteSource { pub raw: String }

impl MatchSource for PasteSource {
    fn load(&self) -> Result<Vec<Match>, SourceError> {
        let trimmed = self.raw.trim_start();
        if trimmed.starts_with('[') {
            serde_json::from_str(trimmed).map_err(|e| SourceError::Parse(e.to_string()))
        } else {
            parse_pipe(&self.raw)
        }
    }
}

fn parse_pipe(raw: &str) -> Result<Vec<Match>, SourceError> {
    let mut out = Vec::new();
    for (i, line) in raw.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() { continue; }
        let f: Vec<&str> = line.split('|').map(|s| s.trim()).collect();
        if f.len() < 8 {
            return Err(SourceError::Parse(format!("第 {} 行字段不足(需 >=8)", i + 1)));
        }
        let num = |s: &str, name: &str| s.parse::<f64>()
            .map_err(|_| SourceError::Parse(format!("第 {} 行 {} 非数字", i + 1, name)));
        out.push(Match {
            id: f[0].into(), league: f[1].into(),
            home: f[2].into(), away: f[3].into(),
            kickoff: f[4].into(),
            odds: Odds { home: num(f[5], "主赔")?, draw: num(f[6], "平赔")?, away: num(f[7], "客赔")? },
            handicap: f.get(8).and_then(|s| s.parse::<i32>().ok()),
        });
    }
    if out.is_empty() {
        return Err(SourceError::Parse("无有效赛事".into()));
    }
    Ok(out)
}
```

在 `main.rs` 加:`mod source;`

- [ ] **Step 4: 运行确认通过**

Run: `cd backend && cargo test source`
Expected: PASS(4 passed)。

- [ ] **Step 5: Commit**

```bash
git add backend/src/source.rs backend/src/main.rs
git commit -m "feat: paste match parser (json + pipe text)"
```

## Task 4: 账本纯函数(ledger.rs — 计算部分)

先实现与存储无关的纯函数:结算计算与统计聚合。SQLite 在 Task 5。

**Files:**
- Create: `backend/src/ledger.rs`
- Modify: `backend/src/main.rs`(加 `mod ledger;`)

- [ ] **Step 1: 写失败测试**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::Outcome;

    #[test]
    fn settle_win_pays_stake_times_odds() {
        let (payout, pnl) = settle_amounts(Outcome::Home, Outcome::Home, 100.0, 2.1);
        assert!((payout - 210.0).abs() < 1e-9);
        assert!((pnl - 110.0).abs() < 1e-9);
    }

    #[test]
    fn settle_loss_loses_stake() {
        let (payout, pnl) = settle_amounts(Outcome::Home, Outcome::Away, 100.0, 2.1);
        assert_eq!(payout, 0.0);
        assert!((pnl + 100.0).abs() < 1e-9);
    }

    #[test]
    fn stats_aggregate() {
        let rows = vec![
            SettledRow { stake: 100.0, pnl: 110.0, won: true },
            SettledRow { stake: 100.0, pnl: -100.0, won: false },
            SettledRow { stake: 50.0, pnl: 40.0, won: true },
        ];
        let s = compute_stats(&rows);
        assert_eq!(s.settled, 3);
        assert!((s.hit_rate - 2.0/3.0).abs() < 1e-9);
        assert!((s.total_pnl - 50.0).abs() < 1e-9);
        assert!((s.roi - 50.0/250.0).abs() < 1e-9);
    }

    #[test]
    fn stats_empty_is_zero() {
        let s = compute_stats(&[]);
        assert_eq!(s.settled, 0);
        assert_eq!(s.hit_rate, 0.0);
        assert_eq!(s.roi, 0.0);
    }
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cd backend && cargo test ledger`
Expected: FAIL(`cannot find settle_amounts`)。

- [ ] **Step 3: 实现纯函数**

```rust
use crate::domain::Outcome;
use serde::Serialize;

/// 返回 (payout, pnl)
pub fn settle_amounts(pick: Outcome, actual: Outcome, stake: f64, odds: f64) -> (f64, f64) {
    if pick == actual {
        let payout = stake * odds;
        (payout, payout - stake)
    } else {
        (0.0, -stake)
    }
}

pub struct SettledRow { pub stake: f64, pub pnl: f64, pub won: bool }

#[derive(Debug, Serialize)]
pub struct Stats {
    pub settled: usize,
    pub hit_rate: f64,
    pub total_pnl: f64,
    pub roi: f64,
}

pub fn compute_stats(rows: &[SettledRow]) -> Stats {
    let settled = rows.len();
    if settled == 0 {
        return Stats { settled: 0, hit_rate: 0.0, total_pnl: 0.0, roi: 0.0 };
    }
    let wins = rows.iter().filter(|r| r.won).count();
    let total_pnl: f64 = rows.iter().map(|r| r.pnl).sum();
    let total_stake: f64 = rows.iter().map(|r| r.stake).sum();
    Stats {
        settled,
        hit_rate: wins as f64 / settled as f64,
        total_pnl,
        roi: if total_stake > 0.0 { total_pnl / total_stake } else { 0.0 },
    }
}
```

在 `main.rs` 加:`mod ledger;`

- [ ] **Step 4: 运行确认通过**

Run: `cd backend && cargo test ledger`
Expected: PASS(4 passed)。

- [ ] **Step 5: Commit**

```bash
git add backend/src/ledger.rs backend/src/main.rs
git commit -m "feat: ledger pure math (settle + stats)"
```

---

## Task 5: 账本存储(ledger.rs — SQLite)

**Files:**
- Modify: `backend/src/ledger.rs`(追加 `Store` 及其测试)

- [ ] **Step 1: 写失败测试**

在 `ledger.rs` 的 `tests` 模块追加:

```rust
    use crate::domain::BetStatus;

    fn mem_store() -> Store { Store::open(":memory:").unwrap() }

    #[test]
    fn insert_and_list_bet() {
        let s = mem_store();
        let id = s.insert_bet("周日001", Outcome::Home, 100.0, 2.1, "2026-06-16T10:00:00").unwrap();
        let bets = s.list_bets(None).unwrap();
        assert_eq!(bets.len(), 1);
        assert_eq!(bets[0].id, id);
        assert!(matches!(bets[0].status, BetStatus::Pending));
    }

    #[test]
    fn settle_updates_status_and_stats() {
        let s = mem_store();
        let id = s.insert_bet("周日001", Outcome::Home, 100.0, 2.1, "2026-06-16T10:00:00").unwrap();
        let st = s.settle(id, Outcome::Home, "2026-06-21T10:00:00").unwrap();
        assert!((st.pnl - 110.0).abs() < 1e-9);
        let stats = s.stats().unwrap();
        assert_eq!(stats.settled, 1);
        assert!((stats.total_pnl - 110.0).abs() < 1e-9);
        let won = s.list_bets(Some(BetStatus::Won)).unwrap();
        assert_eq!(won.len(), 1);
    }
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cd backend && cargo test ledger`
Expected: FAIL(`cannot find Store`)。

- [ ] **Step 3: 实现 Store**

在 `ledger.rs`(`tests` 模块之前)追加。注意 `Outcome`/`BetStatus` 与文本互转的辅助:

```rust
use crate::domain::{Bet, BetStatus, Settlement};
use rusqlite::Connection;
use std::sync::Mutex;

pub struct Store { conn: Mutex<Connection> }

fn outcome_str(o: Outcome) -> &'static str {
    match o { Outcome::Home => "Home", Outcome::Draw => "Draw", Outcome::Away => "Away" }
}
fn outcome_from(s: &str) -> Outcome {
    match s { "Home" => Outcome::Home, "Draw" => Outcome::Draw, _ => Outcome::Away }
}
fn status_str(s: BetStatus) -> &'static str {
    match s { BetStatus::Pending => "Pending", BetStatus::Won => "Won", BetStatus::Lost => "Lost" }
}
fn status_from(s: &str) -> BetStatus {
    match s { "Won" => BetStatus::Won, "Lost" => BetStatus::Lost, _ => BetStatus::Pending }
}

impl Store {
    pub fn open(path: &str) -> rusqlite::Result<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS bets(
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                match_id TEXT NOT NULL,
                pick TEXT NOT NULL,
                stake REAL NOT NULL,
                odds_at_bet REAL NOT NULL,
                status TEXT NOT NULL DEFAULT 'Pending',
                created_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS settlements(
                bet_id INTEGER PRIMARY KEY REFERENCES bets(id),
                actual_result TEXT NOT NULL,
                payout REAL NOT NULL,
                pnl REAL NOT NULL,
                settled_at TEXT NOT NULL
            );",
        )?;
        Ok(Store { conn: Mutex::new(conn) })
    }

    pub fn insert_bet(&self, match_id: &str, pick: Outcome, stake: f64, odds: f64, created_at: &str)
        -> rusqlite::Result<i64> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO bets(match_id,pick,stake,odds_at_bet,status,created_at)
             VALUES(?1,?2,?3,?4,'Pending',?5)",
            rusqlite::params![match_id, outcome_str(pick), stake, odds, created_at],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn list_bets(&self, filter: Option<BetStatus>) -> rusqlite::Result<Vec<Bet>> {
        let conn = self.conn.lock().unwrap();
        let (sql, want) = match filter {
            Some(s) => ("SELECT id,match_id,pick,stake,odds_at_bet,status,created_at FROM bets WHERE status=?1 ORDER BY id DESC", Some(status_str(s))),
            None => ("SELECT id,match_id,pick,stake,odds_at_bet,status,created_at FROM bets ORDER BY id DESC", None),
        };
        let mut stmt = conn.prepare(sql)?;
        let map = |row: &rusqlite::Row| Ok(Bet {
            id: row.get(0)?, match_id: row.get(1)?,
            pick: outcome_from(&row.get::<_, String>(2)?),
            stake: row.get(3)?, odds_at_bet: row.get(4)?,
            status: status_from(&row.get::<_, String>(5)?),
            created_at: row.get(6)?,
        });
        let rows = match want {
            Some(w) => stmt.query_map([w], map)?.collect::<Result<Vec<_>,_>>()?,
            None => stmt.query_map([], map)?.collect::<Result<Vec<_>,_>>()?,
        };
        Ok(rows)
    }

    pub fn settle(&self, bet_id: i64, actual: Outcome, settled_at: &str) -> rusqlite::Result<Settlement> {
        let conn = self.conn.lock().unwrap();
        let (pick_s, stake, odds): (String, f64, f64) = conn.query_row(
            "SELECT pick,stake,odds_at_bet FROM bets WHERE id=?1",
            [bet_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        let pick = outcome_from(&pick_s);
        let (payout, pnl) = settle_amounts(pick, actual, stake, odds);
        let new_status = if pick == actual { BetStatus::Won } else { BetStatus::Lost };
        conn.execute("UPDATE bets SET status=?1 WHERE id=?2",
            rusqlite::params![status_str(new_status), bet_id])?;
        conn.execute(
            "INSERT OR REPLACE INTO settlements(bet_id,actual_result,payout,pnl,settled_at)
             VALUES(?1,?2,?3,?4,?5)",
            rusqlite::params![bet_id, outcome_str(actual), payout, pnl, settled_at],
        )?;
        Ok(Settlement { bet_id, actual_result: actual, payout, pnl, settled_at: settled_at.into() })
    }

    pub fn stats(&self) -> rusqlite::Result<Stats> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT b.stake, s.pnl, b.status FROM settlements s JOIN bets b ON b.id=s.bet_id")?;
        let rows = stmt.query_map([], |r| Ok(SettledRow {
            stake: r.get(0)?, pnl: r.get(1)?,
            won: r.get::<_, String>(2)? == "Won",
        }))?.collect::<Result<Vec<_>,_>>()?;
        Ok(compute_stats(&rows))
    }
}
```

- [ ] **Step 4: 运行确认通过**

Run: `cd backend && cargo test ledger`
Expected: PASS(6 passed)。

- [ ] **Step 5: Commit**

```bash
git add backend/src/ledger.rs
git commit -m "feat: ledger sqlite store (insert/list/settle/stats)"
```

## Task 6: 预测引擎纯函数(predictor.rs — 构造与解析)

把"组请求体""从响应抽文本""解析校验预测 JSON"做成纯函数,便于测试;真实 HTTP 调用在 Task 7。

**Files:**
- Create: `backend/src/predictor.rs`
- Modify: `backend/src/main.rs`(加 `mod predictor;`)

- [ ] **Step 1: 写失败测试**

```rust
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
```

- [ ] **Step 2: 运行确认失败**

Run: `cd backend && cargo test predictor`
Expected: FAIL(`cannot find ApiConfig`)。

- [ ] **Step 3: 实现纯函数**

```rust
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
```

`Probs`/`Outcome` 已 `Deserialize`(Task 2)。在 `main.rs` 加 `mod predictor;`。

- [ ] **Step 4: 运行确认通过**

Run: `cd backend && cargo test predictor`
Expected: PASS(7 passed)。

- [ ] **Step 5: Commit**

```bash
git add backend/src/predictor.rs backend/src/main.rs
git commit -m "feat: predictor body build + response parse + validation"
```

---

## Task 7: 预测引擎 HTTP 调用 + 重试

**Files:**
- Modify: `backend/src/predictor.rs`(追加 async `call_ai` 与 `predict` 重试封装)

- [ ] **Step 1: 写实现(无单测,HTTP 用集成手测)**

在 `predictor.rs` 追加:

```rust
pub async fn call_ai(cfg: &ApiConfig, m: &Match) -> Result<Prediction, PredictError> {
    let body = build_body(cfg, m);
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
    parse_prediction(&m.id, &cfg.model, &text)
}

/// 失败重试一次
pub async fn predict(cfg: &ApiConfig, m: &Match) -> Result<Prediction, PredictError> {
    match call_ai(cfg, m).await {
        Ok(p) => Ok(p),
        Err(_) => call_ai(cfg, m).await,
    }
}
```

- [ ] **Step 2: 验证编译**

Run: `cd backend && cargo build`
Expected: 编译成功。

- [ ] **Step 3: Commit**

```bash
git add backend/src/predictor.rs
git commit -m "feat: predictor async http call with one retry"
```

## Task 8: AI 配置存储(config.rs)

把 `ApiConfig` 读写到本地文件(明文,仅本机)。空文件/不存在返回 `None`。

**Files:**
- Create: `backend/src/config.rs`
- Modify: `backend/src/main.rs`(加 `mod config;`)

- [ ] **Step 1: 写失败测试**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::predictor::{ApiConfig, ApiProtocol};

    #[test]
    fn save_then_load_roundtrip() {
        let dir = std::env::temp_dir().join(format!("wcbp-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.local.json");
        let p = path.to_str().unwrap();

        assert!(load(p).unwrap().is_none());

        let cfg = ApiConfig { base_url: "https://api.anthropic.com".into(),
            api_key: "sk-x".into(), model: "claude-fable-5".into(),
            protocol: ApiProtocol::Anthropic };
        save(p, &cfg).unwrap();

        let got = load(p).unwrap().unwrap();
        assert_eq!(got.model, "claude-fable-5");
        assert!(matches!(got.protocol, ApiProtocol::Anthropic));
    }
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cd backend && cargo test config`
Expected: FAIL(`cannot find load`)。

- [ ] **Step 3: 实现**

```rust
use crate::predictor::ApiConfig;
use std::path::Path;

pub fn load(path: &str) -> std::io::Result<Option<ApiConfig>> {
    if !Path::new(path).exists() {
        return Ok(None);
    }
    let s = std::fs::read_to_string(path)?;
    if s.trim().is_empty() {
        return Ok(None);
    }
    let cfg = serde_json::from_str(&s)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    Ok(Some(cfg))
}

pub fn save(path: &str, cfg: &ApiConfig) -> std::io::Result<()> {
    let s = serde_json::to_string_pretty(cfg)?;
    std::fs::write(path, s)
}

pub fn default_path() -> String {
    "config.local.json".to_string()
}
```

在 `main.rs` 加 `mod config;`。

- [ ] **Step 4: 运行确认通过**

Run: `cd backend && cargo test config`
Expected: PASS(1 passed)。

- [ ] **Step 5: Commit**

```bash
git add backend/src/config.rs backend/src/main.rs
git commit -m "feat: ai config local storage"
```

---

## Task 9: API 状态与路由(api.rs)

定义共享状态(`Store` + 内存 `ApiConfig`)与七个 handler。配置只在 `GET /api/config` 返回时**抹掉 api_key**。

**Files:**
- Create: `backend/src/api.rs`
- Modify: `backend/src/main.rs`(加 `mod api;`)

- [ ] **Step 1: 写实现(handler 逻辑;集成测试在 Task 11)**

```rust
use crate::config;
use crate::domain::{Match, Outcome};
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
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/matches", post(parse_matches))
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
```

在 `main.rs` 加 `mod api;`。

- [ ] **Step 2: 验证编译**

Run: `cd backend && cargo build`
Expected: 编译成功。

- [ ] **Step 3: Commit**

```bash
git add backend/src/api.rs backend/src/main.rs
git commit -m "feat: axum api routes + state"
```

## Task 10: 启动装配(main.rs) + 静态服务

**Files:**
- Modify: `backend/src/main.rs`

- [ ] **Step 1: 写完整 main.rs**

```rust
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
```

> 注:`nest_service("/")` 提供前端静态资源,API 路由优先匹配 `/api/*`。CORS permissive 仅本地自用。

- [ ] **Step 2: 验证编译**

Run: `cd backend && cargo build`
Expected: 编译成功(`frontend/dist` 不存在不影响编译,运行时才读)。

- [ ] **Step 3: Commit**

```bash
git add backend/src/main.rs
git commit -m "feat: main bootstrap + static serving"
```

---

## Task 11: API 集成测试(mock predictor 路径)

用 axum 的 `oneshot` 测全链路:matches → bets → settle → stats(predict 走真实 HTTP,集成测试只覆盖不依赖外部 AI 的端点)。

**Files:**
- Create: `backend/tests/api_flow.rs`
- Modify: `backend/Cargo.toml`(加 `[dev-dependencies]` 的 `tower`、`http-body-util`、`mime`)

- [ ] **Step 1: 加测试依赖**

在 `Cargo.toml` 的 `[dev-dependencies]` 追加:

```toml
tower = { version = "0.4", features = ["util"] }
http-body-util = "0.1"
```

并把 `backend/src/main.rs` 的模块**也暴露为 lib** 以便测试引用:新增 `backend/src/lib.rs`:

```rust
pub mod domain;
pub mod source;
pub mod ledger;
pub mod predictor;
pub mod config;
pub mod api;
```

`Cargo.toml` 增加:

```toml
[lib]
name = "wc_bet_predictor"
path = "src/lib.rs"
```

- [ ] **Step 2: 写集成测试**

```rust
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
```

> 注:`main.rs` 保留 `mod ...;` 不变(二进制入口),`lib.rs` 提供库视图供测试用。两者并存是 cargo 惯例。

- [ ] **Step 3: 运行确认通过**

Run: `cd backend && cargo test --test api_flow`
Expected: PASS(2 passed)。

- [ ] **Step 4: Commit**

```bash
git add backend/Cargo.toml backend/src/lib.rs backend/tests/api_flow.rs
git commit -m "test: api integration flow (matches/bets/settle/stats)"
```

## Task 12: 前端脚手架(Bun + Vite + TS)

**Files:**
- Create: `frontend/package.json`
- Create: `frontend/index.html`
- Create: `frontend/vite.config.ts`
- Create: `frontend/src/api.ts`

- [ ] **Step 1: package.json**

```json
{
  "name": "wc-bet-frontend",
  "private": true,
  "type": "module",
  "scripts": {
    "dev": "vite",
    "build": "vite build"
  },
  "devDependencies": {
    "vite": "^5.4.0",
    "typescript": "^5.5.0"
  }
}
```

- [ ] **Step 2: vite.config.ts(dev 代理到后端)**

```ts
import { defineConfig } from "vite";

export default defineConfig({
  server: {
    proxy: { "/api": "http://127.0.0.1:8787" },
  },
  build: { outDir: "dist" },
});
```

- [ ] **Step 3: index.html(四区块骨架)**

```html
<!doctype html>
<html lang="zh">
<head>
  <meta charset="UTF-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1.0" />
  <title>竞彩预测 · 模拟下单</title>
  <link rel="stylesheet" href="/src/style.css" />
</head>
<body>
  <header><h1>竞彩足球预测 · 模拟账本</h1>
    <p class="note">金额均为虚拟,仅用于复盘。不涉及真实投注。</p></header>
  <main>
    <section id="config-section"><h2>① AI 配置</h2><div id="config-form"></div></section>
    <section id="matches-section"><h2>② 赛事</h2>
      <textarea id="paste" placeholder="粘贴 JSON 数组,或每行:场次|联赛|主|客|开赛|主赔|平赔|客赔[|让球]"></textarea>
      <button id="parse-btn">解析赛事</button>
      <div id="match-list"></div></section>
    <section id="prediction-section"><h2>③ 预测结果</h2><div id="prediction-view"></div></section>
    <section id="ledger-section"><h2>④ 账本</h2><div id="stats-view"></div><div id="bet-list"></div></section>
  </main>
  <script type="module" src="/src/main.ts"></script>
</body>
</html>
```

- [ ] **Step 4: api.ts(REST 封装)**

```ts
export type Outcome = "Home" | "Draw" | "Away";
export interface Odds { home: number; draw: number; away: number; }
export interface Match {
  id: string; league: string; home: string; away: string;
  kickoff: string; odds: Odds; handicap: number | null;
}
export interface Prediction {
  match_id: string; probs: Odds; pick: Outcome;
  confidence: number; rationale: string; model: string; created_at: string;
}
export interface Bet {
  id: number; match_id: string; pick: Outcome;
  stake: number; odds_at_bet: number; status: string; created_at: string;
}
export interface Stats { settled: number; hit_rate: number; total_pnl: number; roi: number; }

async function req<T>(url: string, opts?: RequestInit): Promise<T> {
  const r = await fetch(url, { headers: { "content-type": "application/json" }, ...opts });
  const data = await r.json();
  if (!r.ok) throw new Error(data.error ?? `HTTP ${r.status}`);
  return data as T;
}

export const api = {
  parseMatches: (raw: string) => req<Match[]>("/api/matches", { method: "POST", body: JSON.stringify({ raw }) }),
  predict: (m: Match) => req<Prediction>("/api/predict", { method: "POST", body: JSON.stringify(m) }),
  placeBet: (b: { match_id: string; pick: Outcome; stake: number; odds: number }) =>
    req<{ id: number }>("/api/bets", { method: "POST", body: JSON.stringify(b) }),
  listBets: (status?: string) =>
    req<Bet[]>(`/api/bets${status ? `?status=${status}` : ""}`),
  settle: (bet_id: number, actual_result: Outcome) =>
    req<any>("/api/settle", { method: "POST", body: JSON.stringify({ bet_id, actual_result }) }),
  stats: () => req<Stats>("/api/stats"),
  getConfig: () => req<any>("/api/config"),
  setConfig: (c: any) => req<any>("/api/config", { method: "POST", body: JSON.stringify(c) }),
};
```

- [ ] **Step 5: 安装依赖并验证构建**

Run: `cd frontend && bun install && bun run build`
Expected: `dist/` 生成(此时无 `main.ts`/`style.css` 会报错 → 先放空文件占位,见 Task 13 补全)。临时:`touch src/main.ts src/style.css` 再 build 通过。

- [ ] **Step 6: Commit**

```bash
git add frontend/package.json frontend/index.html frontend/vite.config.ts frontend/src/api.ts
git commit -m "chore: frontend scaffold + api client"
```

---

## Task 13: 前端交互(main.ts + style.css)

把四区块接起来:配置读写、解析赛事并渲染卡片、预测、下注、列表与统计、结算。

**Files:**
- Create: `frontend/src/main.ts`
- Create: `frontend/src/style.css`

- [ ] **Step 1: style.css(克制的卡片式样)**

```css
:root { --bg:#0f1419; --surface:#1a2230; --accent:#3b82f6; --text:#e5e9f0; --muted:#8b95a7; }
* { box-sizing: border-box; }
body { margin:0; font-family: system-ui, sans-serif; background:var(--bg); color:var(--text); }
header { padding:1.5rem 2rem; border-bottom:1px solid #2a3340; }
header h1 { margin:0; font-size:1.4rem; }
.note { color:var(--muted); font-size:.85rem; margin:.3rem 0 0; }
main { display:grid; gap:1.25rem; padding:1.5rem 2rem; max-width:960px; margin:0 auto; }
section { background:var(--surface); border:1px solid #2a3340; border-radius:12px; padding:1.25rem; }
section h2 { margin:0 0 1rem; font-size:1rem; color:var(--muted); }
textarea { width:100%; min-height:110px; background:#0f1419; color:var(--text);
  border:1px solid #2a3340; border-radius:8px; padding:.6rem; font-family:monospace; }
button { background:var(--accent); color:#fff; border:0; border-radius:8px;
  padding:.5rem .9rem; cursor:pointer; font-size:.9rem; }
button:hover { filter:brightness(1.1); }
button:disabled { opacity:.5; cursor:not-allowed; }
input, select { background:#0f1419; color:var(--text); border:1px solid #2a3340;
  border-radius:6px; padding:.4rem; margin:.2rem .4rem .2rem 0; }
.card { background:#0f1419; border:1px solid #2a3340; border-radius:8px;
  padding:.8rem; margin:.6rem 0; }
.prob-bar { display:flex; height:22px; border-radius:5px; overflow:hidden; margin:.5rem 0; }
.prob-bar span { display:flex; align-items:center; justify-content:center; font-size:.75rem; }
.win { color:#34d399; } .loss { color:#f87171; } .muted { color:var(--muted); }
table { width:100%; border-collapse:collapse; font-size:.85rem; }
th,td { text-align:left; padding:.4rem; border-bottom:1px solid #2a3340; }
```

- [ ] **Step 2: main.ts**

```ts
import { api, Match, Prediction, Outcome } from "./api";

const $ = (id: string) => document.getElementById(id)!;
let matches: Match[] = [];

// ---- 配置区 ----
async function renderConfig() {
  const cfg = await api.getConfig().catch(() => ({}));
  $("config-form").innerHTML = `
    <input id="cfg-url" placeholder="Base URL" value="${cfg.base_url ?? ""}" size="32"/>
    <input id="cfg-key" placeholder="API Key${cfg.has_key ? "(已保存)" : ""}" type="password" size="24"/>
    <input id="cfg-model" placeholder="Model" value="${cfg.model ?? ""}" size="18"/>
    <select id="cfg-proto">
      <option value="Anthropic" ${cfg.protocol === "Anthropic" ? "selected" : ""}>Claude</option>
      <option value="OpenAI" ${cfg.protocol === "OpenAI" ? "selected" : ""}>OpenAI</option>
    </select>
    <button id="cfg-save">保存</button>`;
  $("cfg-save").onclick = async () => {
    await api.setConfig({
      base_url: (<HTMLInputElement>$("cfg-url")).value,
      api_key: (<HTMLInputElement>$("cfg-key")).value,
      model: (<HTMLInputElement>$("cfg-model")).value,
      protocol: (<HTMLSelectElement>$("cfg-proto")).value,
    });
    alert("配置已保存");
    renderConfig();
  };
}

// ---- 赛事区 ----
$("parse-btn").onclick = async () => {
  try {
    matches = await api.parseMatches((<HTMLTextAreaElement>$("paste")).value);
    $("match-list").innerHTML = matches.map((m, i) => `
      <div class="card">
        <strong>${m.id}</strong> ${m.league} — ${m.home} vs ${m.away}
        <div class="muted">开赛 ${m.kickoff} · 赔率 ${m.odds.home}/${m.odds.draw}/${m.odds.away}</div>
        <button data-i="${i}" class="predict-btn">预测</button>
      </div>`).join("");
    document.querySelectorAll<HTMLButtonElement>(".predict-btn").forEach(b =>
      b.onclick = () => doPredict(matches[+b.dataset.i!]));
  } catch (e: any) { $("match-list").innerHTML = `<p class="loss">${e.message}</p>`; }
};

// ---- 预测区 ----
async function doPredict(m: Match) {
  $("prediction-view").innerHTML = `<p class="muted">预测中…</p>`;
  try {
    const p: Prediction = await api.predict(m);
    const pct = (n: number) => Math.round(n * 100);
    $("prediction-view").innerHTML = `
      <div class="card">
        <strong>${m.home} vs ${m.away}</strong> — 推荐 <b>${zh(p.pick)}</b>(置信 ${pct(p.confidence)}%)
        <div class="prob-bar">
          <span style="width:${pct(p.probs.home)}%;background:#3b82f6">主 ${pct(p.probs.home)}%</span>
          <span style="width:${pct(p.probs.draw)}%;background:#6b7280">平 ${pct(p.probs.draw)}%</span>
          <span style="width:${pct(p.probs.away)}%;background:#ef4444">客 ${pct(p.probs.away)}%</span>
        </div>
        <p>${p.rationale}</p>
        金额 <input id="stake" type="number" value="100" size="6"/>
        赔率 <input id="odds" type="number" value="${oddsFor(m, p.pick)}" size="6" step="0.01"/>
        <button id="bet-btn">记一笔(虚拟)</button>
      </div>`;
    $("bet-btn").onclick = async () => {
      await api.placeBet({ match_id: m.id, pick: p.pick,
        stake: +(<HTMLInputElement>$("stake")).value,
        odds: +(<HTMLInputElement>$("odds")).value });
      refreshLedger();
    };
  } catch (e: any) { $("prediction-view").innerHTML = `<p class="loss">${e.message}</p>`; }
}

function oddsFor(m: Match, pick: Outcome): number {
  return pick === "Home" ? m.odds.home : pick === "Draw" ? m.odds.draw : m.odds.away;
}
function zh(o: Outcome) { return o === "Home" ? "主胜" : o === "Draw" ? "平局" : "客胜"; }

// ---- 账本区 ----
async function refreshLedger() {
  const [bets, stats] = await Promise.all([api.listBets(), api.stats()]);
  $("stats-view").innerHTML = `
    <div class="card">已结算 ${stats.settled} 笔 ·
      命中率 <b>${(stats.hit_rate * 100).toFixed(1)}%</b> ·
      累计盈亏 <b class="${stats.total_pnl >= 0 ? "win" : "loss"}">${stats.total_pnl.toFixed(2)}</b> ·
      ROI <b class="${stats.roi >= 0 ? "win" : "loss"}">${(stats.roi * 100).toFixed(1)}%</b></div>`;
  $("bet-list").innerHTML = `<table><tr><th>#</th><th>场次</th><th>推荐</th><th>金额</th><th>赔率</th><th>状态</th><th>结算</th></tr>
    ${bets.map(b => `<tr>
      <td>${b.id}</td><td>${b.match_id}</td><td>${zh(b.pick as Outcome)}</td>
      <td>${b.stake}</td><td>${b.odds_at_bet}</td>
      <td class="${b.status === "Won" ? "win" : b.status === "Lost" ? "loss" : "muted"}">${b.status}</td>
      <td>${b.status === "Pending" ? settleCtl(b.id) : "—"}</td></tr>`).join("")}</table>`;
  bets.filter(b => b.status === "Pending").forEach(b => {
    $("settle-" + b.id).onclick = async () => {
      const sel = (<HTMLSelectElement>$("res-" + b.id)).value as Outcome;
      await api.settle(b.id, sel); refreshLedger();
    };
  });
}
function settleCtl(id: number) {
  return `<select id="res-${id}"><option value="Home">主胜</option>
    <option value="Draw">平局</option><option value="Away">客胜</option></select>
    <button id="settle-${id}">结算</button>`;
}

renderConfig();
refreshLedger();
```

- [ ] **Step 3: 验证构建**

Run: `cd frontend && bun run build`
Expected: `dist/index.html` 等产物生成,无 TS 错误。

- [ ] **Step 4: Commit**

```bash
git add frontend/src/main.ts frontend/src/style.css
git commit -m "feat: frontend interactions (config/matches/predict/ledger)"
```

## Task 14: 端到端手测 + README

**Files:**
- Create: `README.md`

- [ ] **Step 1: 写 README**

````markdown
# 竞彩足球预测 · 模拟账本

本地自用工具:粘贴赛事数据 → 大模型(Claude/OpenAI)预测胜平负 → 虚拟账本记账与盈亏统计。

> 金额均为虚拟,仅用于复盘。不自动登录体彩官网、不提交任何真实投注、不绕过任何站点防护。

## 运行

```bash
# 1. 构建前端
cd frontend && bun install && bun run build
# 2. 起后端(同时托管前端 dist)
cd ../backend && cargo run
# 打开 http://127.0.0.1:8787
```

开发模式(前端热更新):`cd frontend && bun run dev`(Vite 代理 /api 到 8787)。

## 数据格式

- JSON 数组:`[{"id":"周日001","league":"世界杯","home":"A","away":"B","kickoff":"2026-06-20T19:00:00","odds":{"home":2.1,"draw":3.2,"away":3.5},"handicap":null}]`
- 或每行:`场次|联赛|主|客|开赛|主赔|平赔|客赔[|让球]`

## 配置

首屏「AI 配置」填 Base URL / API Key / Model / 协议,保存后写入 `backend/config.local.json`(已在 .gitignore,仅本机明文)。
````

- [ ] **Step 2: 全量测试**

Run: `cd backend && cargo test`
Expected: 所有单测 + 集成测试 PASS。

- [ ] **Step 3: 端到端冒烟(手动)**

```bash
cd frontend && bun run build && cd ../backend && cargo run &
sleep 2
# 解析赛事
curl -s -X POST localhost:8787/api/matches -H 'content-type: application/json' \
  -d '{"raw":"周日001|世界杯|A|B|2026-06-20T19:00:00|2.1|3.2|3.5"}'
# 下注 → 结算 → 统计
curl -s -X POST localhost:8787/api/bets -H 'content-type: application/json' \
  -d '{"match_id":"周日001","pick":"Home","stake":100,"odds":2.1}'
curl -s -X POST localhost:8787/api/settle -H 'content-type: application/json' \
  -d '{"bet_id":1,"actual_result":"Home"}'
curl -s localhost:8787/api/stats
```
Expected: stats 返回 `settled=1, total_pnl=110.0`。手测完 `kill %1`。

- [ ] **Step 4: Commit**

```bash
git add README.md
git commit -m "docs: README + run instructions"
```

---

## 完成标准

- [ ] `cargo test` 全绿(domain/source/ledger/predictor/config 单测 + api_flow 集成)
- [ ] `bun run build` 产出 `frontend/dist`
- [ ] `cargo run` 起服务,浏览器四区块可用:配置 → 粘贴解析 → 预测 → 记一笔 → 结算 → 统计刷新
- [ ] `config.local.json`、`*.db` 不入库(.gitignore 已含)
- [ ] 全程不含任何抓取官网/绕过 WAF 的代码







