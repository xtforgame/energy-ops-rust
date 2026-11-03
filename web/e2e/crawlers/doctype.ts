/* eslint-disable no-console, no-await-in-loop */
/**
 * **單別可改 ＋ 一張單開多條流程**的驗收（2026-09-09，使用者要求）。
 *
 * 使用者的原話：
 *
 * > 他現在會顯示為：「依目前金額，這張單會走：電費單核可流程（2026 版） v2」，
 * > AI 幫我判定了，但完全沒有給我改的機會。他在某種意義上也是 AI 識別結果，
 * > 需要如同其他欄位讓使用者可以修改…我們要依照文件類型和地主契約
 * > 來決定這份文件要開啟哪些流程。
 *
 * 所以這一支要證明四件事，而且都是**在畫面上真的操作**：
 *
 * 1. 四份範本被判成**四種不同的單別**（不是兩種）。
 * 2. 校對台上單別是一個**可以改的欄位**，而且看得到「辨識判為什麼」。
 * 3. 「這張單會開哪些流程」是**清單**，而且改了單別**當場就變**。
 * 4. 送審會把該開的流程**全部**開起來（有餘電那張：核可 ＋ 開發票）。
 *
 * 移植自 az `scripts/crawler-energy-ops-doctype`。跑法：`web/e2e/run-crawlers.sh doctype`
 * （示範資料＋範本電費單；每次跑都重建資料庫，因為這支會改資料：改單別、送審一張單）。
 */
import fs from 'node:fs';
import path from 'node:path';
import type { Page } from 'puppeteer';
import {
  Scoreboard, hr, log, launch, shooter, loginAs, selectOrg, goto,
  hardenPage, waitIdle, waitUntil, placeholderPdf, eapi, demoEmail, samplePath, REPO_ROOT,
} from '../lib/energyOpsHarness.ts';

const board = new Scoreboard();
const check = board.check.bind(board);
const shot = shooter('energy-doctype');

/** 要開兩條流程的那一份範本（檔名自己就寫著「需開發票」）。 */
const SURPLUS_FILE = '電費單(有餘電需開發票).pdf';

/** 這個組織的四張示範單（單別 ＋ 電號）。 */
async function bills(page: Page): Promise<any[]> {
  const r = await eapi(page, 'GET', '/bills?limit=50');
  return r.status === 200 ? (r.data?.items ?? []) : [];
}

/** 一張單的詳情（含 `binding.flows`）。 */
async function billDetail(page: Page, billId: number): Promise<any> {
  const r = await eapi(page, 'GET', `/bills/${billId}`);
  return r.status === 200 ? r.data : null;
}

