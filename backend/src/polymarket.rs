use crate::domain::{Match, Odds};
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;

const GAMMA_URL: &str =
    "https://gamma-api.polymarket.com/events?closed=false&limit=100&tag_slug=world-cup";

#[derive(Debug, Clone, Serialize)]
pub struct DateCount { pub date: String, pub count: usize }

async fn fetch_events() -> Result<Value, String> {
    let client = reqwest::Client::new();
    let resp = client
        .get(GAMMA_URL)
        .header("user-agent", "wc-bet-predictor")
        .send().await
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("Polymarket HTTP {}", resp.status()));
    }
    resp.json().await.map_err(|e| e.to_string())
}

/// 拉取并映射为 Match;date=Some 时只保留该解算日,limit 截断数量。
pub async fn fetch_matches(date: Option<&str>, limit: usize) -> Result<Vec<Match>, String> {
    let events = fetch_events().await?;
    let mut ms = map_events(&events);
    if let Some(d) = date {
        ms.retain(|m| m.kickoff == d);
    }
    ms.truncate(limit);
    Ok(ms)
}

/// 拉取并返回每个解算日的可下注市场数量(升序日期)。
pub async fn fetch_dates() -> Result<Vec<DateCount>, String> {
    let events = fetch_events().await?;
    Ok(available_dates(&map_events(&events)))
}

fn pick_date(e: &Value) -> String {
    let s = e["endDate"].as_str()
        .or_else(|| e["startDate"].as_str())
        .unwrap_or("");
    s.chars().take(10).collect()
}

/// 纯函数:把 Polymarket events JSON 映射为 Match 列表。
/// kickoff = 解算日(endDate 截断为 YYYY-MM-DD,回退 startDate)。
/// 仅保留 2-outcome 市场;价格转十进制赔率(1/price);无平局盘 draw=0。
pub fn map_events(events: &Value) -> Vec<Match> {
    let mut out = Vec::new();
    let Some(arr) = events.as_array() else { return out; };
    for e in arr {
        let date = pick_date(e);
        let Some(markets) = e["markets"].as_array() else { continue; };
        for m in markets {
            if let Some(mt) = map_market(m, &date) {
                out.push(mt);
            }
        }
    }
    out
}

/// 按 kickoff(解算日)聚合计数,日期升序。
pub fn available_dates(ms: &[Match]) -> Vec<DateCount> {
    let mut map: BTreeMap<String, usize> = BTreeMap::new();
    for m in ms {
        if !m.kickoff.is_empty() {
            *map.entry(m.kickoff.clone()).or_insert(0) += 1;
        }
    }
    map.into_iter().map(|(date, count)| DateCount { date, count }).collect()
}

fn parse_str_array(v: &Value) -> Vec<String> {
    // 字段是 JSON 编码的字符串,如 "[\"Yes\", \"No\"]"
    match v.as_str() {
        Some(s) => serde_json::from_str::<Vec<String>>(s).unwrap_or_default(),
        None => Vec::new(),
    }
}

fn map_market(m: &Value, date: &str) -> Option<Match> {
    let outcomes = parse_str_array(&m["outcomes"]);
    let prices = parse_str_array(&m["outcomePrices"]);
    if outcomes.len() != 2 || prices.len() != 2 {
        return None; // 只处理两选项市场
    }
    let p0: f64 = prices[0].parse().ok()?;
    let p1: f64 = prices[1].parse().ok()?;
    if p0 <= 0.0 || p1 <= 0.0 {
        return None; // 避免除零/无穷赔率
    }
    let id = m["id"].as_str().unwrap_or("").to_string();
    let label = m["question"].as_str().unwrap_or("Polymarket").to_string();
    Some(Match {
        id,
        league: label,
        home: outcomes[0].clone(),
        away: outcomes[1].clone(),
        kickoff: date.to_string(),
        odds: Odds { home: 1.0 / p0, draw: 0.0, away: 1.0 / p1 },
        handicap: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_two_way_market_with_resolution_date() {
        let events = serde_json::json!([{
            "title": "Will Mexico win Group A in the 2026 FIFA World Cup?",
            "startDate": "2025-12-06T00:00:00Z",
            "endDate": "2026-06-27T00:00:00Z",
            "markets": [{
                "id": "839357",
                "question": "Will Mexico win Group A in the 2026 FIFA World Cup?",
                "outcomes": "[\"Yes\", \"No\"]",
                "outcomePrices": "[\"0.615\", \"0.385\"]"
            }]
        }]);
        let ms = map_events(&events);
        assert_eq!(ms.len(), 1);
        let m = &ms[0];
        assert_eq!(m.id, "839357");
        assert_eq!(m.home, "Yes");
        assert_eq!(m.away, "No");
        assert_eq!(m.kickoff, "2026-06-27"); // resolution date, date-only
        assert!((m.odds.home - 1.0/0.615).abs() < 1e-6);
        assert!((m.odds.away - 1.0/0.385).abs() < 1e-6);
        assert_eq!(m.odds.draw, 0.0);
    }

    #[test]
    fn skips_market_with_zero_price() {
        let events = serde_json::json!([{
            "title": "X", "startDate": "",
            "markets": [{ "id": "1", "question": "X",
                "outcomes": "[\"Yes\", \"No\"]", "outcomePrices": "[\"0\", \"1\"]" }]
        }]);
        assert_eq!(map_events(&events).len(), 0);
    }

    #[test]
    fn skips_non_two_way_market() {
        let events = serde_json::json!([{
            "title": "Group A Winner", "startDate": "",
            "markets": [{ "id": "2", "question": "Group A Winner",
                "outcomes": "[\"Mexico\",\"USA\",\"Canada\"]",
                "outcomePrices": "[\"0.4\",\"0.35\",\"0.25\"]" }]
        }]);
        assert_eq!(map_events(&events).len(), 0);
    }

    #[test]
    fn empty_or_missing_markets_yields_empty() {
        assert_eq!(map_events(&serde_json::json!([])).len(), 0);
        let no_markets = serde_json::json!([{"title":"x","startDate":""}]);
        assert_eq!(map_events(&no_markets).len(), 0);
    }

    #[test]
    fn available_dates_counts_by_resolution_date() {
        let events = serde_json::json!([
            { "endDate":"2026-06-27T00:00:00Z", "markets":[
                {"id":"1","question":"A","outcomes":"[\"Yes\",\"No\"]","outcomePrices":"[\"0.5\",\"0.5\"]"},
                {"id":"2","question":"B","outcomes":"[\"Yes\",\"No\"]","outcomePrices":"[\"0.6\",\"0.4\"]"}
            ]},
            { "endDate":"2026-07-20T00:00:00Z", "markets":[
                {"id":"3","question":"C","outcomes":"[\"Yes\",\"No\"]","outcomePrices":"[\"0.3\",\"0.7\"]"}
            ]}
        ]);
        let ms = map_events(&events);
        let dates = available_dates(&ms);
        assert_eq!(dates.len(), 2);
        assert_eq!(dates[0].date, "2026-06-27"); // BTreeMap → ascending
        assert_eq!(dates[0].count, 2);
        assert_eq!(dates[1].date, "2026-07-20");
        assert_eq!(dates[1].count, 1);
    }
}
