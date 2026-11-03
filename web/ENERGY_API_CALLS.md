# 能源頁面 API 呼叫清單（舊路徑 → 第二階段對新後端用）

## 說明

`src/admin/domain-logic/components/energyOps/` 底下的頁面是從舊 App（App 5）搬來的。

- **第一階段（目前）**：所有能源 API 呼叫都已改成經過 `src/api/energyLegacy.ts`。
  - `energyHttp.get/post/patch/put/delete`：介面與 axios 相同（回 `{ data }`），`withCredentials: true`，request interceptor 會把 `config.url` 丟進 `rewriteLegacyPath()`（第一階段原樣回傳）。
  - `energyBase(orgId)` = `/api/energy-ops/organizations/${orgId}`
  - `ENERGY_ROOT` = `/api/energy-ops`
  - `attachmentDownloadUrl(id)` = `/api/energy-ops/attachments/${id}/download`（有經過 `rewriteLegacyPath`）
  - `legacyTenantConsoleBase(orgId)` = `/api/tenant-console/organizations/${orgId}`
  - `uploadViaStorage(endpoint, file, target, fields)`：組 `FormData` 後 `POST endpoint`。欄位依序是 `file`（檔案本體）、`originalFilename`、`mimeType`（瀏覽器沒給時依副檔名補）、`target`（只放 `target.target`，也就是 `'batch'` 或 `'execution'`；`organizationId`／`batchId`／`executionId` **不會**送出）、再加上 `fields` 的每個鍵（物件會 `JSON.stringify`）。
  - 路徑還是舊 App 的 `/api/energy-ops/*`，**還沒對到新後端**。
- **第二階段**：只改 `energyLegacy.ts`，也就是 `energyBase`／`rewriteLegacyPath`／`attachmentDownloadUrl`／`uploadViaStorage`／`legacyTenantConsoleBase`，把下面每個端點對到新後端 `/api/v1/orgs/{orgId}/energy/...`。呼叫端不用動。回應形狀和新後端不同時，要在 `energyLegacy.ts` 裡轉換（例如用 response interceptor 依路徑轉換），或另外開卡改呼叫端。
- **要留意的兩支不分組織的路徑**：`GET/POST ${ENERGY_ROOT}/executions/{executionId}/attachments` 和 `GET ${ENERGY_ROOT}/attachments/{attachmentId}/download` 的路徑裡沒有 `orgId`。如果新後端一律走 `/api/v1/orgs/{orgId}/energy/...`，`rewriteLegacyPath` 拿不到 orgId，需要另外處理（例如從目前選定的組織取得）。

記號：

- 下文的 `{orgId}` 一律是 `useSelection()?.organizationId`。
- 「呼叫端」的路徑都相對於 `web/`。
- 「403 → denied」表示 `e.response.status === 403` 時頁面改畫「無權限」，其餘錯誤用 `apiErrorCode(e)` 跳通知。這是大多數載入呼叫的共同處理方式，下面只在有特別判斷的地方另外寫出 code。
- `apiErrorCode(e)`／`apiErrorPayload(e)` 來自 `src/admin/dataProvider/apiError`。
- `ALL_BILL_REVIEW_FIELDS` = `customerNumberRaw, billingPeriodStart, billingPeriodEnd, totalAmount, generationKwh, issuedAt, dueDate`（`src/common/plugins/EnergyOpsCommonPlugin/taipowerExtractor.ts`）。

共用的回應形狀（下文直接引用名稱）：

- **ExecutionGraph**（`FlowGraph.tsx`）：`{ executionId?, status?, current?: string[], nodes: [{ nodeKey, name, nodeType, layout: {x,y}|null, state, serviceKey, waiting?, stuck? }], edges: [{ from, to, kind ('sendBack' 等), label, labelKey, hasCondition, implied }] }`
- **TaskAction**（`ActionBar.tsx`）：`{ actionKey, labelKey, kind, requiresReason, sortOrder }`
- **ActionBlocked**（`ActionBar.tsx`）：`{ code, nodeName?, assigneeHint? }`
- **SubjectRef**（`routes.ts`，給 `subjectRoute`／`taskWorkRoute` 決定連結）：`{ kind ('energyBill'|'energyInstallment'), billId?, installmentId?, landownerId?, stationId? }`

