# Design System: 竞彩预测 · 模拟账本 (Prediction Terminal)

> Single source of truth for Stitch screen generation. This is a data-forward sports-prediction and virtual-ledger terminal — not a marketing site. Every screen reads like an analyst's cockpit: dense, quiet, numerically precise. Money shown is virtual; the tone is disciplined, never celebratory or casino-like.

## 1. Visual Theme & Atmosphere

A dark, cockpit-dense analytical terminal — the feel of a professional trading desk repurposed for football odds. Calm and high-contrast, with information packed tightly but never cramped: structure comes from thin dividers and negative space, not boxes-within-boxes. The mood is clinical confidence — you are reading signal, not being sold to. No glow, no celebration, no gamification. Wins and losses are stated as facts in color, not dramatized.

- **Density:** Cockpit Dense (8) — odds, probabilities, P&L, ROI all on screen at once
- **Variance:** Offset Asymmetric (5) — left-weighted control rail, right-weighted data; no centered hero
- **Motion:** Static Restrained → Fluid (4) — functional transitions only; data updates settle, they do not bounce

## 2. Color Palette & Roles

- **Abyss Base** (#0B0E13) — Primary background, near-black with blue undertone (never `#000000`)
- **Slate Surface** (#141A24) — Elevated panels, config rail, table container fill
- **Raised Surface** (#1B222E) — Hover rows, active match card, input wells
- **Hairline Border** (rgba(148,163,184,0.14)) — 1px structural dividers, table row rules, panel edges
- **Bright Ink** (#E7EAF0) — Primary text, headlines, key figures
- **Muted Steel** (#8A94A6) — Secondary text, labels, metadata, table headers
- **Signal Teal** (#3DA890) — Single accent: primary CTAs, focus rings, active tab, selected pick (saturation < 80%, no neon)
- **Profit Green** (#3FB37F) — Semantic only: positive P&L, won bets, hit-rate-good
- **Deficit Red** (#D9635B) — Semantic only: negative P&L, lost bets (muted, not alarm-red)
- **Neutral Draw** (#6B7280) — Semantic only: draw outcome, pending status, zero state

Banned: purple/blue neon, gradient text, oversaturated accents, pure black. Semantic green/red are reserved exclusively for money/result outcomes — never for decoration or CTAs.

## 3. Typography Rules

- **Display / UI:** `Geist` — track-tight headlines, weight-driven hierarchy (600 for section heads, 500 for labels). Scale stays controlled; this is a tool, not a billboard.
- **Body:** `Geist` — 400 weight, relaxed leading, 65ch max for rationale/explanation text, rendered in Muted Steel when secondary.
- **Mono (mandatory for all numerics):** `Geist Mono` — odds (2.10 / 3.20 / 3.50), probability percentages, stake, payout, P&L, ROI, timestamps, bet IDs. Density exceeds 7, so every number is monospace and right-aligned in tables.
- **Banned:** `Inter`, all generic serifs (`Times`, `Georgia`, `Garamond`), system-default stacks. Serif is fully banned here — this is a dashboard.

## 4. Component Stylings

* **Buttons:** Flat fill, no outer glow. Primary = Signal Teal fill with Abyss Base text; secondary = ghost with Hairline Border. Tactile -1px translate on `:active`. "解析赛事 / 预测 / 记一笔 / 结算" are all single-purpose — one primary action per zone.
* **Match Cards:** Slate Surface, 12px radius, Hairline Border. Elevation only on the actively-selected match (Raised Surface + 1px Signal Teal left-edge). High-density list rows below use border-top dividers, not stacked cards.
* **Probability Bar:** Horizontal 3-segment fill (主/平/客) — Signal Teal for the predicted pick, Muted Steel for the rest, percentage label in Geist Mono inside each segment. No rounded candy gradients; flat segments, 6px radius on the bar container only.
* **Inputs (config + stake/odds):** Label above in Muted Steel, value in Bright Ink. Input well in Raised Surface, focus ring in Signal Teal. API Key field masked; show "(已保存)" hint inline, never echo the key. Errors inline below in Deficit Red.
* **Ledger Table:** Header row in Muted Steel uppercase-tracked, numerics right-aligned monospace. Status cell colored semantically (Profit Green / Deficit Red / Neutral Draw). Row hover → Raised Surface.
* **Loaders:** Skeletal shimmer matching the prediction card and table dimensions — never a circular spinner. "预测中…" replaces the card body with a 3-line shimmer.
* **Empty States:** Composed, instructive — e.g. ledger empty shows "粘贴赛事 → 预测 → 记一笔，盈亏统计将在结算后出现", not "No data".
* **Stats Strip:** Single inline row — 已结算 N 笔 · 命中率 · 累计盈亏 · ROI — each figure monospace, P&L and ROI colored by sign. No gauge charts, no donut.

## 5. Layout Principles

CSS Grid, max-width 1100px centered. Asymmetric two-zone shell: a left control rail (AI 配置 + 粘贴输入, ~360px) and a right data column (赛事 → 预测 → 账本) that carries the weight. No centered hero. Generous internal padding on panels (24px), tight rhythm inside data tables. Full-height shell uses `min-h-[100dvh]`. Sections separated by Hairline Border and vertical space, not nested card stacks. The banned "3 equal cards" feature row never appears — match results stack as a divider-ruled list.

## 6. Motion & Interaction

Spring physics (`stiffness: 100, damping: 20`) for panel and card entrance; prediction result reveals with a short staggered cascade (probability bar fills left-to-right after the card mounts). Updated ledger figures cross-fade on value change — no count-up theatrics. Restrained loop: the "预测中…" shimmer is the only perpetual animation. Animate `transform` and `opacity` only. No bouncing chevrons, no scroll prompts, no celebratory flashes on a winning settlement — the figure simply turns Profit Green.

## 7. Anti-Patterns (Banned)

- No emojis anywhere
- No `Inter`; no serif fonts (this is a dashboard)
- No pure black (`#000000`) — Abyss Base only
- No neon, outer-glow, or gradient-text effects
- No purple/blue "AI" aesthetic
- No oversaturated accents; semantic green/red reserved for money/results only
- No circular spinners — skeletal loaders only
- No 3-column equal card grids
- No centered hero / centered marketing layout
- No gamified celebration on wins; no casino motifs
- No AI copywriting clichés ("Elevate", "Seamless", "Unleash", "Next-Gen")
- No fake round numbers; show real computed figures (命中率 66.7%, ROI 20.0%)
- No generic placeholder names; use real-format match IDs (周日001) and team labels
- No broken Unsplash links — use SVG marks or `picsum.photos` if imagery is ever needed
- No filler UI text ("Scroll to explore", "Swipe down")
- Reminder banner stays present: amounts are virtual, for review only
