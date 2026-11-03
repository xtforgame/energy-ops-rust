/**
 * **一筆流程單的「單號」—— 對外指認用的代號**（`FRONTEND.md §2 P12` 欄位表）。
 *
 * ### 它要解決什麼
 *
 * P12 是流程單清單。沒有單號的話，要在電話上／email 裡／跨部門講「哪一張」，
 * 只能用描述：「屏東枋寮一號那張十三萬八的」——
 * 同一個案場**每個月都有單**，那種講法會撞、會誤指。
 *
 * ### 它**不是**路由的一部分
 *
 * 所有連結走的都是 **DB id**（`/energyExecutionConsole/4/show`）。
 * 單號是**純顯示**：加它不影響任何導覽，
 * 但反過來也代表**單號目前貼不進網址** —— 要支援「同事給我單號我貼上去就開得起來」
 * 得另外做（詳情頁用單號查，或清單支援單號搜尋）。
 *
 * ### 為什麼預設是 DB id 而不是自己編一個
 *
 * 自己編（`TP-2026-03-A` 那種）就得發明一整套規則：前綴代表什麼、
 * 唯一範圍是全系統還是每組織、重跑的單沿用還是給新的…… 那些**都是業務決策**，
 * 不該由實作順手決定 —— 而編號一旦印在畫面上就會被拿去對帳。
 *
 * 所以這裡的策略是「**有真的就用真的，沒有就用 id**」：
 *
 * | 情況 | 顯示 |
 * |---|---|
 * | `documentNo` 有值 | 用它（**外部世界真實存在的編號**，例如台電單據號） |
 * | 沒有 | `#<執行 id>` |
 *
 * **`documentNo` 是正規欄位**（`energyWorkflowExecution.documentNo`）：
 * 組織內 unique（partial index，`deleted_at IS NULL`）、可搜尋、可貼網址。
 * nullable 是刻意的 —— 抽不到單據號的單不可以因此送不出去。
 */

/** 帶得出單號的東西。 */
export interface DisplayIdSource {
  id?: number | string | null;
  /** **正規欄位**（`energyWorkflowExecution.documentNo`）—— 組織內唯一、可搜尋。 */
  documentNo?: string | null;
}

/**
 * 顯示用的單號。
 *
 * @example
 * getDisplayId({ id: 4 })                                  // '#4'
 * getDisplayId({ id: 4, documentNo: 'TP-2026-03-A' })       // 'TP-2026-03-A'
 */
export const getDisplayId = (source?: DisplayIdSource | null): string => {
  if (!source) return '';
  // 只接受**非空字串** —— 空字串／null 都要落回 id，
  // 否則畫面上會出現一個空白的單號欄，比顯示 `#4` 更難查。
  const no = source.documentNo;
  if (typeof no === 'string' && no.trim()) return no.trim();
  return source.id != null ? `#${source.id}` : '';
};

/**
 * 判斷一個網址片段是「DB id」還是「單號」。
 *
 * 這讓 **「同事給我單號，我貼進網址就開得起來」** 成立：
 * `/energyExecutionConsole/4/show` 與
 * `/energyExecutionConsole/TP-2026-03-A/show` 都打得開。
 * 判準是「**純數字才當 id**」——單號不可能是純數字（它是外部編號，
 * 台電那種都帶分隔符）；萬一真的來一個純數字單號，會被當成 id，
 * 那時查不到就回 404，不會拿錯一筆給人看。
 */
export const isDbId = (raw: string | number | null | undefined): boolean => (
  /^\d+$/.test(String(raw ?? '').trim())
);

/**
 * 正規化要寫進 `documentNo` 欄位的值。
 *
 * 抽不到就回 **`null`**（不是空字串）——
 * Postgres 的 unique index 對 NULL 不衝突，但**空字串會衝突**：
 * 第二張沒有單號的單就會撞 unique 而送不出去。
 */
export const normalizeDocumentNo = (raw?: string | null): string | null => (
  typeof raw === 'string' && raw.trim() ? raw.trim() : null
);
