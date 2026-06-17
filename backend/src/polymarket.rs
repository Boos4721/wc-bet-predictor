use crate::domain::{Match, Odds};
use serde_json::Value;

const GAMMA_URL: &str =
    "https://gamma-api.polymarket.com/events?closed=false&limit=100&tag_slug=world-cup";

/// 拉取 Polymarket 世界杯市场并映射为 Match 列表
pub async fn fetch_matches() -> Result<Vec<Match>, String> {
    let client = reqwest::Client::new();
    let resp = client
        .get(GAMMA_URL)
        .header("user-agent", "wc-bet-predictor")
        .send().await
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("Polymarket HTTP {}", resp.status()));
    }
    let events: Value = resp.json().await.map_err(|e| e.to_string())?;
    Ok(map_events(&events))
}

/// 纯函数:把 Polymarket events JSON 映射为 Match 列表。
/// 仅保留 2-outcome 市场;价格转十进制赔率(1/price);无平局盘 draw=0。
pub fn map_events(events: &Value) -> Vec<Match> {
    let mut out = Vec::new();
    let Some(arr) = events.as_array() else { return out; };
    for e in arr {
        let kickoff = e["startDate"].as_str().unwrap_or("").to_string();
        let Some(markets) = e["markets"].as_array() else { continue; };
        for m in markets {
            if let Some(mt) = map_market(m, &kickoff) {
                out.push(mt);
            }
        }
    }
    out
}

fn parse_str_array(v: &Value) -> Vec<String> {
    // 字段是 JSON 编码的字符串,如 "[\"Yes\", \"No\"]"
    match v.as_str() {
        Some(s) => serde_json::from_str::<Vec<String>>(s).unwrap_or_default(),
        None => Vec::new(),
    }
}

fn map_market(m: &Value, kickoff: &str) -> Option<Match> {
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
        kickoff: kickoff.to_string(),
        odds: Odds { home: 1.0 / p0, draw: 0.0, away: 1.0 / p1 },
        handicap: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_two_way_market_to_match() {
        let events = serde_json::json!([{
            "title": "Will Mexico win Group A in the 2026 FIFA World Cup?",
            "startDate": "2026-06-11T19:00:00Z",
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
        assert_eq!(m.odds.draw, 0.0);
        // 1/0.615 ≈ 1.626 ; 1/0.385 ≈ 2.597
        assert!((m.odds.home - 1.0/0.615).abs() < 1e-6);
        assert!((m.odds.away - 1.0/0.385).abs() < 1e-6);
        assert_eq!(m.kickoff, "2026-06-11T19:00:00Z");
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
}
