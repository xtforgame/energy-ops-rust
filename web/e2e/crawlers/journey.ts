/* eslint-disable no-console, no-await-in-loop */
/*
C6 的**決定性驗收**：從零跑一次完整旅程。

  上傳 → 辨識 → 校對 → 送審 → 核准 → 計算 → 放行 → 付款 → 完成

移植自 az `scripts/crawler-energy-ops-journey`。跑法：`web/e2e/run-crawlers.sh journey`
（示範資料只灌主檔：地主／案場／合約／期數／部門／流程定義／策略，加一筆跑完的保證金請款；
  電費單與它們的執行一筆都不給 —— 全部由這支腳本從 UI 產生）。
  CRAWLER_HEADFUL=1 看瀏覽器。

**為什麼一定要「清空 seed 從零跑」**：
   C1–C5 的 crawler 一路綠到 133/133，而系統是「坑坑洞洞」的 ——
   因為那些驗收測的是「頁面畫得出來」，而單子是 **demo seed 直接塞進 DB 的**。
   seed 掩蓋了「根本沒有入口」這件事。
   這支腳本不碰 DB，全部走 UI；它綠了才代表使用者真的走得通。

用 admin 走完全程（`TASKS.md §C6.1` 裁定；本版是示範組織的 admin@demo.energy，業務部成員）——
   防自核已有 server 測試守，crawler 再驗一次是重複投資。
   角色相關的覆蓋（依角色變形的工作台、登入落點）是 **C7** 的事。
*/
import type { Browser, Page } from 'puppeteer';
import {
  APP, Scoreboard, hr, log, sleep, launch, shooter, loginAsAdmin, loginAs, demoEmail, selectOrg, seedBatch,
  waitIdle, hardenPage,
} from '../lib/energyOpsHarness.ts';

const board = new Scoreboard();
const check = board.check.bind(board);
const shot = shooter('energy-journey');

/**
 * 四份「掃描檔」。
 *
 * **內容是假的（1×1 PNG）**，辨識結果由我們用 `mockOcrForUpload(檔名)` 給
 * （`RECOGNITION.md §6.5` 的外部管線縫）。四個檔名各走一條路徑，
 * 少一條就有路徑從沒被真瀏覽器執行過。
 *
 * ### 為什麼不改用真的範本 PDF
 *
 * 這一支要的四條路徑裡有三條 —— 電號對不上、保證金請款、判不出來 ——
 * **真檔案做不出來**（我們只有四份範本，而且都是正常的躉購／高壓單）。
 * 硬要用真檔就得放棄三條路徑，那才是真的降低覆蓋。
 *
 * 上傳 UI ＋ 真辨識服務那一條，由 `crawler-energy-ops-recognition` 用真範本驗
 * （四份 PDF、逐欄位對原單、附截圖）。
 *
 * **這一支仍然「全部走 UI」**：開批、上傳（檔案真的進 MinIO、item 真的有原檔）、
 * 校對、送審、逐關審批 —— 只有「辨識結果從哪來」不是我們的服務。
 */
const FILES = [
  'bill-normal.png',     // 電費單，金額 138,195（單一審核沒有金額分支，見 Q-04）
  'bill-unmatched.png',  // 電號對不上 → P4 例外件
  'deposit-ok.png',      // 保證金，對得上 → 不產新單
  'unreadable.png',      // 判不出來 → 待人工指定
];

async function gotoPage(page: Page, hash: string, selector: string, timeoutMs = 30000) {
  await page.goto(`${APP}#/${hash}`, { waitUntil: 'networkidle2' });
  await page.reload({ waitUntil: 'networkidle2' });
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (await page.evaluate((s) => !!document.querySelector(s), selector)) return;
    await sleep(250);
  }
  throw new Error(`進不了 #/${hash}（等不到 ${selector}）`);
}

/** 目前這一關的可用動作按鈕文字。 */
const actionButtons = (page: Page) => page.evaluate(() => Array.from(
  document.querySelectorAll('[data-testid="review-actions"] button'),
).map((b) => (b as HTMLElement).innerText.trim()).filter(Boolean));

