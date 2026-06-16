# 世界杯体彩预测 + 模拟下单 — 设计文档

日期:2026-06-16
状态:已确认,待实现计划

## 1. 目标与范围

构建一个**本地自用**的工具:输入竞彩足球赛事数据,调用大模型(Claude / OpenAI 兼容)给出胜平负预测与中文理由,并把推荐方案记录成**虚拟投注账本**,用于跟踪命中率与盈亏复盘。

**明确包含**:
- 赛事数据输入(可插拔,默认手动粘贴)
- 大模型预测(胜平负概率 + 推荐 + 中文理由)
- 虚拟投注账本(下单 / 结算 / 统计)
- 本地 Web 界面

**明确不包含(边界)**:
- 不自动登录体彩官网、不提交任何真实投注订单
- 不使用反检测浏览器或任何手段绕过官网 WAF / 反爬防护
- 账本中的金额均为虚拟金额,仅用于复盘,不涉及真实资金

## 2. 技术栈

- 后端:Rust(`axum` HTTP 服务 + `rusqlite` / SQLite 账本持久化)
- 前端:Bun(构建 + 本地静态服务)
- 运行:`cargo run` 起后端,Bun 产出前端静态资源,纯本机使用

## 3. 数据流

```
粘贴赛事数据 → 解析成 Match → 大模型预测(后端代发) → Prediction
                                        ↓
                          你确认 → 写一笔虚拟 Bet 进 SQLite
                                        ↓
            赛后填入真实赛果 → Settlement(算赔付/盈亏) → 命中率 & 累计盈亏 & ROI 统计
```

数据流是一条直线,便于调试与测试。

## 4. 数据模型

```rust
struct Match {
    id: String,            // 官网场次编号,如 "周日001"
    league: String,        // 赛事/联赛
    home: String, away: String,
    kickoff: DateTime,
    odds: Odds,            // { home, draw, away } 胜平负赔率
    handicap: Option<i32>, // 让球数,可空(竞彩让球盘)
}

struct Prediction {
    match_id: String,
    probs: Probs,          // 模型给的 {胜, 平, 负} 概率,和为 1
    pick: Outcome,         // Home | Draw | Away
    confidence: f32,
    rationale: String,     // 模型给的中文推理
    model: String, created_at: DateTime,
}

struct Bet {
    id: i64, match_id: String,
    pick: Outcome, stake: f64,  // 虚拟金额
    odds_at_bet: f64,           // 下单时锁定的赔率
    status: BetStatus,          // Pending | Won | Lost
    created_at: DateTime,
}

struct Settlement {
    bet_id: i64,
    actual_result: Outcome,
    payout: f64,                // 命中则 stake * odds_at_bet
    pnl: f64,                   // payout - stake
    settled_at: DateTime,
}

enum Outcome { Home, Draw, Away }
enum BetStatus { Pending, Won, Lost }
```

## 5. 数据输入抽象(取数隔离)

取数方式被隔离在一个 trait 后,默认实现为手动粘贴。未来若接入其他**正当**数据源,只需新增实现,不影响其他模块。

```rust
trait MatchSource {
    fn load(&self) -> Result<Vec<Match>, SourceError>;
}
struct PasteSource { raw: String }  // 默认:解析粘贴的 JSON / 文本
```

## 6. 预测引擎

- 前端填写 AI 配置:Base URL、API Key、Model、协议(Claude / OpenAI)。
- 后端代发请求(避开浏览器 CORS,Key 不暴露在页面网络请求中)。
- 强制模型返回严格 JSON(system 提示给定 schema:`probs{home,draw,away}` 和为 1、`pick`、`confidence`、`rationale` 中文)。
- 解析校验:概率和不为 1 或字段缺失 → 判失败,重试一次。

```rust
enum ApiProtocol { Anthropic, OpenAI }

struct ApiConfig {
    base_url: String,
    api_key: String,
    model: String,
    protocol: ApiProtocol,
}

trait Predictor {
    fn predict(&self, m: &Match) -> Result<Prediction, PredictError>;
}
// Anthropic: POST /v1/messages,header `x-api-key` + `anthropic-version`,body {model, max_tokens, system, messages}
// OpenAI:    POST /chat/completions,header `Authorization: Bearer`,body {model, messages}
```

API Key 存储:后端内存 + 可选写入本地配置文件(明文,仅本机可读)。不入库、不上传。

## 7. 账本算法(纯函数)

```
下单:  Bet{ pick, stake, odds_at_bet=当前赔率, status=Pending }
结算:  命中(pick==actual) → payout = stake * odds_at_bet, pnl = payout - stake
        未命中            → payout = 0,                  pnl = -stake
统计:  命中率   = Won 笔数 / 已结算笔数
        累计盈亏 = Σ pnl
        ROI      = Σ pnl / Σ stake
```

## 8. 后端接口(axum)

```
POST /api/matches    粘贴文本/JSON → 解析返回 Match[]
POST /api/predict    {match} → Prediction(后端代发 AI 请求)
POST /api/bets       下一笔虚拟注
GET  /api/bets       列出注单(可按状态过滤)
POST /api/settle     {bet_id, actual_result} → Settlement
GET  /api/stats      命中率 / 累计盈亏 / ROI
GET/POST /api/config AI 配置读写(Key 存本地配置文件)
```

## 9. 前端(Bun,4 区块)

1. 配置区 — 填 Base URL / Key / Model / 协议
2. 赛事区 — 粘贴框 + 解析出的赛事卡片,每张卡有「预测」按钮
3. 预测结果 — 概率条、推荐、模型中文理由、「记一笔」按钮
4. 账本区 — 注单列表 + 命中率/累计盈亏/ROI 图表 + 赛后填赛果结算

## 10. 错误处理

- 边界校验:粘贴数据解析失败给出明确字段/行号提示;AI 配置缺失时禁用预测按钮。
- AI 调用兜底:超时 / 非 2xx / JSON 不合法 → 重试一次,仍失败把原始返回回显,不静默吞。
- DB 错误:显式返回,前端给出可读提示。

## 11. 测试

- Rust 单测:解析器、账本赔付/盈亏/ROI 数学、双格式(Anthropic/OpenAI)请求体构造。
- API 集成测试:mock predictor,覆盖 matches → predict → bets → settle → stats 全链路。
- 前端:关键交互冒烟测试。
- 目标覆盖率 80%+(核心纯函数与解析逻辑优先)。

## 12. 模块边界小结

| 模块 | 职责 | 依赖 |
|------|------|------|
| `source` | 取数(默认粘贴解析) | — |
| `predictor` | 调 AI、解析校验预测 | `ApiConfig` |
| `ledger` | 虚拟下单/结算/统计 | SQLite |
| `api` | REST 编排 | 以上三者 |
| `web`(Bun) | 界面 | `api` |
