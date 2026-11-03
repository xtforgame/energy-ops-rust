/* eslint-disable no-console, no-await-in-loop */
/**
 * **LIFECYCLE 【6】的驗收：單據詳情頁的五個模式**（§6.8 ＋ §6.7.2-3）。
 *
 * 後端的 `mode` 判定在【3】就做好了，這一支驗的是**前端把它消費掉了**：
 *
 * | 依 mode 變的三處 | 來源 |
 * |---|---|
 * | 欄位可編輯性 | `canEditSubject` |
 * | 動作列（空的話印那一句） | `availableActions` / `modeReason` |
 * | 附件頁籤的「補件」 | `canAddAttachment`（比 `canEditSubject` **寬**） |
 *
 * 左欄的原檔**不依 mode 變**：五個模式都要有，沒有例外 ——
 * 主管核准的動作本質上就是拿螢幕上的數字對紙上的數字，看不到紙就沒辦法核。
 *
 * ### 這一支怎麼跑
 *
 * 一張單走完整條路，每換一個模式就照一張左欄：
 *
 * ```
 * intake（operator@，還沒送審）
 *   → 送審 → revise（operator@ 自己在 clerk-review 就改得動）
 *   → 送出 → readonly／excludedOwner（operator@ 打開停在**主管核准**的自己的單）
 *   → 主管退回 → revise（回到原送件者手上，他才是看過那張紙的人）
 *   → act（manager@ 在 manager-approve）＋ 補件
 *   → 走完 → readonly／finished
 * ```
 *
 * ### `revise` 落在誰身上（2026-08-29 改過）
 *
 * `clerk-review` 的 `allowSelfApproval` 從 false 翻成 **true**。
 *
 * 這一關同時承擔「改」與「核」兩種語意，而它主要是**改**。
 * 設成 false 的後果是：被退回的單，**原送件者改不了** ——
 * 退回理由傳不到唯一知道原委的人手上。
 *
 * 四眼原則的價值在**核可**環節，所以 `manager-approve` /
 * `finance-release` / `payment` 三關一律保持 false ——
 * 這一支的 ③ 就是在證明那三關沒被一起放寬。
 *
 * 移植自 az `scripts/crawler-energy-ops-modes`。跑法：`web/e2e/run-crawlers.sh modes`。
 *
 * **可重複跑**：每次自己上傳一張新的單（檔名帶時間戳 → 不同的業務身分），
 * 所有斷言都只看那一張，不看全域的絕對數字。
 */
import type { Page } from 'puppeteer';
import {
  Scoreboard, hr, log, sleep, launch, shooter, loginAs, selectOrg, goto,
  seedBatch, writeUploadFiles, hardenPage,
  waitIdle, eapi, demoEmail,
} from '../lib/energyOpsHarness.ts';

const board = new Scoreboard();
const check = board.check.bind(board);
const shot = shooter('energy-modes');

/** 詳情頁上「這一頁現在長什麼樣」的一次快照 —— 五個模式都用同一組欄位比。 */
async function snapshot(page: Page) {
  return page.evaluate(() => {
    const el = (s: string) => document.querySelector(s) as HTMLElement | null;
    const amount = el('[data-testid="final-totalAmount"]') as HTMLInputElement | null;
    return {
      hash: window.location.hash,
      mode: el('[data-testid="review-mode-reason"]')?.dataset.mode
        ?? el('[data-testid="review-actions-reason"]')?.dataset.mode ?? '',
      reasonCode: el('[data-testid="review-mode-reason"]')?.dataset.code
        ?? el('[data-testid="review-actions-reason"]')?.dataset.code ?? '',
      reasonText: el('[data-testid="review-actions-reason"]')?.innerText
        ?? el('[data-testid="review-mode-reason"]')?.innerText ?? '',
      // 欄位可編輯性 ← canEditSubject
      fieldFound: !!amount,
      fieldDisabled: amount ? amount.disabled : null,
      saveDisabled: (el('[data-testid="review-save"]') as HTMLButtonElement | null)?.disabled ?? null,
      // 動作列 ← availableActions
      actionCount: Number(el('[data-testid="review-actions"]')?.dataset.count ?? '-1'),
      actionButtons: Array.from(document.querySelectorAll('[data-testid="review-actions"] button'))
        .map((b) => (b as HTMLElement).innerText.trim()).filter(Boolean),
      // 左欄原檔 —— 五個模式都要有
      hasSource: !!el('[data-testid="review-open-source"]'),
      sourceId: el('[data-testid="review-source"]')?.dataset.attachment ?? '',
      // 補件 ← canAddAttachment（比 canEditSubject 寬）
      hasAddAttachment: !!el('[data-testid="review-add-attachment"]'),
      hasSubmit: !!el('[data-testid="review-submit"]'),
      noBusinessKey: !!el('[data-testid="review-no-business-key"]'),
    };
  });
}

