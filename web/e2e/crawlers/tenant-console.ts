/* eslint-disable no-console, no-await-in-loop */
/*
Usage:
  移植自 az `scripts/crawler-tenant-console-energy-ops`。跑法：`web/e2e/run-crawlers.sh tenant-console`
  （每次都重建專用資料庫，示範資料＋四份範本電費單）。看瀏覽器：CRAWLER_HEADFUL=1。

  這支會**真的動資料**（E 段造兩張電號對不上的單、F 段指認掉一張），
     所以斷言用的是絕對數字，一定要在剛灌好的資料庫上跑。
     不要因為「第二次跑紅了」就把斷言放寬成「>= 1」——那等於把網子剪一個洞。

說明：
  energy-ops-console Slice 1（組織模型＋角色體系）的**真瀏覽器**驗收。
  對應 docs/features/energy-ops-console/TASKS.md 的「驗收清單」最後一條
  ——測試全綠但畫面不對不算完成。

  驗收項：
   A. 成員頁：角色下拉**只**看得到五個中文具名角色（系統管理員／高階主管／業務承辦／
      財務／唯讀），整頁 innerText 不得出現 `energy:` / `ryxis:` 字串。
   B. 部門頁：真的用 UI 建一個部門、指派一位 manager，重新載入後仍在（＝真的寫進 DB）。
   C. console 無 error。

  原版用 admin@foo.bar（平台 super-user，同時是三個部門的成員）。本版用示範組織的
     admin@demo.energy：它是組織建立者（組織管理員）＋ energy:admin（所有能源閘都過），
     而且同樣是業務／會計／財務三個部門的成員 —— 工作台與校對台那幾段要的是「部門成員」這個身分，
     平台超級使用者（admin@example.com）不是示範組織的成員，選不到這個組織。
     所以這支驗的是**畫面與資料流**，不是權限判定；權限判定（CXO 指派 CXO → 403 等）由後端整合測試負責。
*/
import type { Browser, Page } from 'puppeteer';
import {
  APP, Scoreboard, hr, log, sleep, launch, shooter, loginAsAdmin, loginAs, demoEmail, selectOrg, seedBatch,
  hardenPage, eapi,
} from '../lib/energyOpsHarness.ts';

const board = new Scoreboard();
const check = board.check.bind(board);
const shootPng = shooter('energy-ops');

const FIVE_ROLES = ['系統管理員', '高階主管', '業務承辦', '財務', '唯讀'];
/** 這一輪要建的部門（帶時間戳，重跑不撞 key 的 unique index）。 */
const DEPT_KEY = `crawler-${Date.now().toString(36)}`;
const DEPT_NAME = `【測試】業務部 ${new Date().toISOString().slice(11, 19)}`;
/** 這支自己建的部門的 key 前綴（含早期版本用過的 `sales-mt*`），開跑前一律先掃掉。 */
const CRAWLER_KEY_RE = /^(crawler-|sales-mt)/;

async function shot(page: Page, name: string) {
  await shootPng(page, name);
}

/**
 * **關掉登入落點導向**（`LandingRedirect`）。
 *
 * 那支只在**根路徑**導，而且每個 session 只導一次 ——
 * 但 `page.goto(APP#/xxx)` 的第一個 render 有機會落在 `/`
 * （hash router 還沒接手），於是它就把我們導去 admin 的落點
 * （`energyWorkflowConsole`），下一個 `gotoPage` 直接 throw、整支腳本掛掉。
 *
 * 這是**間歇的**：跑三次可能只中一次，而症狀是「進不了成員頁」——
 * 看起來像成員頁壞了。
 *
 * 這支 crawler **不驗落點**（那是 roles crawler 的事），所以直接把
 * 「已經導過」的記號先寫進去。不要改產品碼來配合驗收。
 */
async function disableLandingRedirect(page: Page) {
  await page.evaluate(() => {
    for (let i = 1; i <= 50; i += 1) sessionStorage.setItem(`energy-landing-done:${i}`, 'crawler');
  });
}

/** 走 hash route 進某一頁，等到頁面標題文字出現。 */
async function gotoPage(page: Page, hash: string, expectText: string) {
  await page.goto(`${APP}#/${hash}`, { waitUntil: 'networkidle2' });
  await disableLandingRedirect(page);
  await page.reload({ waitUntil: 'networkidle2' });
  for (let i = 0; i < 40; i += 1) {
    const seen = await page.evaluate((t) => (document.body.innerText || '').includes(t), expectText);
    if (seen) return;
    await sleep(250);
  }
  throw new Error(`進不了 #/${hash}（找不到「${expectText}」），停在 ${page.url()}`);
}

/**
 * 等某個 selector 出現。
 *
 * **`gotoPage` 只等文字，不夠**：頁面標題那幾個字（「總覽」「行事曆」…）
 *    在**選單上也有**，所以 `gotoPage` 會在內容還沒 render 出來時就回來。
 *    Dashboard 自從掛上 three.js 之後首次載入慢很多，這個差別就變成整段紅。
 *    → 需要真的等內容的地方一律用這支，不要靠 `sleep` 猜。
 */
async function waitFor(page: Page, selector: string, timeoutMs = 30000): Promise<boolean> {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    // eslint-disable-next-line no-await-in-loop
    const hit = await page.evaluate((sel) => !!document.querySelector(sel), selector);
    if (hit) return true;
    // eslint-disable-next-line no-await-in-loop
    await sleep(250);
  }
  return false;
}

/** 整頁可見文字裡有沒有 uniqueName 洩漏。回傳命中的片段（空陣列＝乾淨）。 */
async function leakedPrefixes(page: Page): Promise<string[]> {
  return page.evaluate(() => {
    const text = (document.body.innerText || '');
    const hits: string[] = [];
    for (const m of text.matchAll(/(energy:|ryxis:)[\w:-]*/g)) hits.push(m[0]);
    return [...new Set(hits)];
  });
}

