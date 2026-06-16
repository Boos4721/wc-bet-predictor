# 竞彩足球预测 · 模拟账本

本地自用工具:粘贴赛事数据 → 大模型(Claude/OpenAI)预测胜平负 → 虚拟账本记账与盈亏统计。

> 金额均为虚拟,仅用于复盘。不自动登录体彩官网、不提交任何真实投注、不绕过任何站点防护。

## 运行

```bash
# 1. 构建前端
cd frontend && bun install && bun run build
# 2. 起后端(同时托管前端 dist)
cd ../backend && cargo run
# 打开 http://127.0.0.1:8787
```

开发模式(前端热更新):`cd frontend && bun run dev`(Vite 代理 /api 到 8787)。

## 数据格式

- JSON 数组:`[{"id":"周日001","league":"世界杯","home":"A","away":"B","kickoff":"2026-06-20T19:00:00","odds":{"home":2.1,"draw":3.2,"away":3.5},"handicap":null}]`
- 或每行:`场次|联赛|主|客|开赛|主赔|平赔|客赔[|让球]`

## 配置

首屏「AI 配置」填 Base URL / API Key / Model / 协议,保存后写入 `backend/config.local.json`(已在 .gitignore,仅本机明文)。