/** 那一批產生的電費單 id。 */
async function billOfBatch(page: Page, batchId: number): Promise<number> {
  const r = await eapi(page, 'GET', `/ingest-batches/${batchId}`);
  if (r.status !== 200) return 0;
  const item = (r.data?.items ?? []).find((i: any) => i.billId != null);
  return item ? Number(item.billId) : 0;
}

/** 開附件頁籤（它是第 0 個，但切過別的頁籤之後要切回來）。 */
async function openAttachmentsTab(page: Page) {
  await page.evaluate(() => {
    const tab = Array.from(document.querySelectorAll('[role=tab]'))[0] as HTMLElement | undefined;
    tab?.click();
  });
  await waitIdle(page);
}

/** 在工作台上對這一筆執行做一個動作（認領 → 按下去）。 */
/**
 * 在「我的工作」上把一筆推進一關。
 *
 * **動線在【9】改過**（`LIFECYCLE.md §8.5`）：〔認領〕已經從列上移走，
 * 它是「開始處理」的第一步、只存在於抽屜裡。
 *
 * 前一版還在列上找「認領」按鈕 —— 找不到就靜默跳過，
 * 於是沒認領就去按動作、推進失敗，而畫面看起來像「核可關卡沒有動作按鈕」。
 * **那個紅燈長得跟產品壞掉一模一樣**，其實是這支 helper 沒跟上動線。
 */
/**
 * 在工作台上對**指定的那一張單**做一個動作。
 *
 * **不可以「按第一列」**：佇列裡本來就會有別的東西
 * （demo seed 自己就發起了一筆停在承辦確認關的執行）。
 * 按第一列的後果是**推進了別人的單**，然後這一支後面每一條斷言都在問
 * 「為什麼我的單還停在原地」—— 而畫面上看起來像核准功能壞了。
 * 2026-09-08 實際踩到：17 條紅燈，根因是這一行。
 */
