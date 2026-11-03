/* eslint-disable no-console, no-await-in-loop */
/**
 * **LIFECYCLE 【5】的驗收：§7.3 我的工作 ＋ §7.4 例外件併回電費單。**
 *
 * ### 為什麼是一支「小」crawler
 *
 * `crawler-energy-ops-roles` 是全套回歸：2200 行、16 段、**跑一次 12 分鐘**，
 * 加上 `init:hasura` ＋ 重灌 seed 一輪 15 分鐘。
 * 拿它當「改一行 → 看對不對」的內圈是錯的 ——
 * 這一輪就有兩次 15 分鐘花在「crawler 自己有語法錯」與「頁面 render 就炸」上。
 *
 * > **不要用 12 分鐘的東西當內圈。**
 *
 * 這一支只驗這個切片改到的東西，**~2 分鐘**，而且：
 *
 * - **可重複跑，不必重灌 seed** —— 待指認的斷言一律用**相對量**
 *   （先讀 N，上傳 2 筆 → 期望 N+2，指認 1 筆 → 期望 N+1）。
 *   寫死「應該是 2」的話，第二次跑就紅，然後人會開始用「重灌一次再說」
 *   代替看懂它 —— 那是把驗收變成儀式。
 * - 全套回歸留在 `crawler-energy-ops-roles`（收尾跑一次就好）。
 *
 * ### 前置
 *
 * 移植自 az `scripts/crawler-energy-ops-mywork`。`web/e2e/run-crawlers.sh mywork`
 * （示範資料＋四份範本電費單）。
 *
 * seed 必須含「曾在候選池但沒認領」的那一筆（`EO_DEMO_SAMPLE_BILLS` 的含電表租費那張）——
 * 沒有它，「參與過」的定義就只是文件上的一句話。
 */
import type { Page } from 'puppeteer';
import {
  Scoreboard, hr, log, sleep, launch, shooter, loginAs, selectOrg, goto,
  seedBatch, hardenPage,
  waitIdle, eapi, demoEmail,
} from '../lib/energyOpsHarness.ts';

const board = new Scoreboard();
const check = board.check.bind(board);
const shot = shooter('energy-mywork');

/** 目前有幾筆待指認 —— 直接問後端（畫面上的數字要跟它一致，那本身就是一條驗收）。 */
async function unmatchedCount(page: Page): Promise<number> {
  const r = await eapi(page, 'GET', '/bills/unmatched');
  return r.status === 200 && Array.isArray(r.data?.items) ? r.data.items.length : -1;
}

