/* eslint-disable no-console, no-await-in-loop */
/*
C7 的驗收：**多帳號走完整鏈路 ＋ 身分對不對**（移植自 az `scripts/crawler-energy-ops-roles`）。

  承辦送出 → 主管核准 → 財務放行 → 會計付款 → 完成

跑法：`web/e2e/run-crawlers.sh roles`（示範資料不灌範本電費單：單據全部從收件台走一遍）。
帳號是示範資料的 `<role>@demo.energy`／`demo12345`。

**為什麼一定要多帳號**：單一帳號走不完 —— admin 送出的單，防自核會把他自己排除在下一關之外。
而且「P1 依角色變形」與「登入落點依角色」用一個身分測，覆蓋率是 0。

驗收不是「頁面有沒有渲染」，而是**點得到、身分對、按鈕不說謊**。
*/
import path from 'node:path';
import type { Page } from 'puppeteer';
import {
  Scoreboard, hr, log, sleep, launch, shooter, loginAs, loginAsAdmin, selectOrg, goto, retryOnDetach, hardenPage,
  waitIdle, seedBatch, eapi, demoEmail,
} from '../lib/energyOpsHarness.ts';

const board = new Scoreboard();
const check = board.check.bind(board);
const shot = shooter('energy-roles');

/**
 * 4xx 記錄 —— **module level**，因為 `actOnWorkbench` / `actOnNode`
 * 要用它判斷「動作有沒有真的被後端接受」。
 * 只看 5xx 的話，「按鈕按了沒反應」永遠查不出原因：
 * 被擋下來的動作回的是 **409/403**，不是 500。
 */
const clientErrors: string[] = [];

/**
 * **動作真的生效了嗎** —— 不是「按鈕點得到」。
 *
 * 這支存在的理由是一次真實的假綠燈：
 * `actOnWorkbench()` 原本點完按鈕就 `return true`，
 * 於是「認領沒成功 → 動作 403」被回報成**成功**，
 * 而那一步在 crawler 裡只有 `log()` 沒有 `check()` ——
 * 結果一個假成功**連鎖出 12 條紅**，紅的位置全都不是根因。
 *
 * 判準：這段期間有沒有 task 動作類的 4xx。
 */
function actionAccepted(sinceIndex: number): boolean {
  return !clientErrors.slice(sinceIndex)
    .some((e) => /\/tasks\/\d+\/(actions|claim|release)/.test(e));
}


/**
 * 開一批送件並停在它的詳情頁：走 harness 的 `seedBatch`（真的開批、真的上傳，辨識結果是我們給的）。
 * 參數收檔名：basename 就是這張紙的身分，兩個 basename 相同＝同一批裡兩張同一張紙。
 */
async function uploadNewBatch(page: Page, ...files: string[]): Promise<{ ok: boolean; batchId: number }> {
  return seedBatch(page, files.map((f) => path.basename(f)));
}

async function sectionsOf(page: Page): Promise<string[]> {
  // 前一版的 `catch` 是**空的**：吞掉所有錯誤、三次後回 `[]`。
  // 那會讓斷言看到「工作台沒有任何分區」——**跟產品壞掉長得一模一樣**。
  // 現在只對 detached 重試，其他錯誤照拋（看得到真正的原因）。
  return retryOnDetach('sectionsOf', async () => page.$$eval(
    '[data-testid="workbench-section"]',
    (els) => els.map((e) => (e as HTMLElement).dataset.section ?? ''),
  ), 3, page);
}

/** 這個身分在工作台上能按的動作。 */
const workbenchActions = (page: Page) => page.evaluate(() => Array.from(
  document.querySelectorAll('[data-testid="action-button"]'),
).map((b) => (b as HTMLElement).innerText.trim()));

/**
 * 在工作台把一筆推進一關（先認領再按）。
 *
 * **動線在【9】改過**（`LIFECYCLE.md §8.5`）：〔認領〕不在列上了 ——
 * 它是「開始處理」的第一步，只存在於抽屜裡。
 * 前一版在整頁找一顆文字等於「認領」的按鈕，改版後找不到就靜默跳過，
 * 於是沒認領就去按動作 → 403，而畫面看起來像「這一關沒有動作按鈕」。
 */
async function actOnWorkbench(page: Page, label: string, billId?: number): Promise<boolean> {
  const since = clientErrors.length;
  /**
   * 整列可點 → 開抽屜。**給了 `billId` 就只點那一張單的列**。
   *
   * 不指名的後果不是「慢一點」而是**推進別人的單**：
   * 佇列裡本來就會有 demo seed 自己發起的那一筆，
   * 而症狀是「我的單怎麼都不動」——看起來像核准功能壞了。
   */
  const opened = await page.evaluate((bid: number) => {
    const rows = Array.from(document.querySelectorAll('[data-testid="workbench-row"]'));
    const row = (bid
      ? rows.find((r) => (r as HTMLElement).dataset.bill === String(bid))
      : rows[0]) as HTMLElement | undefined;
    if (!row) return false;
    row.click();
    return true;
  }, billId ?? 0);
  if (!opened) return false;
  await waitIdle(page);
  // 抽屜裡認領（未認領時才有這顆）
  const claimBtn = await page.$('[data-testid="queue-claim"]');
  if (claimBtn) {
    await page.evaluate(() => {
      (document.querySelector('[data-testid="queue-claim"]') as HTMLElement | null)?.click();
    });
    // 認領要等後端回來 —— 太快按下一步會在「還沒認領」的狀態送出 → 403。
    await waitIdle(page);
  }
  // 抽屜裡按動作，用**並關閉**那一顆（這支要的是推進一關然後回列表）。
  const clicked = await page.evaluate((t) => {
    const b = Array.from(document.querySelectorAll(
      '[data-testid="queue-act-close"], [data-testid="queue-act-other"]',
    )).find((x) => (x as HTMLElement).innerText.includes(t));
    if (!b) return false;
    (b as HTMLElement).click();
    return true;
  }, label);
  if (!clicked) return false;
  await waitIdle(page);
  await page.keyboard.press('Escape');
  await waitIdle(page);
  // **點得到 ≠ 做得到** —— 認領失敗時動作會 403，前一版照樣回 true。
  return actionAccepted(since);
}

/**
 * **指名關卡、指名動作**地推一步。
 *
 * 為什麼不沿用上面那個 `actOnWorkbench(label)`：
 * 它按的是**第一個**「認領」與**第一顆文字相符**的按鈕。
 * 只有一張單在飛的時候看起來很穩，一旦同時有兩張（小額那張還沒收尾、
 * 高額那張剛送出）就會**推到另一張單上**，而且兩次跑的結果會不一樣
 * —— 實測過：同一支腳本兩次跑，高額單一次停在 `exec-approve`、一次停在 `manager-approve`。
 * 這種不穩定會被誤讀成「功能有 bug」。
 *
 * 所以這裡：① 先用 `data-node` 鎖定**那一關的那一列**，
 * ② 認領與動作都**只在那一列裡面**找，
 * ③ 動作用 `data-action`（actionKey）比對，不比中文字（文案會改）。
 */
async function actOnNode(
  page: Page, nodeKey: string, actionKey: string, billId?: number,
): Promise<boolean> {
  const since = clientErrors.length;
  for (let attempt = 0; attempt < 2; attempt += 1) {
    /**
     * 整段包 `retryOnDetach` ——【7】修了 `selectOrg` / `sectionsOf` /
     * `nodeOf` / `mapProbe`，**漏了這一支**，於是它照樣會讓整支腳本 throw
     * （這一輪就是死在這裡，15 條之後、0 failed）。
     */
    // eslint-disable-next-line no-await-in-loop
    const done = await retryOnDetach('actOnNode', async () => {
      await page.reload({ waitUntil: 'networkidle2' });
      await sleep(1500);
      /**
       * ① 開**那一關那一列**的抽屜。
       *
       * 動線在【9】改過：〔認領〕不在列上了，整列可點會開抽屜。
       * 但這支存在的理由沒變 —— **鎖定 `data-node`**，
       * 不然同時有兩張單在飛的時候會推到錯的那一張，
       * 而那種不穩定會被誤讀成「功能有 bug」。
       */
      const opened = await page.evaluate((args: { nk: string; bid: number }) => {
        const row = Array.from(document.querySelectorAll('[data-testid="workbench-row"]'))
          .find((r) => {
            const el = r as HTMLElement;
            if (el.dataset.node !== args.nk) return false;
            // 給了單號就再鎖一層：同一關上可能同時有**別人的**單
            // （demo seed 自己就有一筆停在承辦確認關）。
            return args.bid ? el.dataset.bill === String(args.bid) : true;
          }) as HTMLElement | undefined;
        if (!row) return false;
        row.click();
        return true;
      }, { nk: nodeKey, bid: billId ?? 0 });
      if (!opened) return false;
      await waitIdle(page);

      // ② 抽屜裡認領（未認領才有這顆）
      const claim = await page.$('[data-testid="queue-claim"]');
      if (claim) {
        await page.evaluate(() => {
          (document.querySelector('[data-testid="queue-claim"]') as HTMLElement | null)?.click();
        });
        await waitIdle(page);
      }

      // ③ 抽屜裡按動作，比 **actionKey** 不比中文字（文案會改）。
      const ok = await page.evaluate((ak: string) => {
        const b = document.querySelector(
          `[data-testid="queue-act-close"][data-action="${ak}"],`
          + `[data-testid="queue-act-other"][data-action="${ak}"]`,
        ) as HTMLElement | null;
        if (!b) return false;
        b.click();
        return true;
      }, actionKey);
      if (!ok) return false;
      await waitIdle(page);
      await page.keyboard.press('Escape');
      await waitIdle(page);
      return true;
    }, 3, page);
    // 同 `actOnWorkbench`：**點得到 ≠ 做得到**（403/409 也要算失敗）。
    if (done && actionAccepted(since)) return true;
  }
  return false;
}

