# 能源頁面 API 對照（App 5 舊路徑 → 新後端）

## 說明

`src/admin/domain-logic/components/energyOps/` 底下的頁面是從 App 5 搬來的。第二階段起，所有能源呼叫都經過
`src/api/energy.ts`，路徑改成新後端的組織前綴 **`E = /api/v1/orgs/{orgId}/energy`**（契約：`docs/API-ENERGY.md`）。

- `energyBase(orgId)` = `/api/v1/orgs/${orgId}/energy`；`{orgId}` 一律是 `useSelection()?.organizationId`。
- `energyHttp.get/post/patch/put/delete`：介面同 axios（回 `{ data }`），`withCredentials: true`。
- **回應轉換表 `ENERGY_ROUTES`**：契約上的 80 條路由各一列（方法＋路徑樣式＋可選的 `adapt`）。response interceptor 依「方法＋路徑」
  找到那一列，套 `adapt` 後交給頁面，所以頁面拿到的仍是 App 5 的形狀（例如清單是陣列、流程單清單有 `rows`）。
  比對時靜態段多的優先（`/bills/unmatched` 贏過 `/bills/:billId`）。`responseType: 'blob'` 的下載不轉換。
- `attachmentDownloadUrl(id, orgId?)` = `E/attachments/{id}/download`；沒傳組織時取目前選定的組織。
- `uploadViaStorage(endpoint, file, target, fields)`：先 `POST E/uploads/sign`（`{target, batchId|executionId, mimeType, size, originalFilename}`），
  拿到 `{stagingKey, signedUrl}` 就 `PUT` 原檔到簽名網址、再對原端點送 JSON `{stagingKey, originalFilename, mimeType, ...fields}`；
  sign 回 404 `route_not_found`（或 501／405）才退回 multipart（欄位 `file, originalFilename, mimeType, target, ...fields`），
  並在這一頁的生命週期內記住、不再問。sign 回其他錯誤（403、`batch_not_found`、413）照常丟給頁面。
- `fetchRecognitionState(orgId)`：`GET E/recognition/status` → `on|off|down|unknown`。送件頁 `off` 時顯示「辨識服務未開」並且不送辨識；
  端點未上線（404）當 `unknown`，畫面不顯示。
- `installEnergyDefaults(orgId)`：`POST E/setup/install-defaults`；流程總覽在組織沒有任何流程定義時顯示「初始化能源設定」（需 `energy:workflow:manage`）。
- 401：能源 axios 的錯誤 interceptor 呼叫 `notifyUnauthorized()`，與平台 `http` 共用 `onUnauthorized` 監聽者；
  `layout/UnauthorizedWatcher` 收到就 `logout()` 回登入頁。
- 錯誤形狀 `{code, message, details?}` 是物件；`apiErrorCode`／`apiErrorPayload` 原本就收物件，頁面的錯誤碼判斷不用改。
- 守門測試：`src/api/energy.test.ts` 檢查 `src/admin` 底下沒有 `/api/energy-ops/`、`/api/tenant-console/` 字串。

記號：「狀態」欄的「待上線（收件組）」是附件、電費單、收件批次、辨識那一組，後端另一條分支在做；前端已照契約接好，
在那之前打到會是 404 `route_not_found`（頁面照 App 5 的方式處理：多數吞掉、清單頁跳通知）。

---

## 對照總表（頁面實際呼叫的 55 個端點）

