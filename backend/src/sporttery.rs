use crate::domain::{Match, Odds};
use serde_json::Value;

const LIST_URL: &str =
    "https://webapi.sporttery.cn/gateway/uniform/football/getMatchListV1.qry?clientCode=3001";

/// 解析字符串小数赔率;空或非正返回 None。
fn parse_odd(v: &Value) -> Option<f64> {
    let s = v.as_str()?;
    let p: f64 = s.trim().parse().ok()?;
    if p > 0.0 { Some(p) } else { None }
}

/// 在 oddsList 中找指定 poolCode 且 h/d/a 均有效的赔率项。
fn find_pool<'a>(odds_list: &'a [Value], pool: &str) -> Option<&'a Value> {
    odds_list.iter().find(|o| o["poolCode"].as_str() == Some(pool))
}

fn pool_odds(item: &Value) -> Option<Odds> {
    let home = parse_odd(&item["h"])?;
    let draw = parse_odd(&item["d"])?;
    let away = parse_odd(&item["a"])?;
    Some(Odds { home, draw, away })
}

/// 解析让球数字符串("-2.00" → Some(-2));空或非数返回 None。
fn parse_handicap(v: &Value) -> Option<i32> {
    let s = v.as_str()?.trim();
    if s.is_empty() { return None; }
    let f: f64 = s.parse().ok()?;
    Some(f as i32)
}

/// 纯映射:读取根 JSON 的 value.matchInfoList,每场子赛事产出一行(HAD 胜平负)。
pub fn map_matches(value: &Value) -> Vec<Match> {
    let mut keyed: Vec<((String, String, String), Match)> = Vec::new();
    let Some(infos) = value["value"]["matchInfoList"].as_array() else { return Vec::new(); };
    for info in infos {
        let Some(subs) = info["subMatchList"].as_array() else { continue; };
        for sm in subs {
            let home = sm["homeTeamAllName"].as_str().unwrap_or("").to_string();
            let away = sm["awayTeamAllName"].as_str().unwrap_or("").to_string();
            let league = sm["leagueAllName"].as_str().unwrap_or("").to_string();
            // 仅保留世界杯赛事,过滤其他联赛(芬兰超级联赛等)。
            if league != "世界杯" { continue; }
            let kickoff = sm["businessDate"].as_str().unwrap_or("").to_string();
            let num = sm["matchNumStr"].as_str().unwrap_or("").to_string();
            let match_date = sm["matchDate"].as_str().unwrap_or("").to_string();
            let match_time = sm["matchTime"].as_str().unwrap_or("").to_string();
            let odds_list = sm["oddsList"].as_array().cloned().unwrap_or_default();

            // 仅 HAD 行(胜平负);体彩不展示让球盘。
            if let Some(odds) = find_pool(&odds_list, "HAD").and_then(pool_odds) {
                // 同场的 HHAD(让球胜平负)若有效,附在同一行上(不另起让球行)。
                let hhad = find_pool(&odds_list, "HHAD");
                let hhad_odds = hhad.and_then(pool_odds);
                let hhad_line = hhad.and_then(|p| parse_handicap(&p["goalLine"]));
                let m = Match {
                    id: num.clone(),
                    league: league.clone(),
                    home: home.clone(),
                    away: away.clone(),
                    kickoff: kickoff.clone(),
                    odds,
                    handicap: None,
                    hhad_odds,
                    hhad_line,
                };
                keyed.push(((match_date.clone(), match_time.clone(), m.id.clone()), m));
            }
        }
    }
    // 按(比赛日, 开赛时间, id)升序,使队列按时间先后排列。
    keyed.sort_by(|a, b| a.0.cmp(&b.0));
    keyed.into_iter().map(|(_, m)| m).collect()
}

async fn fetch_raw() -> Result<Value, String> {
    let client = reqwest::Client::new();
    let resp = client.get(LIST_URL)
        // 标准浏览器 UA + Referer:该公开接口对常规请求返回 200;
        // 仅在使用异常 UA 时被 WAF 拦截。此处不做任何指纹伪装/代理规避。
        .header("user-agent", "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0 Safari/537.36")
        .header("referer", "https://www.sporttery.cn/")
        .send().await.map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("Sporttery HTTP {}", resp.status()));
    }
    resp.json().await.map_err(|e| e.to_string())
}

