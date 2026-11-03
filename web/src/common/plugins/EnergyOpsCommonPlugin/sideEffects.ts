/**
 * **副作用的顯示名。**
 *
 * 副作用的識別碼（`payment.sent`）是**給程式看的**：
 * 節點設定用它、回退鎖用它、稽核的 payload 也記它。
 * 但它**不可以直接印在畫面上** —— 稽核頁「對象」那一欄曾經整欄是
 * `payment.sent`，跟旁邊寫著「主管核准」的列並排，同一頁兩種語言。
 *
 * ### 為什麼放 common 而不是前端 i18n
 *
 * 稽核的 `targetLabel` 是**寫入時就固定下來的字串快照**（存進 DB），
 * 不是讀取時才翻譯的 key —— 所以解析的動作發生在**後端**，
 * 前端那份 `energy.sideEffect.*` 是給**還活著的**設定畫面用的，
 * 兩者服務的時間點不同，不是重複。
 *
 * 查不到就原樣回傳識別碼：**寧可漏一個字串，不要吞掉一筆稽核**。
 */
export const SIDE_EFFECT_LABELS: Record<string, string> = {
  'payment.sent': '付款已送出',
};

export const sideEffectLabel = (key?: string | null): string => {
  const k = String(key ?? '').trim();
  if (!k) return '';
  return SIDE_EFFECT_LABELS[k] ?? k;
};

/**
 * 流程單的「對象」是誰 —— 給稽核的 `targetLabel` 用。
 *
 * `subjectType` 是 AMM model 名（`energyBill`），一樣不該印在畫面上；
 * 而光印一個 `subjectId`（「20」）也讀不懂 ——
 * 稽核頁上一整欄的裸數字沒有辦法讓人指認任何東西。
 */
const SUBJECT_LABELS: Record<string, string> = {
  energyBill: '電費單',
  energyInstallment: '保證金期數',
};

export const subjectLabel = (subjectType?: string | null, subjectId?: number | string | null): string => {
  const t = String(subjectType ?? '').trim();
  const id = String(subjectId ?? '').trim();
  const name = SUBJECT_LABELS[t] ?? t;
  if (!name) return id ? `#${id}` : '';
  return id ? `${name} #${id}` : name;
};
