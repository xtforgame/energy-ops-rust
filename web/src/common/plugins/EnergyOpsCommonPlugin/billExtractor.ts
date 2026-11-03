/**
 * **OCR 邊界**（`TASKS.md §OCR 邊界`，2026-08-26 使用者裁定）。
 *
 * > 原話：「OCR 你先不用管，這現在訂不了，但你只要知道我們會**掃出一個整理好的
 * > json object**，你可以預期**某個 mock function 會吃這個 object 然後 return 你要的值**。」
 *
 * ```
 * OCR（外部，製作中）  ──▶  整理好的 JSON object（shape 未定，且會變）
 * │
 * billExtractor(json) → ExtractedBill[]   ← 我們只定這一層
 * │
 * energyBill { ocrRaw ＋ ocrExtracted ＋ 人工修正後最終值 }
 * ```
 *
 * **為什麼這樣切，OCR 的 JSON 長怎樣就不再是我們的風險：**
 *
 * 1. 原始 JSON 存成 `ocrRaw` jsonb，**不 pin shape** —— OCR 改版 → 內容變，**schema 不變**。
 * 2. 只 pin「我們真的要用到的抽出欄位」，而那組欄位**已經被 `RATING.md §2.3` 的三種策略
 * 反推定死了**（電號／計費期間／總金額／發電度數）。
 * 3. **回傳陣列而不是單筆** —— 這一步把「一張單含多個電號？」（`§九-3`）從
 * **schema 風險**降級成 **mock 的實作細節**：一個上傳檔產出 N 筆 `energyBill`，
 * 1 或 N 都**不改表結構**。
 *
 * 真 OCR 交付時**只換實作，不動介面、不動 schema、不動流程**。
 */

import { z } from 'zod';

/**
 * `billExtractor` 的輸出。**這是我們唯一 pin 的形狀。**
 *
 * 每個欄位都可能抽不到（OCR 會失敗、單子會糊）——所以除了 `customerNumber`
 * 以外全部 optional。不要為了「型別漂亮」把它們設成必填：
 * 抽不到就是抽不到，那正是人工校對存在的理由。
 */
export interface ExtractedBill {
  /** 電號 —— 全系統的 join key。抽不到就沒辦法配對，但**仍要建 bill**（例外件）。 */
  customerNumber?: string;
  /** `YYYY-MM-DD`。取版依據（`WORKFLOW.md §3.3`）。 */
  billingPeriodStart?: string;
  billingPeriodEnd?: string;
  /** `revenueShare` 策略要用。 */
  totalAmount?: number;
  /** `generationUnitPrice` 策略要用。 */
  generationKwh?: number;
  /** 台電開單日。 */
  issuedAt?: string;
  /**
   * **台電單據號** —— 外部世界本來就唯一的編號。
   *
   * 送審時會被帶進 `execution.data.displayId`，成為 P12 的「單號」欄
   * （`common/plugins/EnergyOpsCommonPlugin/displayId.ts`）。
   * 用它而不是自己編號：我們不必發明格式、也不必保證唯一性。
   * mock 抽取器**不產生**這個欄位 —— 真 OCR 接上前，P12 的單號一律顯示 `#id`。
   */
  documentNo?: string;
  /**
   * 抽取器對這一筆的信心（0–1）。**純提示，不參與任何判斷** ——
   * 不要拿它去自動決定「要不要人工校對」，那是人的決定。
   */
  confidence?: number;

  // ── 以下是真辨識服務接上之後才有的（`RECOGNITION.md §7.1`）────────────────
  //
  // 加欄位不改 schema：它們落在 `energyBill.ocrExtracted` 的 jsonb 裡，
  // 只有**查詢維度**才升級成真欄位（`templateId` / `dueDate`）。

  /**
   * 服務判定的版型（`renewable_purchase` / `high_voltage` / …）。
   * 校對台要靠它決定顯示哪一組欄位 —— 兩種單的核心欄位不一樣。
   */
  templateId?: string;
  /**
   * 繳費期限（高壓單有、躉購單沒有）。
   * **會計依它排序與逾期警示**，所以它是查詢維度不是顯示欄位。
   */
  dueDate?: string;
  /**
   * 單上的契約編號**原樣**。與 `energyContract.contractNo` 交叉檢查用；
   * 對不上不猜，只標示。
   */
  contractNumberRaw?: string;
}

const dateish = z.string().regex(/^\d{4}-\d{2}-\d{2}$/);

export const extractedBillSchema = z.object({
  customerNumber: z.string().min(1).max(60).optional(),
  billingPeriodStart: dateish.optional(),
  billingPeriodEnd: dateish.optional(),
  totalAmount: z.number().finite().optional(),
  generationKwh: z.number().finite().optional(),
  issuedAt: dateish.optional(),
  documentNo: z.string().min(1).max(120).optional(),
  confidence: z.number().min(0).max(1).optional(),
  templateId: z.string().min(1).max(60).optional(),
  dueDate: dateish.optional(),
  contractNumberRaw: z.string().min(1).max(120).optional(),
});

