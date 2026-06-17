// 竞彩预测 · 模拟账本 — terminal logic, ported to TypeScript.
// 1:1 behavioral port of the former inline script; backend is source of truth.
import "./app.css";
import type {
  Match, Prediction, ProbOption, Bet, Stats, AiConfig,
  DayCount, SourceStatus, Pick, SettleResult, SourceId,
  CalcLeg, Ticket, ChatMsg,
} from "./types";

// ---- typed DOM helpers ----
function el<T extends HTMLElement = HTMLElement>(id: string): T {
  const node = document.getElementById(id);
  if (!node) throw new Error(`missing #${id}`);
  return node as T;
}
// non-throwing variant for elements that only exist in some render states
function maybe<T extends HTMLElement = HTMLElement>(id: string): T | null {
  return document.getElementById(id) as T | null;
}

function esc(s: unknown): string {
  return String(s).replace(/[&<>"]/g, (c) =>
    ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" } as Record<string, string>)[c]);
}

// ---- fetch helpers ----
async function api<T = any>(path: string, opts?: RequestInit): Promise<T> {
  const res = await fetch(path, opts || {});
  const txt = await res.text();
  let data: any = {};
  try { data = txt ? JSON.parse(txt) : {}; } catch { data = {}; }
  if (!res.ok) throw new Error(data.error || `HTTP ${res.status}`);
  return data as T;
}
function postJSON<T = any>(path: string, body: unknown): Promise<T> {
  return api<T>(path, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(body),
  });
}

// ---- state (matches kept client-side for selection) ----
let matches: Match[] = [];
let matchMap: Record<string, string> = {};
let selId: string | null = null;
let prediction: Prediction | null = null;
let play = "HAD";          // active 玩法 (bet type) for /api/predict
let source: SourceId = "sporttery"; // active data source; drives pager URLs

// 玩法 presets per data source — [code, name]. 体彩 / Polymarket differ.
const PLAYS: Record<string, [string, string][]> = {
  sporttery: [["HAD", "胜平负"], ["HHAD", "让球胜平负"], ["CRS", "比分"], ["TTG", "总进球数"], ["HAFU", "半全场"]],
  polymarket: [["HAD", "输赢"], ["CRS", "比分"], ["HT", "半场"]],
};
function playList(): [string, string][] { return PLAYS[source] || PLAYS.sporttery; }
function defaultPlay(): string { return playList()[0][0]; }

const PICK_LABEL: Record<string, string> = { Home: "主胜", Draw: "平局", Away: "客胜" };
const CUSTOM = "__custom__";
const MODEL_PRESETS: Record<string, string[]> = {
  Anthropic: ["claude-opus-4-8", "claude-opus-4-6", "claude-sonnet-4-6"],
  OpenAI: ["gpt-5.5", "gpt-5.4", "gpt-5.4-mini"],
};

// ---- clock ----
function tick(): void {
  const d = new Date();
  const p = (n: number) => (n < 10 ? "0" : "") + n;
  el("clock").textContent =
    `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} ` +
    `${p(d.getHours())}:${p(d.getMinutes())}:${p(d.getSeconds())}`;
}

// ---- AI config ----
// Rebuild the model <select> for the active protocol. `keep` pre-selects a
// remembered model (custom mode if it's not a preset for this protocol).
function populateModels(keep?: string): void {
  const sel = el<HTMLSelectElement>("modelSel");
  const list = MODEL_PRESETS[el<HTMLSelectElement>("protocol").value] || [];
  sel.innerHTML = "";
  list.forEach((m) => {
    const o = document.createElement("option");
    o.value = m; o.textContent = m;
    sel.appendChild(o);
  });
  const custom = document.createElement("option");
  custom.value = CUSTOM; custom.textContent = "自定义…";
  sel.appendChild(custom);

  const customInput = el<HTMLInputElement>("modelCustom");
  if (keep && list.indexOf(keep) === -1) {
    sel.value = CUSTOM;
    customInput.value = keep;
  } else {
    sel.value = keep || list[0];
    customInput.value = "";
  }
  syncCustomVisibility();
}

function syncCustomVisibility(): void {
  const isCustom = el<HTMLSelectElement>("modelSel").value === CUSTOM;
  const input = el<HTMLInputElement>("modelCustom");
  input.hidden = !isCustom;
  if (isCustom) input.focus();
}

// resolved model string the rest of the app saves/sends
function currentModel(): string {
  const sel = el<HTMLSelectElement>("modelSel");
  return sel.value === CUSTOM ? el<HTMLInputElement>("modelCustom").value.trim() : sel.value;
}

function loadConfig(): void {
  api<AiConfig>("/api/config").then((c) => {
    if (c.base_url) el<HTMLInputElement>("baseurl").value = c.base_url;
    if (c.protocol) el<HTMLSelectElement>("protocol").value = c.protocol;
    populateModels(c.model);
    const keyhint = el("keyhint");
    if (c.has_key) { keyhint.hidden = false; el<HTMLInputElement>("apikey").placeholder = "已保存 · 重新输入以替换"; }
    else { keyhint.hidden = true; }
  }).catch(() => { /* config endpoint optional on first boot */ });
}

function saveConfig(): void {
  const keyerr = el("keyerr");
  keyerr.hidden = true;
  const base = el<HTMLInputElement>("baseurl").value.trim();
  const model = currentModel();
  const key = el<HTMLInputElement>("apikey").value.trim();
  const protocol = el<HTMLSelectElement>("protocol").value;
  if (!base || !model || !key) {
    keyerr.textContent = "请填写 Base URL、模型与 API Key 后再保存。";
    keyerr.hidden = false; return;
  }
  postJSON("/api/config", { base_url: base, api_key: key, model, protocol })
    .then(() => {
      el<HTMLInputElement>("apikey").value = "";
      keyerr.style.color = "var(--teal-text)";
      keyerr.textContent = "配置已保存。";
      keyerr.hidden = false;
      setTimeout(() => { keyerr.hidden = true; keyerr.style.color = ""; }, 2200);
      loadConfig();
    })
    .catch((e) => {
      keyerr.style.color = "";
      keyerr.textContent = "保存失败：" + e.message;
      keyerr.hidden = false;
    });
}

// ---- odds helpers (draw === 0 means 2-way market) ----
function oddsTxt(o: number): string { return o > 0 ? o.toFixed(2) : "—"; }
function oddsLine(m: Match): string {
  return [oddsTxt(m.odds.home), oddsTxt(m.odds.draw), oddsTxt(m.odds.away)].join(" / ");
}

function setMatches(list: Match[]): void {
  matches = list || [];
  matchMap = {};
  matches.forEach((m) => { matchMap[m.id] = m.home + " vs " + m.away; });
  selId = matches.length ? matches[0].id : null;
  prediction = null;
  renderMatches(true); renderPredEmpty(); renderCalcMatch();
}

function parseMatches(): void {
  const parseerr = el("parseerr");
  parseerr.hidden = true;
  const raw = el<HTMLTextAreaElement>("paste").value;
  if (!raw.trim()) {
    parseerr.textContent = "请先粘贴赛事内容。";
    parseerr.hidden = false; return;
  }
  postJSON<Match[]>("/api/matches", { raw })
    .then((list) => {
      if (!list.length) throw new Error("未能识别任何赛事，请检查格式。");
      setMatches(list);
    })
    .catch((e) => {
      parseerr.textContent = "解析失败：" + e.message;
      parseerr.hidden = false;
    });
}

// ---- dual-source day-by-day pager (体彩官方 / Polymarket) ----
let polyDates: DayCount[] = []; // ascending from backend for active source
let polyIdx = 0;

function todayISO(): string {
  const d = new Date();
  const p = (n: number) => (n < 10 ? "0" : "") + n;
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`;
}

// freshness line — strips the '@' prefix off the unix-seconds updated_at
function loadStatus(): void {
  const fresh = maybe("srcFresh");
  if (!fresh) return;
  fresh.textContent = "数据加载中…";
  api<SourceStatus>("/api/matches/" + source + "/status")
    .then((st) => {
      const raw = st && st.updated_at;
      if (!raw) { fresh.textContent = "数据加载中…"; return; }
      const secs = parseInt(String(raw).replace(/^@/, ""), 10);
      if (!(secs > 0)) { fresh.textContent = "数据加载中…"; return; }
      const d = new Date(secs * 1000);
      const p = (n: number) => (n < 10 ? "0" : "") + n;
      fresh.textContent = `数据更新于 ${p(d.getMonth() + 1)}-${p(d.getDate())} ` +
        `${p(d.getHours())}:${p(d.getMinutes())}`;
    })
    .catch(() => { /* status is best-effort */ });
}

function renderPager(): void {
  const sel = el<HTMLSelectElement>("polyDate");
  const prev = el<HTMLButtonElement>("polyPrev");
  const next = el<HTMLButtonElement>("polyNext");
  const label = el("polyDayLabel");
  if (!polyDates.length) {
    sel.innerHTML = '<option value="">该数据源暂无比赛日</option>';
    label.textContent = "该数据源暂无比赛日";
    prev.disabled = true; next.disabled = true;
    return;
  }
  sel.innerHTML = "";
  polyDates.forEach((d, i) => {
    const opt = document.createElement("option");
    opt.value = d.date;
    opt.textContent = d.date.slice(5) + " · " + d.count + "场";
    if (i === polyIdx) opt.selected = true;
    sel.appendChild(opt);
  });
  const cur = polyDates[polyIdx];
  label.innerHTML = `<b>${esc(cur.date.slice(5))} · ${cur.count} 场</b>` +
    `<br><span style="color:var(--steel)">第 ${polyIdx + 1} / 共 ${polyDates.length} 天</span>`;
  prev.disabled = polyIdx <= 0;
  next.disabled = polyIdx >= polyDates.length - 1;
}

function loadPolyDay(): void {
  if (!polyDates.length) return;
  const polyerr = el("polyerr");
  polyerr.hidden = true;
  const date = polyDates[polyIdx].date;
  el("matchcount").textContent = "加载中…";
  api<Match[]>("/api/matches/" + source + "?limit=40&date=" + encodeURIComponent(date))
    .then((list) => { setMatches(list); loadStatus(); })
    .catch((e) => {
      el("matchcount").textContent = matches.length + " 场";
      polyerr.textContent = "拉取失败：" + e.message;
      polyerr.hidden = false;
    });
}

function gotoPolyIdx(i: number): void {
  if (!polyDates.length) return;
  polyIdx = Math.max(0, Math.min(polyDates.length - 1, i));
  renderPager();
  loadPolyDay();
}

// load available match-days for the active source, then auto-load the first
// day >= today (else index 0)
function loadPolyDates(): void {
  const polyerr = el("polyerr");
  polyerr.hidden = true;
  api<DayCount[]>("/api/matches/" + source + "/dates")
    .then((dates) => {
      polyDates = dates || [];
      if (!polyDates.length) {
        renderPager();
        polyerr.textContent = "该数据源暂无比赛日";
        polyerr.hidden = false;
        return;
      }
      const today = todayISO();
      polyIdx = 0;
      for (let i = 0; i < polyDates.length; i++) {
        if (polyDates[i].date >= today) { polyIdx = i; break; }
      }
      renderPager();
      loadPolyDay();
    })
    .catch((e) => {
      polyerr.textContent = "日期加载失败：" + e.message;
      polyerr.hidden = false;
    });
}

// ---- render match queue ----
// `animate` true only on data arrival (parse / day load), not on selection
// re-renders — so clicking a row never replays the stagger.
function renderMatches(animate?: boolean): void {
  const host = el("matches"); host.innerHTML = "";
  el("matchcount").textContent = matches.length + " 场";
  el("matchesEmpty").hidden = matches.length > 0;
  populateCalcMatches();
  matches.forEach((m, i) => {
    const row = document.createElement("div");
    row.className = "match-row" + (m.id === selId ? " active" : "") + (animate ? " row-in" : "");
    if (animate) row.style.animationDelay = Math.min(i * 26, 260) + "ms";
    const leagueTxt = m.league + (m.handicap != null ? " 让" + m.handicap : "");
    row.innerHTML =
      `<div class="mid mono">${esc(m.id)}</div>` +
      `<div><div class="teams">${esc(m.home)}<span class="vs">vs</span>${esc(m.away)}</div>` +
        `<div class="league">${esc(leagueTxt)}</div></div>` +
      `<div class="odds mono"><span>主<b>${oddsTxt(m.odds.home)}</b></span>` +
        `<span>平<b>${oddsTxt(m.odds.draw)}</b></span>` +
        `<span>客<b>${oddsTxt(m.odds.away)}</b></span></div>`;
    row.addEventListener("click", () => { selId = m.id; prediction = null; renderMatches(); renderPredEmpty(); });
    host.appendChild(row);
  });
}

function currentMatch(): Match | null {
  return matches.filter((m) => m.id === selId)[0] || null;
}

// fill the calculator's match dropdown from the loaded queue
function populateCalcMatches(): void {
  const sel = el<HTMLSelectElement>("calcMatch");
  if (!matches.length) {
    sel.innerHTML = '<option value="">（先在左侧加载赛事）</option>';
    return;
  }
  sel.innerHTML = matches.map((m) =>
    `<option value="${esc(m.id)}">${esc(m.id)} ${esc(m.home)} vs ${esc(m.away)}</option>`).join("");
}

// add a leg from the selected match + outcome (主/平/客); odds auto-filled.
// (implementation lives near the calculator section below)

// ---- prediction panel ----
function renderPredEmpty(): void {
  const m = currentMatch();
  const host = el("predHost");
  if (!m) {
    host.innerHTML = '<div class="pred-empty"><p>从左侧粘贴赛事并解析，选中一场比赛后点击「预测」。</p></div>';
    return;
  }
  const list = playList();
  if (!list.some((p) => p[0] === play)) play = list[0][0];
  const playOpts = list.map((p) =>
    `<option value="${p[0]}"${p[0] === play ? " selected" : ""}>${esc(p[1])}</option>`).join("");
  host.innerHTML =
    '<div class="pred-card">' +
      `<div class="pred-title"><span class="t">${esc(m.home)} vs ${esc(m.away)}</span><span class="id mono">${esc(m.id)}</span></div>` +
      `<div class="pred-league">${esc(m.league)} · 赔率 ${oddsLine(m)}</div>` +
      '<div class="field" style="max-width:220px;margin-bottom:16px"><label for="playSel">玩法</label>' +
        `<select class="well" id="playSel">${playOpts}</select></div>` +
      '<div class="err" id="prederr" hidden></div>' +
      '<button class="btn btn-primary" id="predbtn" style="width:auto;padding-left:32px;padding-right:32px">预测</button>' +
    '</div>';
  el<HTMLSelectElement>("playSel").addEventListener("change", function () { play = (this as HTMLSelectElement).value; });
  el("predbtn").addEventListener("click", runPredict);
}

function runPredict(): void {
  const m = currentMatch(); if (!m) return;
  el("predHost").innerHTML =
    '<div class="pred-card"><div class="shimmer-label mono">预测中…</div>' +
    '<div class="shimmer-line w50"></div><div class="shimmer-line bar"></div>' +
    '<div class="shimmer-line"></div><div class="shimmer-line w70"></div></div>';
  postJSON<Prediction>("/api/predict", { match: m, play })
    .then((p) => { prediction = p; showPrediction(m, p); })
    .catch((e) => showPredError(e));
}

function showPredError(e: Error): void {
  const msg = /未配置|HTTP 400/.test(e.message)
    ? "尚未配置 AI 模型，请先在左上「AI 配置」中填写 Base URL、模型与 API Key。"
    : "预测失败：" + e.message;
  renderPredEmpty();
  const box = maybe("prederr");
  if (box) { box.textContent = msg; box.hidden = false; }
}

// Build the probability visual. ≤3 outcomes → segmented bar (主胜/平/客胜).
// More (比分, 半场比分…) → ranked list so labels stay readable; show the top
// rows, always include the pick, fold the long tail into one row.
function buildProbViz(options: ProbOption[], pickLabel: string | null | undefined): string {
  if (options.length <= 3) {
    const segs = options.map((o) => {
      const cls = o.label === pickLabel ? "pick" : "rest";
      const w = Math.round((o.prob || 0) * 100);
      return `<div class="prob-seg ${cls}" style="flex-grow:${o.prob || 0}">` +
        `<span class="lab">${esc(o.label)}</span><span class="num">${w}%</span></div>`;
    }).join("");
    return `<div class="prob-bar" id="probBar">${segs}</div>`;
  }

  const TOP = 8;
  let shown = options.slice(0, TOP);
  if (pickLabel && !shown.some((o) => o.label === pickLabel)) {
    const pick = options.filter((o) => o.label === pickLabel)[0];
    if (pick) shown = shown.slice(0, TOP - 1).concat([pick]);
  }
  const rest = options.filter((o) => shown.indexOf(o) === -1);

  const rows = shown.map((o) => {
    const cls = o.label === pickLabel ? " pick" : "";
    const w = Math.round((o.prob || 0) * 100);
    return `<div class="prob-item${cls}">` +
      `<span class="pl-label">${esc(o.label)}</span>` +
      `<span class="pl-track"><span class="pl-fill" style="width:${Math.max(w, 2)}%"></span></span>` +
      `<span class="pl-pct">${w}%</span></div>`;
  });

  if (rest.length) {
    const restProb = rest.reduce((s, o) => s + (o.prob || 0), 0);
    const rw = Math.round(restProb * 100);
    rows.push(`<div class="prob-item">` +
      `<span class="pl-label">其余 ${rest.length} 项</span>` +
      `<span class="pl-track"><span class="pl-fill" style="width:${Math.max(rw, 2)}%"></span></span>` +
      `<span class="pl-pct">${rw}%</span></div>`);
  }
  return `<div class="prob-list" id="probBar">${rows.join("")}</div>`;
}

function showPrediction(m: Match, p: Prediction): void {
  const options = (p.options || []).slice().sort((a, b) => (b.prob || 0) - (a.prob || 0));
  const conf = (p.confidence || 0) * 100;
  const hasOdds = typeof p.pick_odds === "number" && p.pick !== null && p.pick !== undefined;

  const viz = buildProbViz(options, p.pick_label);
  const oddsCell = hasOdds ? (p.pick_odds as number).toFixed(2) : "—";

  const actions = hasOdds
    ? '<div class="pred-actions">' +
        '<div class="stake-field field" style="margin:0"><label for="stake">模拟本金</label>' +
          '<input class="well mono" id="stake" type="number" min="1" step="1" value="100" /></div>' +
        '<button class="btn btn-primary" id="logbtn" style="width:auto;flex:0 0 auto">记一笔</button>' +
        '<button class="btn btn-ghost" id="addCalcBtn" style="width:auto;flex:0 0 auto">加入计算器</button>' +
      '</div>' +
      '<div class="err" id="logerr" hidden></div>'
    : '<div class="pred-rationale" style="border-top:none;padding-top:12px;color:var(--steel)">该玩法为纯预测，不计入账本。</div>';

  el("predHost").innerHTML =
    '<div class="pred-card pred-reveal">' +
      `<div class="pred-title"><span class="t">${esc(m.home)} vs ${esc(m.away)}</span><span class="id mono">${esc(m.id)}</span></div>` +
      `<div class="pred-league">${esc(m.league)} · 模型 ${esc(p.model || "")}</div>` +
      `<div class="prob-wrap">${viz}` +
        '<div class="prob-legend"><span><i class="k" style="background:var(--teal)"></i>建议结果</span>' +
        '<span><i class="k" style="background:#CDD5DF"></i>其余结果</span></div></div>' +
      '<div class="pred-grid">' +
        `<div class="pred-cell"><div class="l">建议结果</div><div class="v teal">${esc(p.pick_label || "—")}</div></div>` +
        `<div class="pred-cell"><div class="l">置信概率</div><div class="v num">${conf.toFixed(1)}%</div></div>` +
        `<div class="pred-cell"><div class="l">选项赔率</div><div class="v num">${oddsCell}</div></div>` +
      '</div>' +
      `<div class="pred-rationale">${esc(p.rationale || "")}</div>` +
      actions +
    '</div>';

  // reveal: clip-path wipe (segmented bar) or scaleX fill (ranked list) —
  // both compositor-only, no per-frame layout. drop will-change after.
  requestAnimationFrame(() => {
    const bar = maybe("probBar");
    if (!bar) return;
    bar.classList.add("shown");
    bar.addEventListener("transitionend", function onEnd() {
      bar.style.willChange = "auto";
      bar.removeEventListener("transitionend", onEnd);
    });
  });

  if (hasOdds) {
    el("logbtn").addEventListener("click", () => {
      const stake = parseFloat(el<HTMLInputElement>("stake").value);
      if (!(stake > 0)) { el<HTMLInputElement>("stake").focus(); return; }
      logBet(m, p.pick as Pick, p.pick_odds as number, stake);
    });
    el("addCalcBtn").addEventListener("click", () => {
      const label = p.pick_label ? `${m.home} ${p.pick_label}` : `${m.home} vs ${m.away}`;
      const errEl = el("logerr");
      if (addCalcLeg(label, p.pick_odds as number)) {
        errEl.hidden = true;
      } else {
        errEl.textContent = `最多 ${MAX_LEGS} 条投注选项,请先删除部分。`;
        errEl.hidden = false;
      }
    });
  }
}

// ---- batch predict — run every queued match through /api/predict with the
// CURRENT play, sequentially (avoid hammering the AI / rate limits). ----
interface BatchRow { home: string; away: string; pickLabel: string; conf: number; oddsTxt: string; odds: number | null; }

async function predictAll(): Promise<void> {
  const btn = el<HTMLButtonElement>("predictAllBtn");
  const host = el("predHost");
  if (!matches.length) {
    host.innerHTML = '<div class="pred-empty"><p>请先加载今天的赛事。</p></div>';
    return;
  }
  const orig = btn.textContent;
  btn.disabled = true;
  const total = matches.length;
  const rows: BatchRow[] = [];
  for (let i = 0; i < total; i++) {
    const m = matches[i];
    btn.textContent = `预测中 ${i + 1}/${total}…`;
    try {
      const p = await postJSON<Prediction>("/api/predict", { match: m, play });
      const hasOdds = typeof p.pick_odds === "number" && p.pick !== null && p.pick !== undefined;
      rows.push({
        home: m.home,
        away: m.away,
        pickLabel: p.pick_label || "—",
        conf: (p.confidence || 0) * 100,
        oddsTxt: hasOdds ? (p.pick_odds as number).toFixed(2) : "—",
        odds: hasOdds ? (p.pick_odds as number) : null,
      });
    } catch (e) {
      const msg = /未配置|HTTP 400/.test((e as Error).message)
        ? "请先在 AI 配置 填写后再预测。"
        : "预测失败：" + (e as Error).message;
      renderBatchResults(rows, msg);
      btn.disabled = false;
      btn.textContent = orig;
      return;
    }
  }
  renderBatchResults(rows, null);
  feedCalcFromBatch(rows);
  btn.disabled = false;
  btn.textContent = orig;
}

// 一键预测后:把有赔率的推荐项按置信度取前 8 自动加入计算器并计算(默认 N串1)
function feedCalcFromBatch(rows: BatchRow[]): void {
  const picks = rows
    .filter((r) => typeof r.odds === "number" && (r.odds as number) > 1)
    .sort((a, b) => b.conf - a.conf)
    .slice(0, MAX_LEGS);
  if (!picks.length) return;
  calcLegs = picks.map((r) => ({ label: `${r.home} vs ${r.away} ${r.pickLabel}`, odds: r.odds as number }));
  calcWays = { [calcLegs.length]: true }; // 默认 N串1
  renderCalc(); // 触发 updateCalcReadout 自动计算
}

function renderBatchResults(rows: BatchRow[], err: string | null): void {
  const sorted = rows.slice().sort((a, b) => b.conf - a.conf);
  const body = sorted.map((r) =>
    '<tr>' +
      `<td><span class="led-team">${esc(r.home)} vs ${esc(r.away)}</span></td>` +
      `<td><span class="led-pick">${esc(r.pickLabel)}</span></td>` +
      `<td class="r num">${r.conf.toFixed(1)}%</td>` +
      `<td class="r num">${esc(r.oddsTxt)}</td>` +
    '</tr>').join("");
  const table = sorted.length
    ? '<table class="ledger"><thead><tr><th>赛事</th><th>推荐</th>' +
      '<th class="r">置信</th><th class="r">赔率</th></tr></thead>' +
      `<tbody>${body}</tbody></table>`
    : "";
  const errBox = err ? `<div class="err" style="margin-bottom:12px">${esc(err)}</div>` : "";
  el("predHost").innerHTML = '<div class="pred-card">' + errBox + table + '</div>';
}

// ---- chat agent — today's matches as context ----
let chatMsgs: ChatMsg[] = [];

function renderChat(): void {
  const log = el("chatLog");
  log.innerHTML = chatMsgs.map((m) => {
    const isUser = m.role === "user";
    const align = isUser ? "flex-end" : "flex-start";
    const bg = isUser ? "var(--teal-dim)" : "var(--raised)";
    const color = isUser ? "var(--teal-text)" : "var(--ink)";
    return `<div style="align-self:${align};max-width:82%;background:${bg};color:${color};` +
      `border:1px solid var(--hairline);border-radius:10px;padding:9px 12px;font-size:13px;` +
      `line-height:1.55;white-space:pre-wrap;word-break:break-word">${esc(m.content)}</div>`;
  }).join("");
  log.scrollTop = log.scrollHeight;
}