---

## 總表（依 method＋路徑去重）

共 **55 個端點**、**77 個呼叫點**（`attachmentDownloadUrl` 的 `<a href>` 也算呼叫點）。

| # | 端點（舊路徑） | 呼叫端 |
|---|---|---|
| 1 | `GET /api/energy-ops/organizations/{orgId}/audit-events` | `src/admin/domain-logic/components/energyOps/AuditPage.tsx:99`、`.../BillDetailBody.tsx:254` |
| 2 | `GET /api/energy-ops/organizations/{orgId}/audit-actors` | `.../AuditPage.tsx:116` |
| 3 | `GET /api/energy-ops/organizations/{orgId}/bills` | `.../BillsPage.tsx:145` |
| 4 | `GET /api/energy-ops/organizations/{orgId}/bills/unmatched` | `.../BillsMenuItem.tsx:38`、`.../BillsPage.tsx:173` |
| 5 | `GET /api/energy-ops/organizations/{orgId}/bills/{billId}` | `.../BillDetailBody.tsx:219`、`.../IngestBatchDetailPage.tsx:238`、`.../IngestBatchDetailPage.tsx:305`、`.../IngestBatchDetailPage.tsx:524` |
| 6 | `PATCH /api/energy-ops/organizations/{orgId}/bills/{billId}` | `.../BillDetailBody.tsx:378`、`.../BillDetailBody.tsx:387`、`.../BillDocTypeField.tsx:58`、`.../IngestBatchDetailPage.tsx:289` |
| 7 | `GET /api/energy-ops/organizations/{orgId}/bills/{billId}/payouts` | `.../BillDetailBody.tsx:240` |
| 8 | `GET /api/energy-ops/organizations/{orgId}/bills/{billId}/branch-preview` | `.../BillDetailBody.tsx:296` |
| 9 | `POST /api/energy-ops/organizations/{orgId}/bills/{billId}/match` | `.../BillDetailBody.tsx:491`、`.../IngestBatchDetailPage.tsx:377` |
| 10 | `POST /api/energy-ops/organizations/{orgId}/bills/{billId}/submit` | `.../BillDetailBody.tsx:615`、`.../BillsPage.tsx:188` |
| 11 | `GET /api/energy-ops/organizations/{orgId}/executions` | `.../ExecutionsPage.tsx:173` |
| 12 | `GET /api/energy-ops/organizations/{orgId}/executions/{executionId}` | `.../BillDetailBody.tsx:273`、`.../ExecutionDetailPage.tsx:102` |
| 13 | `GET /api/energy-ops/organizations/{orgId}/executions/{executionId}/graph` | `.../ExecutionDetailPage.tsx:109`、`.../FlowGraph.tsx:340` |
| 14 | `GET /api/energy-ops/organizations/{orgId}/executions/{executionId}/lineage` | `.../BillDetailBody.tsx:256` |
| 15 | `GET /api/energy-ops/organizations/{orgId}/executions/{executionId}/actions` | `.../BillDetailBody.tsx:261` |
| 16 | `GET /api/energy-ops/executions/{executionId}/attachments` | `.../BillDetailBody.tsx:252` |
| 17 | `POST /api/energy-ops/executions/{executionId}/attachments`（multipart，`uploadViaStorage`） | `.../BillDetailBody.tsx:548` |
| 18 | `GET /api/energy-ops/attachments/{attachmentId}/download` | `.../shared.tsx:506`（blob）、`.../BillDetailBody.tsx:966`（href）、`.../BillDetailBody.tsx:1387`（href）、`.../IngestBatchDetailPage.tsx:466`（href）、`.../IngestBatchDetailPage.tsx:930`（href） |
| 19 | `POST /api/energy-ops/organizations/{orgId}/tasks/{taskId}/actions` | `.../BillDetailBody.tsx:409`、`.../WorkbenchPage.tsx:160`、`.../WorkflowOverviewPage.tsx:133` |
| 20 | `POST /api/energy-ops/organizations/{orgId}/tasks/{taskId}/claim` | `.../BillDetailBody.tsx:446` |
| 21 | `GET /api/energy-ops/organizations/{orgId}/workbench` | `.../WorkbenchPage.tsx:124` |
| 22 | `GET /api/energy-ops/organizations/{orgId}/workflows` | `.../ExecutionsPage.tsx:208`、`.../WorkflowOverviewPage.tsx:77` |
| 23 | `GET /api/energy-ops/organizations/{orgId}/workflows/{definitionId}/graph` | `.../WorkflowOverviewPage.tsx:103` |
| 24 | `GET /api/energy-ops/organizations/{orgId}/workflows/{definitionId}/nodes/{nodeKey}/tasks` | `.../WorkflowOverviewPage.tsx:113` |
| 25 | `GET /api/energy-ops/organizations/{orgId}/unbound-stations` | `.../WorkflowOverviewPage.tsx:93` |
| 26 | `GET /api/energy-ops/organizations/{orgId}/stations` | `.../BillDetailBody.tsx:472`、`.../IngestBatchDetailPage.tsx:357` |
| 27 | `GET /api/energy-ops/organizations/{orgId}/station-list` | `.../BillsPage.tsx:162`、`.../DashboardPage.tsx:102` |
| 28 | `GET /api/energy-ops/organizations/{orgId}/stations/{stationId}/overview` | `.../StationDetailPage.tsx:65` |
| 29 | `GET /api/energy-ops/organizations/{orgId}/stations/{stationId}/bindings` | `.../StationDetailPage.tsx:77` |
| 30 | `GET /api/energy-ops/organizations/{orgId}/stations/{stationId}/bindings/preview` | `.../BindingActions.tsx:97` |
| 31 | `POST /api/energy-ops/organizations/{orgId}/stations/{stationId}/bindings` | `.../BindingActions.tsx:110` |
| 32 | `POST /api/energy-ops/organizations/{orgId}/stations/{stationId}/bindings/switch` | `.../BindingActions.tsx:114` |
| 33 | `POST /api/energy-ops/organizations/{orgId}/stations/{stationId}/bindings/end` | `.../BindingActions.tsx:118` |
| 34 | `GET /api/energy-ops/organizations/{orgId}/binding-options` | `.../BindingActions.tsx:82` |
| 35 | `GET /api/energy-ops/organizations/{orgId}/stations/{stationId}/geocode` | `.../DashboardPage.tsx:113` |
| 36 | `POST /api/energy-ops/organizations/{orgId}/stations/{stationId}/geocode` | `.../DashboardPage.tsx:124` |
| 37 | `GET /api/energy-ops/organizations/{orgId}/dashboard` | `.../DashboardPage.tsx:93` |
| 38 | `GET /api/energy-ops/organizations/{orgId}/calendar-events` | `.../CalendarPage.tsx:92` |
| 39 | `GET /api/energy-ops/organizations/{orgId}/landowners` | `.../LandownersPage.tsx:49` |
| 40 | `GET /api/energy-ops/organizations/{orgId}/landowners/{landownerId}/overview` | `.../LandownerDetailPage.tsx:48` |
| 41 | `GET /api/energy-ops/organizations/{orgId}/me/landing` | `.../LandingRedirect.tsx:41` |
| 42 | `GET /api/energy-ops/organizations/{orgId}/departments` | `.../DepartmentsPage.tsx:60` |
| 43 | `POST /api/energy-ops/organizations/{orgId}/departments` | `.../DepartmentsPage.tsx:80` |
| 44 | `DELETE /api/energy-ops/organizations/{orgId}/departments/{departmentId}` | `.../DepartmentsPage.tsx:95` |
| 45 | `POST /api/energy-ops/organizations/{orgId}/departments/{departmentId}/members` | `.../DepartmentsPage.tsx:108` |
| 46 | `DELETE /api/energy-ops/organizations/{orgId}/departments/{departmentId}/members/{userId}` | `.../DepartmentsPage.tsx:121` |
| 47 | `GET /api/tenant-console/organizations/{orgId}/members`（舊成員 API，第二階段改平台 API） | `.../DepartmentsPage.tsx:70` |
| 48 | `GET /api/energy-ops/organizations/{orgId}/ingest-batches` | `.../IngestConsolePage.tsx:100` |
| 49 | `POST /api/energy-ops/organizations/{orgId}/ingest-batches` | `.../IngestConsolePage.tsx:125` |
| 50 | `GET /api/energy-ops/organizations/{orgId}/ingest-batches/{batchId}` | `.../IngestBatchDetailPage.tsx:196` |
| 51 | `POST /api/energy-ops/organizations/{orgId}/ingest-batches/{batchId}/attachments`（multipart，`uploadViaStorage`） | `.../IngestConsolePage.tsx:136` |
| 52 | `POST /api/energy-ops/organizations/{orgId}/ingest-batches/{batchId}/recognize` | `.../IngestConsolePage.tsx:149` |
| 53 | `PATCH /api/energy-ops/organizations/{orgId}/ingest-items/{itemId}` | `.../IngestBatchDetailPage.tsx:270` |
| 54 | `POST /api/energy-ops/organizations/{orgId}/ingest-items/{itemId}/submit` | `.../IngestBatchDetailPage.tsx:319` |
| 55 | `POST /api/energy-ops/organizations/{orgId}/ingest-items/{itemId}/discard` | `.../BillDetailBody.tsx:633`、`.../IngestBatchDetailPage.tsx:339` |

