#!/usr/bin/env bash
# 開發用：起 Postgres＋MinIO（docker compose）、後端（cargo run）、前端（vite dev server）。
#   ./dev-run.sh              # 後端 10950、前端 5190
#   ./dev-run.sh --fresh      # 先清掉資料庫與物件（docker volume）
#   ./dev-run.sh --no-web     # 只起後端
#   ./dev-run.sh --demo       # 灌示範資料（EO_DEMO_SEED=1）
set -euo pipefail
cd "$(dirname "$0")"

FRESH=0; WEB=1; DEMO=0
for a in "$@"; do
  case "$a" in
    --fresh) FRESH=1 ;;
    --no-web) WEB=0 ;;
    --demo) DEMO=1 ;;
    *) echo "unknown option: $a" >&2; exit 2 ;;
  esac
done

[ -f .env ] || cp .env.example .env
if [ "$FRESH" = 1 ]; then docker compose down -v; fi
docker compose up -d --wait

export EO_BOOTSTRAP_ADMIN_USER="${EO_BOOTSTRAP_ADMIN_USER:-admin@example.com}"
export EO_BOOTSTRAP_ADMIN_PASSWORD="${EO_BOOTSTRAP_ADMIN_PASSWORD:-admin12345}"
[ "$DEMO" = 1 ] && export EO_DEMO_SEED=1

pids=()
cleanup() { for p in "${pids[@]}"; do kill "$p" 2>/dev/null || true; done; }
trap cleanup EXIT INT TERM

cargo run &
pids+=($!)

if [ "$WEB" = 1 ] && [ -d web ]; then
  (cd web && { [ -d node_modules ] || npm ci; } && npm run dev -- --port "${EO_WEB_PORT:-5190}" --strictPort) &
  pids+=($!)
  echo "前端：http://127.0.0.1:${EO_WEB_PORT:-5190}  （登入 ${EO_BOOTSTRAP_ADMIN_USER} / ${EO_BOOTSTRAP_ADMIN_PASSWORD}）"
fi
wait
