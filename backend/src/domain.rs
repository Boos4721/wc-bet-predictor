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