/** 抽取器的簽名。**真 OCR 交付時只換實作**。 */
export type BillExtractor = (ocrJson: unknown) => ExtractedBill[];

// ---------------------------------------------------------------------------
// v1 的 mock 實作
// ---------------------------------------------------------------------------

const num = (v: unknown): number | undefined => {
  if (typeof v === 'number' && Number.isFinite(v)) return v;
  if (typeof v === 'string') {
    // 台電的單子上金額有千分位；`1,234.5` → 1234.5
    const cleaned = v.replace(/[,\s]/g, '');
    if (!cleaned) return undefined;
    const n = Number(cleaned);
    return Number.isFinite(n) ? n : undefined;
  }
  return undefined;
};

const str = (v: unknown): string | undefined => {
  if (typeof v === 'string' && v.trim()) return v.trim();
  if (typeof v === 'number') return String(v);
  return undefined;
};

/** `2026/01/31`、`2026-1-3`、`20260131` → `2026-01-31`；認不得就回 undefined。 */
export function normalizeDate(v: unknown): string | undefined {
  const s = str(v);
  if (!s) return undefined;
  const compact = s.match(/^(\d{4})(\d{2})(\d{2})$/);
  if (compact) return `${compact[1]}-${compact[2]}-${compact[3]}`;
  const parts = s.match(/^(\d{4})[-/.](\d{1,2})[-/.](\d{1,2})$/);
  if (!parts) return undefined;
  const [, y, m, d] = parts;
  return `${y}-${m.padStart(2, '0')}-${d.padStart(2, '0')}`;
}

/**
 * v1 的抽取器。**吃「整理好的 JSON」，不吃圖片、不做辨識。**
 *
 * 接受兩種形狀（都只是 mock 的細節，不是契約）：
 * - `{ bills: [ {...}, {...} ] }` —— 一張掃描檔含多個電號
 * - `{ ... }` 單一物件 —— 視為只有一筆
 *
 * **抽不到就留空，不要猜**：`billExtractor` 的職責是「忠實轉換」，
 * 補值是人的工作（校對台）。猜錯的值比空值難發現得多。
 */
export const mockBillExtractor: BillExtractor = (ocrJson: unknown): ExtractedBill[] => {
  const root = (ocrJson ?? {}) as any;
  const items: any[] = Array.isArray(root?.bills) ? root.bills
    : Array.isArray(root) ? root
      : [root];

  return items
    .map((raw): ExtractedBill => {
      const out: ExtractedBill = {
        // 兼容幾個常見的欄位名 —— 這是 **mock 的細節**，真 OCR 來了就整段換掉。
        customerNumber: str(raw?.customerNumber ?? raw?.customer_number ?? raw?.電號),
        billingPeriodStart: normalizeDate(raw?.billingPeriodStart ?? raw?.periodStart ?? raw?.計費起),
        billingPeriodEnd: normalizeDate(raw?.billingPeriodEnd ?? raw?.periodEnd ?? raw?.計費訖),
        totalAmount: num(raw?.totalAmount ?? raw?.amount ?? raw?.總金額),
        generationKwh: num(raw?.generationKwh ?? raw?.kwh ?? raw?.度數),
        issuedAt: normalizeDate(raw?.issuedAt ?? raw?.issueDate ?? raw?.開單日),
        confidence: typeof raw?.confidence === 'number' ? raw.confidence : undefined,
      };
      // 丟掉 undefined，讓 `ocrExtracted` 的 jsonb 乾淨（不存一堆 null）。
      return Object.fromEntries(
        Object.entries(out).filter(([, v]) => v !== undefined),
      ) as ExtractedBill;
    })
    // 整筆都抽不到東西就不產生 bill —— 但只要**有任何一個欄位**抽到就產生，
    // 因為「抽到一半」正是校對台要處理的情況。
    .filter((b) => Object.keys(b).length > 0);
};

// ---------------------------------------------------------------------------
// 第四個「接縫」：欄位在原檔上的位置（bounding box）
// ---------------------------------------------------------------------------
//
// **這個接縫已經接上真的了**（2026-09-08）。
//
// 實作在 `taipowerExtractor.sourceFieldBoxes()`：座標由辨識服務的
// `evidence.box` 提供（`RECOGNITION.md §9-5`），而它**只在能唯一定位到
// 那個 token 時才給** —— 同一個數字在頁面上出現兩次就不給。
//
// 原本這裡有一個 `mockFieldBoxLocator`：一組寫死、看起來合理的假座標。
// 它在「還沒有真座標」的階段是對的（讓互動先做得起來），
// 但真座標接上之後留著它就變成一種謊 —— 假框讓人以為系統知道欄位在哪，
// 而那與 `FRONTEND.md §2.8` 說的「畫得出來但按不動的按鈕」是同一件事。
// 所以它被刪掉了，沒有座標就不畫框。
