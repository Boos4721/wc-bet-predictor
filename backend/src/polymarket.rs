use crate::domain::{Match, Odds};
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;

const SERIES_URL: &str =
    "https://gamma-api.polymarket.com/events?closed=false&series_id=11433";

#[derive(Debug, Clone, Serialize)]
pub struct DateCount { pub date: String, pub count: usize }

async fn fetch_events() -> Result<Value, String> {
    let client = reqwest::Client::new();
    let mut all: Vec<Value> = Vec::new();
    let mut offset = 0;
    loop {
        let url = format!("{SERIES_URL}&limit=100&offset={offset}");
        let resp = client.get(&url)
            .header("user-agent", "wc-bet-predictor")
            .send().await.map_err(|e| e.to_string())?;
        if !resp.status().is_success() {
            return Err(format!("Polymarket HTTP {}", resp.status()));
        }
        let page: Value = resp.json().await.map_err(|e| e.to_string())?;
        let arr = page.as_array().cloned().unwrap_or_default();
        let n = arr.len();
        all.extend(arr);
        if n < 100 || all.len() >= 500 { break; }
        offset += 100;
    }
    Ok(Value::Array(all))
}

pub async fn fetch_matches(date: Option<&str>, limit: usize) -> Result<Vec<Match>, String> {
    let events = fetch_events().await?;
    let mut ms = map_events(&events);
    if let Some(d) = date {
        ms.retain(|m| m.kickoff == d);
    }
    ms.truncate(limit);
    Ok(ms)
}

pub async fn fetch_dates() -> Result<Vec<DateCount>, String> {
    let events = fetch_events().await?;
    Ok(available_dates(&map_events(&events)))
}

/// 取 slug 末尾的 YYYY-MM-DD(形如 fifwc-eng-hrv-2026-06-17)
fn slug_date(slug: &str) -> Option<String> {
    if slug.len() < 10 { return None; }
    let tail = &slug[slug.len() - 10..];
    let b = tail.as_bytes();
    let ok = b[4] == b'-' && b[7] == b'-'
        && b.iter().enumerate().all(|(i, c)| {
            if i == 4 || i == 7 { *c == b'-' } else { c.is_ascii_digit() }
        });
    if ok { Some(tail.to_string()) } else { None }
}

/// "England vs. Croatia" → ("England","Croatia"). 兼容 " vs " 与 " vs. "。
fn split_vs(title: &str) -> Option<(String, String)> {
    for sep in [" vs. ", " vs "] {
        if let Some(i) = title.find(sep) {
            let h = title[..i].trim().to_string();
            let a = title[i + sep.len()..].trim().to_string();
            if !h.is_empty() && !a.is_empty() { return Some((h, a)); }
        }
    }
    None
}

/// 该子市场 Yes 价格(outcomePrices[0]),解析为 f64;<=0 返回 None。
fn yes_price(m: &Value) -> Option<f64> {
    let raw = m["outcomePrices"].as_str()?;
    let arr: Vec<String> = serde_json::from_str(raw).ok()?;
    let p: f64 = arr.first()?.parse().ok()?;
    if p > 0.0 { Some(p) } else { None }
}

fn map_event(e: &Value) -> Option<Match> {
    let slug = e["slug"].as_str().unwrap_or("");
    let date = slug_date(slug)?;
    let title = e["title"].as_str().unwrap_or("");
    let (home, away) = split_vs(title)?;
    let markets = e["markets"].as_array()?;
    let (mut oh, mut od, mut oa) = (None, None, None);
    for m in markets {
        let git = m["groupItemTitle"].as_str().unwrap_or("");
        let Some(p) = yes_price(m) else { continue };
        if git.starts_with("Draw") { od = Some(1.0 / p); }
        else if git == home { oh = Some(1.0 / p); }
        else if git == away { oa = Some(1.0 / p); }
    }
    let id = e["id"].as_str().map(|s| s.to_string())
        .unwrap_or_else(|| slug.to_string());
    Some(Match {
        id,
        league: "世界杯".to_string(),
        home, away,
        kickoff: date,
        odds: Odds { home: oh?, draw: od?, away: oa? },
        handicap: None,
    })
}