async function sendChat(): Promise<void> {
  const input = el<HTMLTextAreaElement>("chatInput");
  const sendBtn = el<HTMLButtonElement>("chatSend");
  const errBox = el("chatErr");
  const content = input.value.trim();
  if (!content) return;
  errBox.hidden = true;
  chatMsgs.push({ role: "user", content });
  renderChat();
  input.value = "";
  sendBtn.disabled = true;

  // transient placeholder while the model thinks
  const placeholder: ChatMsg = { role: "assistant", content: "思考中…" };
  chatMsgs.push(placeholder);
  renderChat();

  try {
    const res = await postJSON<{ reply: string }>("/api/chat", { messages: chatMsgs.slice(0, -1), matches });
    chatMsgs[chatMsgs.length - 1] = { role: "assistant", content: res.reply || "" };
    renderChat();
  } catch (e) {
    chatMsgs.pop(); // drop the placeholder
    renderChat();
    const msg = /未配置|HTTP 400/.test((e as Error).message)
      ? "尚未配置 AI 模型，请先在左上「AI 配置」中填写 Base URL、模型与 API Key。"
      : "对话失败：" + (e as Error).message;
    errBox.textContent = msg;
    errBox.hidden = false;
  } finally {
    sendBtn.disabled = false;
  }
}

