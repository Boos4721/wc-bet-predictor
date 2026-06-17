use crate::domain::Match;
use serde::Serialize;
use std::collections::BTreeMap;
use std::sync::RwLock;

#[derive(Debug, Clone, Serialize)]
pub struct DateCount { pub date: String, pub count: usize }

pub fn available_dates(ms: &[Match]) -> Vec<DateCount> {
    let mut map: BTreeMap<String, usize> = BTreeMap::new();
    for m in ms {
        if !m.kickoff.is_empty() { *map.entry(m.kickoff.clone()).or_insert(0) += 1; }
    }
    map.into_iter().map(|(date, count)| DateCount { date, count }).collect()
}

pub struct MatchCache {
    matches: RwLock<Vec<Match>>,
    updated: RwLock<Option<String>>,
    path: String,
}
impl MatchCache {
    pub fn new(path: &str) -> Self {
        MatchCache { matches: RwLock::new(Vec::new()), updated: RwLock::new(None), path: path.to_string() }
    }
    /// 冷启动:若磁盘快照存在则载入内存(不联网)。
    pub fn load_disk(&self) {
        if let Ok(s) = std::fs::read_to_string(&self.path) {
            if let Ok(ms) = serde_json::from_str::<Vec<Match>>(&s) {
                *self.matches.write().unwrap() = ms;
            }
        }
    }
    /// 换入新数据(写内存+磁盘+更新时间)。不持锁跨 await:调用方先 fetch 完再 store。
    pub fn store(&self, ms: Vec<Match>) {
        if let Ok(s) = serde_json::to_string(&ms) { let _ = std::fs::write(&self.path, s); }
        *self.matches.write().unwrap() = ms;
        *self.updated.write().unwrap() = Some(now_stamp());
    }
    pub fn snapshot(&self) -> Vec<Match> { self.matches.read().unwrap().clone() }
    pub fn updated(&self) -> Option<String> { self.updated.read().unwrap().clone() }
    pub fn len(&self) -> usize { self.matches.read().unwrap().len() }
}

pub fn now_stamp() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    format!("@{secs}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::Odds;

    fn mk(kickoff: &str) -> Match {
        Match {
            id: "x".into(), league: "世界杯".into(), home: "A".into(), away: "B".into(),
            kickoff: kickoff.into(), odds: Odds { home: 2.0, draw: 3.0, away: 3.5 }, handicap: None,
        }
    }

    #[test]
    fn cache_snapshot_empty_when_no_disk() {
        let c = MatchCache::new("/tmp/wcbp-nonexistent-cache-xyz.json");
        c.load_disk();
        assert_eq!(c.snapshot().len(), 0);
        assert_eq!(c.updated(), None);
    }

    #[test]
    fn store_then_snapshot_roundtrips() {
        let path = std::env::temp_dir().join("wcbp-cache-test-store.json");
        let c = MatchCache::new(path.to_string_lossy().as_ref());
        c.store(vec![mk("2026-06-17"), mk("2026-06-18")]);
        assert_eq!(c.len(), 2);
        assert!(c.updated().is_some());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn available_dates_groups_and_counts() {
        let ms = vec![mk("2026-06-17"), mk("2026-06-17"), mk("2026-06-19")];
        let dates = available_dates(&ms);
        assert_eq!(dates.len(), 2);
        assert_eq!(dates[0].date, "2026-06-17");
        assert_eq!(dates[0].count, 2);
        assert_eq!(dates[1].date, "2026-06-19");
        assert_eq!(dates[1].count, 1);
    }
}
