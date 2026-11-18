# 能源 API 契約

後端能源 plugin 照這份實作，前端照這份改接。語意（閘、狀態機、錯誤情境）以 App 5 為準（`backend/edge-functions/energy-api/energy/AcEnergyOps/AcEnergyOpsRouter.ts` 與各 service）；這份只規定**新的路徑與形狀約定**。

## 約定

- 前綴 `E = /api/v1/orgs/{orgId}/energy`。所有能源端點都在組織前綴下（含附件），組織由路徑決定、由 `OrgAccess` 判權；掛在 execution 底下的資源再過一層 `ExecutionAccess`（參與者或 `energy:flow:view-all`）。
- 每個閘都另外接受 `platform:rbac:manage`（取代舊的 `ryxis:rbac:manage`），由 `Rbac::authorize_org` 自動加上。
- JSON 一律 camelCase；id 是數字；金額、度數、比例是 decimal **字串**（例 `"4321.50"`）；日期 `YYYY-MM-DD`；時間 RFC 3339。
- 清單回 `{ items, total? }`；單筆直接回物件。
- 錯誤 `{ code, message, details? }`。`code` 沿用 App 5 的錯誤碼字串（例 `duplicate_document`、`already_in_review`、`bill_needs_station`、`no_matching_transition`、`invalid_task_transition`、`last_governor`），HTTP 狀態照 App 5。
- 舊端點在 `{ORG} = /api/energy-ops/organizations/:organizationId`；下表「舊」欄給對照。

## 組織治理

| 方法 | 新路徑 | 舊 | 閘 |
|---|---|---|---|
| GET | `E/departments` | `{ORG}/departments` | GOVERN_ORG |
| POST | `E/departments` body 可帶 `parentId`（卡 C／G01，上層部門 id；沒給或 `null`＝最上層） | `{ORG}/departments` | GOVERN_ORG |
| PATCH | `E/departments/{departmentId}` body 可帶 `parentId`（同上；給 `null` 是移到最上層） | 同 | GOVERN_ORG |
| DELETE | `E/departments/{departmentId}` | 同 | GOVERN_ORG |
| PUT | `E/departments/{departmentId}/members/{userId}` body `{role: manager\|member}` | `POST {ORG}/departments/:id/members` | GOVERN_ORG |
| DELETE | `E/departments/{departmentId}/members/{userId}` | 同 | GOVERN_ORG |
| GET | `E/members` 成員＋能源角色＋部門 | （舊用 tenant-console members） | ASSIGN_ROLE |
| DELETE | `E/members/{userId}` 移除成員（治理判定看 `energy:govern:org` ∪ `platform:rbac:manage`） | tenant-console members DELETE | ASSIGN_ROLE |
| GET | `E/assignable-roles` | `{ORG}/assignable-roles` | ASSIGN_ROLE |
| POST | `E/role-assignments` body `{userId, roleUniqueName}` | 同 | ASSIGN_ROLE（特權角色另要 `energy:role:assign-privileged`） |
| POST | `E/role-assignments/revoke` body `{userId, roleUniqueName, reason?}` | 同 | ASSIGN_ROLE（同上＋最後一位治理者） |
| GET | `E/audit-events` | 同 | `energy:audit:view` |
| GET | `E/audit-actors` | 同 | `energy:audit:view` |
| GET | `/api/v1/admin/users/{userId}/relations`（卡 C／G03，一個帳號的整體關係總覽；跨組織查，不走 `E/` 前綴） | 無 | 只准超級使用者（否則 403 `super_user_required`） |

## 儀表板、工作台

| 方法 | 新路徑 | 舊 | 閘 |
|---|---|---|---|
| GET | `E/dashboard` | 同 | FLOW_VIEW |
| GET | `E/calendar-events` | 同 | FLOW_VIEW |
| GET | `E/me/landing` | 同 | FLOW_VIEW |
| GET | `E/workbench` | 同 | FLOW_VIEW |

