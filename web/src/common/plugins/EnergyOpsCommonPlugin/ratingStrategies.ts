/**
 * 計算策略 registry —— **策略即資料**（`RATING.md §2`）。
 *
 * 形狀與 `auditEvents.ts` **同構**（type 白名單 ＋ per-type Zod）——
 * `RESEARCH-WORKFLOW.md §8` 明說這是同一招，不要發明第二種寫法。
 *
 * 三個約束：
 * 1. **type 白名單**：`evaluateStrategy` 收到未註冊 type → 拒絕，不猜。
 * 2. **per-type Zod**：`params` 寫入前驗證，未宣告的鍵丟棄。
 * 3. **查詢維度是真欄位**（`type`/`status`/`key`/`version`），只有「點開才看的參數」在 `params`。
 *
 * **算式細節是 `REQUIREMENTS.md §九-2` 唯一仍待客戶的一項**。
 * 下面三條是依拍板的字面意思寫的；客戶的實際算法若有調整（例如比例要乘某個係數），
 * **改的是本檔的 `evaluate` 與 `paramsSchema`，不是表結構**。
 */

import { z } from 'zod';

// ---------------------------------------------------------------------------
// 十進位運算 —— **錢不能有浮點誤差**
// ---------------------------------------------------------------------------
//
// `0.1 + 0.2 !== 0.3`；`98000 * 12.5 / 100` 在 JS 裡是 `12250.000000000002`。
// 一筆一筆看不出來，對帳時才會發現差幾分錢，而且**找不到是哪一筆**。
// 所以全部走 BigInt 定點運算，不用 `Number` 做金額乘除。

/**
 * `10n ** n` 的替代品。
 *
 * **不可以用 `**` 運算子** —— 本 repo 的 Babel 設定會把它轉成 `Math.pow`，
 * 而 `Math.pow` 吃到 BigInt 直接 `TypeError: Cannot convert a BigInt value to a number`。
 * 這種錯只有在**真的跑到**那行才會爆（型別檢查看不出來），所以寫成函式並在此註明。
 */
function pow10(n: number): bigint {
  let out = 1n;
  for (let i = 0; i < n; i += 1) out *= 10n;
  return out;
}

/** 把十進位字串／數字轉成「放大 `scale` 倍的整數」。`12.5` @4 → `125000n`。 */
export function toScaled(value: string | number, scale: number): bigint {
  const s = typeof value === 'number' ? value.toFixed(Math.max(scale, 0)) : String(value).trim();
  if (!/^-?\d+(\.\d+)?$/.test(s)) throw new Error(`not a decimal: ${s}`);
  const neg = s.startsWith('-');
  const [intPart, fracRaw = ''] = (neg ? s.slice(1) : s).split('.');
  const frac = (fracRaw + '0'.repeat(scale)).slice(0, scale);
  const n = BigInt(intPart + (scale > 0 ? frac : ''));
  return neg ? -n : n;
}

/** 放大整數 → 十進位字串。 */
export function fromScaled(value: bigint, scale: number): string {
  const neg = value < 0n;
  const abs = neg ? -value : value;
  const s = abs.toString().padStart(scale + 1, '0');
  const out = scale === 0 ? s : `${s.slice(0, -scale)}.${s.slice(-scale)}`;
  return neg ? `-${out}` : out;
}

/**
 * 從 `scale` 位縮到 2 位（金額），**四捨五入（half-up）**。
 *
 * 「四捨五入」是一個**業務決定**，不是技術細節 —— 銀行家捨入（half-even）與
 * 無條件捨去在對帳時會差錢。⏳ `§九-2` 的算式細節回來時要一併確認；
 * 要改就改這一個函式。
 */
export function roundToMoney(scaled: bigint, scale: number): string {
  if (scale <= 2) return fromScaled(scaled * pow10(2 - scale), 2);
  const divisor = pow10(scale - 2);
  const neg = scaled < 0n;
  const abs = neg ? -scaled : scaled;
  const q = abs / divisor;
  const r = abs % divisor;
  // half-up：餘數 ≥ 半個單位就進位。
  const rounded = r * 2n >= divisor ? q + 1n : q;
  return fromScaled(neg ? -rounded : rounded, 2);
}

// ---------------------------------------------------------------------------
// registry
// ---------------------------------------------------------------------------

/** 算式吃的輸入 —— 就是電費單的**最終值**（校對後的那一層）。 */
export interface RatingInput {
  totalAmount?: string | number | null;
  generationKwh?: string | number | null;
  billingPeriodStart?: string | null;
  billingPeriodEnd?: string | null;
}

