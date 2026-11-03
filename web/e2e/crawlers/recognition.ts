/* eslint-disable no-console, no-await-in-loop */
/*
**R7 的決定性驗收**：上傳**四份真的電費單 PDF**，用真的辨識服務跑，
然後在畫面上親眼確認「原檔預覽 ＋ 真欄位」。

  上傳（真 PDF）→ 辨識（taipower-recognition，三來源 L2）→ 收件台就地校對

移植自 az `scripts/crawler-energy-ops-recognition`。跑法：`web/e2e/run-crawlers.sh recognition`
（示範資料＋範本電費單；辨識服務是 `energy-ops fake-tpr`，依檔名回四份範本的真實辨識輸出，
主檔要有 `00-60-1600-79-5` 躉購、`11-72-0328-95-9` / `07-42-8560-90-1` 高壓那幾座案場）。
  CRAWLER_HEADFUL=1 ... 看瀏覽器

**為什麼這一支不能用假檔**（其餘 crawler 上傳的是 1×1 PNG）：
   那些驗收要的是流程語意，用檔名挑 fixture 就夠了。
   這一支要驗的是**辨識本身** —— 版型判讀、欄位抽取、座標、
   `resolution` 與 `issues`。假檔一律走不到那條路，
   而「頁面畫得出來」跟「畫的是這張紙上的數字」是兩回事。

這支會真的走辨識服務的協定（上傳 → 建 job → 輪詢 → 取結果）；
   逾時設得比其他 crawler 長是刻意的（真服務一份要十幾秒）。
*/
import fs from 'node:fs';
import path from 'node:path';
import type { Page } from 'puppeteer';
import {
  APP, sleep, log, hr, Scoreboard, launch, hardenPage, shooter,
  loginAsAdmin, selectOrg, goto, waitFor,
  waitIdle, eapi, SAMPLE_DIR,
} from '../lib/energyOpsHarness.ts';

const shot = shooter('energy-recognition');
const sb = new Scoreboard();


/**
 * 四份範本各自的**真值**（截自 `RECOGNITION.md §1.1` 的實跑，可拿原單對）。
 *
 * `docType` 是我們這一側的分類結果：躉購（台電付我們）走 `bill-approval`，
 * 高壓用電（我們付台電）走 `utility-bill` —— **錢的方向相反**（`§9-1`）。
 */
interface Sample {
  file: string;
  templateId: string;
  docType: string;
  /** 校對台並排欄位的辨識值（`ocrExtracted`）。 */
  expect: Record<string, string>;
  /** 這張單**該有**與**不該有**的欄位列（依版型）。 */
  hasField: string;
  lacksField: string;
  /** 這份 PDF 幾頁（高壓單是兩頁 —— 框帶頁碼，viewer 要跳得過去）。 */
  pages: number;
  /** 在校對台點這個欄位 → 原檔上那一格要亮起來。 */
  highlightField: string;
}

const SAMPLES: Sample[] = [
  {
    file: '電費單(有餘電需開發票).pdf',
    templateId: 'renewable_purchase',
    // 版型只有兩種，但單別有**四種** —— 判別在「有沒有賣出去的電」。
    docType: 'renewableSurplus',
    expect: {
      billingPeriodStart: '2026-06-09',
      billingPeriodEnd: '2026-08-10',
      totalAmount: '127848',
      generationKwh: '21072',
    },
    hasField: 'issuedAt',
    lacksField: 'dueDate',
    pages: 1,
    highlightField: 'totalAmount',
  },
  {
    file: '逆變器電費單(僅設備發電使用電費).pdf',
    templateId: 'high_voltage',
    // 判別在「有沒有電器租費那一列」。
    docType: 'consumptionDevice',
    expect: {
      billingPeriodStart: '2026-07-01',
      billingPeriodEnd: '2026-07-31',
      totalAmount: '5035',
      generationKwh: '800',
    },
    hasField: 'dueDate',
    lacksField: 'issuedAt',
    pages: 2,
    highlightField: 'dueDate',
  },
];

/** 另外兩份一起上傳（要驗的是「四份都跑得完」，欄位真值由上面兩份負責）。 */
const EXTRA_FILES = [
  '電費單(無餘電需付款).pdf',
  '逆變器電費單(含電表租費&設備發電使用電費).pdf',
];