## 流程

| 方法 | 新路徑 | 閘 |
|---|---|---|
| GET | `E/executions` | FLOW_VIEW（範圍依 view-all） |
| POST | `E/executions` | FLOW_ACT |
| GET | `E/executions/{executionIdOrNo}` | EXEC(view) |
| GET | `E/executions/{executionId}/actions` | EXEC(view) |
| GET | `E/executions/{executionId}/graph` | EXEC(view) |
| GET | `E/executions/{executionId}/revertable` | EXEC(view) |
| GET | `E/executions/{executionId}/lineage` | EXEC(view) |
| POST | `E/executions/{executionId}/abort` | FLOW_ACT |
| POST | `E/executions/{executionId}/force-advance` | FLOW_ADMIN |
| POST | `E/executions/{executionId}/terminate`（新：引擎已有，舊版沒路由） | FLOW_ADMIN |
| POST | `E/tasks/{taskId}/claim` | FLOW_ACT |
| POST | `E/tasks/{taskId}/release` | FLOW_ACT |
| POST | `E/tasks/{taskId}/actions` body `{actionKey, reason?, payload?, note?, targetNodeKey?}` | FLOW_ACT |
| POST | `E/tasks/{taskId}/abort-decision` | ABORT_APPROVE |
| POST | `E/tasks/{taskId}/reassign` | FLOW_ADMIN |
| POST | `E/side-effects/{sideEffectId}/release` | FLOW_ADMIN |
| GET | `E/delegations`（新：列出我設的與替我設的） | FLOW_VIEW |
| POST | `E/delegations` | 替自己 FLOW_ACT；替別人 `energy:delegation:manage` |
| POST | `E/delegations/{delegationId}/revoke`（新：引擎已有，舊版沒路由） | 同上 |
| POST | `E/lineage` | FLOW_ACT |
| GET | `E/workflows` | FLOW_VIEW |
| GET | `E/workflows/{definitionId}/graph` | FLOW_VIEW |
| GET | `E/workflows/{definitionId}/nodes/{nodeKey}/tasks` | FLOW_VIEW |
| POST | `E/setup/install-defaults`（新：App 5 只在 demo seed 裝） | `energy:workflow:manage` |

`setup/install-defaults` 冪等地替組織安裝內建流程定義（`bill-approval` 七節點版、`deposit-request`、`utility-bill`、`invoice-issue`，直接發佈）、預設計算策略（`station-payout` v1「售電分潤 10%」）與定義引用的三個部門（`sales`／`accounting`／`finance`，空的）。同 key 已有任何版本就略過（新版本要走換版，不是重裝）。不建綁定：綁定是案場層的設定，走 `stations/{id}/bindings`。回 `{definitions, strategies, departments}`（這次真的新裝的）。

## 附件

| 方法 | 新路徑 | 舊 | 閘 |
|---|---|---|---|
| GET | `E/executions/{executionId}/attachments` | `/api/energy-ops/executions/:id/attachments` | EXEC(attachment:download) |
| POST | `E/executions/{executionId}/attachments`（multipart；或 JSON `{stagingKey,…}`，見 presign） | 同 | EXEC(attachment:upload) |
| GET | `E/attachments/{attachmentId}/download`（直接回檔案；`?redirect=1` 時 302 到預簽網址） | `/api/energy-ops/attachments/:id/download` | execution 的走 EXEC(download)；batch 的走組織層 download |
| POST | `E/attachment-links/{linkId}/remove` body `{reason}` | `/api/energy-ops/attachment-links/:id/remove` | EXEC(attachment:unlink) |
| POST | `E/uploads/sign`（P6：presign 直傳） | `/api/energy-ops/uploads/sign` | 同上傳 |

## 電費單與收件