export interface RatingStrategyDef {
  type: string;
  /** i18n key。**不印 type 字串給使用者看**。 */
  labelKey: string;
  paramsSchema: z.ZodTypeAny;
  /**
   * 這個 type 需要電費單提供哪些欄位。
   * 缺欄位時要**早期報錯**，不可算成 0（那會變成一筆看起來合法的錯帳）。
   */
  requiredBillFields: Array<keyof RatingInput>;
  /** 純函式：不碰 DB，只吃參數與輸入。回傳 2 位小數的金額字串。 */
  evaluate: (params: any, input: RatingInput) => string;
}

const money = (v: unknown) => toScaled(v as any, 2);

export const RATING_STRATEGY_DEFS: RatingStrategyDef[] = [
  {
    // 每期固定給付金額。「每期」＝這張電費單的計費期間；
    // v1 不做「雙月單拆成兩個月」的換算（那是 proration，已拍板不做）。
    type: 'fixedFee',
    labelKey: 'energy.rating.type.fixedFee',
    paramsSchema: z.object({
      amount: z.union([z.number(), z.string()]).refine(
        (v) => { try { return toScaled(v as any, 2) > 0n; } catch { return false; } },
        '金額必須是正數',
      ),
    }),
    requiredBillFields: [],
    evaluate: (params) => fromScaled(money(params.amount), 2),
  },
  {
    // 電費單金額 × %
    type: 'revenueShare',
    labelKey: 'energy.rating.type.revenueShare',
    paramsSchema: z.object({
      sharePercent: z.union([z.number(), z.string()]).refine(
        (v) => {
          try {
            const n = toScaled(v as any, 4);
            return n >= 0n && n <= toScaled(100, 4);
          } catch { return false; }
        },
        '比例必須介於 0–100',
      ),
    }),
    requiredBillFields: ['totalAmount'],
    evaluate: (params, input) => {
      // amount(2) × percent(4) → 6 位；再除以 100 → 仍是 6 位的分子 / 100
      const amount = money(input.totalAmount!);
      const percent = toScaled(params.sharePercent as any, 4);
      // 先乘再除，不先除（先除會提早損失精度）。
      const scaled6 = (amount * percent) / 100n;
      return roundToMoney(scaled6, 6);
    },
  },
  {
    // 度數 × 元/度
    type: 'generationUnitPrice',
    labelKey: 'energy.rating.type.generationUnitPrice',
    paramsSchema: z.object({
      unitPrice: z.union([z.number(), z.string()]).refine(
        (v) => { try { return toScaled(v as any, 4) > 0n; } catch { return false; } },
        '單價必須是正數',
      ),
    }),
    requiredBillFields: ['generationKwh'],
    evaluate: (params, input) => {
      // kwh(3) × price(4) → 7 位
      const kwh = toScaled(input.generationKwh as any, 3);
      const price = toScaled(params.unitPrice as any, 4);
      return roundToMoney(kwh * price, 7);
    },
  },
];

const BY_TYPE: Record<string, RatingStrategyDef> = RATING_STRATEGY_DEFS.reduce(
  (acc, d) => { acc[d.type] = d; return acc; },
  {} as Record<string, RatingStrategyDef>,
);

export const getRatingStrategyDef = (type: string): RatingStrategyDef | undefined => BY_TYPE[type];
export const isKnownRatingType = (type: string): boolean => !!BY_TYPE[type];
export const ratingStrategyTypes = (): string[] => RATING_STRATEGY_DEFS.map((d) => d.type);

export class RatingEvaluationError extends Error {
  code: string;

  details?: unknown;

  constructor(code: string, details?: unknown) {
    super(code);
    this.name = 'RatingEvaluationError';
    this.code = code;
    this.details = details;
  }
}

/**
 * **純函式的計算入口**（`RATING.md §4`）。
 *
 * 金額計算是這個系統最需要被反覆測試的地方 —— 不該綁在 DB 上。
 *
 * **算不出來就 throw，絕不回 0** —— 一筆金額 0 的紀錄看起來完全合法，
 * 但它是錯帳，而且要很久以後才有人發現。
 */
export function evaluateStrategy(type: string, params: unknown, input: RatingInput): string {
  const def = getRatingStrategyDef(type);
  if (!def) throw new RatingEvaluationError('unknown_strategy_type', { type });

  const parsed = def.paramsSchema.safeParse(params ?? {});
  if (!parsed.success) {
    throw new RatingEvaluationError('invalid_strategy_params', (parsed as any).error?.issues);
  }

  const missing = def.requiredBillFields.filter((f) => {
    const v = input[f];
    return v === null || v === undefined || v === '';
  });
  if (missing.length) {
    throw new RatingEvaluationError('missing_bill_fields', { missing, type });
  }

  return def.evaluate(parsed.data, input);
}
