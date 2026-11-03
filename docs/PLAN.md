# energy-ops-rust 計畫

能源營運租戶後台（雲豹能源：地主／案場／合約／台電電費單 → 辨識 → 校對 → 逐關核可 → 算應付）的 Rust 後端版。
前端沿用現有的 react-admin＋MUI 頁面；後端照 ryxis-core 的 azldi plugin 方式管理 API。

## 0. 已定案（使用者 2026-11-04）

| # | 議題 | 決定 |
|---|---|---|
| D1 | 組織範圍 | 多組織，最完整的 RBAC：org／project／resource 三層 scope、complex permission、自訂角色與 permission（後端＋前端管理頁）、邀請 |
| D2 | API 相容 | 不追舊形狀，重新設計乾淨的 API；前端加一層 typed API client 改接 |
| D3 | DB 層 | sqlx（理由見 §3） |
| D4 | 前端 | 保留 react-admin＋MUI；Next.js 殼換成 Vite，由後端 binary 供應 |
| D5 | 流程 | 輕量：PLAN／DECISIONS 加 cargo test 與一支 gate 腳本，不用 azd 工作項卡 |
| D6 | 部署 | 單一 binary＋Postgres＋MinIO；presign 直傳是 nice to have |
| D7 | 帳號與密碼 | 不從舊系統遷移資料，但密碼系統照 az 原版不改：bcrypt、cost 10（az `src/server/domain-logic/common.ts` 的 `hashPassword`／`comparePasswordHash`），雜湊存在 account link 的 `provider_user_access_info.password`，帳號模型沿用 `accountLink`（provider_id、provider_user_id、provider_user_access_info、identifiers）。Rust 端用 `bcrypt` crate，產出與驗證 `$2a$`／`$2b$` 格式都和 node `bcrypt` 互通。曾考慮 argon2，但 Postgres 的 pgcrypto 沒有 argon2（只有 bf／md5／xdes／des 與 sha256crypt／sha512crypt），使用者決定不改 |

## 1. 規格來源

| 範圍 | 以誰為準 | 位置 |
|---|---|---|
| 能源業務（端點、表、規則） | App 5（Supabase 版，最新） | `../az-fullstack-sequelize/temp/tenant-console-compare-20261104/3-online-energy-ops-app`（main `85631c6`） |
| 能源業務的原版與設計文件 | az repo EnergyOpsPlugin | az repo `feat-tenant-console-route-transition`（`0afe402aba`）：`src/server/plugins/EnergyOpsPlugin`、`docs/features/energy-ops-console/*` |
| 組織、RBAC、登入、邀請 | az repo master 既有的 plugin | `MultitenancyPlugin`、`RbacPlugin`、`AuthnPlugin`、`TenantConsolePlugin`；設計在 `docs/features/rbacHardening/*`、`docs/features/ryxis-rbac/*` |
| RBAC 管理頁 | az repo raadmin5 | `src/raadmin5/src/domain-logic/components/{rbacPermissions,rbacRoles,userRbacRoles}` |
| 前端能源頁 | App 5 的 `web/` | `web/src/admin/domain-logic/components/energyOps/*` |
| 多組織前端（選擇器、成員、邀請） | 1-route-transition | `src/layout/ProjectSelector.tsx`、`invitations/*` |
| 辨識服務 | 外部 `taipower-recognition`，不搬 | az repo branch 的 `taipower-recognition/`（API：`api/openapi.yaml`） |

2-energy-ops-next 沒有能源功能（通用租戶後台＋serverless），不作為規格來源。

### 不照抄的已知瑕疵

- App 5 `assigneeHintOf` 依部門 key 查部門沒限定組織（跨租戶）。
- 所有能源閘都 OR 上 `ryxis:rbac:manage`，App 5 卻沒 seed 它；本版改成明確的平台權限 `platform:rbac:manage`，並由 seed 宣告。
- `displayId.ts` 註解稱 `execution.documentNo` 是正規欄位，schema 沒有。

## 2. 架構