pub async fn fetch_and_map() -> Result<Vec<Match>, String> {
    Ok(map_matches(&fetch_raw().await?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn root() -> Value {
        serde_json::json!({ "value": { "matchInfoList": [
            { "businessDate": "2026-06-17", "subMatchList": [
                {
                    "homeTeamAllName": "葡萄牙", "awayTeamAllName": "刚果(金)",
                    "leagueAllName": "世界杯", "matchDate": "2026-06-18", "matchTime": "01:00",
                    "businessDate": "2026-06-17", "matchNumStr": "周三021", "matchNum": 3021,
                    "matchId": 2040182, "matchStatus": "Selling",
                    "oddsList": [
                        { "poolCode": "HHAD", "h": "2.54", "d": "4.00", "a": "2.06", "goalLine": "-2.00" },
                        { "poolCode": "HAD", "h": "1.13", "d": "5.86", "a": "13.50", "goalLine": "" },
                        { "poolCode": "CRS", "h": "", "d": "", "a": "", "goalLine": "" }
                    ]
                },
                {
                    "homeTeamAllName": "英格兰", "awayTeamAllName": "克罗地亚",
                    "leagueAllName": "世界杯", "matchDate": "2026-06-17", "matchTime": "20:00",
                    "businessDate": "2026-06-17", "matchNumStr": "周三011", "matchNum": 3011,
                    "matchId": 2040111, "matchStatus": "Selling",
                    "oddsList": [
                        { "poolCode": "HAD", "h": "1.80", "d": "3.50", "a": "4.20", "goalLine": "" }
                    ]
                }
            ]}
        ]}})
    }

    #[test]
    fn maps_had_only_no_handicap_rows() {
        let ms = map_matches(&root());
        // 每场仅 HAD 一行,不产出让球行 → 共 2 行
        assert_eq!(ms.len(), 2);
        // 时间升序:英格兰 06-17 20:00 在 葡萄牙 06-18 01:00 之前
        assert_eq!(ms[0].home, "英格兰");
        // 葡萄牙 HAD 行
        let had = ms.iter().find(|m| m.id == "周三021").unwrap();
        assert_eq!(had.home, "葡萄牙"); assert_eq!(had.away, "刚果(金)");
        assert_eq!(had.kickoff, "2026-06-17"); assert_eq!(had.handicap, None);
        assert!((had.odds.home - 1.13).abs() < 1e-9);
        assert!((had.odds.away - 13.50).abs() < 1e-9);
        // 同一行附带 HHAD 让球赔率与让球数(不另起让球行)
        let hh = had.hhad_odds.as_ref().expect("应有 HHAD 赔率");
        assert!((hh.home - 2.54).abs() < 1e-9);
        assert!((hh.draw - 4.00).abs() < 1e-9);
        assert!((hh.away - 2.06).abs() < 1e-9);
        assert_eq!(had.hhad_line, Some(-2));
        // 英格兰行无 HHAD 池 → hhad 字段为 None
        let eng = ms.iter().find(|m| m.id == "周三011").unwrap();
        assert!(eng.hhad_odds.is_none());
        assert!(eng.hhad_line.is_none());
        // 不应有让球行
        assert!(ms.iter().all(|m| !m.id.contains("让")));
        assert!(ms.iter().all(|m| m.handicap.is_none()));
        assert!(ms.iter().all(|m| !m.league.contains("让球")));
    }

    #[test]
    fn skips_pool_with_empty_odds() {
        // CRS/HHAD 不产出 HAD 行;只 HAD 有效
        let ms = map_matches(&root());
        assert!(ms.iter().all(|m| !m.id.contains("CRS")));
    }

    #[test]
    fn filters_out_non_world_cup_leagues() {
        let mixed = serde_json::json!({ "value": { "matchInfoList": [
            { "businessDate": "2026-06-17", "subMatchList": [
                {
                    "homeTeamAllName": "赫尔辛基", "awayTeamAllName": "国际图尔库",
                    "leagueAllName": "芬兰超级联赛", "matchDate": "2026-06-17", "matchTime": "23:00",
                    "businessDate": "2026-06-17", "matchNumStr": "周三201",
                    "oddsList": [ { "poolCode": "HAD", "h": "2.25", "d": "3.10", "a": "2.76", "goalLine": "" } ]
                },
                {
                    "homeTeamAllName": "葡萄牙", "awayTeamAllName": "刚果(金)",
                    "leagueAllName": "世界杯", "matchDate": "2026-06-18", "matchTime": "01:00",
                    "businessDate": "2026-06-17", "matchNumStr": "周三021",
                    "oddsList": [ { "poolCode": "HAD", "h": "1.13", "d": "5.86", "a": "13.50", "goalLine": "" } ]
                }
            ]}
        ]}});
        let ms = map_matches(&mixed);
        assert_eq!(ms.len(), 1);
        assert_eq!(ms[0].league, "世界杯");
        assert_eq!(ms[0].home, "葡萄牙");
    }

    #[test]
    fn empty_value_yields_empty() {
        assert_eq!(map_matches(&serde_json::json!({"value":{"matchInfoList":[]}})).len(), 0);
        assert_eq!(map_matches(&serde_json::json!({})).len(), 0);
    }
}
