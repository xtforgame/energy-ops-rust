/**
 * **每個欄位一個顏色** —— 左邊紙上的框與右邊表格的那一列，用同一個顏色連起來。
 *
 * ### 為什麼需要它
 *
 * 校對的動作是「拿螢幕上的數字對紙上的數字」。在此之前，紙上的框全部同一個顏色，
 * 所以看到一個框只知道「這裡有東西被認出來」，**不知道它是哪一欄** ——
 * 要嘛一個一個點過去，要嘛乾脆不看。顏色一對上，那個對照就變成一眼的事。
 *
 * ### 為什麼是固定十色而不是隨機
 *
 * 顏色必須**跨頁面、跨重新整理都一樣**：同一張單今天看是藍色的計費期間，
 * 明天還要是藍色的，不然它就不是一個可以記住的線索。
 * 所以是「欄位 → 顏色」的固定對應，不是渲染順序決定。
 *
 * ### 選色的兩個限制
 *
 * 1. **要在白紙上看得出來** —— 這些框疊在掃描件上，太淡等於沒有。
 * 2. **不要撞到狀態色的語意**：燒橘（`jvTokens.warn`）＝要人處理、
 *    綠（`ok`）＝完成。這裡的顏色是**身分**不是狀態，所以刻意避開那兩個色相，
 *    也不用純紅（那在這個系統裡從來不代表「這一欄」）。
 */

/** 十個色相分得開、明度接近的顏色 —— 沒有哪一個看起來「比較重要」。 */
export const FIELD_PALETTE = [
  '#2563EB', // 藍
  '#059669', // 翠綠
  '#7C3AED', // 紫
  '#DB2777', // 桃
  '#D97706', // 琥珀
  '#0891B2', // 青
  '#65A30D', // 萊姆
  '#4F46E5', // 靛
  '#92400E', // 褐
  '#475569', // 石板灰
] as const;

/**
 * 欄位的固定順序 —— 顏色由這裡的**位置**決定。
 *
 * 在中間插一個新欄位會讓後面所有欄位換色（使用者記的顏色會突然不一樣），
 * 所以**新欄位一律加在最後面**。
 */
const FIELD_ORDER = [
  'customerNumberRaw',
  'billingPeriodStart',
  'billingPeriodEnd',
  'totalAmount',
  'generationKwh',
  'issuedAt',
  'dueDate',
  'documentNo',
  'contractNumberRaw',
  'companyTaxId',
] as const;

/** 不在名單上的欄位用字串雜湊挑一個 —— 一樣是固定的，不會每次 render 都變。 */
function hashIndex(field: string): number {
  let h = 0;
  for (let i = 0; i < field.length; i += 1) h = (h * 31 + field.charCodeAt(i)) >>> 0;
  return h % FIELD_PALETTE.length;
}

/** 這個欄位的顏色。同一個欄位在任何地方、任何時候都是同一個色。 */
export function colorForField(field: string): string {
  const i = FIELD_ORDER.indexOf(field as typeof FIELD_ORDER[number]);
  return FIELD_PALETTE[i >= 0 ? i % FIELD_PALETTE.length : hashIndex(field)];
}

/**
 * 同一個顏色的**淡底**（疊在紙上當填色用）。
 * 用 8 位 hex 的 alpha，而不是另外準備十個淡色 —— 那會變成第二份要維護的對應表。
 */
export function fieldFill(field: string, alpha = '22'): string {
  return `${colorForField(field)}${alpha}`;
}