pub fn map_events(events: &Value) -> Vec<Match> {
    let mut out = Vec::new();
    let Some(arr) = events.as_array() else { return out; };
    for e in arr {
        if let Some(m) = map_event(e) { out.push(m); }
    }
    // 按比赛日 + 主队稳定排序
    out.sort_by(|a, b| a.kickoff.cmp(&b.kickoff).then(a.home.cmp(&b.home)));
    out
}

pub fn available_dates(ms: &[Match]) -> Vec<DateCount> {
    let mut map: BTreeMap<String, usize> = BTreeMap::new();
    for m in ms {
        if !m.kickoff.is_empty() {
            *map.entry(m.kickoff.clone()).or_insert(0) += 1;
        }
    }
    map.into_iter().map(|(date, count)| DateCount { date, count }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_event() -> Value {
        serde_json::json!({
            "id": "evt1",
            "slug": "fifwc-eng-hrv-2026-06-17",
            "title": "England vs. Croatia",
            "markets": [
                { "groupItemTitle": "Croatia", "outcomePrices": "[\"0.175\", \"0.825\"]" },
                { "groupItemTitle": "England", "outcomePrices": "[\"0.565\", \"0.435\"]" },
                { "groupItemTitle": "Draw (England vs. Croatia)", "outcomePrices": "[\"0.255\", \"0.745\"]" }
            ]
        })
    }

    #[test]
    fn maps_three_way_game() {
        let ms = map_events(&serde_json::json!([sample_event()]));
        assert_eq!(ms.len(), 1);
        let m = &ms[0];
        assert_eq!(m.home, "England");
        assert_eq!(m.away, "Croatia");
        assert_eq!(m.kickoff, "2026-06-17");
        assert!((m.odds.home - 1.0/0.565).abs() < 1e-6);
        assert!((m.odds.draw - 1.0/0.255).abs() < 1e-6);
        assert!((m.odds.away - 1.0/0.175).abs() < 1e-6);
    }

    #[test]
    fn skips_event_missing_an_outcome() {
        let ev = serde_json::json!({
            "slug": "fifwc-aaa-bbb-2026-06-18", "title": "AAA vs. BBB",
            "markets": [
                { "groupItemTitle": "AAA", "outcomePrices": "[\"0.5\", \"0.5\"]" },
                { "groupItemTitle": "Draw (AAA vs. BBB)", "outcomePrices": "[\"0.3\", \"0.7\"]" }
            ] // no away market → can't form 3-way
        });
        assert_eq!(map_events(&serde_json::json!([ev])).len(), 0);
    }

    #[test]
    fn skips_event_with_bad_slug_date() {
        let ev = serde_json::json!({
            "slug": "some-non-dated-slug", "title": "X vs. Y",
            "markets": [
                { "groupItemTitle": "X", "outcomePrices": "[\"0.5\",\"0.5\"]" },
                { "groupItemTitle": "Y", "outcomePrices": "[\"0.4\",\"0.6\"]" },
                { "groupItemTitle": "Draw (X vs. Y)", "outcomePrices": "[\"0.2\",\"0.8\"]" }
            ]
        });
        assert_eq!(map_events(&serde_json::json!([ev])).len(), 0);
    }

    #[test]
    fn available_dates_groups_and_counts() {
        let e1 = sample_event(); // 2026-06-17
        let mut e2 = sample_event();
        e2["slug"] = serde_json::json!("fifwc-bra-hai-2026-06-19");
        e2["title"] = serde_json::json!("Brazil vs. Haiti");
        e2["markets"] = serde_json::json!([
            { "groupItemTitle": "Brazil", "outcomePrices": "[\"0.885\",\"0.115\"]" },
            { "groupItemTitle": "Haiti", "outcomePrices": "[\"0.0425\",\"0.9575\"]" },
            { "groupItemTitle": "Draw (Brazil vs. Haiti)", "outcomePrices": "[\"0.078\",\"0.922\"]" }
        ]);
        let ms = map_events(&serde_json::json!([e1, e2]));
        let dates = available_dates(&ms);
        assert_eq!(dates.len(), 2);
        assert_eq!(dates[0].date, "2026-06-17");
        assert_eq!(dates[0].count, 1);
        assert_eq!(dates[1].date, "2026-06-19");
        assert_eq!(dates[1].count, 1);
    }

    #[test]
    fn empty_input_yields_empty() {
        assert_eq!(map_events(&serde_json::json!([])).len(), 0);
    }
}