// ---- ledger ----
function logBet(m: Match, pick: Pick, odds: number, stake: number): void {
  const logerr = maybe("logerr");
  if (logerr) logerr.hidden = true;
  postJSON("/api/bets", { match_id: m.id, pick, stake, odds })
    .then(() => refreshLedger())
    .catch((e) => {
      const box = maybe("logerr");
      if (box) { box.textContent = "记录失败：" + e.message; box.hidden = false; }
    });
}

function settle(betId: number, actual: SettleResult): void {
  postJSON("/api/settle", { bet_id: betId, actual_result: actual })
    .then(() => refreshLedger())
    .catch((e) => {
      const empty = el("ledgerEmpty");
      empty.textContent = "结算失败：" + e.message;
      empty.hidden = false;
    });
}

function refreshLedger(): void {
  api<Bet[]>("/api/bets").then(renderLedger).catch((e) => {
    const empty = el("ledgerEmpty");
    empty.textContent = "无法加载账本：" + e.message;
    empty.hidden = false;
  });
  api<Ticket[]>("/api/tickets").then(renderTickets).catch(() => { /* best-effort */ });
  api<Stats>("/api/stats").then(renderStats).catch(() => { /* best-effort */ });
}

function pnlFor(b: Bet): number | null {
  if (b.status === "Won") return b.stake * (b.odds_at_bet - 1);
  if (b.status === "Lost") return -b.stake;
  return null;
}

