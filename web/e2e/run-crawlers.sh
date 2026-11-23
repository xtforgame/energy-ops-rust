#!/usr/bin/env bash
# e2e crawler 驗收：每支 crawler 都在一個重建過的專用資料庫上跑（示範資料重灌），
# 各自起一個假辨識服務與一個後端，跑完收掉，最後印出通過／未通過的總表。
#
#   web/e2e/run-crawlers.sh                       # 全部
#   web/e2e/run-crawlers.sh flowgraph mywork      # 指定幾支
#
# 環境變數：
#   E2E_DB            專用資料庫名（預設 eo_e2e；只會 DROP／CREATE 這一個；也接受 energy_ops_cardf_* 開頭）
#   E2E_S3_BUCKET     MinIO bucket（預設 energy-files；不帶時行為不變）
#   E2E_API_PORT      後端固定埠（預設隨機找空埠）
#   E2E_TPR_PORT      假辨識服務固定埠（預設隨機找空埠）
#   CARGO_TARGET_DIR  後端 binary 所在（預設 <repo>/target）
#   E2E_SKIP_BUILD=1  不重建後端與 web/dist（直接用現有的）
#   CRAWLER_HEADFUL=1 開著瀏覽器視窗跑
#   E2E_TPR_POLLS     假辨識服務回幾次「處理中」才完成（預設 3：辨識中的畫面要看得到）
# 前置：docker compose 的 Postgres（56432）與 MinIO（56900）起著。
set -u
cd "$(dirname "$0")/../.."
ROOT=$(pwd)

ALL=(audit-i18n flowgraph mywork queue binding executions doctype modes recognition journey roles tenant-console)
if [ $# -gt 0 ]; then CRAWLERS=("$@"); else CRAWLERS=("${ALL[@]}"); fi

DB=${E2E_DB:-eo_e2e}
case "$DB" in eo_e2e*|energy_ops_cardf_*|energy_ops_cardf2_*|energy_ops_cardg1_*|energy_ops_cardg2_*|energy_ops_cardg3_*|energy_ops_cardg4_*) ;; *) echo "E2E_DB 必須以 eo_e2e 或 energy_ops_cardf_ 開頭（避免誤刪別的資料庫）：$DB" >&2; exit 2 ;; esac
TARGET=${CARGO_TARGET_DIR:-$ROOT/target}
BIN=$TARGET/debug/energy-ops
SHOTS=$ROOT/web/e2e/shots
mkdir -p "$SHOTS"
SUMMARY=$SHOTS/summary.txt
: > "$SUMMARY"

if [ "${E2E_SKIP_BUILD:-0}" != 1 ]; then
  echo "== 建後端"; CARGO_TARGET_DIR=$TARGET cargo build -q || exit 1
  echo "== 建 web/dist"; (cd web && npm run -s build >/dev/null) || exit 1
fi
[ -x "$BIN" ] || { echo "找不到 $BIN" >&2; exit 1; }

free_port() { node -e 'const s=require("net").createServer();s.listen(0,"127.0.0.1",()=>{console.log(s.address().port);s.close()})'; }
psql_admin() { docker compose exec -T postgres psql -U energy -d postgres -v ON_ERROR_STOP=1 -qAt "$@"; }

wait_http() { # url, 秒
  for _ in $(seq 1 "$2"); do curl -sf -o /dev/null "$1" && return 0; sleep 1; done; return 1
}

pids=()
stop_all() { for p in "${pids[@]:-}"; do [ -n "$p" ] && kill "$p" 2>/dev/null; done; for p in "${pids[@]:-}"; do [ -n "$p" ] && wait "$p" 2>/dev/null; done; pids=(); }
trap 'stop_all; exit 130' INT TERM

