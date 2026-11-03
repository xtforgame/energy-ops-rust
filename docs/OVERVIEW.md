# energy-ops-rust 說明文件

雲豹能源的營運租戶後台：把散在 Excel 的地主、案場、合約、台電電費單、分潤收進系統，
每張電費單走「上傳 → 辨識 → 校對 → 逐關核可 → 算應付 → 付款」。
這個 repo 是它的 Rust 版：後端單一 binary，外部只依賴 Postgres 與 MinIO；前端沿用原本的 react-admin 頁面。

狀態（2026-11-04）：功能完整、測試全綠，在 branch `rust-port`，尚未 push、尚未併 master。

---

## 1. 從哪裡來

| 範圍 | 規格來源 |
|---|---|
| 能源業務（端點、資料表、規則） | App 5（Supabase 版，最新規格），commit `85631c6` |
| 多組織、RBAC、登入、邀請 | az repo master 的 Multitenancy／Rbac／Authn／TenantConsole plugin |
| 能源原版與設計文件、e2e crawler | az repo `feat-tenant-console-route-transition`（`0afe402aba`） |
| RBAC 管理頁 | az repo raadmin5 |

細節與 fixture 的 sha256 在 `PROVENANCE.md`；當初的調研與決定在 `docs/PLAN.md`。

## 2. 起服務

```sh
docker compose up -d            # Postgres 18（56432）＋MinIO（56900，console 56901）
./dev-run.sh --demo             # 後端 :10950 ＋ 前端 :5190，灌示範資料
cargo run -- fake-tpr 18080     # （另開）假的辨識服務；再以 EO_TPR_URL=http://127.0.0.1:18080 起後端
```

| 帳號 | 密碼 | 用途 |
|---|---|---|
| `admin@example.com` | `admin12345` | 超級使用者（`EO_BOOTSTRAP_ADMIN_*`） |
| `admin@demo.energy` | `demo12345` | 示範組織的系統管理員 |
| `operator@`／`manager@`／`cxo@`／`finance@`／`accounting@`／`viewer@demo.energy` | `demo12345` | 承辦、主管、高層、財務、會計、唯讀 |

正式部署：`cargo build --release`，設定 `.env.example` 列的環境變數，`EO_WEB_DIR` 指到 `web/dist`（`cd web && npm run build`），
啟動時自動套 migration、寫 RBAC seed、建 bucket。

## 3. 架構

```
src/                 組裝根：build_system（註冊 plugin）、configure_app、binary
crates/eo-domain     純邏輯，不碰 IO
crates/eo-platform   plugin 框架與平台
crates/eo-energy     能源 plugin
migrations/          sqlx migration（編進 binary）
web/                 前端（Vite＋React 19＋react-admin＋MUI）
tests/               整合測試（每條自己開一個資料庫）
```

- **plugin**：照 ryxis-core 的 azldi。每個 plugin 實作 `AppPlugin`：`configure_routes`、`on_start`、`on_destroy`、`rbac_seeds`。
  依賴與順序用 `deps`／`run_before` 宣告，開機時拓撲排序。`/health` 列出實際註冊的全部路由。
- **型別閘**：handler 拿到 `OrgAccess`（組織）、`ProjectAccess`、`ResourceAccess`、`ExecutionAccess`（流程單可視度）的唯一路是判權函式；
  資料存取函式都收這些型別、從它取組織條件。漏判權或漏組織條件編不過。
- **交易**：業務變更與稽核寫在同一個交易（稽核寫不進去整筆回滾）；授權變更用 `Rbac::commit` 提交後才讓權限快取失效。
- **時間**：一律取 `core.clock`（測試可快轉）；「今天」以台灣時間計。
- **依賴方向**：`tests/layer_guard.rs` 守住 eo-domain 不碰 IO、eo-platform 不依賴 eo-energy。

## 4. 平台功能（eo-platform）

| plugin | 內容 |
|---|---|
| database／storage／health／static_web | 連線池與 migration、MinIO（自寫 SigV4，含 presign）、健康檢查、供應前端（SPA fallback） |
| authn | 帳密登入（bcrypt cost 10，與 az 原版互通）、DB 存的 session token（cookie `eo_session` 或 Bearer）、閒置 14 天滑動、絕對 183 天、登出即失效、註冊需憑邀請 |
| user | 個人資料、改密碼（撤其他 session）、超級使用者的帳號管理 |
| multitenancy | 組織、專案（每個組織一個 lobby）、個人組織、成員、邀請（組織／專案入口、preset、token、未註冊者先註冊再接受） |
| rbac | permission（simple／complex）、角色、三層 scope 授權、超級使用者、行程內快取、seed 收集與驗證、自訂角色與 permission（可屬於某組織）、最後一位管理者防呆、`/me/permissions` 等 |

平台權限：`platform:rbac:manage`（萬能鑰匙，所有閘都接受）、`platform:org:manage`、`platform:member:manage`、`platform:project:manage`；
角色 `platform:org-admin`（建組織者自動持有）、`platform:project-admin`。