| # | 舊（App 5） | 新 | 回應轉換 | 備註 | 狀態 | 呼叫端 |
|---|---|---|---|---|---|---|
| 1 | `GET /api/energy-ops/organizations/{orgId}/audit-events` | `GET E/audit-events` | `{items}` → 陣列 | — | 已上線 | `src/admin/domain-logic/components/energyOps/AuditPage.tsx:99`、`.../BillDetailBody.tsx:254` |
| 2 | `GET /api/energy-ops/organizations/{orgId}/audit-actors` | `GET E/audit-actors` | `{items}` → 陣列 | — | 已上線 | `.../AuditPage.tsx:116` |
| 3 | `GET /api/energy-ops/organizations/{orgId}/bills` | `GET E/bills` | `{items}` → 陣列 | — | 待上線（收件組） | `.../BillsPage.tsx:145` |
| 4 | `GET /api/energy-ops/organizations/{orgId}/bills/unmatched` | `GET E/bills/unmatched` | `{items}` → 陣列 | — | 待上線（收件組） | `.../BillsMenuItem.tsx:38`、`.../BillsPage.tsx:173` |
| 5 | `GET /api/energy-ops/organizations/{orgId}/bills/{billId}` | `GET E/bills/{billId}` | — | — | 待上線（收件組） | `.../BillDetailBody.tsx:219`、`.../IngestBatchDetailPage.tsx:238`、`.../IngestBatchDetailPage.tsx:305`、`.../IngestBatchDetailPage.tsx:524` |
| 6 | `PATCH /api/energy-ops/organizations/{orgId}/bills/{billId}` | `PATCH E/bills/{billId}` | — | — | 待上線（收件組） | `.../BillDetailBody.tsx:378`、`.../BillDetailBody.tsx:387`、`.../BillDocTypeField.tsx:58`、`.../IngestBatchDetailPage.tsx:289` |
| 7 | `GET /api/energy-ops/organizations/{orgId}/bills/{billId}/payouts` | `GET E/bills/{billId}/payouts` | `{items}` → 陣列 | — | 待上線（收件組） | `.../BillDetailBody.tsx:240` |
| 8 | `GET /api/energy-ops/organizations/{orgId}/bills/{billId}/branch-preview` | `GET E/bills/{billId}/branch-preview` | — | — | 待上線（收件組） | `.../BillDetailBody.tsx:296` |
| 9 | `POST /api/energy-ops/organizations/{orgId}/bills/{billId}/match` | `POST E/bills/{billId}/match` | — | — | 待上線（收件組） | `.../BillDetailBody.tsx:491`、`.../IngestBatchDetailPage.tsx:377` |
| 10 | `POST /api/energy-ops/organizations/{orgId}/bills/{billId}/submit` | `POST E/bills/{billId}/submit` | — | — | 待上線（收件組） | `.../BillDetailBody.tsx:615`、`.../BillsPage.tsx:188` |
| 11 | `GET /api/energy-ops/organizations/{orgId}/executions` | `GET E/executions` | `items` 另掛成 `rows` | — | 已上線 | `.../ExecutionsPage.tsx:173` |
| 12 | `GET /api/energy-ops/organizations/{orgId}/executions/{executionId}` | `GET E/executions/{executionId}` | — | — | 已上線 | `.../BillDetailBody.tsx:273`、`.../ExecutionDetailPage.tsx:102` |
| 13 | `GET /api/energy-ops/organizations/{orgId}/executions/{executionId}/graph` | `GET E/executions/{executionId}/graph` | — | — | 已上線 | `.../ExecutionDetailPage.tsx:109`、`.../FlowGraph.tsx:340` |
| 14 | `GET /api/energy-ops/organizations/{orgId}/executions/{executionId}/lineage` | `GET E/executions/{executionId}/lineage` | `{items}` → 陣列 | — | 已上線 | `.../BillDetailBody.tsx:256` |
| 15 | `GET /api/energy-ops/organizations/{orgId}/executions/{executionId}/actions` | `GET E/executions/{executionId}/actions` | — | — | 已上線 | `.../BillDetailBody.tsx:261` |
| 16 | `GET /api/energy-ops/executions/{executionId}/attachments` | `GET E/executions/{executionId}/attachments` | `{items}` → 陣列 | 組織取自 `useSelection()`（頁面傳入；沒傳時 `selectionStore`） | 待上線（收件組） | `.../BillDetailBody.tsx:252` |
| 17 | `POST /api/energy-ops/executions/{executionId}/attachments` | `POST E/executions/{executionId}/attachments` | — | 組織取自 `useSelection()`（頁面傳入；沒傳時 `selectionStore`）；`uploadViaStorage`：先試 `POST E/uploads/sign`，404 route_not_found／501 才退回 multipart | 待上線（收件組） | `.../BillDetailBody.tsx:548` |
| 18 | `GET /api/energy-ops/attachments/{attachmentId}/download` | `GET E/attachments/{attachmentId}/download` | — | 組織取自 `useSelection()`（頁面傳入；沒傳時 `selectionStore`） | 待上線（收件組） | `.../shared.tsx:506`（blob）、`.../BillDetailBody.tsx:966`（href）、`.../BillDetailBody.tsx:1387`（href）、`.../IngestBatchDetailPage.tsx:466`（href）、`.../IngestBatchDetailPage.tsx:930`（href） |
| 19 | `POST /api/energy-ops/organizations/{orgId}/tasks/{taskId}/actions` | `POST E/tasks/{taskId}/actions` | — | — | 已上線 | `.../BillDetailBody.tsx:409`、`.../WorkbenchPage.tsx:160`、`.../WorkflowOverviewPage.tsx:133` |
| 20 | `POST /api/energy-ops/organizations/{orgId}/tasks/{taskId}/claim` | `POST E/tasks/{taskId}/claim` | — | — | 已上線 | `.../BillDetailBody.tsx:446` |
| 21 | `GET /api/energy-ops/organizations/{orgId}/workbench` | `GET E/workbench` | — | — | 已上線 | `.../WorkbenchPage.tsx:124` |
| 22 | `GET /api/energy-ops/organizations/{orgId}/workflows` | `GET E/workflows` | — | — | 已上線 | `.../ExecutionsPage.tsx:208`、`.../WorkflowOverviewPage.tsx:77` |
| 23 | `GET /api/energy-ops/organizations/{orgId}/workflows/{definitionId}/graph` | `GET E/workflows/{definitionId}/graph` | — | — | 已上線 | `.../WorkflowOverviewPage.tsx:103` |
| 24 | `GET /api/energy-ops/organizations/{orgId}/workflows/{definitionId}/nodes/{nodeKey}/tasks` | `GET E/workflows/{definitionId}/nodes/{nodeKey}/tasks` | `{items}` → 陣列 | — | 已上線 | `.../WorkflowOverviewPage.tsx:113` |
| 25 | `GET /api/energy-ops/organizations/{orgId}/unbound-stations` | `GET E/unbound-stations` | `{items}` → 陣列 | — | 已上線 | `.../WorkflowOverviewPage.tsx:93` |
| 26 | `GET /api/energy-ops/organizations/{orgId}/stations` | `GET E/stations` | `{items}` → 陣列 | — | 已上線 | `.../BillDetailBody.tsx:472`、`.../IngestBatchDetailPage.tsx:357` |
| 27 | `GET /api/energy-ops/organizations/{orgId}/station-list` | `GET E/station-list` | `{items}` → 陣列 | — | 已上線 | `.../BillsPage.tsx:162`、`.../DashboardPage.tsx:102` |
| 28 | `GET /api/energy-ops/organizations/{orgId}/stations/{stationId}/overview` | `GET E/stations/{stationId}/overview` | — | — | 已上線 | `.../StationDetailPage.tsx:65` |
| 29 | `GET /api/energy-ops/organizations/{orgId}/stations/{stationId}/bindings` | `GET E/stations/{stationId}/bindings` | — | — | 已上線 | `.../StationDetailPage.tsx:77` |
| 30 | `GET /api/energy-ops/organizations/{orgId}/stations/{stationId}/bindings/preview` | `GET E/stations/{stationId}/bindings/preview` | — | — | 已上線 | `.../BindingActions.tsx:97` |
| 31 | `POST /api/energy-ops/organizations/{orgId}/stations/{stationId}/bindings` | `POST E/stations/{stationId}/bindings` | — | — | 已上線 | `.../BindingActions.tsx:110` |
| 32 | `POST /api/energy-ops/organizations/{orgId}/stations/{stationId}/bindings/switch` | `POST E/stations/{stationId}/bindings/switch` | — | — | 已上線 | `.../BindingActions.tsx:114` |
| 33 | `POST /api/energy-ops/organizations/{orgId}/stations/{stationId}/bindings/end` | `POST E/stations/{stationId}/bindings/end` | — | — | 已上線 | `.../BindingActions.tsx:118` |
| 34 | `GET /api/energy-ops/organizations/{orgId}/binding-options` | `GET E/binding-options` | `{items}` → 陣列 | — | 已上線 | `.../BindingActions.tsx:82` |
| 35 | `GET /api/energy-ops/organizations/{orgId}/stations/{stationId}/geocode` | `GET E/stations/{stationId}/geocode` | `result.latitude/longitude` 轉數字 | — | 已上線 | `.../DashboardPage.tsx:113` |
| 36 | `POST /api/energy-ops/organizations/{orgId}/stations/{stationId}/geocode` | `POST E/stations/{stationId}/geocode` | — | — | 已上線 | `.../DashboardPage.tsx:124` |
| 37 | `GET /api/energy-ops/organizations/{orgId}/dashboard` | `GET E/dashboard` | — | — | 已上線 | `.../DashboardPage.tsx:93` |
| 38 | `GET /api/energy-ops/organizations/{orgId}/calendar-events` | `GET E/calendar-events` | `{items}` → 陣列 | — | 已上線 | `.../CalendarPage.tsx:92` |
| 39 | `GET /api/energy-ops/organizations/{orgId}/landowners` | `GET E/landowners` | `{items}` → 陣列 | — | 已上線 | `.../LandownersPage.tsx:49` |
| 40 | `GET /api/energy-ops/organizations/{orgId}/landowners/{landownerId}/overview` | `GET E/landowners/{landownerId}/overview` | — | — | 已上線 | `.../LandownerDetailPage.tsx:48` |
| 41 | `GET /api/energy-ops/organizations/{orgId}/me/landing` | `GET E/me/landing` | — | — | 已上線 | `.../LandingRedirect.tsx:41` |
| 42 | `GET /api/energy-ops/organizations/{orgId}/departments` | `GET E/departments` | `{items}` → 陣列 | — | 已上線 | `.../DepartmentsPage.tsx:60` |
| 43 | `POST /api/energy-ops/organizations/{orgId}/departments` | `POST E/departments` | — | — | 已上線 | `.../DepartmentsPage.tsx:80` |
| 44 | `DELETE /api/energy-ops/organizations/{orgId}/departments/{departmentId}` | `DELETE E/departments/{departmentId}` | — | — | 已上線 | `.../DepartmentsPage.tsx:95` |
| 45 | `POST /api/energy-ops/organizations/{orgId}/departments/{departmentId}/members` | `PUT E/departments/{departmentId}/members/{userId}` | — | body 改 `{ role }`（userId 進路徑）；頁面已改 | 已上線 | `.../DepartmentsPage.tsx:108` |
| 46 | `DELETE /api/energy-ops/organizations/{orgId}/departments/{departmentId}/members/{userId}` | `DELETE E/departments/{departmentId}/members/{userId}` | — | — | 已上線 | `.../DepartmentsPage.tsx:121` |
| 47 | `GET /api/tenant-console/organizations/{orgId}/members` | `GET E/members` | `{items}` → 陣列＋補 `id = userId` | `fetchMemberOptions`：沒 ASSIGN_ROLE 時退回平台 `GET /api/v1/orgs/{orgId}/members` | 已上線 | `.../DepartmentsPage.tsx:70` |
| 48 | `GET /api/energy-ops/organizations/{orgId}/ingest-batches` | `GET E/ingest-batches` | `{items}` → 陣列 | — | 待上線（收件組） | `.../IngestConsolePage.tsx:100` |
| 49 | `POST /api/energy-ops/organizations/{orgId}/ingest-batches` | `POST E/ingest-batches` | — | — | 待上線（收件組） | `.../IngestConsolePage.tsx:125` |
| 50 | `GET /api/energy-ops/organizations/{orgId}/ingest-batches/{batchId}` | `GET E/ingest-batches/{batchId}` | — | — | 待上線（收件組） | `.../IngestBatchDetailPage.tsx:196` |
| 51 | `POST /api/energy-ops/organizations/{orgId}/ingest-batches/{batchId}/attachments` | `POST E/ingest-batches/{batchId}/attachments` | — | `uploadViaStorage`：先試 `POST E/uploads/sign`，404 route_not_found／501 才退回 multipart | 待上線（收件組） | `.../IngestConsolePage.tsx:136` |
| 52 | `POST /api/energy-ops/organizations/{orgId}/ingest-batches/{batchId}/recognize` | `POST E/ingest-batches/{batchId}/recognize` | — | — | 待上線（收件組） | `.../IngestConsolePage.tsx:149` |
| 53 | `PATCH /api/energy-ops/organizations/{orgId}/ingest-items/{itemId}` | `PATCH E/ingest-items/{itemId}` | — | — | 待上線（收件組） | `.../IngestBatchDetailPage.tsx:270` |
| 54 | `POST /api/energy-ops/organizations/{orgId}/ingest-items/{itemId}/submit` | `POST E/ingest-items/{itemId}/submit` | — | — | 待上線（收件組） | `.../IngestBatchDetailPage.tsx:319` |
| 55 | `POST /api/energy-ops/organizations/{orgId}/ingest-items/{itemId}/discard` | `POST E/ingest-items/{itemId}/discard` | — | — | 待上線（收件組） | `.../BillDetailBody.tsx:633`、`.../IngestBatchDetailPage.tsx:339` |