function renderLedger(bets: Bet[]): void {
  const body = el("ledgerBody");
  const has = bets.length > 0;
  el("ledgerTable").hidden = !has;
  el("ledgerEmpty").hidden = has;
  body.innerHTML = "";
  bets.forEach((b) => {
    const tr = document.createElement("tr");
    const statusCls = b.status === "Won" ? "profit" : b.status === "Lost" ? "deficit" : "pending";
    const statusTxt = b.status === "Won" ? "命中" : b.status === "Lost" ? "未中" : "待结算";
    const pnl = pnlFor(b);
    const pnlCell = pnl === null
      ? '<span class="num" style="color:var(--steel)">—</span>'
      : `<span class="num" style="color:${pnl >= 0 ? "var(--profit)" : "var(--deficit)"}">${pnl >= 0 ? "+" : ""}${pnl.toFixed(2)}</span>`;
    const teams = matchMap[b.match_id] || b.match_id;
    const action = b.status === "Pending"
      ? '<div class="settle-btns">' +
          `<button class="btn btn-ghost btn-sm" data-bet="${b.id}" data-res="Home">主胜</button>` +
          `<button class="btn btn-ghost btn-sm" data-bet="${b.id}" data-res="Draw">平</button>` +
          `<button class="btn btn-ghost btn-sm" data-bet="${b.id}" data-res="Away">客胜</button></div>`
      : '<span class="num" style="color:var(--steel);font-size:12px">已结算</span>';
    tr.innerHTML =
      `<td class="mono">${esc(String(b.id))}</td>` +
      `<td><span class="led-team">${esc(teams)}</span><br><span class="mono" style="color:var(--steel);font-size:11px">${esc(b.match_id)}</span></td>` +
      `<td><span class="led-pick">${esc(PICK_LABEL[b.pick] || b.pick)}</span></td>` +
      `<td class="r num">${b.odds_at_bet.toFixed(2)}</td>` +
      `<td class="r num">${b.stake.toFixed(2)}</td>` +
      `<td class="r">${pnlCell}</td>` +
      `<td><span class="status ${statusCls}">${statusTxt}</span></td>` +
      `<td class="r">${action}</td>`;
    body.appendChild(tr);
  });
  body.querySelectorAll<HTMLButtonElement>("[data-bet]").forEach((btn) => {
    btn.addEventListener("click", () => {
      settle(parseInt(btn.getAttribute("data-bet") || "0", 10), btn.getAttribute("data-res") as SettleResult);
    });
  });
}