## 5. 能源功能（eo-energy，前綴 `/api/v1/orgs/{orgId}/energy`）

| 模組 | 內容 |
|---|---|
| audit | 56 種事件 registry（payload 白名單、severity）、寫入與查詢 |
| org | 部門與部門成員、能源成員清單、角色指派（五角色；指派管理者要特權、最後一位治理者防呆） |
| master_data | 地主、案場、總覽頁資料、地理編碼（mock） |
| bindings／rating | 流程與計費的效期綁定（綁、換版、解除、預覽）；計費策略與定點數計算、不可變的應付紀錄 |
| workflow | 流程引擎：人工關卡、計算節點、五種動作（推進／回退／設狀態／副作用／中斷）、並行、回退鎖、中斷核可、代理、轉移、強制推進、終止、血緣；可視度、模式判定、清單、流程圖；`setup/install-defaults` 安裝內建流程 |
| workbench | 工作台、登入落點、儀表板、行事曆 |
| attachments | 先存 MinIO 驗過才寫 DB、sha256、上限、換版、只解除連結不刪、RFC 5987 檔名、presign 直傳 |
| ingest | 收件批次、電費單三層值（辨識原始／抽取／最終）、校對、去重、送審開流程、例外件、辨識驅動（背景任務、租約、分批、時間預算） |
| demo_seed | `EO_DEMO_SEED=1` 建示範組織；`EO_DEMO_SAMPLE_BILLS=1` 另灌四份範本電費單 |

能源五角色：系統管理員、高階主管、業務承辦、財務、唯讀（`energy:admin|cxo|operator|finance|viewer`）。
完整路由與形狀見 `docs/API-ENERGY.md`。

辨識服務：正式環境接外部的 taipower-recognition（`EO_TPR_URL`）；`energy-ops fake-tpr` 是講同一協定的假服務，
依檔名回四份真範本的辨識結果，給開發與測試用。

## 6. 前端（web/）

- react-admin＋MUI 的能源頁沿用 App 5；外殼由 Next.js 換成 Vite，建置後由後端供應。
- 新增：多組織／專案切換器（記住上次選擇、吃 `?org=`）、成員與邀請、接受邀請（公開頁）、專案、RBAC 管理（角色、permission、授權）、個人資料、超級使用者帳號管理、成員的能源角色指派。
- 所有能源呼叫經 `web/src/api/energy.ts`（路由表＋回應轉換，頁面拿到的仍是原本的形狀）。舊→新對照在 `web/ENERGY_API_CALLS.md`。

## 7. 測試

| 層 | 指令 | 數量 |
|---|---|---|
| Rust（純邏輯、平台、能源整合、layer guard） | `cargo test --workspace` | 266 |
| 前端單元 | `cd web && npx vitest --run` | 173 |
| 完整關卡 | `./scripts/gate.sh`（clippy、上兩者、前端 build；約 30 秒） | — |
| e2e crawler（真瀏覽器，12 支，移植自 az） | `web/e2e/run-crawlers.sh [名稱…]`（全部約 14 分鐘） | 595 條斷言 |

crawler 每支在重建的專用資料庫 `eo_e2e` 上跑，自起假辨識服務與後端；截圖與 log 在 `web/e2e/shots/`。
整合測試用 docker compose 的 Postgres 與 MinIO，每條自開資料庫、結束自刪。

## 8. 刻意與原版不同的地方

- API 重新設計：`/api/v1/...`、camelCase、金額為 decimal 字串、清單 `{items}`、錯誤 `{code, message, details}`。
- session 存在 DB（不用 JWT）；萬能鑰匙改名 `platform:rbac:manage` 並正式 seed。
- 修掉的原版缺陷：部門依 key 查沒限定組織（跨租戶）、副作用可重複套用（連按兩下記兩筆付款）、工作台「被退回」區永遠是空的、
  geocode 稽核在交易外、節點待辦的 `claimable` 語意不一致、成員頁沒有能源角色指派入口、登出換人登入會落在上一個人的頁面。
- 沒搬：App 5 的 RLS 與 plpgsql 判權（判定在 Rust）、pg_cron（改背景任務）、辨識的「平台模式」（Supabase 部署專用）、標記回報工具、解說影片腳本。
- 逐項差異記在 `docs/API-ENERGY.md` 各節與各模組開頭註解。

## 9. 已知限制與後續

- rustfmt 不在共用工具鏈裡，gate 只在有裝時檢查格式。
- 前端主 bundle 約 1.7MB（gzip 540KB），可再拆。
- 主檔（地主、案場、合約）與流程定義、計費策略沒有建立／編輯畫面，與原版相同（靠匯入或 seed）。
- 多個流程綁在同一案場時，分支預覽取第一條（照 App 5）。
- 解說影片腳本沒有移植（使用者決定不需要）。