（呼叫端的 `.../` = `src/admin/domain-logic/components/energyOps/`）

## 卡 20261118-0427-a 新增（沒有 App 5 對應，資料模型重整與主檔維護）

| 方法 | 新路徑 | 回應轉換 | 呼叫端 |
|---|---|---|---|
| GET | `E/station-list`（可篩 `siteType`、`enabled`） | `{items}` → 陣列 | `StationsPage.tsx:67` |
| POST | `E/stations` | — | `StationsPage.tsx:121` |
| PATCH | `E/stations/{stationId}` | — | `StationsPage.tsx:119` |
| GET | `E/stations/{stationId}/meters` | `{items}` → 陣列 | `StationDetailPage.tsx:37` |
| POST | `E/stations/{stationId}/meters`（重疊回 409 `meter_overlap`） | `{items}` → 陣列 | `StationDetailPage.tsx:46` |
| PATCH | `E/meters/{meterId}` | — | `StationDetailPage.tsx`（`update_handler`，`MetersSection.save`） |
| GET | `E/stations/{stationId}/allocations` | `{items}` → 陣列 | `StationDetailPage.tsx:161` |
| POST | `E/stations/{stationId}/allocations` | `{items}` → 陣列 | `StationDetailPage.tsx:179` |
| PATCH | `E/allocations/{allocationId}`（一般編輯／結束共用，帶 `validTo`＋`changeReason` 就是結束） | — | `StationDetailPage.tsx:178`、`:206` |
| GET | `E/stations/{stationId}/allocation-history` | `{items}` → 陣列 | `StationDetailPage.tsx:162` |
| GET | `E/landowners?partyType=`（可篩類型） | `{items}` → 陣列 | `LandownersPage.tsx:67`、`StationsPage.tsx:82`、`StationDetailPage.tsx:163` |
| POST | `E/landowners` | — | `LandownersPage.tsx:100` |
| PATCH | `E/landowners/{landownerId}` | — | `LandownersPage.tsx:99` |

## 新後端另有、頁面目前沒有呼叫的端點

都已列在 `ENERGY_ROUTES`（形狀原樣，清單類照樣 `{items}` → 陣列），之後開頁面時直接用 `energyHttp`：

| 方法 | 新路徑 | 說明 |
|---|---|---|
| PATCH | `E/departments/{departmentId}` | 改部門名稱／排序 |
| GET | `E/members` | 成員＋能源角色＋部門（部門頁挑選器已用） |
| DELETE | `E/members/{userId}` | 移除成員（最後一位治理者 400 `cannot_remove_last_admin`） |
| GET | `E/assignable-roles` | 回 `{items, canAssignPrivileged}`（原樣，不拆） |
| POST | `E/role-assignments`、`E/role-assignments/revoke` | 能源角色指派／撤銷 |
| POST | `E/executions` | 手動開單 `{stationKey, workflowKey, subjectType, subjectId, businessDate, attributes?}` |
| GET | `E/executions/{executionId}/revertable` | 可否回退 |
| POST | `E/executions/{executionId}/abort`、`/force-advance`、`/terminate` | 中斷申請、強制推進、終止（新） |
| POST | `E/tasks/{taskId}/release`、`/abort-decision`、`/reassign` | 釋放、中斷審批、改派 |
| POST | `E/side-effects/{sideEffectId}/release` | 解除回退鎖 |
| GET | `E/delegations?all=1` | 代理清單（新），每列帶 `active`、`mine`、`delegatorName`、`delegateName` |
| POST | `E/delegations`、`E/delegations/{delegationId}/revoke` | 設代理、收回（新；已收回 409 `delegation_already_revoked`） |
| POST | `E/lineage` | 連血緣 |
| POST | `E/setup/install-defaults` | 初始化能源設定（流程總覽空狀態已用） |
| GET | `E/landowners/{landownerId}` | 單一地主 |
| POST | `E/bills/ingest`、`E/ingest-items/{itemId}/recognition` | 收件組（待上線） |
| POST | `E/attachment-links/{linkId}/remove` | 收件組（待上線） |
| POST | `E/uploads/sign` | 收件組（待上線）；`uploadViaStorage` 自動偵測 |
| GET | `E/recognition/status` | 收件組（待上線）；送件頁「辨識服務未開」的來源 |

