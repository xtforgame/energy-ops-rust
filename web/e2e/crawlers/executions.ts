/* eslint-disable no-console, no-await-in-loop */
/**
 * **P12 流程單（執行）** —— 從 `crawler-energy-ops-roles` 的 ⑯ 拆出來。
 *
 * ### 為什麼要拆
 *
 * 同 `crawler-energy-ops-binding`：原本這一段排在一支 12 分鐘回歸的**最後面**，
 * 而那支兩次都在跑到它之前就被瀏覽器崩掉了（101/131、0 failed）。
 * 報表上「沒跑到」跟「跑過而且綠」長得一模一樣 ——
 * 所以這一段其實**從來沒有被驗證過**。
 *
 * ### 驗的是什麼
 *
 * 使用者實測時說的是：「**我現在也看不到哪一頁在顯示所有 execution**」。
 * 他是對的 —— 舊的 P2 列的是 `energyBill`，而**保證金請款根本沒有 bill**。
 * 我們做了第二條流程，然後把它做成看不見的。
 *
 * 所以 P12「流程單」與 P2「電費單」是**兩個資源**，不是同一頁的兩種篩選：
 *
 * · P12 列的是**執行**（含沒有 bill 的保證金請款）
 * · P2 列的是**電費單**（含還沒送審、因此沒有執行的那些）
 *
 * 兩邊都要能反過來找到對方，而且四個落點都要**帶著篩選**過去 ——
 * 數字點下去卻只跳到一個沒篩選的清單，等於沒有落點。
 *
 * ### 這一支怎麼跑
 *
 * 移植自 az `scripts/crawler-energy-ops-executions`：`web/e2e/run-crawlers.sh executions`
 * （示範資料、不灌範本電費單）。
 *
 * **可重複跑**：自己上傳一張**帶時間戳、且刻意不送出**的單來製造
 * 「尚未送審」的狀態（檔名決定業務身分，換一次跑就是另一張紙），
 * 斷言全部是相對值（「多一筆」「兩邊各自有」），不看全域絕對數字。
 */
import type { Page } from 'puppeteer';
import {
  Scoreboard, hr, log, sleep, launch, shooter, loginAs, loginAsAdmin, selectOrg, goto,
  seedBatch, hardenPage,
  waitIdle,
  eapi, demoEmail,
} from '../lib/energyOpsHarness.ts';

const board = new Scoreboard();
const check = board.check.bind(board);
const shot = shooter('energy-executions');

/** 上傳一批 → 回傳那一批產生的電費單 id（0 ＝ 沒產生）。 */
async function uploadAndGetBill(page: Page, file: string): Promise<number> {
  const batch = await seedBatch(page, [file]);
  if (!batch.ok) return 0;
  const r = await eapi(page, 'GET', `/ingest-batches/${batch.batchId}`);
  if (r.status !== 200) return 0;
  const item = (r.data?.items ?? []).find((i: any) => i.billId != null);
  return item ? Number(item.billId) : 0;
}

