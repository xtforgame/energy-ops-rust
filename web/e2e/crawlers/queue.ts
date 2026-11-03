/* eslint-disable no-console, no-await-in-loop */
/**
 * **「我的工作」是一條佇列，不是一張清單**（`LIFECYCLE.md §8`）。
 *
 * 使用者實測回報四件事，根因是同一個 ——
 * 這一頁被當成「清單」設計，但它實際上是一條「佇列」：
 *
 * | | 目標 | 動線特性 |
 * |---|---|---|
 * | 清單 | 找到東西 | 可隨時重排、點進去就離開、回來重找 |
 * | **佇列** | **把它清空** | **位置必須穩定**、就地檢視、裁斷完直接接下一件 |
 *
 * 這一支驗的全部是**動線**，不是「頁面畫不畫得出來」。
 *
 * ### 為什麼是獨立一支
 *
 * 它原本接在 `crawler-energy-ops-mywork` 的最後面，第一次跑就撞上
 * `Session closed`（32 條綠、然後整支掛掉）——
 * **排在長腳本最後面的段落等於沒有守衛**（【7】拆 roles crawler 學到的同一件事）。
 *
 * ### 這一支怎麼跑
 *
 * 移植自 az `scripts/crawler-energy-ops-queue`：`web/e2e/run-crawlers.sh queue`
 * （示範資料、不灌範本電費單）。
 *
 * **可重複跑**：自己上傳兩張帶時間戳的單並送審（「處理下一筆」要有下一筆），
 * 斷言全部是相對值（順序相同／列數不變／有沒有第二支 `/workbench`）。
 */
import {
  Scoreboard, hr, log, sleep, launch, shooter, loginAs, selectOrg, goto,
  seedBatch, hardenPage,
  waitIdle,
  eapi, demoEmail,
} from '../lib/energyOpsHarness.ts';

const board = new Scoreboard();
const check = board.check.bind(board);
const shot = shooter('energy-queue');