function renderStats(st: Stats): void {
  const settled = st.settled || 0;
  el("stSettled").textContent = settled + " 笔";
  const hitEl = el("stHit"), pnlEl = el("stPnl"), roiEl = el("stRoi");
  if (!settled) {
    hitEl.textContent = "—"; hitEl.className = "v num draw";
    pnlEl.textContent = "0.00"; pnlEl.className = "v num draw";
    roiEl.textContent = "—"; roiEl.className = "v num draw";
    return;
  }
  const hit = (st.hit_rate || 0) * 100;
  const pnl = st.total_pnl || 0;
  const roi = (st.roi || 0) * 100;
  hitEl.textContent = hit.toFixed(1) + "%";
  hitEl.className = "v num " + (hit >= 50 ? "profit" : "draw");
  pnlEl.textContent = (pnl >= 0 ? "+" : "") + pnl.toFixed(2);
  pnlEl.className = "v num " + (pnl > 0 ? "profit" : pnl < 0 ? "deficit" : "draw");
  roiEl.textContent = (roi >= 0 ? "+" : "") + roi.toFixed(1) + "%";
  roiEl.className = "v num " + (roi > 0 ? "profit" : roi < 0 ? "deficit" : "draw");
}

// ---- 下单计算器 (parlay calculator) ----
const MAX_LEGS = 8;
const UNIT = 2; // 单注基础金额 2 元
let calcLegs: CalcLeg[] = [];
let calcWays: Record<number, boolean> = {}; // chosen 过关方式 by leg-count k