async function run() {
  const browser = await launch();
  // `hardenPage`：把 `page.evaluate` 包上 detach 復原（見 harness 裡的說明）。
  const page = hardenPage(await browser.newPage());
  const serverErrors: string[] = [];
  page.on('response', (r) => {
    if (r.status() >= 500) serverErrors.push(`${r.status()} ${r.url()}`);
  });
  /**
   * **檔名就是這張紙的身分**（`ingestFixtures.periodForName`）——
   * 帶時間戳才不會第二次跑就撞上自己上一輪留下的單。
   * `-small` 前綴要留著：fixture 靠它決定金額（60,000 < 加簽門檻）。
   */
  const stamp = String(process.pid);
  const runFile = `bill-small-run-${stamp}.png`;     // 送審 → 產生一筆**進行中**的執行
  const orphanFile = `bill-small-orphan-${stamp}.png`; // 刻意不送 → 「校對完卻忘了送」那一張

  try {
    // ── ⑯ P2 流程單清單（【0.5】重定位）—— 使用者實測撞到的第二個缺口 ──
    //
    // 使用者實測：「**我現在也看不到哪一頁在顯示所有 execution**」。
    // 他是對的：舊 P2 列的是 `energyBill`，而 **保證金請款根本沒有 bill**
    // —— 我們做了第二個流程，然後把它做成看不見的。
    /**
     * ### ⓪ 這一支**自己把要驗的狀態做出來**
     *
     * 拆出來之前它排在全套回歸的最後面，靠前面十四段留下的殘留狀態過活 ——
     * 「進行中有幾列」「有沒有被修正過的單」全都不是它自己造的。
     * 第一次真的單獨跑的時候，九條斷言一起紅，而它們驗的東西其實沒壞。
     *
     * **靠別人留下的狀態＝沒有辦法單獨跑＝實際上沒有守衛。**
     *
     * 三個狀態都用**真實 UI 操作**做（不塞 seed）：
     *
     * | 狀態 | 怎麼做 | 為什麼需要 |
     * |---|---|---|
     * | 進行中的執行 | 上傳 → 開單 → 按送審 | P12「進行中」那一格要有列可以點 |
     * | 尚未送審的單 | 上傳 → **不按送出** | 那是電費單的狀態，家在 P2 |
     * | 被人工修正過 | 改金額 → 存檔 | `changedFields` → P2 上的「已修正」標記 |
     *
     * 收件台**上傳就會產生 `energyBill`**，`startExecution` 要到**送出**才發生
     * （`IngestService`）—— 所以「上傳但不按送出」正好就是第二個狀態。
     */
    hr('⓪ 先把要驗的三個狀態做出來（真實 UI 操作）');
    await loginAs(page, demoEmail('operator'));
    await selectOrg(page);

    const runBillId = await uploadAndGetBill(page, runFile);
    check('上傳產生得出電費單（要送審的那張）', runBillId > 0, { runBillId });
    await goto(page, `energyBillConsole/${runBillId}/show`, '[data-testid="review-field"], table');
    await waitIdle(page);
    await page.evaluate(() => {
      (document.querySelector('[data-testid="review-submit"]') as HTMLElement | null)?.click();
    });
    await waitIdle(page);
    const started = await (async () => {
      const r = await eapi(page, 'GET', `/bills/${runBillId}`);
      if (r.status !== 200) return { status: r.status };
      const b = r.data;
      return { execs: (b.executions ?? []).length, state: b.executions?.[0]?.status };
    })();
    log(`  送審結果：${JSON.stringify(started)}`);
    /**
     * 這一條是**探針自檢**，不是產品斷言。
     * 沒有進行中的執行的話，下面每一條「點一列」都會拿到 `no row` ——
     * 而那長得跟「詳情頁壞了」一模一樣。先在這裡叫，省下一輪誤判。
     */
    check('送審真的起了一筆執行（下面每一條都靠它）',
      ((started as any)?.execs ?? 0) > 0 && (started as any)?.state === 'running', started);

    const orphanBillId = await uploadAndGetBill(page, orphanFile);
    check('上傳產生得出電費單（刻意不送的那張）',
      orphanBillId > 0 && orphanBillId !== runBillId, { runBillId, orphanBillId });
    // **刻意不按送出** —— 這一張就是「校對完卻忘了送」的那種單。
    // 順手把金額改掉：`changedFields` 一有值，P2 上就該出現「已修正」。
    await goto(page, `energyBillConsole/${orphanBillId}/show`, '[data-testid="review-field"], table');
    await waitIdle(page);
    const corrected = await page.evaluate(async () => {
      const wait = (ms: number) => new Promise((res) => { setTimeout(res, ms); });
      const el = document.querySelector('[data-testid="final-totalAmount"]') as HTMLInputElement | null;
      if (!el || el.disabled) return { note: 'field not editable' };
      const before = el.value;
      const setter = Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, 'value')!.set!;
      setter.call(el, '61234');
      el.dispatchEvent(new Event('input', { bubbles: true }));
      await wait(800);
      (document.querySelector('[data-testid="review-save"]') as HTMLElement | null)?.click();
      await wait(3500);
      return { before, after: '61234' };
    });
    log(`  人工修正：${JSON.stringify(corrected)}`);
    check('改得動金額（intake 模式）', !(corrected as any)?.note, corrected);
    await waitIdle(page);

    hr('① P12 流程單（執行）—— 與 P2 電費單是兩個資源');

    await loginAsAdmin(page);
    await selectOrg(page);
    await goto(page, 'energyExecutionConsole', '[data-testid="exec-tabs"]');
    await waitIdle(page);

    const p2 = await page.evaluate(async () => {
      const wait = (ms: number) => new Promise((res) => { setTimeout(res, ms); });
      const rowsOf = () => Array.from(document.querySelectorAll('[data-testid="exec-row"]'))
        .map((r) => ({
          kind: r.getAttribute('data-kind'),
          waiting: Number(r.getAttribute('data-waiting') ?? ''),
          // 指名取這一欄 —— 不要從 innerText 猜（見下面那條斷言的教訓）。
          stationName: r.getAttribute('data-station-name') ?? '',
          text: (r as HTMLElement).innerText.replace(/\n/g, ' | '),
        }));
      const out: any = { running: rowsOf() };

      /**
       * 保證金那一列在不在（`data-kind` 是後端給的 subject 種類）。
       *
       * **要看兩個頁籤** —— 保證金是三節點流程（送審 → 財務放行 → 結束），
       * 而前面 ⑤ 財務那一段會在工作台上把它放行掉 → 它會**變成「已完成」**。
       * 只查「進行中」會紅，但那**不是缺陷**，只是它已經跑完了。
       * 這條要問的是「保證金那一類看不看得見」，不是「它在哪一格」。
       */
      const completedTab = document.querySelector('[data-testid="exec-tab-completed"]') as HTMLElement | null;
      if (completedTab) { completedTab.click(); await wait(3000); }
      out.completed = rowsOf();
      // 切回進行中（下面排序那段要在同一格上做）。
      (document.querySelector('[data-testid="exec-tab-running"]') as HTMLElement | null)?.click();
      await wait(3000);
      out.running = rowsOf();

      const both = [...out.running, ...out.completed];
      out.hasDeposit = both.some((r: any) => r.kind === 'energyInstallment');
      // 保證金那列的**案場欄不可以是空的** ——
      // `stationId` 有值但 `stationName` 沒查到的話那格是 `—`，而且不會報錯。
      out.depositStation = both.find((r: any) => r.kind === 'energyInstallment')?.stationName ?? '';
      out.hasBill = both.some((r: any) => r.kind === 'energyBill');
      // 畫面上不可以出現 raw key。
      out.rawKeyLeak = both.some((r: any) => /deposit-request|bill-approval|energy:/.test(r.text));

      /**
       * 「等了幾天」那一欄可排序。
       *
       * **不要斷言「順序有沒有變」** —— demo 資料是同一天灌進來的，
       * 每一列的 `waitingDays` 都是 0，**順序沒變才是對的**。
       * （第一版就是這樣寫的，紅了，而那個紅燈是**假的**。）
       * 改成看**數列本身**：降冪要非遞增、升冪要非遞減 —— 有平手也成立。
       */
      const sortBtn = document.querySelector('[data-testid="exec-sort-waiting"]') as HTMLElement | null;
      out.hasSort = !!sortBtn;
      out.descSeq = out.running.map((r: any) => r.waiting);
      if (sortBtn) { sortBtn.click(); await wait(2500); }
      out.ascSeq = rowsOf().map((r: any) => r.waiting);
      const monotonic = (a: number[], dir: 'asc' | 'desc') => a.every(
        (v, i) => i === 0 || (dir === 'asc' ? a[i - 1] <= v : a[i - 1] >= v),
      );
      out.sortOk = monotonic(out.descSeq, 'desc') && monotonic(out.ascSeq, 'asc');

      // 這一頁**不可以有**「尚未送審」頁籤 ——
      // 那等於在 execution 清單上列出**不存在的 execution**。
      // 它是電費單的狀態，家在 P2（下面 ⑯-d 驗）。
      out.hasUnsubmittedTab = !!document.querySelector('[data-testid="exec-tab-unsubmitted"]');
      return out;
    });
    log(`  P12：${JSON.stringify(p2).slice(0, 900)}`);

    // 本節最重要的一條 —— 實作退回「查 bills 再 join」時它會紅。
    check('保證金請款（沒有 bill 的那一類）出現在**流程單**頁',
      (p2 as any)?.hasDeposit === true,
      { running: ((p2 as any)?.running ?? []).map((r: any) => r.kind),
        completed: ((p2 as any)?.completed ?? []).map((r: any) => r.kind) });
    check('電費單與保證金在**同一張表**上（「類別」是一欄，不是兩頁）',
      (p2 as any)?.hasBill === true && (p2 as any)?.hasDeposit === true);
    /**
     * 保證金那列要看得出**案場**（多型只做一半的第四個現場）。
     *
     * **第一版這條是假紅燈**：我去掃整列 innerText 找 `| — |`，
     * 結果匹配到的是「**等了幾天**」那一欄 —— 已完成的單沒有等待天數，
     * 那格本來就是 `—`。產品是對的，錯的是斷言。
     * 改成**指名讀那一欄**（`data-station-name`）—— 這是第三次犯同一類錯了。
     */
    check('保證金那列的案場欄不是空的',
      ((p2 as any)?.depositStation ?? '').length > 0, (p2 as any)?.depositStation);
    check('畫面上不出現 raw key（deposit-request / bill-approval / energy:）',
      (p2 as any)?.rawKeyLeak === false,
      ((p2 as any)?.running ?? []).map((r: any) => r.text));
    check('「等了幾天」可排序（降冪非遞增、升冪非遞減 —— 不是「順序有變」）',
      (p2 as any)?.hasSort === true && (p2 as any)?.sortOk === true, {
        hasSort: (p2 as any)?.hasSort,
        desc: (p2 as any)?.descSeq, asc: (p2 as any)?.ascSeq,
      });
    // **反向斷言**：流程單頁**不可以有**「尚未送審」頁籤。
    // 需要「這東西不存在」的頁籤，代表那個清單的主體選錯了。
    check('流程單頁**沒有**「尚未送審」頁籤（那是電費單的狀態）',
      (p2 as any)?.hasUnsubmittedTab === false, (p2 as any)?.hasUnsubmittedTab);
    await shot(page, '16-p12-executions');

    // ── ⑯-a2 點一列要去 **execution 自己的家** ────────────────
    //
    // 使用者實測抓到的：「`energyExecutionConsole` 裡面卻是連到
    // `energyBillConsole/1/show`？」—— 他是對的。
    // 當時 P12 **只有清單沒有詳情**，我就讓後端替每列算一個 `subject.route`，
    // 整列點下去跳到**電費單校對台**；而保證金那類被導去**地主頁**。
    // 那不是「連結」，那是**把 P12 併進 P2**（`FRONTEND.md §1.4.5` 禁止）。
    const detail = await page.evaluate(async () => {
      const wait = (ms: number) => new Promise((res) => { setTimeout(res, ms); });
      const row = document.querySelector('[data-testid="exec-row"]') as HTMLElement | null;
      if (!row) return { note: 'no row' };
      row.click(); await wait(3500);
      return {
        hash: location.hash,
        title: !!document.querySelector('[data-testid="exec-detail-title"]'),
        head: !!document.querySelector('[data-testid="exec-detail-head"]'),
        tasks: document.querySelectorAll('[data-testid="exec-task-row"]').length,
        // 「它的對象」是**一個具名連結**，不是整列變成對象的入口。
        subjectLink: !!document.querySelector('[data-testid="exec-open-subject"]'),
      };
    });
    log(`  P12 詳情：${JSON.stringify(detail)}`);
    check('點一列去 **execution 自己的詳情**（不是跳到電費單校對台）',
      /energyExecutionConsole\/\d+\/show/.test((detail as any)?.hash ?? ''), (detail as any)?.hash);
    check('詳情頁看得到關卡歷程', ((detail as any)?.tasks ?? 0) > 0, detail);
    check('「它的對象」是一個具名連結（`§1.4.5`：連結，不是合併）',
      (detail as any)?.subjectLink === true, detail);
    await shot(page, '16a2-p12-detail');

    // ── ⑯-a3 **保證金那類的待辦點得動**（多型不可以只做一半）────────
    //
    // 前一版四個地方都寫 `subject.billId ? navigate(bill) : undefined` ——
    // 保證金沒有 bill → **點了什麼都沒發生，也沒有任何提示**。
    // 症狀跟系統壞掉一模一樣（按鈕在、沒反應、沒有錯誤）。
    /**
     * **先回清單** —— 上一段點進了詳情頁，人已經不在清單上了。
     *
     * 這裡踩過一個很會騙人的坑：原本是在 `evaluate` 裡直接改
     * `location.hash = '…?status=completed'`。頁籤**不是**看那個 query 決定的，
     * 所以切不過去，回報是 `no deposit row` ——
     * 而「探針停在錯的頁面」跟「保證金那類真的不見了」長得一模一樣。
     */
    await goto(page, 'energyExecutionConsole', '[data-testid="exec-tabs"]');
    await waitIdle(page);
    const depositNav = await page.evaluate(async () => {
      const wait = (ms: number) => new Promise((res) => { setTimeout(res, ms); });
      (document.querySelector('[data-testid="exec-tab-completed"]') as HTMLElement | null)?.click();
      await wait(3500);
      const rows = Array.from(document.querySelectorAll('[data-testid="exec-row"]'));
      const dep = rows.find((r) => r.getAttribute('data-kind') === 'energyInstallment');
      if (!dep) return { note: 'no deposit row' };
      (dep as HTMLElement).click(); await wait(3500);
      const before = location.hash;
      // 詳情頁上的「它的對象」按鈕 —— 保證金要連得到（合約與期數）。
      const sub = document.querySelector('[data-testid="exec-open-subject"]') as HTMLElement | null;
      if (!sub) return { detail: before, subjectBtn: false };
      sub.click(); await wait(3000);
      return { detail: before, subjectBtn: true, after: location.hash };
    });
    log(`  保證金導覽：${JSON.stringify(depositNav)}`);
    check('保證金的流程單點得進詳情（不是點了沒反應）',
      /energyExecutionConsole\/\d+\/show/.test((depositNav as any)?.detail ?? ''), depositNav);
    check('保證金詳情有「它的對象」連結，且**真的跳得過去**',
      (depositNav as any)?.subjectBtn === true
      && /energyLandowners\/\d+/.test((depositNav as any)?.after ?? ''), depositNav);
    await shot(page, '16a3-deposit-nav');

    // ── ⑯-b 可視度：同一頁，不同人不同量 ────────────────────────
    //
    // 可視度由**後端**過濾（`WORKFLOW.md §7.3`）——前端不做第二套。
    /**
     * **挑對比較的人**（第一版挑錯了，紅得莫名其妙）。
     *
     * 不能拿 `operator@` 比：**這些單全部是他送的** ——
     * 發起人本來就是參與者，所以他看到的跟 admin 一樣多。
     * 那個紅燈不是缺陷，是我挑錯了對照組。
     *
     * 用 `accounting@`（`energy:finance` 角色 ＝ participate，**沒有 view-all**）：
     * 他只在**付款關**的候選池裡，所以停在「承辦確認／財務放行」的單他看不到。
     * 而且要順帶證明**不是「他什麼都看不到」** ——
     * 他經手過的那張（已完成）必須看得到，否則這條就只是在驗一個壞掉的頁。
     */
    const countFor = async (who: string, tab: 'running' | 'completed') => {
      await loginAs(page, who);
      await selectOrg(page);
      await goto(page, 'energyExecutionConsole', '[data-testid="exec-tabs"]');
      await waitIdle(page);
      return page.evaluate(async (t) => {
        const wait = (ms: number) => new Promise((res) => { setTimeout(res, ms); });
        (document.querySelector(`[data-testid="exec-tab-${t}"]`) as HTMLElement | null)?.click();
        await wait(3000);
        return document.querySelectorAll('[data-testid="exec-row"]').length;
      }, tab);
    };
    const adminCount = (p2 as any)?.running?.length ?? 0;
    const acctRunning = await countFor(demoEmail('accounting'), 'running');
    const acctCompleted = await countFor(demoEmail('accounting'), 'completed');
    log(`  可視度：admin(進行中)=${adminCount} / accounting@(進行中)=${acctRunning}`
      + ` / accounting@(已完成)=${acctCompleted}`);
    check('沒有 view-all 的人看到的比較少（前端不做第二套過濾）',
      acctRunning < adminCount, { adminCount, acctRunning });
    check('而且不是「他什麼都看不到」—— 他經手過的那張看得到',
      acctCompleted > 0, { acctCompleted });
    await shot(page, '16b-p2-visibility');

    // ── ⑯-c 四個落點：連結要**帶著篩選**過去 ──────────────────────
    //
    // 導到一張沒篩選的清單，那個數字就等於一張海報 ——
    // 使用者看到「3 件卡關」卻得自己回選單再篩一次。
    hr('② 四個落點都帶著篩選');
    await loginAsAdmin(page);
    await selectOrg(page);
    const p2Landing = await page.evaluate(async () => {
      const wait = (ms: number) => new Promise((res) => { setTimeout(res, ms); });
      const out: any = {};
      // ① Dashboard「幾件卡關」→ `?status=running&waitingDaysMin=N`
      location.hash = '/energyDashboardConsole'; await wait(3500);
      const kpis = Array.from(document.querySelectorAll('[data-testid="kpi-card"]'));
      const stuck = kpis.find((k) => /卡關|Stuck/i.test((k as HTMLElement).innerText));
      if (stuck) { (stuck as HTMLElement).click(); await wait(3000); }
      out.stuck = location.hash;

      // ② P11 案場「看全部」→ `?stationId=`
      location.hash = '/energyLandownerConsole'; await wait(3000);
      (document.querySelector('[data-testid="landowner-row"]') as HTMLElement)?.click();
      await wait(3000);
      (document.querySelector('[data-testid="landowner-open-station"]') as HTMLElement)?.click();
      await wait(3000);
      (document.querySelector('[data-testid="station-all-executions"]') as HTMLElement)?.click();
      await wait(2500);
      out.station = location.hash;

      // ③ P5 地主「看他的所有單」→ `?landownerId=`
      location.hash = '/energyLandownerConsole'; await wait(3000);
      (document.querySelector('[data-testid="landowner-row"]') as HTMLElement)?.click();
      await wait(3000);
      (document.querySelector('[data-testid="landowner-all-bills"]') as HTMLElement)?.click();
      await wait(2500);
      out.landowner = location.hash;
      out.landownerChip = !!document.querySelector('[data-testid="exec-landowner-filter"]');

      // ④ P9 節點 drawer「看全部」→ `?workflowKey=&nodeKey=`
      location.hash = '/energyWorkflowConsole'; await wait(3500);
      // 先選一個**有單在跑**的版本 —— 在「進行中 = 0」那張圖上點節點
      // drawer 永遠是空的，「看全部」按鈕當然也就驗不到。
      const wfRows = Array.from(document.querySelectorAll('[data-testid="workflow-row"]'));
      const busyRow = wfRows.find((r) => Number((r.querySelector('[data-testid="workflow-running"]') as HTMLElement)
        ?.innerText.trim() ?? 0) > 0) ?? wfRows[0];
      (busyRow as HTMLElement | null)?.click();
      await wait(3000);
      // 點的是 `.react-flow__node`（外層），不是裡面那個 span ——
      // react-flow 的點擊處理掛在外層，點 span 不會觸發 `onNodeClick`。
      const hot = document.querySelector('[data-testid="flow-node"]');
      (hot?.closest('.react-flow__node') as HTMLElement | null)?.click();
      await wait(3000);
      out.drawerOpened = !!document.querySelector('[data-testid="node-drawer"]');
      (document.querySelector('[data-testid="node-drawer-see-all"]') as HTMLElement)?.click();
      await wait(2500);
      out.node = location.hash;
      out.nodeChip = !!document.querySelector('[data-testid="exec-node-filter"]');
      return out;
    });
    log(`  落點：${JSON.stringify(p2Landing)}`);
    check('Dashboard「幾件卡關」→ P2 且帶 waitingDaysMin',
      /energyExecutionConsole\?.*waitingDaysMin=\d+/.test((p2Landing as any)?.stuck ?? ''), (p2Landing as any)?.stuck);
    check('P11 案場「看全部」→ P2 且帶 stationId',
      /energyExecutionConsole\?.*stationId=\d+/.test((p2Landing as any)?.station ?? ''), (p2Landing as any)?.station);
    check('P5 地主「看他的所有單」→ P2 且帶 landownerId（不是沒篩選的清單）',
      /energyExecutionConsole\?.*landownerId=\d+/.test((p2Landing as any)?.landowner ?? ''), (p2Landing as any)?.landowner);
    check('P9 節點 drawer「看全部」→ P2 且帶 workflowKey ＋ nodeKey',
      /energyExecutionConsole\?.*workflowKey=[^&]+.*nodeKey=[^&]+/.test((p2Landing as any)?.node ?? ''), (p2Landing as any)?.node);
    // 帶進來的篩選要**看得見** —— 不然使用者以為清單就這麼少。
    check('帶進來的篩選在畫面上看得見（chip）',
      (p2Landing as any)?.landownerChip === true || (p2Landing as any)?.nodeChip === true, p2Landing);
    await shot(page, '16c-p12-landings');

    // ── ⑯-d P2 電費單 —— 它就只是電費單 ────────────────────
    //
    // 資源分層（`FRONTEND.md §1.4`）：把「雲豹能源」換成任何一家公司，
    // 這張表**不在** —— 電號、計費期間、發電度數只有雲豹需要。
    // 所以這一頁**不可以**有關卡、等待天數、流程版本，
    // 也**不可以**有保證金請款（它根本不是電費單）。
    hr('③ P2 電費單（就只是電費單）');
    await goto(page, 'energyBillConsole', '[data-testid="bill-row"], table');
    await waitIdle(page);
    const p2bills = await page.evaluate(async () => {
      const wait = (ms: number) => new Promise((res) => { setTimeout(res, ms); });
      const rowsOf = () => Array.from(document.querySelectorAll('[data-testid="bill-row"]'))
        .map((r) => ({
          submitted: r.getAttribute('data-submitted'),
          text: (r as HTMLElement).innerText.replace(/\n/g, ' | '),
        }));
      const out: any = { rows: rowsOf() };
      out.corrected = document.querySelectorAll('[data-testid="bill-corrected"]').length;
      out.unsubmitted = document.querySelectorAll('[data-testid="bill-unsubmitted"]').length;
      out.openExecution = document.querySelectorAll('[data-testid="bill-open-execution"]').length;
      // 這一頁不可以出現工作流語彙。
      out.flowLeak = out.rows.some((r: any) => /等了|目前關卡|保證金/.test(r.text));

      /**
       * 「尚未送審」那一張要**按得下去**，而且按完之後它就有流程單了。
       *
       * **不可以只按第一顆就下結論**：清單上可能有「同一張紙的第二份」
       * （驗收自己製造的重複），那一列按下去會被去重**正確地**擋成 409。
       * 只按第一顆的話，一個正常運作的系統會被判成壞的。
       * 這裡逐顆試，直到有一張真的送出去為止。
       */
      const before = document.querySelectorAll('[data-testid="bill-unsubmitted"]').length;
      out.hasSubmitBtn = before > 0
        && !!document.querySelector('[data-testid="bill-submit"]');
      out.tried = 0;
      for (let i = 0; i < 6; i += 1) {
        const btns = Array.from(document.querySelectorAll('[data-testid="bill-submit"]'));
        if (!btns[i]) break;
        out.tried += 1;
        (btns[i] as HTMLElement).click();
        await wait(5000);
        if (document.querySelectorAll('[data-testid="bill-unsubmitted"]').length < before) break;
      }
      out.afterUnsubmitted = document.querySelectorAll('[data-testid="bill-unsubmitted"]').length;
      out.afterOpenExecution = document.querySelectorAll('[data-testid="bill-open-execution"]').length;
      return out;
    });
    log(`  P2 電費單：${JSON.stringify(p2bills).slice(0, 900)}`);

    /**
     * **相對值**：這一支自己上傳了兩張，所以清單至少要有兩張。
     * 原本寫的是「≥ 4（seed 的 4 張）」—— 那是**別人**留下的資料，
     * seed 帶 `ENERGY_DEMO_NO_BILLS=1` 跑的時候一張都不會有。
     * 斷言只該看自己造出來的東西。
     */
    check('電費單頁列得出這一支自己送進去的兩張',
      ((p2bills as any)?.rows ?? []).length >= 2, ((p2bills as any)?.rows ?? []).length);
    // 「金額被人工修正過」那張要有標記（辨識值 vs 最終值）。
    check('被人工修正過的單有標記', ((p2bills as any)?.corrected ?? 0) > 0, (p2bills as any)?.corrected);
    // 混用禁令：電費單頁不可以有關卡/等待天數，也不可以有保證金。
    check('電費單頁沒有工作流語彙（關卡／等了幾天／保證金）',
      (p2bills as any)?.flowLeak === false,
      ((p2bills as any)?.rows ?? []).map((r: any) => r.text).slice(0, 4));
    // 「尚未送審」的家在這裡，而且**可以直接送**。
    check('「尚未送審」標記在電費單頁，且有送審按鈕',
      ((p2bills as any)?.unsubmitted ?? 0) > 0 && (p2bills as any)?.hasSubmitBtn === true,
      { unsubmitted: (p2bills as any)?.unsubmitted, btn: (p2bills as any)?.hasSubmitBtn });
    check('送審之後那一列就有「流程單」連結了（不再是尚未送審）',
      ((p2bills as any)?.afterUnsubmitted ?? 99) < ((p2bills as any)?.unsubmitted ?? 0)
      && ((p2bills as any)?.afterOpenExecution ?? 0) > ((p2bills as any)?.openExecution ?? 0),
      {
        before: { un: (p2bills as any)?.unsubmitted, ex: (p2bills as any)?.openExecution },
        after: { un: (p2bills as any)?.afterUnsubmitted, ex: (p2bills as any)?.afterOpenExecution },
      });
    await shot(page, '16d-p2-bills');
    hr('伺服器');
    check('沒有 5xx', serverErrors.length === 0, [...new Set(serverErrors)]);
  } finally {
    await browser.close();
  }
  board.finish('P12 流程單');
}

run().catch((e) => { console.error(e); process.exitCode = 1; });
