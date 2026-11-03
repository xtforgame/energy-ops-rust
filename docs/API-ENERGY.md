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
| POST | `E/departments` | 同上 | GOVERN_ORG |
| PATCH | `E/departments/{departmentId}` | 同 | GOVERN_ORG |
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
| GET | `E/station-list` | FLOW_VIEW |
| GET | `E/unbound-stations` | FLOW_VIEW |
| GET | `E/stations/{stationId}/overview` | FLOW_VIEW |
| GET | `E/stations/{stationId}/geocode` | FLOW_VIEW |
| POST | `E/stations/{stationId}/geocode` | FLOW_ACT |
| GET | `E/stations/{stationId}/bindings` | FLOW_VIEW |
| GET | `E/stations/{stationId}/bindings/preview` | 依 kind |
| POST | `E/stations/{stationId}/bindings` | kind=workflow：`energy:workflow:manage`；kind=rating：`energy:rating:manage` |
| POST | `E/stations/{stationId}/bindings/switch` | 同上 |
| POST | `E/stations/{stationId}/bindings/end` | 同上 |
| GET | `E/binding-options?kind=` | 依 kind |
| GET | `E/landowners` | FLOW_VIEW |
| GET | `E/landowners/{landownerId}` | FLOW_VIEW |
| GET | `E/landowners/{landownerId}/overview` | FLOW_VIEW |

閘的常數（照 App 5 `AcEnergyOpsController.ts:88-104`）：
GOVERN_ORG＝department:manage ∪ member:manage；ASSIGN_ROLE＝member:manage；FLOW_ACT＝flow:act；FLOW_VIEW＝flow:view ∪ flow:view-all；FLOW_ADMIN＝flow:admin；ABORT_APPROVE＝flow:abort-approve（皆 `energy:` 前綴）。