interface CalcResult { n: number; mult: number; ks: number[]; betCount: number; stake: number; maxReturn: number; }

// combinations of `arr` taken `k` at a time → array of sub-arrays
function combos<T>(arr: T[], k: number): T[][] {
  const out: T[][] = [];
  (function pick(start: number, acc: T[]) {
    if (acc.length === k) { out.push(acc.slice()); return; }
    for (let i = start; i < arr.length; i++) { acc.push(arr[i]); pick(i + 1, acc); acc.pop(); }
  })(0, []);
  return out;
}

// compute bet_count, stake, max_return from legs/ways/multiplier
function computeCalc(): CalcResult {
  const n = calcLegs.length;
  const mult = Math.max(1, parseInt(el<HTMLInputElement>("calcMult").value, 10) || 1);
  const odds = calcLegs.map((l) => l.odds);
  const ks = Object.keys(calcWays).map(Number)
    .filter((k) => calcWays[k] && k >= 1 && k <= n)
    .sort((a, b) => a - b);
  let betCount = 0, oddsSum = 0;
  ks.forEach((k) => {
    const cs = combos(odds, k);
    betCount += cs.length;
    cs.forEach((combo) => { oddsSum += combo.reduce((p, o) => p * o, 1); });
  });
  const stake = betCount * UNIT * mult;
  const maxReturn = oddsSum * UNIT * mult;
  return { n, mult, ks, betCount, stake, maxReturn };
}

function addCalcLeg(label: string, odds: number): boolean {
  if (calcLegs.length >= MAX_LEGS) return false;
  calcLegs.push({ label: label || "", odds: odds > 1 ? odds : 1.01 });
  syncDefaultWays();
  renderCalc();
  return true;
}

// populate the 计算器 match dropdown from the current queue, preserving the
// prior selection when that match is still present.
function renderCalcMatch(): void {
  const sel = maybe<HTMLSelectElement>("calcMatch");
  if (!sel) return;
  const prev = sel.value;
  if (!matches.length) {
    sel.innerHTML = '<option value="">先在上方解析或加载赛事</option>';
    renderCalcPlays();
    return;
  }
  sel.innerHTML = matches.map((m) =>
    `<option value="${esc(m.id)}">${esc(m.id + " " + m.home + " vs " + m.away)}</option>`).join("");
  if (prev && matches.some((m) => m.id === prev)) sel.value = prev;
  renderCalcPlays();
}

// 返回某场赛事当前可选的玩法 → 各玩法的结果选项(label+odds)。
// 只列出有真实赔率的玩法;总进球数/半全场无赔率源,需手动添加腿。
function matchPlayOptions(m: Match): Array<{ play: string; label: string; opts: ProbOption[] }> {
  const out: Array<{ play: string; label: string; opts: ProbOption[] }> = [];
  const o = m.odds;
  if (o && o.home > 1 && o.away > 1) {
    const three: ProbOption[] = [{ label: "主胜", prob: o.home }];
    if (o.draw > 1) three.push({ label: "平局", prob: o.draw });
    three.push({ label: "客胜", prob: o.away });
    out.push({ play: "胜平负", label: "胜平负", opts: three });
  }
  const h = m.hhad_odds;
  if (h && h.home > 1 && h.away > 1) {
    const ln = m.hhad_line != null ? `(让${m.hhad_line})` : "";
    const three: ProbOption[] = [{ label: "主胜", prob: h.home }];
    if (h.draw > 1) three.push({ label: "平局", prob: h.draw });
    three.push({ label: "客胜", prob: h.away });
    out.push({ play: "让球胜平负" + ln, label: "让球胜平负", opts: three });
  }
  if (m.pm_score && m.pm_score.length) {
    out.push({ play: "比分", label: "比分", opts: m.pm_score.map((s) => ({ label: s.label, prob: s.odds })) });
  }
  if (m.pm_halftime && m.pm_halftime.length) {
    out.push({ play: "半场", label: "半场", opts: m.pm_halftime.map((s) => ({ label: s.label, prob: s.odds })) });
  }
  return out;
}

function currentCalcMatch(): Match | null {
  const sel = maybe<HTMLSelectElement>("calcMatch");
  return sel && sel.value ? matches.filter((mm) => mm.id === sel.value)[0] || null : null;
}

// 当所选赛事变化:重建玩法下拉
function renderCalcPlays(): void {
  const playSel = maybe<HTMLSelectElement>("calcPlay");
  if (!playSel) return;
  const m = currentCalcMatch();
  const groups = m ? matchPlayOptions(m) : [];
  playSel.innerHTML = groups.length
    ? groups.map((g, i) => `<option value="${i}">${esc(g.play)}</option>`).join("")
    : '<option value="">无可选玩法</option>';
  renderCalcOutcomes();
}

// 当所选玩法变化:重建结果下拉
function renderCalcOutcomes(): void {
  const outSel = maybe<HTMLSelectElement>("calcOutcome");
  const playSel = maybe<HTMLSelectElement>("calcPlay");
  if (!outSel || !playSel) return;
  const m = currentCalcMatch();
  const groups = m ? matchPlayOptions(m) : [];
  const g = groups[+playSel.value];
  outSel.innerHTML = g
    ? g.opts.map((o, i) => `<option value="${i}">${esc(o.label)} @ ${o.prob.toFixed(2)}</option>`).join("")
    : '<option value="">—</option>';
}

