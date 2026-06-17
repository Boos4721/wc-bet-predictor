#!/usr/bin/env bash
# 一键打包:构建前端 → 嵌入并编译 release 二进制 → 组装可分发产物 → 打成 tar.gz
# 用法:./build.sh
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$ROOT"

BIN_NAME="wc-bet-predictor"
PKG_DIR="dist-package"
VERSION="$(grep -m1 '^version' Cargo.toml | sed -E 's/.*"(.*)".*/\1/')"
OS="$(uname -s | tr '[:upper:]' '[:lower:]')"
ARCH="$(uname -m)"
PKG_NAME="${BIN_NAME}-${VERSION}-${OS}-${ARCH}"

echo "==> [1/4] 构建前端 (web/dist)"
( cd web && bun install && bun run build )

echo "==> [2/4] 编译 release 二进制 (嵌入前端)"
# rust-embed 在编译期嵌入 web/dist;仅改动 dist 文件不会触发 cargo 重编,
# 故先 touch 嵌入源,强制重新编译以纳入最新前端。
touch src/static_assets.rs
cargo build --release

echo "==> [3/4] 组装 ${PKG_DIR}/${PKG_NAME}/"
OUT="${PKG_DIR}/${PKG_NAME}"
rm -rf "$PKG_DIR"
mkdir -p "$OUT"
cp "target/release/${BIN_NAME}" "$OUT/"
cp README.md "$OUT/" 2>/dev/null || true

# 启动脚本:在产物目录内运行二进制(数据文件落在此目录)
cat > "$OUT/run.sh" <<'EOF'
#!/usr/bin/env bash
# 启动竞彩预测终端。数据文件(ledger.db / config.local.json / *_cache.json)
# 写入本目录。打开 http://127.0.0.1:8787
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")"
exec ./wc-bet-predictor
EOF
chmod +x "$OUT/run.sh" "$OUT/${BIN_NAME}"

echo "==> [4/4] 打包 ${PKG_NAME}.tar.gz"
( cd "$PKG_DIR" && tar -czf "${PKG_NAME}.tar.gz" "$PKG_NAME" )

SIZE="$(ls -lh "$PKG_DIR/${PKG_NAME}.tar.gz" | awk '{print $5}')"
echo
echo "完成 ✓"
echo "  目录: ${PKG_DIR}/${PKG_NAME}/   (二进制 + run.sh + README)"
echo "  压缩: ${PKG_DIR}/${PKG_NAME}.tar.gz  (${SIZE})"
echo
echo "运行: ${PKG_DIR}/${PKG_NAME}/run.sh   然后打开 http://127.0.0.1:8787"