async function run() {
  const browser: Browser = await launch();
  const page = hardenPage(await browser.newPage());
  const consoleErrors: string[] = [];
  page.on('console', (m) => { if (m.type() === 'error') consoleErrors.push(m.text()); });
  // console 的 "Failed to load resource: 403" 不會說是哪個 URL —— 自己記下來。
  const forbidden: string[] = [];
  page.on('response', (r) => { if (r.status() === 403) forbidden.push(`${r.request().method()} ${new URL(r.url()).pathname}`); });
  // 401 也記下來：只有登入前問 session 的那一發是預期的。
  const unauthorized: string[] = [];
  page.on('response', (r) => { if (r.status() === 401) unauthorized.push(`${r.request().method()} ${new URL(r.url()).pathname}`); });

  try {
    hr('登入');
    await loginAsAdmin(page);
    const orgId = await selectOrg(page);
    check('選得到 demo 組織', orgId != null);

    // ---------------------------------------------------------------
    hr('A. 成員頁：角色下拉只有五個中文角色');
    // 本版的成員頁是平台頁（組織成員），等成員表出現；「管理權限」是能源組織才畫的入口。
    await gotoPage(page, 'organizationMembers', '組織成員');
    check('成員表載入', await waitFor(page, '[data-testid^="member-grant-"]'));
    await shot(page, 'A1-members-page');

    // 開第一位成員的「管理權限」對話框（鑰匙圖示）
    const opened = await page.evaluate(() => {
      // MUI 圖示的 data-testid 只在開發版才有，正式建置要靠按鈕自己的 testid。
      const btn = document.querySelector('[data-testid^="member-grant-"]');
      if (!btn) return false;
      (btn as HTMLElement).click();
      return true;
    });
    check('找得到「管理權限」按鈕', opened);
    await sleep(1200);

    // 展開角色下拉。用真滑鼠點 —— MUI 的 Select 靠 mousedown 開啟，
    // `element.click()` 只送 click，選單不會展開（第一輪就卡在這，量到 0 個選項）。
    await page.waitForSelector('.MuiDialog-root [role="combobox"]', { timeout: 15000 });
    await page.click('.MuiDialog-root [role="combobox"]');
    await sleep(800);
    await shot(page, 'A2-role-dropdown');

    const options = await page.evaluate(() => Array.from(document.querySelectorAll('li[role="option"]'))
      .map((li) => (li as HTMLElement).innerText.trim())
      .filter((t) => t.length > 0));
    log('  下拉選項：', JSON.stringify(options));

    check('下拉剛好 5 個選項', options.length === 5, options);
    for (const role of FIVE_ROLES) {
      check(`下拉含「${role}」`, options.some((o) => o.startsWith(role)), options);
    }
    const dropdownLeak = options.filter((o) => /energy:|ryxis:/.test(o));
    check('下拉沒有 energy:/ryxis: 字串', dropdownLeak.length === 0, dropdownLeak);

    // 關掉下拉與對話框後，量整頁
    await page.keyboard.press('Escape');
    await sleep(300);
    await page.keyboard.press('Escape');
    await sleep(600);

    const memberLeaks = await leakedPrefixes(page);
    check('成員頁整頁沒有 energy:/ryxis: 字串', memberLeaks.length === 0, memberLeaks);

    // ---------------------------------------------------------------
    hr('B. 部門頁：建部門 + 指派 manager + 重整仍在');
    await gotoPage(page, 'departments', '部門清單');

    // 乾淨起點：先掃掉前幾輪殘留的測試部門（舊產物會讓後面的斷言被矇混過去）。
    for (let i = 0; i < 20; i += 1) {
      const removed = await page.evaluate((src) => {
        const re = new RegExp(src);
        const row = Array.from(document.querySelectorAll('tbody tr'))
          .find((tr) => re.test(((tr as HTMLElement).innerText.split('\t')[1] || '').trim()));
        const btn = row?.querySelector('[data-testid="department-delete"]');
        if (!btn) return false;
        (btn as HTMLElement).click();
        return true;
      }, CRAWLER_KEY_RE.source);
      if (!removed) break;
      await sleep(900);
    }
    await shot(page, 'B1-departments-empty');

    const deptLeaks = await leakedPrefixes(page);
    check('部門頁整頁沒有 energy:/ryxis: 字串', deptLeaks.length === 0, deptLeaks);

    // 新增部門
    await page.evaluate(() => {
      const btn = Array.from(document.querySelectorAll('button')).find((b) => (b.textContent || '').includes('新增部門'));
      (btn as HTMLElement | undefined)?.click();
    });
    await sleep(700);
    const inputs = await page.$$('.MuiDialog-root input');
    check('新增對話框有兩個輸入欄（名稱 + 識別碼）', inputs.length >= 2, inputs.length);
    if (inputs.length >= 2) {
      await inputs[0].type(DEPT_NAME, { delay: 10 });
      await inputs[1].type(DEPT_KEY, { delay: 10 });
    }
    await shot(page, 'B2-create-dialog');
    await page.evaluate(() => {
      const btns = Array.from(document.querySelectorAll('.MuiDialog-root button'));
      const create = btns.find((b) => /建立|新增|Create/.test(b.textContent || ''));
      (create as HTMLElement | undefined)?.click();
    });
    await sleep(1500);

    let bodyText = await page.evaluate(() => document.body.innerText || '');
    check(`部門「${DEPT_NAME}」出現在清單`, bodyText.includes(DEPT_NAME), bodyText.slice(0, 400));
    await shot(page, 'B3-department-created');

    // 指派 manager。要按**這一輪建的那一列**的按鈕 —— 直接抓第一個 PersonAddIcon
    // 會按到上一輪殘留的部門，斷言仍然會過，但驗到的是別的東西（探針量錯對象）。
    const clickedRow = await page.evaluate((name) => {
      const row = Array.from(document.querySelectorAll('tr')).find((tr) => (tr as HTMLElement).innerText.includes(name));
      const btn = row?.querySelector('[data-testid="department-assign"]');
      if (!btn) return false;
      (btn as HTMLElement).click();
      return true;
    }, DEPT_NAME);
    check(`按到「${DEPT_NAME}」該列的指派按鈕`, clickedRow);
    await sleep(900);
    // 成員下拉（第一個 combobox）→ 選第一位（同樣要真滑鼠）
    await page.waitForSelector('.MuiDialog-root [role="combobox"]', { timeout: 15000 });
    const memberBoxes = await page.$$('.MuiDialog-root [role="combobox"]');
    await memberBoxes[0].click();
    await sleep(700);
    const pickedUser = await page.evaluate(() => {
      const opt = document.querySelector('li[role="option"]');
      if (!opt) return null;
      const label = (opt as HTMLElement).innerText.trim();
      (opt as HTMLElement).click();
      return label;
    });
    check('成員下拉有人可選', !!pickedUser, pickedUser);
    await sleep(400);
    // 部門角色下拉 → 選「部門主管」
    const roleBoxes = await page.$$('.MuiDialog-root [role="combobox"]');
    await roleBoxes[roleBoxes.length - 1].click();
    await sleep(700);
    await page.evaluate(() => {
      const opt = Array.from(document.querySelectorAll('li[role="option"]')).find((o) => (o as HTMLElement).innerText.includes('部門主管'));
      (opt as HTMLElement | undefined)?.click();
    });
    await sleep(400);
    await shot(page, 'B4-assign-dialog');
    await page.evaluate(() => {
      const btn = Array.from(document.querySelectorAll('.MuiDialog-root button')).find((b) => (b.textContent || '').includes('指派成員'));
      (btn as HTMLElement | undefined)?.click();
    });
    await sleep(1500);
    await shot(page, 'B5-manager-assigned');

    // 重整：資料是不是真的落地（不是只在 React state 裡）
    await gotoPage(page, 'departments', '部門清單');
    await sleep(1200);
    bodyText = await page.evaluate(() => document.body.innerText || '');
    check(`重整後部門「${DEPT_NAME}」仍在（真的寫進 DB）`, bodyText.includes(DEPT_NAME));
    // 斷言到**那一列**，不是整頁 —— 整頁比對會被別的部門的主管矇混過去。
    const rowText = await page.evaluate((name) => {
      const row = Array.from(document.querySelectorAll('tr')).find((tr) => (tr as HTMLElement).innerText.includes(name));
      return row ? (row as HTMLElement).innerText : null;
    }, DEPT_NAME);
    log(`  「${DEPT_NAME}」該列：`, JSON.stringify(rowText));
    check(
      `重整後該列的主管是「${pickedUser}」`,
      !!pickedUser && !!rowText && rowText.includes(String(pickedUser)),
      rowText,
    );
    check('該列的識別碼正確', !!rowText && rowText.includes(DEPT_KEY), rowText);
    await shot(page, 'B6-after-reload');

    const finalLeaks = await leakedPrefixes(page);
    check('重整後部門頁仍沒有 energy:/ryxis: 字串', finalLeaks.length === 0, finalLeaks);

    // 收尾：刪掉這一輪建的部門，讓重跑從乾淨狀態開始（別在 local DB 累積測試殘留）。
    const deleted = await page.evaluate((name) => {
      const row = Array.from(document.querySelectorAll('tr')).find((tr) => (tr as HTMLElement).innerText.includes(name));
      const btn = row?.querySelector('[data-testid="department-delete"]');
      if (!btn) return false;
      (btn as HTMLElement).click();
      return true;
    }, DEPT_NAME);
    await sleep(1500);
    const gone = await page.evaluate((name) => !(document.body.innerText || '').includes(name), DEPT_NAME);
    check('刪除部門後它從清單消失', deleted && gone);


    // ---------------------------------------------------------------
    // C1：四頁骨架（P1 待辦 / P2 單據 / P4 例外件 / P5 地主）
    //
    // 前置：`yarn dev:sr scripts/energy-ops-demo-seed` 灌好展示資料。
    //   空表格證明不了任何事 —— 下面斷言的是**絕對數字**：
    //      4 張單、其中 2 張例外件、2 位地主、陳明德底下 2 座案場（1 座異常）。
    // ---------------------------------------------------------------
    hr('D. 選單：能源營運四頁都在');
    // LIFECYCLE 【4】【5】之後的名字：
    // 「收件台／送單紀錄」→「送件」、「工作台」→「我的工作」、
    // 「例外件」**整項消失**（併成電費單的一個頁籤，見 §7.4）。
    // 卡 A（20261118-0427-a）併回：地主頁改名「受款對象」（地主/受款對象合併）。
    const menuLabels = ['我的工作', '送件', '電費單', '受款對象'];
    const menuText = await page.evaluate(() => document.body.innerText || '');
    for (const label of menuLabels) {
      check(`選單有「${label}」`, menuText.includes(label));
    }
    await shot(page, 'D1-menu');

    // ---------------------------------------------------------------
    hr('E. P2 單據清單');
    /**
     * 原版 seed 自帶兩張電號對不上的單；本版的示範資料四個電號都種成了案場（與 az 現行 seed 相同），
     * 例外件要從收件那條路自己造：真的開批、真的上傳，辨識結果用「電號對不上」的 fixture。
     * 造完是 4（範本）＋ 2 ＝ 6 張，下面的絕對數字照舊。
     */
    const unmatched = await seedBatch(page, ['bill-unmatched-tc1.png', 'bill-unmatched-tc2.png']);
    check('造出兩張電號對不上的單', unmatched.ok, unmatched);
    await gotoPage(page, 'energyBillConsole', '單據清單');
    await sleep(800);
    const billRows = await page.$$eval('[data-testid="bill-row"]', (els) => els.length);
    /**
     * demo seed 的**絕對數字**（刻意不寫 `>=`：絕對值才擋得住「靜默少一張」）。
     *
     * 六張 ＝ 示範資料的四份範本（`demo_seed::run_sample_bills`：主線已校對 ／ 校對完沒送出 ／
     * 金額被人工修正且流程跑完的候選池情境 ／ 無餘電那張）＋ 上面造的兩張電號對不上。
     *
     * **前置條件：這一支要在乾淨 DB 上跑**（run-crawlers.sh 每支都重建資料庫）。
     * 不改成 `>=` 是因為那會讓「seed 少給一張」被殘留資料補滿而漏掉 ——
     * 這條斷言的意圖就是擋那件事。
     *
     * 卡 G3：七張＝原本六張 ＋ 卡 D commit 3eb6f8e 加的「本關要求」示範假單
     * （電號 00-00-0000-00-0，`demo_seed.rs:977-1013`）。
     */
    check('7 張單全在（demo seed 的絕對數字）', billRows === 7, { billRows });

    const billText = await page.evaluate(() => document.body.innerText || '');
    check('顯示案場中文名（不是 id）', billText.includes('屏東枋寮一號'), null);
    // 校對狀態一律翻成中文，畫面上不出現 `reviewed` / `pendingMatch` 這種 enum 值。
    check('校對狀態是中文', billText.includes('已校對') && billText.includes('待校對'));
    check('畫面上沒有 enum 原文', !/\breviewed\b|\bpendingMatch\b|\bpendingReview\b/.test(billText),
      billText.match(/\breviewed\b|\bpendingMatch\b|\bpendingReview\b/g));
    // 沒指認到案場的那兩張要標「未指認」。
    const unmatchedTags = (billText.match(/未指認/g) || []).length;
    check('2 張標為「未指認」', unmatchedTags === 2, { unmatchedTags });
    // demo seed **只有一張**最終金額被改過 → 「已人工修正」要**剛好一個**。
    // 這裡刻意數個數而不是 `includes` —— 用 `includes` 時整批都被誤標也照樣綠燈
    //    （真的發生過：seed 漏給 `ocrExtracted.issuedAt`，四張全掛標記）。
    const correctedTags = (billText.match(/已人工修正/g) || []).length;
    check('「已人工修正」剛好 1 個（不是「有就好」）', correctedTags === 1, { correctedTags });
    await shot(page, 'E1-bills');

    // 篩選：只留「待指認案場」→ 應剩 2 列。
    await page.click('[data-testid="bills-review-status"]');
    await sleep(400);
    const pickedFilter = await page.evaluate(() => {
      const opt = Array.from(document.querySelectorAll('li[role=option]'))
        .find((li) => (li as HTMLElement).innerText.trim() === '待指認案場');
      if (!opt) return false;
      (opt as HTMLElement).click();
      return true;
    });
    check('篩選下拉打得開且有「待指認案場」', pickedFilter);
    await sleep(1200);
    const filtered = await page.$$eval('[data-testid="bill-row"]', (els) => els.length);
    check('篩「待指認案場」後剩 2 列', filtered === 2, { filtered });
    await shot(page, 'E2-bills-filtered');

    // ---------------------------------------------------------------
    // 例外件已經**不是一個資源**了（`LIFECYCLE.md §7.4`）——
    // 它是電費單清單的一個頁籤，指認的動作在**電費單詳情頁**。
    hr('F. 待指認：真的指認一座案場');
    await gotoPage(page, 'energyBillConsole?tab=pendingMatch', '單據清單');
    await sleep(1200);
    const before = await page.$$eval('[data-testid="bill-row"]', (els) => els.length);
    check('「待指認」頁籤有 2 筆', before === 2, { before });
    const exText = await page.evaluate(() => document.body.innerText || '');
    check('說明講清楚「會卡住」', exText.includes('停在這裡不進審批'));
    await shot(page, 'F1-unmatched');

    /**
     * 指認的動作搬到**電費單詳情頁**（`LIFECYCLE.md §7.4`）——
     * 清單頁只負責「哪幾張要處理」，處理發生在那一張自己的頁上。
     * 所以這裡先點進第一列，再按左欄警示上的那顆「指認案場」。
     */
    const openedRow = await page.evaluate(() => {
      const row = document.querySelector('[data-testid="bill-row"]') as HTMLElement | null;
      if (!row) return false;
      row.click();
      return true;
    });
    check('點得進待指認那一張的詳情頁', openedRow);
    await sleep(3000);
    const openedMatch = await page.evaluate(() => {
      const btn = document.querySelector('[data-testid="review-match-station"]') as HTMLElement | null;
      if (!btn) return false;
      btn.click();
      return true;
    });
    // 只說問題不給出口，使用者唯一能做的就是關掉頁面然後困惑 ——
    // 那顆按鈕必須**就在那句警示旁邊**。
    check('詳情頁的警示上就有「指認案場」的出口', openedMatch);
    await sleep(900);

    // 搜尋既有案場（demo seed 的「屏東枋寮一號」）。
    const searchBox = await page.$('[data-testid="station-search"]');
    check('對話框有搜尋框', !!searchBox);
    if (searchBox) {
      await searchBox.click({ clickCount: 3 });
      await searchBox.type('屏東枋寮一號', { delay: 20 });
    }
    await sleep(1200);
    await shot(page, 'F2-match-dialog');

    // 用真滑鼠點 —— MUI Select 靠 mousedown 開啟，element.click() 打不開。
    const picker = await page.$('[data-testid="station-picker"]');
    if (picker) await picker.click();
    await sleep(500);
    const optionTexts = await page.$$eval('li[role=option]', (els) => els.map((e) => (e as HTMLElement).innerText));
    check('搜尋得到「屏東枋寮一號」', optionTexts.some((t) => t.includes('屏東枋寮一號')), optionTexts);
    const chose = await page.evaluate(() => {
      const opt = Array.from(document.querySelectorAll('li[role=option]'))
        .find((li) => (li as HTMLElement).innerText.includes('屏東枋寮一號'));
      if (!opt) return false;
      (opt as HTMLElement).click();
      return true;
    });
    check('選得到案場', chose);
    await sleep(400);
    const submitted = await page.evaluate(() => {
      const btn = Array.from(document.querySelectorAll('.MuiDialogActions-root button'))
        .find((b) => (b as HTMLElement).innerText.includes('指認'));
      if (!btn || (btn as HTMLButtonElement).disabled) return false;
      (btn as HTMLElement).click();
      return true;
    });
    check('送出指認', submitted);
    await sleep(2000);

    // 真的寫進去了嗎 —— 重新載入再數一次。
    await gotoPage(page, 'energyBillConsole?tab=pendingMatch', '單據清單');
    await sleep(1200);
    const after = await page.$$eval('[data-testid="bill-row"]', (els) => els.length);
    check('指認後待指認從 2 筆變 1 筆（重整仍成立＝真的寫進 DB）', after === 1, { before, after });
    await shot(page, 'F3-unmatched-after');

    // ---------------------------------------------------------------
    hr('G. P5 地主 / 發電站');
    await gotoPage(page, 'energyLandownerConsole', '地主');
    await sleep(1000);
    // 卡 A 併回：這頁現在混了受款對象（台電區處／子公司／公司／機關），
    // 不核總數（會隨卡 A 的示範資料變動），只核兩位地主都還在清單裡。
    const ownerRows = await page.$$eval('[data-testid="landowner-row"]', (els) => els.length);
    const ownerText = await page.evaluate(() => document.body.innerText || '');
    check('清單裡有陳明德、林秀英', ownerText.includes('陳明德') && ownerText.includes('林秀英'), { ownerRows });
    check('異常案場標出來了', ownerText.includes('1 異常'), null);
    await shot(page, 'G1-landowners');

    // 點開陳明德 → 詳情抽屜（「一個地主的所有錢與時間」）
    const openedOwner = await page.evaluate(() => {
      // 只有列裡第一格（名字那格）掛了 onClick 導頁，點整列不會動
      // （`LandownersPage.tsx`：onClick 在 `TableCell` 上，不是 `TableRow`）。
      const row = Array.from(document.querySelectorAll('[data-testid="landowner-row"]'))
        .find((tr) => (tr as HTMLElement).innerText.includes('林秀英'));
      const cell = row?.querySelector('td') as HTMLElement | null;
      if (!cell) return false;
      cell.click();
      return true;
    });
    check('點得開地主詳情', openedOwner);
    /**
     * 這一段原本是驗「詳情**抽屜**＋四個頁籤」——
     * C7b 之後地主詳情變成**獨立路由**（`#/energyLandowners/:id`），
     * 而且版面改成左右兩欄（案場／合約 ｜ 現在正在發生／應付／事件），**沒有頁籤**。
     *
     * 那幾條斷言從那時起就一直是紅的，只是這支 crawler 隔了好幾個切片沒跑
     * —— **沒有人跑的驗收等於沒有驗收**。
     */
    await sleep(2500);
    const detail = await page.evaluate(() => ({
      hash: window.location.hash,
      name: (document.querySelector('[data-testid="landowner-name"]') as HTMLElement)?.innerText ?? '',
      text: (document.querySelector('main') as HTMLElement)?.innerText ?? '',
      stations: document.querySelectorAll('[data-testid="landowner-station"]').length,
    }));
    check('點一列會**換路由**到那個地主自己的頁', /#\/energyLandowners\/\d+/.test(detail.hash), detail.hash);
    check('標頭是那個地主的名字', detail.name.includes('林秀英'), detail.name);
    check('詳情有匯款資訊',
      detail.text.includes('第一銀行') && detail.text.includes('098-765-432100'), detail.text.slice(0, 200));
    // 四個區塊**都在同一頁上**（不是頁籤）——「一個地主的所有錢與時間」。
    check('四個區塊都在（案場／合約／近期事件／近期應付）',
      ['案場', '合約', '近期事件'].every((t) => detail.text.includes(t)), detail.text.slice(0, 300));
    check('列得出他的案場', detail.stations > 0, detail.stations);
    await shot(page, 'G2-landowner-detail');

    // 合約區：建設期要顯示「已付 1 / 3 期」（同一頁，不必切頁籤）。
    check('建設期顯示「已付 1 / 3 期」', detail.text.includes('已付 1 / 3 期'), detail.text.slice(0, 400));
    check('階段是中文', detail.text.includes('建設期'));
    await shot(page, 'G3-landowner-contracts');

    // ---------------------------------------------------------------
    hr('H. P1 角色工作台');
    /**
     * Q-04（2026-11-16）：bill-approval 換成單一審核，候選人只剩部門
     * `bill-reviewers`（manager／finance／accounting）。admin（業務部）
     * 已經不是候選人了，工作台上看不到示範主線那筆 —— 這一段換成用
     * 審核組帳號（finance）看，J 段看完流程圖再切回 admin（後面 K～P
     * 段要的是 admin 的管理權限）。
     */
    await loginAs(page, demoEmail('finance'));
    await selectOrg(page);
    await gotoPage(page, 'energyMyWorkConsole', '我的工作');
    // 「工作台」在**選單上也有** → `gotoPage` 會太早回來。等真正的內容。
    check('工作台內容 render 出來了',
      await waitFor(page, '[data-testid="workbench-section"], .MuiAlert-root'));
    await sleep(1500);

    const wb = await page.evaluate(() => {
      const secs = Array.from(document.querySelectorAll('[data-testid="workbench-section"]'));
      return {
        sections: secs.map((e) => (e as HTMLElement).dataset.section),
        // 空的區不該出現（`FRONTEND.md §2 P1`）——「等我核准（0）」的空殼
        //    是在告訴使用者他有事沒做。
        emptySections: secs.filter((e) => !e.querySelector('[data-testid="workbench-row"]'))
          .map((e) => (e as HTMLElement).dataset.section),
        rows: document.querySelectorAll('[data-testid="workbench-row"]').length,
        hasSort: !!document.querySelector('[data-testid="workbench-sort-waiting"]'),
        text: (document.querySelector('main') as HTMLElement)?.innerText ?? '',
      };
    });

    // demo seed 發起了一筆執行、finance 是部門 bill-reviewers 成員 ⇒ 他是
    // 「平台資料審核」的候選人（Q-04：單一審核，admin 已經不是候選人了）。
    check('工作台有分區（不是通用清單）', wb.sections.length > 0, wb.sections);
    check('沒有空的分區', wb.emptySections.length === 0, wb.emptySections);
    check('至少一列', wb.rows >= 1, wb);
    // 「等了幾天」是這頁最重要的欄位，且要能排序。
    check('「等待」欄可排序', wb.hasSort, wb);
    // 列的資訊密度：案場 · 地主 · 金額 · 關卡（不只有單號）。
    check('列上有案場名', /屏東枋寮一號/.test(wb.text), wb.text.slice(0, 300));
    check('列上有地主', /陳明德|林秀英/.test(wb.text), wb.text.slice(0, 300));
    check('列上有目前關卡', /平台資料審核/.test(wb.text), wb.text.slice(0, 300));
    check('畫面上沒有 nodeKey',
      !/clerk-review|manager-approve|finance-release|platform-review/.test(wb.text));
    check('畫面上沒有 raw key', !/energy:|ryxis:/.test(wb.text));
    await shot(page, 'H1-workbench');

    // 動作按鈕不說謊：要嘛有按鈕、要嘛有一句說明，不會兩個都沒有。
    const act = await page.evaluate(() => ({
      buttons: document.querySelectorAll('[data-testid="action-button"]').length,
      blocked: document.querySelectorAll('[data-testid="action-blocked"]').length,
      blockedText: (document.querySelector('[data-testid="action-blocked"]') as HTMLElement)?.innerText ?? '',
    }));
    check('每一列要嘛有動作、要嘛說得出為什麼不能動',
      act.buttons > 0 || act.blocked > 0, act);
    // 顯示句子不顯示 code。
    check('說明句不含 code', !/excludedOwner|notCandidate|claimedByOther/.test(act.blockedText), act);

    // 跨頁連結：工作台的列 → 校對台（用路由不是 modal）。
    const jumped = await page.evaluate(() => {
      const b = document.querySelector('[data-testid="workbench-open"]');
      if (!b) return false;
      (b as HTMLElement).click();
      return true;
    });
    check('列點得進校對台', jumped);
    await sleep(2500);
    /**
     * 原版這裡斷言「換路由到 `energyBillConsole/:id/show`」。之後的佇列動線（`LIFECYCLE.md §8`，
     * az 的 `crawler-energy-ops-queue` 驗的就是它）把這件事改成：點列在「我的工作」上開右側抽屜，
     * 抽屜裡是**同一個**單據詳情元件，網址寫 `?task=`（可分享、重整回得來）。
     * 這一條改成驗新的版本：同一個校對台元件、網址可分享。
     */
    const inDrawer = await page.evaluate(() => ({
      hash: window.location.hash,
      drawer: !!document.querySelector('[data-testid="mywork-drawer"]'),
      detail: !!document.querySelector('[data-testid="mywork-drawer"] [data-testid="review-field"]'),
    }));
    check('真的到了校對台（抽屜裡是同一個單據詳情元件，網址寫 ?task= 可分享）',
      inDrawer.drawer && inDrawer.detail && /[?&]task=\d+/.test(inDrawer.hash), inDrawer);

    // ---------------------------------------------------------------
    hr('J. C2 唯讀流程圖（在校對台裡 —— 它真正的家）');
    // 流程圖原本掛在舊「我的待辦」的展開列上；`WorkbenchPage` 取代那頁之後，
    //    它唯一的落點就是 P3 校對台（`FRONTEND.md §2-P3` 的版面本來就含它）。
    check('校對台載入', await waitFor(page, '[data-testid="flow-graph"], [data-testid="review-field"]'));
    await sleep(3000);
    const graphInfo = await page.evaluate(() => {
      const el = document.querySelector('[data-testid="flow-current"]') as HTMLElement | null;
      const nodes = Array.from(document.querySelectorAll('.react-flow__node'))
        .map((n) => (n as HTMLElement).innerText.trim());
      return {
        present: !!document.querySelector('[data-testid="flow-graph"]'),
        current: el?.dataset.current ?? '',
        nodeCount: el?.dataset.nodeCount ?? '',
        sendBackCount: el?.dataset.sendbackCount ?? '',
        nodes,
        legend: !!document.querySelector('[data-testid="flow-legend"]'),
      };
    });
    check('流程圖畫出來了', graphInfo.present, graphInfo);
    // Q-04：bill-approval 換成單一審核，3 個節點（start → platform-review → end）。
    check('3 個節點（單一審核 ＋ start/end）', graphInfo.nodeCount === '3', graphInfo);
    check('react-flow 真的 render 了 3 個節點', graphInfo.nodes.length === 3, graphInfo.nodes);
    check('圖上是中文節點名不是 nodeKey',
      graphInfo.nodes.includes('平台資料審核')
      && !graphInfo.nodes.some((n) => /-/.test(n) && /[a-z]/.test(n)),
      graphInfo.nodes);
    // 單一審核沒有退回（Q-04 lack 待確認），不再驗「退回路徑有畫出來」。
    check('目前停在平台資料審核', graphInfo.current === 'platform-review', graphInfo);
    check('有圖例（沒圖例分不出灰色虛線是什麼）', graphInfo.legend);
    await shot(page, 'J1-flow-graph');

    const draggable = await page.evaluate(() => {
      const n = document.querySelector('.react-flow__node');
      return n ? n.classList.contains('draggable') : null;
    });
    check('節點不可拖曳（唯讀）', draggable === false, { draggable });

    // J 段看完流程圖，切回 admin —— 後面 K～P 段要的是 admin 的管理權限。
    await loginAsAdmin(page);
    await selectOrg(page);

    // ---------------------------------------------------------------
    hr('K. C3 校對台（P3）');
    // demo seed 有一張已進流程的單 —— 從 P2 點進去。
    //
    // **不可以用「屏東枋寮一號」找那一列**：F 段剛把一件例外件指認到同一座案場，
    //    所以現在有**兩列**叫這個名字，`.find` 會抓到錯的那一張。
    //    → 用主線那張的金額 **127,848.00**（範本「有餘電需開發票」紙上的值）這個唯一值定位。
    //    （這是探針的錯不是產品的錯，但症狀長得一模一樣：辨識值對不上、沒有流程圖。）
    await gotoPage(page, 'energyBillConsole', '單據清單');
    await sleep(900);
    const entered = await page.evaluate(() => {
      const row = Array.from(document.querySelectorAll('[data-testid="bill-row"]'))
        .find((tr) => (tr as HTMLElement).innerText.includes('127,848.00'));
      if (!row) return false;
      (row as HTMLElement).click();
      return true;
    });
    check('從單據清單點得進校對台', entered);
    await sleep(2500);

    const review = await page.evaluate(() => {
      const rows = Array.from(document.querySelectorAll('[data-testid="review-field"]'));
      return {
        fields: rows.map((r) => (r as HTMLElement).dataset.field),
        ocrValues: rows.map((r) => (r.querySelector('[data-testid="ocr-value"]') as HTMLElement)?.innerText.trim()),
        finalValues: rows.map((r) => (r.querySelector('input') as HTMLInputElement)?.value),
        changed: rows.map((r) => (r as HTMLElement).dataset.changed),
        hasGraph: !!document.querySelector('[data-testid="flow-graph"]'),
        actionButtons: Array.from(
          document.querySelectorAll('[data-testid="review-actions"] button'),
        ).map((b) => (b as HTMLElement).innerText.trim()),
        text: document.body.innerText || '',
      };
    });
    /**
     * 欄位組依版型（R7）：躉購單（`renewable_purchase`）是六欄，比原版固定的五欄多了「台電開單日」。
     * 這張是真的範本 PDF，紙上沒有開單日 —— 那一欄要誠實地寫「未辨識到」、最終值留空，
     * 其餘五欄（原版驗的那五欄）辨識值與最終值都要有。
     */
    const EXPECTED_FIELDS = ['customerNumberRaw', 'billingPeriodStart', 'billingPeriodEnd', 'totalAmount', 'generationKwh', 'issuedAt'];
    check('版型的六個欄位都在（依版型的欄位組）',
      JSON.stringify(review.fields) === JSON.stringify(EXPECTED_FIELDS), review.fields);
    const onPaper = review.fields.map((f, i) => ({ f, i })).filter((x) => x.f !== 'issuedAt');
    // 這頁的核心：辨識值與最終值**並排且各自獨立**。
    check('辨識值有值（不是被最終值覆蓋）',
      onPaper.length === 5 && onPaper.every(({ i }) => !!review.ocrValues[i] && review.ocrValues[i] !== '（未辨識到）'), review.ocrValues);
    check('最終值有值', onPaper.every(({ i }) => !!review.finalValues[i]), review.finalValues);
    const issuedIdx = review.fields.indexOf('issuedAt');
    check('紙上沒有的開單日寫「未辨識到」、最終值留空（不編一個值）',
      review.ocrValues[issuedIdx] === '（未辨識到）' && !review.finalValues[issuedIdx],
      { ocr: review.ocrValues[issuedIdx], final: review.finalValues[issuedIdx] });
    const amountIdx = review.fields.indexOf('totalAmount');
    // demo seed 沒改這張單的金額 → 辨識值與最終值應該一致。
    check('未修改的欄位：辨識值 ≈ 最終值',
      Number(review.ocrValues[amountIdx]) === Number(review.finalValues[amountIdx]),
      { ocr: review.ocrValues[amountIdx], final: review.finalValues[amountIdx] });
    // C2 的流程圖真的嵌進來了（不是重畫一張）。
    check('流程圖嵌在校對台裡', review.hasGraph);
    check('校對台上沒有 raw key', !/energy:|ryxis:/.test(review.text));
    await shot(page, 'K1-review-console');

    // 真的改一個值 → 存 → 重整後「已人工修正」標記要出現。
    //
    // **admin 不能改這張單**：Q-04 起它由 admin 發起、單一審核流程停在平台資料審核，
    //    admin 是發起人被 `resolve_mode.rs` 排除（readonly／ExcludedOwner），輸入框
    //    disabled，Puppeteer 打不進去（`FINDINGS.md` S1）。
    //    → 不是換單，是換**審核者**：finance（`demo_seed.rs` 種進 `bill-reviewers`
    //       部門）不是發起人、又是「平台資料審核」這一關的候選人
    //       （`workflow_defs.rs:686` 部門規則 ＋ `:689` `.subject_edit(true)`），
    //       `resolve_mode.rs` 判成 `Revise`、可改。單還是同一張（127,848），
    //       execution／稽核歷程都留著，K3 才驗得到稽核內容。
    //       （上一版換成「校對完沒送出」那張沒有 execution 的單，K3「稽核有內容」
    //        就必然是空的——那是這次改法自己造成的，不是示範資料漂移。）
    await loginAs(page, demoEmail('finance'));
    await selectOrg(page);
    await gotoPage(page, 'energyBillConsole', '單據清單');
    await sleep(900);
    const enteredAsFinance = await page.evaluate(() => {
      const row = Array.from(document.querySelectorAll('[data-testid="bill-row"]'))
        .find((tr) => (tr as HTMLElement).innerText.includes('127,848.00'));
      if (!row) return false;
      (row as HTMLElement).click();
      return true;
    });
    check('K 段改值：finance 開得進主線單', enteredAsFinance);
    await sleep(2500);

    // 卡 G3：直接打 /audit-events 量狀態碼（不只是看畫面字），finance 沒有
    //    `energy:audit:view`（`energy_rbac.rs:121-126`）→ 預期 403。
    const financeBillHash = await page.evaluate(() => window.location.hash);
    const financeBillId = financeBillHash.match(/energyBillConsole\/(\d+)/)?.[1] ?? '';
    const financeBillDetail = financeBillId ? await eapi(page, 'GET', `/bills/${financeBillId}`) : { status: 0, data: null };
    const financeExecId = (financeBillDetail.data as any)?.executions?.[0]?.id ?? '';
    const financeAuditApi = financeExecId
      ? await eapi(page, 'GET', `/audit-events?targetType=energyWorkflowExecution&targetId=${financeExecId}`)
      : { status: 0, data: null };
    log('  卡 G3 AUDIT-403 finance:', JSON.stringify({ status: financeAuditApi.status, data: financeAuditApi.data }).slice(0, 200));
    check('finance 打 /audit-events 回 403（直接量狀態碼）', financeAuditApi.status === 403, financeAuditApi);

    const amountInput = await page.$('[data-testid="final-totalAmount"]');
    check('找得到最終值輸入框', !!amountInput);
    if (amountInput) {
      await amountInput.click({ clickCount: 3 });
      await amountInput.type('127900.00', { delay: 15 });
    }
    await sleep(400);
    const saved = await page.evaluate(() => {
      const btn = document.querySelector('[data-testid="review-save"]') as HTMLButtonElement | null;
      if (!btn || btn.disabled) return false;
      btn.click();
      return true;
    });
    check('儲存鈕在有變更時才能按', saved);
    await sleep(2500);

    const correctedRow = await page.evaluate(() => {
      const rows = Array.from(document.querySelectorAll('[data-testid="review-field"]'));
      const row = rows.find((r) => (r as HTMLElement).dataset.field === 'totalAmount');
      return {
        changed: (row as HTMLElement)?.dataset.changed,
        ocr: (row?.querySelector('[data-testid="ocr-value"]') as HTMLElement)?.innerText.trim(),
        final: (row?.querySelector('input') as HTMLInputElement)?.value,
      };
    });
    // 這三條合起來就是「三層值」的證明：改了最終值，**辨識值一個字都沒動**。
    check('改過的欄位被標記出來', correctedRow.changed === '1', correctedRow);
    check('辨識值仍是原值（校對不可覆蓋辨識值）', Number(correctedRow.ocr) === 127848, correctedRow);
    check('最終值是新值', Number(correctedRow.final) === 127900, correctedRow);
    await shot(page, 'K2-review-corrected');

    // 卡 G3：finance 沒有 `energy:audit:view`（`energy_rbac.rs:121-126`），/audit-events
    //    會回 403——還是這個 finance 身分、還在同一張主線單，點「稽核歷程」頁籤應該看到
    //    HF11 拍板的 B 文案，不是跟「真的沒資料」一樣的「沒有資料。」。
    const financeAuditTab = await page.evaluate(() => {
      const tab = Array.from(document.querySelectorAll('[role=tab]'))
        .find((t) => (t as HTMLElement).innerText.includes('稽核歷程'));
      if (!tab) return false;
      (tab as HTMLElement).click();
      return true;
    });
    check('finance 切得到稽核頁籤', financeAuditTab);
    await sleep(1200);
    const financeAuditText = await page.evaluate(
      () => (document.querySelector('[data-testid="review-audit"]') as HTMLElement)?.innerText || '',
    );
    check('finance 沒有稽核權限：看到「沒權限」的字，不是「沒有資料。」',
      financeAuditText.includes('你沒有檢視稽核紀錄的權限') && !financeAuditText.includes('沒有資料。'),
      financeAuditText.slice(0, 200));
    await shot(page, 'K2b-review-audit-forbidden');

    // K3 看稽核不需要用剛才改值的身分：finance 編輯主線單沒問題，但看
    // `/audit-events` 要 `gates::AUDIT_VIEW`（`audit/routes.rs:240`），這支
    // demo 角色不一定有；admin 兩者都有（上一版跑主線單時就驗過看得到稽核）。
    // 換回 admin 再開回同一張單——跟誰改值無關，K3 驗的是「這張單有稽核內容」。
    await loginAsAdmin(page);
    await selectOrg(page);
    await gotoPage(page, 'energyBillConsole', '單據清單');
    await sleep(900);
    // 金額欄這時已經被 K2 改成 127900.00，不能再用 127,848.00 找這一列——
    // 改用電號（00-60-1600-79-5，K1 段已經確認過是這張單的，編輯不會動到它）。
    const reopenedAsAdmin = await page.evaluate(() => {
      const row = Array.from(document.querySelectorAll('[data-testid="bill-row"]'))
        .find((tr) => (tr as HTMLElement).innerText.includes('00-60-1600-79-5'));
      if (!row) return false;
      (row as HTMLElement).click();
      return true;
    });
    check('K3 看稽核：admin 開得回主線單', reopenedAsAdmin);
    await sleep(2000);

    // 卡 G3：admin 有 `energy:audit:view`（`energy_rbac.rs:66-70`）→ 同一支 API 預期 200、有資料。
    const adminBillHash = await page.evaluate(() => window.location.hash);
    const adminBillId = adminBillHash.match(/energyBillConsole\/(\d+)/)?.[1] ?? '';
    const adminBillDetail = adminBillId ? await eapi(page, 'GET', `/bills/${adminBillId}`) : { status: 0, data: null };
    const adminExecId = (adminBillDetail.data as any)?.executions?.[0]?.id ?? '';
    const adminAuditApi = adminExecId
      ? await eapi(page, 'GET', `/audit-events?targetType=energyWorkflowExecution&targetId=${adminExecId}`)
      : { status: 0, data: null };
    // `eapi()` 是頁面內原生 fetch，不會經過 `web/src/api/energy.ts:66` 那條
    // `unwrapItems` adapter——後端這支本來就回 `{ items: [...] }`，前端的
    // `energyHttp` 透過 adapter 轉成裸陣列，這裡要自己仿一次，不是反過來懷疑產品。
    const adminAuditItems = (adminAuditApi.data as any)?.items;
    log('  卡 G3 AUDIT-403 admin:', JSON.stringify({ status: adminAuditApi.status, count: Array.isArray(adminAuditItems) ? adminAuditItems.length : null }).slice(0, 200));
    check('admin 打 /audit-events 回 200、有資料（直接量狀態碼）',
      adminAuditApi.status === 200 && Array.isArray(adminAuditItems) && adminAuditItems.length > 0, adminAuditApi);

    // 稽核頁籤：改值要留下紀錄，而且是**中文**。
    const auditTab = await page.evaluate(() => {
      const tab = Array.from(document.querySelectorAll('[role=tab]'))
        .find((t) => (t as HTMLElement).innerText.includes('稽核歷程'));
      if (!tab) return false;
      (tab as HTMLElement).click();
      return true;
    });
    check('切得到稽核頁籤', auditTab);
    await sleep(1200);
    const auditText = await page.evaluate(
      () => (document.querySelector('[data-testid="review-audit"]') as HTMLElement)?.innerText || '',
    );
    check('稽核有內容', auditText.length > 20, auditText.slice(0, 120));
    check('稽核顯示中文不是 raw type',
      !/flow\.|bill\.|payout\./.test(auditText), auditText.slice(0, 200));
    await shot(page, 'K3-review-audit');
    // L 段以後已經是 admin 身分（K3 前換回去了），不用再換一次。

    // ---------------------------------------------------------------
    hr('L. C4-1 Dashboard（P7）');
    await gotoPage(page, 'energyDashboardConsole', '總覽');
    check('Dashboard 內容 render 出來了', await waitFor(page, '[data-testid="kpi-card"]'));
    await sleep(1500);
    const dash = await page.evaluate(() => {
      const cards = Array.from(document.querySelectorAll('[data-testid="kpi-card"]'))
        .map((c) => (c as HTMLElement).innerText.replace(/\n/g, ' | '));
      return {
        cards,
        stationRows: document.querySelectorAll('[data-testid="station-row"]').length,
        text: document.body.innerText || '',
      };
    });
    /**
     * 六張（不是四張）：本月應付／進行中的流程／卡關／已放行未付／**待指認案場**／
     * **找不到生效合約**。最後那兩張是後來加的 —— 而這條斷言從那時起就一直紅著沒人看到。
     *
     * 卡 G3：卡 B commit 6b3c5aa 加了「找不到生效合約」那一格（workbench/mod.rs:91-97，
     * count review_status='pendingContract'）→ 原本五張變六張。
     */
    check('六張 KPI 卡都在', dash.cards.length === 6, dash.cards);
    // 卡 G3：順便核 6 個標題都在（不是只看張數）。
    check('六張 KPI 卡的標題都在（含「找不到生效合約」）',
      ['本月應付', '進行中的流程', '卡關', '已放行未付', '待指認案場', '找不到生效合約']
        .every((title) => dash.cards.some((c) => c.includes(title))), dash.cards);
    // demo seed 有 7 座案場（卡 A 20261118-0427-a 加了 s7 示範雙電號案場；
    // demo_seed.rs s1-s7：1 座異常、3 座建設中，其中 2 座（s5/s6）還沒有座標），
    // 加卡 G3：卡 D 第 1 輪審查修正 c1f3f57 加的專屬假案場「示範案場（本關要求）」
    // （demo_seed.rs:977-988，ensure_station(..., None, "")，那張 commit 訊息自己
    //  寫了「案場從 7 變 8」）→ 8 座。
    check('案場表有 8 列', dash.stationRows === 8, { rows: dash.stationRows });
    check('狀態是中文', dash.text.includes('運轉中') && dash.text.includes('異常'));
    // 卡關門檻由後端回，文案要寫出天數（不是一個沒頭沒尾的數字）。
    check('卡關卡片寫出門檻天數', /超過 \d+ 天/.test(dash.text), dash.cards);
    // demo seed 剩 1 件例外件（F 段指認掉一件）→ KPI 要對得上。
    check('待指認案場的數字對得上（1）',
      dash.cards.some((c) => c.includes('待指認案場') && /\b1\b/.test(c)), dash.cards);
    check('Dashboard 沒有 raw key', !/energy:|ryxis:/.test(dash.text));
    await shot(page, 'L1-dashboard');

    // ---------------------------------------------------------------
    hr('M. C4-2 行事曆（P8）');
    await gotoPage(page, 'energyCalendarConsole', '行事曆');
    await sleep(1200);
    // 切到清單檢視 —— demo seed 的事件在 2026-01~04，不一定落在「這個月」。
    const listed = await page.evaluate(() => {
      const btn = document.querySelector('[data-testid="calendar-view-list"]');
      if (!btn) return false;
      (btn as HTMLElement).click();
      return true;
    });
    check('切得到清單檢視', listed);
    await sleep(1500);
    const cal = await page.evaluate(() => ({
      rows: document.querySelectorAll('[data-testid="calendar-list-row"]').length,
      text: (document.querySelector('[data-testid="calendar-list"]') as HTMLElement)?.innerText || '',
    }));
    // demo seed 灌 3 筆分期事件 ＋ 卡 E（02e1e96）demo_seed.rs:650-664 加的「合約到期
    // （示範）」那一筆 = 4 筆（卡 G3：併回後沒人同步改這條，判定在本卡範圍內一併修）。
    check('清單有 4 筆事件', cal.rows === 4, cal);
    check('顯示地主名', cal.text.includes('林秀英'), cal.text.slice(0, 200));
    check('狀態是中文', /待處理|已完成|已逾期/.test(cal.text), cal.text.slice(0, 200));
    await shot(page, 'M1-calendar-list');

    // 月曆檢視：切到有事件的那個月，格子裡要看得到事件。
    const backToMonth = await page.evaluate(() => {
      const btn = document.querySelector('[data-testid="calendar-view-month"]');
      if (!btn) return false;
      (btn as HTMLElement).click();
      return true;
    });
    check('切得回月曆檢視', backToMonth);
    await sleep(800);
    const monthInput = await page.$('[data-testid="calendar-month"]');
    if (monthInput) {
      await monthInput.click({ clickCount: 3 });
      await monthInput.type('2026-03', { delay: 20 });
    }
    await sleep(1800);
    const grid = await page.evaluate(() => ({
      hasGrid: !!document.querySelector('[data-testid="calendar-grid"]'),
      days: document.querySelectorAll('[data-testid="calendar-day"]').length,
      events: document.querySelectorAll('[data-testid="calendar-event"]').length,
      // 用 data-date 驗第一格對不對 —— 時區弄錯的話整個月會偏一格，
      //    而且只有在某些時區看得出來。
      firstDate: (document.querySelector('[data-testid="calendar-day"]') as HTMLElement)?.dataset.date,
    }));
    check('月曆格子畫出來了', grid.hasGrid);
    check('2026-03 有 31 天', grid.days === 31, grid);
    check('第一格是 2026-03-01（時區沒把日期推掉）', grid.firstDate === '2026-03-01', grid);
    check('該月的事件出現在格子裡', grid.events >= 1, grid);
    await shot(page, 'M2-calendar-month');

    // ---------------------------------------------------------------
    hr('N. C4-3 稽核頁（P6）');
    await gotoPage(page, 'energyAuditConsole', '稽核紀錄');
    await sleep(1800);
    const audit = await page.evaluate(() => {
      const rows = Array.from(document.querySelectorAll('[data-testid="audit-row"]'));
      return {
        count: rows.length,
        text: rows.slice(0, 12).map((r) => (r as HTMLElement).innerText.replace(/\n/g, ' | ')),
        pageText: document.body.innerText || '',
      };
    });
    check('稽核有列', audit.count > 0, { count: audit.count });
    // 這是這頁最重要的一條：畫面上不可以出現 raw type。
    check('沒有 raw type（flow.xxx / bill.xxx / payout.xxx）',
      !/flow\.\w+|bill\.\w+|payout\.\w+|attachment\.\w+/.test(audit.pageText),
      audit.text.slice(0, 4));
    check('沒有 audit.type. 這種 i18n key', !/audit\.type\./.test(audit.pageText));
    /**
     * **句子換過了（2026-08-29）**：`audit.type.*` 從「只把事件名翻一次」
     * （「認領關卡」「完成關卡」）改成講出「對什麼做了什麼」
     * （「認領了這一關」「把電費單指認到案場」）。
     * 這一條跟著換 —— 它問的是「那一欄有沒有中文」，不是「是哪幾個字」。
     * 逐句的比對在 `crawler-energy-ops-audit-i18n`（中英各一輪）。
     */
    check('事件是中文', /認領了這一關|完成了這一關|流程開始跑|指認到案場|修正了電費單|由誰處理/.test(audit.pageText),
      audit.text.slice(0, 4));
    // 操作者是人名不是 id（本版的操作者是示範組織的「系統管理員」）。
    check('操作者顯示人名', audit.pageText.includes('系統管理員'), audit.text.slice(0, 4));
    await shot(page, 'N1-audit');

    // 篩選：選一個中文分組 → 列數應該變少（不是變 0 也不是不動）。
    const before2 = audit.count;
    await page.click('[data-testid="audit-group"]');
    await sleep(500);
    const pickedGroup = await page.evaluate(() => {
      const opt = Array.from(document.querySelectorAll('li[role=option]'))
        .find((li) => (li as HTMLElement).innerText.trim() === '電費單');
      if (!opt) return false;
      (opt as HTMLElement).click();
      return true;
    });
    check('分組下拉有「電費單」（不是 bill.）', pickedGroup);
    await sleep(1200);
    const after2 = await page.$$eval('[data-testid="audit-row"]', (els) => els.length);
    check('篩選後列數變少但不為 0', after2 > 0 && after2 < before2, { before: before2, after: after2 });
    await shot(page, 'N2-audit-filtered');

    // 展開一列 → payload 的差異對。
    const expanded = await page.evaluate(() => {
      const btn = document.querySelector('[data-testid="audit-row"] button');
      if (!btn) return false;
      (btn as HTMLElement).click();
      return true;
    });
    check('展得開明細', expanded);
    await sleep(900);
    await shot(page, 'N3-audit-detail');


    // ---------------------------------------------------------------
    hr('O. C5-1 地圖接真實案場（P7）');
    await gotoPage(page, 'energyDashboardConsole', '總覽');
    check('地圖容器出現', await waitFor(page, '[data-testid="station-map"] canvas'));
    // WebGL 場景要建 context ＋ 跑進場動畫（俯衝約 3s），等太短會拍到黑畫面。
    await sleep(9000);

    const map = await page.evaluate(() => {
      const info = document.querySelector('[data-testid="station-map-info"]') as HTMLElement | null;
      const host = document.querySelector('[data-testid="station-map"]') as HTMLElement | null;
      const canvas = host?.querySelector('canvas') as HTMLCanvasElement | null;
      return {
        present: !!host,
        hasCanvas: !!canvas,
        // 0×0 的 canvas ＝ context 沒建起來（swiftshader 那個坑的症狀）。
        canvasW: canvas?.width ?? 0,
        canvasH: canvas?.height ?? 0,
        plotted: info?.dataset.plotted ?? '',
        skipped: info?.dataset.skipped ?? '',
        text: (host as HTMLElement)?.innerText || '',
      };
    });
    check('地圖元件在', map.present);
    check('WebGL canvas 建起來了（不是 0×0）', map.hasCanvas && map.canvasW > 100 && map.canvasH > 100, map);
    // 絕對數字：demo seed 有 8 座案場（卡 A 加了 s7；卡 G3 的卡 D c1f3f57 又加了
    //    一座假案場 coords=None），其中 **5 座有座標、3 座沒有**。
    //    WebGL 裡面讀不回來，只能靠場景回拋的計數 —— 這是唯一能證明「點真的畫出去」的訊號。
    check('地圖上畫了 5 個點', map.plotted === '5', map);
    check('3 座沒座標的**沒有**被標在 (0,0)', map.skipped === '3', map);
    check('圖例三個狀態都在', /運轉中/.test(map.text) && /建設中/.test(map.text) && /異常/.test(map.text), map.text);
    await shot(page, 'O1-station-map');

    // **畫面上真的看得到嗎** —— 計數對不代表畫得出來。
    //
    // 這條是真的救回一次：第一版案場層沿用 `pal.alpha.cone`，而 MONO 主題
    //    把它設成 **0**（那是刻意的，它是背景圖層）。結果是「點都建好了、
    //    `data-plotted` 也回 4、console 乾乾淨淨」，但畫面上一個點都沒有。
    //    只靠計數的斷言完全攔不住這種事。
    //
    // 作法（`VISUAL_VERIFICATION.md` 那一套）：外層截圖 → 頁內 decode → 數像素。
    // 不用 `canvas.toDataURL()`：沒開 `preserveDrawingBuffer` 時讀回來是空白。
    const mapBox = await page.evaluate(() => {
      const c = document.querySelector('[data-testid="station-map"] canvas');
      if (!c) return null;
      const r = c.getBoundingClientRect();
      return {
        x: Math.round(r.x), y: Math.round(r.y), w: Math.round(r.width), h: Math.round(r.height),
      };
    });
    check('地圖容器有實際尺寸', !!mapBox && mapBox.w > 300 && mapBox.h > 200, mapBox);

    if (mapBox) {
      const shotB64 = await page.screenshot({ encoding: 'base64' });
      const pixels = await page.evaluate(async (d, box) => new Promise<any>((res) => {
        const img = new Image();
        img.onload = () => {
          const cv = document.createElement('canvas');
          cv.width = box.w; cv.height = box.h;
          const g = cv.getContext('2d')!;
          // 截圖是 CSS 像素座標（deviceScaleFactor=1），直接切出地圖那一塊。
          g.drawImage(img, box.x, box.y, box.w, box.h, 0, 0, box.w, box.h);
          const { data } = g.getImageData(0, 0, box.w, box.h);
          let nonWhite = 0;
          let colored = 0;
          for (let i = 0; i < data.length; i += 4) {
            const r = data[i]; const gg = data[i + 1]; const b = data[i + 2];
            // 白底面板：離白夠遠才算「有畫東西」。
            if (r < 244 || gg < 244 || b < 244) nonWhite += 1;
            // 有彩度的像素 ＝ 標記/邊界的顏色（灰階的背景與陰影不算）。
            const mx = Math.max(r, gg, b); const mn = Math.min(r, gg, b);
            if (mx - mn > 26 && mx > 40) colored += 1;
          }
          const total = box.w * box.h;
          res({ nonWhitePct: +(nonWhite / total * 100).toFixed(2), coloredPct: +(colored / total * 100).toFixed(3) });
        };
        img.src = `data:image/png;base64,${d}`;
      }), shotB64, mapBox);

      // 台灣本島 + 邊界至少要佔掉個位數百分比；整張近乎全白就是沒畫出來。
      check('地圖不是空白（非白像素 > 3%）', pixels.nonWhitePct > 3, pixels);
      // 有彩度的像素 ＝ 案場標記真的看得到（狀態色：綠/金/燒橘）。
      //
      // **門檻是量出來的，不是猜的**（2026-08-27，MONO 灰底 ＝ 正式主題）：
      //      隱形（`STATION_ALPHA` 設 0）→ **0.000%**
      //      正常（alpha 0.95）        → **0.034%**
      //    取中間的 0.015%：離 0 有兩倍餘裕、離實測值也有兩倍餘裕。
      //
      // 這個數字**跟主題綁在一起**：紫色（`jv`，只存在於 demo）會多出 22 個縣市錐
      //    的顏色，量到的是 0.6%＋。換主題就要重新量，不要沿用。
      check('案場標記在畫面上看得到（有彩度像素 > 0.015%）', pixels.coloredPct > 0.015, pixels);
    }

    // 回歸：C5 動過 `geo.ts`（把 `project` 抽成 `projectLonLat`）與 `createScene`
    //    （加 `stations` / `restingDistance`）—— 既有的 `#/taiwan-map` 那頁**不可以壞**。
    //    那頁的預設取景（r=11.5）與 MONO 主題都不該被動到。
    await page.goto(`${APP}#/taiwan-map`, { waitUntil: 'networkidle2' });
    await waitFor(page, 'canvas');
    await sleep(9000);
    const legacy = await page.evaluate(() => {
      const c = document.querySelector('canvas') as HTMLCanvasElement | null;
      return { hasCanvas: !!c, w: c?.width ?? 0, h: c?.height ?? 0, cam: (document.querySelector('[data-cam]') as HTMLElement)?.dataset.cam ?? '' };
    });
    check('既有的台灣圖層頁沒有壞', legacy.hasCanvas && legacy.w > 100, legacy);
    // 預設 restingDistance 仍是 11.5（沒被面板那個 8 影響）。
    check('既有頁的取景距離仍是 11.5',
      !!legacy.cam && Math.abs(Number(legacy.cam.split(',')[0]) - 11.5) < 0.6, legacy.cam);
    await shot(page, 'O2-legacy-taiwan-map');

    // ---------------------------------------------------------------
    hr('P. C5-2 由地址推估座標');
    // 上一段跑去看 `#/taiwan-map` 了 —— **要先回總覽**。
    //    忘了回來的話這一整段會查在錯的頁上，症狀是「按鈕 0 顆」，
    //    跟「功能沒做出來」長得一模一樣。
    await gotoPage(page, 'energyDashboardConsole', '總覽');
    await waitFor(page, '[data-testid="geocode-btn"]');
    // demo seed 有兩座「只有地址、沒有座標」的案場 → 兩顆按鈕。
    const geoBtns = await page.$$eval('[data-testid="geocode-btn"]', (els) => els.length);
    check('只有沒座標的案場才有推估按鈕（2 顆）', geoBtns === 2, { geoBtns });

    // ① 有門牌的那座 → 可以採用
    const openedOk = await page.evaluate(() => {
      const row = Array.from(document.querySelectorAll('[data-testid="station-row"]'))
        .find((tr) => (tr as HTMLElement).innerText.includes('屏東枋寮三號'));
      const btn = row?.querySelector('[data-testid="geocode-btn"]');
      if (!btn) return false;
      (btn as HTMLElement).click();
      return true;
    });
    check('打得開推估對話框（有門牌的）', openedOk);
    await sleep(2000);
    const okDlg = await page.evaluate(() => ({
      coords: (document.querySelector('[data-testid="geocode-coords"]') as HTMLElement)?.innerText || '',
      blocked: !!document.querySelector('[data-testid="geocode-blocked"]'),
      applyDisabled: (document.querySelector('[data-testid="geocode-apply"]') as HTMLButtonElement)?.disabled,
      text: (document.querySelector('[data-testid="geocode-dialog"]') as HTMLElement)?.innerText || '',
    }));
    check('回得出座標', /22\.36/.test(okDlg.coords), okDlg);
    check('精度寫的是中文', /門牌/.test(okDlg.text), okDlg.text);
    check('「採用」可以按', okDlg.applyDisabled === false, okDlg);
    await shot(page, 'P1-geocode-ok');

    const applied = await page.evaluate(() => {
      const btn = document.querySelector('[data-testid="geocode-apply"]') as HTMLButtonElement | null;
      if (!btn || btn.disabled) return false;
      btn.click();
      return true;
    });
    check('採用送出', applied);
    await sleep(3000);
    // 真的寫進去了嗎 —— 重新載入再數一次地圖上的點。
    await gotoPage(page, 'energyDashboardConsole', '總覽');
    await waitFor(page, '[data-testid="station-map"] canvas');
    await sleep(9000);
    const after3 = await page.evaluate(() => {
      const info = document.querySelector('[data-testid="station-map-info"]') as HTMLElement | null;
      return {
        plotted: info?.dataset.plotted ?? '',
        skipped: info?.dataset.skipped ?? '',
        btns: document.querySelectorAll('[data-testid="geocode-btn"]').length,
        sources: Array.from(document.querySelectorAll('[data-testid="coord-source"]'))
          .map((e) => (e as HTMLElement).innerText.trim()),
      };
    });
    // 卡 A 加了 s7（本來就有座標，算人工標定）：原本 5 manual＋2 無座標，
    // 這裡採用一座（屏東枋寮三號，mock 地址推估）之後變 6 個點、剩 1 座沒座標；
    // 卡 G3 的假案場（卡 D c1f3f57，coords=None、地址是空字串）沒有推估按鈕
    // （下面「推估按鈕剩 1 顆」PASS 對得上），推不掉、也採用不到 → 沒座標的剩 2 座。
    check('採用後地圖上變成 6 個點（重整仍成立＝真的寫進 DB）', after3.plotted === '6', after3);
    check('沒座標的剩 2 座', after3.skipped === '2', after3);
    check('推估按鈕剩 1 顆', after3.btns === 1, after3);
    // 來源要看得出來：5 座 manual ＋ 1 座 mock。
    check('座標來源標得出「人工標定」與「地址推估」',
      after3.sources.filter((t) => t.includes('人工標定')).length === 5
      && after3.sources.filter((t) => t.includes('地址推估')).length === 1,
      after3.sources);
    await shot(page, 'P2-after-apply');

    // ② 地段地號那座 → 精度不足，不可採用
    const openedCoarse = await page.evaluate(() => {
      const row = Array.from(document.querySelectorAll('[data-testid="station-row"]'))
        .find((tr) => (tr as HTMLElement).innerText.includes('台南七股一號'));
      const btn = row?.querySelector('[data-testid="geocode-btn"]');
      if (!btn) return false;
      (btn as HTMLElement).click();
      return true;
    });
    check('打得開推估對話框（地段地號）', openedCoarse);
    await sleep(2000);
    const coarseDlg = await page.evaluate(() => ({
      blocked: !!document.querySelector('[data-testid="geocode-blocked"]'),
      applyDisabled: (document.querySelector('[data-testid="geocode-apply"]') as HTMLButtonElement)?.disabled,
      text: (document.querySelector('[data-testid="geocode-dialog"]') as HTMLElement)?.innerText || '',
    }));
    // 這是 §1.2.1 約束 1＋2 的畫面版：分數再高，只到行政區就是不能用。
    check('地段地號被擋下來', coarseDlg.blocked === true, coarseDlg);
    check('「採用」不可按', coarseDlg.applyDisabled === true, coarseDlg);
    check('說明講出「請手動標定」', /手動標定/.test(coarseDlg.text), coarseDlg.text);
    await shot(page, 'P3-geocode-blocked');
    await page.evaluate(() => {
      const btns = Array.from(document.querySelectorAll('.MuiDialogActions-root button'));
      (btns[0] as HTMLElement)?.click();
    });
    await sleep(600);

    // ---------------------------------------------------------------
    hr('I. 每一頁都不得洩漏 uniqueName');
    for (const [hash, title] of [
      ['energyMyWorkConsole', '我的工作'], ['energyBillConsole', '單據清單'],
      ['energyLandownerConsole', '地主'],
      ['energyDashboardConsole', '總覽'], ['energyCalendarConsole', '行事曆'],
      ['energyAuditConsole', '稽核紀錄'],
    ] as Array<[string, string]>) {
      await gotoPage(page, hash, title);
      await sleep(600);
      const leaks = await leakedPrefixes(page);
      check(`${title}：沒有 energy:/ryxis: 字串`, leaks.length === 0, leaks);
    }

    // ---------------------------------------------------------------
    hr('C. console');
    // React Admin 的 i18n missing-key 警告與自簽憑證噪音不算，其他 error 都算。
    log('  403 的請求：', JSON.stringify([...new Set(forbidden)]));
    // 登入前 RA 會先探一次 /api/sessions/me（authProvider.checkError 的 403 探針），
    //    那一發必然 403，是設計行為不是 bug。其餘 403 才算。
    // H／J 段改用 finance@ 看校對台（Q-04：platform-review 候選人只剩 bill-reviewers，
    //    admin 不是候選人了）：finance 沒有 `energy:audit:view`（跟 modes.ts 的註解
    //    同一件事），BillDetailBody 背景拉 audit-events 分頁資料必然 403，前端已經
    //    `.catch` 吞掉不顯示錯誤，是設計行為不是 bug。
    const realForbidden = [...new Set(forbidden)].filter((u) => !u.endsWith('/api/sessions/me')
      && !u.endsWith('/audit-events'));
    check('沒有非預期的 403', realForbidden.length === 0, realForbidden);
    // 本版沒有 403 探針；對應的是登入頁先問一次 `/api/v1/auth/session`（未登入 → 401，checkAuth 的設計行為）。
    log('  401 的請求：', JSON.stringify([...new Set(unauthorized)]));
    const realUnauthorized = [...new Set(unauthorized)].filter((u) => u !== 'GET /api/v1/auth/session');
    check('沒有非預期的 401', realUnauthorized.length === 0, realUnauthorized);
    const realErrors = consoleErrors.filter(
      (e) => !/favicon|net::ERR_CERT|Download the React DevTools|Missing translation/.test(e)
        && !(realForbidden.length === 0 && /status of 403/.test(e))
        && !(realUnauthorized.length === 0 && /status of 401/.test(e)),
    );
    check('console 無 error', realErrors.length === 0, realErrors.slice(0, 5));
  } finally {
    await browser.close();
  }

  board.finish('租戶後台能源頁');
}

run().catch((e) => {
  console.error(e);
  process.exitCode = 1;
});