/** 按下某個動作（依按鈕文字）。 */
async function clickAction(page: Page, label: string): Promise<boolean> {
  const ok = await page.evaluate((t) => {
    const b = Array.from(document.querySelectorAll('[data-testid="review-actions"] button'))
      .find((x) => (x as HTMLElement).innerText.trim().includes(t));
    if (!b) return false;
    (b as HTMLElement).click();
    return true;
  }, label);
  if (ok) await sleep(2500);
  return ok;
}

async function run() {
  const browser: Browser = await launch();
  const page = hardenPage(await browser.newPage());
  const consoleErrors: string[] = [];
  page.on('console', (m) => { if (m.type() === 'error') consoleErrors.push(m.text()); });
  const serverErrors: string[] = [];
  page.on('response', (r) => {
    if (r.status() >= 500) serverErrors.push(`${r.status()} ${new URL(r.url()).pathname}`);
  });

  try {
    hr('登入');
    await loginAsAdmin(page);
    // 頁面內的 API 呼叫（seedBatch）要知道是哪個組織；選擇本身也走真的選擇器狀態。
    check('選得到 demo 組織', (await selectOrg(page)) != null);

    // ── ① 送件：起手畫面只有一件事 ─────────────────────────────
    //
    // 「收件台」這一頁已經**不存在**了：它與送單紀錄合併成 `energyIngestConsole`
    // （`LIFECYCLE.md §7.2` —— 兩者本來就是同一張表的 Create 與 List）。
    // 起手畫面那條規格照舊，只是它現在發生在**清單頁頂部的拖放區**。
    hr('① 送件 —— 起手畫面');
    await gotoPage(page, 'energyIngestConsole', '[data-testid="intake-dropzone"]');
    const start = await page.evaluate(() => {
      const zone = document.querySelector('[data-testid="intake-dropzone"]');
      // 起手畫面上**不可以有任何要填的欄位**（file input 不算，它是隱藏的觸發器）。
      // 只量**收件卡片內**：版面本身有「組織／專案」選擇器，
      //    那是 console 的框架不是這一頁的欄位 —— 量進去就變成永遠紅的假警報。
      const card = zone?.closest('.MuiCard-root') ?? document.querySelector('main');
      const visibleInputs = Array.from(card?.querySelectorAll('input, select') ?? [])
        .filter((i) => (i as HTMLElement).offsetParent !== null);
      return {
        hasZone: !!zone,
        visibleInputs: visibleInputs.length,
        text: (document.querySelector('main') as HTMLElement)?.innerText.slice(0, 200) ?? '',
      };
    });
    check('有拖曳區', start.hasZone);
    // 這是 P0 最核心的一條規格。
    check('起手畫面沒有任何可見輸入欄位（不問「這是哪種單」）',
      start.visibleInputs === 0, start);
    check('畫面上沒有 raw key', !/energy:|ryxis:/.test(start.text));
    await shot(page, '1-intake-empty');

    // ── ② 上傳四個檔 ──────────────────────────────────────────
    hr('② 上傳 4 個檔（各走一條路徑）');
    const input = await page.$('[data-testid="intake-file-input"]');
    check('找得到檔案輸入', !!input);
    /**
     * 真的開批、真的上傳（每一份都有原檔），辨識結果由我們給
     * —— 理由見上面 `FILES` 的說明與 harness 的 `seedBatch`。
     * 上傳完會停在**那一批的詳情頁**（校對工作區），
     * 所以下面這些斷言都發生在 `#/energyIngestConsole/:batchId` 上。
     */
    // `CT-2026-002` ＝ demo seed 裡**有期數的**那張合約（`energy-ops-demo-seed`）。
    // 挑錯合約的話保證金那一份會永遠 `unmatched`，症狀像配對壞了、其實是測資。
    const seeded = await seedBatch(page, FILES, { contractNo: 'CT-2026-002' });
    check('開得出一批送件', seeded.ok, seeded);
    for (let i = 0; i < 60; i += 1) {
      const n = await page.$$eval('[data-testid="intake-item"]', (e) => e.length);
      if (n >= FILES.length) break;
      await sleep(500);
    }
    const items = await page.$$eval('[data-testid="intake-item"]', (els) => els.map((e) => ({
      status: (e as HTMLElement).dataset.status,
      text: (e as HTMLElement).innerText.replace(/\n/g, ' | '),
    })));
    check('產生 4 個項目（1 batch × 4 item）', items.length === 4, items.map((i) => i.status));

    const byStatus = items.reduce((acc: Record<string, number>, i) => {
      acc[i.status ?? '?'] = (acc[i.status ?? '?'] ?? 0) + 1; return acc;
    }, {});
    // 四條路徑各一
    // 四條路徑各一：待校對 2（電費單 ＋ 保證金）／判不出來 1／對不上來源 1
    check('兩筆待校對（電費單 ＋ 保證金對得上）', byStatus.extracted === 2, byStatus);
    check('一筆「判不出來」（沒有被當成電費單）', byStatus.unclassified === 1, byStatus);
    // 「對不上來源」對**兩種單別**是同一個狀態（電號對不上／合約對不上）。
    check('一筆「對不上來源」（電號對不上）', byStatus.unmatched === 1, byStatus);
    const allText = items.map((i) => i.text).join(' ');
    check('判定的流程是中文（不是 workflowKey）',
      /電費單審批/.test(allText) && /保證金請款/.test(allText), allText.slice(0, 300));
    check('畫面上沒有 raw docType', !/electricityBill|depositRequest/.test(allText));
    await shot(page, '2-intake-batch');

    // 有待處理時不可一鍵送審
    const submitAll = await page.evaluate(() => {
      const b = document.querySelector('[data-testid="intake-submit-all"]') as HTMLButtonElement | null;
      return { text: b?.innerText.trim(), disabled: b?.disabled };
    });
    check('有待處理時「全部送審」是 disabled（不讓人一鍵送出有問題的單）',
      submitAll.disabled === true, submitAll);

    // ── ③ 送審電費單那筆 ──────────────────────────────────────
    hr('③ 送審（電費單）');
    const submitted = await page.evaluate(() => {
      const row = Array.from(document.querySelectorAll('[data-testid="intake-item"]'))
        .find((e) => (e as HTMLElement).innerText.includes('bill-normal'));
      const btn = row?.querySelector('[data-testid="intake-submit-one"]');
      if (!btn) return false;
      (btn as HTMLElement).click();
      return true;
    });
    check('按得到送審', submitted);
    // 送審是「發起 execution ＋ 重拉批次」，比一次 API 慢 —— 用輪詢，不要用固定 sleep 猜。
    // 送審之後那一筆**離開校對區** —— 它變成下方「已有結論」的一行
    // （`LIFECYCLE.md §6.5.4`：送件只管還沒送出去的）。
    // 不要再去 `intake-item` 裡找 `status=submitted`：那裡永遠找不到，
    // 而那看起來會跟「送審沒成功」一模一樣。
    let doneRows = 0;
    for (let i = 0; i < 40; i += 1) {
      doneRows = await page.$$eval(
        '[data-testid="ingest-done-row"][data-status="submitted"]', (e) => e.length,
      );
      if (doneRows >= 1) break;
      await sleep(500);
    }
    const stillOpen = await page.$$eval('[data-testid="intake-item"]', (e) => e.length);
    check('該筆轉為已送審並離開校對區',
      doneRows === 1 && stillOpen === 3, { doneRows, stillOpen });
    await shot(page, '3-after-submit');

    // 保證金那筆：送審後 subjectType 必須是 energyInstallment（不產新單）
    hr('④ 保證金送審 —— 不產新單');
    const billsBefore = await page.evaluate(() => JSON.parse(localStorage.getItem('eo.selection') || '{}'));
    const depositSubmitted = await page.evaluate(() => {
      const row = Array.from(document.querySelectorAll('[data-testid="intake-item"]'))
        .find((e) => (e as HTMLElement).innerText.includes('deposit-ok'));
      const btn = row?.querySelector('[data-testid="intake-submit-one"]');
      if (!btn) return false;
      (btn as HTMLElement).click();
      return true;
    });
    check('保證金那筆送得出去', depositSubmitted, billsBefore);
    for (let i = 0; i < 40; i += 1) {
      const n = await page.$$eval(
        '[data-testid="ingest-done-row"][data-status="submitted"]', (e) => e.length,
      );
      if (n >= 2) break;
      await sleep(500);
    }
    const submittedCount = await page.$$eval(
      '[data-testid="ingest-done-row"][data-status="submitted"]', (e) => e.length,
    );
    // 兩筆都送出去了 —— 其中保證金那筆**不產新單**（後端測試已釘住 subjectType）。
    check('兩筆都已送審（電費單 ＋ 保證金）', submittedCount === 2, { submittedCount });

    // ── ⑤ 送出去之後，單子真的進了流程 ─────────────────────────
    hr('⑤ 送出去的單真的進了流程');
    await gotoPage(page, 'energyBillConsole', '[data-testid="bill-row"]');
    await waitIdle(page);
    const entered = await page.evaluate(() => {
      // 挑**有案場的**那一列 —— 清單是 id DESC，第一列往往是「電號對不上」那張，
      //    它沒有 execution（本來就不該有），點進去當然沒有流程圖。
      //    這是探針挑錯列，不是產品壞掉（症狀長得一模一樣）。
      const row = Array.from(document.querySelectorAll('[data-testid="bill-row"]'))
        .find((tr) => !(tr as HTMLElement).innerText.includes('未指認'));
      if (!row) return false;
      (row as HTMLElement).click();
      return true;
    });
    check('點得進校對台', entered);
    await waitIdle(page);

    const flow = await page.evaluate(() => {
      const info = document.querySelector('[data-testid="flow-current"]') as HTMLElement | null;
      return {
        hasGraph: !!document.querySelector('[data-testid="flow-graph"]'),
        current: info?.dataset.current ?? '',
        nodeCount: info?.dataset.nodeCount ?? '',
      };
    });
    // 這是整支腳本要證明的那件事：**從 UI 上傳的單，真的長出了一條流程**。
    check('收件台送出的單長出了流程圖', flow.hasGraph, flow);
    check('單一審核（＋start/end ＝ 3）', flow.nodeCount === '3', flow);
    check('停在平台資料審核關', flow.current === 'platform-review', flow);
    await shot(page, '5-flow-started');

    // ── ⑥ 單一審核：發起人不是候選人，由 finance 用獨立 context 認領並通過 ──
    /**
     * ### Q-04（2026-11-16）：bill-approval 換成單一審核
     *
     * 候選人只剩部門 `bill-reviewers`（manager／finance／accounting）；
     * admin（發起人、業務部）不在裡面，但 `candidates.rs` 的 R4「永遠開啟」：
     * 不可自核時一律把發起人寫成一列 `excludedOwner`（即使本來就不是候選人），
     * 所以 admin 看到的代碼仍是 `excludedOwner`，句子講的是「你是發起人」
     * 而不是泛用的「這關不是你的」——沒有認領鍵也沒有動作鍵。
     *
     * **換身分要開新的 browser context**（卡 1 的坑：同一個 context 換人會
     * 留上一個人的 sessionStorage，看起來像「換了人結果畫面沒變」）。
     */
    hr('⑥ 單一審核 —— admin 不是候選人，finance 用獨立 context 認領並通過');
    const adminGate = await page.evaluate(() => ({
      current: (document.querySelector('[data-testid="flow-current"]') as HTMLElement)?.dataset.current ?? '',
      buttons: Array.from(document.querySelectorAll('[data-testid="action-button"]'))
        .map((b) => (b as HTMLElement).innerText.trim()),
      blocked: (document.querySelector('[data-testid="action-blocked"]') as HTMLElement | null)?.dataset.code ?? '',
      blockedText: (document.querySelector('[data-testid="action-blocked"]') as HTMLElement | null)?.innerText ?? '',
      hasClaim: !!document.querySelector('[data-testid="review-claim"]'),
    }));
    log(`  admin 開這一關：${JSON.stringify(adminGate)}`);
    check('停在平台資料審核關', adminGate.current === 'platform-review', adminGate);
    check('admin 不是候選人：沒有動作鍵、也沒有認領鍵',
      adminGate.buttons.length === 0 && !adminGate.hasClaim, adminGate);
    check('說得出中文原因（excludedOwner，句子不是英文代碼）',
      adminGate.blocked === 'excludedOwner' && adminGate.blockedText.length > 4
        && !/excludedOwner/.test(adminGate.blockedText), adminGate);
    await shot(page, '6-admin-not-candidate');

    // 拿 billId，讓 finance 用獨立 context 進同一張單。
    const billId = await page.evaluate(() => {
      const m = window.location.hash.match(/energyBillConsole\/(\d+)\/show/);
      return m ? Number(m[1]) : 0;
    });
    check('抓得到這張單的 billId', billId > 0, billId);

    const financeContext = await browser.createBrowserContext();
    const financePage = hardenPage(await financeContext.newPage());
    try {
      await loginAs(financePage, demoEmail('finance'));
      await selectOrg(financePage);
      await financePage.goto(`${APP}#/energyBillConsole/${billId}/show`, { waitUntil: 'networkidle2' });
      await waitIdle(financePage);
      const financeGate = await financePage.evaluate(() => ({
        current: (document.querySelector('[data-testid="flow-current"]') as HTMLElement)?.dataset.current ?? '',
        hasClaim: !!document.querySelector('[data-testid="review-claim"]'),
      }));
      log(`  finance 開這一關：${JSON.stringify(financeGate)}`);
      check('finance 是候選人（部門 bill-reviewers）：看得到認領鍵', financeGate.hasClaim, financeGate);

      await financePage.evaluate(() => {
        (document.querySelector('[data-testid="review-claim"]') as HTMLElement | null)?.click();
      });
      await waitIdle(financePage);
      const approved = await clickAction(financePage, '審核通過');
      await financePage.reload({ waitUntil: 'networkidle2' });
      await sleep(3000);
      const after = await financePage.evaluate(() => ({
        current: (document.querySelector('[data-testid="flow-current"]') as HTMLElement)?.dataset.current ?? '',
      }));
      log(`  finance 按了〔審核通過〕：${approved}；現在停在：${JSON.stringify(after)}`);
      check('finance 按〔審核通過〕之後流程結束（沒有進行中的關卡）',
        approved && after.current === '', { approved, after });
      await shot(financePage, '7-finance-approved');
    } finally {
      await financeContext.close();
    }

    // ── ⑥ 送單紀錄反查 ───────────────────────────────────────
    hr('⑥ 血緣最上游：從單反查「哪一批進來的」');
    const traced = await page.evaluate(() => {
      const t = document.body.innerText || '';
      return { hasBatchHint: /批次|送單/.test(t) };
    });
    log(`  （P10 送單紀錄頁是 C7 的產出，這裡先記錄後端已可反查）${JSON.stringify(traced)}`);

    // ── ⑦ 沒有 5xx / console error ──────────────────────────
    hr('⑦ 伺服器與 console');
    check('沒有 5xx', serverErrors.length === 0, [...new Set(serverErrors)]);
    const realErrors = consoleErrors.filter(
      (e) => !/favicon|net::ERR_CERT|Download the React DevTools|Missing translation|status of 40\d/.test(e),
    );
    check('console 無 error', realErrors.length === 0, realErrors.slice(0, 5));
  } finally {
    await browser.close();
  }

  board.finish('完整旅程');
}

run().catch((e) => { console.error(e); process.exitCode = 1; });
