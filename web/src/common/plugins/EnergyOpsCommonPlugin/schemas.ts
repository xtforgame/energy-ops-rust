/**
 * energy-ops-console BFF 的請求 body schema（前後端共用）。
 *
 * 後端：`this.basicUtils.validateBody(schema)` 當 middleware（parse 失敗自動 400）。
 * 前端：送出前先 `schema.parse(formData)`，錯誤在畫面上就擋掉。
 *
 * DB 層沒有 NOT NULL（AMM 產出的 column 皆 nullable），
 * `key` / `name` 的必填是**在這裡**強制的。
 */

import { z } from 'zod';
import { BILL_DOC_TYPES } from './documentIngest';

/** 部門在組織內的穩定識別碼；規則 params 用它指涉部門（不用 id）。 */
export const departmentKeySchema = z.string()
  .min(1)
  .max(80)
  .regex(/^[a-z0-9][a-z0-9-]*$/, 'key 只能用小寫英數與連字號');

export const departmentMemberRoleSchema = z.enum(['manager', 'member']);
export type DepartmentMemberRole = z.infer<typeof departmentMemberRoleSchema>;

export const createDepartmentSchema = z.object({
  key: departmentKeySchema,
  name: z.string().min(1).max(200),
  sortOrder: z.number().int().optional(),
});
export type CreateDepartmentInput = z.infer<typeof createDepartmentSchema>;

export const updateDepartmentSchema = z.object({
  name: z.string().min(1).max(200).optional(),
  sortOrder: z.number().int().optional(),
}).refine(
  (v) => v.name !== undefined || v.sortOrder !== undefined,
  { message: '至少要給一個要更新的欄位' },
);
export type UpdateDepartmentInput = z.infer<typeof updateDepartmentSchema>;

export const setDepartmentMemberSchema = z.object({
  userId: z.union([z.number().int().positive(), z.string().min(1)]),
  role: departmentMemberRoleSchema.default('member'),
});
export type SetDepartmentMemberInput = z.infer<typeof setDepartmentMemberSchema>;

/**
 * 角色指派／撤銷。用 `roleUniqueName` 而不是 `roleId`
 * —— id 隨 env / re-init 變，uniqueName 才是穩定真值。
 */
export const roleAssignmentSchema = z.object({
  userId: z.union([z.number().int().positive(), z.string().min(1)]),
  roleUniqueName: z.string().min(1),
  reason: z.string().max(500).optional(),
});
export type RoleAssignmentInput = z.infer<typeof roleAssignmentSchema>;

// ---------------------------------------------------------------------------
// 流程（Slice 2b）
// ---------------------------------------------------------------------------

/** `YYYY-MM-DD`。業務效期是「日」的概念，帶時分秒只會製造時區麻煩。 */
export const businessDateSchema = z.string().regex(/^\d{4}-\d{2}-\d{2}$/, '日期格式須為 YYYY-MM-DD');

export const startExecutionSchema = z.object({
  stationKey: z.string().min(1).max(120),
  workflowKey: z.string().min(1).max(120),
  subjectType: z.string().min(1).max(60),
  subjectId: z.union([z.string().min(1).max(120), z.number()]),
  /**
   * **業務日期，不是今天**：補登五月的單要用五月生效的那份約
   * （`WORKFLOW.md §3.3`）。做成必填就是為了逼呼叫端想清楚這件事
   * ——給預設值等於讓「用 now() 取版」變成預設行為。
   */
  businessDate: businessDateSchema,
  /** 分支條件求值用的單據屬性快照。 */
  attributes: z.record(z.string(), z.unknown()).optional(),
});
export type StartExecutionInput = z.infer<typeof startExecutionSchema>;

/** `WORKFLOW.md §2.6.1` 的五種動作 kind（呈現用；前端**不需要**知道差別）。 */
export const actionKindSchema = z.enum(['advance', 'revert', 'setState', 'sideEffect', 'abort']);

