/**
 * crawler 用的假辨識結果（移植自 az `src/server/plugins/EnergyOpsPlugin/bills/ingestFixtures.ts`）。
 *
 * 只給 crawler 造「電號對不上」「保證金請款」「判不出來」「同一張紙送兩次」「金額跨過加簽門檻」
 * 這些真檔案做不出來的情境：harness 的 `seedBatch` 真的開批、真的上傳一份佔位 PDF，
 * 再經 `POST E/ingest-items/{id}/recognition {ocrRaw}`（外部管線那個縫）套上這裡的結果。
 * 真檔案 → 真辨識那一條由 `crawlers/recognition.ts` 用四份範本 PDF 驗。
 *
 * 約定都靠檔名：同檔名 → 同計費期間 → 同一張紙；`-unmatched` → 電號對不上；
 * `small`／`low` → 低於加簽門檻；`deposit` → 保證金請款；`unreadable` → 判不出來。
 */

export interface OcrFixture {
  /** 給人看的說明（收件台的除錯用）。 */
  label: string;
  ocrRaw: unknown;
}

/**
 * **計費期間由檔名決定**（mock 專用的約定）。
 *
 * 這一段是被 `businessKey` 去重逼出來的：期間如果寫死，
 * 同一個組織上傳的**每一張電費單都是同一個業務身分**
 * （電號來自主檔、期間固定）—— 於是第二張就被 409 擋下來，
 * 而那不是產品的問題，是 fixture 讓所有檔案都變成同一張紙。
 *
 * 規則：**同檔名 → 同期間 → 同一張紙**。這正好也是驗收要的兩件事：
 * - 想製造重複 → 上傳**同一個檔名**兩次
 * - 想製造不同的單 → 換一個檔名
 *
 * 真 OCR 接上時整段消失（期間來自紙本），`electricityBill()` 的簽名不變。
 */
const periodForName = (originalFilename: string): { start: string; end: string; issued: string } => {
  const n = String(originalFilename || '');
  // FNV-1a（不是 `Math.random`，也不是時鐘）—— 同一個檔名任何時候都算出同一組。
  let h = 0x811c9dc5;
  for (let i = 0; i < n.length; i += 1) {
    h ^= n.charCodeAt(i);
    h = Math.imul(h, 0x01000193) >>> 0;
  }
  /**
   * **值域要開得夠大**。
   *
   * 第一版只用 `h % 22`（兩年 24 個月）：驗收才用五個檔名就撞到 ——
   * `bill-small-revisit.png` 與 `bill-big.png` 落在同一個期間，
   * 於是它們變成同一張紙，第二次送審被 409 擋掉。
   * 症狀跟「去重壞了」一模一樣，其實是**值域太窄**。
   *
   * 現在：240 個月 × 28 個起日 ＝ 6720 個身分。
   */
  const startMonth = 1 + (h % 240);        // 2015-01 起算，20 年
  const startDay = 1 + (Math.floor(h / 240) % 28);
  const y0 = 2015 + Math.floor((startMonth - 1) / 12);
  const m0 = ((startMonth - 1) % 12) + 1;
  const endTotal = startMonth + 2;
  const y1 = 2015 + Math.floor((endTotal - 1) / 12);
  const m1 = ((endTotal - 1) % 12) + 1;
  // 期末＝兩個月後的前一天（同 `05 → 04` 的既有形狀）。
  const endDay = startDay === 1 ? 28 : startDay - 1;
  const pad = (v: number) => String(v).padStart(2, '0');
  return {
    start: `${y0}-${pad(m0)}-${pad(startDay)}`,
    end: `${y1}-${pad(m1)}-${pad(endDay)}`,
    issued: `${y1}-${pad(m1)}-28`,
  };
};

/** 電費單（正常）。 */
const electricityBill = (
  customerNumber: string, totalAmount: number, generationKwh: number, originalFilename: string,
): unknown => {
  const p = periodForName(originalFilename);
  return {
    raw: '台灣電力股份有限公司　電費通知',
    電號: customerNumber,
    計費期間: `${p.start} ～ ${p.end}`,
    用電度數: generationKwh,
    bills: [{
      customerNumber,
      totalAmount,
      generationKwh,
      billingPeriodStart: p.start,
      billingPeriodEnd: p.end,
      issuedAt: p.issued,
      dueDate: p.issued,
    }],
  };
};

/** 保證金請款單。 */
const depositRequest = (contractNo: string, seq: number, amount: number): unknown => ({
  raw: '履約保證金請款單',
  合約編號: contractNo,
  期別: `第 ${seq} 期`,
  應付日: '2026-03-15',
  請款金額: String(amount),
});

/**
 * **故意判不出來**的那一份。
 * 這條路徑必須有測試與真瀏覽器走過 —— 否則它會在真實 OCR 接上的那天第一次執行。
 */
const unreadable = (): unknown => ({
  raw: '（掃描品質不佳，僅辨識出零星文字）',
  fragments: ['-----', '3?8', '□□□'],
});

/**
 * 依**檔名**挑 fixture。
 *
 * 這是 mock 專用的約定，不是產品行為 —— 真 OCR 只看檔案內容，不看檔名。
 * 用檔名是為了讓**真瀏覽器驗收**能挑得到想走的那條路徑
 * （crawler 上傳 `deposit-bad.jpg` 就一定會走到「對不上」）。
 */
export function mockOcrForUpload(originalFilename: string, opts: {
  /** 已知的電號（要讓它對得上時傳進來）。 */
  customerNumber?: string;
  /** 已知的合約編號。 */
  contractNo?: string;
} = {}): OcrFixture {
  const n = String(originalFilename || '').toLowerCase();

  if (/unreadable|junk|unknown|noise/.test(n)) {
    return { label: '判不出來（故意的）', ocrRaw: unreadable() };
  }
  if (/deposit|保證金|installment/.test(n)) {
    // `-bad` ＝ 合約編號對不上 → 走 P4 例外件。
    const bad = /bad|unmatch|nomatch/.test(n);
    return {
      label: bad ? '保證金請款（合約對不上）' : '保證金請款',
      // `opts.contractNo` 是**這個組織真的有的**合約（呼叫端查出來的）；
      // 沒有的話就沒有對得上的可能 —— 那不是 bug，是主檔還沒建。
      ocrRaw: depositRequest(bad ? 'JV-0000-0000' : (opts.contractNo ?? 'JV-0000-0000'), 2, 150000),
    };
  }
  // 預設：電費單。`-unmatched` ＝ 電號對不上（US-5 的例外件）。
  const unmatched = /unmatched|nostation/.test(n);
  // `-small` 走低於加簽門檻的金額 —— 讓「分支條件是資料」在真瀏覽器上驗得到。
  const amount = /small|low/.test(n) ? 60000 : 138195;
  return {
    label: unmatched ? '電費單（電號對不上）' : '電費單',
    ocrRaw: electricityBill(
      // 同上：對得上的那份用**真實**電號，不寫死。
      unmatched ? '09-0000-0000' : (opts.customerNumber ?? '09-0000-0000'),
      amount,
      48250,
      originalFilename,
    ),
  };
}
