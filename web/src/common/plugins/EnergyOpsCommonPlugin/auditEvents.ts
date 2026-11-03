/**
 * energy-ops-console 的稽核事件 registry（前後端共用）。
 *
 * 設計出處：`docs/features/energy-ops-console/AUDIT.md` §3.3。
 *
 * **新增事件類型＝在這個檔加一筆常數**，不動 model、不跑 migration。
 * 三個約束讓它不會變成爛帳：
 * 1. **type 白名單** —— `AuditLogger.record` 收到未註冊的 type 直接拒絕。
 * 2. **payload 有 per-type Zod schema** —— schema 沒宣告的鍵一律丟棄（白名單語意，§4.5）。
 * 3. **查詢維度一律是真欄位** —— 要被篩選/排序/索引的東西必須升級成 `energyAuditEvent`
 * 的欄位；payload 只放「點開才看的細節」。**新增查詢維度是要改 schema 的**，這是刻意的摩擦。
 *
 * 為什麼是**純常數模組**而不是 `getXxx()` 的 IoC 收集（像 `getRbacSeeds()`）：
 * 稽核寫入在 hot path 上，registry 應該 import 即可用、不需要 plugin 啟動
 * （同 `RyxisPlugin/rbacSeeds.ts` 抽成純常數的理由）。
 *
 * 命名文法是 `<domain>.<object>.<action>`，**動詞用過去式**（`approved` 而非 `approve`）
 * ——稽核記的是已發生的事實，不是意圖。用 `.` 不用 RBAC 的 `:`：那是兩個命名空間
 * （permission＝能不能做，event＝做了什麼），同分隔符會讓人誤以為兩者一一對應。
 */

import { z } from 'zod';

/** 呈現分級。不入庫（`AUDIT.md §2.1`）——存進列裡會在調整分級時產生新舊不一致。 */
export type AuditSeverity = 'info' | 'notable' | 'critical';

export interface AuditEventDef {
  /** 事件命名空間（短名），例 `department.created`。 */
  type: string;
  /** 目標的 **AMM model 名**（不是短名）——它直接就是 `getSqlzModel(targetType)` 的參數。 */
  targetType: string;
  /** per-type 的 payload 形狀。這條就是「不亂塞」的閘。 */
  payloadSchema: z.ZodTypeAny;
  /** i18n key，前端翻譯用。 */
  summaryKey: string;
  severity: AuditSeverity;
  /**
   * 這個動作有沒有外部副作用 / 是否不可逆。
   * - `true` → **fail-closed**：稽核寫失敗則整個 transaction rollback，動作視為沒發生。
   * - `false` → best-effort：寫失敗只記 log，不影響主流程。
   *
   * 預設方向與邀請寄信的 best-effort **相反** —— 稽核的預設是 fail-closed，
   * best-effort 才是例外（`AUDIT.md §4.4`）。
   */
  hasSideEffect: boolean;
}

// ---------------------------------------------------------------------------
// payload 的共用片段
// ---------------------------------------------------------------------------

/**
 * 「差異對」——`AUDIT.md §5` 的預設粒度：只存**真的變了**的欄位，不存整包快照。
 * 機敏欄位不走這裡，降級成 `changedFields`（只記「動過」，不記前後值）。
 */
export const changesSchema = z.record(
  z.string(),
  z.object({ from: z.unknown().optional(), to: z.unknown().optional() }),
);

/**
 * 跨門檻修正的 payload（`AUDIT.md §7.5.1`）。
 *
 * `gatesBefore`/`gatesAfter` 存的是**當下**算出來的關卡清單 ——
 * 不要日後回頭重算：流程定義會換版，重算會得到跟當時不一樣的答案。
 */
const routeGateSchema = z.object({
  field: z.string(),
  before: z.union([z.string(), z.number(), z.null()]),
  after: z.union([z.string(), z.number(), z.null()]),
  gatesBefore: z.array(z.string()),
  gatesAfter: z.array(z.string()),
  gatesDelta: z.object({
    added: z.array(z.string()),
    removed: z.array(z.string()),
  }),
  /** 關卡的顯示名（畫面上不印 nodeKey）。 */
  gateNames: z.record(z.string(), z.string()).optional(),
});

/**
 * 效期綁定的三個動作（`WORKFLOW.md §3.2.9`）—— **兩種 kind 同形狀**。
 *
 * 一邊三個 type、一邊一個 type 的話，**分不出嚴重度**：
 * 「解除」與「換版」的後果完全不同（解除＝這個案場之後收不了單／算不出錢）。
 */