| 方法 | 新路徑 | 閘 |
|---|---|---|
| POST | `E/bills/ingest` | FLOW_ACT |
| GET | `E/bills` | FLOW_VIEW |
| GET | `E/bills/unmatched` | FLOW_VIEW |
| GET | `E/bills/{billId}` | FLOW_VIEW |
| PATCH | `E/bills/{billId}` | FLOW_ACT（另有模式檢查） |
| GET | `E/bills/{billId}/branch-preview` | FLOW_VIEW |
| GET | `E/bills/{billId}/payouts` | FLOW_VIEW |
| POST | `E/bills/{billId}/submit` | FLOW_ACT |
| POST | `E/bills/{billId}/match` | FLOW_ACT |
| POST | `E/ingest-batches` | FLOW_ACT |
| GET | `E/ingest-batches` | FLOW_VIEW |
| GET | `E/ingest-batches/{batchId}`（先推一次辨識進度） | FLOW_VIEW |
| POST | `E/ingest-batches/{batchId}/attachments`（multipart） | `energy:attachment:upload` |
| POST | `E/ingest-batches/{batchId}/recognize` | FLOW_ACT |
| PATCH | `E/ingest-items/{itemId}` | FLOW_ACT |
| POST | `E/ingest-items/{itemId}/recognition` | FLOW_ACT |
| POST | `E/ingest-items/{itemId}/submit` | FLOW_ACT |
| POST | `E/ingest-items/{itemId}/discard` | FLOW_ACT |
| GET | `E/recognition/status`（新：辨識服務有沒有設定、能不能用） | FLOW_VIEW |

## 主檔與綁定

| 方法 | 新路徑 | 閘 |
|---|---|---|
| GET | `E/stations`（輕量搜尋） | FLOW_VIEW |
| POST | `E/stations`（卡 20261118-0427-a：新增案場，同交易建主電號＋主要受款對象 100% 分配） | MASTER_MANAGE |
| GET | `E/station-list`（可篩 `siteType`、`enabled`；回傳帶 `meterCount`、`projectCodeMissing`） | FLOW_VIEW |
| PATCH | `E/stations/{stationId}`（卡 A：編輯，整包送上來，重算專案編號） | MASTER_MANAGE |
| GET | `E/unbound-stations` | FLOW_VIEW |
| GET | `E/stations/{stationId}/overview` | FLOW_VIEW |
| GET | `E/stations/{stationId}/geocode` | FLOW_VIEW |
| POST | `E/stations/{stationId}/geocode` | FLOW_ACT |
| GET | `E/stations/{stationId}/meters`（卡 A：電號一對多清單） | FLOW_VIEW |
| POST | `E/stations/{stationId}/meters`（新增電號；有效期重疊回 409 `meter_overlap`） | MASTER_MANAGE |
| PATCH | `E/meters/{meterId}` | MASTER_MANAGE |
| GET | `E/stations/{stationId}/allocations`（卡 A：地主分配清單） | FLOW_VIEW |
| POST | `E/stations/{stationId}/allocations`（新增分配；寫 `allocation.created`） | MASTER_MANAGE |
| PATCH | `E/allocations/{allocationId}`（一般編輯寫 `allocation.updated`；帶 `validTo`＋`changeReason` 寫 `allocation.ended`） | MASTER_MANAGE |
| GET | `E/stations/{stationId}/allocation-history`（讀稽核表，新到舊） | FLOW_VIEW |
| GET | `E/stations/{stationId}/bindings` | FLOW_VIEW |
| GET | `E/stations/{stationId}/bindings/preview` | 依 kind |
| POST | `E/stations/{stationId}/bindings` | kind=workflow：`energy:workflow:manage`；kind=rating：`energy:rating:manage` |
| POST | `E/stations/{stationId}/bindings/switch` | 同上 |
| POST | `E/stations/{stationId}/bindings/end` | 同上 |
| GET | `E/binding-options?kind=` | 依 kind |
| GET | `E/landowners`（卡 A：可篩 `partyType`，回傳帶 `partyType`、`code`、`address`） | FLOW_VIEW |
| POST | `E/landowners`（卡 A：新增受款對象） | MASTER_MANAGE |
| PATCH | `E/landowners/{landownerId}`（卡 A：編輯，整包送上來） | MASTER_MANAGE |
| GET | `E/landowners/{landownerId}` | FLOW_VIEW |
| GET | `E/landowners/{landownerId}/overview` | FLOW_VIEW |