---

## 頁面讀的回應形狀（App 5 形狀，轉換表保證不變）

共用的回應形狀（下文直接引用名稱）：

- **ExecutionGraph**（`FlowGraph.tsx`）：`{ executionId?, status?, current?: string[], nodes: [{ nodeKey, name, nodeType, layout: {x,y}|null, state, serviceKey, waiting?, stuck? }], edges: [{ from, to, kind ('sendBack' 等), label, labelKey, hasCondition, implied }] }`
- **TaskAction**（`ActionBar.tsx`）：`{ actionKey, labelKey, kind, requiresReason, sortOrder }`
- **ActionBlocked**（`ActionBar.tsx`）：`{ code, nodeName?, assigneeHint? }`
- **SubjectRef**（`routes.ts`，給 `subjectRoute`／`taskWorkRoute` 決定連結）：`{ kind ('energyBill'|'energyInstallment'), billId?, installmentId?, landownerId?, stationId? }`

## 依檔案分節的明細（路徑寫的是 App 5 舊路徑，新路徑見上表）

### src/admin/domain-logic/components/energyOps/AuditPage.tsx

**:99 `GET /api/energy-ops/organizations/{orgId}/audit-events`**
- query：`actorUserId?`、`outcome?`（`success|denied|failed`）、`from?`、`to?`、`limit=300`。事件類型分組（`department.`、`member.`、`flow.`、`delegation.`、`lineage.`、`attachment.`、`bill.`、`payout.`、`strategy.`）是前端用 `type.startsWith()` 過濾的，**不送** query。
- 回應：陣列（`Array.isArray(data)`）。每列讀 `id`、`type`、`occurredAt`、`summaryKey`、`payload`（插值用：`payload.docType`、`payload.gateNames[]`、`payload.gatesDelta`、`payload.sideEffectKey`）、`outcome`、`actorUserId`、`actorLabel`、`actorName`、`onBehalfOfUserId`、`onBehalfOfName`、`targetType`、`targetId`、`targetLabel`、`targetExecutionId`、`targetBillId`、`causedBy.{id,type,summaryKey}`。型別上還宣告了 `severity`、`summary`、`traceId`、`causedBy.summary/payload`。
- 錯誤：403 → denied。

**:116 `GET /api/energy-ops/organizations/{orgId}/audit-actors`**
- 無參數。
- 回應：陣列 `[{ id, name }]`，用來做操作者下拉。
- 錯誤：吞掉，改成空陣列。

### src/admin/domain-logic/components/energyOps/BillDetailBody.tsx

電費單校對台本體（路由頁與 `BillDetailDrawer` 共用）。`{billId}` 來自 props；`{executionId}` = 從 `bill.executions[]` 選出的那一筆（優先取 `queue.executionId`，其次取 `status` 為 `running`/`abortPending` 的那筆，最後取第一筆）。

**:219 `GET /api/energy-ops/organizations/{orgId}/bills/{billId}`**
- 無參數。
- 回應（物件）：`id`、`stationId`、`customerNumberRaw`、`sourceAttachmentId`、`ingest.{batchId,itemId,originalFilename,attachmentId,submittedAt,submittedByName}`、`ocrRaw`（給 `recognitionSummary()` 和 `sourceFieldBoxes()`）、`ocrExtracted`、`changedFields[]`（給 `FieldDiffGrid`）、`reviewStatus`、`station.{id,name,customerNumber}`、`executions[].{id,status,workflowKey}`（型別上還有 `currentNodeKey`、`businessDate`）、`templateId`、`docType`、`recognizedDocType`、`docTypeEditable`、`binding`（整包交給 `BillDocTypeField`：`binding.flows[].{workflowKey,workflowName,version,contractNo}`；本檔另外讀 `binding.bound`、`binding.reason`（`'no_binding'`）、`binding.stationName`、`binding.customerNumber`）、`businessKey`、`documentNo`、`mode`（`intake|revise|act|readonly|denied`）、`modeReason.{code,nodeName,assigneeHint,at}`、`canEditSubject`、`canAddAttachment`，以及 `ALL_BILL_REVIEW_FIELDS` 每個欄位的值（當草稿初值）。
- 錯誤：403 → denied。

**:240 `GET /api/energy-ops/organizations/{orgId}/bills/{billId}/payouts`**
- 回應：陣列。每列讀 `id`、`status`、`amount`、`currency`、`businessDate`、`strategyName`、`strategyType`、`strategyVersion`、`paramsSnapshot`、`inputSnapshot`（後兩個直接 `JSON.stringify` 顯示）。
- 錯誤：吞掉，改成空陣列。

**:252 `GET /api/energy-ops/executions/{executionId}/attachments`**（`ENERGY_ROOT`，不分組織）
- 回應：陣列。讀 `id`、`originalFilename`、`size`（顯示成 KB）、`links[].role`（`source`／`billScan` 算原始檔，第一個 `role` 當 `data-role`）。
- 錯誤：吞掉，改成空陣列。

**:254 `GET /api/energy-ops/organizations/{orgId}/audit-events`**
- query：`targetType='energyWorkflowExecution'`、`targetId=String(executionId)`。
- 回應：陣列。讀 `id`、`occurredAt`、`summaryKey`、`actorLabel`、`actorUserId`、`onBehalfOfUserId`。
- 錯誤：吞掉，改成空陣列。

**:256 `GET /api/energy-ops/organizations/{orgId}/executions/{executionId}/lineage`**
- 回應：陣列，或物件時取 `data.nodes`。每列讀 `id`、`relation ?? kind`、`executionId ?? upstreamExecutionId ?? id`。
- 錯誤：吞掉，改成空陣列。

**:261 `GET /api/energy-ops/organizations/{orgId}/executions/{executionId}/actions`**
- 回應：`availableActions[]`（TaskAction）、`actionBlocked`（ActionBlocked｜null）、`claimable`、`claimedByMe`。
- 錯誤：吞掉，清空。

**:273 `GET /api/energy-ops/organizations/{orgId}/executions/{executionId}`**
- 回應：只讀 `tasks[].{id,state}`，找 `state ∈ {ready, reserved, inProgress}` 的那個 task 當 `openTaskId`。
- 錯誤：吞掉。