const bindingSchema = z.object({
  /** 判別子：`workflow` | `rating`（兩套機制不要分開做，會走鐘）。 */
  kind: z.enum(['workflow', 'rating']),
  stationId: z.number().int(),
  stationKey: z.string().optional(),
  stationName: z.string().optional(),
  /** `bill-approval` / `revenue-share` 之類。 */
  targetKey: z.string(),
  /** 新段指向的版本。解除時沒有。 */
  toVersion: z.number().int().optional(),
  /** 換版時：被截斷的那一段原本指向哪一版。 */
  fromVersion: z.number().int().optional(),
  validFrom: z.string().optional(),
  validTo: z.string().nullable().optional(),
  /** 回溯生效時，按下當時算出來的後果（事後重算會得到不同答案）。 */
  isBackdated: z.boolean().optional(),
  affectedDraftBills: z.number().int().optional(),
  runningExecutions: z.number().int().optional(),
  /** 解除必填 —— 人寫的自由文字。 */
  reason: z.string().optional(),
});

/** 機敏欄位的降級形式：只記欄位名。 */
export const changedFieldsSchema = z.array(z.string());

const departmentPayload = z.object({
  departmentKey: z.string(),
  departmentName: z.string().optional(),
});

const departmentMemberPayload = z.object({
  departmentKey: z.string(),
  departmentName: z.string().optional(),
  memberUserId: z.number().int(),
  memberLabel: z.string().optional(),
  role: z.enum(['manager', 'member']),
});

// ---------------------------------------------------------------------------
// v1 事件清單
// ---------------------------------------------------------------------------
//
// **只宣告真的有人寫的 type**。宣告了卻沒人寫，就是重演 `chatbotEvent.rawRef`
// 的坑（`AUDIT.md §1`）—— 看起來有、實際上永遠是空的。
// `bill.*` / `payout.*` / `landowner.*` 等要等領域模型（Slice 4）；
// `flow.*` 12 個 type 在 Slice 2b 隨引擎一起加。