### 2.1 Plugin（照 ryxis-core `src/azldi`、`src/plugins/mod.rs`）

基底 `Plugin`（`name`、`init_deps`、`init`、`deps`、`run_before`）＋衍生 `AppPlugin`：

- `configure_routes(&self, &mut RouteRegistrar, &Arc<dyn AppPlugin>)`：登記路由（`RouteTable` 記下真的註冊了哪些路徑，health 會列出）
- `async on_start(&self, &StartContext) -> Result<(), StartError>`：連 DB、套 migration、啟背景任務
- `rbac_seeds(&self) -> RbacSeeds`：本 plugin 宣告的 permission／complex／role；`rbac` plugin 開機時收集、驗證（跨 plugin 不重名、complex 不包 complex、role 與 permission 同 scope）、冪等 upsert，只增不刪
- `background_tasks`：在 `on_start` 裡 spawn，跟著 graceful shutdown 收

| 類別 | plugin | 內容 |
|---|---|---|
| 基礎 | `config` | 設定檔＋環境變數 |
| | `database` | sqlx `PgPool`；`sqlx::migrate!` 編進 binary，開機自動套 |
| | `storage` | MinIO／S3：put、get、head、delete，之後加 presign |
| | `http_server` | actix-web；錯誤統一成 `{code, message, details}` |
| | `health` | `GET /health` 列出路由與 DB、MinIO 狀態 |
| | `static_web` | 供應 `web/dist`，SPA fallback |
| 平台 | `user` | 使用者、個人資料、改密碼 |
| | `authn` | 帳密登入（bcrypt，照 az `BasicAuthnProvider`）、JWT（cookie 或 Bearer）、滑動續期（閒置 14 天、剩一半重簽、絕對上限 183 天以 sessionStart 為錨）、登出 |
| | `multitenancy` | org、project（每個 org 一個 lobby）、個人 org、成員、邀請（org／project 兩層 preset、token、效期、接受時同交易建成員再授權） |
| | `rbac` | 表與 seed manager、resolver（super-user 讀 DB、三層涵蓋、complex 展開一層、OR 語意）、版本戳快取（行程內，commit 後才失效）、grant／revoke service、自訂 permission／role CRUD（`custom:` 前綴、system／plugin 列不可改、被引用不可刪）、`/me/permissions`、`/me/check`、`/me/orgs-with-permission` |
| 能源 | `energy_org` | 部門、部門成員、角色指派（特權角色要 `energy:role:assign-privileged`、最後一位治理者防呆） |
| | `energy_audit` | 59 種事件 registry（payload 白名單、severity、fail-closed／best-effort）、AuditLogger、查詢 |
| | `energy_master_data` | 地主、案場、合約、期數、行事曆；geocode（mock） |
| | `energy_workflow` | 定義／節點／邊／動作、綁定取版、引擎（WS-HumanTask 狀態機、並行與 join、回退與副作用鎖、中斷、代理、轉移、強制推進、血緣）、可視度、模式判定、路線預覽、清單 |
| | `energy_bindings` | workflow／rating 效期綁定：綁、換版、解除、預覽 |
| | `energy_rating` | 策略白名單、定點數計算、應付紀錄（不可變、supersede 鏈） |
| | `energy_ingest` | 收件批次、項目、電費單（三層值）、去重、送審開流程；辨識驅動（背景任務＋`FOR UPDATE SKIP LOCKED` 租約，取代 pg_cron） |
| | `energy_attachments` | 先存 MinIO 驗過才寫 DB、sha256、上限、supersede、只解除連結不刪、RFC 5987 檔名、下載稽核 |
| | `energy_workbench` | 工作台、登入落點、儀表板、行事曆、案場地圖資料 |

### 2.2 型別閘（照 ryxis-core `src/auth.rs` 的 `Authorized`）