**:296 `GET /api/energy-ops/organizations/{orgId}/bills/{billId}/branch-preview`**
- query：`totalAmount`（草稿金額，數字；debounce 400ms）。
- 回應：`available`、`definitionName`、`version`、`nodes[].{nodeKey,name,added}`、`removed[].{nodeKey,name}`。
- 錯誤：吞掉，設成 null。

**:378 `PATCH /api/energy-ops/organizations/{orgId}/bills/{billId}`**（存檔）
- body：只送和 `bill` 不同的 `ALL_BILL_REVIEW_FIELDS` 欄位。空字串送 `null`；`totalAmount`、`generationKwh` 轉成數字；四個日期欄位取前 10 碼；其餘 `trim`。
- 回應：未使用（成功後重新 `load()`）。

**:387 `PATCH /api/energy-ops/organizations/{orgId}/bills/{billId}`**（標記已校對）
- body：`{ markReviewed: true }`。
- 回應：未使用。

**:409 `POST /api/energy-ops/organizations/{orgId}/tasks/{taskId}/actions`**
- body：`{ actionKey, reason? }`（`requiresReason` 的動作才帶 `reason`）。
- 回應：`data.row`；在佇列（抽屜）模式下交給 `queue.onRowChanged`，也就是 `WorkbenchPage.replaceRow`，會讀 `row.id`、`row.state`，並把整列合併回原列。

**:446 `POST /api/energy-ops/organizations/{orgId}/tasks/{taskId}/claim`**
- body：無。
- 回應：`data.row`，同上交給 `queue.onRowChanged`。

**:472 `GET /api/energy-ops/organizations/{orgId}/stations`**
- query：`q?`（初值是 `bill.customerNumberRaw`）。
- 回應：陣列 `[{ id, name, customerNumber }]`（型別上還有 `status`）。
- 錯誤：吞掉，改成空陣列。

**:491 `POST /api/energy-ops/organizations/{orgId}/bills/{billId}/match`**
- body：`{ stationId: number }`。
- 回應：未使用。錯誤用 `apiErrorCode` 顯示在對話框。

**:548 `POST /api/energy-ops/executions/{executionId}/attachments`**（`uploadViaStorage`，multipart）
- form：`file`、`originalFilename`、`mimeType`、`target='execution'`、`role`（`'source'|'added'`，由 `fields` 帶）。
- 回應：未使用（成功後 `load()`）。

**:615 `POST /api/energy-ops/organizations/{orgId}/bills/{billId}/submit`**
- body：無。
- 回應：未使用。
- 錯誤：`apiErrorPayload(e)` 的 `code ∈ {bill_already_submitted, already_in_review, duplicate_document}` 時開重複件對話框，讀 `payload.details.{documentNo,subjectId,submittedByName,submittedAt,currentNodeName,executionId}`；其他錯誤用 `apiErrorCode` 跳通知。

**:633 `POST /api/energy-ops/organizations/{orgId}/ingest-items/{itemId}/discard`**
- `{itemId}` = `bill.ingest.itemId`。
- body：`{ reason: 'duplicate' }`。
- 回應：未使用。

**:966 `GET /api/energy-ops/attachments/{attachmentId}/download`**（`<Button href>` 開新分頁）
- `{attachmentId}` = `sourceAttachmentId`（`bill.sourceAttachmentId` → 附件裡 role 為 source 的那筆 → `bill.ingest.attachmentId`）。靠同網域 cookie。

**:1387 `GET /api/energy-ops/attachments/{attachmentId}/download`**（附件頁籤每列的 `href`）
- `{attachmentId}` = 附件列的 `a.id`。

另外：本檔把 `orgId`、`execution.id` 交給 `<FlowGraph>`（:1238），由 `FlowGraph.tsx:340` 自己抓圖；把 `bill.binding` 等交給 `BillDocTypeField`；把 `sourceAttachmentId` 交給 `SourcePreviewFrame`（`shared.tsx:506` 下載 blob）。

### src/admin/domain-logic/components/energyOps/BillDocTypeField.tsx

**:58 `PATCH /api/energy-ops/organizations/{orgId}/bills/{billId}`**
- body：`{ docType: string }`。
- 回應：未使用（成功後呼叫 `onSaved()`，由呼叫端重讀 bill）。
- 錯誤：`apiErrorCode(e) === 'doc_type_locked_by_execution'` 時顯示「已開流程不能改」警告；其他錯誤跳 error 通知。
- 備註：`binding` 是呼叫端傳進來的 `bill.binding`，本檔讀 `binding.flows[].{workflowKey,workflowName,version,contractNo}`。

### src/admin/domain-logic/components/energyOps/BillsMenuItem.tsx

**:38 `GET /api/energy-ops/organizations/{orgId}/bills/unmatched`**
- 回應：陣列，只用 `data.length` 當選單角標。
- 錯誤：一律當作 0（包含 403）。

### src/admin/domain-logic/components/energyOps/BillsPage.tsx

**:145 `GET /api/energy-ops/organizations/{orgId}/bills`**
- query：`reviewStatus?`（頁籤「待指認」時固定送 `pendingMatch`）、`from?`＝`{month}-01`、`to?`＝下個月 1 號（半開區間；`month` 來自網址 `?month=`）。關鍵字和「只看修正過」都在前端過濾。
- 回應：陣列。每列讀 `id`、`stationId`、`customerNumberRaw`、`billingPeriodStart`、`billingPeriodEnd`、`totalAmount`、`generationKwh`、`issuedAt`、`reviewStatus`、`changedFields[]`、`executionId`（型別上還有 `ocrExtracted`，實際沒用到）。
- 錯誤：403 → denied。

**:162 `GET /api/energy-ops/organizations/{orgId}/station-list`**
- 回應：陣列，讀 `id`、`name`、`landownerName`（電號 → 案場名／地主名）。
- 錯誤：吞掉。

**:173 `GET /api/energy-ops/organizations/{orgId}/bills/unmatched`**
- 回應：只用 `data.length`（頁籤數字）。
- 錯誤：吞掉，當作 0。

**:188 `POST /api/energy-ops/organizations/{orgId}/bills/{billId}/submit`**
- body：無。
- 回應：未使用。錯誤用 `apiErrorCode` 跳通知（這裡沒有判斷重複件 code）。

### src/admin/domain-logic/components/energyOps/BindingActions.tsx

`{stationId}` 來自 props；`kind` 是 `'workflow'|'rating'`。

**:82 `GET /api/energy-ops/organizations/{orgId}/binding-options`**
- query：`kind`。
- 回應：陣列 `[{ id, name, version }]`（型別上還有 `key`、`type`）。
- 錯誤：吞掉。

**:97 `GET /api/energy-ops/organizations/{orgId}/stations/{stationId}/bindings/preview`**
- query：`kind`、`effectiveFrom`（`YYYY-MM-DD`；debounce 350ms）。
- 回應：`isBackdated`、`affectedDraftBills`、`runningExecutions`。
- 錯誤：吞掉。

