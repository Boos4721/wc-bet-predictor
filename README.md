# 竞彩足球预测 · 模拟账本

本地自用工具:粘贴赛事数据 → 大模型(Claude/OpenAI)预测胜平负 → 虚拟账本记账与盈亏统计。

> 金额均为虚拟,仅用于复盘。不自动登录体彩官网、不提交任何真实投注、不绕过任何站点防护。

## 运行

### 打包成单一二进制(推荐)

前端会被 `rust-embed` 嵌入到二进制中,产物是一个自包含可执行文件,可在任意目录运行(无需 `frontend/dist`):

```bash
# 1. 构建前端(产出 frontend/dist,编译时被嵌入)
cd frontend && bun install && bun run build
# 2. 编译 release 二进制(嵌入 dist)
cd ../backend && cargo build --release
# 3. 运行(可拷贝到任意位置)
./target/release/wc-bet-predictor
# 打开 http://127.0.0.1:8787
```

二进制启动时会在当前工作目录读写 `ledger.db`、`config.local.json`、
`poly_cache.json`、`sporttery_cache.json`。

### 开发模式

```bash
cd backend && cargo run        # debug:rust-embed 从磁盘读 ../frontend/dist
cd frontend && bun run dev     # 前端热更新(Vite 代理 /api 到 8787)
```

## 数据来源

- **体彩官方**:`getMatchListV1` 公开接口,仅世界杯赛事,胜平负 + 让球赔率。
- **Polymarket**:世界杯比赛 series(三路盘)。
- 两者后端缓存(内存 + 磁盘快照)+ 15 分钟定时刷新;前端按比赛日分页。
- 手动粘贴:JSON 数组,或每行 `场次|联赛|主|客|开赛|主赔|平赔|客赔[|让球]`。

## 配置

首屏「AI 配置」填 Base URL / API Key / Model / 协议,保存后写入 `config.local.json`(已在 .gitignore,仅本机明文,不回显 Key)。