total_fail=0
for c in "${CRAWLERS[@]}"; do
  SCRIPT=$ROOT/web/e2e/crawlers/$c.ts
  [ -f "$SCRIPT" ] || { echo "$c: 找不到 $SCRIPT" | tee -a "$SUMMARY"; total_fail=1; continue; }
  OUT=$SHOTS/$c
  rm -rf "$OUT"; mkdir -p "$OUT"

  # 有幾支要從收件台真的走一遍（單據留白）；其餘灌四份範本帳單與它們的流程。
  case "$c" in binding|executions|queue|roles|journey) samples=0 ;; *) samples=1 ;; esac

  psql_admin -c "DROP DATABASE IF EXISTS $DB WITH (FORCE)" -c "CREATE DATABASE $DB" >/dev/null || { echo "$c: 重建資料庫失敗" | tee -a "$SUMMARY"; total_fail=1; continue; }

  TPR_PORT=${E2E_TPR_PORT:-$(free_port)}; API_PORT=${E2E_API_PORT:-$(free_port)}
  "$BIN" fake-tpr "$TPR_PORT" "${E2E_TPR_POLLS:-3}" > "$OUT/fake-tpr.log" 2>&1 & pids+=($!)
  env -i PATH="$PATH" HOME="$HOME" RUST_LOG=info \
    EO_DATABASE_URL="postgres://energy:energy@127.0.0.1:${EO_PG_PORT:-56432}/$DB" \
    EO_S3_ENDPOINT="http://127.0.0.1:${EO_MINIO_PORT:-56900}" EO_S3_REGION=us-east-1 EO_S3_BUCKET="${E2E_S3_BUCKET:-energy-files}" \
    EO_S3_ACCESS_KEY=energyminio EO_S3_SECRET_KEY=energyminio-secret \
    EO_S3_KEY_PREFIX="e2e/$c-$(date +%s)/" \
    EO_HTTP_PORT="$API_PORT" EO_WEB_DIR="$ROOT/web/dist" \
    EO_TPR_URL="http://127.0.0.1:$TPR_PORT" EO_RECOGNITION_TICK_SECS=2 \
    EO_BOOTSTRAP_ADMIN_USER=admin@example.com EO_BOOTSTRAP_ADMIN_PASSWORD=admin12345 \
    EO_DEMO_SEED=1 EO_DEMO_SAMPLE_BILLS=$samples EO_DEMO_SAMPLES_DIR="$ROOT/tests/fixtures/samples" \
    "$BIN" > "$OUT/backend.log" 2>&1 & pids+=($!)

  wait_http "http://127.0.0.1:$TPR_PORT/health" 30 || { echo "$c: 假辨識服務起不來" | tee -a "$SUMMARY"; stop_all; total_fail=1; continue; }
  if ! wait_http "http://127.0.0.1:$API_PORT/health" 90; then
    echo "$c: 後端起不來（見 $OUT/backend.log）" | tee -a "$SUMMARY"; stop_all; total_fail=1; continue
  fi

  t0=$(date +%s)
  (cd web && E2E_BASE_URL="http://127.0.0.1:$API_PORT" E2E_SHOTS_DIR="$OUT" E2E_DB="$DB" \
    node "$SCRIPT") > "$OUT/crawler.log" 2>&1
  ec=$?
  secs=$(( $(date +%s) - t0 ))
  stop_all
  # 計分板最後印一行 E2E_RESULT；中途 throw 沒走到那一行就退回數勾叉。
  res=$(grep -o "E2E_RESULT pass=[0-9]* fail=[0-9]*" "$OUT/crawler.log" | tail -1)
  if [ -n "$res" ]; then p=${res#*pass=}; p=${p%% *}; f=${res##*fail=}
  else p=$(grep -c "  ✅ " "$OUT/crawler.log"); f=$(( $(grep -c "  ❌ " "$OUT/crawler.log") + 1 )); fi
  status=PASS; { [ $ec -ne 0 ] || [ "$f" -gt 0 ]; } && { status=FAIL; total_fail=1; }
  echo "$c $status exit=$ec pass=$p fail=$f secs=$secs samples=$samples" | tee -a "$SUMMARY"
done

echo; echo "== 總表（${SUMMARY}）"; cat "$SUMMARY"
exit $total_fail
