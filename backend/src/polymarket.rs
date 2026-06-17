use crate::domain::{Match, Odds};
use serde_json::Value;

const SERIES_URL: &str =
    "https://gamma-api.polymarket.com/events?closed=false&series_id=11433";

/// 英文国家队名 → 中文。未收录的原样返回(不臆造)。
fn translate_team(name: &str) -> String {
    match name {
        "England" => "英格兰", "Croatia" => "克罗地亚", "Ghana" => "加纳",
        "Panama" => "巴拿马", "Portugal" => "葡萄牙", "DR Congo" => "刚果（金）",
        "Uzbekistan" => "乌兹别克斯坦", "Colombia" => "哥伦比亚", "Brazil" => "巴西",
        "Haiti" => "海地", "Mexico" => "墨西哥", "Korea Republic" => "韩国",
        "South Korea" => "韩国", "Netherlands" => "荷兰", "Sweden" => "瑞典",
        "New Zealand" => "新西兰", "Egypt" => "埃及", "Ecuador" => "厄瓜多尔",
        "Germany" => "德国", "Côte d'Ivoire" => "科特迪瓦", "Cote d'Ivoire" => "科特迪瓦",
        "Ivory Coast" => "科特迪瓦", "Belgium" => "比利时", "Iran" => "伊朗",
        "Canada" => "加拿大", "Qatar" => "卡塔尔", "Switzerland" => "瑞士",
        "Bosnia and Herzegovina" => "波黑", "Bosnia-Herzegovina" => "波黑",
        "Bosnia" => "波黑", "Czechia" => "捷克",
        "Czech Republic" => "捷克", "South Africa" => "南非", "Spain" => "西班牙",
        "Saudi Arabia" => "沙特阿拉伯", "Argentina" => "阿根廷", "France" => "法国",
        "USA" => "美国", "United States" => "美国", "Japan" => "日本",
        "Australia" => "澳大利亚", "Senegal" => "塞内加尔", "Morocco" => "摩洛哥",
        "Nigeria" => "尼日利亚", "Cameroon" => "喀麦隆", "Uruguay" => "乌拉圭",
        "Denmark" => "丹麦", "Poland" => "波兰", "Serbia" => "塞尔维亚",
        "Italy" => "意大利", "Wales" => "威尔士", "Scotland" => "苏格兰",
        "Norway" => "挪威", "Austria" => "奥地利", "Turkey" => "土耳其",
        "Türkiye" => "土耳其", "Greece" => "希腊", "Peru" => "秘鲁",
        "Chile" => "智利", "Paraguay" => "巴拉圭", "Venezuela" => "委内瑞拉",
        "Costa Rica" => "哥斯达黎加", "Jamaica" => "牙买加", "Honduras" => "洪都拉斯",
        "Algeria" => "阿尔及利亚", "Tunisia" => "突尼斯", "Mali" => "马里",
        "Ukraine" => "乌克兰", "Slovenia" => "斯洛文尼亚", "Slovakia" => "斯洛伐克",
        "Hungary" => "匈牙利", "Romania" => "罗马尼亚", "China" => "中国",
        "Iraq" => "伊拉克", "United Arab Emirates" => "阿联酋", "UAE" => "阿联酋",
        "Jordan" => "约旦", "Oman" => "阿曼", "Cape Verde" => "佛得角",
        "Curaçao" => "库拉索", "Curacao" => "库拉索", "Panama " => "巴拿马",
        other => other,
    }.to_string()
}

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

fn map_event(e: &Value) -> Option<(String, Match)> {
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
    // 真实开赛时间(ISO8601),用于排序;缺失则退回比赛日。
    let sort_key = e["startTime"].as_str()
        .or_else(|| e["endDate"].as_str())
        .unwrap_or(&date)
        .to_string();
    let m = Match {
        id,
        league: "世界杯".to_string(),
        home: translate_team(&home),
        away: translate_team(&away),
        kickoff: date,
        odds: Odds { home: oh?, draw: od?, away: oa? },
        handicap: None,
    };
    Some((sort_key, m))
}

pub fn map_events(events: &Value) -> Vec<Match> {
    let mut keyed: Vec<(String, Match)> = Vec::new();
    let Some(arr) = events.as_array() else { return Vec::new(); };
    for e in arr {
        if let Some(km) = map_event(e) { keyed.push(km); }
    }
    // 按真实开赛时间升序(同时间按主队稳定排序)
    keyed.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.home.cmp(&b.1.home)));
    keyed.into_iter().map(|(_, m)| m).collect()
}

pub async fn fetch_and_map() -> Result<Vec<Match>, String> {
    let events = fetch_events().await?;
    Ok(map_events(&events))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::available_dates;

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
        assert_eq!(m.home, "英格兰");
        assert_eq!(m.away, "克罗地亚");
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

    #[test]
    fn unknown_team_falls_back_to_english() {
        assert_eq!(translate_team("Wakanda"), "Wakanda");
        assert_eq!(translate_team("Brazil"), "巴西");
    }

    #[test]
    fn bosnia_hyphenated_translates() {
        assert_eq!(translate_team("Bosnia-Herzegovina"), "波黑");
        assert_eq!(translate_team("Bosnia and Herzegovina"), "波黑");
    }

    #[test]
    fn sorts_by_start_time_within_day() {
        // 同一比赛日,两场不同开赛时间;乱序输入应按 startTime 升序排出
        let late = serde_json::json!({
            "id": "late", "slug": "fifwc-aaa-bbb-2026-06-17", "title": "AAA vs. BBB",
            "startTime": "2026-06-17T20:00:00Z",
            "markets": [
                { "groupItemTitle": "AAA", "outcomePrices": "[\"0.5\",\"0.5\"]" },
                { "groupItemTitle": "BBB", "outcomePrices": "[\"0.4\",\"0.6\"]" },
                { "groupItemTitle": "Draw (AAA vs. BBB)", "outcomePrices": "[\"0.2\",\"0.8\"]" }
            ]
        });
        let early = serde_json::json!({
            "id": "early", "slug": "fifwc-ccc-ddd-2026-06-17", "title": "CCC vs. DDD",
            "startTime": "2026-06-17T14:00:00Z",
            "markets": [
                { "groupItemTitle": "CCC", "outcomePrices": "[\"0.5\",\"0.5\"]" },
                { "groupItemTitle": "DDD", "outcomePrices": "[\"0.4\",\"0.6\"]" },
                { "groupItemTitle": "Draw (CCC vs. DDD)", "outcomePrices": "[\"0.2\",\"0.8\"]" }
            ]
        });
        let ms = map_events(&serde_json::json!([late, early]));
        assert_eq!(ms.len(), 2);
        assert_eq!(ms[0].id, "early"); // 14:00 在前
        assert_eq!(ms[1].id, "late");  // 20:00 在后
        assert_eq!(ms[0].kickoff, "2026-06-17"); // kickoff 仍为比赛日
    }
}