// 把当前赛事/玩法/结果加为一条投注腿(混合过关)
function addLegFromMatch(): void {
  const calcErr = el("calcPickErr");
  calcErr.hidden = true;
  const m = currentCalcMatch();
  if (!m) { calcErr.textContent = "请先选择一场赛事。"; calcErr.hidden = false; return; }
  const groups = matchPlayOptions(m);
  const g = groups[+(maybe<HTMLSelectElement>("calcPlay")?.value || -1)];
  const o = g && g.opts[+(maybe<HTMLSelectElement>("calcOutcome")?.value || -1)];
  if (!g || !o) { calcErr.textContent = "该赛事暂无可选玩法。"; calcErr.hidden = false; return; }
  if (!(o.prob > 1)) { calcErr.textContent = "该选项暂无有效赔率。"; calcErr.hidden = false; return; }
  if (!addCalcLeg(`${m.home} vs ${m.away} ${g.label} ${o.label}`, o.prob)) {
    calcErr.textContent = `最多 ${MAX_LEGS} 条投注选项,请先删除部分。`;
    calcErr.hidden = false;
  }
}

// default 过关 selection: N==1 → 1串1; N>=2 → N串1. Preserve still-valid user
// choices; only apply default when nothing valid remains selected.
function syncDefaultWays(): void {
  const n = calcLegs.length;
  const kept: Record<number, boolean> = {};
  Object.keys(calcWays).forEach((k) => { if (calcWays[+k] && +k <= n && +k >= 1) kept[+k] = true; });
  if (!Object.keys(kept).length && n >= 1) kept[n] = true;
  calcWays = kept;
}

function waysLabel(k: number): string { return k + "串1"; }

function renderCalc(): void {
  const host = el("calcLegs"); host.innerHTML = "";
  el("calcLegsEmpty").hidden = calcLegs.length > 0;
  calcLegs.forEach((leg, i) => {
    const row = document.createElement("div");
    row.className = "calc-leg";
    row.innerHTML =
      `<input class="well" type="text" data-ci="${i}" data-cf="label" placeholder="选择/赛事" value="${esc(leg.label)}" />` +
      `<input class="well mono" type="number" min="1.01" step="0.01" data-ci="${i}" data-cf="odds" value="${leg.odds}" />` +
      `<button class="btn btn-ghost btn-sm del" data-cdel="${i}" title="删除">✕</button>`;
    host.appendChild(row);
  });
  host.querySelectorAll<HTMLInputElement>("[data-ci]").forEach((inp) => {
    inp.addEventListener("input", () => {
      const i = +(inp.getAttribute("data-ci") || 0);
      const f = inp.getAttribute("data-cf");
      if (f === "odds") calcLegs[i].odds = parseFloat(inp.value) || 0;
      else calcLegs[i].label = inp.value;
      updateCalcReadout();
    });
  });
  host.querySelectorAll<HTMLButtonElement>("[data-cdel]").forEach((btn) => {
    btn.addEventListener("click", () => {
      calcLegs.splice(+(btn.getAttribute("data-cdel") || 0), 1);
      syncDefaultWays();
      renderCalc();
    });
  });

  // ways checkboxes (k = 1..N)
  const ways = el("calcWays"); ways.innerHTML = "";
  const n = calcLegs.length;
  if (!n) ways.innerHTML = '<span class="calc-empty" style="padding:0">添加投注后选择过关方式。</span>';
  for (let k = 1; k <= n; k++) {
    const lab = document.createElement("label");
    lab.className = "calc-way" + (calcWays[k] ? " on" : "");
    lab.innerHTML = `<input type="checkbox"${calcWays[k] ? " checked" : ""} />${waysLabel(k)}`;
    lab.querySelector("input")!.addEventListener("change", (e) => {
      if ((e.target as HTMLInputElement).checked) calcWays[k] = true; else delete calcWays[k];
      lab.classList.toggle("on", !!calcWays[k]);
      updateCalcReadout();
    });
    ways.appendChild(lab);
  }

  updateCalcReadout();
}

function updateCalcReadout(): void {
  const c = computeCalc();
  el("calcCount").textContent = String(c.betCount);
  el("calcStake").textContent = "¥" + c.stake.toFixed(2);
  el("calcReturn").textContent = "¥" + c.maxReturn.toFixed(2);
  renderSlip(c);
  const ok = c.n > 0 && c.ks.length > 0 && c.betCount > 0 &&
    calcLegs.every((l) => l.odds > 1 && l.label.trim());
  el<HTMLButtonElement>("calcLogBtn").disabled = !ok;
}

function renderSlip(c: CalcResult): void {
  const slip = el("calcSlip");
  if (!c.n || !c.ks.length) { slip.hidden = true; slip.innerHTML = ""; return; }
  slip.hidden = false;
  const legLines = calcLegs.map((l) =>
    `<div class="leg-line"><span>${esc(l.label || "未命名")}</span><span class="lo num">@ ${(l.odds || 0).toFixed(2)}</span></div>`).join("");
  const waysTxt = c.ks.map(waysLabel).join(", ");
  slip.innerHTML =
    "<h3>投注方案</h3>" + legLines +
    '<div class="slip-meta">' +
      `<span>过关 <b>${esc(waysTxt)}</b></span>` +
      `<span>倍数 <b>${c.mult}</b></span>` +
      `<span>注数 <b>${c.betCount}</b></span>` +
      `<span>投注额 <b class="num">¥${c.stake.toFixed(2)}</b></span>` +
      `<span>最高奖金 <b class="num" style="color:var(--teal-text)">¥${c.maxReturn.toFixed(2)}</b></span>` +
    "</div>" +
    '<div class="calc-notice">' +
      '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8"><circle cx="12" cy="12" r="9"/><path d="M12 8v5"/><path d="M12 16.5v.5"/></svg>' +
      "<span>凭此方案到中国体育彩票实体店购买。本工具仅作虚拟记录与复盘，不提供代购、不涉及真实投注。</span>" +
    "</div>";
}

function logTicket(): void {
  const calcErr = el("calcErr");
  calcErr.hidden = true;
  const c = computeCalc();
  if (el<HTMLButtonElement>("calcLogBtn").disabled) return;
  postJSON("/api/tickets", {
    legs: calcLegs.map((l) => ({ label: l.label, odds: l.odds })),
    ways: c.ks,
    multiplier: c.mult,
    bet_count: c.betCount,
    stake: c.stake,
    max_return: c.maxReturn,
  }).then(() => refreshLedger())
    .catch((e) => { calcErr.textContent = "记入失败：" + e.message; calcErr.hidden = false; });
}

// ---- 投注单 (tickets) ----
function settleTicket(id: number, payout: number): void {
  postJSON("/api/tickets/settle", { id, payout })
    .then(() => refreshLedger())
    .catch((e) => alert("结算失败：" + e.message));
}