**:110 `POST /api/energy-ops/organizations/{orgId}/stations/{stationId}/bindings`**（新綁）
- body：`{ kind, versionId, validFrom, docType: string|null }`。
- 回應：未使用。

**:114 `POST /api/energy-ops/organizations/{orgId}/stations/{stationId}/bindings/switch`**（換版）
- body：`{ kind, toVersionId, effectiveFrom, docType: string|null }`。
- 回應：未使用。

**:118 `POST /api/energy-ops/organizations/{orgId}/stations/{stationId}/bindings/end`**（解除）
- body：`{ kind, validTo, reason, docType: string|null, targetKey: string|null }`。
- 回應：未使用。
- 上面三支的錯誤都只用 `apiErrorCode` 跳通知。

### src/admin/domain-logic/components/energyOps/CalendarPage.tsx

**:92 `GET /api/energy-ops/organizations/{orgId}/calendar-events`**
- query：月曆檢視送 `from=YYYY-MM-01`、`to=月底`；清單檢視不帶參數。
- 回應：陣列，讀 `id`、`eventDate`（用來當 `YYYY-MM-DD` 分組鍵）、`title`、`landownerName`、`status`（`missed`／`done`／其他）。
- 錯誤：403 → denied。

### src/admin/domain-logic/components/energyOps/DashboardPage.tsx

**:93 `GET /api/energy-ops/organizations/{orgId}/dashboard`**
- query：`month`（`YYYY-MM`）。
- 回應：`payableThisMonth`、`payoutCount`、`runningCount`、`stuckCount`、`stuckDays`、`releasedUnpaidCount`、`unmatchedCount`、`stationTotal`、`stationByStatus`（`Record<status, number>`）。型別上還有 `month`。
- 錯誤：403 → denied。

**:102 `GET /api/energy-ops/organizations/{orgId}/station-list`**
- 回應：陣列。本頁讀 `id`、`name`、`customerNumber`、`status`、`address`、`latitude`、`longitude`、`geocodeSource`、`landownerName`；整包也傳給子元件 `StationMap`（讀 `id`、`name`、`status`、`latitude`、`longitude`）。型別上還有 `geocodePrecision`、`geocodedAt`。
- 錯誤：吞掉。

**:113 `GET /api/energy-ops/organizations/{orgId}/stations/{stationId}/geocode`**（取建議座標）
- 回應：`address`、`usable`、`reason`（`no_address|not_found|too_coarse`）、`result.{latitude,longitude,precision,matchedAddress}`（型別上還有 `stationId`、`result.source`）。

**:124 `POST /api/energy-ops/organizations/{orgId}/stations/{stationId}/geocode`**（採用建議）
- body：無。
- 回應：未使用。

### src/admin/domain-logic/components/energyOps/DepartmentsPage.tsx

**:60 `GET /api/energy-ops/organizations/{orgId}/departments`**
- 回應：陣列 `[{ id, key, name, members: [{ userId, name, role ('manager'|'member') }] }]`（型別上還有 `sortOrder`）。
- 錯誤：403 → denied；其他錯誤顯示 `e.response.data.message`（這裡沒用 `apiErrorCode`）。

**:70 `GET /api/tenant-console/organizations/{orgId}/members`**（舊成員 API）
- 回應：陣列，讀 `m.id`（當成 userId 送出）、`m.name`，用來做指派對話框的人員下拉。
- 錯誤：吞掉，改成空陣列（退回手動輸入 id）。
- **第二階段**：改用平台的 `GET /api/v1/orgs/{orgId}/members`，回 `items[].userId/name/email`。要把 `items[].userId` 對到這裡的 `id`（`legacyTenantConsoleBase` 加回應轉換，或改呼叫端）。

**:80 `POST /api/energy-ops/organizations/{orgId}/departments`**
- body：`{ key, name }`（都 `trim`）。
- 回應：未使用。
- 錯誤：`apiErrorCode(e) === 'department_key_taken'` 時顯示「代碼已被使用」。

**:95 `DELETE /api/energy-ops/organizations/{orgId}/departments/{departmentId}`**
- 回應：未使用。

**:108 `POST /api/energy-ops/organizations/{orgId}/departments/{departmentId}/members`**
- body：`{ userId: number, role: 'manager'|'member' }`。
- 回應：未使用。錯誤顯示 `apiErrorCode`。

**:121 `DELETE /api/energy-ops/organizations/{orgId}/departments/{departmentId}/members/{userId}`**
- 回應：未使用。

### src/admin/domain-logic/components/energyOps/ExecutionDetailPage.tsx

**:102 `GET /api/energy-ops/organizations/{orgId}/executions/{executionId}`**
- 回應：`displayId`、`workflowName`、`version`、`nodeName`、`currentNodeKey`、`status`（`abortPending` 時加標）、`waitingDays`、`startedByName`、`startedAt`、`businessDate`、`subject.{kind,billId,landownerId,stationId,amount}`（給 `subjectRoute`／`stationRoute` 決定連結）、`tasks[].{id,seq,nodeName,nodeKey,state,assigneeName,outcome,reason}`。型別上另有 `id`、`subjectType`、`subjectId`、`workflowKey`、`definitionId`、`subject.{installmentId,contractNo,seq,stationName,landownerName,periodStart,periodEnd,dueDate}`、`tasks[].{nodeType,createdAt,completedAt}`。
- 錯誤：403 → denied。

**:109 `GET /api/energy-ops/organizations/{orgId}/executions/{executionId}/graph`**
- 回應：ExecutionGraph，整包傳給子元件 `FlowGraph`（`graph` prop）。
- 錯誤：吞掉。

### src/admin/domain-logic/components/energyOps/ExecutionsPage.tsx

**:173 `GET /api/energy-ops/organizations/{orgId}/executions`**
- query：`scope`（`all|mine`）、`status?`（`running|completed`；「我參與過的」不送）、`workflowKey?`、`nodeKey?`、`stationId?`、`landownerId?`、`waitingDaysMin?`、`month?`、`executionId?`、`sort=waitingDays:asc|desc`、`pageSize=200`。
- 回應：`data.rows[]`、`data.total`。每列讀 `id`、`displayId`、`status`、`workflowName`、`version`、`nodeKey`、`nodeName`、`waitingDays`、`startedAt`、`startedByName`、`subject.{kind,billId,installmentId,contractNo,seq,stationName,landownerName,amount,periodStart,periodEnd}`。
- 錯誤：403 → denied（`scope=all` 但沒有 view-all 權限時，後端回 403）。

**:208 `GET /api/energy-ops/organizations/{orgId}/workflows`**
- 回應：只讀 `data.definitions[].{key,name}`（依 key 去重後做流程種類下拉）。
- 錯誤：吞掉。

### src/admin/domain-logic/components/energyOps/FlowGraph.tsx