閘的常數（照 App 5 `AcEnergyOpsController.ts:88-104`）：
GOVERN_ORG＝department:manage ∪ member:manage；ASSIGN_ROLE＝member:manage；FLOW_ACT＝flow:act；FLOW_VIEW＝flow:view ∪ flow:view-all；FLOW_ADMIN＝flow:admin；ABORT_APPROVE＝flow:abort-approve；MASTER_MANAGE＝`energy:master:manage`（卡 20261118-0427-a 新增：案場／電號／受款對象／地主分配的新增與編輯）（皆 `energy:` 前綴）。

## 回應形狀（本期已實作的部分）

- 清單一律 `{ items }`；流程單清單另帶 `{ tab, scope, total, page, pageSize }`；`assignable-roles` 另帶 `canAssignPrivileged`。
- 會改資料的 task 端點（`claim`、`actions`）回 `{ ok, row }`：`row` 是更新後的那一列（與工作台列同一份 DTO），前端就地替換。其餘動作回 `{ ok: true }`。
- `PUT E/departments/{id}/members/{userId}` body `{ role?: "manager"|"member" }`（預設 member），回 `{ userId, name, role }`；`DELETE` 回 `{ removed }`。
- `GET E/members` 每列 `{ userId, name, email, orgRole, energyRoles: [{uniqueName, name}], departments: [{id, key, name, role}] }`。`DELETE E/members/{userId}` 撤掉該員在本組織的所有授權、成員關係與部門成員列；撤完若組織已無人持有 `energy:govern:org` 或 `platform:rbac:manage` 就整筆拒絕（400 `cannot_remove_last_admin`）。
- 部門列（卡 C／G01）多一欄 `parentId`（`number | null`）。新增、修改可以帶 `parentId`：上層必須是同一個組織、還沒刪除的部門，不然 400 `parent_not_found`；不能把部門掛到自己或自己的下層，不然 400 `department_cycle`；刪除部門時它的子部門改掛到它的上層。
- `GET /api/v1/admin/users/{userId}/relations`（卡 C／G03）回 `{ organizations, projects, departments, grants }`：`organizations` 每列 `{ organizationId, organizationName, role }`；`projects` 每列 `{ projectId, projectName, organizationId, organizationName }`；`departments` 每列 `{ departmentId, path, organizationName, role }`（`path` 是後端組好的完整路徑，例「總管理部 › 會計部門」）；`grants` 每列 `{ roleUniqueName, roleName, scope, scopeName, grantedAt, grantedBy }`（`scope` 是 `organization`／`project`／`resource`，`scopeName` 是對應的組織／專案名稱或資源識別碼）。
- `GET E/executions/{idOrNo}`：純數字當 id，否則當單號（單號在 subject 上，同單號多列時已通過的優先）。回執行摘要＋`tasks`＋模式判定（`mode`、`modeReason`、`canEditSubject`、`canAddAttachment`、`availableActions`、`executionId`）。
- `GET E/executions/{id}/actions?nodeKey=` 回 `{ availableActions, actionBlocked, taskId, claimable, claimedByMe }`（動作已依「我」能不能做過濾）。
- `GET E/delegations?all=1`：預設列「我設的與替我設的」（`all=1` 且有 `energy:delegation:manage` 時列全組織），每列帶 `active`、`mine`、`delegatorName`、`delegateName`。`POST E/delegations/{id}/revoke` 已收回的回 409 `delegation_already_revoked`。
- `POST E/executions/{id}/terminate` body `{ reason }`；已結束（completed／aborted／terminated）的回 409 `execution_not_running`。
- 錯誤碼沿用 App 5：跨組織指到別家的資源回 403 `<resource>_not_in_org`（execution、task、department、station、landowner、definition、version），本組織查無回 404 `<resource>_not_found`。