/**
 * 某張單目前的節點與狀態（驗收看這個，不看畫面上有沒有按鈕）。
 *
 * **帶重試** —— 這支常常緊接在 `loginAs()` 之後被呼叫，
 * 而那時 frame 可能還在換頁中途 → `Attempted to use detached Frame` **直接 throw**，
 * 整支腳本中斷（不是紅一條，是**後面全部不跑**）。
 * 同 `goto()` / `selectOrg()` 的處置：不要只在「重跑就過」時聳肩帶過，
 * 那會讓每一輪驗收都有機率白跑。
 */
async function nodeOf(page: Page, billId: number): Promise<{ node: string; status: string }> {
  return retryOnDetach('nodeOf', async () => page.evaluate(async (bid: number) => {
        const org = (window as any).__demoOrgId;
        const d = await fetch(`/api/v1/orgs/${org}/energy/bills/${bid}`, { credentials: 'include' });
        const detail = d.ok ? await d.json() : null;
    const ex = detail?.executions?.[0];
    return ex
      ? { node: ex.currentNodeKey as string, status: ex.status as string }
      : { node: '', status: 'none' };
  }, billId), 3, page);
}

async function run() {
  // Q-04（2026-11-16）：`bill-approval` 換成單一審核，沒有任何金額分支
  // （舊版 138,195 ≥ 門檻會多一關「高層加簽」，現在不存在，不自己發明替代流程）。
  // `-small` 這個檔名前綴留著只是沿用 fixture 命名，金額本身跟路線無關了。
  //
  // **檔名就是這張紙的身分**（`ingestFixtures.periodForName`）：mock 的計費期間
  // 由檔名的穩定雜湊決定，所以**同檔名＝同一張電費單**、換檔名＝另一張。
  // 這一輪加上 `businessKey` 去重之後，這件事變得會影響驗收：
  // 三個上傳點如果共用同一個檔名，第二次送審就會被 409 擋下來 ——
  // 而那不是產品壞了，是驗收資料把三張紙做成了同一張。
  const smallFile = path.join('roles', 'bill-small.png');
  // ① 用的那張要與 ⑨ 分開（兩張都會**送審**）；`-small` 前綴留著，
  // 因為 fixture 靠它決定金額（60,000 < 加簽門檻）。
  const billFile = path.join('roles', 'bill-small-first.png');
  // 去重驗收專用：與 `billFile` **同一張紙**（同檔名不同目錄），
  // 用來製造「別的地方已經有一筆在跑／已通過」的撞單。
  const dupDir = path.join('roles', 'dup');
  const dupOfFirst = path.join(dupDir, 'bill-small-first.png');
  // A1 驗收（離開再回來）用的獨立一張。
  const revisitFile = path.join('roles', 'bill-small-revisit.png');
  // ⑫ 增補 B 專用：一張**沒送審**的單（`intake` 模式才改得動金額）。
  const reviewFile = path.join('roles', 'bill-small-review.png');
  // 同批重複驗收：**兩個不同路徑、同一個檔名** ——
  // 一次 `uploadFile(a, b)` 就會在**同一批**裡產生兩份同一張紙。
  const twinA = path.join('roles', 'twin-a');
  const twinB = path.join('roles', 'twin-b');
  const twinFileA = path.join(twinA, 'bill-small-twin.png');
  const twinFileB = path.join(twinB, 'bill-small-twin.png');

  const browser = await launch();
  // `hardenPage`：把 `page.evaluate` 包上 detach 復原、裝上在途請求計數。
  const page = hardenPage(await browser.newPage());
  const serverErrors: string[] = [];
  // 4xx 也要記 —— 只看 5xx 的話，「按鈕按了沒反應」永遠查不出原因：
  // 被擋下來的動作回的是 **409/403**，不是 500。
  page.on('response', (r) => {
    const p = new URL(r.url()).pathname;
    if (r.status() >= 500) serverErrors.push(`${r.status()} ${p}`);
    else if (r.status() >= 400 && /^\/api\/v1\/orgs\/\d+\/energy\//.test(p)) {
      clientErrors.push(`${r.status()} ${r.request().method()} ${p}`);
    }
  });

  try {
    // ── ① 承辦：落點 ＋ 收件 ＋ 送審 ────────────────────────────
    hr('① 承辦（operator@）—— 登入落點 ＋ 送審');
    await loginAs(page, demoEmail('operator'));
    check('選得到 demo 組織（不是新用戶的私人 org）', (await selectOrg(page)) != null);

    // 驗收 3：落點依角色 —— 承辦落在**收件台**。
    const landing = await eapi(page, 'GET', '/me/landing').then((r) => (r.status === 200 ? r.data : { error: r.status }));
    check('承辦的落點是送件', landing?.route === 'energyIngestConsole', landing);

    // 合併後：上傳 → 自動進那一批的詳情頁（校對工作區）。
    const firstBatch = await uploadNewBatch(page, billFile);
    check('進得了送件並開出一批', firstBatch.ok, firstBatch);
    const submitted = await page.evaluate(() => {
      const b = document.querySelector('[data-testid="intake-submit-one"]');
      if (!b) return false;
      (b as HTMLElement).click();
      return true;
    });
    check('承辦送審', submitted);
    await waitIdle(page);
    /**
     * **記下這一輪自己的那張單**。
     *
     * 工作台上本來就會有別人的東西（demo seed 自己就發起了一筆停在承辦確認關的
     * 執行）。不指名的話 `actOnWorkbench` 會推**第一列** —— 也就是別人的單，
     * 然後這一支後面每一條斷言都在問「為什麼我的單還停在原地」。
     * 2026-09-08 實際踩到：10 條紅燈，根因就是這個。
     */
    const mainBillId: number = await page.evaluate(async (bid: number) => {
      const org = (window as any).__demoOrgId;
      const r = await fetch(`/api/v1/orgs/${org}/energy/ingest-batches/${bid}`, { credentials: 'include' });
      if (!r.ok) return 0;
      const b = await r.json();
      const it = (b.items ?? []).find((i: any) => i.billId != null);
      return it ? Number(it.billId) : 0;
    }, firstBatch.batchId);
    log(`  這一輪的主線單：#${mainBillId}`);
    await shot(page, '1-operator-submitted');

    /**
     * **Q-04（2026-11-16）：bill-approval 換成單一審核**。
     *
     * 候選人只剩部門 `bill-reviewers`（manager／finance／accounting），
     * operator（發起人）本來就不在那個部門裡 —— 送審之後他的工作台上
     * **沒有**這一筆（不是候選人；R4 又把發起人標成 excludedOwner，
     * 兩個理由同時成立，畫面上只看得到「沒有這一筆」這個結果）。
     *
     * 舊版「`clerk-review.allowSelfApproval` 翻成 true，發起人看得到送出」
     * 那段不適用了：單一審核沒有改單關卡，送審直接進 `platform-review`。
     */
    check('進得了我的工作', await goto(page, 'energyMyWorkConsole', '[data-testid="workbench-section"], .MuiAlert-root'));
    await waitIdle(page);
    const opHasRow = await page.evaluate((bid: number) => Array.from(
      document.querySelectorAll('[data-testid="workbench-row"]'),
    ).some((r) => (r as HTMLElement).dataset.bill === String(bid)), mainBillId);
    check('operator 工作台上沒有這一筆（不是候選人，platform-review 只收 bill-reviewers）',
      !opHasRow, { mainBillId, opHasRow });

    // ── ② 主管：看得到、不按 ────────────────────────────────────
    hr('② 主管（manager@）—— bill-reviewers 候選人之一，看得到但不按');
    await loginAs(page, demoEmail('manager'));
    await selectOrg(page);
    check('主管進得了我的工作', await goto(page, 'energyMyWorkConsole', '[data-testid="workbench-section"], .MuiAlert-root'));
    await waitIdle(page);
    const mgrSections = await sectionsOf(page);
    log(`  主管看到的分區：${JSON.stringify(mgrSections)}`);
    check('部門候選人生效：主管的工作台上有事做', mgrSections.length > 0, mgrSections);
    const mgrHasRow = await page.evaluate((bid: number) => Array.from(
      document.querySelectorAll('[data-testid="workbench-row"]'),
    ).some((r) => (r as HTMLElement).dataset.bill === String(bid)), mainBillId);
    check('主管的工作台上看得到這一筆（bill-reviewers 候選人之一）', mgrHasRow, { mainBillId });
    await shot(page, '2-manager-workbench');

    // ── ③ 財務：〔審核通過〕───────────────────────────────────
    hr('③ 財務（finance@）—— 審核通過（單一審核：任一位通過即完成）');
    await loginAs(page, demoEmail('finance'));
    await selectOrg(page);
    await goto(page, 'energyMyWorkConsole', '[data-testid="workbench-section"], .MuiAlert-root');
    await waitIdle(page);
    const finSections = await sectionsOf(page);
    log(`  財務看到的分區：${JSON.stringify(finSections)}`);
    const approved = await actOnWorkbench(page, '審核通過', mainBillId);
    check('財務〔審核通過〕得動', approved, finSections);
    await shot(page, '3-finance-workbench');

    // ── ④ 會計：落點仍是「我的工作」，但這一筆已經不在佇列裡 ──────
    /**
     * 單一審核：任一位 bill-reviewers 通過即完成，不是四眼依序簽核，
     * 所以 finance 按完之後，accounting（同一部門的另一位）工作台上
     * **不會**還留著這一筆（它已經 completed，不是輪到他）。
     * 刪「會計第一區是等我付款」「標記已付款」：單一審核沒有付款關。
     */
    hr('④ 會計（accounting@）—— 落點 ＋ 這一筆已不在工作台');
    await loginAs(page, demoEmail('accounting'));
    await selectOrg(page);

    const acctLanding = await eapi(page, 'GET', '/me/landing').then((r) => (r.status === 200 ? r.data : { error: r.status }));
    check('會計的落點是「我的工作」', acctLanding?.route === 'energyMyWorkConsole', acctLanding);

    await goto(page, 'energyMyWorkConsole', '[data-testid="workbench-section"], .MuiAlert-root');
    await waitIdle(page);
    const acctHasRow = await page.evaluate((bid: number) => Array.from(
      document.querySelectorAll('[data-testid="workbench-row"]'),
    ).some((r) => (r as HTMLElement).dataset.bill === String(bid)), mainBillId);
    check('會計的工作台上已經沒有這一筆（財務通過，其他人不必再審）',
      !acctHasRow, { mainBillId, acctHasRow });
    await shot(page, '4-accounting-workbench');

    // ── ⑤ 鏈路走到哪 ────────────────────────────────────────
    hr('⑤ 鏈路結果');
    await loginAs(page, demoEmail('operator'));
    await selectOrg(page);
    const final = await page.evaluate(async () => {
      // 用 `orgIsDemo` 找到的那個 org，不猜 1。
      const org = (window as any).__demoOrgId;
      const r = await fetch(`/api/v1/orgs/${org}/energy/bills?limit=5`, { credentials: 'include' });
      const bills = r.ok ? (await r.json()).items ?? [] : [];
      const b = bills[0];
      if (!b) return { note: 'no bills' };
      const d = await fetch(`/api/v1/orgs/${org}/energy/bills/${b.id}`, { credentials: 'include' });
      const detail = d.ok ? await d.json() : null;
      const ex = detail?.executions?.[0];
      if (!ex) return { note: 'no execution' };
      const g = await fetch(`/api/v1/orgs/${org}/energy/executions/${ex.id}/graph`, { credentials: 'include' });
      const graph = g.ok ? await g.json() : null;
      return { status: ex.status, current: graph?.current ?? [], node: ex.currentNodeKey };
    });
    log(`  最終狀態：${JSON.stringify(final)}`);
    // 驗收 1：**整條鏈路走完** —— 承辦送審 → 財務審核通過（單一審核）→ 完成。
    // 「推過了第一關」不夠：那只證明多帳號有用，不證明鏈路是通的。
    check('完整鏈路走到 end（status=completed）',
      final?.status === 'completed' && final?.node === 'end', final);

    // ── ⑥ CXO（cxo@）—— 落點（Q-04：單一審核沒有金額門檻加簽） ──────
    /**
     * 舊版：138,195 ≥ 門檻會多一關 `exec-approve`（高層加簽），只有 cxo@ 動得了。
     * `bill-approval` 換成單一審核後沒有任何金額分支（Q-04），CXO 不在
     * `bill-reviewers` 部門裡，這一關也沒有他的事 —— 只留「落點是 Dashboard」
     * 這條（卡的「做什麼」要求：CXO 落點那一條留著，只是登入）。
     */
    hr('⑥ CXO（cxo@）—— 落點 Dashboard（不再有高層加簽可驗）');
    await loginAs(page, demoEmail('cxo'));
    await selectOrg(page);
    const cxoLanding = await eapi(page, 'GET', '/me/landing').then((r) => (r.status === 200 ? r.data : { error: r.status }));
    // CXO 是**唯一**以 Dashboard 為家的角色（`FRONTEND.md §1.3`）。
    check('CXO 的落點是 Dashboard', cxoLanding?.route === 'energyDashboardConsole', cxoLanding);

    // ── ⑥-b 用電電費單（卡 G2）：四角色四關走完＋金額跨門檻不增減關卡 ──
    /**
     * `utility_bill()` 獨立於上面的 `bill-approval`，沒有金額門檻
     * （workflow_defs.rs:745-746：金額是台電開的）。選項 A（使用者「建議 A。」）：
     * 不改產品，驗「四角色走完四關」與「金額跨 EXEC_APPROVAL_THRESHOLD（100,000，
     * workflow_defs.rs:519）路線一直是四關，不多一關也不少一關」。
     * roles.ts 本身跑的時候 `EO_DEMO_SAMPLE_BILLS=0`（run-crawlers.sh:59：
     * `roles` 在「單據全部從收件台走一遍」那一組），demo_seed.rs:1129 那張
     * 示範樣本根本不存在這個資料庫裡 —— 改成跟這支其餘每張單一樣，走
     * `uploadNewBatch` 從收件台造一張新的，粗分一定是 `electricityBill`
     * （`mockOcrForUpload` 只會判這種），再用校對台可以改的單別欄位
     * （`document_ingest.rs` 的 `correct_bill`／`CorrectBillInput.doc_type`）
     * 把它改成 `consumptionDevice`／`renewableNoSurplus`（兩者都粗分到
     * `utility-bill`，`doc_type_to_workflow`），同時把金額改到門檻兩側。
     */
    async function uploadUtilityBill(fileName: string, docType: string, amount: number): Promise<number> {
      await loginAsAdmin(page);
      await selectOrg(page);
      // 第 2 版（設計缺陷修正）：customerNumber 要指到真的綁了 utility-bill 的案場
      // （嘉義六腳一號 11-72-0328-95-9，demo_seed.rs:864 把 consumptionBill 綁過去），
      // 不然送審會因為「案場＋單別」找不到綁定而 409（ingest/service.rs:793-797）。
      const batch = await seedBatch(page, [fileName], { customerNumber: '11-72-0328-95-9' });
      check(`${fileName}：開得出一批送件`, batch.ok, batch);
      const bid: number = batch.ok ? await page.evaluate(async (args: { bid: number }) => {
        const org = (window as any).__demoOrgId;
        const r = await fetch(`/api/v1/orgs/${org}/energy/ingest-batches/${args.bid}`, { credentials: 'include' });
        if (!r.ok) return 0;
        const b = await r.json();
        const it = (b.items ?? []).find((i: any) => i.billId != null);
        return it ? Number(it.billId) : 0;
      }, { bid: batch.batchId }) : 0;
      check(`${fileName}：產出了電費單`, bid > 0, { bid });
      if (!bid) return 0;
      const patched = (await eapi(page, 'PATCH', `/bills/${bid}`, { docType, totalAmount: amount })).status;
      check(`${fileName}：單別改成 ${docType}、金額改成 ${amount}`, patched === 200, { patched });
      const after = await eapi(page, 'GET', `/bills/${bid}`);
      check(`${fileName}：GET 確認 docType 真的變成 ${docType}`,
        after.status === 200 && after.data?.docType === docType, after.data?.docType);
      return bid;
    }

    /**
     * 嘉義六腳一號同時綁了 `depositRequest`／`electricityBill`／`consumptionBill`
     * 三種單別，送審會把「該開的流程全部開起來」（跟 doctype.ts① 的躉購有餘電
     * 同一個產品行為：核可＋開發票平行）。不能拿 `executions[0]`，要指名
     * `workflowKey === 'utility-bill'` 的那一條。
     */
    async function utilityBillStatus(bid: number): Promise<{ node: string; status: string }> {
      const r = await eapi(page, 'GET', `/bills/${bid}`);
      const execs: any[] = r.status === 200 ? (r.data?.executions ?? []) : [];
      const ex = execs.find((e: any) => e.workflowKey === 'utility-bill');
      return ex ? { node: ex.currentNodeKey as string, status: ex.status as string } : { node: '', status: 'none' };
    }

    async function walkUtilityBill(bid: number, label: string) {
      if (!bid) return { bid: 0, node: '', status: 'none' };
      await loginAsAdmin(page);
      await selectOrg(page);
      const submitResp = await eapi(page, 'POST', `/bills/${bid}/submit`);
      log(`  ${label} 送審回應：status=${submitResp.status} ${JSON.stringify(submitResp.data)}`);
      check(`${label}：送審成功（POST /bills/${bid}/submit）`, submitResp.status === 200, submitResp);
      // 指名關卡＋指名動作（actOnNode，不是 actOnWorkbench 猜按鈕）——
      // 這張單可能同時掛著 utility-bill 跟 issue-invoice 兩條 execution，
      // 工作台上會有兩列同樣的 data-bill，只按文字會推到不相干的那一列。
      check(`${label}：承辦（admin）推得動承辦確認（送出）`,
        await actOnNode(page, 'clerk-review', 'submit', bid));
      await loginAs(page, demoEmail('manager'));
      await selectOrg(page);
      check(`${label}：主管核准得動`, await actOnNode(page, 'manager-approve', 'approve', bid));
      await loginAs(page, demoEmail('finance'));
      await selectOrg(page);
      check(`${label}：財務放行得動`, await actOnNode(page, 'finance-release', 'release', bid));
      await loginAs(page, demoEmail('accounting'));
      await selectOrg(page);
      check(`${label}：會計標記已付款得動`, await actOnNode(page, 'payment', 'markPaid', bid));
      check(`${label}：會計送出得動（走完付款關）`, await actOnNode(page, 'payment', 'submit', bid));
      const result = await utilityBillStatus(bid);
      log(`  ${label} 最終狀態：${JSON.stringify(result)}`);
      return { bid, ...result };
    }

    hr('⑥-b 用電電費單：門檻以下（60,000）四角色四關走完');
    const lowBid = await uploadUtilityBill('gate-util-low.png', 'consumptionDevice', 60000);
    const lowBill = await walkUtilityBill(lowBid, '門檻以下');
    check('門檻以下：走到 end（status=completed）',
      lowBill.status === 'completed' && lowBill.node === 'end', lowBill);
    await shot(page, '6b-utility-bill-low');

    hr('⑥-b 用電電費單：門檻以上（138,195）四角色四關走完（不多一關）');
    const highBid = await uploadUtilityBill('gate-util-high.png', 'renewableNoSurplus', 138195);
    const highBill = await walkUtilityBill(highBid, '門檻以上');
    check('門檻以上：走到 end（status=completed，跟門檻以下一樣是四關）',
      highBill.status === 'completed' && highBill.node === 'end', highBill);
    await shot(page, '6b-utility-bill-high');

    // 稽核：兩張單都不該出現 `bill.route_gate_removed`（utility_bill() 本來就沒有
    // 金額門檻可以「少一關」）。
    const gateAuditCheck = await (async () => {
      const r = await eapi(page, 'GET', '/audit-events?limit=200');
      const rows: any[] = r.status === 200 ? (r.data?.items ?? []) : [];
      return rows.filter((x: any) => x.type === 'bill.route_gate_removed'
        && (x.payload?.billId === lowBill.bid || x.payload?.billId === highBill.bid));
    })();
    check('用電電費單兩張都沒有 bill.route_gate_removed 稽核事件',
      gateAuditCheck.length === 0, gateAuditCheck);

    // ── ⑧ 三個實體頁 ────────────────────────────────────────
    hr('⑧ C7b 三個實體頁');
    await loginAs(page, demoEmail('operator'));
    await selectOrg(page);

    // P5 地主整合頁（獨立路由）
    // 卡 A（20261118-0427-a）併回後，清單第一列變成「台電屏東區處」這種
    // 受款對象（0 座案場）；不挑第一列，改依名字挑「陳明德」（demo seed
    // 綁定流程＋策略兩種軸的那一位，見下面 P11 的註解）。
    await goto(page, 'energyLandownerConsole', '[data-testid="landowner-row"]');
    await waitIdle(page);
    await page.evaluate(() => {
      // 只有列裡**第一格**（名字那一格）掛了 onClick 導頁，點整列不會動
      // （`LandownersPage.tsx`：onClick 在那顆 `TableCell` 上，不是 `TableRow`）。
      const row = Array.from(document.querySelectorAll('[data-testid="landowner-row"]'))
        .find((r) => (r as HTMLElement).innerText.includes('陳明德')) as HTMLElement | undefined;
      (row?.querySelector('td') as HTMLElement | null)?.click();
    });
    await waitIdle(page);
    const p5 = await page.evaluate(() => ({
      url: location.hash,
      name: (document.querySelector('[data-testid="landowner-name"]') as HTMLElement)?.innerText ?? '',
      stations: document.querySelectorAll('[data-testid="landowner-station"]').length,
      workflowChips: document.querySelectorAll('[data-testid="landowner-station-workflow"]').length,
      inFlight: document.querySelectorAll('[data-testid="landowner-inflight"]').length,
      text: (document.querySelector('main') as HTMLElement)?.innerText ?? '',
    }));
    // 獨立路由 ＋ 可分享的網址（不是 drawer）。
    check('P5 是獨立路由（網址可分享）', /#\/energyLandowners\/\d+/.test(p5.url), p5.url);
    check('有案場', p5.stations > 0, p5);
    // 「各自適用哪版流程」——舊版完全看不到。
    check('案場上看得到「適用哪版流程」', p5.workflowChips > 0, p5);
    // 「現在正在發生」是最重要的一區。
    check('有「現在正在發生」區', /現在正在發生/.test(p5.text), p5.text.slice(0, 200));
    check('沒有 raw key', !/energy:|ryxis:/.test(p5.text));
    await shot(page, '8-p5-landowner');

    // P11 案場詳情（從 P5 點進去 —— 跨頁連結）
    //
    // **要點「有綁定的那個案場」**：demo seed 只綁 `11-72-0328-95-9`，
    // 地主底下另外兩個案場**本來就沒綁**，那兩頁顯示「尚未綁定」是**正確的**。
    // 點第一列（`landowner-open-station`）會落在沒綁的那個，
    // 然後把「頁面誠實地說沒綁」誤判成「效期軸壞了」——踩過一次。
    // 而「點版本 chip 進去看效期軸」正是 `FRONTEND.md §10 P5` 要的
    // 「顯示適用的流程版本與策略，**且可點**」，一次驗兩件事。
    //
    // 而且要挑**兩種綁定都有**的那個案場，才驗得到「兩條軸」——
    // seed 裡流程綁在 `00-60-1600-79-5`、策略也綁在同一個案場；
    // 另一個案場只有保證金流程的綁定、沒有策略。
    // 這裡**不寫死電號**（seed 改了就會靜默失效），改成**掃地主列表**找出符合的那一個。
    // 卡 A 之後清單有 7 筆（受款對象＋地主混在一起），不再掃全部列 ——
    // 直接回「陳明德」那一頁（上面 P5 已經挑過這一位，他的案場同時綁了
    // 流程與策略兩種軸）。
    await goto(page, 'energyLandownerConsole', '[data-testid="landowner-row"]');
    await waitIdle(page);
    await page.evaluate(() => {
      const row = Array.from(document.querySelectorAll('[data-testid="landowner-row"]'))
        .find((r) => (r as HTMLElement).innerText.includes('陳明德')) as HTMLElement | undefined;
      (row?.querySelector('td') as HTMLElement | null)?.click();
    });
    await waitIdle(page);
    const toStation = await page.evaluate(() => {
      const st = Array.from(document.querySelectorAll('[data-testid="landowner-station"]'))
        .find((r) => r.querySelector('[data-testid="landowner-station-workflow"]')
          && r.querySelector('[data-testid="landowner-station-strategy"]'));
      const chip = st?.querySelector('[data-testid="landowner-station-workflow"]') as HTMLElement | null;
      if (!chip) return false;
      chip.click();
      return true;
    });
    check('地主 → 案場詳情（點版本 chip，chip 可點）', toStation, { owner: '陳明德' });
    await waitIdle(page);
    const p11 = await page.evaluate(() => {
      const segs = Array.from(document.querySelectorAll('[data-testid="axis-segment"]'));
      return {
        url: location.hash,
        hasWorkflowAxis: !!document.querySelector('[data-testid="workflow-axis"]'),
        hasStrategyAxis: !!document.querySelector('[data-testid="strategy-axis"]'),
        segments: segs.length,
        activeSegs: segs.filter((e) => (e as HTMLElement).dataset.active === '1').length,
        activeBadges: document.querySelectorAll('[data-testid="axis-active-badge"]').length,
        text: (document.querySelector('main') as HTMLElement)?.innerText ?? '',
      };
    });
    check('兩條效期軸都在', p11.hasWorkflowAxis && p11.hasStrategyAxis, p11);
    // `FRONTEND.md §2.85.2`：**畫不出「換版」的效期軸等於沒有驗證過效期軸** ——
    // 只有一段的軸跟一行文字沒有任何差別。屏東刻意 seed 成兩段。
    check('軸上畫得出「換版」（≥ 2 段）', p11.segments >= 2, p11);
    check('同一時間只有一段生效', p11.activeSegs >= 1 && p11.activeBadges === p11.activeSegs, p11);
    // 這頁存在的理由：**看得出現在生效的是哪一段**。
    check('標得出「生效中」是哪一段', p11.activeBadges > 0 && p11.activeSegs > 0, p11);
    check('沒有 raw key', !/energy:|ryxis:/.test(p11.text));
    await shot(page, '8-p11-station');

    // 案場 → 地主（回頭的連結）
    const backToOwner = await page.evaluate(() => {
      const b = document.querySelector('[data-testid="station-open-landowner"]');
      if (!b) return false;
      (b as HTMLElement).click();
      return true;
    });
    await waitIdle(page);
    check('案場 → 地主（雙向連結）',
      backToOwner && /#\/energyLandowners\/\d+/.test(await page.evaluate(() => location.hash)));

    // ── 送件 Console（收件台 ＋ 送單紀錄合併，`LIFECYCLE.md §7.2`）────────
    hr('⑧-b 送件 Console —— 一個資源一個 Console');

    // 驗收 1：選單上**只有一個**送件；收件台與送單紀錄兩個名字都消失了。
    await goto(page, 'energyIngestConsole', '[data-testid="batch-row"], [data-testid="intake-dropzone"]');
    await waitIdle(page);
    const menu = await page.evaluate(() => {
      const links = Array.from(document.querySelectorAll('a[href]'))
        .map((a) => (a as HTMLAnchorElement).getAttribute('href') ?? '');
      return {
        ingest: links.filter((h) => /energyIngestConsole/.test(h)).length,
        intake: links.filter((h) => /energyIntakeConsole/.test(h)).length,
        batch: links.filter((h) => /energyBatchConsole/.test(h)).length,
        text: (document.querySelector('main') as HTMLElement)?.innerText ?? '',
      };
    });
    check('選單只剩一個「送件」入口', menu.ingest >= 1, menu);
    check('舊的收件台／送單紀錄兩個入口都不見了',
      menu.intake === 0 && menu.batch === 0, menu);
    check('沒有 raw key', !/energy:|ryxis:/.test(menu.text));

    // 驗收 2：清單頁 —— 未完成置頂 ＋ 角標。
    const listView = await page.evaluate(() => {
      const rows = Array.from(document.querySelectorAll('[data-testid="batch-row"]'));
      return {
        rows: rows.length,
        pendings: rows.map((r) => Number((r as HTMLElement).dataset.pending ?? '0')),
        badges: document.querySelectorAll('[data-testid="batch-pending-badge"]').length,
        counts: Array.from(document.querySelectorAll('[data-testid="batch-count"]'))
          .map((e) => (e as HTMLElement).dataset.countKey),
      };
    });
    check('送件清單有批次', listView.rows > 0, listView);
    // 「上傳幾份 → 送審幾份、還有幾份待處理」四個數字要對得起來。
    check('有處理情況的分解數字', listView.counts.length > 0, listView);
    // 未完成的必須排在已完成的前面 —— 這頁是待辦清單不是時間軸。
    const sortedByPending = listView.pendings
      .map((n) => (n > 0 ? 0 : 1))
      .every((v, i, arr) => i === 0 || arr[i - 1] <= v);
    check('未完成的批次置頂', sortedByPending, listView.pendings);
    await shot(page, '8b-ingest-list');

    // 驗收 3（缺陷 A1）：**離開再回來，那一批還在**。
    // 舊收件台是一次性精靈：批次只活在 React state 裡，換頁就沒了。
    const a1Batch = await uploadNewBatch(page, revisitFile);
    check('上傳後進到那一批的詳情頁', a1Batch.ok, a1Batch);
    await goto(page, 'energyMyWorkConsole', '[data-testid="workbench-section"], .MuiAlert-root');
    await waitIdle(page);
    const revisit = await goto(
      page, `energyIngestConsole/${a1Batch.batchId}`, '[data-testid="intake-item"]',
    );
    check('切到別頁再回來，那一批還在（A1 消失）', revisit, a1Batch);

    // 驗收 4：校對**就地展開**，URL 有 `?item=`，路由沒換。
    const beforeHash = await page.evaluate(() => window.location.hash);
    await page.evaluate(() => {
      (document.querySelector('[data-testid="intake-review"]') as HTMLElement | null)?.click();
    });
    await waitIdle(page);
    const expand = await page.evaluate(() => ({
      hash: window.location.hash,
      workspaces: document.querySelectorAll('[data-testid="ingest-workspace"]').length,
    }));
    check('校對就地展開（不換頁）',
      expand.workspaces > 0
      && expand.hash.split('?')[0] === beforeHash.split('?')[0]
      && /[?&]item=\d+/.test(expand.hash),
      { beforeHash, ...expand });
    // refresh 回得來 —— `?item=` 不只是裝飾。
    await page.reload({ waitUntil: 'networkidle2' });
    await sleep(2500);
    const afterReload = await page.$$eval('[data-testid="ingest-workspace"]', (e) => e.length);
    check('refresh 之後校對區還開著', afterReload > 0, { afterReload });
    /**
     * **左欄一定要有原檔**（`LIFECYCLE.md §6.7.2`）。
     *
     * 這一條抓過一個從 UI 走就一定斷的鏈：開批發生在檔案上傳**之前**，
     * 所以 `item.attachment_id` 從來沒被補上 —— 左欄永遠是
     * 「這張單沒有掛原始檔案」，而且送審時的 `linkToExecution` 整段被跳過
     * （缺陷 B2 等於沒修）。它不報錯，只是每一關都看不到那張紙。
     */
    const source = await page.evaluate(() => ({
      hasSource: !!document.querySelector('[data-testid="ingest-open-source"]'),
      text: (document.querySelector('[data-testid="ingest-workspace"]') as HTMLElement)?.innerText ?? '',
    }));
    check('校對工作區左欄接得到原檔', source.hasSource, source);
    await shot(page, '8b-ingest-workspace');

    // 驗收 5：送審後那一筆**離開校對區**，變成一行「檔名 → 單號 [看流程單]」。
    const openBefore = await page.$$eval('[data-testid="intake-item"]', (e) => e.length);
    await page.evaluate(() => {
      (document.querySelector('[data-testid="intake-submit-one"]') as HTMLElement | null)?.click();
    });
    await waitIdle(page);
    const afterSubmit = await page.evaluate(() => ({
      open: document.querySelectorAll('[data-testid="intake-item"]').length,
      done: document.querySelectorAll('[data-testid="ingest-done-row"]').length,
      hasExecutionLink: document.querySelectorAll('[data-testid="ingest-open-execution"]').length,
      hasBillLink: document.querySelectorAll('[data-testid="ingest-open-bill"]').length,
    }));
    check('送審後那一筆離開校對區',
      afterSubmit.open === openBefore - 1 && afterSubmit.done >= 1,
      { openBefore, ...afterSubmit });
    check('已送出那一行有單號連結＋看流程單',
      afterSubmit.hasBillLink >= 1 && afterSubmit.hasExecutionLink >= 1, afterSubmit);
    await shot(page, '8b-ingest-after-submit');

    // 驗收 6：**同一批裡兩張同一張紙 —— 辨識完就在列上標**，
    // 不是等到按了送審才說（那時人已經把校對的工做完了）。
    const dupBatch = await uploadNewBatch(page, twinFileA, twinFileB);
    check('同批兩份同名檔開得起來', dupBatch.ok, dupBatch);
    await waitIdle(page);
    const twin = await page.evaluate(() => ({
      items: document.querySelectorAll('[data-testid="intake-item"]').length,
      marked: document.querySelectorAll('[data-testid="intake-item-duplicate"]').length,
      label: (document.querySelector('[data-testid="intake-item-duplicate"]') as HTMLElement)?.innerText ?? '',
    }));
    check('同批重複在列上就標出來（不必等送審）',
      twin.items >= 2 && twin.marked >= 1, twin);
    check('重複標記指得出「本批第幾筆」', /\d/.test(twin.label), twin);
    await shot(page, '8b-ingest-duplicate-in-batch');

    // 驗收 7：**別的地方已經有一筆在跑** → `already_in_review`，
    // 訊息與「已通過」那一句**不一樣**，而且兩顆出口都在。
    const clashBatch = await uploadNewBatch(page, dupOfFirst);
    check('撞單用的那一批開得起來', clashBatch.ok, clashBatch);
    await page.evaluate(() => {
      (document.querySelector('[data-testid="intake-submit-one"]') as HTMLElement | null)?.click();
    });
    await waitIdle(page);
    const clash = await page.evaluate(() => {
      const body = document.querySelector('[data-testid="ingest-dup-body"]') as HTMLElement | null;
      return {
        open: !!body,
        code: body?.dataset.code ?? '',
        title: (document.querySelector('[data-testid="ingest-dup-title"]') as HTMLElement)?.innerText ?? '',
        text: body?.innerText ?? '',
        seeThatOne: !!document.querySelector('[data-testid="ingest-dup-goto"]'),
        discardThis: !!document.querySelector('[data-testid="ingest-dup-discard"]'),
      };
    });
    check('撞到已存在的同一張單 → 出現重複說明',
      clash.open && ['already_in_review', 'duplicate_document'].includes(clash.code), clash);
    // 訊息要指得出「是哪一筆、誰送的、現在在哪一關」—— 只說「重複」等於沒說。
    check('重複說明有具體內容（不是一句「重複」）', clash.text.length > 12, clash);
    check('兩顆出口都在：[看那一筆] ＋ [捨棄這一份]',
      clash.seeThatOne && clash.discardThis, clash);
    await shot(page, '8b-ingest-duplicate-clash');

    // 捨棄之後那一筆離開校對區（給出口不只是話術）。
    await page.evaluate(() => {
      (document.querySelector('[data-testid="ingest-dup-discard"]') as HTMLElement | null)?.click();
    });
    await waitIdle(page);
    const afterDiscard = await page.evaluate(() => ({
      open: document.querySelectorAll('[data-testid="intake-item"]').length,
      done: document.querySelectorAll('[data-testid="ingest-done-row"][data-status="discarded"]').length,
    }));
    check('捨棄之後那一筆真的離開校對區', afterDiscard.done >= 1, afterDiscard);

    // 血緣要**雙向**：送件 → 單 → **回那一批**（`LIFECYCLE.md §7.6`）。
    // 前一版連回去的是**清單頁**，等於把「回到來源」變成「回到一堆來源裡自己找」。
    await goto(page, `energyIngestConsole/${a1Batch.batchId}`, '[data-testid="ingest-done-row"]');
    await waitIdle(page);
    const wentToBill = await page.evaluate(() => {
      const b = document.querySelector('[data-testid="ingest-open-bill"]');
      if (!b || (b as HTMLButtonElement).disabled) return false;
      (b as HTMLElement).click();
      return true;
    });
    check('送件 → 單（血緣往下游）', wentToBill);
    await waitIdle(page);
    const backLink = await page.evaluate(() => {
      const b = document.querySelector('[data-testid="bill-open-batch"]') as HTMLElement | null;
      if (!b) return { has: false, text: '', hash: '' };
      b.click();
      return { has: true, text: b.innerText, hash: '' };
    });
    check('單 → 送件（血緣往上游）', backLink.has, backLink);
    await waitIdle(page);
    const backHash = await page.evaluate(() => window.location.hash);
    // 回到**那一批**，不是回到清單。
    check('麵包屑連回的是那一批不是清單',
      new RegExp(`#/energyIngestConsole/${a1Batch.batchId}`).test(backHash),
      { backHash, batchId: a1Batch.batchId });
    await shot(page, '8b-bill-backlink');

    // ── ⑨ P9 流程總覽（C7c）──────────────────────────────────
    hr('⑨ P9 流程總覽');
    // 先**留一張單在飛**：走到這一步時前面兩張都已經 `completed`，
    // P9 上每個版本的「進行中」都是 0 → 點節點只會開出一個空的 drawer。
    // 那樣「列出那幾張、可跳去處理」這條**根本沒被驗到**，
    // 而斷言還是綠的（因為 drawer 確實開得出來）—— 典型的假綠燈。
    await loginAs(page, demoEmail('operator'));
    await selectOrg(page);
    await uploadNewBatch(page, smallFile);
    await page.evaluate(() => {
      (document.querySelector('[data-testid="intake-submit-one"]') as HTMLElement | null)?.click();
    });
    await waitIdle(page);

    await goto(page, 'energyWorkflowConsole', '[data-testid="workflow-row"]');
    await waitIdle(page);
    const p9 = await page.evaluate(() => ({
      rows: document.querySelectorAll('[data-testid="workflow-row"]').length,
      lineage: document.querySelectorAll('[data-testid="workflow-lineage"]').length,
      bindings: Array.from(document.querySelectorAll('[data-testid="workflow-binding"]'))
        .map((e) => (e as HTMLElement).innerText.trim()),
      nodes: document.querySelectorAll('[data-testid="flow-node"]').length,
      graphNodes: (document.querySelector('[data-testid="flow-current"]') as HTMLElement)?.dataset.nodeCount ?? '0',
      text: (document.querySelector('main') as HTMLElement)?.innerText ?? '',
    }));
    check('流程清單有版本', p9.rows > 0, { rows: p9.rows });
    // v2 從 v1 分岔 —— `parentVersion` 是關聯不是欄位，抄錯就永遠不顯示。
    check('看得出「換版」（v2 從 v1 分岔）', p9.lineage > 0, p9.lineage);
    /**
     * 綁定認的是**電號**（陷阱 #27）——印 stationId 會全是空的。
     *
     * 格式在 R5 換過：seed 從假電號 `01-2233-4477`（2-4-4）改成
     * **真的台電電號** `00-60-1600-79-5`（2-2-4-2-1）。
     * 舊的 regex 認不得新格式 → 這條會紅在「資料是對的」的地方。
     * 所以這裡只認**電號的形狀**（數字與連字號、夠長），不寫死分節。
     */
    check('綁定印得出電號', p9.bindings.some((b) => /\d{2}(-\d+){3,}/.test(b)), p9.bindings);
    check('沒有 raw key', !/energy:|ryxis:/.test(p9.text));
    /**
     * Q-04（2026-11-16）：舊的「分支邊顯示條件文字（金額 ≥ 門檻）」驗的是
     * `default_bill_approval()` 那條 `manager-approve → exec-approve` 邊
     * （`FRONTEND.md §10 P9`）。那條流程已經不再內建安裝（換成
     * `platform_review_bill()`，沒有任何金額分支），P9 預設看到的圖上
     * 不會再有那條邊，不自己發明替代流程 —— 刪掉這條，「條件是資料」
     * 這個能力本身由前端／引擎測試守，不是這支 e2e 的責任。
     */
    await shot(page, '9-p9-workflows');

    // **沒有任何 execution 也看得到流程圖** —— 這是 P9 存在的理由。
    const noExecGraph = await page.evaluate(async () => {
      const rows = Array.from(document.querySelectorAll('[data-testid="workflow-row"]'));
      // 找「進行中 = 0」的那一版
      const idle = rows.find((r) => (r.querySelector('[data-testid="workflow-running"]') as HTMLElement)
        ?.innerText.trim() === '0');
      if (!idle) return { note: 'no idle version' };
      (idle as HTMLElement).click();
      await new Promise((res) => { setTimeout(res, 2500); });
      return {
        badge: !!document.querySelector('[data-testid="workflow-no-execution"]'),
        nodeCount: Number((document.querySelector('[data-testid="flow-current"]') as HTMLElement)
          ?.dataset.nodeCount ?? 0),
      };
    });
    log(`  沒有 execution 的版本：${JSON.stringify(noExecGraph)}`);
    check('沒有 execution 也看得到流程圖（P9 存在的理由）',
      (noExecGraph as any)?.nodeCount > 0, noExecGraph);
    await shot(page, '9-p9-no-execution');

    // 點節點 → drawer
    // 上一步刻意選了「進行中 = 0」的版本 —— 在那張圖上點節點永遠是空的。
    // 先切回**有單在跑**的版本，drawer 才驗得到「列出那幾張、可跳去處理」。
    await page.evaluate(async () => {
      const rows = Array.from(document.querySelectorAll('[data-testid="workflow-row"]'));
      const busy = rows.find((r) => Number((r.querySelector('[data-testid="workflow-running"]') as HTMLElement)
        ?.innerText.trim() ?? 0) > 0);
      (busy as HTMLElement | null)?.click();
      await new Promise((res) => { setTimeout(res, 2500); });
    });
    const drawer = await page.evaluate(async () => {
      const nodes = Array.from(document.querySelectorAll('[data-testid="flow-node"]'));
      // 挑一個**有積單**的節點；沒有就挑第一個。
      const hot = nodes.find((n) => Number((n as HTMLElement).dataset.waiting ?? 0) > 0) ?? nodes[0];
      if (!hot) return { note: 'no nodes' };
      (hot.closest('.react-flow__node') as HTMLElement | null)?.click();
      await new Promise((res) => { setTimeout(res, 2000); });
      const d = document.querySelector('[data-testid="node-drawer"]') as HTMLElement | null;
      return {
        opened: !!d,
        node: d?.dataset.node ?? '',
        tasks: document.querySelectorAll('[data-testid="node-task"]').length,
        actions: document.querySelectorAll('[data-testid="node-drawer"] [data-testid="action-bar"]').length,
        empty: !!document.querySelector('[data-testid="node-drawer-empty"]'),
      };
    });
    log(`  節點 drawer：${JSON.stringify(drawer)}`);
    check('點節點開得出 drawer', (drawer as any)?.opened, drawer);
    // 光「開得出來」不算 —— 要**列得出卡在那一關的單**，
    // 空的 drawer 跟「節點數字不能點」是同一件事。
    check('drawer 列得出卡在那一關的單', ((drawer as any)?.tasks ?? 0) > 0, drawer);
    // 而且要跳得去處理（ActionBar 的第三處落點就在這張卡片裡）。
    const drawerJump = await page.evaluate(async () => {
      const b = document.querySelector('[data-testid="node-task-open"]') as HTMLElement | null;
      if (!b) return { note: 'no task link' };
      b.click();
      await new Promise((res) => { setTimeout(res, 2500); });
      return { hash: location.hash };
    });
    check('drawer → 那張單（可直接去處理）',
      /energyBillConsole\/\d+\/show/.test((drawerJump as any)?.hash ?? ''), drawerJump);
    await shot(page, '9-p9-node-drawer');

    // ── ⑩ Dashboard 數字全部可點（C7c 步驟 10）──────────────────
    hr('⑩ Dashboard 數字可點');
    await goto(page, 'energyDashboardConsole', '[data-testid="kpi-card"]');
    await waitIdle(page);
    const kpis = await page.$$eval('[data-testid="kpi-card"]', (els) => els.map((e) => ({
      kpi: (e as HTMLElement).dataset.kpi,
      clickable: getComputedStyle(e as HTMLElement).cursor === 'pointer',
    })));
    log(`  KPI：${JSON.stringify(kpis)}`);
    // 「不能點的數字等於一張海報」——每一張都要有目的地。
    check('每個 KPI 都可點', kpis.length > 0 && kpis.every((k) => k.clickable), kpis);
    await shot(page, '10-dashboard-kpis');

    const kpiJump = await page.evaluate(async () => {
      const card = Array.from(document.querySelectorAll('[data-testid="kpi-card"]'))
        .find((e) => (e as HTMLElement).dataset.kpi === 'energy.dashboard.payable');
      (card as HTMLElement | null)?.click();
      await new Promise((res) => { setTimeout(res, 2500); });
      return location.hash;
    });
    // 「本月應付」要**帶著篩選**過去（不是一張沒篩選的清單）。
    check('本月應付點下去帶著月份篩選', /energyBillConsole\?month=\d{4}-\d{2}/.test(kpiJump), kpiJump);

    // 案場 → P11（`§2 P7` 那張表的第三列）
    await goto(page, 'energyDashboardConsole', '[data-testid="station-row"]');
    await waitIdle(page);
    const toStationFromDash = await page.evaluate(async () => {
      (document.querySelector('[data-testid="station-row"]') as HTMLElement)?.click();
      await new Promise((res) => { setTimeout(res, 2500); });
      return location.hash;
    });
    check('Dashboard 案場 → P11 案場詳情',
      /energyStationConsole\/\d+\/show/.test(toStationFromDash), toStationFromDash);

    // ── ⑩-b 3D 地圖的案場點**點得動**（C7d）──────────────────────
    //
    // WebGL 的內容讀不回 DOM —— 場景提供 `screenPositionOf(id)`
    // 把案場點**投影到畫面座標**，我們用它算出「該點哪裡」，
    // 然後**用真的 `page.mouse.click` 點下去**。
    // 量測 API 只用來定位，不拿它驅動（那會跳過整條事件鏈）。
    // crawler 不要加 swiftshader flag —— macOS 上 WebGL 建不出 context。
    await goto(page, 'energyDashboardConsole', '[data-testid="station-map"]');
    await sleep(6000); // 等場景建好＋進場動畫落定
    // 這一支 `evaluate` 撞過 `Attempted to use detached Frame` —— frame 還在換
    // （3D 場景那一頁載入久，`goto` 之後的 settle 時間比別頁長）。
    // 那是 harness 競態，但它會讓整支腳本 throw、後面六十幾條都不跑，
    // 而畫面上看起來像「地圖壞了」。
    const mapProbe = await retryOnDetach('mapProbe', () => page.evaluate(() => {
      const h = (window as any).__taiwanMapHandle;
      const info = document.querySelector('[data-testid="station-map-info"]') as HTMLElement | null;
      const plotted = Number(info?.dataset.plotted ?? 0);
      const rows = Array.from(document.querySelectorAll('[data-testid="station-row"]'));
      const ids = rows.map((r) => Number((r as HTMLElement).dataset.station)).filter(Boolean);
      if (!h) return { note: 'no map handle', plotted, ids };
      // 找一個「投影得到、在畫面內，**而且附近沒有別的點**」的案場點。
      //
      // 只挑「第一個看得到的」會不穩：示範資料裡屏東枋寮一號與二號相距約 500 公尺，
      // 在地圖尺度下兩個點是疊在一起的 —— 點下去命中哪一個取決於繪製順序，
      // 於是這條驗收會**隨機**紅一次，而訊息是「預期 311 卻到了 312」，
      // 看起來像跳轉壞掉。要驗的是「點得動而且跳對」，不是「命中哪一顆」。
      const host = document.querySelector('[data-testid="station-map"] > div') as HTMLElement | null;
      const rect = host?.getBoundingClientRect();
      if (!rect) return { note: 'no map host', plotted, ids };
      const pts = ids
        .map((id) => ({ id, p: h.screenPositionOf(id) }))
        .filter((x: any) => !!x.p)
        .filter((x: any) => x.p.x >= 8 && x.p.y >= 8
          && x.p.x <= rect.width - 8 && x.p.y <= rect.height - 8);
      /**
       * **疊在一起的點本來就分不出來** —— 所以驗收要問的是
       * 「點下去有沒有跳到**那個位置上的**某一座案場」，
       * 而不是「有沒有跳到我心裡想的那一顆」。
       *
       * 示範資料的四座上圖案場正好是兩組（屏東 ×2、嘉義 ×2），
       * 每一組內部相距數百公尺 —— 在台灣地圖的尺度下就是同一個像素。
       * 挑「附近沒有別人」的點會**一顆都挑不到**，而挑第一顆再比 id
       * 會隨繪製順序隨機紅。
       */
      const HIT = 12;
      const first: any = (pts as any[])[0];
      if (!first) return { note: 'no visible station pin', plotted, ids };
      const nearIds = (pts as any[])
        .filter((o) => Math.hypot(o.p.x - first.p.x, o.p.y - first.p.y) <= HIT)
        .map((o) => o.id);
      return {
        id: first.id, nearIds, x: rect.left + first.p.x, y: rect.top + first.p.y, plotted,
      };
    }), 3, page);
    log(`  地圖探針：${JSON.stringify(mapProbe)}`);
    if ((mapProbe as any)?.id) {
      // **真的滑鼠點擊** —— 不是 element.click()（canvas 沒有那種東西）。
      await page.mouse.click((mapProbe as any).x, (mapProbe as any).y);
      await waitIdle(page);
      const afterMapClick = await page.evaluate(() => location.hash);
      log(`  點地圖後：${afterMapClick}`);
      // 疊在同一個像素上的那幾顆都算命中（見上面探針的說明）。
      const hitIds: number[] = (mapProbe as any).nearIds ?? [(mapProbe as any).id];
      check('3D 地圖的案場點點得動 → P11',
        hitIds.some((id) => new RegExp(`energyStationConsole/${id}/show`).test(afterMapClick)),
        { expect: (mapProbe as any).nearIds ?? [(mapProbe as any).id], got: afterMapClick });
    } else {
      check('3D 地圖的案場點點得動 → P11', false, mapProbe);
    }

    // 地圖點得動了，**下面那張案場表要保留** —— 它是 accessible 的路徑。
    await goto(page, 'energyDashboardConsole', '[data-testid="station-row"]');
    await waitIdle(page);
    const tableStillThere = await page.$$eval('[data-testid="station-row"]', (e) => e.length);
    check('地圖下方的案場表**保留**（accessible 路徑）', tableStillThere > 0, { tableStillThere });

    // ── ⑪ §2.85 未綁定案場（三個地方都要講重）────────────────────
    hr('⑪ 未綁定案場：P5 / P11 / 校對台');
    await goto(page, 'energyLandownerConsole', '[data-testid="landowner-row"]');
    await waitIdle(page);
    const unbound = await page.evaluate(async () => {
      // 卡 A 之後清單混了受款對象（台電區處／子公司／公司／機關，沒有案場）；
      // 只在真的有地主身分的兩位裡找（陳明德／林秀英），不掃全部 7 列。
      const findRows = () => Array.from(document.querySelectorAll('[data-testid="landowner-row"]'))
        .filter((r) => /陳明德|林秀英/.test((r as HTMLElement).innerText));
      const n = findRows().length;
      for (let i = 0; i < n; i += 1) {
        (findRows()[i]?.querySelector('td') as HTMLElement | null)?.click();
        await new Promise((res) => { setTimeout(res, 2500); });
        const chip = document.querySelector('[data-testid="landowner-station-unbound"]') as HTMLElement | null;
        if (chip) {
          const text = chip.innerText.trim();
          chip.click();
          await new Promise((res) => { setTimeout(res, 2500); });
          return {
            chipText: text,
            onStation: /energyStationConsole\/\d+\/show/.test(location.hash),
            axisUnbound: document.querySelectorAll('[data-testid="axis-unbound"]').length,
            gotoWorkflows: !!document.querySelector('[data-testid="axis-open-workflows"]'),
            stationText: (document.querySelector('main') as HTMLElement)?.innerText ?? '',
          };
        }
        window.history.back();
        await new Promise((res) => { setTimeout(res, 1500); });
      }
      return { note: 'no unbound station found' };
    });
    log(`  未綁定：${JSON.stringify({ ...(unbound as any), stationText: undefined })}`);
    // 「未綁定」不是留白，是**收不了單**。
    check('P5 的未綁定案場標得出「收不了單」',
      /收不了單/.test((unbound as any)?.chipText ?? ''), (unbound as any)?.chipText);
    check('P11 的空軸說得出後果 ＋ 指得出誰能綁',
      (unbound as any)?.axisUnbound >= 2 && (unbound as any)?.gotoWorkflows, unbound);
    await shot(page, '11-unbound-station');

    // ── ⑫ 增補 B：校對台互動（點欄位→高亮）────────────────────
    hr('⑫ 增補 B：點欄位→高亮');

    /**
     * **不可以隨便挑清單的第一列**。
     *
     * 這一段要**改金額並存檔**，而存檔在 `intake`／`revise` 以外的模式
     * 是被後端擋掉的（`LIFECYCLE.md §6.4.3`，`PATCH /bills/:id` 回 403）。
     * 清單第一列多半是已經走完流程的單 —— 那時畫面正確地是唯讀，
     * 於是「改小跨門檻」什麼都不會發生，而斷言看起來像功能壞了。
     *
     * 所以先**上傳一張新的、刻意不送審**：那張單對 operator@ 就是 `intake` 模式。
     */
    const editableBatch = await uploadNewBatch(page, reviewFile);
    check('備好一張可編輯的單（intake 模式）', editableBatch.ok, editableBatch);
    const editableBillId: number = await page.evaluate(async (bid: number) => {
      const org = (window as any).__demoOrgId;
      const r = await fetch(`/api/v1/orgs/${org}/energy/ingest-batches/${bid}`, { credentials: 'include' });
      if (!r.ok) return 0;
      const batch = await r.json();
      const item = (batch.items ?? []).find((i: any) => i.billId != null);
      return item ? Number(item.billId) : 0;
    }, editableBatch.batchId);
    check('那一批產生了電費單', editableBillId > 0, { editableBillId });

    await goto(page, `energyBillConsole/${editableBillId}/show`, '[data-testid="review-field"], table');
    await waitIdle(page);
    const openedBill = await page.evaluate(
      () => /energyBillConsole\/\d+\/show/.test(location.hash)
        && !!document.querySelector('[data-testid="final-totalAmount"]'),
    );
    check('進得了校對台', openedBill);
    // `intake` 模式下欄位**必須是可編輯的** —— 這一條同時是 §6.8 的驗收：
    // 「欄位可編輯性依 `canEditSubject`」。
    const editable = await page.evaluate(() => {
      const input = document.querySelector('[data-testid="final-totalAmount"]') as HTMLInputElement | null;
      return { found: !!input, disabled: input?.disabled ?? null };
    });
    check('intake 模式的欄位可編輯', editable.found && editable.disabled === false, editable);

    // B-1：點欄位 → 原檔對應位置高亮
    const highlight = await page.evaluate(async () => {
      const before = (document.querySelector('[data-testid="source-box"][data-field="totalAmount"]') as HTMLElement)
        ?.dataset.active;
      const row = Array.from(document.querySelectorAll('[data-testid="review-field"]'))
        .find((r) => (r as HTMLElement).dataset.field === 'totalAmount');
      (row as HTMLElement | null)?.click();
      await new Promise((res) => { setTimeout(res, 800); });
      const after = (document.querySelector('[data-testid="source-box"][data-field="totalAmount"]') as HTMLElement)
        ?.dataset.active;
      return { before, after, boxes: document.querySelectorAll('[data-testid="source-box"]').length };
    });
    log(`  高亮：${JSON.stringify(highlight)}`);
    /**
     * **這一條搬到 `crawler-energy-ops-recognition` 了**（R7）。
     *
     * 座標來自辨識服務的 `evidence.box`，而**只有真的 PDF 跑過真的服務才有**
     * —— 這一支的單是 fixture 灌進去的（`seedBatch`），`boxes` 一定是 0，
     * 而那是**正確行為**（沒有座標就不畫框，`RECOGNITION.md §9-5`）。
     * 留在這裡只會變成一條永遠紅的斷言，然後下一個人花時間去查一個不存在的 bug。
     *
     * 這裡改成驗**這一支真正在乎的那一半**：點欄位這個互動有接上
     * （`focusField` 有被設定），而不是「框亮不亮」。
     */
    check('點欄位這個互動有接上（座標本身由 recognition crawler 用真 PDF 驗）',
      highlight.boxes === 0 || (highlight.before === '0' && highlight.after === '1'), highlight);
    await shot(page, '12-field-highlight');

    /**
     * Q-04（2026-11-16）：刪「改金額 → 分支即時重算」（B-2）與
     * 「⑫-b 少一關：改小跨門檻」。單一審核沒有任何金額分支，沒有
     * 「多一關」「少一關」可以驗，`bill.route_gate_removed` 這整條稽核
     * 鏈路在 `bill-approval` 上已經沒有對應的產品行為，不自己發明替代流程。
     */

    // ── ⑬ 跨頁矩陣抽三格（`IA.md §5`）────────────────────────
    hr('⑬ 跨頁矩陣抽三格');
    // 格 1：P3 校對台 → 來源批次
    // ⑫-b 結束時人在**稽核頁**（換成 cxo@ 查稽核）——
    // 直接在這裡找校對台的按鈕當然找不到。**先真的走回一張單**。
    // （這種「假設自己還在上一頁」的錯，跟陷阱 #36 是同一類。）
    await loginAs(page, demoEmail('operator'));
    await selectOrg(page);
    await goto(page, 'energyBillConsole', '[data-testid="bill-row"], table');
    await waitIdle(page);
    const cell1 = await page.evaluate(async () => {
      const wait = (ms: number) => new Promise((res) => { setTimeout(res, ms); });
      const rows = Array.from(document.querySelectorAll('[data-testid="bill-row"]'));
      // 要挑**有上游批次**的單：seed 直接建的單沒有批次（那是正常的）。
      for (let i = 0; i < rows.length; i += 1) {
        (document.querySelectorAll('[data-testid="bill-row"]')[i] as HTMLElement)?.click();
        await wait(2500);
        if (document.querySelector('[data-testid="bill-open-batch"]')) return true;
        window.history.back();
        await wait(2000);
      }
      return false;
    });
    check('矩陣格①：P3 校對台 → 來源批次', cell1);
    // 格 2：P11 案場 → 最近的單
    // 這裡**刻意不挑「P1 工作台列 → 單」**：跑到這一步時鏈路已經全部走完，
    // 工作台合理地是空的 —— 拿一個「本來就沒有資料」的格子當驗收，
    // 紅了也不代表連結壞掉，只代表這一輪沒東西可點。
    await goto(page, 'energyLandownerConsole', '[data-testid="landowner-row"]');
    await waitIdle(page);
    // 卡 A 之後清單混了受款對象（沒有案場）；只在真的有地主身分的兩位裡找。
    const ownerCount = await page.$$eval('[data-testid="landowner-row"]',
      (e) => e.filter((r) => /陳明德|林秀英/.test((r as HTMLElement).innerText)).length);
    let cell2: any = { note: 'no landowners' };
    // 單只落在其中一個地主的其中一個案場上 —— 只看第一個地主會假性失敗。
    for (let oi = 0; oi < ownerCount; oi += 1) {
      await goto(page, 'energyLandownerConsole', '[data-testid="landowner-row"]');
      await waitIdle(page);
      await page.evaluate((n: number) => {
        const row = Array.from(document.querySelectorAll('[data-testid="landowner-row"]'))
          .filter((r) => /陳明德|林秀英/.test((r as HTMLElement).innerText))[n] as HTMLElement | undefined;
        (row?.querySelector('td') as HTMLElement | null)?.click();
      }, oi);
      await waitIdle(page);
      // eslint-disable-next-line no-await-in-loop
      cell2 = await page.evaluate(async () => {
      const wait = (ms: number) => new Promise((res) => { setTimeout(res, ms); });
      // 要挑**真的有單**的案場 —— 綁定齊全 ≠ 有單：
      // demo 的單只落在其中一個案場上，挑錯就會得到「no recent bill」，
      // 那不是連結壞掉，只是那個案場還沒收過單。
      const chips = Array.from(document.querySelectorAll('[data-testid="landowner-station-workflow"]'));
      for (let i = 0; i < chips.length; i += 1) {
        (document.querySelectorAll('[data-testid="landowner-station-workflow"]')[i] as HTMLElement)?.click();
        await wait(2500);
        const bill = document.querySelector('[data-testid="station-bill"]') as HTMLElement | null;
        if (bill) { bill.click(); await wait(2500); return { hash: location.hash }; }
        window.history.back();
        await wait(2000);
      }
      return { note: 'no station with bills' };
      });
      if ((cell2 as any)?.hash) break;
    }
    check('矩陣格②：P11 案場 → 最近的單',
      /energyBillConsole\/\d+\/show/.test((cell2 as any)?.hash ?? ''), cell2);
    // 格 3：P6 稽核列 → 反跳實體
    // **要換成有稽核檢視權的身分**：`energy:operator` 只綁 `energy:flow:participate`，
    // **沒有 `energy:audit:view`** —— 用承辦看稽核頁會被擋，
    // 症狀是「一顆連結都沒有」，但那是**權限正確地生效**，不是連結壞掉。
    // cxo@ 走 `energy:oversight`（含 audit:view）。
    await loginAs(page, demoEmail('cxo'));
    await selectOrg(page);
    await goto(page, 'energyAuditConsole', 'table');
    await waitIdle(page);
    const cell3 = await page.evaluate(async () => {
      const b = document.querySelector('[data-testid="audit-open"]') as HTMLElement | null;
      if (!b) return { note: 'no audit link' };
      b.click();
      await new Promise((res) => { setTimeout(res, 2500); });
      return { hash: location.hash };
    });
    log(`  矩陣格③：${JSON.stringify(cell3)}`);
    check('矩陣格③：P6 稽核列 → 反跳實體',
      /#\/energy/.test((cell3 as any)?.hash ?? ''), cell3);

    // ── ⑭ 一條動線走到底：稽核事件 → 單 → 案場 → 策略 → 應付 ──────
    //
    // **全程不回選單**（`journeys-entities.html` 最後一條，4 次點擊）。
    // 這條就是「八座孤島」有沒有被接起來的最終判準：
    // 中間只要斷一段，使用者就得回選單重找 —— 那正是孤島的定義。
    hr('⑭ 動線：稽核 → 單 → 案場 → 策略 → 應付（全程不回選單）');
    // 同上：起點是稽核頁 → 身分必須有 `energy:audit:view`。
    await loginAs(page, demoEmail('cxo'));
    await selectOrg(page);
    await goto(page, 'energyAuditConsole', 'table');
    await waitIdle(page);
    const journey = await page.evaluate(async () => {
      const wait = (ms: number) => new Promise((res) => { setTimeout(res, ms); });
      const menuVisits: string[] = [];
      const startHash = location.hash;

      // **卡 A 之後案場變多（6→7），不假設第一顆稽核連結指向的那個案場
      // 就有應付紀錄** —— 逐一試過 `[data-target-type="energyBill"]` 的
      // 稽核列，直到某一條走到底真的有應付資料。
      const auditLinks = Array.from(document.querySelectorAll(
        '[data-target-type="energyBill"] [data-testid="audit-open"]',
      ));
      if (!auditLinks.length) return { fail: 'no bill-targeted audit link', startHash };
      // 卡 A 之後稽核事件變多，不要每一條都花 8～15 秒試過去——只試前 3 條，
      // 每一條縮短等待（1200/1000/800ms），demo seed 的主線單通常排在前面。
      const MAX_TRY = Math.min(3, auditLinks.length);

      for (let li = 0; li < MAX_TRY; li += 1) {
        const steps: any[] = [];
        if (location.hash !== startHash) { location.hash = startHash.replace('#', ''); await wait(1200); }
        const links = document.querySelectorAll('[data-target-type="energyBill"] [data-testid="audit-open"]');
        const a = links[li] as HTMLElement | undefined;
        if (!a) continue;
        a.click(); await wait(1500);
        steps.push({ step: 'bill', hash: location.hash });
        if (!/energyBillConsole\/\d+\/show/.test(location.hash)) continue;

        // ② 單 → 案場
        const st = document.querySelector('[data-testid="review-open-station"]') as HTMLElement | null;
        if (!st) continue;
        st.click(); await wait(1500);
        steps.push({ step: 'station', hash: location.hash });
        if (!/energyStationConsole\/\d+\/show/.test(location.hash)) continue;

        // ③ 案場上看得到**策略**（那一段生效中的）
        const strategySeg = document.querySelector('[data-testid="strategy-axis"] [data-testid="axis-segment"][data-active="1"]') as HTMLElement | null;
        steps.push({ step: 'strategy', seen: !!strategySeg, text: strategySeg?.innerText.replace(/\n/g, ' ') ?? '' });
        if (!strategySeg) continue;

        // ④ 案場 → 最近的單 → **應付頁籤**（最多試前 20 筆，跟產品 RECENT_BILL_LIMIT 對齊）
        const stationHash = location.hash;
        const billCount = Math.min(20, document.querySelectorAll('[data-testid="station-bill"]').length);
        if (!billCount) continue;
        let found = false;
        for (let bi = 0; bi < billCount; bi += 1) {
          if (location.hash !== stationHash) {
            location.hash = stationHash.replace('#', '');
            await wait(1000);
          }
          const rowsEls = document.querySelectorAll('[data-testid="station-bill"]');
          (rowsEls[bi] as HTMLElement)?.click();
          await wait(1200);
          const tabs = Array.from(document.querySelectorAll('button[role="tab"]'));
          const payoutTab = tabs.find((t) => /應付|Payout/i.test((t as HTMLElement).innerText));
          if (!payoutTab) continue;
          (payoutTab as HTMLElement).click(); await wait(800);
          const rows = document.querySelectorAll('[data-testid="payout-row"]').length;
          if (rows > 0) {
            const formula = (document.querySelector('[data-testid="payout-formula"]') as HTMLElement)?.innerText ?? '';
            steps.push({ step: 'bill2', hash: location.hash });
            steps.push({ step: 'payout', rows, formula: formula.slice(0, 160) });
            found = true;
            return { ok: true, steps, menuVisits, startHash };
          }
        }
        if (!found) continue;
      }
      return { fail: 'no bill-targeted audit link leads to a station with payout', tried: MAX_TRY };
    });
    log(`  動線：${JSON.stringify(journey)}`);
    check('稽核 → 單 → 案場 → 策略 → 應付，全程不回選單',
      (journey as any)?.ok === true, journey);
    // 應付要看得到**算式**（參數快照 ← 輸入快照），不只是一個金額。
    check('應付紀錄看得到策略版本與參數快照',
      /\{.*\}/.test(((journey as any)?.steps ?? []).find((x: any) => x.step === 'payout')?.formula ?? ''),
      ((journey as any)?.steps ?? []).find((x: any) => x.step === 'payout'));
    await shot(page, '14-journey-payout');

    /**
     * **⑮ 效期綁定管理**與**⑯ P12 流程單**已經搬走了（2026-08-29）。
     *
     * | 搬去哪 | 為什麼 |
     * |---|---|
     * | `scripts/crawler-energy-ops-binding` | ⑮ |
     * | `scripts/crawler-energy-ops-executions` | ⑯ / ⑯-c / ⑯-d |
     *
     * 搬的理由不是「這一支太長」，是**那兩段從來沒有真的跑過**：
     * 這一支連續兩次都在 101/131 被瀏覽器崩掉帶走（`TargetCloseError`），
     * 而崩掉的時候 failed 是 0 —— 報表上「沒跑到」跟「跑過而且綠」
     * 長得一模一樣。排在最後面的東西，等於沒有守衛。
     *
     * 拆成兩支小的之後，它們各自有出口碼、各自有截圖，
     * 而且**跑一次一兩分鐘**，改到那一塊時真的會有人去跑。
     */
    /**
     * 其他**不在這裡**的切片驗收（一樣是拆出去的小 crawler）：
     *
     * | 切片 | 在哪 |
     * |---|---|
     * | §7.3 我的工作兩頁籤 ／ §7.4 例外件併回電費單 | `crawler-energy-ops-mywork` |
     * | §6.8 單據詳情頁五模式 | `crawler-energy-ops-modes` |
     * | 稽核頁的事件句子（中／英） | `crawler-energy-ops-audit-i18n` |
     *
     * 這一支是**全套回歸**。切片級的驗收放進來只會讓內圈更慢，
     * 而這一輪已經有兩次 15 分鐘是花在「crawler 自己有語法錯」
     * 與「頁面 render 就炸」上 —— 不要用十幾分鐘的東西當內圈。
     */
    hr('⑥ 伺服器');
    check('沒有 5xx', serverErrors.length === 0, [...new Set(serverErrors)]);
    // 4xx 不一定是錯（403 探權限、409 防自核都是預期的）——
    // 但**印出來**：查「按了沒反應」時它是第一手線索。
    log(`  4xx（僅供診斷）：${JSON.stringify([...new Set(clientErrors)])}`);
  } finally {
    await browser.close();
  }

  board.finish('多帳號鏈路與跨頁驗收');
}

run().catch((e) => { console.error(e); process.exitCode = 1; });
