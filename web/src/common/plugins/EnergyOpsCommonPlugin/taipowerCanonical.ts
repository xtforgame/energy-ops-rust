/**
 * 台電辨識服務（`taipower-recognition/`）的 **canonical 結果型別**。
 *
 * 這一份是 `taipower-recognition/api/canonical-schema.json` 的 TypeScript 對照，
 * 放 common 是因為**前後端都要讀它**：後端把它轉成 `energyBill`，
 * 前端要顯示逐欄的來源、狀態與座標（`RECOGNITION.md §7`）。
 *
 * **不要在這裡把 119 個欄位一個個列出來。**
 * 欄位清單是**服務那一側的契約**，它會長；我們只 pin
 * 「一個欄位長什麼樣」與「我們真的會讀的那幾個 key」。
 * 這與 `billExtractor.ts` 的 `ocrRaw` 不 pin shape 是同一條原則。
 */

/** 一個欄位的狀態。`missing` 與 `not_applicable` 是**不同的事**，不可都轉成空字串。 */
export type CanonicalFieldStatus =
  | 'extracted'
  | 'ocr_extracted'
  | 'edited'
  /** parser 看到欄位、且確認原單就是空的。 */
  | 'source_blank'
  /** 沒取得或無法確認。 */
  | 'missing'
  /** 有候選值但不可靠。 */
  | 'ambiguous'
  /** 這個版型沒有這一欄。 */
  | 'not_applicable';

/** 值是怎麼被決定的（三來源交叉驗證的結果，`RECOGNITION.md §9-4`）。 */
export type CanonicalResolution =
  /** 文字層讀得到且唯一 —— 最可信。 */
  | 'pdf_text'
  /** 多個來源給出同一個值。 */
  | 'cross_verified'
  /** 只有一個來源看到它。 */
  | 'single_engine'
  /** 來源不一致，靠金額勾稽裁決。 */
  | 'rule_resolved'
  /** 來源不一致且無法裁決 —— 值會是 null，要人看。 */
  | 'conflict'
  | 'expected_missing'
  | 'source_blank';

export type CanonicalSource = 'pdf_text' | 'paddleocr' | 'tesseract';

/** 欄位在原檔上的位置（0–1 相對座標）。**沒有＝不確定**，不是 0,0。 */
export interface CanonicalBox {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface CanonicalEvidence {
  pageNumber: number;
  region: string;
  rawText: string;
  source?: CanonicalSource;
  confidence?: number | null;
  resolution?: CanonicalResolution;
  box?: CanonicalBox;
  candidates?: unknown[];
}

export interface CanonicalField {
  value: string | number | null;
  rawValue: string;
  status: CanonicalFieldStatus;
  evidence: CanonicalEvidence | null;
}

export interface CanonicalIssue {
  code: string;
  message: string;
  fields: string[];
  /** true ＝ 核心欄位有問題，**不該就這樣送審**。 */
  blocking: boolean;
}

/** 服務判定的版型。`null` ＝ 認不得（走 generic parser，欄位會很少）。 */
export type CanonicalTemplateId =
  | 'legacy_renewable_purchase'
  | 'renewable_purchase'
  | 'high_voltage'
  | 'unknown'
  | null;

export interface CanonicalRecord {
  fields: Record<string, CanonicalField>;
  extractionMethod: 'pdf_text' | 'ocr';
  /**
   * **不要拿它當「要不要人工校對」的訊號**。
   * 交付版的自動化路徑從來沒有重算過它（永遠是 `needs_review`）；
   * 我們雖然已經修好（`processBill` 補上 `classifyReview`），
   * 但真正的判準是 `issues` 裡有沒有 `blocking` —— 那才是逐條說得出理由的東西。
   */
  reviewStatus: string;
  templateId: CanonicalTemplateId;
}

export interface CanonicalDocumentSuccess {
  sourceFileId: string;
  sourceFileName: string;
  status: 'succeeded';
  record: CanonicalRecord;
  issues: CanonicalIssue[];
}

export interface CanonicalDocumentFailure {
  sourceFileId: string;
  sourceFileName: string;
  status: 'failed';
  error: { code: string; message: string };
}

export type CanonicalDocument = CanonicalDocumentSuccess | CanonicalDocumentFailure;

export interface CanonicalJobResult {
  schemaVersion: string;
  jobId: string;
  status: 'completed' | 'completed_with_issues' | 'failed';
  createdAt: string;
  completedAt: string;
  documents: CanonicalDocument[];
}

export type TprJobStatus = 'queued' | 'processing' | 'completed' | 'completed_with_issues' | 'failed';

export interface TprJobSummary {
  jobId: string;
  status: TprJobStatus;
  createdAt: string;
  updatedAt: string;
  completedAt?: string;
  files: Array<{
    sourceFileId: string;
    sourceFileName: string;
    status: 'queued' | 'processing' | 'succeeded' | 'failed';
    error?: { code: string; message: string };
  }>;
}

export const TPR_TERMINAL_STATUSES: readonly TprJobStatus[] = [
  'completed', 'completed_with_issues', 'failed',
];

export const isTprTerminal = (s: string): boolean => TPR_TERMINAL_STATUSES.includes(s as TprJobStatus);

/** 檔案需要密碼 —— 與「檔案損壞」是兩件事，畫面上要給不同的出口。 */
export const TPR_PASSWORD_REQUIRED = 'password_required';

// ---------------------------------------------------------------------------
// 讀 canonical 的小工具（前後端共用，不要各寫一份）
// ---------------------------------------------------------------------------

export const fieldOf = (record: CanonicalRecord, key: string): CanonicalField | null => record.fields[key] ?? null;

/** 取字串值。`not_applicable` / `missing` / 空字串一律回 undefined。 */
export function textOf(record: CanonicalRecord, key: string): string | undefined {
  const f = fieldOf(record, key);
  if (!f || f.value === null || f.value === undefined) return undefined;
  const s = String(f.value).trim();
  return s.length ? s : undefined;
}

/** 取數值。台電的單上金額**可以是負的**（無餘電時應付總金額 -132）。 */
export function numberOf(record: CanonicalRecord, key: string): number | undefined {
  const f = fieldOf(record, key);
  if (!f || f.value === null || f.value === undefined) return undefined;
  if (typeof f.value === 'number') return Number.isFinite(f.value) ? f.value : undefined;
  const n = Number(String(f.value).replace(/[,\s]/g, ''));
  return Number.isFinite(n) ? n : undefined;
}

/** `YYYY-MM-DD`；服務已經把民國年換算好了，這裡只做形狀確認。 */
export function dateOf(record: CanonicalRecord, key: string): string | undefined {
  const s = textOf(record, key);
  return s && /^\d{4}-\d{2}-\d{2}$/.test(s) ? s : undefined;
}

export const blockingIssues = (issues: readonly CanonicalIssue[]): CanonicalIssue[] => issues.filter((i) => i.blocking);