## 前端第二階段對接時發現的落差（待後端確認；契約本身未改）

對 `rust-port`＠815dfdd 實跑（2026-11-04）。前端都已在 `web/src/api/energy.ts` 吸收或照下面的假設接好。

1. **`GET E/workflows` 不是 `{ items }`**：回 `{ definitions, stuckDays }`（與 App 5 相同）。和「清單一律 `{ items }`」的約定不一致；前端照實讀 `definitions`。要統一的話請一併改這裡，前端轉換表加一行即可。
2. **`GET E/workflows/{id}/nodes/{nodeKey}/tasks` 的 `claimable` 與 `actionBlocked` 互相矛盾**：不是候選人時列上是 `claimable: true` 且 `actionBlocked.code = "notCandidate"`；同一個 task 在 `GET E/executions/{id}/actions` 是 `claimable: false`。工作台列與流程總覽抽屜共用這份 DTO，應該照 `node_actions` 的判法（被擋就不能認領）。
3. **附件那一節（`E/executions/{id}/attachments`、`E/attachments/{id}/download`、`E/attachment-links/{id}/remove`、`E/uploads/sign`）在 rust-port 尚未有路由**（404 `route_not_found`），前端把它們和收件組一起標成「待上線」。
4. **`POST E/uploads/sign` 的形狀契約沒寫**。前端照 App 5：body `{ target: "batch"|"execution", batchId|executionId, mimeType, size, originalFilename }`，回 `{ stagingKey, signedUrl }`（也接受 `uploadUrl`／`url`，可選 `method`、`headers`），之後瀏覽器 `PUT` 原檔（`content-type` = mimeType）到簽名網址，再對原上傳端點送 JSON `{ stagingKey, originalFilename, mimeType, ...欄位 }`。端點不存在時要回 404 `route_not_found` 或 501，前端才會退回 multipart；其他錯誤碼（403、404 `batch_not_found`、413）會直接顯示給使用者。簽名網址指向 MinIO 時要開 CORS（瀏覽器直接 PUT）。
5. **`GET E/recognition/status` 的形狀契約沒寫**。前端讀法：`configured: false`（或 `enabled: false`、`status: "not_configured"`）→「辨識服務未開」；`available: false`（或 `reachable: false`、`status: "unavailable"`）→「暫時連不上」；`configured: true`／`available: true`／`status: "ok"` → 正常；其他或 404 → 不顯示。建議回 `{ configured, available, reason? }`。
6. **候選池在關卡 ready 時快照**：task 進 ready 之後才加進部門的人，仍是 `notCandidate`（工作台看不到、不能認領）。照 App 5 語意是對的，但畫面沒有提示；之後要不要給「重新解析候選池」的管理動作另議。
7. **`GET E/executions` 清單列的頂層 `stationId` 是 null**（`subject.stationId` 有值；以 `stationKey` 開的單）。前端只讀 `subject.*`，不受影響；`stationId` 篩選條件若是比這個欄位會漏。
8. **`POST E/setup/install-defaults` 的稽核**：只看到「發佈了新的計算策略版本」與三筆「新增了部門」，四個流程定義的安裝沒有稽核事件。

### 處理結果（2026-11-04，後端）

- 1：`GET E/workflows` 維持 `{definitions, stuckDays}`（App 5 的形狀，前端已直接使用），這裡記為例外。
- 2：已改。節點待辦清單的 `claimable` 與 `executions/{id}/actions` 同語意＝ready 且我是候選人。
- 6：維持 App 5 行為（候選人在關卡進入 ready 時快照）。部門之後異動不影響已開出的關卡；要重新指派用 `tasks/{id}/reassign`。
- 7：已改。清單列的 `stationId` 在 execution 沒有時退回 subject 推導的值。
- 8：維持 App 5。稽核 registry 沒有「安裝流程定義」事件，install-defaults 只對策略與部門留紀錄。
## 回應形狀：附件、電費單與收件

