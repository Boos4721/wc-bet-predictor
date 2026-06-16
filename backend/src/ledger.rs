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