- `authn::current_user(req)` 是唯一決定使用者是誰的地方。
- `rbac.authorize_org(&user, org_id, &[perm…]) -> Result<OrgAccess>`；`OrgAccess` 欄位私有，只有 rbac 造得出來。
- 所有 org 範圍的 repo 函式第一個參數是 `&OrgAccess`，org 條件從它取，不收裸的 `org_id`。漏判權或漏 org 條件編不過。
- project／resource 範圍：`ProjectAccess`、`ResourceAccess`；project 的 org 一律由 server 從 project 推導。
- execution 範圍：`energy_workflow.authorize_execution(&OrgAccess, exec_id, need) -> ExecutionAccess`（參與者或 `flow:view-all`）。附件、task、血緣都經它。

### 2.3 Crate 分層

| crate | 內容 | 可依賴 |
|---|---|---|
| `crates/eo-domain` | 純函式與型別：canonical 型別、單別判定與欄位抽取、businessKey、displayId、計費定點數、流程條件求值、resolveMode、routePreview、稽核事件 registry、RBAC 涵蓋判定與 seed 驗證 | 無 IO |
| `crates/eo-platform` | azldi、基礎 plugin、user、authn、multitenancy、rbac | eo-domain |
| `crates/eo-energy` | 能源 plugin | eo-domain、eo-platform |
| 根 crate `energy-ops` | 組裝根 `build_app`、binary | 全部 |

`tests/layer_guard.rs` 守依賴方向。

### 2.4 API 約定

- 前綴 `/api/v1`；org 範圍 `/api/v1/orgs/{orgId}/…`，project 範圍 `/api/v1/projects/{projectId}/…`。
- JSON 一律 camelCase；id 是數字；金額、度數是 decimal 字串；日期 `YYYY-MM-DD`，時間 RFC 3339。
- 錯誤 `{code, message, details?}`，`code` 是穩定的 snake_case。
- 清單：`?page&pageSize&sort&order` ＋ 各自篩選，回 `{items, total}`。
- 請求驗證用 serde（`deny_unknown_fields`）＋手寫 validate；稽核 payload 照 registry 白名單丟棄未宣告的鍵。

## 3. 為什麼用 sqlx

- schema 已經是 SQL（App 5 的 `001_schema.sql`），可直接當 migration 起點；`sqlx::migrate!` 編進 binary。
- 關鍵語意都是 SQL：partial unique、`FOR UPDATE SKIP LOCKED`、jsonb 原子租約、RBAC 的 join。ORM 幫不上忙。
- 交易明傳 `&mut Transaction`：「業務與稽核同一交易」「commit 後才失效快取」看得見。
- `query!` 編譯期核對欄位；`.sqlx/` 離線快取進版控，編譯不必連 DB。
- 軟刪除兩邊都不會自動處理，集中在各 plugin 的 repo，不在 handler 散寫 SQL。
- 代價：CRUD 樣板多、動態篩選用 `QueryBuilder`、改 SQL 後要 `cargo sqlx prepare`。

## 4. 資料表

以 App 5 `db/migrations/001_schema.sql` 為底，調整：

- 平台表照 az master 補齊：`tbl_project`（type、is_default）、`mn_user_project`、`tbl_organization_invitation`、`tbl_project_invitation`、`tbl_account_link`（照 az master 的 accountLink；bcrypt 雜湊在 `provider_user_access_info.password`）、`tbl_session`（若要可撤銷 session）。
- RBAC 表照 az rbacHardening SPEC §2：permission／role 加 `type`（system／plugin／user-defined／synthetic）、`created_by_plugin`；grant 表的 partial unique 用 `COALESCE`；`rbacUserRolePermission` 不搬。
- 共通欄位：`id bigserial`、`created_at`、`updated_at`、`deleted_at`；`updated_at` trigger 保留「只動 deleted_at 不算變更」。
- App 5 的 RLS、`energy_rbac_check` plpgsql、pg_cron／pg_net／Vault、`006_review_reports` 不搬（判定在 Rust）。

## 5. 測試

