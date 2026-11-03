#!/usr/bin/env bash
# 完整關卡：fmt、clippy、Rust 全部測試（含整合，需要 docker compose 的 Postgres＋MinIO）、前端 vitest 與 build。
set -euo pipefail
cd "$(dirname "$0")/.."

step() { echo; echo "== $*"; }

step "docker compose"
docker compose up -d --wait

step "清掉上次留下的測試資料庫"
docker compose exec -T postgres psql -U energy -d postgres -Atc \
  "select datname from pg_database where datname like 'eo_t_%'" | while read -r d; do
  [ -n "$d" ] && docker compose exec -T postgres psql -U energy -d postgres -qc "DROP DATABASE IF EXISTS $d" >/dev/null
done

# rustfmt 不在共用工具鏈裡（與 ryxis-core 共用，不擅自加元件）；有裝時才檢查。
if cargo fmt --version >/dev/null 2>&1; then
  step "cargo fmt --check"
  cargo fmt --all -- --check
fi

step "cargo clippy"
cargo clippy --workspace --all-targets -- -D warnings

step "cargo test"
cargo test --workspace

if [ -d web ]; then
  step "web: vitest"
  (cd web && { [ -d node_modules ] || npm ci; } && npx vitest --run)
  step "web: build"
  (cd web && npm run build)
fi

echo; echo "gate: all green"
