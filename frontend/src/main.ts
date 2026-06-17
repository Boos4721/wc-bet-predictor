import { api, Match, Prediction, Outcome } from "./api";

const $ = (id: string) => document.getElementById(id)!;
let matches: Match[] = [];

// 转义不可信字符串(粘贴数据、模型返回、错误信息)后再插入 innerHTML
function esc(v: unknown): string {
  return String(v ?? "").replace(/[&<>"']/g, c => (
    { "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]!
  ));
}

// ---- 配置区 ----
async function renderConfig() {
  const cfg = await api.getConfig().catch(() => ({}));
  $("config-form").innerHTML = `
    <input id="cfg-url" placeholder="Base URL" value="${esc(cfg.base_url)}" size="32"/>
    <input id="cfg-key" placeholder="API Key${cfg.has_key ? "(已保存)" : ""}" type="password" size="24"/>
    <input id="cfg-model" placeholder="Model" value="${esc(cfg.model)}" size="18"/>
    <select id="cfg-proto">
      <option value="Anthropic" ${cfg.protocol === "Anthropic" ? "selected" : ""}>Claude</option>
      <option value="OpenAI" ${cfg.protocol === "OpenAI" ? "selected" : ""}>OpenAI</option>
    </select>
    <button id="cfg-save">保存</button>`;
  $("cfg-save").onclick = async () => {
    await api.setConfig({
      base_url: (<HTMLInputElement>$("cfg-url")).value,
      api_key: (<HTMLInputElement>$("cfg-key")).value,
      model: (<HTMLInputElement>$("cfg-model")).value,
      protocol: (<HTMLSelectElement>$("cfg-proto")).value,
    });
    alert("配置已保存");
    renderConfig();
  };
}

// ---- 赛事区 ----
function renderMatches() {
  $("match-list").innerHTML = matches.map((m, i) => `
    <div class="card">
      <strong>${esc(m.id)}</strong> ${esc(m.league)} — ${esc(m.home)} vs ${esc(m.away)}
      <div class="muted">开赛 ${esc(m.kickoff)} · 赔率 ${m.odds.home.toFixed(2)}/${m.odds.draw > 0 ? m.odds.draw.toFixed(2) : "—"}/${m.odds.away.toFixed(2)}</div>
      <button data-i="${i}" class="predict-btn">预测</button>
    </div>`).join("");
  document.querySelectorAll<HTMLButtonElement>(".predict-btn").forEach(b =>
    b.onclick = () => doPredict(matches[+b.dataset.i!]));
}

async function loadMatches(loader: () => Promise<Match[]>) {
  try {
    matches = await loader();
    renderMatches();
  } catch (e: any) { $("match-list").innerHTML = `<p class="loss">${esc(e.message)}</p>`; }
}

$("parse-btn").onclick = () => loadMatches(() => api.parseMatches((<HTMLTextAreaElement>$("paste")).value));
$("poly-btn").onclick = () => loadMatches(() => api.polymarketMatches());

// ---- 预测区 ----
async function doPredict(m: Match) {
  $("prediction-view").innerHTML = `<p class="muted">预测中…</p>`;
  try {
    const p: Prediction = await api.predict(m);
    const pct = (n: number) => Math.round(n * 100);
    $("prediction-view").innerHTML = `
      <div class="card">
        <strong>${esc(m.home)} vs ${esc(m.away)}</strong> — 推荐 <b>${zh(p.pick)}</b>(置信 ${pct(p.confidence)}%)
        <div class="prob-bar">
          <span style="width:${pct(p.probs.home)}%;background:#3b82f6">主 ${pct(p.probs.home)}%</span>
          <span style="width:${pct(p.probs.draw)}%;background:#6b7280">平 ${pct(p.probs.draw)}%</span>
          <span style="width:${pct(p.probs.away)}%;background:#ef4444">客 ${pct(p.probs.away)}%</span>
        </div>
        <p>${esc(p.rationale)}</p>
        金额 <input id="stake" type="number" value="100" size="6"/>
        赔率 <input id="odds" type="number" value="${oddsFor(m, p.pick)}" size="6" step="0.01"/>
        <button id="bet-btn">记一笔(虚拟)</button>
      </div>`;
    $("bet-btn").onclick = async () => {
      await api.placeBet({ match_id: m.id, pick: p.pick,
        stake: +(<HTMLInputElement>$("stake")).value,
        odds: +(<HTMLInputElement>$("odds")).value });
      refreshLedger();
    };
  } catch (e: any) { $("prediction-view").innerHTML = `<p class="loss">${esc(e.message)}</p>`; }
}

function oddsFor(m: Match, pick: Outcome): number {
  return pick === "Home" ? m.odds.home : pick === "Draw" ? m.odds.draw : m.odds.away;
}
function zh(o: Outcome) { return o === "Home" ? "主胜" : o === "Draw" ? "平局" : "客胜"; }

// ---- 账本区 ----
async function refreshLedger() {
  const [bets, stats] = await Promise.all([api.listBets(), api.stats()]);
  $("stats-view").innerHTML = `
    <div class="card">已结算 ${stats.settled} 笔 ·
      命中率 <b>${(stats.hit_rate * 100).toFixed(1)}%</b> ·
      累计盈亏 <b class="${stats.total_pnl >= 0 ? "win" : "loss"}">${stats.total_pnl.toFixed(2)}</b> ·
      ROI <b class="${stats.roi >= 0 ? "win" : "loss"}">${(stats.roi * 100).toFixed(1)}%</b></div>`;
  $("bet-list").innerHTML = `<table><tr><th>#</th><th>场次</th><th>推荐</th><th>金额</th><th>赔率</th><th>状态</th><th>结算</th></tr>
    ${bets.map(b => `<tr>
      <td>${b.id}</td><td>${esc(b.match_id)}</td><td>${zh(b.pick as Outcome)}</td>
      <td>${b.stake}</td><td>${b.odds_at_bet}</td>
      <td class="${b.status === "Won" ? "win" : b.status === "Lost" ? "loss" : "muted"}">${b.status}</td>
      <td>${b.status === "Pending" ? settleCtl(b.id) : "—"}</td></tr>`).join("")}</table>`;
  bets.filter(b => b.status === "Pending").forEach(b => {
    $("settle-" + b.id).onclick = async () => {
      const sel = (<HTMLSelectElement>$("res-" + b.id)).value as Outcome;
      await api.settle(b.id, sel); refreshLedger();
    };
  });
}
function settleCtl(id: number) {
  return `<select id="res-${id}"><option value="Home">主胜</option>
    <option value="Draw">平局</option><option value="Away">客胜</option></select>
    <button id="settle-${id}">结算</button>`;
}

renderConfig();
refreshLedger();