async function run() {
  const browser = await launch();
  // `hardenPage`：把 `page.evaluate` 包上 detach 復原（見 harness 裡的說明）。
  const page = hardenPage(await browser.newPage());
  const serverErrors: string[] = [];
  page.on('response', (r) => {
    if (r.status() >= 500) serverErrors.push(`${r.status()} ${new URL(r.url()).pathname}`);
  });

  try {
    /**
     * 那一筆的**單號從 API 問**，不寫死。
     *
     * 它以前是 seed 手建的 `SEED-CAND-1`（一張沒有原始檔的假單）——
     * 使用者第一眼就問「這張怎麼沒有原始檔案」。現在情境跑在**真的範本單**上，
     * 單號是紙上印的那一個，所以驗收要問資料而不是背一個字串。
     *
     * **問的人是 `manager@`，不是 `operator@`**：
     *
     * - `operator@` 沒有 `view-all`，`scope=all` 對他**本來就回 403**
     *   （下面第 ④ 段就在驗這件事）—— 拿它當單號來源永遠問不到。
     * - 更重要的是**不能循環**：要驗的正是「`operator@` 看得到這一筆」，
     *   所以單號必須來自**別人的**視角。`manager@` 是認領並推完它的人，
     *   那一筆在他自己的 `scope=mine` 裡，與候選池那條邏輯完全無關。
     */
    const candidateDocNo: string = await (async () => {
      await loginAs(page, demoEmail('manager'));
      await selectOrg(page);
      // 選完組織畫面還在換 —— 這時 evaluate 會撞 `detached Frame`
      // （第一版就是這樣整支掛掉的，而畫面上看起來像「查不到單號」）。
      await waitIdle(page);
      const docNo = await (async () => {
        const r = await eapi(page, 'GET', '/executions?status=completed&scope=mine');
        if (r.status !== 200) return '';
        const list: any[] = r.data?.items ?? [];
        // 單號住在 `subject` 上（`ExecutionListService.decorate`）——
        // execution 自己不留副本，所以不要往頂層找。
        const hit = list.find((e: any) => e.subject?.kind === 'energyBill' && e.subject?.documentNo);
        return hit ? String(hit.subject.documentNo) : '';
      })();
      return docNo;
    })();

    // ── ① §7.3 我的工作：兩個頁籤 ────────────────────────────────
    hr('① §7.3 我的工作 —— 兩個頁籤，不是兩頁');
    await loginAs(page, demoEmail('operator'));
    check('選得到 demo 組織', (await selectOrg(page)) != null);

    /**
     * **承辦的落點是「送件」不是「我的工作」** —— 那是 C7a 就定下來的，
     * 而且有理由：防自核讓承辦在自己送出的單上**沒有任何待辦**，
     * 落在「等我處理」等於落在一張空白頁。
     * §7.3 的「登入落點是『等我處理』頁籤」講的是**落在這一頁的那些角色**
     * （財務／會計／部門主管）—— 下面用會計驗。
     */
    const opLanding = await (async () => {
      const r = await eapi(page, 'GET', '/me/landing');
      return r.status === 200 ? r.data : { error: r.status };
    })();
    check('承辦的落點是送件（C7a 的既有決定，沒被這一輪改掉）',
      opLanding?.route === 'energyIngestConsole', opLanding);

    check('進得了我的工作', await goto(page, 'energyMyWorkConsole', '[data-testid="mywork-tabs"]'));
    await waitIdle(page);
    const todo = await page.evaluate(() => ({
      active: (document.querySelector('[data-testid="mywork-tabs"]') as HTMLElement)?.dataset.active ?? '',
      sections: document.querySelectorAll('[data-testid="workbench-section"]').length,
      hasAlert: !!document.querySelector('.MuiAlert-root'),
      hash: window.location.hash,
      text: (document.querySelector('main') as HTMLElement)?.innerText ?? '',
    }));
    check('預設落在「等我處理」', todo.active === 'todo', todo);
    // 內容照舊：不是分區就是「沒事做」那一句 —— 兩個都沒有＝空白畫面。
    check('「等我處理」的內容與改版前一致（分區或「沒事做」）',
      todo.sections > 0 || todo.hasAlert, { sections: todo.sections, hasAlert: todo.hasAlert });
    check('預設頁籤不寫進網址（落點的網址要乾淨）', !/[?&]tab=/.test(todo.hash), todo.hash);
    check('畫面上沒有 raw key', !/energy:|ryxis:/.test(todo.text));
    await shot(page, '1-mywork-todo');

    // ── ② 我參與過的 ──────────────────────────────────────────
    hr('② 我參與過的 —— 含已完成 ＋ 候選池那一條');
    const mine = await page.evaluate(async () => {
      const wait = (ms: number) => new Promise((res) => { setTimeout(res, ms); });
      const tab = document.querySelector('[data-testid="mywork-tab-mine"]') as HTMLElement | null;
      if (!tab) return { ok: false };
      tab.click();
      await wait(3500);
      return {
        ok: true,
        active: (document.querySelector('[data-testid="mywork-tabs"]') as HTMLElement)?.dataset.active ?? '',
        hash: window.location.hash,
        scope: (document.querySelector('[data-testid="exec-scope-note"]') as HTMLElement)?.dataset.scope ?? '',
        // 那一格不該有「進行中／已完成」頁籤：它問的是「我碰過什麼」。
        hasStatusTabs: !!document.querySelector('[data-testid="exec-tabs"]'),
        rows: Array.from(document.querySelectorAll('[data-testid="exec-row"]')).map((r) => ({
          status: r.getAttribute('data-status'),
          text: (r as HTMLElement).innerText.replace(/\n/g, ' | '),
        })),
      };
    });
    check('切得到「我參與過的」', (mine as any).ok && (mine as any).active === 'mine', mine);
    check('頁籤寫進網址（可分享、refresh 回得來）', /[?&]tab=mine/.test((mine as any).hash ?? ''), (mine as any).hash);
    check('那一格問的是 scope=mine', (mine as any).scope === 'mine', (mine as any).scope);
    check('「我參與過的」沒有進行中／已完成頁籤', (mine as any).hasStatusTabs === false, mine);

    const rows: any[] = (mine as any).rows ?? [];
    check('列得出我參與過的單', rows.length > 0, rows.length);
    check('含**已完成**的單（不是只列進行中）',
      rows.some((r) => ['completed', 'aborted', 'terminated'].includes(String(r.status))),
      rows.map((r) => r.status));

    /**
     * **這一輪最重要的一條**：`SEED-CAND-1`。
     *
     * 那一筆是 `manager@` 認領並推完的，`operator@` **從頭到尾沒碰過它** ——
     * 唯一的關聯是**候選池**（`clerk-review` 指派給業務部全員）。
     *
     * 只看「目前 task 的 assignee 是不是我」的實作，在畫面上**長得一模一樣**
     * （有資料、能點、不報錯），只是少了這一筆 ——
     * 而那恰好是使用者最想回頭查的（「那張單後來怎麼了？」）。
     */
    log(`  候選池那一筆的單號：${candidateDocNo || '(問不到)'}`);
    check('問得到那一筆已完成的單號（不然下面那條沒有意義）', !!candidateDocNo, candidateDocNo);
    check('列得出「我曾在候選池但沒認領」的那一筆',
      !!candidateDocNo && rows.some((r) => String(r.text).includes(candidateDocNo)),
      rows.map((r) => String(r.text).slice(0, 60)));
    await shot(page, '2-mywork-mine');

    // ── ③ 落在這一頁的角色：會計 ───────────────────────────────
    hr('③ 會計的落點就是這一頁');
    await loginAs(page, demoEmail('accounting'));
    await selectOrg(page);
    const acct = await (async () => {
      const r = await eapi(page, 'GET', '/me/landing');
      return r.status === 200 ? r.data : { error: r.status };
    })();
    check('會計的落點是「我的工作」', acct?.route === 'energyMyWorkConsole', acct);

    // ── ④ §6.6.1 沒有 view-all：選單隱藏 ＋ scope=all 回 403 ──────
    hr('④ §6.6.1 沒有 view-all：選單整條隱藏 ＋ scope=all 403');
    await loginAs(page, demoEmail('finance'));
    await selectOrg(page);
    await goto(page, 'energyMyWorkConsole', '[data-testid="mywork-tabs"]');
    await waitIdle(page);
    const menu = await page.evaluate(() => {
      const hrefs = Array.from(document.querySelectorAll('a[href]'))
        .map((a) => (a as HTMLAnchorElement).getAttribute('href') ?? '');
      return {
        exec: hrefs.filter((h) => /energyExecutionConsole/.test(h)).length,
        bills: hrefs.filter((h) => /energyBillConsole/.test(h)).length,
        myWork: hrefs.filter((h) => /energyMyWorkConsole/.test(h)).length,
      };
    });
    // 整條隱藏，不是顯示空清單 —— 空清單會讓人以為「今天沒有單」。
    check('finance@ 看不到「營運 › 流程單」這個選單項', menu.exec === 0, menu);
    check('但他看得到電費單與我的工作（不是整片消失）',
      menu.bills > 0 && menu.myWork > 0, menu);

    const api = {
      all: (await eapi(page, 'GET', '/executions?scope=all')).status,
      mine: (await eapi(page, 'GET', '/executions?scope=mine')).status,
    };
    /**
     * **靜默降級的畫面看起來完全正常** —— 有資料、能篩選、不報錯，
     * 只是使用者以為自己看到的是全組織。所以這裡要的是 **403**，
     * 不是「200 但只回我參與的」。
     */
    check('scope=all 沒有 view-all → 403（不是靜默降級成 mine）', api.all === 403, api);
    check('同一個人 scope=mine 正常（403 只針對越權的那一問）', api.mine === 200, api);

    /**
     * 但**路由本身要留著** —— P12 詳情是**參與者**就看得到的，
     * 而工作台／稽核／送件／重複對話框都連得到它。
     * 照選單的閘去收緊 resource → 承辦點自己的單會落到沒註冊的路由上，畫面全白。
     */
    const detailRoute = await goto(page, 'energyExecutionConsole/1/show', 'main');
    const detailText = await page.evaluate(
      () => (document.querySelector('main') as HTMLElement)?.innerText ?? '',
    );
    check('沒有 view-all 也進得了 P12 詳情的路由（不是白畫面）',
      detailRoute && detailText.trim().length > 0, detailText.slice(0, 80));

    // ── ⑤ §7.4 例外件併回電費單 ────────────────────────────────
    hr('⑤ §7.4 例外件 → 電費單的一個頁籤');
    await loginAs(page, demoEmail('operator'));
    await selectOrg(page);

    // **相對量**：先讀現況，不寫死「應該是 2」——這支要可以重複跑。
    const before = await unmatchedCount(page);
    check('讀得到目前的待指認筆數', before >= 0, { before });

    /**
     * 造兩筆「電號對不上」的單（fixture 認檔名 `-unmatched`）。
     *
     * 不從 dropzone 丟假圖：收件台改成「先上傳、後辨識」之後，
     * 假圖會**真的**送進辨識服務，然後理所當然地判不出來
     * （`seedBatch` 的說明）。這裡要的是「電號對不上」這個情境，
     * 而不是驗上傳；上傳那條由 `crawler-energy-ops-recognition` 用真 PDF 驗。
     * 兩個檔名不同 → 兩個不同的業務身分（同檔名會變成同一張紙）。
     */
    const stamp = String(before);
    const batch = await seedBatch(page, [
      `bill-unmatched-a${stamp}.png`, `bill-unmatched-b${stamp}.png`,
    ]);
    check('上傳兩張電號對不上的單', batch.ok, batch);
    await waitIdle(page);

    await goto(page, 'energyBillConsole', '[data-testid="bills-tabs"]');
    await waitIdle(page);
    const tabs = await page.evaluate(() => {
      const t = document.querySelector('[data-testid="bills-tab-pendingMatch"]') as HTMLElement | null;
      return {
        hasTabs: !!document.querySelector('[data-testid="bills-tabs"]'),
        count: Number(t?.dataset.count ?? '-1'),
        label: t?.innerText ?? '',
        badge: Number((document.querySelector('[data-testid="menu-bill-badge"]') as HTMLElement)?.dataset.count ?? '-1'),
        menuException: Array.from(document.querySelectorAll('a[href]'))
          .filter((a) => /energyExceptionConsole/.test((a as HTMLAnchorElement).getAttribute('href') ?? '')).length,
      };
    });
    check('電費單頁有「待指認」頁籤', tabs.hasTabs, tabs);
    check('待指認筆數 ＝ 原本 + 2', tabs.count === before + 2, { before, ...tabs });
    // 併進來之後最怕的是**變得更安靜** —— 角標就是在擋那個。
    check('選單角標與頁籤數字一致', tabs.badge === tabs.count, tabs);
    check('選單上「例外件」不見了', tabs.menuException === 0, tabs);
    await shot(page, '5-bills-pendingmatch');

    // ── ⑥ 在電費單詳情頁指認 → 數字 -1 ─────────────────────────
    hr('⑥ 指認的動作在電費單詳情頁');
    const opened = await page.evaluate(async () => {
      const wait = (ms: number) => new Promise((res) => { setTimeout(res, ms); });
      (document.querySelector('[data-testid="bills-tab-pendingMatch"]') as HTMLElement | null)?.click();
      await wait(2500);
      const row = document.querySelector('[data-testid="bill-row"]') as HTMLElement | null;
      if (!row) return { note: 'no row' };
      row.click();
      await wait(3500);
      const btn = document.querySelector('[data-testid="review-match-station"]') as HTMLElement | null;
      if (!btn) return { note: 'no match button', hash: window.location.hash };
      btn.click();
      await wait(1200);
      return { opened: !!document.querySelector('[data-testid="station-picker"]') };
    });
    // 只說問題不給出口，使用者唯一能做的就是關掉頁面然後困惑。
    check('待指認那一列點進詳情頁，警示上就有「指認案場」的出口',
      (opened as any)?.opened === true, opened);

    /**
     * **一定要先搜尋**。對話框預帶的是那張單**原樣的電號**，
     * 而它對不上任何案場 —— 那正是它變成例外件的原因。
     * 不搜尋就直接開下拉，選項是空的（症狀看起來像「指認功能壞了」）。
     */
    const searchBox = await page.$('[data-testid="station-search"]');
    check('指認對話框有搜尋框', !!searchBox);
    if (searchBox) {
      await searchBox.click({ clickCount: 3 });
      await searchBox.type('屏東', { delay: 20 });
    }
    await waitIdle(page);
    // 用真滑鼠點 —— MUI Select 靠 mousedown 開啟，`element.click()` 打不開。
    const picker = await page.$('[data-testid="station-picker"]');
    if (picker) await picker.click();
    await sleep(800);
    const chose = await page.evaluate(() => {
      const opt = document.querySelector('li[role=option]') as HTMLElement | null;
      if (!opt) return false;
      opt.click();
      return true;
    });
    check('搜尋得到並選得到案場', chose);
    await sleep(400);
    await page.evaluate(() => {
      (document.querySelector('[data-testid="station-match-confirm"]') as HTMLElement | null)?.click();
    });
    await waitIdle(page);
    await shot(page, '6-after-match');

    // 重整之後仍成立 ＝ 真的寫進 DB，不是畫面上的樂觀更新。
    await goto(page, 'energyBillConsole', '[data-testid="bills-tabs"]');
    await waitIdle(page);
    const after = await page.evaluate(() => ({
      count: Number((document.querySelector('[data-testid="bills-tab-pendingMatch"]') as HTMLElement)?.dataset.count ?? '-1'),
      badge: Number((document.querySelector('[data-testid="menu-bill-badge"]') as HTMLElement)?.dataset.count ?? '-1'),
    }));
    check('指認後待指認 -1（重整仍成立＝真的寫進 DB）', after.count === tabs.count - 1, { tabs, after });
    check('角標跟著變', after.badge === after.count, after);

    // ── ⑦ 兩個舊 resource key 的殘留 ───────────────────────────
    hr('⑦ 舊 resource key 不留殘留');
    const ghosts = await page.evaluate(() => {
      const html = document.documentElement.innerHTML;
      return {
        exception: (html.match(/energyExceptionConsole/g) ?? []).length,
        workbench: (html.match(/energyWorkbenchConsole/g) ?? []).length,
      };
    });
    check('畫面上沒有 energyExceptionConsole / energyWorkbenchConsole',
      ghosts.exception === 0 && ghosts.workbench === 0, ghosts);

    /**
     * **§8 的佇列動線不在這一支** —— 它在 `scripts/crawler-energy-ops-queue`。
     *
     * 拆出去的理由跟【7】拆 roles crawler 是同一個：
     * 它原本接在這一支的最後面，而這一支跑到那裡時瀏覽器 session 已經很長，
     * 第一次跑就撞上 `Session closed`（**32 條綠、然後整個掛掉**）。
     * 排在最後面的段落等於沒有守衛。
     */
    hr('伺服器');
    check('沒有 5xx', serverErrors.length === 0, [...new Set(serverErrors)]);
  } finally {
    await browser.close();
  }

  board.finish('LIFECYCLE 【5】§7.3 ＋ §7.4');
}

run().catch((e) => { console.error(e); process.exitCode = 1; });
