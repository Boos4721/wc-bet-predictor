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
