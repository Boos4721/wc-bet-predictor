// API response shapes — the backend is the source of truth. These mirror
// the JSON returned by the axum routes in backend/src/api.rs.

export type Pick = "Home" | "Draw" | "Away";
export type SettleResult = Pick;
export type BetStatus = "Pending" | "Won" | "Lost";
export type Protocol = "Anthropic" | "OpenAI";
export type SourceId = "sporttery" | "polymarket";

export interface Odds {
  home: number;
  draw: number; // 0 means a 2-way market (no draw)
  away: number;
}

export interface Match {
  id: string;
  league: string;
  home: string;
  away: string;
  kickoff: string;
  odds: Odds;
  handicap?: number | null;
  hhad_odds?: Odds | null;          // 让球胜平负赔率(体彩)
  hhad_line?: number | null;        // 让球数
  pm_score?: PlayOption[] | null;   // 比分(Polymarket)
  pm_halftime?: PlayOption[] | null; // 半场(Polymarket)
}

// 一个玩法选项:展示标签 + 赔率(用于混合过关选腿)
export interface PlayOption {
  label: string;
  odds: number;
}

export interface ProbOption {
  label: string;
  prob: number; // 0..1
}

export interface Prediction {
  model?: string;
  options?: ProbOption[];
  confidence?: number; // 0..1
  pick?: Pick | null;
  pick_label?: string | null;
  pick_odds?: number | null;
  rationale?: string;
}

export interface Bet {
  id: number;
  match_id: string;
  pick: Pick;
  stake: number;
  odds_at_bet: number;
  status: BetStatus;
}

export interface Stats {
  settled?: number;
  hit_rate?: number; // 0..1
  total_pnl?: number;
  roi?: number; // 0..1
}

export interface AiConfig {
  base_url?: string;
  model?: string;
  protocol?: Protocol;
  has_key?: boolean;
}

export interface DayCount {
  date: string; // ISO YYYY-MM-DD
  count: number;
}

export interface SourceStatus {
  updated_at?: string | number; // unix seconds, sometimes "@<secs>"
}

// ---- parlay calculator + tickets ----
export interface CalcLeg {
  label: string;
  odds: number;
}

export interface Ticket {
  id: number;
  legs: string; // JSON-encoded CalcLeg[]
  ways: string; // JSON-encoded number[]
  multiplier?: number;
  stake?: number;
  max_return?: number;
  status: BetStatus;
  payout?: number;
}