async function run() {
  const browser = await launch();
  const page = hardenPage(await browser.newPage());
  const serverErrors: string[] = [];
  page.on('response', (r) => {
    if (r.status() >= 500) serverErrors.push(`${r.status()} ${new URL(r.url()).pathname}`);
  });

  try {
    hr('① 四份範本 → 四種單別');
    await loginAs(page, demoEmail('operator'));
    check('選得到 demo 組織', (await selectOrg(page)) != null);

    const rows = await bills(page);
    const byDocType = new Map<string, any>();
    rows.forEach((b: any) => { if (b.docType) byDocType.set(b.docType, b); });
    log('  單別分佈：', [...byDocType.keys()].join(', ') || '(空)');
    check('示範資料有四張單', rows.length >= 4, rows.length);
    // **四種**不是兩種 —— 版型只有兩種，但後續作業是四種。
    check('四張單是四種不同的單別', byDocType.size === 4, [...byDocType.keys()]);
    check('躉購·有餘電那一張在（它是要開兩條流程的那張）',
      byDocType.has('renewableSurplus'), [...byDocType.keys()]);
    check('逆變器·含電器租費那一張在（判別欄位是「電器租費」那一列）',
      byDocType.has('consumptionDeviceRental'), [...byDocType.keys()]);

    const surplus = byDocType.get('renewableSurplus');
    const detail = await billDetail(page, Number(surplus.id));
    check('後端回得出「會開哪些流程」的清單', Array.isArray(detail?.binding?.flows), detail?.binding);
    const flowKeys = (detail?.binding?.flows ?? []).map((f: any) => f.workflowKey).sort();
    check('有餘電那張會開**兩條**：核可 ＋ 開發票',
      flowKeys.join(',') === 'bill-approval,invoice-issue', flowKeys);

    // ── ② 校對台：單別是可以改的欄位 ─────────────────────────────
    //
    // 用**還沒送審**的那一張（示範資料裡的「已校對未送審」）：
    // 已經在跑的單本來就鎖著單別，拿它驗「改得動」等於驗錯對象。
    hr('② 校對台上的單別（未送審那張）');
    const draft = rows.find((b: any) => !b.executionId && b.stationId
      && b.docType && b.docType !== 'renewableSurplus');
    check('找得到一張未送審、已配到案場的單', !!draft, draft?.id ?? null);
    const draftId = Number(draft?.id ?? 0);

    await goto(page, `energyBillConsole/${draftId}/show`, '[data-testid="bill-doc-type"]');
    await waitIdle(page);
    const ui = await page.evaluate(() => {
      const box = document.querySelector('[data-testid="bill-doc-type"]') as HTMLElement | null;
      const combo = box?.querySelector('[role="combobox"]') as HTMLElement | null;
      const flows = Array.from(document.querySelectorAll('[data-testid="bill-flow"]'))
        .map((c) => (c as HTMLElement).dataset.workflow);
      return {
        exists: !!box,
        docType: box?.dataset.docType ?? '',
        text: box?.innerText ?? '',
        // MUI 的 disabled 會掛在 combobox 上（`aria-disabled`）。
        disabled: combo?.getAttribute('aria-disabled') === 'true',
        flows,
      };
    });
    check('校對台上有「單別」這一格', ui.exists, ui);
    check('它顯示的是這張單的單別', ui.docType === draft?.docType, ui.docType);
    check('看得到「辨識判為」', /辨識判為/.test(ui.text), ui.text.slice(0, 120));
    check('單別是**可以改的**（不是一句唯讀的結論）', ui.disabled === false, ui);
    check('會開哪些流程列在畫面上', (ui.flows ?? []).length >= 1, ui.flows);
    check('畫面上沒有 raw docType', !/renewableSurplus|consumptionDevice/.test(ui.text), ui.text.slice(0, 200));

    /**
     * **同一頁不可以自相矛盾**。
     *
     * 下方那張「流程」卡片（金額分支預覽）一度寫死用躉購核可流程解版，
     * 於是一張用電電費單的詳情頁上，上面說「會開：用電電費單付款流程」、
     * 下面說「這張單會走：電費單核可流程 v2」——
     * 而使用者正是拿下面那一句來判斷「送出去會發生什麼」。
     */
    const routeCard = await page.evaluate(() => (
      (document.querySelector('[data-testid="branch-preview"]') as HTMLElement)?.innerText ?? ''
    ));
    // 流程 chip 在契約發動時多帶「 · <契約編號>」（本版的畫面），比對前和版號一起去掉。
    const flowNames = await page.evaluate(() => Array.from(
      document.querySelectorAll('[data-testid="bill-flow"]'),
    ).map((c) => (c as HTMLElement).innerText.replace(/\s*·.*$/, '').replace(/\s*v\d+$/, '').trim()));
    check('下方「流程」卡片講的是同一條流程（不自相矛盾）',
      !routeCard || flowNames.some((n) => n && routeCard.includes(n)),
      { routeCard: routeCard.slice(0, 80), flowNames });
    await shot(page, '1-doctype-field');

    // ── ③ 改單別 → 會開哪些流程當場跟著變 ────────────────────────
    hr('③ 改單別，流程清單跟著變');
    const flowsBefore = (ui.flows ?? []).join(',');
    /** 打開下拉、挑**指名的那一種**單別、等畫面上的值真的變過去。 */
    const pick = async (want: string) => {
      // 上一次的選單要真的關掉、下拉解除 busy 才按下一次：關閉動畫期間按下去，
      // 滑鼠會落在還沒消失的舊選項上，選到的是別的單別。
      await waitUntil(page, () => !document.querySelector('[data-testid="doc-type-option"]')
        && document.querySelector('[data-testid="doc-type-select"] [role="combobox"]')?.getAttribute('aria-disabled') !== 'true',
      { timeout: 15000, label: '單別下拉回到可按的狀態' });
      await waitIdle(page);
      // **用真的滑鼠點** —— MUI 的 Select 是靠 mousedown 開的，
      // 程式化 `.click()` 打不開它（第一版就是這樣紅的：選項數 0）。
      await page.click('[data-testid="doc-type-select"] [role="combobox"]');
      await waitUntil(page, () => !!document.querySelector('[data-testid="doc-type-option"]'),
        { timeout: 15000, label: '單別下拉打開' });
      // 用 `data-doc-type` 指名 —— 比對中文字串會被翻譯與換字咬到
      // （第一版就是這樣選錯選項，把示範資料改成另一種單別）。
      const n = await page.evaluate((w: string) => {
        const opts = Array.from(document.querySelectorAll('[data-testid="doc-type-option"]'));
        const target = opts.find((o) => (o as HTMLElement).dataset.docType === w);
        (target as HTMLElement | undefined)?.click();
        return opts.length;
      }, want);
      // `waitUntil` 不帶參數進頁面 —— 把期望值先放到 window 上再比。
      await page.evaluate((w: string) => { (window as any).__wantDocType = w; }, want);
      const ok = await waitUntil(page, () => (
        (document.querySelector('[data-testid="bill-doc-type"]') as HTMLElement)?.dataset.docType
          === (window as any).__wantDocType
      ), { timeout: 20000, label: `單別變成 ${want}` });
      return { count: n, ok };
    };

    const toSurplus = await pick('renewableSurplus');
    check('下拉裡列得出四種單別', toSurplus.count >= 4, toSurplus.count);
    check('單別真的被改掉了（後端存了）', toSurplus.ok);

    const after = await page.evaluate(() => ({
      flows: Array.from(document.querySelectorAll('[data-testid="bill-flow"]'))
        .map((c) => (c as HTMLElement).dataset.workflow),
      changed: (document.querySelector('[data-testid="bill-doc-type"]') as HTMLElement)?.dataset.changed,
      none: !!document.querySelector('[data-testid="bill-flows-none"]'),
    }));
    log(`  流程：[${flowsBefore}] → [${(after.flows ?? []).join(',')}]`);
    check('流程清單跟著變了（同一張單、換個單別就換一組流程）',
      (after.flows ?? []).join(',') !== flowsBefore, { before: flowsBefore, after: after.flows });
    check('改過的單別會標「已人工修正」', after.changed === '1', after);
    await shot(page, '2-doctype-switched');

    /**
     * 改回**它原本的**單別（這一支要能重複跑，而且不可以弄髒示範資料）。
     *
     * 第一版這裡寫死成「僅設備發電」——但挑到的那張單不一定是那一種，
     * 於是這支 crawler 自己把示範資料改壞了，
     * 而症狀出現在**後面那一段**（「四張單各解出兩條」少一張），
     * 看起來像產品的 bug。
     */
    const back = await pick(String(draft?.docType));
    check('改得回它原本的單別（不弄髒示範資料）', back.ok, draft?.docType);

    // ── ④ 送審 → 該開的流程**全部**開起來 ────────────────────────
    //
    // 用**真的上傳一份範本**：示範資料裡那張有餘電的單早就在跑了
    // （它是「發起執行」那個情境），拿它送審只會得到 `bill_already_submitted`。
    hr('④ 真的上傳一份「有餘電需開發票」→ 送審一次開兩條');
    const file = samplePath(SURPLUS_FILE);
    check('範本檔在', fs.existsSync(file), file);

    await goto(page, 'energyIngestConsole', '[data-testid="intake-dropzone"]');
    const input = await page.$('[data-testid="intake-file-input"]');
    check('收件台有檔案輸入框', !!input);
    await (input as any).uploadFile(file);
    const landed = await waitUntil(page, () => /#\/energyIngestConsole\/\d+/.test(window.location.hash),
      { timeout: 120000, label: '跳到批次詳情頁' });
    check('上傳後跳到批次詳情頁', landed);

    /**
     * **辨識中要看得到「它正在跑」**（2026-09-09，使用者要求）：
     *
     * > 目前識別中的單可以給他一個等待的動畫嗎？使用者比較知道是正在等，
     * > 他不用做事。
     *
     * 所以在辨識**還沒結束的那個視窗內**就要抓到它 ——
     * 等跑完再看是看不到的，而那正是這條斷言唯一有意義的時機。
     */
    // 頁面剛跳過來時 batch 還在載入 —— 等它出現（辨識視窗有十幾秒，夠等）。
    await waitUntil(page, () => !!document.querySelector('[data-testid="recognizing-indicator"]'),
      { timeout: 20000, poll: 300, label: '等待指示出現' });
    const spinner = await page.evaluate(() => {
      const box = document.querySelector('[data-testid="recognizing-indicator"][data-variant="block"]');
      const rows = document.querySelectorAll('[data-testid="recognizing-indicator"][data-variant="inline"]');
      return {
        block: !!box,
        rows: rows.length,
        // 動畫說「還活著」，這句說「你不用做事」——兩件事都要在。
        hint: /不用等在這裡/.test((box as HTMLElement | null)?.innerText ?? ''),
        // MUI 的轉圈是 SVG 動畫 —— 有它才是「會動的」而不是一張靜止圖示。
        spinning: !!box?.querySelector('svg circle'),
      };
    });
    log('  等待指示：', JSON.stringify(spinner));
    if (!spinner.block) {
      const dbg = await page.evaluate(() => ({
        batchStatus: (document.querySelector('[data-testid="intake-batch-status"]') as HTMLElement)?.dataset.status ?? '(無)',
        itemStatuses: Array.from(document.querySelectorAll('[data-testid="intake-item"]'))
          .map((r) => (r as HTMLElement).dataset.status),
      }));
      log('  當下畫面：', JSON.stringify(dbg));
    }
    check('辨識中的批次看得到等待動畫', spinner.block, spinner);
    check('動畫是真的會動的（不是靜止圖示）', spinner.spinning, spinner);
    check('而且說了「不用等在這裡」', spinner.hint, spinner);
    check('每一列還在等的單自己也有指示', spinner.rows >= 1, spinner);
    /**
     * **等待不可以是「整頁一直閃」**。
     *
     * 辨識中每 3 秒會重讀一次批次；那一次重讀如果掀起整頁的載入遮罩，
     * 畫面就會每三秒暗一下、而且那幾百毫秒什麼都點不了 ——
     * 那跟「安心等待」正好相反（第一版截圖就是這樣）。
     * 這裡連看三秒：遮罩一次都不該出現。
     */
    let veiled = 0;
    let backdrops = 0;
    for (let i = 0; i < 12; i += 1) {
      // eslint-disable-next-line no-await-in-loop
      const v = await page.evaluate(() => ({
        page: document.querySelectorAll('[data-testid="page-loading"]').length,
        backdrop: document.querySelectorAll('.MuiBackdrop-root').length,
      }));
      if (v.page > 0) veiled += 1;
      if (v.backdrop > 0) backdrops += 1;
      // eslint-disable-next-line no-await-in-loop
      await new Promise((r) => { setTimeout(r, 300); });
    }
    check('等待期間整頁不會一直閃載入遮罩', veiled === 0, { veiled, backdrops });
    await shot(page, '5-recognizing');

    // 三來源全跑很慢（一頁 10–15 秒）—— 等到批次離開「辨識中」為止。
    const batchStatus = await waitUntil(page, () => {
      const el = document.querySelector('[data-testid="intake-batch-status"]');
      const st = el?.getAttribute('data-status') ?? '';
      return !!st && !['uploading', 'recognizing', 'classifying'].includes(st);
    }, { timeout: 15 * 60 * 1000, poll: 1000, label: '辨識完成' });
    check('辨識跑完了', batchStatus);
    // 跑完就要**消失** —— 一個永遠在轉的圈跟卡住一樣糟。
    const spinnerGone = await page.evaluate(() => (
      document.querySelectorAll('[data-testid="recognizing-indicator"]').length
    ));
    check('辨識完成之後等待指示消失', spinnerGone === 0, spinnerGone);

    const item = await page.evaluate(() => {
      const row = document.querySelector('[data-testid="intake-item"]') as HTMLElement | null;
      const batchId = /#\/energyIngestConsole\/(\d+)/.exec(window.location.hash)?.[1] ?? '';
      return row
        ? { id: Number(row.dataset.itemId), docType: row.dataset.docType ?? '', batchId: Number(batchId) }
        : null;
    });
    log('  新進來的那一筆：', JSON.stringify(item));
    check('新上傳的那份被判成躉購·有餘電', item?.docType === 'renewableSurplus', item);

    /**
     * 換一個計費期間再送 —— 示範資料裡已經有同一張紙（同電號、同期間），
     * 而**同一個業務身分只准一筆在跑**（去重）。這不是繞過檢查：
     * 那本來就是「另一個月的單」的正常樣子。
     */
    const uniqueMonth = `2024-${String((new Date().getMinutes() % 12) + 1).padStart(2, '0')}`;
    const patched = await (async () => {
      // 單一 item 沒有 GET 端點 —— 從批次讀（畫面讀的也是這一份）。
      const r = await eapi(page, 'GET', `/ingest-batches/${item?.batchId}`);
      const batch = r.status === 200 ? r.data : null;
      const it = (batch?.items ?? []).find((x: any) => Number(x.id) === Number(item?.id));
      if (!it?.billId) return { ok: false, batch: batch?.id ?? null, items: (batch?.items ?? []).length } as any;
      const p = await eapi(page, 'PATCH', `/bills/${it.billId}`, {
        billingPeriodStart: `${uniqueMonth}-01`, billingPeriodEnd: `${uniqueMonth}-28`,
      });
      return { ok: p.status === 200, billId: it.billId, status: p.status, body: p.status === 200 ? undefined : p.data };
    })();
    check('把它改成另一個月（避開去重）', patched.ok, patched);

    const newBillId = Number(patched.billId ?? 0);
    const predicted = await billDetail(page, newBillId);
    const predictedKeys = (predicted?.binding?.flows ?? []).map((f: any) => f.workflowKey).sort();
    check('送審**前**就看得到會開兩條', predictedKeys.join(',') === 'bill-approval,invoice-issue', predictedKeys);

    const submitted = await (async () => {
      const r = await eapi(page, 'POST', `/ingest-items/${item?.id}/submit`, {});
      return { ok: r.status === 200, status: r.status, body: JSON.stringify(r.data).slice(0, 200) };
    })();
    log('  送審：', submitted.status, submitted.body);
    check('送審成功', submitted.ok, submitted);

    const detail2 = await billDetail(page, newBillId);
    const execs = (detail2?.executions ?? []).map((e: any) => ({
      id: e.id, status: e.status, name: e.workflowName ?? e.definitionName ?? null,
    }));
    log('  這張單的流程：', JSON.stringify(execs));
    check('這張單上有**兩筆**流程（核可 ＋ 開發票）', execs.length === 2, execs);
    check('兩筆都在跑', execs.every((e: any) => e.status === 'running'), execs);

    await goto(page, `energyBillConsole/${newBillId}/show`, '[data-testid="bill-doc-type"]');
    await waitIdle(page);
    const locked = await page.evaluate(() => {
      const combo = document.querySelector('[data-testid="doc-type-select"] [role="combobox"]');
      return combo?.getAttribute('aria-disabled') === 'true';
    });
    // 已經在跑的單不准改單別 —— 流程已經照舊的單別開了。
    check('送審之後單別就鎖住了（畫面先擋，不是等後端跳錯）', locked);
    await shot(page, '3-after-submit');

    /**
     * ── ⑤ **四份契約 × 兩條流程 = 8 條**（2026-09-09，使用者要求）────────
     *
     * > 我希望你做四個契約，分別對應這四種文件，依照範例的電號綁到對應的案場上，
     * > 然後每個契約還會因應文件開出 workflow A 和 workflow B…
     * > 這樣我可以完整測完不同契約對應不同案場且多個 flow 同時被發起。
     *
     * 所以這一段直接問四張單：每一張都要解出**兩條**，而且**各自來自自己那份契約**。
     */
    hr('⑤ 四份契約 × 兩條流程');
    const matrix: any[] = [];
    for (const b of rows) {
      const d = await billDetail(page, Number(b.id));
      matrix.push({
        docType: d?.docType,
        customerNumber: d?.customerNumberRaw,
        station: d?.station?.name ?? null,
        flows: (d?.binding?.flows ?? []).map((f: any) => `${f.workflowKey}@${f.contractNo ?? '案場層'}`),
      });
    }
    matrix.forEach((m) => log(`  ${m.docType} · ${m.station} · ${m.customerNumber} → ${m.flows.join(' ＋ ')}`));

    check('四張單各自配到案場（沒有例外件）',
      matrix.every((m) => !!m.station), matrix.map((m) => m.station));
    check('四張單**各解出兩條**流程', matrix.every((m) => m.flows.length === 2), matrix);
    check('每一條都來自**契約**（不是案場層保底）',
      matrix.every((m) => m.flows.every((f: string) => f.includes('@CT-'))), matrix);
    check('四張單分別來自**四份不同的**契約',
      new Set(matrix.map((m) => m.flows[0]?.split('@')[1])).size === 4,
      matrix.map((m) => m.flows[0]));
    check('每一份契約都是 A ＋ B（B 是開立發票）',
      matrix.every((m) => m.flows.some((f: string) => f.startsWith('invoice-issue@'))), matrix);
    await shot(page, '4-contract-matrix');

    /**
     * ── ⑥ **收不下的檔案與認不出的檔案都要有出口**（2026-09-09，使用者回報）───
     *
     * > 我上傳圖片，他直接回 unsupported type 然後卡死。但是卡片成立。
     * > 我上傳一個完全無關的 pdf，他可以讓我選類型，
     * > 但沒讓我有辦法逐個填寫無法識別的資料，例如電號等等。
     */
    hr('⑥ 不支援／認不出的檔案：不卡死，而且填得下去');
    const junkDir = path.join(REPO_ROOT, 'web/e2e/.tmp', 'crawler-junk');
    fs.mkdirSync(junkDir, { recursive: true });
    // HEIC：附件層收得下（iPhone 照片），**辨識服務不收** —— 這就是「卡死」那一份。
    const heicPath = path.join(junkDir, 'iphone-photo.heic');
    fs.writeFileSync(heicPath, Buffer.from('00000018667479706865696300000000', 'hex'));
    // 一份**完全無關**的 PDF：認得出是 PDF，但不是電費單。
    const junkPdfPath = path.join(junkDir, 'unrelated.pdf');
    fs.writeFileSync(junkPdfPath, placeholderPdf('unrelated.pdf'));

    check('回得到收件台', await goto(page, 'energyIngestConsole', '[data-testid="intake-dropzone"]'));
    // 等輸入框真的掛上去 —— 上一段停在單據頁，換頁那一瞬間它還不在。
    await waitUntil(page, () => !!document.querySelector('[data-testid="intake-file-input"]'),
      { timeout: 20000, label: '收件台的檔案輸入框' });
    const junkInput = await page.$('[data-testid="intake-file-input"]');
    check('收件台有檔案輸入框（第二次進來也要有）', !!junkInput);
    await (junkInput as any).uploadFile(heicPath, junkPdfPath);
    check('兩份都收得下（附件層不擋）', await waitUntil(page,
      () => /#\/energyIngestConsole\/\d+/.test(window.location.hash),
      { timeout: 120000, label: '跳到批次詳情頁' }));

    const settled = await waitUntil(page, () => {
      const st = document.querySelector('[data-testid="intake-batch-status"]')?.getAttribute('data-status') ?? '';
      return !!st && !['uploading', 'recognizing', 'classifying'].includes(st);
    }, { timeout: 5 * 60 * 1000, poll: 1000, label: '批次離開辨識中' });
    // **這一條就是「卡死」**：不支援的檔案被退回重送 → 永遠停在辨識中。
    check('批次不會卡在「辨識中」', settled);

    const junkRows = await page.evaluate(() => Array.from(
      document.querySelectorAll('[data-testid="intake-item"]'),
    ).map((r) => {
      const el = r as HTMLElement;
      return {
        file: el.querySelector('[data-testid="intake-item-file"]')?.textContent?.trim() ?? '',
        status: el.dataset.status ?? '',
        text: el.innerText.replace(/\s+/g, ' '),
        hasAssign: !!el.querySelector('[data-testid="intake-assign-type"]'),
      };
    }));
    junkRows.forEach((r) => log(`  ${r.file} · ${r.status} · 出口=${r.hasAssign}`));
    const heicRow = junkRows.find((r) => /heic/i.test(r.file));
    check('HEIC 那一份標成失敗（不是永遠在辨識）', heicRow?.status === 'failed', heicRow);
    check('而且說得出為什麼與下一步（PDF／JPG、或自己填）',
      /PDF/.test(heicRow?.text ?? ''), heicRow?.text?.slice(0, 160));
    check('失敗的那一份**有出口**（可以指定單別）', heicRow?.hasAssign === true, heicRow);
    const pdfRow = junkRows.find((r) => /unrelated/.test(r.file));
    check('無關的 PDF 判不出來，但也有出口', !!pdfRow?.hasAssign, pdfRow);
    await shot(page, '6-junk-uploaded');

    // 指定單別 → 要生出一張**空白但真實**的單，讓人逐格填。
    /**
     * **用真的滑鼠點** —— MUI 的 Select 是靠 mousedown 開的，
     * 程式化 `.click()` 打不開它（同 `pick()` 的說明）。
     * 那一列的下拉沒有唯一選擇器，所以先量出它的座標再點。
     */
    const assignBox = await page.evaluate(() => {
      const row = Array.from(document.querySelectorAll('[data-testid="intake-item"]'))
        .find((r) => /unrelated/.test((r as HTMLElement).innerText)) as HTMLElement | undefined;
      const combo = row?.querySelector('[role="combobox"]') as HTMLElement | null;
      if (!combo) return null;
      const b = combo.getBoundingClientRect();
      return { x: b.left + b.width / 2, y: b.top + b.height / 2 };
    });
    check('無關的 PDF 那一列有單別下拉', !!assignBox, assignBox);
    if (assignBox) await page.mouse.click(assignBox.x, assignBox.y);
    await waitUntil(page, () => document.querySelectorAll('[role="option"], li[role="option"]').length > 0,
      { timeout: 15000, label: '單別選單打開' });
    await page.evaluate(() => {
      const opt = Array.from(document.querySelectorAll('[role="option"], li[role="option"]'))
        .find((o) => /有餘電/.test((o as HTMLElement).innerText));
      (opt as HTMLElement | undefined)?.click();
    });
    const becameUnmatched = await waitUntil(page, () => Array.from(
      document.querySelectorAll('[data-testid="intake-item"]'),
    ).some((r) => /unrelated/.test((r as HTMLElement).innerText)
      && (r as HTMLElement).dataset.status === 'unmatched'),
    { timeout: 20000, label: '指定完單別 → 變成待指認' });
    check('指定單別之後**生出一張單**（狀態變成待指認，不是原地不動）', becameUnmatched);

    // 展開那一列 → 校對區要有可以填的欄位（辨識值誠實地空著）。
    const opened = await page.evaluate(() => {
      const row = Array.from(document.querySelectorAll('[data-testid="intake-item"]'))
        .find((r) => /unrelated/.test((r as HTMLElement).innerText)) as HTMLElement | undefined;
      const btn = row?.querySelector('[data-testid="intake-review"]') as HTMLElement | null;
      btn?.click();
      return !!btn;
    });
    // 沒有這顆按鈕，那一列就是**看得到卻填不了** —— 那正是使用者回報的死路。
    check('待指認的那一列有「就地校對」入口', opened);
    const canFill = await waitUntil(page, () => !!document.querySelector('[data-testid="ingest-field-customerNumberRaw"]'),
      { timeout: 20000, label: '出現可填的欄位' });
    check('逐格填得下去（電號那一格在）', canFill);
    const emptyOcr = await page.evaluate(() => {
      const row = document.querySelector('[data-testid="ingest-field-row"][data-field="customerNumberRaw"]') as HTMLElement | null;
      return row?.innerText ?? '';
    });
    check('辨識值那一欄誠實地寫「未辨識到」（不是憑空生一個值）',
      /未辨識到|—/.test(emptyOcr), emptyOcr.slice(0, 80));
    await shot(page, '7-manual-fill');

    /**
     * ── ⑦ **抽屜綁的是「這一筆待辦自己的流程」**（`TASKS.md #76`）────────
     *
     * 一張單同時開兩條之後，「我的工作」的抽屜以前綁的是 `bill.executions[0]`
     * ——那是**最新的那一筆**（id DESC），也就是開發票那條。
     * 於是點開「承辦確認」的人，看到的是開發票那條流程的關卡：
     * 畫面沒有壞，只是**動作全部不見了**。
     *
     * 所以這一段從**核可**那條的關卡進去（承辦確認落在同部門的另一個人身上，
     * 送件者不能自核），問一句：抽屜綁的是不是我點的那一筆。
     */
    hr('⑦ 多流程之後，抽屜要綁「這一筆待辦自己的流程」');
    await loginAs(page, demoEmail('manager'));
    check('主管也選得到 demo 組織', (await selectOrg(page)) != null);
    await goto(page, 'energyMyWorkConsole', '[data-testid="workbench-row"], .MuiAlert-root');
    await waitIdle(page);

    const queued = await page.evaluate((billId) => {
      const el = Array.from(document.querySelectorAll('[data-testid="workbench-row"]'))
        .find((r) => Number((r as HTMLElement).dataset.bill) === billId) as HTMLElement | undefined;
      if (!el) return null;
      el.click();
      return { node: el.dataset.node ?? '', execution: el.dataset.execution ?? '' };
    }, newBillId);
    log('  主管佇列裡的那一筆：', JSON.stringify(queued));
    check('主管的佇列裡有這張單（承辦確認那一關）',
      !!queued && queued.node === 'clerk-review', queued);

    const drawerReady = await waitUntil(page, () => !!document.querySelector(
      '[data-testid="bill-detail-execution"][data-execution-id]:not([data-execution-id=""])',
    ), { timeout: 20000, label: '抽屜綁上流程' });
    check('抽屜真的開起來了', drawerReady);
    const bound = await page.evaluate(() => {
      const el = document.querySelector('[data-testid="bill-detail-execution"]') as HTMLElement | null;
      return { id: el?.dataset.executionId ?? '', key: el?.dataset.workflowKey ?? '' };
    });
    log('  抽屜綁到的流程：', JSON.stringify(bound));
    // 這兩條就是那個 bug 的形狀：綁錯的時候 id 會是**另一筆**、key 會是 invoice-issue。
    check('抽屜綁的是我點的那一筆（不是這張單的第一筆）',
      !!queued && bound.id === queued.execution, { bound, queued });
    check('而且它是**核可**那條流程', bound.key === 'bill-approval', bound);
    await shot(page, '8-drawer-owns-execution');

    hr('伺服器');
    check('沒有 5xx', serverErrors.length === 0, [...new Set(serverErrors)]);
  } finally {
    await browser.close();
  }

  board.finish('單別可改 ＋ 一張單開多條流程');
}

run().catch((e) => { console.error(e); process.exitCode = 1; });