（表中 `.../` = `src/admin/domain-logic/components/energyOps/`）

沒有 API 呼叫的檔：`ActionBar.tsx`、`BillDetailDrawer.tsx`（內容是 `BillDetailBody`）、`FieldDiffGrid.tsx`、`fieldColors.ts`、`MyWorkConsolePage.tsx`（內嵌 `WorkbenchPage`／`ExecutionsPage`）、`RecognitionNote.tsx`、`RecognizingIndicator.tsx`、`ReviewConsolePage.tsx`、`roleDisplay.ts`、`routes.ts`、`StationMap.tsx`（吃 `DashboardPage` 傳進來的 `station-list`）。

---

## 依檔案分節的明細

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

下列舊 App 的呼叫在第一階段已拿掉，**不在**上面的清單裡：

- **舊 App 的成員頁**：已由平台的 `/api/v1/orgs/{orgId}/members` 與 `/api/v1/orgs/{orgId}/rbac/grants` 取代。
  - `GET /api/tenant-console/organizations/{orgId}/members`
  - `POST /api/tenant-console/organizations/{orgId}/members`
  - `DELETE /api/tenant-console/organizations/{orgId}/members/...`
  - `GET /api/tenant-console/organizations/{orgId}/grants`
  - `POST /api/energy-ops/organizations/{orgId}/role-assignments`
  - `POST /api/energy-ops/organizations/{orgId}/role-assignments/revoke`
  - `GET /api/energy-ops/organizations/{orgId}/assignable-roles`
- **舊 Supabase 直傳**：`/api/energy-ops/uploads/sign`（簽名網址）已拿掉。`uploadViaStorage` 暫時改成 multipart 直接送到原端點（`.../ingest-batches/{batchId}/attachments`、`/api/energy-ops/executions/{executionId}/attachments`），第二階段依新後端的物件儲存 API 調整。
- **還沒改的一支**：`DepartmentsPage.tsx:70` 仍呼叫舊的 `GET /api/tenant-console/organizations/{orgId}/members`（指派部門成員的下拉）。**第二階段**改用 `GET /api/v1/orgs/{orgId}/members`，回 `items[].userId/name/email`；呼叫端目前讀的是陣列的 `id`／`name`，要把 `items[].userId` 對到 `id`。
