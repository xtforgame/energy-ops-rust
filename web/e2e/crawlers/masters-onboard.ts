/* eslint-disable no-console, no-await-in-loop */
/*
卡 20261124-2316 的決定性驗收：案場、地主、合約各自獨立；案場可以沒有電號就建檔；
合約可以掛多位地主；陌生電號人工繫結之後算輸入完畢，下一期自動對到。

  不填電號建案場 → 三合一 onboard（案場＋地主×2＋合約） → 合約清單看兩位地主 →
  上傳陌生電號第一期（真檔＋真辨識 fake-tpr）→ 候選灰掉沒合約的案場 → 指認 →
  電號表多一列 → 上傳同電號第二期 → 自動對到 → 舊畫面回歸三張

跑法：`web/e2e/run-crawlers.sh masters-onboard`（E2E_DB 要 eo_e2e 開頭）。
CRAWLER_HEADFUL=1 看瀏覽器。

**為什麼用真檔＋真辨識**：這張卡要測的正是「陌生電號」這個身分怎麼從一張真的紙
（`fake_tpr.rs` 的 `cn-<假電號>` 檔名慣例覆寫 accountNumber）變成案場電號表的一列；
用 `seedBatch` 的注入式 OCR 繞過了辨識服務，就測不到 `cn_override` 那條縫。
*/
import type { Page } from 'puppeteer';
import {
  APP, sleep, log, hr, Scoreboard, launch, hardenPage, shooter,
  loginAsAdmin, selectOrg, goto, waitFor, waitUntil, waitIdle, eapi, writeUploadFiles,
} from '../lib/energyOpsHarness.ts';

const shot = shooter('masters-onboard');
const sb = new Scoreboard();
const check = sb.check.bind(sb);
const CN = '99-3217-2001';

/** 真滑鼠點開 MUI Select（mousedown 開；程式化 `.click()` 打不開），選單裡挑文字含 text 的選項。 */
async function pickSelect(page: Page, comboSelector: string, optionText: string) {
  const handle = await page.$(comboSelector);
  if (!handle) throw new Error(`找不到下拉：${comboSelector}`);
  await handle.click();
  await waitUntil(page, () => document.querySelectorAll('li[role="option"]').length > 0,
    { timeout: 10000, label: `${comboSelector} 選單打開` });
  const picked = await page.evaluate((t: string) => {
    const opt = Array.from(document.querySelectorAll('li[role="option"]'))
      .find((o) => (o as HTMLElement).innerText.includes(t));
    if (!opt) return false;
    (opt as HTMLElement).click();
    return true;
  }, optionText);
  if (!picked) throw new Error(`下拉裡找不到「${optionText}」`);
  // 選完要等選單真的關掉（backdrop 退場有轉場），不等的話下一個點擊會打在還沒退場的
  // backdrop 上，看起來像「點了沒反應」（下拉沒關就接著點別的按鈕，症狀跟按鈕壞掉一樣）。
  await waitUntil(page, () => !document.querySelector('ul[role="listbox"]'),
    { timeout: 5000, label: `${comboSelector} 選單關閉` });
}

async function typeInto(page: Page, selector: string, text: string) {
  const el = await page.$(selector);
  if (!el) throw new Error(`找不到輸入框：${selector}`);
  await el.click({ clickCount: 3 });
  await el.type(text, { delay: 5 });
}

/** `type="date"` 的 input 不能用按鍵打字（會打進瀏覽器自己的年/月/日子欄位，對不上）——
 * 照 smoke-masters.mjs 的寫法，直接用原生 setter 塞值再送 input 事件。 */
async function setDate(page: Page, selector: string, value: string) {
  await page.waitForSelector(selector, { timeout: 10000 });
  await page.evaluate((sel: string, val: string) => {
    const el = document.querySelector(sel) as HTMLInputElement | null;
    if (!el) return;
    const setter = Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, 'value')!.set!;
    setter.call(el, val);
    el.dispatchEvent(new Event('input', { bubbles: true }));
  }, selector, value);
}

/** 等批次離開「還在辨識」的狀態；每次查詢 `GET /ingest-batches/{id}` 都會順便 tick 一次服務。 */
async function waitBatchDone(page: Page, batchId: number, budgetMs = 60000): Promise<any> {
  const deadline = Date.now() + budgetMs;
  let last: any = null;
  while (Date.now() < deadline) {
    const r = await eapi(page, 'GET', `/ingest-batches/${batchId}`);
    if (r.status === 200) {
      last = r.data;
      const pending = (r.data.items ?? []).some((i: any) => ['uploading', 'recognizing', 'classifying', 'uploaded'].includes(i.status));
      if (!pending) return r.data;
    }
    await sleep(1000);
  }
  return last;
}