export const ENERGY_AUDIT_EVENTS: AuditEventDef[] = [
  // ── 組織架構（Slice 1 的寫入路徑）──
  {
    type: 'department.created',
    targetType: 'energyDepartment',
    payloadSchema: departmentPayload,
    summaryKey: 'audit.type.department.created',
    severity: 'info',
    hasSideEffect: true,
  },
  {
    type: 'department.updated',
    targetType: 'energyDepartment',
    payloadSchema: departmentPayload.extend({ changes: changesSchema.optional() }),
    summaryKey: 'audit.type.department.updated',
    severity: 'info',
    hasSideEffect: true,
  },
  {
    type: 'department.deleted',
    targetType: 'energyDepartment',
    payloadSchema: departmentPayload.extend({ removedMembers: z.number().int().optional() }),
    summaryKey: 'audit.type.department.deleted',
    severity: 'notable',
    hasSideEffect: true,
  },
  {
    type: 'department.member_added',
    targetType: 'energyDepartment',
    payloadSchema: departmentMemberPayload,
    summaryKey: 'audit.type.department.memberAdded',
    severity: 'info',
    hasSideEffect: true,
  },
  {
    type: 'department.member_removed',
    targetType: 'energyDepartment',
    payloadSchema: departmentMemberPayload.partial({ role: true }),
    summaryKey: 'audit.type.department.memberRemoved',
    severity: 'info',
    hasSideEffect: true,
  },
  {
    // 主管異動會改變主責者候選池（`WORKFLOW.md §2.4.4` 的 R1/R2）→ notable。
    type: 'department.manager_changed',
    targetType: 'energyDepartment',
    payloadSchema: departmentMemberPayload.extend({
      previousRole: z.enum(['manager', 'member']),
    }),
    summaryKey: 'audit.type.department.managerChanged',
    severity: 'notable',
    hasSideEffect: true,
  },

  // ── 權限（Slice 1 的角色指派路徑）──
  {
    // `targetType` 用 `user` 而不是 `AUDIT.md §3.2` 骨架寫的 `userOrganization`：
    // 這條事件真正要回答的問題是「**這個人**的權限歷程」，把 target 定成 user 才吃得到
    // `energy_audit_event_org_target` 索引。grant row 的 id 反而放 payload
    // （它會被 revoke 換掉，不是穩定的查詢主體）。§3.2 明文「骨架不是承諾」。
    type: 'member.role_changed',
    targetType: 'user',
    payloadSchema: z.object({
      action: z.enum(['granted', 'revoked']),
      roleUniqueName: z.string(),
      roleName: z.string().optional(),
      grantId: z.number().int().optional(),
      reason: z.string().optional(),
    }),
    summaryKey: 'audit.type.member.roleChanged',
    severity: 'critical',
    hasSideEffect: true,
  },

  // ── 流程引擎（Slice 2b 的 walking skeleton 用到的六個）──
  // `WORKFLOW.md §6.1` 的完整清單有 20 個 type；這裡**只加 2b 真的會寫的**，
  // 回退/並行/副作用/中斷/血緣那些隨 2c 一起加。
  {
    type: 'flow.started',
    targetType: 'energyWorkflowExecution',
    // payload 記 definitionId/version/businessDate ——「為什麼綁這版」的證據。
    payloadSchema: z.object({
      workflowKey: z.string(),
      definitionId: z.number().int(),
      version: z.number().int(),
      businessDate: z.string(),
      stationKey: z.string(),
      subjectType: z.string(),
      subjectId: z.string(),
    }),
    summaryKey: 'audit.type.flow.started',
    severity: 'info',
    hasSideEffect: true,
  },
  {
    type: 'flow.candidates_resolved',
    targetType: 'energyWorkflowTask',
    payloadSchema: z.object({
      nodeKey: z.string(),
      potentialOwners: z.array(z.number().int()),
      excludedOwners: z.array(z.number().int()),
      resolvedBy: z.string(),
      // 解析出 0 人不是「沒事」，是要有人處理的狀態（`WORKFLOW.md §2.4.5`）。
      empty: z.boolean().optional(),
    }),
    summaryKey: 'audit.type.flow.candidatesResolved',
    severity: 'info',
    hasSideEffect: false,
  },
  {
    type: 'flow.task_claimed',
    targetType: 'energyWorkflowTask',
    payloadSchema: z.object({ nodeKey: z.string() }),
    summaryKey: 'audit.type.flow.taskClaimed',
    severity: 'info',
    hasSideEffect: false,
  },
  {
    type: 'flow.task_released',
    targetType: 'energyWorkflowTask',
    payloadSchema: z.object({ nodeKey: z.string() }),
    summaryKey: 'audit.type.flow.taskReleased',
    severity: 'info',
    hasSideEffect: false,
  },
  {
    type: 'flow.task_completed',
    targetType: 'energyWorkflowTask',
    payloadSchema: z.object({
      nodeKey: z.string(),
      outcome: z.string(),
      nextNodeKey: z.string().nullable().optional(),
      reason: z.string().optional(),
    }),
    summaryKey: 'audit.type.flow.taskCompleted',
    severity: 'info',
    hasSideEffect: true,
  },
  {
    type: 'flow.completed',
    targetType: 'energyWorkflowExecution',
    payloadSchema: z.object({ endNodeKey: z.string() }),
    summaryKey: 'audit.type.flow.completed',
    severity: 'info',
    hasSideEffect: true,
  },

  // ── 流程引擎（Slice 2c）──
  {
    type: 'flow.task_sent_back',
    targetType: 'energyWorkflowTask',
    // `reason` 必填 —— 回退／駁回沒有理由等於沒有稽核價值。
    payloadSchema: z.object({
      nodeKey: z.string(),
      targetNodeKey: z.string(),
      targetMode: z.string(),
      actionKey: z.string(),
      reason: z.string().min(1),
    }),
    summaryKey: 'audit.type.flow.taskSentBack',
    severity: 'notable',
    hasSideEffect: true,
  },
  {
    type: 'flow.branch_cancelled',
    targetType: 'energyWorkflowTask',
    // **每條被取消的分支各一筆**，不可只記一筆總結（`WORKFLOW.md §2.7.2-4`）。
    payloadSchema: z.object({
      nodeKey: z.string(),
      branchKey: z.string().nullable().optional(),
      stateWhenCancelled: z.string(),
      cause: z.enum(['revert', 'join', 'abort', 'terminate']),
    }),
    summaryKey: 'audit.type.flow.branchCancelled',
    severity: 'notable',
    hasSideEffect: true,
  },
  {
    type: 'flow.task_reassigned',
    targetType: 'energyWorkflowTask',
    payloadSchema: z.object({
      nodeKey: z.string(),
      fromUserId: z.number().int().nullable(),
      toUserId: z.number().int(),
      reason: z.string().min(1),
    }),
    summaryKey: 'audit.type.flow.taskReassigned',
    severity: 'notable',
    hasSideEffect: true,
  },
  {
    type: 'flow.state_set',
    targetType: 'energyWorkflowTask',
    // `kind='setState'`：改狀態**不移動節點**。
    payloadSchema: z.object({
      nodeKey: z.string(),
      actionKey: z.string(),
      stateKey: z.string(),
      value: z.unknown().optional(),
    }),
    summaryKey: 'audit.type.flow.stateSet',
    severity: 'info',
    hasSideEffect: false,
  },
  {
    type: 'flow.side_effect_applied',
    targetType: 'energyWorkflowSideEffect',
    // 「已付款」按下去的那一筆。`note` 是人輸入的匯款單號 —— 系統無從驗證。
    payloadSchema: z.object({
      sideEffectKey: z.string(),
      executionId: z.number().int(),
      nodeKey: z.string(),
      locksRevert: z.boolean(),
      note: z.string().optional(),
    }),
    summaryKey: 'audit.type.flow.sideEffectApplied',
    severity: 'notable',
    hasSideEffect: true,
  },
  {
    type: 'flow.side_effect_released',
    targetType: 'energyWorkflowSideEffect',
    // 管理員解除回退鎖 —— **這是全系統最需要被看見的一筆**。reason 必填。
    payloadSchema: z.object({
      sideEffectKey: z.string(),
      executionId: z.number().int(),
      reason: z.string().min(1),
    }),
    summaryKey: 'audit.type.flow.sideEffectReleased',
    severity: 'critical',
    hasSideEffect: true,
  },
  {
    type: 'flow.force_advanced',
    targetType: 'energyWorkflowExecution',
    // 管理員強制推進 —— 稽核紅旗。
    payloadSchema: z.object({
      fromNodeKey: z.string().nullable(),
      toNodeKey: z.string(),
      reason: z.string().min(1),
    }),
    summaryKey: 'audit.type.flow.forceAdvanced',
    severity: 'critical',
    hasSideEffect: true,
  },
  {
    type: 'flow.terminated',
    targetType: 'energyWorkflowExecution',
    payloadSchema: z.object({ reason: z.string().min(1) }),
    summaryKey: 'audit.type.flow.terminated',
    severity: 'notable',
    hasSideEffect: true,
  },
  {
    type: 'flow.abort_requested',
    targetType: 'energyWorkflowExecution',
    payloadSchema: z.object({ reason: z.string().min(1), approvalTaskId: z.number().int() }),
    summaryKey: 'audit.type.flow.abortRequested',
    severity: 'notable',
    hasSideEffect: true,
  },
  {
    type: 'flow.abort_approved',
    targetType: 'energyWorkflowExecution',
    // 中斷**不解除副作用**：已付款後中斷仍然中斷，但那筆副作用留在清單裡。
    payloadSchema: z.object({
      reason: z.string().optional(),
      requestReason: z.string(),
      openSideEffects: z.array(z.string()).optional(),
    }),
    summaryKey: 'audit.type.flow.abortApproved',
    severity: 'critical',
    hasSideEffect: true,
  },
  {
    type: 'flow.abort_rejected',
    targetType: 'energyWorkflowExecution',
    payloadSchema: z.object({ reason: z.string().optional(), requestReason: z.string() }),
    summaryKey: 'audit.type.flow.abortRejected',
    severity: 'notable',
    hasSideEffect: true,
  },
  {
    type: 'flow.rerun',
    targetType: 'energyWorkflowExecution',
    payloadSchema: z.object({ supersedesExecutionId: z.number().int(), reason: z.string().min(1) }),
    summaryKey: 'audit.type.flow.rerun',
    severity: 'notable',
    hasSideEffect: true,
  },
  {
    type: 'delegation.created',
    targetType: 'energyWorkflowDelegation',
    payloadSchema: z.object({
      delegatorUserId: z.number().int(),
      delegateUserId: z.number().int(),
      validFrom: z.string(),
      validTo: z.string().nullable().optional(),
      reason: z.string().optional(),
    }),
    summaryKey: 'audit.type.delegation.created',
    severity: 'notable',
    hasSideEffect: true,
  },
  {
    type: 'delegation.revoked',
    targetType: 'energyWorkflowDelegation',
    payloadSchema: z.object({
      delegatorUserId: z.number().int(),
      delegateUserId: z.number().int(),
    }),
    summaryKey: 'audit.type.delegation.revoked',
    severity: 'notable',
    hasSideEffect: true,
  },
  {
    type: 'lineage.linked',
    targetType: 'energyExecutionLineage',
    // 純紀錄，**不觸發行為**（`WORKFLOW.md §4.9.1`）。
    payloadSchema: z.object({
      upstreamExecutionId: z.number().int(),
      downstreamExecutionId: z.number().int(),
      relationKind: z.string(),
      note: z.string().optional(),
    }),
    summaryKey: 'audit.type.lineage.linked',
    severity: 'info',
    hasSideEffect: false,
  },

  // ── 流程綁定的三個動作（`WORKFLOW.md §3.2.6`）──
  //
  // **不做 CRUD**：效期不重疊是硬約束，暴露 validFrom/validTo 給人編輯，
  // 第一件事就是撞上約束而且錯誤訊息看不懂。
  // 把危險的狀態轉移包成**具名動作**（同 `energyWorkflowAction` 的思路）。
  {
    type: 'flow.binding_created',
    targetType: 'energyWorkflowBinding',
    payloadSchema: bindingSchema,
    summaryKey: 'audit.type.flow.bindingCreated',
    severity: 'info',
    hasSideEffect: true,
  },
  {
    // 換版＝**一個動作做兩件事**（截斷舊段 ＋ 開新段），不讓人手改 validTo。
    type: 'flow.binding_switched',
    targetType: 'energyWorkflowBinding',
    payloadSchema: bindingSchema,
    summaryKey: 'audit.type.flow.bindingSwitched',
    severity: 'notable',
    hasSideEffect: true,
  },
  {
    // critical —— 解除之後這個案場**收不了單**。
    type: 'flow.binding_ended',
    targetType: 'energyWorkflowBinding',
    payloadSchema: bindingSchema,
    summaryKey: 'audit.type.flow.bindingEnded',
    severity: 'critical',
    hasSideEffect: true,
  },

  // ── 附件（Slice 3）──
  {
    type: 'attachment.uploaded',
    targetType: 'energyAttachment',
    // hasSideEffect=true → fail-closed：上傳成功但沒留痕＝檔案來路不明。
    payloadSchema: z.object({
      // **兩種歸屬二擇一**（C6）：流程進行中掛上去的有 `executionId`；
      // **收件時的原始掃描檔只有 `ingestBatchId`** —— 那時 execution 還不存在。
      // 這裡若維持 `executionId` 必填，收件的每一次上傳都會被
      // fail-closed 擋成 500（而且錯誤訊息完全指不到「因為沒有 execution」）。
      executionId: z.number().int().nullable().optional(),
      ingestBatchId: z.number().int().nullable().optional(),
      taskId: z.number().int().nullable().optional(),
      role: z.string(),
      originalFilename: z.string(),
      size: z.number().int(),
      hash: z.string(),
      mimeType: z.string(),
    }),
    summaryKey: 'audit.type.attachment.uploaded',
    severity: 'info',
    hasSideEffect: true,
  },
  {
    /**
     * **補件**（`LIFECYCLE.md §6.7.3`）—— 與 `attachment.uploaded` 分開的理由：
     *
     * 那一支涵蓋所有上傳（收件時的原始掃描檔佔絕大多數）。
     * 「**流程中**才補上的」是另一件事：它通常是**有人要求**的結果
     * （主管說「缺一張證明」），而稽核在回答「這張單為什麼卡了三天」時，
     * 要能一眼看到那個往返。混在 `attachment.uploaded` 裡就要靠
     * `payload.role` 過濾 —— 而稽核頁是**按 type 篩**的。
     *
     * `severity: info` —— 補件**不推進流程、不解除任何鎖**，它只是多一份檔案。
     * `hasSideEffect: true` 同 `attachment.uploaded`：上傳成功但沒留痕
     * ＝ 檔案來路不明。
     */
    type: 'flow.attachment_added',
    targetType: 'energyAttachment',
    payloadSchema: z.object({
      executionId: z.number().int(),
      taskId: z.number().int().nullable().optional(),
      role: z.string(),
      originalFilename: z.string(),
      size: z.number().int(),
    }),
    summaryKey: 'audit.type.flow.attachment_added',
    severity: 'info',
    hasSideEffect: true,
  },
  {
    // 本設計**唯一記錄「讀取」的事件**：下載一份合約 ≈ 匯出，
    // 量級遠小於「誰看了哪一頁」。
    type: 'attachment.downloaded',
    targetType: 'energyAttachment',
    payloadSchema: z.object({ executionId: z.number().int() }),
    summaryKey: 'audit.type.attachment.downloaded',
    severity: 'info',
    hasSideEffect: false,
  },
  {
    type: 'attachment.superseded',
    targetType: 'energyAttachment',
    payloadSchema: z.object({ replacedByAttachmentId: z.number().int() }),
    summaryKey: 'audit.type.attachment.superseded',
    severity: 'notable',
    hasSideEffect: true,
  },
  {
    // 這不是刪檔 —— 檔案與 attachment 列都留著。`reason` 必填。
    type: 'attachment.link_removed',
    targetType: 'energyAttachmentLink',
    payloadSchema: z.object({
      attachmentId: z.number().int(),
      taskId: z.number().int().nullable().optional(),
      role: z.string(),
      reason: z.string().min(1),
    }),
    summaryKey: 'audit.type.attachment.linkRemoved',
    severity: 'notable',
    hasSideEffect: true,
  },

  // ── 電費單（Slice B2）──
  {
    /**
     * **辨識判不出來，改由人工建單**（2026-09-09）。
     *
     * 與 `bill.matched`／`bill.unmatched` 不同：那兩個講的是「抽出來的電號
     * 配到／配不到案場」，這一個講的是**這張單根本不是辨識產生的**。
     * 事後看到一張沒有辨識值的單時，第一個問題是「它哪來的」——
     * 少了這一筆，答案只能靠猜。
     */
    type: 'bill.created_manually',
    targetType: 'energyBill',
    payloadSchema: z.object({
      itemId: z.number().int(),
      docType: z.string(),
    }),
    summaryKey: 'audit.type.bill.created_manually',
    severity: 'notable',
    hasSideEffect: true,
  },
  {
    type: 'bill.matched',
    targetType: 'energyBill',
    payloadSchema: z.object({
      customerNumberRaw: z.string().nullable().optional(),
      stationId: z.number().int().nullable().optional(),
      previousStationId: z.number().int().nullable().optional(),
      extractedFields: z.array(z.string()).optional(),
      sourceAttachmentId: z.number().int().nullable().optional(),
      /** true ＝ 人工指認（US-5），false/undefined ＝ 抽取時自動配上。 */
      manual: z.boolean().optional(),
    }),
    summaryKey: 'audit.type.bill.matched',
    severity: 'info',
    hasSideEffect: true,
  },
  {
    // 這**不是錯誤**，是一個要有人處理的狀態（例外件，US-5）。
    // 記成 `outcome='failed'` 只是為了讓稽核清單篩得到「有東西卡住」。
    type: 'bill.unmatched',
    targetType: 'energyBill',
    payloadSchema: z.object({
      customerNumberRaw: z.string().nullable().optional(),
      stationId: z.null().optional(),
      extractedFields: z.array(z.string()).optional(),
      sourceAttachmentId: z.number().int().nullable().optional(),
    }),
    summaryKey: 'audit.type.bill.unmatched',
    severity: 'notable',
    hasSideEffect: true,
  },
  {
    type: 'bill.corrected',
    targetType: 'energyBill',
    payloadSchema: z.object({
      changes: changesSchema,
      markReviewed: z.boolean().optional(),
      // 一併記辨識值 —— 稽核列自己就答得出「OCR 當時抽到什麼」。
      ocrExtracted: z.record(z.string(), z.unknown()).optional(),
    }),
    summaryKey: 'audit.type.bill.corrected',
    severity: 'info',
    hasSideEffect: true,
  },

  // ── 人工修正**改變了審批路線**（`AUDIT.md §7.5`）──────────────
  //
  // 為什麼是**新的 type** 而不是「把 `bill.corrected` 的 severity 調高」：
  // `severity` 是**由 type 查這張表得到的靜態值**（§3.3）——
  // 做成「單筆覆寫 severity」會讓呈現分級變成**寫入端可以操縱**的東西。
  // 而且分開記更誠實：「他改了一個數字」與「那件事改變了誰要看這張單」
  // **是兩件事**。用 `causedByEvent` 串起來就好。
  //
  // **危險的方向是「少一關」不是「多一關」**：
  // 把 138,195 改成 99,999，高層加簽那一關就消失了 ——
  // 而且沒有任何人會知道。大額控管（US-8）最省事的繞法就是改那個數字。
  {
    type: 'bill.route_gate_added',
    targetType: 'energyBill',
    payloadSchema: routeGateSchema,
    summaryKey: 'audit.type.bill.route_gate_added',
    // 多一關是**無害的方向**：值得看見，但不是警訊。
    severity: 'notable',
    hasSideEffect: true,
  },
  {
    type: 'bill.route_gate_removed',
    targetType: 'energyBill',
    payloadSchema: routeGateSchema,
    summaryKey: 'audit.type.bill.route_gate_removed',
    // **critical** —— 這是規避控管的方向。
    severity: 'critical',
    hasSideEffect: true,
  },

  // ── 送件批次（Slice C6）──
  {
    /**
     * 一批文件被處理完。
     *
     * 為什麼要有這一筆（而不是只靠每張單的 `bill.matched`）：
     * **沒產出單的那些檔案在 bills 表裡查不到** —— 判不出型別的、
     * 抽取失敗的、型別還沒接的。稽核問「這一批到底發生什麼事」時，
     * 只有這一筆答得完整。
     */
    type: 'ingest.batch_classified',
    targetType: 'energyIngestBatch',
    payloadSchema: z.object({
      fileCount: z.number().int().nonnegative(),
      byStatus: z.record(z.string(), z.number()).optional(),
    }),
    summaryKey: 'audit.type.ingest.batchClassified',
    severity: 'info',
    hasSideEffect: true,
  },

  // ── 台電辨識服務（RECOGNITION.md §8）──
  {
    /** 一批檔案被送去辨識。payload 記 jobId，出事時要能對回服務端的那一批。 */
    type: 'ingest.recognition_requested',
    targetType: 'energyIngestBatch',
    payloadSchema: z.object({
      jobId: z.string().min(1),
      fileCount: z.number().int().nonnegative(),
      withPassword: z.number().int().nonnegative().optional(),
    }),
    summaryKey: 'audit.type.ingest.recognitionRequested',
    severity: 'info',
    hasSideEffect: true,
  },
  {
    /** 辨識完成並寫回 item。payload 記每一份的版型與 blocking 問題數。 */
    type: 'ingest.recognition_completed',
    targetType: 'energyIngestBatch',
    payloadSchema: z.object({
      jobId: z.string().min(1),
      documents: z.number().int().nonnegative(),
      failed: z.number().int().nonnegative(),
      byTemplate: z.record(z.string(), z.number()).optional(),
      blockingIssues: z.number().int().nonnegative().optional(),
      elapsedMs: z.number().int().nonnegative().optional(),
    }),
    summaryKey: 'audit.type.ingest.recognitionCompleted',
    severity: 'info',
    hasSideEffect: true,
  },
  {
    /**
     * **外部管線直接灌一份辨識結果**（`RECOGNITION.md §6.5`）。
     *
     * 與 `recognition_completed` 分開記，因為它們的**信任來源不同**：
     * 一個是我們自己的服務跑出來的，一個是呼叫端交給我們的。
     * 混在一起的話，日後要回答「這張單的辨識值是誰給的」就沒有依據。
     */
    type: 'ingest.recognition_supplied',
    targetType: 'energyIngestItem',
    payloadSchema: z.object({
      itemId: z.number().int().positive(),
      batchId: z.number().int().positive(),
      docType: z.string().min(1),
      billId: z.number().int().positive().nullable().optional(),
      status: z.string().min(1),
    }),
    summaryKey: 'audit.type.ingest.recognitionSupplied',
    severity: 'info',
    hasSideEffect: true,
  },
  {
    /**
     * 辨識沒能完成（服務連不上、逾時、整批失敗）。
     * `notable` 是刻意的：檔案還在，但**沒有人會知道它卡住了**，除非這一筆被看見。
     */
    type: 'ingest.recognition_failed',
    targetType: 'energyIngestBatch',
    payloadSchema: z.object({
      jobId: z.string().optional(),
      code: z.string().min(1),
      message: z.string().optional(),
      itemCount: z.number().int().nonnegative().optional(),
    }),
    summaryKey: 'audit.type.ingest.recognitionFailed',
    severity: 'notable',
    hasSideEffect: true,
  },
  {
    /**
     * 辨識服務那一側的**第二份原始檔**已刪除。
     *
     * 服務會把每一份上傳檔另存在自己的 volume；我們在 MinIO 已經有一份有稽核的。
     * 不刪＝地主統編／地址／電號在兩個地方各留一份，而只有其中一份查得到誰看過。
     * 「刪掉了沒」因此必須是**查得到的事實**，不是一句「我們會刪」。
     */
    type: 'ingest.recognition_purged',
    targetType: 'energyIngestBatch',
    payloadSchema: z.object({
      jobId: z.string().min(1),
      ok: z.boolean(),
    }),
    summaryKey: 'audit.type.ingest.recognitionPurged',
    severity: 'info',
    hasSideEffect: false,
  },

  // ── 案場座標（Slice C5）──
  {
    /**
     * 案場座標被更新。
     *
     * **改座標是有後果的**：電廠會在地圖上換位置、之後任何「這附近有哪些案場」
     * 的判斷都跟著變。所以它要留痕，且要記**從哪來**（`source`／`precision`）——
     * 日後換 geocoder provider 時才知道哪些座標是舊的爛來源產的、該重跑。
     */
    type: 'station.geocoded',
    targetType: 'energyStation',
    payloadSchema: z.object({
      changes: changesSchema,
      address: z.string().optional(),
      // precision 是一級欄位（不是 score）—— 見 `geocoder.ts` 的說明。
      precision: z.string().optional(),
      source: z.string().optional(),
      matchedAddress: z.string().optional(),
    }),
    summaryKey: 'audit.type.station.geocoded',
    severity: 'info',
    hasSideEffect: true,
  },

  // ── 計算策略與應付（Slice B3）──
  {
    type: 'payout.calculated',
    targetType: 'energyPayoutRecord',
    payloadSchema: z.object({
      billId: z.number().int(),
      /**
       * **參數有兩種來源**（2026-09-09）：案場綁的策略版本，或**契約**
       * （`energyContractStation.params`）。契約那一條沒有策略版本可引，
       * 所以這兩欄可空 —— 而 `paramsSource` 說得出到底是哪一種。
       * 少了 `paramsSource`，「這筆錢照誰的參數算的」就只能靠猜。
       */
      strategyId: z.number().int().nullable(),
      version: z.number().int().nullable(),
      contractId: z.number().int().nullable().optional(),
      paramsSource: z.enum(['contract', 'stationStrategy']).optional(),
      strategyType: z.string(),
      // 金額用**字串** —— JSON number 會把 decimal 變成浮點。
      amount: z.string(),
      trigger: z.enum(['workflowNode', 'manualRecompute', 'correction']),
      supersedesRecordId: z.number().int().nullable().optional(),
    }),
    summaryKey: 'audit.type.payout.calculated',
    severity: 'info',
    hasSideEffect: true,
  },
  {
    // **算不出來也要留痕** —— 否則「為什麼這張單沒有應付金額」永遠查不到。
    type: 'payout.calculation_failed',
    targetType: 'energyBill',
    payloadSchema: z.object({
      reason: z.string(),
      details: z.unknown().optional(),
      trigger: z.string(),
    }),
    summaryKey: 'audit.type.payout.calculationFailed',
    severity: 'notable',
    hasSideEffect: false,
  },
  {
    type: 'payout.superseded',
    targetType: 'energyPayoutRecord',
    payloadSchema: z.object({ replacedByRecordId: z.number().int() }),
    summaryKey: 'audit.type.payout.superseded',
    severity: 'notable',
    hasSideEffect: true,
  },
  {
    // 新版本上線＝影響往後所有計算。
    type: 'strategy.published',
    targetType: 'energyRatingStrategy',
    payloadSchema: z.object({
      key: z.string(), version: z.number().int(), strategyType: z.string(),
    }),
    summaryKey: 'audit.type.strategy.published',
    severity: 'notable',
    hasSideEffect: true,
  },
  // ── 策略綁定的三個動作（`RATING.md §2.4`／`WORKFLOW.md §3.2.9`）──
  //
  // 原本只有一個 `strategy.binding_changed`(notable) —— **拆成三個**：
  // 「解除」與「換版」的嚴重度不同，混在一起就分不出來。
  {
    type: 'strategy.binding_created',
    targetType: 'energyRatingStrategyBinding',
    payloadSchema: bindingSchema,
    summaryKey: 'audit.type.strategy.bindingCreated',
    severity: 'info',
    hasSideEffect: true,
  },
  {
    type: 'strategy.binding_switched',
    targetType: 'energyRatingStrategyBinding',
    payloadSchema: bindingSchema,
    summaryKey: 'audit.type.strategy.bindingSwitched',
    severity: 'notable',
    hasSideEffect: true,
  },
  {
    // critical —— 解除之後這個案場**算不出錢**。
    type: 'strategy.binding_ended',
    targetType: 'energyRatingStrategyBinding',
    payloadSchema: bindingSchema,
    summaryKey: 'audit.type.strategy.bindingEnded',
    severity: 'critical',
    hasSideEffect: true,
  },
];

const BY_TYPE: Record<string, AuditEventDef> = ENERGY_AUDIT_EVENTS.reduce(
  (acc, d) => { acc[d.type] = d; return acc; },
  {} as Record<string, AuditEventDef>,
);

/** 取事件定義；找不到回 undefined（`AuditLogger` 據此拒絕未註冊的 type）。 */
export const getAuditEventDef = (type: string): AuditEventDef | undefined => BY_TYPE[type];

export const isKnownAuditEventType = (type: string): boolean => !!BY_TYPE[type];

/** 全部已註冊的 type（測試與前端篩選器用）。 */
export const auditEventTypes = (): string[] => ENERGY_AUDIT_EVENTS.map((d) => d.type);