### 附件

- `GET E/executions/{id}/attachments` → `{ items: [{ id, originalFilename, mimeType, size, hash, status: active|superseded, uploadedByUserId, uploadedAt, supersedesAttachmentId, links: [{ id, taskId, role, seq, note }] }] }`（新到舊）。閘：`energy:attachment:download` ＋ execution 可視度（非參與者 403 `execution_not_visible`）。
- `POST E/executions/{id}/attachments`：multipart 欄位 `file`，其餘 form 欄位 `taskId`、`role`（預設 `submitted`；`added` 另記 `flow.attachment_added`）、`note`、`supersedesAttachmentId`；或 JSON 直傳（見下）。回 `{ id, originalFilename, mimeType, size, hash, status, executionId, ingestBatchId, supersedesAttachmentId }`。錯誤：415 `unsupported_mime_type`（`details.allowed`）、400 `empty_file`／`filename_required`／`file_required`／`task_not_in_execution`／`supersedes_not_in_execution`、413 `file_too_large`、409 `too_many_files`／`execution_quota_exceeded`、502 `storage_verify_failed`（物件沒落地，不寫 DB）。
- `GET E/attachments/{id}/download`：回檔案本體，`Content-Type` 為原 mime、`Content-Disposition` 同時帶 `filename` 與 `filename*=UTF-8''…`。`?redirect=1` 回 302，`Location` 是 15 分鐘的預簽 GET（帶 `response-content-disposition`）。兩種都記 `attachment.downloaded`（best-effort）。execution 的附件走可視度；收件批次的附件只看組織層 `energy:attachment:download`。別家的 403 `attachment_not_in_org`，查無 404。
- `POST E/attachment-links/{linkId}/remove` body `{ reason }` → `{ ok: true }`。只解除連結，附件列與物件都留著；`reason` 空白 400 `reason_required`。閘：`energy:attachment:unlink` ＋ 可視度。
- `POST E/uploads/sign` body `{ target: "batch"|"execution", batchId|executionId, mimeType, size, originalFilename? }` → `{ stagingKey, signedUrl, method: "PUT", headers: { "content-type": mimeType }, expiresInSeconds }`。閘與真正的上傳相同（batch：`energy:attachment:upload` ＋ 批次屬於本組織，查無 404 `batch_not_found`；execution：同上＋可視度）。415／400 `empty_file`／413 照上傳的規則先擋。瀏覽器把原檔 PUT 到 `signedUrl`（網址的主機就是設定的 `EO_S3_ENDPOINT`），再對原上傳端點送 JSON `{ stagingKey, originalFilename, mimeType, …其餘欄位 }`。`stagingKey` 必須以 `energy/staging/u<我的 userId>/<batch-<id>|exec-<id>>/` 開頭（否則 400 `staging_key_invalid`），物件不在 400 `staged_file_missing`；成功或失敗都會刪掉暫存物件。
  - CORS：MinIO 社群版沒有 bucket 層級的 PutBucketCors，預設（`MINIO_API_CORS_ALLOW_ORIGIN` 未設＝`*`）已允許任何來源的 PUT；正式環境若收緊 CORS，要把前端來源加進 MinIO 的 `MINIO_API_CORS_ALLOW_ORIGIN`。直傳失敗時前端仍可退回 multipart。

### 電費單

單據列（`BillView`）：`{ id, stationId, customerNumberRaw, sourceAttachmentId, ocrExtracted, billingPeriodStart, billingPeriodEnd, totalAmount, generationKwh, issuedAt, dueDate, templateId, docType, recognizedDocType, reviewStatus: pendingMatch|pendingReview|reviewed, changedFields: string[], businessKey, documentNo, approvalState }`。三層值：`ocrRaw`（只在詳情）／`ocrExtracted`（辨識抽出值，永不被校對改）／其餘欄位是最終值。金額 `"98765.00"`、度數 `"4321.500"`。