function renderTickets(tickets: Ticket[]): void {
  const list = tickets || [];
  const body = el("ticketBody");
  el("ticketTable").hidden = !(list.length > 0);
  body.innerHTML = "";
  list.forEach((t) => {
    let legs: CalcLeg[] = [], ways: number[] = [];
    try { legs = JSON.parse(t.legs || "[]"); } catch { legs = []; }
    try { ways = JSON.parse(t.ways || "[]"); } catch { ways = []; }
    const waysTxt = ways.map((k) => k + "串1").join(", ");
    const statusCls = t.status === "Won" ? "profit" : t.status === "Lost" ? "deficit" : "pending";
    const statusTxt = t.status === "Won" ? "命中" : t.status === "Lost" ? "未中" : "待结算";
    const legSummary = legs.map((l) => esc(l.label || "选项") + " @" + (+l.odds || 0).toFixed(2)).join("、");
    const action = t.status === "Pending"
      ? '<div class="settle-btns">' +
          `<input class="well mono" type="number" min="0" step="0.01" placeholder="实际奖金" data-tpay="${t.id}" style="height:32px;width:96px;padding:4px 8px;font-size:12px" />` +
          `<button class="btn btn-ghost btn-sm" data-tid="${t.id}">结算</button></div>`
      : `<span class="num" style="color:var(--steel);font-size:12px">奖金 ¥${typeof t.payout === "number" ? t.payout.toFixed(2) : "0.00"}</span>`;
    const tr = document.createElement("tr");
    tr.innerHTML =
      `<td class="mono">${esc(String(t.id))}</td>` +
      `<td><span class="led-team">${legs.length} 项 · ${esc(waysTxt || "—")}</span>` +
        `<br><span style="color:var(--steel);font-size:11px">${legSummary || "—"}</span></td>` +
      `<td class="r num">${t.multiplier || 1}</td>` +
      `<td class="r num">${(+(t.stake || 0)).toFixed(2)}</td>` +
      `<td class="r num" style="color:var(--teal-text)">${(+(t.max_return || 0)).toFixed(2)}</td>` +
      `<td><span class="status ${statusCls}">${statusTxt}</span></td>` +
      `<td class="r">${action}</td>`;
    body.appendChild(tr);
  });
  body.querySelectorAll<HTMLButtonElement>("[data-tid]").forEach((btn) => {
    btn.addEventListener("click", () => {
      const id = parseInt(btn.getAttribute("data-tid") || "0", 10);
      const inp = body.querySelector<HTMLInputElement>(`[data-tpay="${id}"]`);
      const payout = parseFloat(inp ? inp.value : "");
      if (!(payout >= 0)) { if (inp) inp.focus(); return; }
      settleTicket(id, payout);
    });
  });
}

// ---- 一键导出截图(懒加载 html2canvas)----
async function exportScreenshot(): Promise<void> {
  const btn = el<HTMLButtonElement>("exportBtn");
  const orig = btn.textContent;
  btn.disabled = true; btn.textContent = "导出中…";
  try {
    const { default: html2canvas } = await import("html2canvas");
    const node = (document.querySelector(".wrap") as HTMLElement) || document.body;
    const bg = getComputedStyle(document.body).backgroundColor || "#fff";
    const canvas = await html2canvas(node, { backgroundColor: bg, scale: 2, useCORS: true });
    const url = canvas.toDataURL("image/png");
    const a = document.createElement("a");
    const ts = new Date().toISOString().slice(0, 16).replace(/[:T]/g, "-");
    a.href = url; a.download = `竞彩预测-${ts}.png`;
    a.click();
  } catch (e) {
    alert("导出失败：" + (e as Error).message);
  } finally {
    btn.disabled = false; btn.textContent = orig;
  }
}

// ---- wiring (elements are static in app.html, so bind once at module load) ----
el("exportBtn").addEventListener("click", exportScreenshot);
el<HTMLSelectElement>("protocol").addEventListener("change", () => populateModels());
el<HTMLSelectElement>("modelSel").addEventListener("change", syncCustomVisibility);
el("savekey").addEventListener("click", saveConfig);
el("parsebtn").addEventListener("click", parseMatches);
el("predictAllBtn").addEventListener("click", predictAll);
el("chatSend").addEventListener("click", sendChat);
el<HTMLTextAreaElement>("chatInput").addEventListener("keydown", (e) => {
  if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); sendChat(); }
});
el("polyPrev").addEventListener("click", () => gotoPolyIdx(polyIdx - 1));
el("polyNext").addEventListener("click", () => gotoPolyIdx(polyIdx + 1));
el<HTMLSelectElement>("polyDate").addEventListener("change", function () {
  const v = (this as HTMLSelectElement).value;
  for (let i = 0; i < polyDates.length; i++) {
    if (polyDates[i].date === v) { gotoPolyIdx(i); return; }
  }
});
// source toggle — reset pager, reset 玩法 to the new source's first play,
// reload days (so a stale 体彩-only play doesn't carry into Polymarket).
el<HTMLSelectElement>("srcSel").addEventListener("change", function () {
  source = (this as HTMLSelectElement).value as SourceId;
  play = defaultPlay();
  polyDates = []; polyIdx = 0;
  setMatches([]);
  renderPager();
  loadPolyDates();
  loadStatus();
});
el("calcAddLeg").addEventListener("click", () => addCalcLeg("", 2.00));
el<HTMLSelectElement>("calcMatch").addEventListener("change", renderCalcPlays);
el<HTMLSelectElement>("calcPlay").addEventListener("change", renderCalcOutcomes);
el("calcAddPick").addEventListener("click", addLegFromMatch);
el("calcMult").addEventListener("input", updateCalcReadout);
el("calcLogBtn").addEventListener("click", logTicket);
el("clearLedgerBtn").addEventListener("click", () => {
  if (!confirm("确认清空账本？将删除全部单注、投注单与结算记录，且不可恢复。")) return;
  postJSON("/api/ledger/clear", {}).then(() => refreshLedger())
    .catch((e) => {
      const empty = el("ledgerEmpty");
      empty.textContent = "清空失败：" + e.message;
      empty.hidden = false;
    });
});

// ---- boot ----
tick(); setInterval(tick, 1000);
populateModels();
loadConfig();
loadPolyDates();
loadStatus();
renderMatches(); renderPredEmpty();
renderChat();
renderCalc();
renderCalcMatch();
refreshLedger();