**:340 `GET /api/energy-ops/organizations/{orgId}/executions/{executionId}/graph`**
- 只有沒傳 `graph` prop 時才打（`BillDetailBody` 用這條）。
- 回應：ExecutionGraph。讀 `nodes[]`、`edges[]`（`kind === 'sendBack'` 等）、`current[]`。
- 錯誤：顯示 `e.response.data.code || e.message`。

### src/admin/domain-logic/components/energyOps/IngestBatchDetailPage.tsx

`{batchId}` = 路由 `:id`；`{billId}` = 展開那一列的 `item.billId`。

**:196 `GET /api/energy-ops/organizations/{orgId}/ingest-batches/{batchId}`**
- 狀態是 `recognizing` 時每 3 秒輪詢一次。
- 回應：`id`、`status`、`submittedAt`、`submittedByName`、`items[]`（型別上還有 `fileCount`、`pendingCount`）。每個 item 讀 `id`、`originalFilename`、`status`（`extracted|classified|unclassified|unmatched|failed|submitted|discarded|uploading|recognizing|classifying|uploaded`）、`docType`、`workflowKey`、`classifyReason`、`attachmentId`、`billId`、`executionId`、`stationId`、`subjectType`、`subjectId`、`documentNo`、`errorMessage`、`amountMismatch`、`alreadyPaid`、`duplicateOfItemId`、`duplicateOfSeq`、`extracted.{customerNumberRaw,contractNoRaw,amount,seq}`、`recognition`（整包傳給子元件 `RecognitionNote`）。型別上另有 `batchId`、`installmentId`、`businessKey`、`approvalState`、`reviewStatus`、`recognitionErrorCode`。
- 錯誤：403 → denied。

**:238 `GET /api/energy-ops/organizations/{orgId}/bills/{billId}`**（展開時載入）
- 回應：`id`、`stationId`、`station.customerNumber`、`customerNumberRaw`、`templateId`、`ocrRaw`（給 `sourceFieldBoxes()`）、`ocrExtracted`／`changedFields`（傳給 `FieldDiffGrid`）、`docType`、`recognizedDocType`、`binding`（整包傳給 `BillDocTypeField`）、`docTypeEditable`、`canEditSubject`、`executions[]`（只看長度）、`ALL_BILL_REVIEW_FIELDS` 各欄位的值（草稿初值）。
- 錯誤：吞掉。

**:270 `PATCH /api/energy-ops/organizations/{orgId}/ingest-items/{itemId}`**
- body：目前只有一種，`{ docType }`（:730，`failed`／`unclassified` 列的「指定單別」，可選值是 `BILL_DOC_TYPES` 加上 `depositRequest`）。
- 回應：未使用（之後 `load()`）。

**:289 `PATCH /api/energy-ops/organizations/{orgId}/bills/{billId}`**
- body：`{ markReviewed: boolean, ...ALL_BILL_REVIEW_FIELDS }`。草稿裡有的欄位都送（不比對差異）；空字串送 `null`；`totalAmount`、`generationKwh` 轉成數字；日期取前 10 碼；其餘 `trim`。
- 回應：未使用（接著打 :305 重讀）。

**:305 `GET /api/energy-ops/organizations/{orgId}/bills/{billId}`**（存檔後重讀）
- 回應：整包 `setBill`，並取 `ALL_BILL_REVIEW_FIELDS` 重設草稿（欄位同 :238）。

**:319 `POST /api/energy-ops/organizations/{orgId}/ingest-items/{itemId}/submit`**
- body：`{}`。「全部送出」會對每個 ready 項逐一呼叫。
- 回應：未使用。
- 錯誤：`apiErrorPayload(e).code ∈ {duplicate_document, already_in_review, duplicate_in_batch}` 時開重複件對話框，讀 `details.{itemId,seq,originalFilename,submittedByName,submittedAt,currentNodeName,documentNo,subjectId,executionId}`；其他錯誤用 `apiErrorCode` 跳通知。

**:339 `POST /api/energy-ops/organizations/{orgId}/ingest-items/{itemId}/discard`**
- body：`{ reason: 'duplicate' }`。
- 回應：未使用。

**:357 `GET /api/energy-ops/organizations/{orgId}/stations`**
- query：`q?`（初值是 `item.extracted.customerNumberRaw ?? contractNoRaw`）。
- 回應：陣列 `[{ id, name, customerNumber }]`。
- 錯誤：吞掉。

**:377 `POST /api/energy-ops/organizations/{orgId}/bills/{billId}/match`**
- body：`{ stationId: number }`。
- 回應：未使用。錯誤顯示 `apiErrorCode`。

**:466 `GET /api/energy-ops/attachments/{attachmentId}/download`**（`href`，工作區「開原始檔」）
- `{attachmentId}` = `item.attachmentId`。另外同一個 id 也交給 `SourcePreviewFrame`（`shared.tsx:506`）。

**:524 `GET /api/energy-ops/organizations/{orgId}/bills/{billId}`**（`BillDocTypeField.onSaved` 後重讀）
- 回應：整包 `setBill`（主要為了 `binding.flows`）。
- 錯誤：吞掉。

**:930 `GET /api/energy-ops/attachments/{attachmentId}/download`**（`href`，已關閉列的下載鈕）
- `{attachmentId}` = `i.attachmentId`。

### src/admin/domain-logic/components/energyOps/IngestConsolePage.tsx

**:100 `GET /api/energy-ops/organizations/{orgId}/ingest-batches`**
- 回應：陣列。每列讀 `id`、`submittedAt`、`submittedByName`、`fileCount`、`status`、`counts`（`Record<itemStatus, n>`）、`pendingCount`、`workflowCounts`（`Record<workflowKey, n>`）。型別上還有 `billIds[]`。
- 錯誤：403 → denied。

**:125 `POST /api/energy-ops/organizations/{orgId}/ingest-batches`**
- body：`{ fileCount }`。
- 回應：`created.id`（當 batchId）。
- 錯誤：403 → denied；其他用 `apiErrorCode`。

**:136 `POST /api/energy-ops/organizations/{orgId}/ingest-batches/{batchId}/attachments`**（`uploadViaStorage`，multipart，每個檔一次）
- form：`file`、`originalFilename`、`mimeType`、`target='batch'`（沒有額外 `fields`）。
- 回應：未使用。單檔失敗吞掉，不中斷整批。

**:149 `POST /api/energy-ops/organizations/{orgId}/ingest-batches/{batchId}/recognize`**
- body：無。
- 回應：`outcome.jobId`（沒有時視為沒送出）、`outcome.skipped`（原因 code）。
- 錯誤／skipped code：`serverless_runner_ref_budget_exceeded`、`serverless_runner_budget_exceeded` → 預算用完；`serverless_runner_ref_disabled`、`serverless_runner_service_disabled` → 服務沒開；以 `serverless_runner_`／`tpr_`／`http_` 開頭的其他 code → 「稍後辨識」；其餘 code 原樣顯示。

### src/admin/domain-logic/components/energyOps/LandingRedirect.tsx