async function run() {
  const browser = await launch();
  // `hardenPage`：把 `page.evaluate` 包上 detach 復原（見 harness 裡的說明）。
  const page = hardenPage(await browser.newPage());
  const serverErrors: string[] = [];
  page.on('response', (r) => {
    if (r.status() >= 500) serverErrors.push(`${r.status()} ${new URL(r.url()).pathname}`);
  });

  try {
    // ── ⑧ §8 這一頁是一條**佇列**不是清單 ─────────────────────
    /**
     * 使用者實測回報四件事，根因是同一個：
     * **這一頁被當成清單設計，但它實際上是一條佇列。**
     *
     * 清單回答「東西在哪」→ 可隨時重排、點進去就離開、回來重找
     * 佇列回答「還剩幾件」→ **位置必須穩定**、就地檢視、裁斷完直接接下一件
     *
     * 下面每一條驗的都是「佇列的動線」，不是「頁面畫得出來」。
     */
    hr('⑧ §8 佇列動線：認領不跳位、就地檢視、接下一筆');

    // 先造兩筆待辦 —— 「處理下一筆」要有下一筆才驗得到。
    await loginAs(page, demoEmail('operator'));
    await selectOrg(page);
    const qStamp = String(process.pid);
    // 一批一張 —— 兩張要分開送審才會變成兩筆待辦。
    // （`seedBatch` 走外部管線那個縫；假圖丟 dropzone 會被真辨識服務判成 unclassified。）
    const queueFiles = [`bill-small-q1-${qStamp}.png`, `bill-small-q2-${qStamp}.png`];
    for (const f of queueFiles) {
      const b = await seedBatch(page, [f]);
      const billId = await (async () => {
        const r = await eapi(page, 'GET', `/ingest-batches/${b.batchId}`);
        const it = (r.data?.items ?? []).find((i: any) => i.billId != null);
        return it ? Number(it.billId) : 0;
      })();
      await goto(page, `energyBillConsole/${billId}/show`, '[data-testid="review-field"], table');
      await waitIdle(page);
      await page.evaluate(() => {
        (document.querySelector('[data-testid="review-submit"]') as HTMLElement | null)?.click();
      });
      await waitIdle(page);
    }

    // 主管的工作台上會有這兩筆（clerk-review 的候選池是業務部全員）。
    await loginAs(page, demoEmail('manager'));
    await selectOrg(page);
    await goto(page, 'energyMyWorkConsole', '[data-testid="workbench-row"], .MuiAlert-root');
    await waitIdle(page);

    const rowIds = () => page.$$eval('[data-testid="workbench-row"]',
      (els) => els.map((e) => (e as HTMLElement).dataset.task ?? ''));
    const qBefore = await rowIds();
    log(`  列的順序（前）：${JSON.stringify(qBefore)}`);
    // 探針自檢：一列都沒有的話下面每一條都會用空陣列假裝通過。
    check('主管的工作台上有待辦（不然下面全部沒有意義）', qBefore.length >= 2, qBefore);

    hr('⑧-a 列的排版：兩行，整列可點');
    const layout = await page.evaluate(() => {
      const row = document.querySelector('[data-testid="workbench-row"]') as HTMLElement | null;
      const main = row?.querySelector('[data-testid="workbench-open"]') as HTMLElement | null;
      const sub = row?.querySelector('[data-testid="workbench-subline"]') as HTMLElement | null;
      if (!row || !main || !sub) return { note: 'missing' };
      const cm = getComputedStyle(main); const cs = getComputedStyle(sub);
      return {
        // 主行粗、次行小且次要色 —— 「案場跟時間範圍黏在一起」就是這兩層沒分開。
        mainWeight: Number(cm.fontWeight), mainSize: parseFloat(cm.fontSize),
        subSize: parseFloat(cs.fontSize), subTabular: cs.fontVariantNumeric,
        // 主行**不可以是按鈕** —— 列本身可點，裡面再放一個看起來可點的東西
        // 只會讓人猶豫該點哪個。
        mainIsButton: main.tagName === 'BUTTON' || !!main.closest('button'),
        rowClickable: getComputedStyle(row).cursor === 'pointer',
        // 次行同時有電號與期間（兩者在**同一行**，跟案場名分開）
        subText: sub.innerText.replace(/\s+/g, ' ').trim(),
      };
    });
    log(`  排版：${JSON.stringify(layout)}`);
    check('主行粗體、次行小字', (layout as any).mainWeight >= 600
      && (layout as any).subSize < (layout as any).mainSize, layout);
    check('案場名不再是按鈕樣式', (layout as any).mainIsButton === false, layout);
    check('整列可點', (layout as any).rowClickable === true, layout);
    check('次行是「電號 · 計費期間」', /·/.test((layout as any).subText ?? ''), layout);

    hr('⑧-b 認領不跳位、也不重抓整頁');
    /**
     * **這是四個問題裡最關鍵的一條。**
     * 用 DOM 順序斷言而不是截圖比對：截圖只證明「看起來一樣」，
     * 序列證明「**真的沒動**」。
     */
    const reqs: string[] = [];
    const onReq = (r: any) => {
      const u = new URL(r.url()).pathname;
      if (/\/workbench$|\/tasks\/\d+\/(claim|actions)$/.test(u)) reqs.push(`${r.method()} ${u}`);
    };
    page.on('request', onReq);

    // 整列可點 → 開抽屜（不是點左邊的案場名）
    await page.evaluate(() => {
      (document.querySelector('[data-testid="workbench-row"]') as HTMLElement | null)?.click();
    });
    await waitIdle(page);
    const drawer = await page.evaluate(() => {
      const d = document.querySelector('[data-testid="mywork-drawer"]') as HTMLElement | null;
      return {
        open: !!d,
        progress: (document.querySelector('[data-testid="drawer-progress"]') as HTMLElement)?.innerText ?? '',
        hasClaim: !!document.querySelector('[data-testid="queue-claim"]'),
        hash: window.location.hash,
        // 抽屜裡是**同一個**單據詳情元件 —— 有它的核心區塊就對了
        hasDetail: !!d?.querySelector('[data-testid="review-field"]'),
      };
    });
    log(`  抽屜：${JSON.stringify(drawer)}`);
    check('點列開的是抽屜，不是換頁', drawer.open === true && /energyMyWorkConsole/.test(drawer.hash), drawer);
    check('抽屜裡是同一個單據詳情元件', drawer.hasDetail === true, drawer);
    check('標頭有「第 N / M 筆」的進度感', /\d+\s*\/\s*\d+|第.*筆/.test(drawer.progress), drawer.progress);
    check('網址寫了 ?task=', /[?&]task=\d+/.test(drawer.hash), drawer.hash);
    check('未認領時抽屜裡有〔認領並開始處理〕', drawer.hasClaim === true, drawer);

    /**
     * **打開抽屜就要看得到「要改的那些欄位」。**
     *
     * 這一條是我自己弄壞一次之後補的：為了修「欄位被壓扁」，
     * 我把雙欄改成在窄容器時堆疊 —— 而左欄的原檔示意圖是 A4 比例，
     * 一變全寬就長到一千多 px，把右邊的欄位整個推到捲軸外。
     *
     * **當時 crawler 全綠**：它驗的是「抽屜開得起來」，
     * 不是「打開之後看得到要做的事」。所以這一條問的是絕對位置與寬度，
     * 不是「元素存不存在」——`display:none` 以外的看不見，`querySelector` 一律看不出來。
     */
    const usable = await page.evaluate(() => {
      const el = document.querySelector('[data-testid="final-totalAmount"]') as HTMLElement | null;
      /**
       * 探針抓的是**左欄那張卡片**，不是原檔示意圖。
       *
       * 示意圖只有「這張單掛了原始檔案」時才畫 —— 而 seed 直接建的單
       * 沒走過收件台、沒有 `role='source'` 的附件，左欄顯示的是
       * 「這張單沒有掛原始檔案」那段文字（`LIFECYCLE.md §6.7.2`）。
       *
       * 前一版拿示意圖當探針，於是**同一條斷言在兩種 seed 狀態下給出不同結果**：
       * 帶 `ENERGY_DEMO_NO_BILLS=1` 跑過收件台的環境綠，預設 seed 紅。
       * 左欄本身無論如何都存在 —— 那才是「並排」要守的東西。
       */
      const left = document.querySelector('[data-testid="review-left-column"]') as HTMLElement | null;
      if (!el) return { missing: '最終值欄位' };
      if (!left) return { missing: '左欄面板' };
      const r = el.getBoundingClientRect();
      const l = left.getBoundingClientRect();
      return {
        top: Math.round(r.top), width: Math.round(r.width),
        viewportH: window.innerHeight,
        inView: r.top >= 0 && r.top < window.innerHeight,
        wideEnough: r.width >= 90,
        value: (el as HTMLInputElement).value,
        /**
         * 左右並排還是上下堆疊 —— 比的是**左右關係**，不是上下距離。
         *
         * 原本用 `|r.top - l.top| < 400`：那在右欄上方多了東西（辨識摘要、
         * 模式說明）時就會失敗，而版面明明還是兩欄。用「欄位在左欄的右邊」
         * 判，才是「並排」這個詞真正的意思。
         */
        leftTop: Math.round(l.top),
        leftRight: Math.round(l.right),
        sideBySide: r.left >= l.right - 8,
      };
    });
    log(`  最終值欄位：${JSON.stringify(usable)}`);
    /**
     * **前提先用明確的失敗講出來。**
     *
     * 前一版寫成 `src ? … : null`，於是「元素根本沒找到」會靜靜地變成
     * `sideBySide: null`，再跟 `true` 比對失敗 ——
     * 斷言在報告「版面錯了」，而它實際遇到的是「面板不存在」。
     * **誤報跟真發現長得一樣**，而且它偽裝成的還是一個已經修過一次的老問題，
     * 下一個人會以為版面又壞了。
     *
     * 這跟 `actOnWorkbench` 學到的是同一句：
     * **前置條件要用 throw／明確的紅燈表達，不要退化成一個看起來像結果的值。**
     */
    check(`量得到那兩塊（缺的話下面三條都測不了）${
      (usable as any).missing ? ` —— 找不到${(usable as any).missing}` : ''}`,
    !(usable as any).missing, usable);
    check('打開抽屜就看得到「最終值」欄位（不必捲）', (usable as any).inView === true, usable);
    check('而且欄位寬到放得下金額（不是被壓成兩個字）',
      (usable as any).wideEnough === true, usable);
    /**
     * 這一條是**擦邊過那次**補的：上一版只問「看不看得見」，
     * 而在 1100px 高的視窗裡，堆疊後的欄位剛好還「勉強」在視窗內（top 1056），
     * 於是斷言綠了、版面卻是錯的。
     * `top 1056 / 視窗 1100` 那個擦邊數字就是徵兆 —— 數字與畫面矛盾時以畫面為準。
     */
    check('左欄與欄位是**左右並排**，不是被推成上下堆疊',
      (usable as any).sideBySide === true, usable);
    await shot(page, '8-drawer-open');

    reqs.length = 0;
    await page.evaluate(() => {
      (document.querySelector('[data-testid="queue-claim"]') as HTMLElement | null)?.click();
    });
    await waitIdle(page);
    const qAfter = await rowIds();
    log(`  列的順序（後）：${JSON.stringify(qAfter)}`);
    log(`  這一段打了哪些請求：${JSON.stringify(reqs)}`);
    check('認領後列的順序**完全相同**（不重排、不換區）',
      JSON.stringify(qAfter) === JSON.stringify(qBefore), { qBefore, qAfter });
    check('認領沒有觸發整頁重抓（沒有第二支 /workbench）',
      reqs.filter((r) => /\/workbench$/.test(r)).length === 0, reqs);
    check('而且真的打了 claim（不然「沒重抓」是廢話）',
      reqs.some((r) => /\/claim$/.test(r)), reqs);
    const claimed = await page.evaluate(() => ({
      chip: !!document.querySelector('[data-testid="row-claimed"]'),
      hasNext: !!document.querySelector('[data-testid="queue-act-next"]'),
    }));
    check('那一列原地多了「你已認領」', claimed.chip === true, claimed);
    check('認領後抽屜出現〔…並處理下一筆〕', claimed.hasNext === true, claimed);
    await shot(page, '8b-claimed-in-place');

    hr('⑧-c 核准並處理下一筆 —— 全程不離開這一頁');
    const firstTask = await page.evaluate(() => new URL(window.location.hash.slice(1), 'http://x')
      .searchParams.get('task'));
    await page.evaluate(() => {
      (document.querySelector('[data-testid="queue-act-next"]') as HTMLElement | null)?.click();
    });
    await waitIdle(page);
    const nextState = await page.evaluate(() => ({
      hash: window.location.hash,
      drawerOpen: !!document.querySelector('[data-testid="mywork-drawer"]'),
      handled: document.querySelectorAll('[data-testid="row-handled"]').length,
      rows: document.querySelectorAll('[data-testid="workbench-row"]').length,
    }));
    const nextTask = await page.evaluate(() => new URL(window.location.hash.slice(1), 'http://x')
      .searchParams.get('task'));
    log(`  下一筆：${JSON.stringify({ firstTask, nextTask, ...nextState })}`);
    check('全程沒有離開這一頁', /energyMyWorkConsole/.test(nextState.hash), nextState.hash);
    check('抽屜直接換成下一件（task 換了、抽屜還開著）',
      nextState.drawerOpen === true && nextTask !== firstTask && !!nextTask,
      { firstTask, nextTask, drawerOpen: nextState.drawerOpen });
    /**
     * 已處理的列**原地變灰保留**，不立即消失 ——
     * 立即消失會讓下一筆跳上來佔住滑鼠位置，那是誤按的主要來源。
     */
    check('處理過的那一列原地留著並標「已處理」', nextState.handled >= 1, nextState);
    check('列數沒有變少（沒有偷偷移除）', nextState.rows === qBefore.length, {
      rows: nextState.rows, before: qBefore.length,
    });
    await shot(page, '8c-next-in-queue');

    hr('⑧-d ?task= refresh 回得來 ／ Esc 關得掉');
    await page.reload({ waitUntil: 'networkidle2' });
    await sleep(3500);
    const afterReload = await page.evaluate(() => ({
      drawerOpen: !!document.querySelector('[data-testid="mywork-drawer"]'),
      hash: window.location.hash,
    }));
    check('refresh 之後抽屜還開在同一筆',
      afterReload.drawerOpen === true && /[?&]task=/.test(afterReload.hash), afterReload);
    await page.keyboard.press('Escape');
    await waitIdle(page);
    const afterEsc = await page.evaluate(() => ({
      drawerOpen: !!document.querySelector('[data-testid="mywork-drawer"]'),
      hash: window.location.hash,
    }));
    check('Esc 關得掉抽屜，網址也清掉 ?task=',
      afterEsc.drawerOpen === false && !/[?&]task=/.test(afterEsc.hash), afterEsc);

    hr('⑧-e 按重新整理，已處理的才真的消失');
    const beforeRefresh = await page.$$eval('[data-testid="workbench-row"]', (e) => e.length);
    await page.evaluate(() => {
      const scope = document.querySelector('main') || document.body;
      const b = Array.from(scope.querySelectorAll('button'))
        .filter((x) => !x.closest('header'))
        .find((x) => x.querySelector('svg[data-testid="RefreshIcon"]'));
      (b as HTMLElement | undefined)?.click();
    });
    await waitIdle(page);
    const afterRefresh = await page.evaluate((done: string | null) => ({
      rows: document.querySelectorAll('[data-testid="workbench-row"]').length,
      handled: document.querySelectorAll('[data-testid="row-handled"]').length,
      // 那一筆**已完成的 task** 還在不在列表上
      doneStillThere: !!document.querySelector(`[data-testid="workbench-row"][data-task="${done}"]`),
      ids: Array.from(document.querySelectorAll('[data-testid="workbench-row"]'))
        .map((e) => (e as HTMLElement).dataset.task ?? ''),
    }), firstTask);
    log(`  重新整理：${beforeRefresh} → ${JSON.stringify(afterRefresh)}`);
    /**
     * **不可以用「列數變少」當判準**（第一版就是這樣寫的，紅了，而那個紅燈是假的）。
     *
     * 這條流程裡「承辦確認」核准之後會推進到「主管核准」，
     * 而 `manager@` 正是下一關的候選人（R1 ＝ 發起人的部門主管）——
     * **那張單回到同一個人手上**，只是換了一個 task。列數當然不會變少。
     *
     * 要問的其實是：**剛剛處理掉的那一個 task 還在不在**。
     */
    check('重新整理之後「已處理」的標記清掉了', afterRefresh.handled === 0, afterRefresh);
    check('而且剛剛處理掉的那一筆 task 不在列表上了（重排是使用者要求的，不是動作的副作用）',
      afterRefresh.doneStillThere === false, { firstTask, ...afterRefresh });
    page.off('request', onReq);

    hr('⑧-f 「我參與過的」也是同一個抽屜');
    await goto(page, 'energyMyWorkConsole?tab=mine', '[data-testid="exec-row"], .MuiAlert-root');
    await waitIdle(page);
    const mineDrawer = await page.evaluate(async () => {
      const wait = (ms: number) => new Promise((r) => { setTimeout(r, ms); });
      const row = document.querySelector('[data-testid="exec-row"]') as HTMLElement | null;
      if (!row) return { note: 'no row' };
      row.click(); await wait(3000);
      return {
        drawerOpen: !!document.querySelector('[data-testid="mywork-drawer"]'),
        hash: window.location.hash,
        hasDetail: !!document.querySelector('[data-testid="mywork-drawer"] [data-testid="review-field"]'),
      };
    });
    log(`  我參與過的：${JSON.stringify(mineDrawer)}`);
    check('「我參與過的」點列也開同一個抽屜，不換頁',
      (mineDrawer as any).drawerOpen === true
      && /energyMyWorkConsole/.test((mineDrawer as any).hash ?? ''), mineDrawer);
    check('而且是同一個單據詳情元件', (mineDrawer as any).hasDetail === true, mineDrawer);
    await shot(page, '8f-mine-drawer');

    hr('⑧-g 退回的理由對話框關掉之後，回到抽屜不是回到列表');
    /**
     * **這一條沒得商量。**
     *
     * 整輪的核心是「不要把使用者踢出佇列」。如果理由對話框關掉之後落回列表，
     * 等於在剛修好的東西內部又挖了同一個洞 ——
     * 而對話框疊在抽屜之上正是 focus 與 z-index 出問題的地方，
     * 那類 bug **不報錯，只讓人覺得怪怪的**。
     *
     * 「退回」只存在於**核可**關卡（`clerk-review` 只有送出／申請中斷），
     * 所以要用前面那一筆推進之後產生的 `manager-approve`。
     */
    await goto(page, 'energyMyWorkConsole', '[data-testid="workbench-row"], .MuiAlert-root');
    await waitIdle(page);
    const sendBack = await page.evaluate(async () => {
      const wait = (ms: number) => new Promise((r) => { setTimeout(r, ms); });
      const row = Array.from(document.querySelectorAll('[data-testid="workbench-row"]'))
        .find((r) => (r as HTMLElement).dataset.node === 'manager-approve') as HTMLElement | undefined;
      if (!row) return { note: '沒有停在主管核准的列' };
      row.click(); await wait(3000);
      const claim = document.querySelector('[data-testid="queue-claim"]') as HTMLElement | null;
      if (claim) { claim.click(); await wait(3500); }
      const taskBefore = new URL(window.location.hash.slice(1), 'http://x').searchParams.get('task');
      // 找需要理由的那一顆（退回 ＝ kind revert，走 queue-act-other）
      const back = document.querySelector('[data-testid="queue-act-other"][data-action="sendBack"]') as HTMLElement | null;
      if (!back) {
        return {
          note: '這一列沒有退回動作',
          taskBefore,
          actions: Array.from(document.querySelectorAll('[data-testid^="queue-act"]'))
            .map((b) => (b as HTMLElement).dataset.action),
        };
      }
      back.click(); await wait(2000);
      const dialogOpen = !!document.querySelector('.MuiDialog-root');
      // 關掉對話框（取消）
      const cancel = Array.from(document.querySelectorAll('.MuiDialogActions-root button'))
        .find((x) => (x as HTMLElement).innerText.includes('取消')) as HTMLElement | undefined;
      cancel?.click(); await wait(2000);
      return {
        taskBefore,
        dialogOpen,
        dialogClosed: !document.querySelector('.MuiDialog-root'),
        drawerStillOpen: !!document.querySelector('[data-testid="mywork-drawer"]'),
        taskAfter: new URL(window.location.hash.slice(1), 'http://x').searchParams.get('task'),
      };
    });
    log(`  退回對話框：${JSON.stringify(sendBack)}`);
    check('找得到「退回」那一顆（不然下面兩條沒有意義）',
      !(sendBack as any).note && (sendBack as any).dialogOpen === true, sendBack);
    check('關掉理由對話框之後**抽屜還開著**（沒有被踢回列表）',
      (sendBack as any).drawerStillOpen === true, sendBack);
    check('而且還是同一筆（?task= 沒變）',
      !!(sendBack as any).taskAfter
      && (sendBack as any).taskAfter === (sendBack as any).taskBefore, sendBack);
    await shot(page, '8g-reason-dialog-back-to-drawer');

    hr('⑧-h 一直按到沒有下一筆 → 抽屜關閉並說一聲');
    /**
     * 我先前把這一條記成「只有兩筆走不到那個分支」——**那是錯的推測**：
     * 兩筆完全到得了（處理第二筆時 `slice(2)` 就是空的）。
     * 真相是那條斷言我根本沒寫。所以這裡真的按到底，讓它自己講話。
     */
    await goto(page, 'energyMyWorkConsole', '[data-testid="workbench-row"], .MuiAlert-root');
    await waitIdle(page);
    const drained = await page.evaluate(async () => {
      const wait = (ms: number) => new Promise((r) => { setTimeout(r, ms); });
      const row = document.querySelector('[data-testid="workbench-row"]') as HTMLElement | null;
      if (!row) return { note: '列表是空的' };
      row.click(); await wait(3000);
      let rounds = 0;
      // 最多按 8 次 —— 迴圈要有上限，不然一個壞掉的終點條件會變成無窮迴圈。
      for (let i = 0; i < 8; i += 1) {
        if (!document.querySelector('[data-testid="mywork-drawer"]')) break;
        const claim = document.querySelector('[data-testid="queue-claim"]') as HTMLElement | null;
        if (claim) { claim.click(); await wait(3500); }
        const next = document.querySelector('[data-testid="queue-act-next"]') as HTMLElement | null;
        if (!next) break;
        next.click(); rounds += 1; await wait(4000);
      }
      const bar = document.querySelector('.MuiSnackbar-root') as HTMLElement | null;
      return {
        rounds,
        drawerOpen: !!document.querySelector('[data-testid="mywork-drawer"]'),
        snackbar: bar?.innerText.replace(/\s+/g, ' ').trim() ?? '',
        hash: window.location.hash,
      };
    });
    log(`  清空佇列：${JSON.stringify(drained)}`);
    check('真的按了好幾輪（不然「到底了」是廢話）',
      ((drained as any).rounds ?? 0) >= 2, drained);
    check('到底了 → 抽屜關閉', (drained as any).drawerOpen === false, drained);
    check('而且說了一聲「這一區處理完了」',
      /處理完了/.test((drained as any).snackbar ?? ''), drained);
    check('網址也清掉 ?task=', !/[?&]task=/.test((drained as any).hash ?? ''), drained);
    await shot(page, '8h-queue-drained');

    hr('伺服器');
    check('沒有 5xx', serverErrors.length === 0, [...new Set(serverErrors)]);
  } finally {
    await browser.close();
  }
  board.finish('LIFECYCLE 【9】§8 佇列動線');
}

run().catch((e) => { console.error(e); process.exitCode = 1; });