async function run() {
  const browser = await launch();
  const page = hardenPage(await browser.newPage());
  const serverErrors: string[] = [];
  page.on('response', (r) => { if (r.status() >= 500) serverErrors.push(`${r.status()} ${new URL(r.url()).pathname}`); });

  try {
    hr('登入並選 org');
    await loginAsAdmin(page);
    const orgId = await selectOrg(page);
    check('找得到有能源資料的 org', !!orgId, { orgId });

    // ── 1. 案場清單起手畫面 ──
    hr('① 案場清單（起手畫面）');
    await goto(page, 'energyStationConsole', '[data-testid="station-list-table"]');
    await shot(page, '01-stations-before');

    // 先用 API 建一座「有案場但沒建合約」的對照案場（S4 候選要用，不是這一步要畫面驗的東西）。
    const noContractId = await page.evaluate(async (oid: number) => {
      const r = await fetch(`/api/v1/orgs/${oid}/energy/stations`, {
        method: 'POST', credentials: 'include', headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ name: 'S4 沒合約案場（示範）' }),
      });
      const data = await r.json();
      return data.id;
    }, orgId);
    check('先備好一座沒合約的案場當候選對照', !!noContractId, noContractId);

    // ── 2. S1：不填電號、不選地主建案場 ──
    hr('② S1：不填電號、不選地主建案場');
    await page.click('[data-testid="station-new"]');
    await waitFor(page, '[data-testid="station-form-dialog"]');
    await typeInto(page, '[data-testid="station-form-name"] input', 'S1 無電號案場（示範）');
    await page.click('[data-testid="station-form-save"]');
    const s1Closed = await waitUntil(page, () => !document.querySelector('[data-testid="station-form-dialog"]'),
      { timeout: 10000, label: 'S1 存檔對話框關閉' });
    check('S1：不帶電號、不選地主也能存檔', s1Closed);
    await waitIdle(page);
    const s1Row = await waitUntil(page, () => Array.from(document.querySelectorAll('[data-testid="station-row"]'))
      .some((r) => (r as HTMLElement).innerText.includes('S1 無電號案場') && !!(r as HTMLElement).querySelector('[data-testid="station-no-meter-chip"]')),
      { timeout: 10000, label: 'S1 案場列出「沒有電號」標籤' });
    check('清單那一列有「沒有電號」標籤', s1Row);
    await shot(page, '02-s1-no-meter');

    // ── 3. S3：一次建案場、地主（一舊一新）、合約 ──
    hr('③ S3：一次建案場、地主、合約（/onboard）');
    const landownerList = await eapi(page, 'GET', '/landowners');
    const landownerItems: any[] = landownerList.data?.items ?? [];
    // 挑真人地主（partyType=individual），不要挑到台電區處／子公司之類的受款對象
    // （commander 審查這張卡時指出：挑錯會讓使用者誤會那是地主）。
    const firstLandowner = landownerItems.find((l) => l.partyType === 'individual') ?? landownerItems[0];
    check('拿得到至少一位既有地主（示範資料）', !!firstLandowner, landownerList.data);
    await page.click('[data-testid="station-onboard-open"]');
    await waitFor(page, '[data-testid="onboard-dialog"]');
    await typeInto(page, '[data-testid="onboard-station-name"] input', '路竹三爺二期（示範）');
    await pickSelect(page, '[data-testid="onboard-landowner-existing-select"] [role="combobox"]', firstLandowner?.name ?? '');
    await typeInto(page, '[data-testid="onboard-landowner-ratio"] input', '60');
    await page.click('[data-testid="onboard-landowner-add"]');
    await waitUntil(page, () => document.querySelectorAll('[data-testid="onboard-landowner-row"]').length >= 2,
      { timeout: 5000, label: '加地主那一列渲染出第二列' });
    const rows = await page.$$('[data-testid="onboard-landowner-row"]');
    check('加了第二位地主的輸入列', rows.length === 2, rows.length);
    const second = rows[1];
    await second.$eval('[data-testid="onboard-landowner-mode-new"]', (b) => (b as HTMLElement).click());
    await second.waitForSelector('[data-testid="onboard-landowner-new-name"] input', { timeout: 5000 });
    const newNameInput = await second.$('[data-testid="onboard-landowner-new-name"] input');
    if (newNameInput) { await newNameInput.click({ clickCount: 3 }); await newNameInput.type('路竹三爺二期新地主（示範）', { delay: 5 }); }
    const ratioInputs = await second.$$('[data-testid="onboard-landowner-ratio"] input');
    if (ratioInputs[0]) { await ratioInputs[0].click({ clickCount: 3 }); await ratioInputs[0].type('40', { delay: 5 }); }
    await typeInto(page, '[data-testid="onboard-contract-no"] input', 'CT-MIM-S3-DEMO');
    await setDate(page, '[data-testid="onboard-contract-validfrom"] input', '2026-01-01');
    await shot(page, '03-s3-onboard-dialog');

    await page.click('[data-testid="onboard-save"]');
    const onboardClosed = await waitUntil(page, () => !document.querySelector('[data-testid="onboard-dialog"]'),
      { timeout: 10000, label: '一次建好三樣：對話框關閉' });
    check('S3：一次存檔全成（對話框關閉，沒有卡在錯誤訊息）', onboardClosed);

    await waitIdle(page);
    const s3Search = await eapi(page, 'GET', '/stations?q=路竹三爺二期');
    const s3 = (s3Search.data?.items ?? []).find((s: any) => /路竹三爺二期/.test(s.name));
    check('找得到剛建好的案場', !!s3, s3Search.data);
    const s3Id: number = s3?.id ?? 0;

    await goto(page, `energyStationConsole/${s3Id}/show`, '[data-testid="station-name"]');
    await waitIdle(page);
    const s3Detail = await page.evaluate(() => ({
      meterRows: document.querySelectorAll('[data-testid="meter-row"]').length,
      allocationRows: document.querySelectorAll('[data-testid="allocation-row"]').length,
      contractRows: document.querySelectorAll('[data-testid="contract-timeline-row"]').length,
    }));
    check('案場詳情：沒有電號（電號表 0 列）', s3Detail.meterRows === 0, s3Detail);
    check('案場詳情：地主分配兩位', s3Detail.allocationRows === 2, s3Detail);
    check('案場詳情：合約一份', s3Detail.contractRows === 1, s3Detail);
    await shot(page, '04-s3-station-detail');

    // ── 4. S2：合約清單顯示兩位地主、依地主篩 ──
    hr('④ S2：合約清單顯示兩位地主、依地主篩');
    await goto(page, 'energyContractConsole', '[data-testid="contract-list-table"]');
    await waitIdle(page);
    await shot(page, '05-s2-contracts');
    const contractRowText = await page.evaluate(() => Array.from(document.querySelectorAll('[data-testid="contract-row"]'))
      .find((r) => (r as HTMLElement).innerText.includes('CT-MIM-S3-DEMO'))?.textContent ?? '');
    check('合約清單那一列顯示兩位地主（新舊地主的名字都在）',
      contractRowText.includes('路竹三爺二期新地主') && (firstLandowner ? contractRowText.includes(firstLandowner.name) : true),
      contractRowText);

    await typeInto(page, '[data-testid="contract-search"]', '路竹三爺二期新地主');
    await waitIdle(page);
    const filtered = await page.evaluate(() => Array.from(document.querySelectorAll('[data-testid="contract-row"]'))
      .map((r) => (r as HTMLElement).innerText));
    check('依地主篩找得到這份合約', filtered.some((t) => t.includes('CT-MIM-S3-DEMO')), filtered);
    await shot(page, '06-s2-filter');
    // 清掉篩選，不影響後面的步驟。
    await typeInto(page, '[data-testid="contract-search"]', '');
    await waitIdle(page);

    // ── 5. S4：上傳第一期陌生電號單 ──
    hr('⑤ S4：上傳第一期陌生電號單（真檔＋真辨識）');
    const [p1] = writeUploadFiles('masters-onboard', [`renewable-no-surplus-cn-${CN}.pdf`]);
    await goto(page, 'energyIngestConsole', '[data-testid="intake-dropzone"]');
    const input1 = await page.$('[data-testid="intake-file-input"]');
    check('找得到檔案輸入', !!input1);
    if (input1) await (input1 as any).uploadFile(p1);
    let batchId1 = 0;
    for (let i = 0; i < 60; i += 1) {
      const hash = await page.evaluate(() => window.location.hash).catch(() => '');
      const m = /#\/energyIngestConsole\/(\d+)/.exec(String(hash));
      if (m) { batchId1 = Number(m[1]); break; }
      await sleep(500);
    }
    check('上傳後進了批次詳情頁', batchId1 > 0, batchId1);
    const done1 = await waitBatchDone(page, batchId1);
    const item1 = (done1?.items ?? [])[0];
    check('第一期辨識完成、配不到案場（例外件）', item1?.status === 'unmatched', item1);
    const bill1Id = item1?.billId ?? 0;
    check('辨識完成生出一張電費單', bill1Id > 0, item1);

    await goto(page, 'energyBillConsole?reviewStatus=pendingMatch', '[data-testid="bills-tabs"]');
    await waitIdle(page);
    await shot(page, '07-s4-unmatched');

    await goto(page, `energyBillConsole/${bill1Id}/show`, '[data-testid="review-match-station"]');
    await page.click('[data-testid="review-match-station"]');
    await waitFor(page, '[data-testid="station-picker"]');
    // 清空預帶的原樣電號（對不上任何案場），讓候選清單回滿清單（沒合約的那座也要看得到）。
    const searchBox = await page.$('[data-testid="station-search"]');
    if (searchBox) { await searchBox.click({ clickCount: 3 }); await searchBox.type(' ', { delay: 5 }); await searchBox.click({ clickCount: 3 }); await page.keyboard.press('Backspace'); }
    await waitIdle(page);
    const picker1 = await page.$('[data-testid="station-picker"]');
    if (picker1) await picker1.click();
    await waitUntil(page, () => document.querySelectorAll('li[role="option"]').length > 0, { timeout: 10000, label: '候選清單打開' });
    // MUI 選單有展開動畫（Grow transition，預設約 225ms），不等跑完截圖會拍到半透明殘影——
    // 固定等待比動畫時間寬裕很多（smoke-masters.mjs 審查第 1 點：用 opacity 輪詢反而會卡死）。
    await sleep(1200);
    const candidates = await page.evaluate(() => Array.from(document.querySelectorAll('li[role="option"]')).map((o) => ({
      text: (o as HTMLElement).innerText, disabled: (o as HTMLElement).getAttribute('aria-disabled') === 'true',
    })));
    const noContractOpt = candidates.find((c) => c.text.includes('S4 沒合約案場'));
    check('候選裡沒合約的案場有列出來、灰掉不能選、標「有案場，但沒建合約」',
      !!noContractOpt && noContractOpt.disabled && noContractOpt.text.includes('有案場，但沒建合約'), candidates);
    const okOpt = candidates.find((c) => c.text.includes('路竹三爺二期') && !c.disabled);
    check('路竹三爺二期（有合約）可以選', !!okOpt, candidates);
    await shot(page, '08-s4-candidates');

    const picked1 = await page.evaluate(() => {
      const opt = Array.from(document.querySelectorAll('li[role="option"]'))
        .find((o) => (o as HTMLElement).innerText.includes('路竹三爺二期') && (o as HTMLElement).getAttribute('aria-disabled') !== 'true');
      if (!opt) return false;
      (opt as HTMLElement).click();
      return true;
    });
    check('選得到路竹三爺二期', picked1);
    await sleep(400);
    await page.click('[data-testid="station-match-confirm"]');
    const matched1 = await waitUntil(page, () => !document.querySelector('[data-testid="station-match-confirm"]'),
      { timeout: 10000, label: '繫結完成、對話框關閉' });
    check('繫結成功（對話框關閉，沒有被 409 擋下）', matched1);
    await waitIdle(page);
    await shot(page, '09-s4-matched');

    // ── 6. 回案場詳情：電號區多一列、「沒有電號」不見了 ──
    hr('⑥ 回案場詳情：電號表多一列');
    await goto(page, `energyStationConsole/${s3Id}/show`, '[data-testid="station-name"]');
    await waitIdle(page);
    const meterRowsAfter = await page.evaluate(() => document.querySelectorAll('[data-testid="meter-row"]').length);
    check('繫結成功寫進電號表（多一列）', meterRowsAfter === 1, meterRowsAfter);
    await goto(page, 'energyStationConsole', '[data-testid="station-list-table"]');
    await waitIdle(page);
    const s3NoMeterGone = await page.evaluate(() => !Array.from(document.querySelectorAll('[data-testid="station-row"]'))
      .some((r) => (r as HTMLElement).innerText.includes('路竹三爺二期') && !!(r as HTMLElement).querySelector('[data-testid="station-no-meter-chip"]')));
    check('案場清單那一列「沒有電號」標籤消失', s3NoMeterGone);
    await shot(page, '10-s4-meter-added');

    // ── 7. 上傳同電號第二期：自動對到，不進待指認 ──
    hr('⑦ 上傳同電號第二期：自動對到');
    const [p2] = writeUploadFiles('masters-onboard', [`high-voltage-device-cn-${CN}.pdf`]);
    await goto(page, 'energyIngestConsole', '[data-testid="intake-dropzone"]');
    const input2 = await page.$('[data-testid="intake-file-input"]');
    if (input2) await (input2 as any).uploadFile(p2);
    let batchId2 = 0;
    for (let i = 0; i < 60; i += 1) {
      const hash = await page.evaluate(() => window.location.hash).catch(() => '');
      const m = /#\/energyIngestConsole\/(\d+)/.exec(String(hash));
      if (m) { batchId2 = Number(m[1]); break; }
      await sleep(500);
    }
    check('第二期也上傳得進批次', batchId2 > 0, batchId2);
    const done2 = await waitBatchDone(page, batchId2);
    const item2 = (done2?.items ?? [])[0];
    check('第二期自動對到案場（不是例外件）', item2?.status !== 'unmatched', item2);
    const bill2Id = item2?.billId ?? 0;
    const bill2 = bill2Id ? (await eapi(page, 'GET', `/bills/${bill2Id}`)).data : null;
    check('第二期配到路竹三爺二期這座案場、待校對（沒進待指認）',
      bill2 && bill2.stationId === s3Id && bill2.reviewStatus === 'pendingReview', bill2);
    await shot(page, '11-s4-auto-matched');

    // ── 8. 舊畫面回歸：示範案場（雙電號）、既有合約、既有案場清單 ──
    hr('⑧ 舊畫面回歸（多對多改完，舊邏輯沒壞）');
    const demoStations = await eapi(page, 'GET', '/stations?q=示範案場');
    const dual = (demoStations.data?.items ?? []).find((s: any) => /雙電號/.test(s.name));
    check('找得到示範資料裡的「示範案場（雙電號）」', !!dual, demoStations.data);
    if (dual) {
      await goto(page, `energyStationConsole/${dual.id}/show`, '[data-testid="station-name"]');
      await waitIdle(page);
      const dualDetail = await page.evaluate(() => ({
        meterRows: document.querySelectorAll('[data-testid="meter-row"]').length,
        allocationRows: document.querySelectorAll('[data-testid="allocation-row"]').length,
      }));
      check('示範案場（雙電號）的電號表照舊兩列', dualDetail.meterRows === 2, dualDetail);
      check('示範案場（雙電號）的地主分配照舊兩位', dualDetail.allocationRows === 2, dualDetail);
      await shot(page, '12-regress-allocations');
    }
    await goto(page, 'energyContractConsole', '[data-testid="contract-list-table"]');
    await waitIdle(page);
    const oldContractRows = await page.evaluate(() => document.querySelectorAll('[data-testid="contract-row"]').length);
    check('合約清單照舊能列出既有（單一地主）的合約', oldContractRows > 0, oldContractRows);
    await shot(page, '13-regress-contracts');
    await goto(page, 'energyStationConsole', '[data-testid="station-list-table"]');
    await waitIdle(page);
    const oldStationHasMeter = await page.evaluate(() => Array.from(document.querySelectorAll('[data-testid="station-row"]'))
      .some((r) => !(r as HTMLElement).querySelector('[data-testid="station-no-meter-chip"]')));
    check('既有案場清單照舊顯示電號（不是全部變成「沒有電號」）', oldStationHasMeter);
    await shot(page, '14-regress-stations');

    hr('⑨ 伺服器錯誤');
    check('沒有 5xx', serverErrors.length === 0, [...new Set(serverErrors)]);
  } finally {
    await browser.close();
  }

  sb.finish('案場／地主／合約獨立、沒有電號建檔、陌生電號繫結');
}

run().catch((e) => { console.error(e); process.exitCode = 1; });