/**
 * 上傳四份真 PDF 並停在批次詳情頁。
 *
 * 不共用 `harness.uploadNewBatch`：那一支等的是 `intake-item` 出現，
 * 而**新流程裡 item 是檔案上傳完就出現的**（`status='uploaded'`），
 * 那時候辨識根本還沒開始。這裡要等的是**辨識跑完**。
 */
async function uploadSamples(page: Page): Promise<number> {
  const files = [...SAMPLES.map((s) => s.file), ...EXTRA_FILES]
    .map((f) => path.join(SAMPLE_DIR, f));
  files.forEach((f) => {
    if (!fs.existsSync(f)) throw new Error(`找不到範本：${f}`);
  });

  await goto(page, 'energyIngestConsole', '[data-testid="intake-dropzone"]');
  const input = await page.$('[data-testid="intake-file-input"]');
  if (!input) throw new Error('收件台沒有檔案輸入框');
  await (input as any).uploadFile(...files);

  for (let i = 0; i < 120; i += 1) {
    const hash = await page.evaluate(() => window.location.hash).catch(() => '');
    const m = /#\/energyIngestConsole\/(\d+)/.exec(String(hash));
    if (m) return Number(m[1]);
    await sleep(500);
  }
  throw new Error('上傳後沒有跳到批次詳情頁');
}

/** 等到整批不再是「辨識中」。三來源全跑很慢，所以這裡的預算是分鐘級。 */
async function waitRecognized(page: Page, budgetMs = 15 * 60 * 1000): Promise<string> {
  const deadline = Date.now() + budgetMs;
  let last = '';
  while (Date.now() < deadline) {
    const status = await page.evaluate(() => document
      .querySelector('[data-testid="intake-batch-status"]')?.getAttribute('data-status') ?? '')
      .catch(() => '');
    if (status && status !== last) { log(`  批次狀態：${status}`); last = String(status); }
    if (status && !['uploading', 'recognizing', 'classifying'].includes(String(status))) {
      return String(status);
    }
    await waitIdle(page);
  }
  return last || '(讀不到)';
}

/** 讀出每一列的檔名 / 狀態 / 單別。 */
async function readItems(page: Page) {
  return page.evaluate(() => Array.from(
    document.querySelectorAll('[data-testid="intake-item"]'),
  ).map((row) => ({
    id: Number(row.getAttribute('data-item-id') || 0),
    filename: row.querySelector('[data-testid="intake-item-file"]')?.textContent?.trim() ?? '',
    status: row.querySelector('[data-testid="intake-item-status"]')?.textContent?.trim() ?? '',
    statusKey: row.getAttribute('data-status') ?? '',
    docType: row.getAttribute('data-doc-type') ?? '',
  })));
}