- 本機用 `docker compose`（Postgres 17＋MinIO）；整合測試每條開自己的 schema（或 `sqlx::test` 的暫時資料庫）。
- `tests/fixtures/canonical-samples.json`（四份真實台電單的辨識結果）複製進來當 golden test：單別、抽取、businessKey、欄位框。
- 規格來源的測試逐條移植，測試名對得上來源：
  - az master `new-test/plugins/RbacPlugin/*`、`TenantConsolePlugin/*`、`AuthnPlugin/*`
  - az branch `new-test/plugins/EnergyOpsPlugin/*`（31 支、約 450 case）
  - App 5 `backend/tests/*`（33 支，含 App 5 新增的 abort、force-advance、reassign、delegation）
- e2e：App 5 的 `tests/crawlers/*`（puppeteer）改接新 API。
- `PROVENANCE.md` 記來源 commit 與複製進來的 fixture sha256。
- `scripts/gate.sh`：fmt、clippy、`cargo test`（含整合）、web vitest、build。

## 6. 分期

| 期 | 內容 | 完成的樣子 |
|---|---|---|
| P0 骨架 | workspace 與三個 crate、azldi＋AppPlugin、config、database（migrate）、storage、http_server、health、static_web、docker compose、`dev-run.sh`、`gate.sh`、layer guard | `./dev-run.sh` 起得來，`/health` 列路由且 DB、MinIO 為 ok |
| P1 平台 | user、authn、multitenancy（含邀請）、rbac（含自訂角色／permission、seed manager、快取） | RBAC／TenantConsole／Authn 的移植測試全綠 |
| P2 純邏輯 | `eo-domain` 全部 | golden test 與純函式測試全綠 |
| P3 能源核心 | energy_org、energy_audit、energy_master_data、energy_workflow | 流程引擎移植測試全綠 |
| P4 能源周邊 | energy_bindings、energy_rating、energy_ingest（含辨識驅動）、energy_attachments | 收件到應付走得通（辨識用假 tpr server） |
| P5 前端 | energy_workbench；web 換 Vite 殼、typed API client、登入改接、多組織選擇器、成員與邀請、RBAC 管理頁（自 raadmin5 移植）；e2e | 瀏覽器從登入走完一張電費單 |
| P6 加分 | presign 直傳 | 大檔上傳不經後端 |

## 7. 開工後定下的事（2026-11-04）

| 議題 | 決定與理由 |
|---|---|
| session | 不用 JWT：隨機 token（cookie `eo_session` 或 Bearer），DB 只存 sha256（`tbl_user_session`）。登出即失效、改密碼撤其他 session、停用撤全部；閒置 14 天滑動、絕對 183 天 |
| SQL | sqlx 的執行期查詢（`query_as` ＋ `FromRow`），不用 `query!` 巨集：省掉 `.sqlx` 離線快取與編譯時連 DB；欄位錯由整合測試抓 |
| Postgres | `postgres:18`（本機已有 image）；MinIO 釘 `RELEASE.2025-04-22T22-12-26Z`（官方已停發 Docker Hub 社群版 image）；bucket 由 storage plugin 開機建立 |
| S3 客戶端 | 自寫 SigV4（`storage.rs`，hmac＋sha2，path-style），put／get／head／delete／presign，不引 aws-sdk |
| 快取 | 行程內 grant 快取，每個使用者一個 generation，commit 後失效；角色定義變了全部失效 |
| 平台權限 | `platform:rbac:manage`（萬能鑰匙，取代 `ryxis:rbac:manage`）、`platform:org:manage`、`platform:member:manage`、`platform:project:manage`；角色 `platform:org-admin`、`platform:project-admin`；建組織者自動持有 org-admin |
| 自訂角色 | 可屬於某組織（`owning_organization_id`），只有該組織看得到；組織只能建 complex 自訂 permission（simple 只有超級使用者能建） |
| 邀請 | 組織與專案邀請合成 `tbl_invitation`；註冊只能憑邀請（`EO_ALLOW_SIGNUP` 可開放）；建組織限超級使用者（`EO_ALLOW_ORG_CREATE` 可開放） |
| rustfmt | 共用工具鏈沒裝；gate 有裝才檢查 |

能源 API 的路徑與形狀約定見 `docs/API-ENERGY.md`。