/**
 * 單一入口的 body。
 * 送的是 **`actionKey`**（定義側 action 表的一列），不是 kind ——
 * **動作是定義的一部分，不是 UI 自己長出來的**（`WORKFLOW.md §2.6`）。
 */
export const performActionSchema = z.object({
  actionKey: z.string().min(1).max(120),
  reason: z.string().max(2000).optional(),
  payload: z.record(z.string(), z.unknown()).optional(),
  /** `kind='sideEffect'` 的備註（匯款單號等）。人輸入的，系統無從驗證。 */
  note: z.string().max(500).optional(),
  /** `targetMode='explicitNode'` 時的目標（需目標節點 `allowJumpTarget=true`）。 */
  targetNodeKey: z.string().max(120).optional(),
});
export type PerformActionInput = z.infer<typeof performActionSchema>;

export const releaseSideEffectSchema = z.object({
  /** 必填 —— 這是全系統最需要被看見的一筆稽核。 */
  reason: z.string().min(1).max(2000),
});

export const decideAbortSchema = z.object({
  approved: z.boolean(),
  reason: z.string().max(2000).optional(),
});

export const createDelegationSchema = z.object({
  delegatorUserId: z.number().int().positive(),
  delegateUserId: z.number().int().positive(),
  validFrom: businessDateSchema,
  validTo: businessDateSchema.nullable().optional(),
  reason: z.string().max(500).optional(),
});

export const linkLineageSchema = z.object({
  upstreamExecutionId: z.number().int().positive(),
  downstreamExecutionId: z.number().int().positive(),
  /** 只是標籤，不觸發任何引擎行為（`WORKFLOW.md §4.9.4`）。 */
  relationKind: z.enum(['derived', 'reversal', 'correction', 'merged']).optional(),
  note: z.string().max(500).optional(),
});

// ---------------------------------------------------------------------------
// 電費單（Slice B2）
// ---------------------------------------------------------------------------

/** 吃的是「OCR 整理後的 JSON」——不 pin shape，任何物件都收。 */
export const ingestBillSchema = z.object({
  ocrJson: z.unknown(),
  sourceAttachmentId: z.number().int().positive().nullable().optional(),
});

/** 人工指認（US-5）：把例外件配到一個發電站。 */
export const matchStationSchema = z.object({
  stationId: z.number().int().positive(),
});

/**
 * 校對：**只改 ③ 最終值**。
 * 這裡刻意**沒有** `ocrRaw` / `ocrExtracted` —— 辨識值不可被前端覆蓋，
 * 否則「OCR 準不準」永遠答不出來（`DESIGN.md §1.4`）。
 */
export const correctBillSchema = z.object({
  /**
   * 電號。改它會**重新配案場**（`BillIngestService.correct`）——
   * 它不只是一個欄位，它是這張單走哪一條流程的依據。
   */
  customerNumberRaw: z.string().max(120).nullable().optional(),
  billingPeriodStart: businessDateSchema.nullable().optional(),
  billingPeriodEnd: businessDateSchema.nullable().optional(),
  totalAmount: z.number().finite().nullable().optional(),
  generationKwh: z.number().finite().nullable().optional(),
  issuedAt: businessDateSchema.nullable().optional(),
  // 繳費期限：高壓用電單（我們付台電）才有，會計依它排付款。
  dueDate: businessDateSchema.nullable().optional(),
  /**
   * **單別**。它跟電號一樣不只是一個欄位 —— 它決定這張單要開哪些流程。
   *
   * 值域由 `BILL_DOC_TYPES` ＋ 兩個舊的粗分值決定，
   * **不在這裡寫死字串陣列**：兩份名單遲早會不一樣，
   * 而不一致的那一天，畫面上選得到的值會被後端擋成 400。
   */
  docType: z.enum([...BILL_DOC_TYPES, 'electricityBill', 'consumptionBill'] as unknown as [string, ...string[]])
    .optional(),
  markReviewed: z.boolean().optional(),
});
