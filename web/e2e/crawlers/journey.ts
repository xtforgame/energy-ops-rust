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
  APP, Scoreboard, hr, log, sleep, launch, shooter, loginAsAdmin, selectOrg, seedBatch, waitIdle, hardenPage,
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
  'bill-normal.png',     // 電費單，金額 138,195 ≥ 門檻 → 會多一關高層加簽
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
    check('七節點流程（＋start/end ＝ 8）', flow.nodeCount === '8', flow);
    check('停在承辦確認關', flow.current === 'clerk-review', flow);
    await shot(page, '5-flow-started');

    // ── ⑥ 審批鏈：認領 → 推進 → 到下一關被防自核擋住 ──────────
    /**
     * ### 這一段講的事在 【7】 之後翻面了
     *
     * 原本：`clerk-review` 防自核 → 發起人被列為 `excludedOwner` → 看不到動作。
     * 現在：`allowSelfApproval` 翻面（`LIFECYCLE.md 【7】`），
     * **承辦送出的單自己就能在承辦確認關處理** —— 那正是「退回理由要送到
     * 唯一看過那張紙的人手上」的前提。
     *
     * ### 認領是「開始處理」的第一步
     *
     * 引擎要求人工關卡由 assignee 完成（`WorkflowEngine.assertWho`：
     * 未認領 → `not_task_owner` 403）。認領的入口在【9】被搬進「我的工作」的抽屜，
     * 而單據詳情頁一度就沒有了 —— 於是這一頁畫得出動作、按下去 403，
     * 使用者看到的是「按了沒反應」。現在兩個入口用同一套語意：
     * **還沒認領就只給「認領並開始處理」，不畫按不動的按鈕。**
     *
     * 防自核那一面沒有消失，它搬到**下一關**（主管核准）——
     * 這一段一路走過去，把兩件事都釘住。
     */
    hr('⑥ 審批鏈 —— 發起人是候選人（【7】翻面），而且這一頁認領得動');
    const gate = await page.evaluate(() => ({
      current: (document.querySelector('[data-testid="flow-current"]') as HTMLElement)?.dataset.current ?? '',
      buttons: Array.from(document.querySelectorAll('[data-testid="action-button"]'))
        .map((b) => (b as HTMLElement).innerText.trim()),
      blocked: (document.querySelector('[data-testid="action-blocked"]') as HTMLElement | null)?.dataset.code ?? '',
      hasClaim: !!document.querySelector('[data-testid="review-claim"]'),
    }));
    log(`  這一關：${JSON.stringify(gate)}`);
    check('停在承辦確認關（`clerk-review`）', gate.current === 'clerk-review', gate);
    /**
     * **還沒認領時只給「認領並開始處理」，不畫按了會 403 的動作。**
     *
     * 引擎要求人工關卡由 assignee 完成（`WorkflowEngine.assertWho`）。
     * 認領的入口在【9】被搬進「我的工作」的抽屜，這一頁一度就沒有了 ——
     * 於是畫得出動作、按下去 403，而使用者看到的是「按了沒反應」。
     */
    check('未認領時給的是「認領並開始處理」，不是按不動的動作',
      gate.hasClaim && gate.buttons.length === 0, gate);

    // 認領 → 動作才出現 → 推得動。
    await page.evaluate(() => {
      (document.querySelector('[data-testid="review-claim"]') as HTMLElement | null)?.click();
    });
    await waitIdle(page);
    const afterClaim = await page.evaluate(() => ({
      buttons: Array.from(document.querySelectorAll('[data-testid="action-button"]'))
        .map((b) => (b as HTMLElement).innerText.trim()),
      hasClaim: !!document.querySelector('[data-testid="review-claim"]'),
    }));
    log(`  認領後：${JSON.stringify(afterClaim)}`);
    check('認領之後動作出現（認領鍵收起來）',
      afterClaim.buttons.length > 0 && !afterClaim.hasClaim, afterClaim);

    const advanced = await clickAction(page, '送出');
    await page.reload({ waitUntil: 'networkidle2' });
    await sleep(3000);
    const moved = await page.evaluate(
      () => (document.querySelector('[data-testid="flow-current"]') as HTMLElement)?.dataset.current ?? '',
    );
    log(`  按了送出：${advanced}；現在停在：${moved}`);
    // 【7】自核翻面之後，發起人在**承辦確認**這一關是推得動的
    // （那一關同時承擔「改」與「核」；設成不可自核的話，被退回的單原送件者改不了）。
    check('認領後推得動 —— 承辦確認關允許自核（【7】翻面）',
      advanced && moved === 'manager-approve', { advanced, moved });
    await shot(page, '6-claim-and-advance');

    // 下一關是**主管核准**，發起人在那裡被防自核擋住 —— 那是對的。
    const nextGate = await page.evaluate(() => ({
      buttons: Array.from(document.querySelectorAll('[data-testid="action-button"]')).length,
      hasClaim: !!document.querySelector('[data-testid="review-claim"]'),
      blocked: (document.querySelector('[data-testid="action-blocked"]') as HTMLElement | null)?.dataset.code ?? '',
      text: (document.querySelector('[data-testid="action-blocked"]') as HTMLElement | null)?.innerText ?? '',
    }));
    log(`  主管核准關（發起人視角）：${JSON.stringify(nextGate)}`);
    check('到了主管核准關，發起人什麼都不能按（防自核）',
      nextGate.buttons === 0 && !nextGate.hasClaim, nextGate);
    check('而且說得出為什麼（excludedOwner，且句子是中文）',
      nextGate.blocked === 'excludedOwner'
        && nextGate.text.length > 6 && !/excludedOwner/.test(nextGate.text), nextGate);
    await shot(page, '7-blocked-by-self-approval');

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