**:41 `GET /api/energy-ops/organizations/{orgId}/me/landing`**
- 回應：`data.route`（前端路由字串，用來 `navigate('/' + route)`）。
- 錯誤：吞掉，不導向。

### src/admin/domain-logic/components/energyOps/LandownerDetailPage.tsx

**:48 `GET /api/energy-ops/organizations/{orgId}/landowners/{landownerId}/overview`**
- 回應：`name`、`status`、`taxId`、`contactName`、`contactPhone`、`contactEmail`、`bankName`、`bankBranch`、`bankAccountName`、`bankAccount`；`stations[].{id,name,customerNumber,status,workflow.{name,version},strategy.name}`；`contracts[].{id,contractNo,validFrom,validTo,stage,installmentPaid,installmentTotal}`；`inFlight[].{executionId,stationName,totalAmount,nodeName,stuck,waitingDays}`；`recentBills[].{id,billingPeriodStart,billingPeriodEnd,totalAmount,payoutAmount}`；`calendarEvents[].{id,eventDate,title,status}`。
- 錯誤：403 → denied。

### src/admin/domain-logic/components/energyOps/LandownersPage.tsx

**:49 `GET /api/energy-ops/organizations/{orgId}/landowners`**
- query：`q?`（關鍵字）。
- 回應：陣列。每列讀 `id`、`name`、`taxId`、`contactName`、`contactPhone`、`stationCount`、`abnormalStationCount`、`contractCount`、`status`。
- 錯誤：403 → denied。

### src/admin/domain-logic/components/energyOps/shared.tsx

**:506 `GET /api/energy-ops/attachments/{attachmentId}/download`**（`SourcePreviewFrame`，`responseType: 'blob'`）
- 回應：`res.data` 當成 Blob（`createObjectURL`，PDF 交給 pdf.js 畫，圖片直接顯示），MIME 取自 `blob.type` 或 `res.headers['content-type']`。
- 錯誤：吞掉，顯示「看不到原檔」。
- 呼叫端：`BillDetailBody`（`sourceAttachmentId`）、`IngestBatchDetailPage`（`item.attachmentId`）。

### src/admin/domain-logic/components/energyOps/StationDetailPage.tsx

**:65 `GET /api/energy-ops/organizations/{orgId}/stations/{stationId}/overview`**
- 回應：`name`、`status`、`customerNumber`、`landowner.{id,name}`、`latitude`、`longitude`、`workflowAxis[]`／`strategyAxis[]`（每段讀 `id`、`validFrom`、`validTo`、`active`、`name`、`version`、`definitionId`；型別上還有 `type`）、`recentBills[].{id,billingPeriodStart,billingPeriodEnd,totalAmount,reviewStatus}`。
- 錯誤：403 → denied。

**:77 `GET /api/energy-ops/organizations/{orgId}/stations/{stationId}/bindings`**
- 回應：`workflow[]`、`rating[]`（以 kind 為鍵；每段讀 `docType`、`targetKey`、`isCurrent`、`name`、`version`）、`canManage.{workflow,rating}`（boolean，傳給 `BindingActions`）。
- 錯誤：吞掉。

### src/admin/domain-logic/components/energyOps/WorkbenchPage.tsx

**:124 `GET /api/energy-ops/organizations/{orgId}/workbench`**
- 回應：`data.sections[]`、`data.payoutOverdueDays`。每個 section 讀 `key`、`titleKey`、`overdueDays`、`rows[]`（型別上還有 `sort`）。每列讀 `id`（taskId）、`executionId`、`nodeKey`、`nodeName`、`waitingDays`、`claimable`、`claimedByMe`、`state`（`completed` 時變灰）、`rejectReason`、`rejectedBy`、`availableActions[]`（TaskAction）、`actionBlocked`（ActionBlocked）、`subject.{billId,stationName,customerNumber,billingPeriodStart,billingPeriodEnd,landownerName,totalAmount}`。`subject.billId` 會傳給 `BillDetailDrawer` → `BillDetailBody`。
- 錯誤：403 → denied。

**:160 `POST /api/energy-ops/organizations/{orgId}/tasks/{taskId}/actions`**
- body：`{ actionKey, reason? }`。
- 回應：`data.row`。有就就地合併（讀 `row.id`、`row.state`）；沒有就重新 `load()`。

### src/admin/domain-logic/components/energyOps/WorkflowOverviewPage.tsx

**:77 `GET /api/energy-ops/organizations/{orgId}/workflows`**
- 回應：`data.definitions[]`。每筆讀 `id`、`key`、`name`、`version`、`parentVersion`、`runningCount`、`bindings[].{id,stationKey,stationId,contractNo,docType,validFrom,validTo}`、`nodes[].{nodeKey,name}`。型別上另有 `status`、`bindings[].contractId`、`nodes[].{nodeType,waiting,stuck}`；`ExecutionsPage` 的註解說回應另有 `stuckDays`。
- 錯誤：403 → denied。

**:93 `GET /api/energy-ops/organizations/{orgId}/unbound-stations`**
- 回應：陣列，讀 `id`、`name`、`customerNumber`、`missingWorkflow`、`missingRating`。
- 錯誤：吞掉。

**:103 `GET /api/energy-ops/organizations/{orgId}/workflows/{definitionId}/graph`**
- 回應：ExecutionGraph 形狀的定義圖（節點帶 `waiting`／`stuck`），整包傳給子元件 `FlowGraph`。
- 錯誤：吞掉。

**:113 `GET /api/energy-ops/organizations/{orgId}/workflows/{definitionId}/nodes/{nodeKey}/tasks`**
- 回應：陣列。每列讀 `id`（taskId）、`executionId`、`waitingDays`、`availableActions[]`、`actionBlocked`、`subject.{stationName,customerNumber,landownerName,totalAmount}`，以及 SubjectRef 欄位（給 `taskWorkRoute`）。
- 錯誤：用 `apiErrorCode` 跳通知。

**:133 `POST /api/energy-ops/organizations/{orgId}/tasks/{taskId}/actions`**
- body：`{ actionKey }`（`requiresReason` 的動作在前端擋下，不送）。
- 回應：未使用（之後重抓節點清單和 `load()`）。

---

## 已移除／改由平台 API 取代

- **舊 App 的成員頁**：由平台的 `/api/v1/orgs/{orgId}/members` 與 `/api/v1/orgs/{orgId}/rbac/grants` 取代（第一階段）。
- **`/api/tenant-console/organizations/{orgId}/members`**（部門頁挑選器）：改 `E/members`，沒權限退回平台成員清單（第二階段）。
- **`/api/energy-ops/uploads/sign`（Supabase 直傳）**：改 `E/uploads/sign`，PUT 到新後端物件儲存的簽名網址；沒上線時退回 multipart（第二階段）。
- `ENERGY_ROOT`、`rewriteLegacyPath`、`legacyTenantConsoleBase` 已刪除；`src/api/energyLegacy.ts` 換成 `src/api/energy.ts`。
