/**
 * **業務身分** —— 「這是不是同一張單」的唯一判準（`LIFECYCLE.md §2.4`／`§6.2`）。
 *
 * ### 它要解決的根因
 *
 * 系統裡本來**沒有「這張單」這個概念**，只有「這一列 `energyBill`」。
 * 兩個承辦各掃同一張紙 → **兩列不同的 `energyBill`** → subject 不同 →
 * 既有的 `(org, subject_type, subject_id)` partial unique **完全擋不住**（缺陷 C1）。
 *
 * `businessKey` 把「業務上的同一張單」變成一個可比對的字串。
 *
 * ### 正規化只做「去非英數 ＋ 轉大寫」
 *
 * **不要補零、不要猜格式、不要對齊長度。**
 * 那會把兩張**真的不同**的單併成一張 —— 而那個錯誤是**靜默的**：
 * 沒有錯誤訊息、沒有紅字，只是第二張單再也送不出去，
 * 而查的人會以為是「重複送審擋住了」，不會想到是 key 算錯了。
 *
 * ### 抽不到就回 `null`，絕不用空字串
 *
 * 空字串會互相碰撞 —— 兩張都抽不到電號的單會被判成「同一張」。
 * 而 partial unique 的 `business_key IS NOT NULL` 條件正是靠 `null` 才成立。
 */

/**
 * 電號正規化。
 *
 * @example
 * normalizeCustomerNumber('01-2233-4455')  // '0122334455'
 * normalizeCustomerNumber('0122334455')    // '0122334455'  ← 同一個
 * normalizeCustomerNumber('  ')            // null
 */
export const normalizeCustomerNumber = (raw?: string | null): string | null => {
  if (!raw) return null;
  // 保留英數（有些電號含英文字母），只去掉分隔符號與空白。
  const cleaned = String(raw).replace(/[^0-9A-Za-z]/g, '').toUpperCase();
  return cleaned.length ? cleaned : null;
};

/** `YYYY-MM-DD`（dateonly 欄位讀出來可能帶時間，一律切前 10 碼）。 */
const day = (v?: string | null): string | null => {
  const s = String(v ?? '').slice(0, 10);
  return /^\d{4}-\d{2}-\d{2}$/.test(s) ? s : null;
};

/**
 * 電費單的業務身分 ＝ `電號 | 計費期間起 | 計費期間迄`。
 *
 * 三者缺一就回 `null`（放行送審，但單上要標「無法判斷是否重複」）——
 * 不可以用部分欄位湊一個 key，那會把不同期間的單併在一起。
 */
export const billBusinessKey = (b: {
  customerNumberRaw?: string | null;
  billingPeriodStart?: string | null;
  billingPeriodEnd?: string | null;
}): string | null => {
  const cn = normalizeCustomerNumber(b.customerNumberRaw);
  const from = day(b.billingPeriodStart);
  const to = day(b.billingPeriodEnd);
  if (!cn || !from || !to) return null;
  return `bill|${cn}|${from}|${to}`;
};

/**
 * 保證金的業務身分 ＝ `合約 | 第 N 期`。
 *
 * 它天生就唯一（合約已有 `(contract_id, seq)` unique）——
 * 這裡算 key 是為了讓**去重邏輯對兩種 subject 是同一套**，
 * 不是為了再擋一次。
 */
export const installmentBusinessKey = (i: {
  contractId?: string | number | null;
  seq?: number | null;
}): string | null => (
  i.contractId != null && i.seq != null ? `inst|${i.contractId}|${i.seq}` : null
);

/** 依 subjectType 分派（多型 —— 每一支都要處理）。 */
export const businessKeyOf = (
  subjectType: string,
  row: any,
): string | null => {
  if (subjectType === 'energyBill') {
    return billBusinessKey({
      customerNumberRaw: row?.customerNumberRaw,
      billingPeriodStart: row?.billingPeriodStart,
      billingPeriodEnd: row?.billingPeriodEnd,
    });
  }
  if (subjectType === 'energyInstallment') {
    return installmentBusinessKey({
      contractId: row?.contract_id ?? row?.contractId,
      seq: row?.seq,
    });
  }
  return null;
};
