export type Outcome = "Home" | "Draw" | "Away";
export interface Odds { home: number; draw: number; away: number; }
export interface Match {
  id: string; league: string; home: string; away: string;
  kickoff: string; odds: Odds; handicap: number | null;
}
export interface Prediction {
  match_id: string; probs: Odds; pick: Outcome;
  confidence: number; rationale: string; model: string; created_at: string;
}
export interface Bet {
  id: number; match_id: string; pick: Outcome;
  stake: number; odds_at_bet: number; status: string; created_at: string;
}
export interface Stats { settled: number; hit_rate: number; total_pnl: number; roi: number; }

async function req<T>(url: string, opts?: RequestInit): Promise<T> {
  const r = await fetch(url, { headers: { "content-type": "application/json" }, ...opts });
  const data = await r.json();
  if (!r.ok) throw new Error(data.error ?? `HTTP ${r.status}`);
  return data as T;
}

export const api = {
  parseMatches: (raw: string) => req<Match[]>("/api/matches", { method: "POST", body: JSON.stringify({ raw }) }),
  predict: (m: Match) => req<Prediction>("/api/predict", { method: "POST", body: JSON.stringify(m) }),
  placeBet: (b: { match_id: string; pick: Outcome; stake: number; odds: number }) =>
    req<{ id: number }>("/api/bets", { method: "POST", body: JSON.stringify(b) }),
  listBets: (status?: string) =>
    req<Bet[]>(`/api/bets${status ? `?status=${status}` : ""}`),
  settle: (bet_id: number, actual_result: Outcome) =>
    req<any>("/api/settle", { method: "POST", body: JSON.stringify({ bet_id, actual_result }) }),
  stats: () => req<Stats>("/api/stats"),
  getConfig: () => req<any>("/api/config"),
  setConfig: (c: any) => req<any>("/api/config", { method: "POST", body: JSON.stringify(c) }),
};