- `POST E/bills/ingest` body `{ ocrJson, sourceAttachmentId? }` → `{ billIds, matched, unmatched }`；一筆都抽不到 422 `nothing_extracted`。
- `GET E/bills?reviewStatus&stationId&limit&from&to` → `{ items: [BillView & { executionId }] }`（`executionId` 是最新一筆流程，null ＝ 尚未送審；`from`/`to` 是計費期間起日的半開區間，格式錯 400 `invalid_date`）。
- `GET E/bills/unmatched` → `{ items: [BillView] }`（例外件）。
- `GET E/bills/{id}` → `BillView` ＋ `ocrRaw`、`station {id,name,customerNumber}|null`、`executions [{id,status,currentNodeKey,businessDate,workflowKey}]`（新到舊）、`ingest {batchId,itemId,originalFilename,attachmentId,submittedAt,submittedByName}|null`、`binding { bound, reason: null|no_station|no_binding|definition_not_published, docType, stationName?, flows: [{workflowKey,workflowName,version,matchedDocType,contractId,contractNo}] }`、`docTypeEditable`，以及模式判定（`mode`、`modeReason`、`canEditSubject`、`canAddAttachment`、`availableActions`、`executionId`）。
- `PATCH E/bills/{id}` body 只收最終值：`customerNumberRaw`、`billingPeriodStart`、`billingPeriodEnd`、`totalAmount`、`generationKwh`、`issuedAt`、`dueDate`（皆可 null）、`docType`、`markReviewed`；其他鍵丟棄。回與 `GET` 相同的整包。模式不是 intake／revise → 403 `subject_not_editable`（`details.mode`）。改電號重新配案場、改電號或期間重算 `businessKey`；流程在跑時改電號／單別 409 `customer_number_locked_by_execution`／`doc_type_locked_by_execution`；沒有案場卻標已校對 409 `station_required_before_review`；單別值域外 400 `invalid_doc_type`。改金額跨過門檻另記 `bill.route_gate_removed`（critical）／`bill.route_gate_added`。
- `GET E/bills/{id}/branch-preview?totalAmount=` → `{ available, reason, definitionId, definitionName, version, currentAmount, previewAmount, nodes: [{nodeKey,name,added}], removed: [{nodeKey,name}] }`；沒案場／沒綁定時 `{ available: false, reason: no_station|no_binding, nodes: [], threshold: null }`；金額不是數字 400 `invalid_amount`。
- `GET E/bills/{id}/payouts` → `{ items: [PayoutView & { strategyName, strategyVersion }] }`（含被取代的）。
- `POST E/bills/{id}/submit` body `{ businessDate? }` → `{ id, status, currentNodeKey, executionIds }`。錯誤：409 `bill_needs_station`、409 `bill_already_submitted`（`details.executionId`）、409 `duplicate_document`／`already_in_review`（`details`：`executionId, status, currentNodeName, submittedByName, submittedAt, subjectId, documentNo?`）、404 `no_binding_for_business_date`。
- `POST E/bills/{id}/match` body `{ stationId }` → `{ id, reviewStatus, stationId }`；送件那一列若是 `unmatched` 一併轉 `extracted`。

### 收件

項目列（`ItemView`）：`{ id, batchId, originalFilename, docType, workflowKey, classifyConfidence, classifyReason, attachmentId, billId, installmentId, executionId, executionIds, status: uploaded|recognizing|classified|extracted|unclassified|unmatched|failed|submitted|discarded, errorMessage, amountMismatch, alreadyPaid, extracted, recognition: RecognitionSummary|null, recognitionErrorCode }`；批次詳情裡再加 `subjectType, subjectId, documentNo, businessKey, approvalState, stationId, reviewStatus, duplicateOfItemId, duplicateOfSeq`（同批重複：第一筆是本尊，`duplicateOfSeq` 是畫面上的第幾筆）。

