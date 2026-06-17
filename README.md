# 竞彩足球预测 · 模拟账本

本地自用工具:加载赛事数据 → 大模型(Claude/OpenAI)预测多种玩法 → 下单计算器(含混合过关)+ 虚拟账本记账与盈亏统计 + 对话 Agent。

> 金额均为虚拟,仅用于复盘。不自动登录体彩官网、不提交任何真实投注、不绕过任何站点防护。

## 目录结构

```
.                  Rust 后端(crate 根)
├── src/           axum + rusqlite + 数据源/预测/账本/对话
├── tests/         集成测试
├── examples/      warm.rs(离线缓存预热)
├── web/           前端(Vite + TypeScript)
│   ├── index.html 极简壳(结构由 src/app.ts 注入)
│   └── src/        app.ts(逻辑)/ layout.ts(结构)/ app.css / types.ts
├── build.sh       一键打包脚本
└── .github/workflows/build.yml  多平台 CI
```

## 运行

### 打包成单一二进制(推荐)

前端被 `rust-embed` 嵌入二进制,产物是一个自包含可执行文件,可在任意目录运行:

```bash
cd web && bun install && bun run build   # 1. 构建前端(产出 web/dist,编译时被嵌入)
cd .. && cargo build --release           # 2. 编译 release(嵌入 dist)
./target/release/wc-bet-predictor        # 3. 运行,打开 http://127.0.0.1:8787
```

或一键打包:`./build.sh` → 产出 `dist-package/<name>.tar.gz`(二进制 + run.sh + README)。

> rust-embed 在编译期嵌入 `web/dist`;仅改前端需 `touch src/static_assets.rs` 触发重编(build.sh 已内置)。

二进制在当前工作目录读写 `ledger.db`、`config.local.json`、`poly_cache.json`、`sporttery_cache.json`。

### 开发模式

```bash
cargo run            # debug:rust-embed 从磁盘读 web/dist
cd web && bun run dev  # 前端热更新(Vite 代理 /api 到 8787)
```

## 功能

- **数据源**:体彩官方(`getMatchListV1`,仅世界杯,胜平负 + 让球)/ Polymarket(世界杯 series,输赢/比分/半场)。后端内存+磁盘缓存,15 分钟定时刷新;前端按比赛日分页。手动粘贴:JSON 数组或 `场次|联赛|主|客|开赛|主赔|平赔|客赔[|让球]`。
- **预测玩法**:胜平负/让球/比分/总进球/半全场/半场(随数据源可用项)。单场预测 + 一键预测全部。
- **下单计算器**:从赛事列表选玩法+结果加为投注选项,支持复式与混合过关,实时算注数/投注额/最高奖金,可记入账本;附线下购买提示(仅虚拟记录)。
- **虚拟账本**:单注 + 投注单(过关),结算后统计命中率/累计盈亏/ROI,可清空。
- **对话 Agent**:带当日赛事上下文的中文问答。
- **一键导出截图**:整页导出 PNG(html2canvas)。

## 配置

「AI 配置」填 Base URL / API Key / Model / 协议,保存后写入 `config.local.json`(已 gitignore,仅本机明文,不回显 Key)。
