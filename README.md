# energy-ops-rust

能源營運租戶後台（地主／案場／合約／台電電費單 → 辨識 → 校對 → 逐關核可 → 算應付）的 Rust 版。
後端是單一 binary（actix-web＋sqlx），以 ryxis-core 的 azldi plugin 方式管理 API；外部服務只有 Postgres 與 MinIO。
前端在 `web/`（Vite＋React＋react-admin＋MUI），建置後由後端供應。

## 起服務

```sh
docker compose up -d          # Postgres 18（56432）＋ MinIO（56900，console 56901）
./dev-run.sh                  # 後端 http://127.0.0.1:10950 ＋ 前端 http://127.0.0.1:5190
./dev-run.sh --fresh          # 清掉資料庫與物件重來
./dev-run.sh --demo           # 灌示範資料
```

預設建一個超級使用者 `admin@example.com` / `admin12345`（`EO_BOOTSTRAP_ADMIN_USER`／`EO_BOOTSTRAP_ADMIN_PASSWORD`）。
設定都在環境變數，範本見 `.env.example`。

辨識服務：正式環境指向真的 taipower-recognition（`EO_TPR_URL`）；本機可以起假的：

```sh
cargo run -- fake-tpr 18080   # 依檔名回四份真範本的辨識結果
EO_TPR_URL=http://127.0.0.1:18080 ./dev-run.sh
```

## 測試

```sh
cargo test --workspace        # 單元＋整合（整合測試每條自己開一個資料庫，需要 docker compose）
./scripts/gate.sh             # 完整關卡：clippy、全部測試、前端 vitest 與 build
```

## 結構

| 路徑 | 內容 |
|---|---|
| `crates/eo-domain` | 純邏輯（單別判定、欄位抽取、計費、流程條件、模式判定、稽核事件 registry、RBAC seed 型別）；不碰 IO |
| `crates/eo-platform` | azldi＋AppPlugin、設定、物件儲存（SigV4）、session 登入、多組織、邀請、RBAC |
| `crates/eo-energy` | 能源 plugin（前綴 `/api/v1/orgs/{orgId}/energy`） |
| `src/` | 組裝根（`build_system`、`configure_app`）與 binary |
| `migrations/` | sqlx migration，編進 binary、開機自動套用 |
| `tests/` | 整合測試（`tests/all.rs` 聚合）、layer guard |
| `web/` | 前端 |
| `docs/PLAN.md` | 計畫與決定 |
| `docs/API-ENERGY.md` | 能源 API 契約 |
| `PROVENANCE.md` | 規格來源 commit 與 fixture |