- `POST E/ingest-batches` body `{ fileCount? , files?: [{ originalFilename, attachmentId?, ocrRaw }], note? }` → 批次詳情。`fileCount` 1–50（400 `no_files`／`too_many_files`）。帶 `ocrRaw` 的檔案直接分類抽取（外部管線）；UI 只開批、再逐檔上傳。
- `GET E/ingest-batches?limit` → `{ items: [{ id, submittedAt, submittedByUserId, submittedByName, fileCount, status, note, counts: {submitted,classified,extracted,unclassified,unmatched,discarded,failed}, pendingCount, billIds, workflowCounts }] }`，未完成的置頂。
- `GET E/ingest-batches/{id}` → `{ id, submittedAt, submittedByUserId, submittedByName, fileCount, status: uploading|recognizing|ready|partial|completed|failed, note, pendingCount, items: [ItemView…] }`。讀取前先對這一批推一次辨識進度。
- `POST E/ingest-batches/{id}/attachments`（multipart `file` 或直傳 JSON）→ `{ id, originalFilename, size, mimeType, itemId }`；閘 `energy:attachment:upload`。
- `POST E/ingest-batches/{id}/recognize` body `{ passwords?: { "<itemId>": "…" } }` → `{ jobId, itemCount, chunks?, skipped?: nothing_to_recognize|tpr_not_configured|no_readable_files|<服務錯誤碼>, configured, batch }`。密碼不落地、不進稽核。
- `PATCH E/ingest-items/{id}` body `{ docType?, workflowKey?, installmentId? }` → `{ id, status, docType, workflowKey, billId, installmentId }`。指定電費單類單別而沒有單時建一張空白單（`bill.created_manually`），item 轉 `unmatched`；指定 `workflowKey` 會釘住（送審不再依綁定解析）；`installmentId` 必須是本組織的期數。
- `POST E/ingest-items/{id}/recognition` body `{ ocrRaw }` → 批次詳情（記 `ingest.recognition_supplied`）；缺 `ocrRaw` 400 `ocr_raw_required`。
- `POST E/ingest-items/{id}/submit` body `{ businessDate? }` → `{ executionId, subjectType, subjectId, executionIds }`。錯誤：409 `item_needs_doc_type`／`item_needs_source_match`／`item_already_submitted`／`item_discarded`／`item_has_no_bill`／`bill_needs_station`／`duplicate_in_batch`（`details: {itemId, seq, originalFilename}`）／`duplicate_document`／`already_in_review`，422 `unsupported_doc_type`，404 `no_binding_for_business_date`。依綁定一次開所有流程（同一交易），每條都接上原檔。
- `POST E/ingest-items/{id}/discard` body `{ reason? }` → `{ id, status }`；已送審 409 `item_already_submitted`。
- `GET E/recognition/status` → `{ configured, available, reason?: tpr_not_configured|tpr_not_ready, batchSize, tickSeconds }`。

辨識驅動：`recognize` 只送第一批（`EO_TPR_BATCH_SIZE`，預設 10、上限 50），其餘留在 `recognizing`；之後每次 tick（批次詳情被讀取時，或背景任務每 `EO_RECOGNITION_TICK_SECS` 秒，預設 60、0 ＝ 不啟動）只問服務一次，完成就寫回並送下一批。同一批同時只有一個 tick（批次 `data.driverLeaseUntil` 的原子租約，3 分鐘）。超過時間預算（每份 120 秒、最少 120 秒）或連不上 → 退回 `uploaded` 並記 `ingest.recognition_failed`；400／413／415 是永久錯誤，直接標 `failed`。停在 `uploading` 超過 10 分鐘的批次由背景任務重送。