async function run() {
  const browser = await launch();
  const page = hardenPage(await browser.newPage());
  page.on('console', (m) => {
    if (m.type() === 'error') log('  [browser error]', m.text().slice(0, 200));
  });

  try {
    hr('登入並選 org');
    await loginAsAdmin(page);
    const orgId = await selectOrg(page);
    sb.check('找得到有能源資料的 org', !!orgId, { orgId });

    hr('上傳四份真的電費單 PDF');
    const batchId = await uploadSamples(page);
    log(`  批次 #${batchId}`);
    await shot(page, '01-uploaded');

    const status = await waitRecognized(page);
    sb.check('辨識跑完（批次離開 recognizing）', !['recognizing', 'uploading'].includes(status), { status });
    await shot(page, '02-recognized');

    const items = await readItems(page);
    sb.check('四份都有 item', items.length === 4, items.map((i) => i.filename));
    sb.check(
      '沒有任何一份辨識失敗',
      items.every((i) => i.statusKey !== 'failed'),
      items.map((i) => `${i.filename}=${i.statusKey}`),
    );
    /**
     * **四份範本 ＝ 四種單別**（2026-09-09）。
     *
     * 以前這裡斷言的是「兩種單別各兩份」—— 那是照**版型**分的。
     * 但四份範本的後續作業各不相同（有餘電要開發票、其餘三種是我們付台電），
     * 而「要開哪些流程」是照後續作業決定的。兩種單別表達不出那件事。
     */
    sb.check(
      '四份範本判成**四種不同的**單別（不是兩種）',
      new Set(items.map((i) => i.docType)).size === 4,
      items.map((i) => `${i.filename}=${i.docType}`),
    );

    for (const sample of SAMPLES) {
      hr(`就地校對：${sample.file}`);
      const item = items.find((i) => i.filename === sample.file);
      if (!item) { sb.check(`找得到 ${sample.file} 那一列`, false, items.map((i) => i.filename)); continue; }
      sb.check(`${sample.file} 判成 ${sample.docType}`, item.docType === sample.docType, item.docType);

      await page.evaluate((id: number) => {
        const row = document.querySelector(`[data-testid="intake-item"][data-item-id="${id}"]`);
        (row?.querySelector('[data-testid="intake-review"]') as HTMLElement | null)?.click();
      }, item.id);

      /**
       * 不可以只等 `[data-testid="ingest-field-row"]` 出現。
       *
       * 一次只有一列展開，但**上一列還在畫面上的那一瞬間**選擇器就已經命中了；
       * 於是第二份的每一條斷言都會拿到第一份的數字 —— 而那看起來像
       * 「版型判錯、金額抽錯」，一路指向不存在的 bug。
       * 這裡等的是「**這一列的**工作區帶著它自己的單」。
       */
      const opened = await (async () => {
        for (let i = 0; i < 60; i += 1) {
          // eslint-disable-next-line no-await-in-loop
          const ready = await page.evaluate((id: number) => {
            const ws = document.querySelector(`[data-testid="ingest-workspace"][data-item="${id}"]`);
            return !!ws && !!ws.querySelector('[data-testid="ingest-field-row"]');
          }, item.id).catch(() => false);
          if (ready) return true;
          // eslint-disable-next-line no-await-in-loop
          await sleep(500);
        }
        return false;
      })();
      sb.check('校對區展開得開（而且是這一列的）', opened);
      await waitIdle(page);

      // ── ① 左欄：**真的那份 PDF**（不是 A4 空框）────────────────
      const preview = await page.evaluate((id: number) => {
        const ws = document.querySelector(`[data-testid="ingest-workspace"][data-item="${id}"]`);
        // PDF 是我們自己用 pdf.js 畫進 canvas 的（不是 iframe）——
        // 只有這樣框才對得準（見 `SourcePreviewFrame`）。
        const canvas = ws?.querySelector('[data-testid="source-preview-pdf"]') as HTMLCanvasElement | null;
        const frame = ws?.querySelector('[data-testid="source-preview"]') as HTMLElement | null;
        return {
          hasCanvas: !!canvas,
          // 真的畫出東西了：canvas 有像素尺寸（沒 render 的話是 0）。
          canvasW: canvas?.width ?? 0,
          canvasH: canvas?.height ?? 0,
          hasImage: !!ws?.querySelector('[data-testid="source-preview-image"]'),
          loaded: frame?.getAttribute('data-loaded') ?? '',
          pageCount: Number(frame?.getAttribute('data-page-count') ?? '0'),
        };
      }, item.id);
      sb.check('左欄是我們自己畫的 PDF（canvas，不是 iframe）', preview.hasCanvas, preview);
      sb.check('那一頁真的被畫出來了（canvas 有尺寸）',
        preview.canvasW > 100 && preview.canvasH > 100, preview);
      sb.check('原檔已載入（data-loaded=1）', preview.loaded === '1', preview);
      sb.check(`頁數讀得出來（${sample.pages} 頁）`, preview.pageCount === sample.pages, preview);

      // ── ② 辨識結果本身：版型 / 取值方式 / resolution ───────────
      const note = await page.evaluate((id: number) => {
        const ws = document.querySelector(`[data-testid="ingest-workspace"][data-item="${id}"]`);
        return {
          template: ws?.querySelector('[data-testid="recognition-template"]')?.getAttribute('data-template') ?? '',
          method: ws?.querySelector('[data-testid="recognition-method"]')?.getAttribute('data-method') ?? '',
          resolutions: Array.from(ws?.querySelectorAll('[data-testid="recognition-resolution"]') ?? [])
            .map((e) => `${e.getAttribute('data-resolution')}=${e.getAttribute('data-count')}`),
          issues: Array.from(ws?.querySelectorAll('[data-testid="recognition-issue"]') ?? [])
            .map((e) => e.getAttribute('data-code')),
        };
      }, item.id);
      sb.check(`版型判成 ${sample.templateId}`, note.template === sample.templateId, note);
      // 文字層拆得到就一定要贏過 OCR —— 這是 §9-4 的整個重點。
      sb.check('取值方式＝文字層（pdf_text）', note.method === 'pdf_text', note);
      sb.check('resolution 有上畫面（L2 跑了才看得到）', note.resolutions.length > 0, note.resolutions);

      // ── ③ 欄位：依版型的那一組，值是這張紙上的數字 ──────────────
      const fields = await page.evaluate((id: number) => {
        const ws = document.querySelector(`[data-testid="ingest-workspace"][data-item="${id}"]`);
        return Array.from(ws?.querySelectorAll('[data-testid="ingest-field-row"]') ?? []).map((row) => ({
          field: row.getAttribute('data-field') ?? '',
          final: (row.querySelector('input') as HTMLInputElement | null)?.value ?? '',
          label: row.querySelector('td')?.textContent?.trim() ?? '',
        }));
      }, item.id);
      const keys = fields.map((f) => f.field);
      sb.check(`欄位組含 ${sample.hasField}`, keys.includes(sample.hasField), keys);
      sb.check(`欄位組不含 ${sample.lacksField}（那是另一種單的欄位）`, !keys.includes(sample.lacksField), keys);

      Object.entries(sample.expect).forEach(([field, want]) => {
        const got = fields.find((f) => f.field === field)?.final ?? '(沒有這一列)';
        // 金額欄位 DB 回來帶尾零（`127848.00`）—— 比數值不比字串。
        const same = Number.isFinite(Number(got)) && Number.isFinite(Number(want))
          ? Number(got) === Number(want)
          : got === want;
        sb.check(`${field} ＝ ${want}`, same, { got });
      });

      if (sample.templateId === 'high_voltage') {
        const label = fields.find((f) => f.field === 'generationKwh')?.label ?? '';
        // 高壓單上這一格是**買進來的**度數 —— 標成「發電度數」會讓人以為發了 800 度。
        sb.check('高壓單的度數標題是「用電度數」', label.includes('用電'), label);
      }

      await shot(page, `03-review-${sample.templateId}`);

      // ── ④ 校對台：**點欄位 → 原檔上那一格亮起來**（R7 的重點互動）──
      //
      // 這一段在**單據詳情頁**驗，不在收件頁：高亮綁在校對台的欄位列上
      // （收件頁刻意不做，見 `IngestBatchDetailPage` 的註解）。
      const billId: number = await (async () => {
        const r = await eapi(page, 'GET', `/ingest-batches/${batchId}`);
        if (r.status !== 200) return 0;
        const it = (r.data?.items ?? []).find((x: any) => Number(x.id) === item.id);
        return it?.billId ? Number(it.billId) : 0;
      })();

      const highlight = await (async () => {
        const targetBill = billId;
        if (!targetBill) return { note: 'no bill id' };
        await goto(page, `energyBillConsole/${targetBill}/show`, '[data-testid="review-field"]');
        await waitIdle(page);
        return page.evaluate(async (preferred: string) => {
          const fieldsWithBox = Array.from(document.querySelectorAll('[data-testid="source-box"]'))
            .map((b) => (b as HTMLElement).dataset.field ?? '');
          /**
           * **有框的欄位才點得出高亮。**
           * 沒有框是正常結果（值在頁面上出現不只一次 → 服務不給座標，
           * `RECOGNITION.md §9-5`），所以這裡挑一個**真的有框**的欄位來驗互動，
           * 而不是寫死某個欄位然後在它剛好沒框時判產品有問題。
           */
          const field = fieldsWithBox.includes(preferred) ? preferred : (fieldsWithBox[0] ?? '');
          if (!field) return { note: 'no box on this page', fieldsWithBox };

          const boxOf = () => document
            .querySelector(`[data-testid="source-box"][data-field="${field}"]`) as HTMLElement | null;
          const before = boxOf()?.dataset.active ?? '(no box)';
          const row = Array.from(document.querySelectorAll('[data-testid="review-field"]'))
            .find((r) => (r as HTMLElement).dataset.field === field);
          (row as HTMLElement | null)?.click();
          await new Promise((res) => { setTimeout(res, 1200); });

          const box = boxOf();
          const frame = document.querySelector('[data-testid="source-preview"]') as HTMLElement | null;
          const canvas = document.querySelector('[data-testid="source-preview-pdf"]') as HTMLElement | null;
          /**
           * 不要讀 `box.style.left` —— MUI 的 `sx` 編成 class，inline style 是空的。
           * 量**實際的幾何**：框相對於 canvas 的位置與大小。
           * 這同時比對得更硬 —— 它證明框真的疊在那張紙上面。
           */
          let rel: any = null;
          if (box && canvas) {
            const b = box.getBoundingClientRect();
            const c = canvas.getBoundingClientRect();
            rel = {
              left: c.width ? (b.left - c.left) / c.width : -1,
              top: c.height ? (b.top - c.top) / c.height : -1,
              width: c.width ? b.width / c.width : -1,
              height: c.height ? b.height / c.height : -1,
            };
          }
          const labelEl = document
            .querySelector(`[data-testid="source-box-label"][data-field="${field}"]`) as HTMLElement | null;
          return {
            field,
            before,
            after: box?.dataset.active ?? '(no box)',
            boxLabel: labelEl?.innerText?.trim() ?? '',
            rel,
            page: frame?.getAttribute('data-page') ?? '',
            boxPage: box?.dataset.page ?? '',
            boxes: document.querySelectorAll('[data-testid="source-box"]').length,
            fieldsWithBox,
          };
        }, sample.highlightField);
      })();
      log(`  高亮：${JSON.stringify(highlight)}`);
      const hl: any = highlight;
      sb.check(
        `點欄位「${hl.field ?? '?'}」→ 原檔上那一格亮起來`,
        hl.before === '0' && hl.after === '1',
        highlight,
      );
      /**
       * 框要**貼著那串字**：位置落在紙面內，寬度是一小段而不是整頁。
       * 這一條擋的是「框畫出來了但疊錯地方」—— 而那比沒有框更糟。
       */
      sb.check(
        '框真的疊在那張紙上（位置在頁內、寬度是一小段）',
        !!hl.rel
          && hl.rel.left > 0 && hl.rel.left < 1
          && hl.rel.top > 0 && hl.rel.top < 1
          && hl.rel.width > 0 && hl.rel.width < 0.6,
        highlight,
      );
      if (sample.pages > 1) {
        // 兩頁式的單：框在第 2 頁時 viewer 要**自己翻過去**，
        // 否則點了什麼都不會發生（框其實畫好了，只是不在眼前）。
        sb.check(
          '框在第幾頁，畫面就翻到第幾頁',
          String((highlight as any).page) === String((highlight as any).boxPage),
          highlight,
        );
      }
      // 框上要標**這一格是哪個欄位** —— 只有一個框而沒有名字，
      // 使用者得自己猜「亮的是哪一欄」。
      sb.check('亮起來的框旁邊標了欄位名', String(hl.boxLabel ?? '').length > 0, highlight);

      // 左欄只有 260px —— 讀不出紙上的數字。點一下要開得出放大視窗。
      const zoom = await page.evaluate(async () => {
        (document.querySelector('[data-testid="source-preview"]') as HTMLElement | null)?.click();
        await new Promise((res) => { setTimeout(res, 2000); });
        const dlg = document.querySelector('[data-testid="source-preview-zoom"]');
        const canvas = dlg?.querySelector('[data-testid="source-preview-pdf"]') as HTMLCanvasElement | null;
        const w = canvas ? canvas.getBoundingClientRect().width : 0;
        return { opened: !!dlg, width: Math.round(w) };
      });
      log(`  放大：${JSON.stringify(zoom)}`);
      sb.check('點原檔開得出放大視窗，而且真的比較大（>600px）',
        zoom.opened && zoom.width > 600, zoom);
      // 截圖要在**還開著的時候**照 —— 關掉之後照到的是一個空對話框，
      // 而那張圖會被當成「放大功能壞了」。
      await shot(page, `05-zoom-${sample.templateId}`);
      await page.evaluate(() => {
        (document.querySelector('[data-testid="source-preview-zoom-close"]') as HTMLElement | null)?.click();
      });
      await sleep(800);

      await shot(page, `04-highlight-${sample.templateId}`);

      // 回到批次頁，準備下一份。
      await goto(page, `energyIngestConsole/${batchId}`, '[data-testid="intake-item"]');
      await waitIdle(page);
    }
  } catch (e: any) {
    sb.check('腳本跑完沒有 throw', false, e?.message ?? String(e));
    try { await shot(page, '99-crash'); } catch { /* 截圖失敗不重要 */ }
  } finally {
    sb.finish('R7 真檔辨識驗收');
    await browser.close();
  }
}

/**
 * **一定要自己呼叫 `run()`。**
 * `sr rs` 只是 `node <檔案>` —— 它不會去找 default export。
 * 只寫 `export default run` 的話整支腳本**一行都不會跑**，
 * 而畫面上是「Done in 2.9s」，跟跑完而且全綠長得一模一樣。
 */
run().catch((e) => { console.error(e); process.exitCode = 1; });