async function actOnWorkbench(page: Page, actionText: string, billId: number, reason?: string) {
  await goto(page, 'energyMyWorkConsole', '[data-testid="workbench-row"], .MuiAlert-root');
  await waitIdle(page);

  // 整列可點 → 開抽屜。**只點 `data-bill` 對得上的那一列。**
  const opened = await page.evaluate((bid: number) => {
    const rows = Array.from(document.querySelectorAll('[data-testid="workbench-row"]'));
    const row = rows.find((r) => (r as HTMLElement).dataset.bill === String(bid)) as HTMLElement | undefined;
    if (!row) return false;
    row.click();
    return true;
  }, billId);
  if (!opened) return false;
  await waitIdle(page);

  // 抽屜裡：先認領（未認領時才有這顆）
  const needClaim = await page.$('[data-testid="queue-claim"]');
  if (needClaim) {
    await page.evaluate(() => {
      (document.querySelector('[data-testid="queue-claim"]') as HTMLElement | null)?.click();
    });
    await waitIdle(page);
  }

  // 抽屜裡：按那個動作。用**並關閉**那一顆 —— 這支 helper 要的是
  // 「推進一關然後回到列表」，不是接著處理下一筆。
  const clicked = await page.evaluate(async (t: string) => {
    const wait = (ms: number) => new Promise((res) => { setTimeout(res, ms); });
    const btns = Array.from(document.querySelectorAll(
      '[data-testid="queue-act-close"], [data-testid="queue-act-other"]',
    ));
    const btn = btns.find((b) => (b as HTMLElement).innerText.includes(t));
    if (!btn) return false;
    (btn as HTMLElement).click();
    await wait(1500);
    return true;
  }, actionText);

  if (reason) {
    // 需要理由的動作會先開對話框 —— 稽核的理由欄不能是空的。
    // 它疊在抽屜之上，關掉後回到抽屜（`LIFECYCLE.md §8.5`）。
    const box = await page.$('.MuiDialog-root textarea');
    if (box) await box.type(reason, { delay: 8 });
    await page.evaluate(() => {
      const b = Array.from(document.querySelectorAll('.MuiDialogActions-root button'))
        .find((x) => !(x as HTMLButtonElement).disabled
          && !(x as HTMLElement).innerText.includes('取消'));
      (b as HTMLElement | undefined)?.click();
    });
  }
  await waitIdle(page);
  // 收尾：關掉抽屜，讓呼叫端回到乾淨的列表狀態。
  await page.keyboard.press('Escape');
  await waitIdle(page);
  return clicked;
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
    // ── 準備：走 UI 上傳一張新的單 ─────────────────────────────
    hr('準備：上傳一張這一輪專用的單');
    await loginAs(page, demoEmail('operator'));
    check('選得到 demo 組織', (await selectOrg(page)) != null);

    // 檔名帶時間戳 → 每次跑都是**不同的一張紙**（fixture 的計費期間由檔名決定），
    // 所以這一支可以重複跑而不會撞到去重。
    const stamp = `${Date.now()}`.slice(-8);
    // 不走 dropzone：假圖會被真的辨識服務判成 unclassified（見 `seedBatch`）。
    // 這一支要的是「同一張紙送兩次」，不是驗上傳。
    const file = `bill-small-modes-${stamp}.png`;
    const batch = await seedBatch(page, [file]);
    check('開得出一批送件', batch.ok, batch);

    const billId: number = await billOfBatch(page, batch.batchId);
    check('那一批產生了電費單', billId > 0, { billId });

    // ── ① intake ────────────────────────────────────────────
    hr('① intake（承辦，還沒送審）');
    await goto(page, `energyBillConsole/${billId}/show`, '[data-testid="review-field"], table');
    await waitIdle(page);
    const intake = await snapshot(page);
    check('intake：欄位可編輯', intake.fieldFound && intake.fieldDisabled === false, intake);
    check('intake：左欄看得到原檔', intake.hasSource, intake);
    check('intake：有「送審」的出口', intake.hasSubmit, intake);
    check('這一張算得出業務身分 → 不該有「無法判斷重複」的警示',
      intake.noBusinessKey === false, intake);
    await shot(page, '1-intake');

    /**
     * **「這張單沒有經過重複檢查」的警示**（`LIFECYCLE.md §6.3.5`）。
     *
     * `businessKey` 為 null ＝ 抽不到電號或計費期間 → 去重整段**放行**。
     * 那是刻意的（不然那張單永遠送不出去），但**核准的人也需要知道** ——
     * 他按下去的那一刻是最後一道關，而系統在這一張上沒有幫他擋過任何東西。
     *
     * 走真實路徑製造它：校對時把計費期間清掉（`correct()` 會重算 businessKey）。
     * 不直接改 DB —— 那樣驗不到「重算」這一段。
     */
    hr('①-b businessKey 為 null → 標「無法判斷是否重複」');
    // 先記下原值 —— 等一下要**原封不動**補回去。
    // 補成別的日期的話這張單的 `businessKey` 就變了，
    // 下一段「送同一張紙的第二份」會撞不到，然後多一筆執行在流程上跑，
    // 把後面整條鏈路推到別張單上（那正是 seed 註解裡記過的坑）。
    const originalFrom: string = await (async () => {
      const r = await eapi(page, 'GET', `/bills/${billId}`);
      return r.status === 200 ? String(r.data?.billingPeriodStart ?? '').slice(0, 10) : '';
    })();
    check('讀得到原本的計費期間（起）', /^\d{4}-\d{2}-\d{2}$/.test(originalFrom), { originalFrom });

    const cleared = (await eapi(page, 'PATCH', `/bills/${billId}`, { billingPeriodStart: null })).status;
    check('清掉計費期間（校對允許）', cleared === 200, { cleared });
    await goto(page, `energyBillConsole/${billId}/show`, '[data-testid="review-field"], table');
    await waitIdle(page);
    const noKey = await snapshot(page);
    check('標出「無法判斷是否重複」', noKey.noBusinessKey === true, noKey);
    await shot(page, '1b-no-business-key');

    // 補回去 —— 後面的流程要能正常跑完。
    const restored = (await eapi(page, 'PATCH', `/bills/${billId}`, { billingPeriodStart: originalFrom })).status;
    check('補回計費期間', restored === 200, { restored });
    await goto(page, `energyBillConsole/${billId}/show`, '[data-testid="review-field"], table');
    await waitIdle(page);
    const back = await snapshot(page);
    check('警示跟著消失（businessKey 真的重算了）', back.noBusinessKey === false, back);

    // ── ①-c 跳 popup 時畫面不可以位移 ──────────────────────────
    /**
     * 使用者回報：「只要下面跳出『已標記為已校對』這種 popup，畫面都會位移閃動一下」。
     *
     * **抖動跟 popup 沒有關係** —— snackbar 是 `position: fixed`，量過，
     * 水平垂直都不推版面。真正的原因是同一個動作觸發的 `load()`：
     * `EnergyPageFrame` 原本把 loading 的 spinner 當成**正常元素**插在內容**上面**，
     * `loading` 一 true 就多出 56px，底下全部往下掉，載完移除又彈回來。
     *
     * 每一個成功動作都是「`notify()` ＋ `load()`」，所以每次跳 popup 都抖一次。
     *
     * 這一條盯的是**內容的絕對位置在整個動作期間不變**，
     * 不是「有沒有 spinner」—— 後者用藏掉 spinner 也能過，而那是另一種壞。
     */
    hr('①-c 跳 popup ／ 重新載入時，內容不位移');
    const shift = await page.evaluate(async () => {
      const topOf = () => {
        const el = document.querySelector('[data-testid="review-field"]') as HTMLElement | null;
        return el ? +el.getBoundingClientRect().top.toFixed(2) : null;
      };
      const before = topOf();
      const tops: number[] = [];
      (document.querySelector('[data-testid="review-mark"]') as HTMLElement | null)?.click();
      const t0 = performance.now();
      // 看**過程中的極值**，不是只比頭尾 —— 頭尾相同不代表中間沒跳過。
      while (performance.now() - t0 < 1800) {
        const t = topOf();
        if (t != null) tops.push(t);
        await new Promise((r) => { requestAnimationFrame(() => r(null)); });
      }
      const mark = document.querySelector('[data-testid="review-mark"]') as HTMLButtonElement | null;
      return {
        before, samples: tops.length,
        min: Math.min(...tops), max: Math.max(...tops),
        spread: +(Math.max(...tops) - Math.min(...tops)).toFixed(2),
        // 動作真的發生了嗎 —— 成功之後 reviewStatus 變 reviewed，這顆就 disabled。
        markedNow: !!mark?.disabled,
      };
    });
    log(`  內容位置：${JSON.stringify(shift)}`);
    /**
     * **兩條自檢先擋住兩種假通過**：
     * · 取樣數 0 → 空陣列比大小，`spread` 會是 NaN/0
     * · 動作根本沒發生（按鈕 disabled、權限不足）→ 沒有 load，當然不位移
     */
    check('量得到內容位置（不然下面那條沒有意義）', shift.samples > 30, shift);
    check('這一輪真的觸發了重新載入（不然「不位移」是廢話）', shift.markedNow === true, shift);
    check('跳 popup 期間內容零位移（修好之前是 56px）', shift.spread === 0, shift);
    await shot(page, '1c-no-layout-shift');

    // ── ② revise（承辦打開自己送出、停在 clerk-review 的單）──────
    hr('② revise：承辦送審後打開自己的單（clerk-review 允許自核）');
    await page.evaluate(() => {
      (document.querySelector('[data-testid="review-submit"]') as HTMLElement | null)?.click();
    });
    await waitIdle(page);
    await goto(page, `energyBillConsole/${billId}/show`, '[data-testid="review-field"], table');
    await waitIdle(page);
    const ownerAfter = await snapshot(page);
    /**
     * `clerk-review` 是**改**的關卡，不是**核**的關卡 ——
     * 原送件者在這裡拿得到 `revise`。
     */
    check('承辦打開自己送出的單：有動作可按（不是死路一條）',
      ownerAfter.actionButtons.length > 0, ownerAfter);
    check('欄位可編輯（allowsSubjectEdit ＋ 自核放行）',
      ownerAfter.fieldDisabled === false, ownerAfter);
    check('revise：左欄一樣看得到原檔', ownerAfter.hasSource, ownerAfter);
    check('revise：補件也開著', ownerAfter.hasAddAttachment, ownerAfter);
    await shot(page, '2-revise-owner');

    // ── ②-b 三種重複之一：already_in_review ────────────────────
    /**
     * 送**同一張紙**的第二份（同檔名 → 同 `businessKey`）。
     *
     * 三句話不共用：對 `already_in_review` 說「已經送審過了」會讓人以為結案，
     * 而它其實還可能被退回 —— 那時這張紙又變成有用的。
     */
    hr('②-b 重複送審：already_in_review 的畫面');
    const twin = await seedBatch(page, [file]);
    const twinBillId: number = await billOfBatch(page, twin.batchId);
    check('第二份也產生了電費單', twinBillId > 0 && twinBillId !== billId, { billId, twinBillId });

    await goto(page, `energyBillConsole/${twinBillId}/show`, '[data-testid="review-field"], table');
    await waitIdle(page);
    await page.evaluate(() => {
      (document.querySelector('[data-testid="review-submit"]') as HTMLElement | null)?.click();
    });
    await waitIdle(page);
    const dupDlg = await page.evaluate(() => {
      const body = document.querySelector('[data-testid="review-dup-body"]') as HTMLElement | null;
      return {
        open: !!body,
        code: body?.dataset.code ?? '',
        title: (document.querySelector('[data-testid="review-dup-title"]') as HTMLElement)?.innerText ?? '',
        text: body?.innerText ?? '',
        seeThatOne: !!document.querySelector('[data-testid="review-dup-goto"]'),
        discardThis: !!document.querySelector('[data-testid="review-dup-discard"]'),
      };
    });
    check('撞到「正在審核中」→ 出現重複說明',
      dupDlg.open && dupDlg.code === 'already_in_review', dupDlg);
    // 只說「被擋了」而不給出口，使用者唯一能做的就是關掉視窗然後困惑。
    check('兩顆出口都在：[看那一筆] ＋ [捨棄這一份]',
      dupDlg.seeThatOne && dupDlg.discardThis, dupDlg);
    check('那一句講得出「還可能被退回」（不是「已經通過了」）',
      /退回/.test(dupDlg.text), dupDlg.text);
    await shot(page, '2b-duplicate-in-review');
    await page.evaluate(() => {
      (document.querySelector('[data-testid="review-dup-discard"]') as HTMLElement | null)?.click();
    });
    await waitIdle(page);
    /**
     * **第二份絕對不可以留在流程上**。
     *
     * 如果它真的被送出去了（撞單沒生效），後面每一步的「按第一顆按鈕」
     * 都會推到**它**身上，然後主鏈路卡住 —— 而症狀看起來像
     * 「核准功能壞了」，跟根因差了十萬八千里。
     */
    const twinExec = await (async () => {
      const r = await eapi(page, 'GET', `/bills/${twinBillId}`);
      return r.status === 200 && Array.isArray(r.data?.executions) ? r.data.executions.length : -1;
    })();
    check('第二份沒有進入流程（不然後面會推到它身上）', twinExec === 0, { twinExec });

    // ── ③ readonly／excludedOwner（**核可**關卡照樣防自核）─────────
    /**
     * 放寬的只有 `clerk-review` 一關。
     * 同一個承辦把自己的單推到**主管核准**之後再打開它 ——
     * 這一次必須是唯讀、零動作、而且講得出為什麼。
     *
     * 一個什麼都不能按又不解釋的頁面，使用者只會以為它壞了。
     */
    hr('③ readonly：同一張單停在「主管核准」，承辦自己打開');
    check('承辦把 clerk-review 推過去', await actOnWorkbench(page, '送出', billId));
    await goto(page, `energyBillConsole/${billId}/show`, '[data-testid="review-field"], table');
    await waitIdle(page);
    const ownerAtGate = await snapshot(page);
    check('停在核可關：沒有動作按鈕',
      ownerAtGate.actionButtons.length === 0, ownerAtGate);
    check('而且印出了 modeReason 那一句', ownerAtGate.reasonText.trim().length > 6, ownerAtGate);
    check('說法是「防自核」不是含糊的一句',
      ownerAtGate.reasonCode === 'excludedOwner', ownerAtGate);
    check('欄位變唯讀', ownerAtGate.fieldDisabled === true, ownerAtGate);
    check('readonly：左欄一樣看得到原檔', ownerAtGate.hasSource, ownerAtGate);
    await shot(page, '3-readonly-owner-at-gate');

    // ── ③-b 退回之後，改單的權回到原送件者手上 ────────────────────
    /**
     * 這一段就是把 `allowSelfApproval` 翻面的**理由**釘在地上：
     * 主管退回 → 退回理由要傳到**唯一看過那張紙的人**手上，
     * 而他就是原送件者。他改不了的話，那句退回理由等於沒有收件人。
     */
    hr('③-b 主管退回 → 原送件者改得動');
    await loginAs(page, demoEmail('manager'));
    await selectOrg(page);
    check('主管退回得動', await actOnWorkbench(page, '退回', billId, '金額跟紙上對不起來，請重新核對'));
    await loginAs(page, demoEmail('operator'));
    await selectOrg(page);
    await goto(page, `energyBillConsole/${billId}/show`, '[data-testid="review-field"], table');
    await waitIdle(page);
    const revise = await snapshot(page);
    check('被退回後：欄位可編輯（節點旗標 allowsSubjectEdit）',
      revise.fieldDisabled === false, revise);
    /**
     * **退回之後那一關是「沒人認領」的狀態**，所以這一頁先給的是
     * 〔認領並開始處理〕，認領完才出現「送出」（＝重新送審）。
     *
     * 這與佇列抽屜是同一套語意（`LIFECYCLE.md §8.5`：認領是「開始處理」的第一步）。
     * 在此之前這一頁沒有認領入口，於是它直接畫「送出」—— 按下去 403。
     * 兩顆都接受，但**至少要有一顆**：一個什麼都不能按的頁面，
     * 對被退回的原送件者來說就是死路。
     */
    check('被退回後：有出口可按（認領 → 送出，＝重新送審）',
      revise.actionButtons.some((t) => t.includes('送出'))
        || revise.actionButtons.some((t) => t.includes('認領')),
      revise.actionButtons);
    check('被退回後：左欄看得到原檔', revise.hasSource, revise);
    check('被退回後：補件也開著（canAddAttachment 不比 canEditSubject 窄）',
      revise.hasAddAttachment, revise);
    await shot(page, '3b-revise-after-sendback');

    // ── ④ act（主管核准那一關：唯讀 ＋ 有動作 ＋ 可補件）──────────
    hr('④ act：輪到的人，欄位唯讀但補得了件');
    check('原送件者重新送審', await actOnWorkbench(page, '送出', billId));
    await loginAs(page, demoEmail('manager'));
    await selectOrg(page);
    await goto(page, `energyBillConsole/${billId}/show`, '[data-testid="review-field"], table');
    await waitIdle(page);
    const act = await snapshot(page);
    check('act：有動作按鈕', act.actionButtons.length > 0, act.actionButtons);
    /**
     * 驗收 2 的核心：**`canAddAttachment` 比 `canEditSubject` 寬**。
     * act 不能改數字（那是送件者的事），但主管說「缺一張證明」時
     * 要有地方上傳 —— 沒有的話那句話就變成死路。
     */
    check('act：欄位唯讀', act.fieldDisabled === true, act);
    check('act：但補件開著（canAddAttachment 比 canEditSubject 寬）',
      act.hasAddAttachment, act);
    check('act：左欄看得到原檔', act.hasSource, act);
    await shot(page, '4-act');

    // ── ⑤ 補件（驗收 6）────────────────────────────────────────
    hr('⑤ act 模式補件');
    await openAttachmentsTab(page);
    const beforeAdded = await page.$$eval(
      '[data-testid="review-attachment"][data-role="added"]', (e) => e.length,
    );
    const [proof] = writeUploadFiles('modes-files', [`proof-${stamp}.png`]);
    const input = await page.$('[data-testid="review-added-input"]');
    check('act 模式看得到補件的輸入', !!input);
    if (input) await (input as any).uploadFile(proof);
    await waitIdle(page);
    await openAttachmentsTab(page);
    const afterAdded = await page.$$eval(
      '[data-testid="review-attachment"][data-role="added"]', (e) => e.length,
    );
    check('附件頁籤多一筆 role=added', afterAdded === beforeAdded + 1, { beforeAdded, afterAdded });

    // 截圖要**照到它宣稱的東西** —— 附件表在摺線以下，不捲過去的話
    // 那張圖只是又一張頁首，證明不了任何事。
    await page.evaluate(() => {
      document.querySelector('[data-testid="review-attachment"]')
        ?.scrollIntoView({ block: 'center' });
    });
    await sleep(600);
    await shot(page, '5-act-attachment');

    // ── ⑥ 唯讀身分（驗收 3）────────────────────────────────────
    hr('⑥ 唯讀身分打開同一張單');
    await loginAs(page, demoEmail('viewer'));
    await selectOrg(page);
    await goto(page, `energyBillConsole/${billId}/show`, '[data-testid="review-field"], table');
    await waitIdle(page);
    const viewer = await snapshot(page);
    check('唯讀：沒有動作按鈕', viewer.actionButtons.length === 0, viewer);
    check('唯讀：沒有補件', viewer.hasAddAttachment === false, viewer);
    check('唯讀：欄位唯讀', viewer.fieldDisabled === true, viewer);
    /**
     * 對唯讀身分說「這一關由『業務部主管』處理」是**誤導的** ——
     * 那句話暗示他等一下會輪到，而他永遠不會。
     */
    check('唯讀：說的是「你是唯讀身分」', viewer.reasonCode === 'viewOnly', viewer);
    check('唯讀：左欄一樣看得到原檔', viewer.hasSource, viewer);
    await shot(page, '6-readonly-viewer');

    /**
     * 補件的稽核（驗收 6 的後半）**用唯讀身分查**。
     *
     * 不能用剛剛補件的 `manager@` 查 —— 他是 `energy:operator`，
     * **沒有 `energy:audit:view`**，`GET /audit-events` 會回 403。
     * 「能做這個動作」與「能看這個動作的紀錄」是兩種權限，
     * 而唯讀角色（`energy:oversight`）拿的正是後者。
     */
    const auditHit = await (async () => {
      const r = await eapi(page, 'GET', '/audit-events?limit=200');
      if (r.status !== 200) return { status: r.status };
      const hit = (r.data?.items ?? []).filter((x: any) => x.type === 'flow.attachment_added');
      return { count: hit.length, severity: hit[0]?.severity ?? '' };
    })();
    check('稽核有 flow.attachment_added', ((auditHit as any).count ?? 0) > 0, auditHit);
    // 補件**不推進流程、不解除任何鎖** —— 所以它是 `info` 不是 `notable`。
    check('它是 info（補件不推進流程、不解除鎖）',
      String((auditHit as any).severity ?? '') === 'info', auditHit);

    // ── ⑦ 走完的單 → readonly（驗收 7）─────────────────────────
    hr('⑦ 走完的單 → readonly');
    await loginAs(page, demoEmail('manager'));
    await selectOrg(page);
    await actOnWorkbench(page, '核准', billId);
    await loginAs(page, demoEmail('finance'));
    await selectOrg(page);
    await actOnWorkbench(page, '放行', billId);
    await loginAs(page, demoEmail('accounting'));
    await selectOrg(page);
    // 付款關：**先鎖副作用再推進** —— 順序反過來會出現「已放行但沒有付款紀錄」的鬼單。
    await actOnWorkbench(page, '標記已付款', billId);
    await actOnWorkbench(page, '送出', billId);

    const finalStatus = await (async () => {
      const r = await eapi(page, 'GET', `/bills/${billId}`);
      if (r.status !== 200) return { status: r.status };
      const b = r.data;
      return { mode: b.mode, code: b.modeReason?.code, exec: b.executions?.[0]?.status };
    })();
    check('這一張真的走完了', (finalStatus as any).exec === 'completed', finalStatus);
    /**
     * 驗收 7：**已完成的單任何人都不能改** ——
     * 就算它停在一個 `allowsSubjectEdit` 的節點上也一樣
     * （`resolveMode` 第 3 步排在 4/5 之前）。
     */
    check('走完的單 → readonly / finished',
      (finalStatus as any).mode === 'readonly' && (finalStatus as any).code === 'finished', finalStatus);

    await goto(page, `energyBillConsole/${billId}/show`, '[data-testid="review-field"], table');
    await waitIdle(page);
    const done = await snapshot(page);
    check('走完的單：沒有動作按鈕', done.actionButtons.length === 0, done);
    check('走完的單：有「已完成」那一句', done.reasonCode === 'finished', done);
    /**
     * 那一句裡的**日期要是日期**。
     *
     * Sequelize 的 `date` 欄位回來是 JS `Date` 物件，
     * `String(date).slice(0, 10)` 會印成 `Sat Aug 29` —— 型別檢查不擋、
     * 單元測試也沒人斷言過那個字串，**只有看畫面才看得出來**。
     */
    check('那一句裡印的是日期不是 `Sat Aug 29`',
      /\d{4}-\d{2}-\d{2}/.test(done.reasonText), done.reasonText);
    check('走完的單：左欄一樣看得到原檔（第五張）', done.hasSource, done);
    await shot(page, '7-readonly-finished');

    hr('伺服器');
    check('沒有 5xx', serverErrors.length === 0, [...new Set(serverErrors)]);
  } finally {
    await browser.close();
  }

  board.finish('LIFECYCLE 【6】單據詳情頁五模式');
}

run().catch((e) => { console.error(e); process.exitCode = 1; });
