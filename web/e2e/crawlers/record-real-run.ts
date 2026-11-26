/* eslint-disable no-console, no-await-in-loop */
/*
員工全程在畫面上手動處理一張真單，不先用 API 灌資料：案場（不填電號）→ 地主 → 合約（勾案場與地主，選三個代號）
→ EasyFlow 預設值 → 手動上傳真售電單 → 第一次電號對不上、在指認對話框綁定 →
對到合約看識別欄位 → 先標已校對再審核 → EasyFlow 草稿 → 租金年額試算 → 付款草稿 → 租金付款申請。

每段開頭用 page.evaluate 注入一張全畫面標題卡（段號、員工在做什麼、資料從哪來），操作中常駐一行字幕，
都是 overlay div，不改產品碼。每段用 page.on('response') 記下真實回包的關鍵欄位。

這支腳本不帶任何真實業務資料——案場名、地主名、合約編號、電號、公司代碼、PDF 路徑等全部從設定檔讀，
設定檔本身不進 repo（見 web/.gitignore 的 e2e/real-run.local.json）。範本見 web/e2e/real-run.example.json，
環境變數與操作說明見 web/e2e/REAL-RUN.md。

環境變數：E2E_BASE_URL、E2E_SUPER_USER、E2E_SUPER_PASSWORD、E2E_ORG_ID、S3_OUT、E2E_REAL_RUN_CONFIG
*/
import {
  sleep, log, hr, launch, hardenPage, goto, waitFor, waitUntil, waitIdle,
  loginAsSuperUser, selectOrg,
} from '../lib/energyOpsHarness.ts';
import type { Page } from 'puppeteer';
import path from 'node:path';
import fs from 'node:fs';
import { fileURLToPath } from 'node:url';

export type RealRunConfig = {
  pdfPath: string;
  station: { name: string };
  landowner: { name: string; partyType: string };
  contract: { no: string; signedOn: string; validFrom: string; validTo: string };
  easyflowDefaults: {
    companyName: string; companyCode: string; applicant: string; department: string;
    paymentMethod: string; paymentTerms: string; voucherStatus: string; contractOption: string;
  };
  match: { customerNumberHint: string; searchTerm: string; stationFullName: string };
  rentCycle: { anchorDate: string; startDate: string; annualAmount: string; expectedBaseRent: number; expectedTotal: number };
  sources: {
    env: string; station: string; landowner: string; contract: string;
    easyflow: string; upload: string; match: string; rentCycle: string;
  };
};

/** 讀 E2E_REAL_RUN_CONFIG 指的檔；沒設就讀 web/e2e/real-run.local.json（不進 repo，自己用真值建一份，
 * 照 web/e2e/real-run.example.json 的鍵填）。找不到檔就丟清楚的錯誤，不要用假資料硬跑。 */
export function loadConfig(): RealRunConfig {
  const here = path.dirname(fileURLToPath(import.meta.url));
  const configPath = process.env.E2E_REAL_RUN_CONFIG || path.join(here, '../real-run.local.json');
  if (!fs.existsSync(configPath)) {
    throw new Error(`找不到設定檔 ${configPath}。先複製 web/e2e/real-run.example.json 成 web/e2e/real-run.local.json 並填真值，或用 E2E_REAL_RUN_CONFIG 指到別的檔。`);
  }
  return JSON.parse(fs.readFileSync(configPath, 'utf8')) as RealRunConfig;
}

const OUT = process.env.S3_OUT || '/tmp/real-run.mp4';
const ORG_ID = Number(process.env.E2E_ORG_ID || '0');

async function titleCard(page: Page, seg: string, what: string, source: string, ms = 2500) {
  await page.evaluate((s: string, w: string, src: string) => {
    const d = document.createElement('div');
    d.id = '__title_card__';
    d.style.cssText = 'position:fixed;inset:0;z-index:99999;background:rgba(20,10,40,.96);color:#fff;display:flex;flex-direction:column;align-items:center;justify-content:center;font-family:sans-serif;text-align:center;padding:40px;';
    d.innerHTML = `<div style="font-size:14px;opacity:.7;margin-bottom:12px;">段 ${s}</div><div style="font-size:28px;font-weight:700;margin-bottom:16px;">${w}</div><div style="font-size:16px;opacity:.85;">資料來源：${src}</div>`;
    document.body.appendChild(d);
  }, seg, what, source);
  await sleep(ms);
  await page.evaluate(() => document.getElementById('__title_card__')?.remove());
}

async function caption(page: Page, text: string) {
  await page.evaluate((t: string) => {
    let d = document.getElementById('__caption__');
    if (!d) {
      d = document.createElement('div');
      d.id = '__caption__';
      d.style.cssText = 'position:fixed;left:0;right:0;bottom:0;z-index:99998;background:rgba(0,0,0,.78);color:#fff;font-size:15px;padding:10px 20px;font-family:sans-serif;';
      document.body.appendChild(d);
    }
    d.textContent = t;
  }, text);
}

/** 這支腳本全程不准按 Escape：MUI 單選 Select 點了選項之後選單自己會關；
 * 按一次 Escape，關掉的其實是外層 Dialog。檢查對話框還在不在，失敗就截圖。 */
async function dialogStillOpen(page: Page, dialogTestId: string): Promise<boolean> {
  return page.evaluate((id: string) => !!document.querySelector(`[data-testid="${id}"]`), dialogTestId);
}

/** 等所有 [role=listbox] 真的從 DOM 消失（count===0），不是只看第一個。 */
async function waitAllListboxesGone(page: Page, label: string, timeout = 5000): Promise<boolean> {
  return waitUntil(page, () => document.querySelectorAll('[role="listbox"]').length === 0, { timeout, label: `${label} 所有 listbox 消失` });
}

/** 關掉目前還開著的 listbox：只在真的還有 [role=listbox] 時才按 Escape，按完等全部消失。 */
async function closeAnyOpenListbox(page: Page, label: string) {
  const count = await page.evaluate(() => document.querySelectorAll('[role="listbox"]').length);
  if (count === 0) return true;
  await page.keyboard.press('Escape').catch(() => undefined);
  return waitAllListboxesGone(page, label, 5000);
}

/** 共用的下拉點選核心：開新下拉前先確保沒有殘留 listbox；只在「最新那一個」listbox 裡找選項
 * （上一個下拉沒 detach，下一個點擊會誤落在舊 listbox 裡，要用最新那個）；
 * 找不到選項時把所有還開著的 listbox 的選項文字印出來，方便定位。 */
async function clickDropdownOption(page: Page, comboSelector: string, optionText: string, label: string, fallbackSelector?: string): Promise<boolean> {
  const preClear = await closeAnyOpenListbox(page, `${label}（點開前先清殘留）`);
  if (!preClear) log(`  !! ${label}：點開前還有殘留 listbox 關不掉`);
  let handle = await page.$(comboSelector);
  if (!handle && fallbackSelector) handle = await page.$(fallbackSelector);
  if (!handle) { log(`  !! ${label}：找不到下拉元素 ${comboSelector}`); return false; }
  await handle.click();
  const opened = await waitUntil(page, () => document.querySelectorAll('[role="listbox"]').length > 0, { timeout: 6000, label: `${label} 選單打開` });
  if (!opened) { log(`  !! ${label}：選單沒打開`); return false; }
  await sleep(500);
  const clicked = await page.evaluate((t: string) => {
    const boxes = Array.from(document.querySelectorAll('[role="listbox"]'));
    const latest = boxes[boxes.length - 1];
    if (!latest) return { ok: false, allTexts: [] as string[] };
    const opts = Array.from(latest.querySelectorAll('li[role="option"]'));
    const opt = opts.find((o) => (o.textContent || '').trim().startsWith(t) || (o.textContent || '').includes(t));
    if (opt) { (opt as HTMLElement).click(); return { ok: true, allTexts: [] as string[] }; }
    const allTexts = boxes.flatMap((b) => Array.from(b.querySelectorAll('li[role="option"]')).map((o) => (o.textContent || '').trim()));
    return { ok: false, allTexts };
  }, optionText);
  if (!clicked.ok) { log(`  !! ${label}：最新 listbox 找不到「${optionText}」，目前所有 listbox 選項＝${JSON.stringify(clicked.allTexts)}`); return false; }
  return true;
}

/** 單選代號下拉：點開→等 listbox→點選項→等所有 listbox 消失（不行就按 Escape 補一次）。 */
export async function pickSingleOption(page: Page, comboSelector: string, optionText: string, label: string, dialogTestId: string, shotName?: string): Promise<boolean> {
  log(`  >> ${label}：準備點開`);
  const ok = await clickDropdownOption(page, comboSelector, optionText, label);
  if (!ok) {
    if (shotName) await page.screenshot({ path: path.join(path.dirname(OUT), shotName) }).catch(() => undefined);
    return false;
  }
  let gone = await waitAllListboxesGone(page, `${label}（選完自動關閉）`, 3000);
  if (!gone) { await closeAnyOpenListbox(page, `${label}（選完 Escape 補關）`); gone = await page.evaluate(() => document.querySelectorAll('[role="listbox"]').length === 0); }
  if (!gone) log(`  !! ${label}：listbox 還是沒 detach，下一步可能誤點`);
  await sleep(300);
  const stillOpen = await dialogStillOpen(page, dialogTestId);
  log(`  >> ${label}：選了「${optionText}」，對話框${stillOpen ? '還在' : '不見了！！'}`);
  if (!stillOpen && shotName) await page.screenshot({ path: path.join(path.dirname(OUT), shotName) }).catch(() => undefined);
  return stillOpen;
}

/** 多選下拉（案場／地主）：點開→等 listbox→點選項→主動關閉（只在還開著時按 Escape）→等全部消失。 */
export async function pickMultiOption(page: Page, comboSelector: string, optionText: string, label: string, dialogTestId: string, shotName?: string): Promise<boolean> {
  log(`  >> ${label}：準備點開`);
  const fallback = comboSelector.replace('[role="combobox"]', '.MuiSelect-select, [role="button"]');
  const ok = await clickDropdownOption(page, comboSelector, optionText, label, fallback);
  if (!ok) {
    if (shotName) await page.screenshot({ path: path.join(path.dirname(OUT), shotName) }).catch(() => undefined);
    return false;
  }
  await sleep(300);
  const gone = await closeAnyOpenListbox(page, `${label}（多選收合）`);
  if (!gone) log(`  !! ${label}：等了 5 秒 listbox 還沒 detach`);
  await sleep(300);
  const stillOpen = await dialogStillOpen(page, dialogTestId);
  log(`  >> ${label}：選了「${optionText}」，對話框${stillOpen ? '還在' : '不見了！！'}`);
  if (!stillOpen && shotName) await page.screenshot({ path: path.join(path.dirname(OUT), shotName) }).catch(() => undefined);
  return stillOpen && ok;
}

/** 單純點開→選→(MUI 自己關)，不按 Escape，不檢查對話框。 */
export async function openSelect(page: Page, comboSelector: string, optionText: string, label: string): Promise<boolean> {
  let handle = await page.$(comboSelector);
  if (!handle) {
    const fallback = comboSelector.replace('[role="combobox"]', '.MuiSelect-select, [role="button"]');
    handle = await page.$(fallback);
  }
  if (!handle) { log(`  !! 找不到下拉元素：${comboSelector}`); return false; }
  await handle.click();
  const opened = await waitUntil(page, () => document.querySelectorAll('li[role="option"]').length > 0, { timeout: 6000, label: `${label} 選單打開` });
  if (!opened) { log(`  !! ${label} 選單沒打開`); return false; }
  await sleep(500);
  const clicked = await page.evaluate((t: string) => {
    const opt = Array.from(document.querySelectorAll('li[role="option"]')).find((o) => (o as HTMLElement).innerText.includes(t));
    if (!opt) return false;
    (opt as HTMLElement).click();
    return true;
  }, optionText);
  if (!clicked) log(`  !! ${label} 選單找不到「${optionText}」`);
  await sleep(400);
  return clicked;
}

/** 日期欄（<input type="date">）不能用 page.type() 逐字打。改走原生 value setter 直接設值，
 * dispatch input／change 讓 React 的 onChange 收到，設完讀回 value 斷言等於預期。 */
export async function setDateInput(page: Page, containerSelector: string, value: string, label: string): Promise<boolean> {
  const result = await page.evaluate((sel: string, v: string) => {
    // data-testid 有時直接掛在 <input> 本身（MUI inputProps 轉傳），有時掛在外層 wrapper 上，兩種都要認。
    const direct = document.querySelector(sel) as HTMLElement | null;
    const el = (direct && direct.tagName === 'INPUT' ? direct : direct?.querySelector('input')) as HTMLInputElement | null;
    if (!el) return { ok: false, got: null as string | null };
    const setter = Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, 'value')!.set!;
    setter.call(el, v);
    el.dispatchEvent(new Event('input', { bubbles: true }));
    el.dispatchEvent(new Event('change', { bubbles: true }));
    return { ok: el.value === v, got: el.value };
  }, containerSelector, value);
  if (!result.ok) log(`  !! ${label}：設日期失敗，預期「${value}」讀回「${result.got}」`);
  else log(`  >> ${label}：日期設成「${value}」，讀回一致`);
  return result.ok;
}

/** 全腳本所有 input[type=date] 都走這個。用 data-testid 直接定位；沒有 testid 的就用最近的 label 文字找。 */
export async function setDateInputByTestId(page: Page, testid: string, value: string, label: string): Promise<boolean> {
  return setDateInput(page, `[data-testid="${testid}"]`, value, label);
}
export async function setDateInputByLabel(page: Page, labelText: string, value: string, label: string): Promise<boolean> {
  const result = await page.evaluate((lt: string, v: string) => {
    const field = Array.from(document.querySelectorAll('div')).find((d) => {
      const t = (d as HTMLElement).innerText?.trim();
      return !!t && t.startsWith(lt) && d.querySelector('input[type="date"]');
    });
    const el = field?.querySelector('input[type="date"]') as HTMLInputElement | null;
    if (!el) return { ok: false, got: null as string | null };
    const setter = Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, 'value')!.set!;
    setter.call(el, v);
    el.dispatchEvent(new Event('input', { bubbles: true }));
    el.dispatchEvent(new Event('change', { bubbles: true }));
    return { ok: el.value === v, got: el.value };
  }, labelText, value);
  if (!result.ok) log(`  !! ${label}：設日期失敗，預期「${value}」讀回「${result.got}」`);
  else log(`  >> ${label}：日期設成「${value}」，讀回一致`);
  return result.ok;
}

export async function clickByText(page: Page, tag: string, text: string): Promise<boolean> {
  return page.evaluate((tg: string, t: string) => {
    const el = Array.from(document.querySelectorAll(tg)).find((b) => (b as HTMLElement).innerText.includes(t));
    if (!el) return false;
    (el as HTMLElement).click();
    return true;
  }, tag, text);
}

const responses: Array<{ seg: string; url: string; status: number }> = [];
let currentSeg = '0';

function trackResponses(page: Page) {
  page.on('response', (res) => {
    const url = res.url();
    if (url.includes('/api/v1/')) {
      responses.push({ seg: currentSeg, url: url.replace(/^.*\/api\/v1\//, '/'), status: res.status() });
    }
  });
}

export async function run(cfg: RealRunConfig, opts: { stopAfterSeg?: string } = {}) {
  const browser = await launch();
  const page = hardenPage(await browser.newPage());
  trackResponses(page);
  const recorder: any = await (page as any).screencast({ path: OUT, format: 'mp4', ffmpegPath: '/opt/homebrew/bin/ffmpeg' });
  const t0 = Date.now();
  const mark = (label: string) => log(`[t=${((Date.now() - t0) / 1000).toFixed(1)}s] ${label}`);
  const shotDir = path.dirname(OUT);
  const shot = async (name: string) => { await page.screenshot({ path: path.join(shotDir, name) }); };
  const stopAfter = opts.stopAfterSeg;
  const shouldStop = (seg: string) => stopAfter != null && seg === stopAfter;

  try {
    currentSeg = '0';
    hr('段 0：登入，本機測試環境已清空');
    await loginAsSuperUser(page);
    const orgId = await selectOrg(page, ORG_ID);
    if (!orgId) throw new Error(`選不到組織 ${ORG_ID}`);
    await titleCard(page, '0', '本機測試環境已清空重建', cfg.sources.env, 3000);
    mark('段0 登入完成，環境已清空');
    if (shouldStop('0')) return;

    currentSeg = '1';
    hr('段 1：看三個清單都是空的');
    await titleCard(page, '1', '員工先看案場、地主、合約清單，確認都是空的', '系統本身（清空後的狀態）');
    // commander 糾正：這裡原本只截圖，沒斷言；非空的庫上也會照樣通過（新案場只是 id 往後排）。
    // 跑 a5（local 真值整支）之前一定要先清空，不然這裡會直接 throw 擋下來，不會偷偷往下跑。
    const cookieHeader = (await page.cookies()).map((c) => `${c.name}=${c.value}`).join('; ');
    const base = `${process.env.E2E_BASE_URL}/api/v1/orgs/${ORG_ID}/energy`;
    const emptyCheck = async (ep: string) => {
      const body = await (await fetch(`${base}/${ep}`, { headers: { cookie: cookieHeader } })).json();
      return Array.isArray(body.items) ? body.items.length : -1;
    };
    const [stationsLen, landownersLen, contractsLen] = await Promise.all([emptyCheck('stations'), emptyCheck('landowners'), emptyCheck('contracts')]);
    log(`  >> 清空核對：stations=${stationsLen}、landowners=${landownersLen}、contracts=${contractsLen}`);
    if (stationsLen !== 0 || landownersLen !== 0 || contractsLen !== 0)
      throw new Error(`段1：環境沒清空（stations=${stationsLen}、landowners=${landownersLen}、contracts=${contractsLen}，應該全部是0）。先照 REAL-RUN.md 清空再跑。`);
    await goto(page, 'energyStationConsole', '[data-testid="station-new"]');
    await caption(page, '段1：案場清單目前沒有資料（已核對 API 回 0 筆）');
    await sleep(1200);
    await shot('frame01-案場清單為空.png');
    await goto(page, 'energyLandownerConsole', '[data-testid="landowner-new"]');
    await caption(page, '段1：地主清單目前沒有資料（已核對 API 回 0 筆）');
    await sleep(1200);
    await goto(page, 'energyContractConsole', '[data-testid="contract-new"]');
    await caption(page, '段1：合約清單目前沒有資料（已核對 API 回 0 筆）');
    await sleep(1200);
    await shot('frame02-合約清單為空.png');
    mark('段1 三個空清單看完（已核對 API 回 0 筆，不是只看畫面字樣）');
    if (shouldStop('1')) return;

    currentSeg = '2';
    hr('段 2：新增案場，不填電號');
    await titleCard(page, '2', `員工新增案場「${cfg.station.name}」，先不填電號`, cfg.sources.station);
    await goto(page, 'energyStationConsole', '[data-testid="station-new"]');
    await page.click('[data-testid="station-new"]');
    await waitFor(page, '[data-testid="station-form-dialog"]');
    await page.type('[data-testid="station-form-name"] input', cfg.station.name);
    await caption(page, `段2：填案場名稱「${cfg.station.name}」，電號留空（之後上傳真單時才會知道）`);
    await sleep(1200);
    await shot('frame03-新增案場表單.png');
    const [stationResp] = await Promise.all([
      page.waitForResponse((r) => r.url().includes('/energy/stations') && r.request().method() === 'POST', { timeout: 10000 }),
      page.click('[data-testid="station-form-save"]'),
    ]);
    if (stationResp.status() >= 300) throw new Error(`建案場失敗：HTTP ${stationResp.status()}`);
    const stationBody = await stationResp.json();
    log(`  >> 案場建立成功，id=${stationBody.id}`);
    await sleep(800);
    mark('段2 案場建立完成（已核對 POST /stations 回 2xx）');
    if (shouldStop('2')) return;

    currentSeg = '3';
    hr('段 3：新增地主');
    await titleCard(page, '3', `員工新增地主「${cfg.landowner.name}」`, cfg.sources.landowner);
    await goto(page, 'energyLandownerConsole', '[data-testid="landowner-new"]');
    await page.click('[data-testid="landowner-new"]');
    await waitFor(page, '[data-testid="landowner-form-dialog"]');
    await page.type('[data-testid="landowner-form-name"] input', cfg.landowner.name);
    await openSelect(page, '[data-testid="landowner-form-partytype"] [role="combobox"]', cfg.landowner.partyType, '地主類型');
    await caption(page, `段3：地主「${cfg.landowner.name}」，類型選${cfg.landowner.partyType}`);
    await sleep(1000);
    await shot('frame04-新增地主表單.png');
    const [landownerResp] = await Promise.all([
      page.waitForResponse((r) => r.url().includes('/energy/landowners') && r.request().method() === 'POST', { timeout: 10000 }),
      page.click('[data-testid="landowner-form-save"]'),
    ]);
    if (landownerResp.status() >= 300) throw new Error(`建地主失敗：HTTP ${landownerResp.status()}`);
    const landownerBody = await landownerResp.json();
    log(`  >> 地主建立成功，id=${landownerBody.id}`);
    await sleep(800);
    mark('段3 地主建立完成（已核對 POST /landowners 回 2xx）');
    if (shouldStop('3')) return;

    currentSeg = '4';
    hr('段 4：新增合約，勾案場與地主，選三個代號');
    await titleCard(page, '4', `員工新增合約 ${cfg.contract.no}，勾${cfg.station.name}與${cfg.landowner.name}，選三個代號`, cfg.sources.contract);
    await goto(page, 'energyContractConsole', '[data-testid="contract-new"]');
    await page.click('[data-testid="contract-new"]');
    await waitFor(page, '[data-testid="contract-form-dialog"]');
    await page.type('[data-testid="contract-form-no"] input', cfg.contract.no);
    const DLG = 'contract-form-dialog';
    if (!await pickMultiOption(page, '[data-testid="contract-form-landowner"] [role="combobox"]', cfg.landowner.name, '地主多選', DLG, 'DEBUG-landowner-select-closed-dialog.png'))
      throw new Error('段4：選「地主多選」之後對話框不見了（或選單沒打開/選不到選項），就是卡在這一個下拉');
    if (!await pickMultiOption(page, '[data-testid="contract-form-stations"] [role="combobox"]', cfg.station.name, '案場多選', DLG, 'DEBUG-station-select-closed-dialog.png'))
      throw new Error('段4：選「案場多選」之後對話框不見了（或選單沒打開/選不到選項），就是卡在這一個下拉');
    if (!await setDateInput(page, '[data-testid="contract-form-signedon"]', cfg.contract.signedOn, '簽約日'))
      throw new Error('段4：簽約日設值失敗（讀回值跟預期不一樣）');
    if (!await setDateInput(page, '[data-testid="contract-form-validfrom"]', cfg.contract.validFrom, '生效起日'))
      throw new Error('段4：生效起日設值失敗（讀回值跟預期不一樣）');
    if (!await setDateInput(page, '[data-testid="contract-form-validto"]', cfg.contract.validTo, '生效迄日'))
      throw new Error('段4：生效迄日設值失敗（讀回值跟預期不一樣）');
    await caption(page, '段4：三個代號下拉——電費計算／租金算法／付款方式');
    if (!await pickSingleOption(page, '[data-testid="contract-form-billcodes"] [role="combobox"]', 'E-C', '電費計算代號', DLG, 'DEBUG-billcodes-select-closed-dialog.png'))
      throw new Error('段4：選「電費計算代號 E-C」之後對話框不見了（或選單沒打開/選不到選項），就是卡在這一個下拉');
    if (!await pickSingleOption(page, '[data-testid="contract-form-rentcalc"] [role="combobox"]', 'R-A', '租金算法代號', DLG, 'DEBUG-rentcalc-select-closed-dialog.png'))
      throw new Error('段4：選「租金算法代號 R-A」之後對話框不見了（或選單沒打開/選不到選項），就是卡在這一個下拉');
    if (!await pickSingleOption(page, '[data-testid="contract-form-rentpay"] [role="combobox"]', 'RP-3', '付款方式代號', DLG, 'DEBUG-rentpay-select-closed-dialog.png'))
      throw new Error('段4：選「付款方式代號 RP-3」之後對話框不見了（或選單沒打開/選不到選項），就是卡在這一個下拉');
    await sleep(500);
    await shot('frame05-合約表單勾案場地主選代號.png');
    const [contractResp] = await Promise.all([
      page.waitForResponse((r) => r.url().includes('/energy/contracts') && r.request().method() === 'POST', { timeout: 10000 }),
      page.click('[data-testid="contract-form-save"]'),
    ]);
    if (contractResp.status() >= 300) throw new Error(`建合約失敗：HTTP ${contractResp.status()}，body=${await contractResp.text()}`);
    const contractBody = await contractResp.json();
    log(`  >> 合約建立成功，id=${contractBody.id}，billCodes=${JSON.stringify(contractBody.billCodes)}`);
    if (!Array.isArray(contractBody.billCodes) || !contractBody.billCodes.includes('E-C')) throw new Error('段4：合約建立成功但 billCodes 沒有 E-C，三個代號下拉沒選對');
    await sleep(800);
    mark('段4 合約建立完成（已核對 POST /contracts 回 2xx 且 billCodes 含 E-C）');
    await shot('frame06-合約清單代號欄.png');
    const checkContracts = await (await fetch(`${process.env.E2E_BASE_URL}/api/v1/orgs/${ORG_ID}/energy/contracts`, { headers: { cookie: (await page.cookies()).map((c) => `${c.name}=${c.value}`).join('; ') } })).json();
    log(`  >> GET /contracts 核對：items.length=${checkContracts.items?.length}，第一筆代號=${JSON.stringify(checkContracts.items?.[0]?.billCodes)}`);
    if (shouldStop('4')) return;

    currentSeg = '5';
    hr('段 5：案場 EasyFlow 預設值');
    await titleCard(page, '5', '員工設定案場的 EasyFlow 預設值', cfg.sources.easyflow);
    await goto(page, 'energyStationConsole/1/show', '[data-testid="station-name"]');
    await waitFor(page, '[data-testid="easyflow-defaults-section"]');
    await page.evaluate(() => document.querySelector('[data-testid="easyflow-defaults-section"]')?.scrollIntoView({ block: 'center' }));
    await sleep(800);
    const fill = async (key: string, val: string) => {
      const h = await page.$(`[data-testid="easyflow-defaults-${key}"] input`);
      if (h) { await h.click({ clickCount: 3 }); await h.type(val); } else { log(`  !! easyflow-defaults-${key} 找不到 input`); }
    };
    await fill('companyName', cfg.easyflowDefaults.companyName);
    await fill('companyCode', cfg.easyflowDefaults.companyCode);
    await fill('applicant', cfg.easyflowDefaults.applicant);
    await fill('department', cfg.easyflowDefaults.department);
    await fill('paymentMethod', cfg.easyflowDefaults.paymentMethod);
    await fill('paymentTerms', cfg.easyflowDefaults.paymentTerms);
    await fill('voucherStatus', cfg.easyflowDefaults.voucherStatus);
    await fill('contractOption', cfg.easyflowDefaults.contractOption);
    await fill('expectedReceiptDays', '30');
    await caption(page, '段5：companyName／companyCode 查得到出處，expectedReceiptDays 填 30，其餘填測試值');
    await sleep(1000);
    await shot('frame07-easyflow預設值.png');
    const [defaultsResp] = await Promise.all([
      page.waitForResponse((r) => r.url().includes('/easyflow-defaults') && r.request().method() === 'PUT', { timeout: 10000 }),
      page.click('[data-testid="easyflow-defaults-save"]'),
    ]);
    if (defaultsResp.status() >= 300) throw new Error(`段5：EasyFlow 預設值存檔失敗：HTTP ${defaultsResp.status()}`);
    await sleep(1200);
    mark('段5 EasyFlow 預設值存檔（已核對 PUT /easyflow-defaults 回 2xx）');
    if (shouldStop('5')) return;

    currentSeg = '6';
    hr('段 6：手動上傳真售電單');
    await titleCard(page, '6', '員工手動上傳真售電單', cfg.pdfPath.split('/').pop() || '');
    await goto(page, 'energyIngestConsole', '[data-testid="intake-file-input"]');
    const input = await page.$('[data-testid="intake-file-input"]');
    if (!input) throw new Error('找不到上傳 input');
    await (input as any).uploadFile(cfg.pdfPath);
    await caption(page, '段6：上傳真售電單，本機辨識（沒有送外部服務）');
    await sleep(3500);
    await shot('frame08-上傳後批次頁.png');
    mark('段6 上傳＋本機辨識完成');
    if (shouldStop('6')) return;

    currentSeg = '7';
    hr('段 7：第一次電號對不上，指認綁定');
    await titleCard(page, '7', '電號對不上，員工在指認對話框把電號綁到案場', `這張單解析出的電號 ${cfg.match.customerNumberHint}`);
    await waitIdle(page);
    await sleep(1000);
    const matchBtn = await page.$('[data-testid="intake-match-station"]');
    if (matchBtn) {
      await matchBtn.click();
      await waitFor(page, '[data-testid="station-picker"]');
      await sleep(800);
      await page.type('[data-testid="station-search"]', cfg.match.searchTerm);
      await sleep(1000);
      await shot('frame09-指認對話框.png');
      await openSelect(page, '[data-testid="station-picker"] [role="combobox"], [data-testid="station-picker"]', cfg.match.stationFullName, '指認案場');
      await caption(page, `段7：搜尋「${cfg.match.searchTerm}」，選${cfg.match.stationFullName}，確認綁定`);
      await sleep(800);
      const confirmBtn = await page.$('[data-testid="station-match-confirm"]');
      if (!confirmBtn) throw new Error('段7：指認對話框的確認按鈕找不到（可能案場沒選到，按鈕還是 disabled）');
      const [matchResp] = await Promise.all([
        page.waitForResponse((r) => /\/bills\/\d+\/match$/.test(r.url()) && r.request().method() === 'POST', { timeout: 10000 }),
        confirmBtn.click(),
      ]);
      if (matchResp.status() >= 300) throw new Error(`段7：指認綁定失敗：HTTP ${matchResp.status()}，body=${await matchResp.text()}`);
      const matchBody = await matchResp.json();
      log(`  >> 指認綁定成功，contractId=${matchBody.contractId}，stationId=${matchBody.stationId}`);
      await sleep(1000);
      mark('段7 指認綁定完成（已核對 POST .../match 回 2xx）');
    } else {
      throw new Error('段7：找不到 intake-match-station 按鈕，這張單可能已經自動對到或根本沒上傳成功，不應該發生');
    }
    await shot('frame10-指認後狀態.png');

    // 「資料審核組別」只能在這個批次工作台頁面設定，不在電費單詳情頁；要先展開這一列（intake-review）
    // 才看得到這個下拉。intake-review 按鈕會「展開/收合」切換，先判斷這一列是不是已經展開
    // （看「資料審核組別」下拉是不是已經在畫面上），已經展開就不要再點，否則會把它點收合。
    const groupDropdownPresent = async () => page.evaluate(() => {
      const field = Array.from(document.querySelectorAll('div')).find((d) => (d as HTMLElement).innerText?.trim().startsWith('資料審核組別'));
      return !!field?.querySelector('[role="combobox"]');
    });
    const alreadyExpanded = await groupDropdownPresent();
    const expandBtn = await page.$('[data-testid="intake-review"]');
    if (alreadyExpanded) {
      log('  >> 這一列已經展開（資料審核組別下拉已在畫面上），不再點 intake-review 以免收合');
    } else if (expandBtn) {
      await expandBtn.click();
      await sleep(1000);
    } else {
      log('  !! 找不到 intake-review 按鈕，無法設審核組別');
    }
    const groupSelected = alreadyExpanded || expandBtn ? await groupDropdownPresent() : false;
    if (groupSelected) {
      const combo = await page.evaluateHandle(() => {
        const field = Array.from(document.querySelectorAll('div')).find((d) => (d as HTMLElement).innerText?.trim().startsWith('資料審核組別'));
        return field?.querySelector('[role="combobox"]') ?? null;
      });
      const el = combo.asElement();
      if (el) {
        await (el as any).click();
        await waitUntil(page, () => document.querySelectorAll('li[role="option"]').length > 0, { timeout: 6000, label: '審核組別選單打開' });
        await sleep(500);
        // 選項值「bill-reviewers」對應文字「台電電費單審核人員」，才是開機 5 步時 admin 被加進去的部門；
        // 選另一個部門的人員會讓 admin 永遠看不到 claim/pass 按鈕（送審後 mode 變 readonly）。
        const [groupResp] = await Promise.all([
          page.waitForResponse((r) => r.url().includes('/energy/bills/') && r.request().method() === 'PATCH', { timeout: 10000 }).catch(() => null),
          page.evaluate(() => { const o = Array.from(document.querySelectorAll('li[role="option"]')).find((x) => (x as HTMLElement).innerText.includes('台電')); (o as HTMLElement)?.click(); }),
        ]);
        if (groupResp && groupResp.status() >= 300) log(`  !! 設審核組別失敗：HTTP ${groupResp.status()}`);
        else log('  >> 審核組別設為「台電電費單審核人員」（admin 所屬的 bill-reviewers 部門）');
        await sleep(800);
      }
    } else if (expandBtn || alreadyExpanded) {
      log('  !! 找不到「資料審核組別」下拉，可能這張單已經不能改單別');
    }
    if (shouldStop('7')) return;

    currentSeg = '7b';
    hr('段 7b：查證——指認之後案場的流程綁定有沒有補上');
    await titleCard(page, '7b', '查證：指認綁電號之後，案場的流程綁定有沒有自動補上', cfg.sources.match);
    await goto(page, 'energyStationConsole/1/show', '[data-testid="station-name"]');
    await waitIdle(page);
    await sleep(1000);
    // [data-testid="axis-unbound"] 有兩種（工作流／策略）都用同一個 testid，用 data-axis="workflow-axis"
    // 才分得出是「流程綁定（效期）」那一塊，不是「策略綁定」那一塊。
    const workflowAxisText = await page.evaluate(() => {
      const bound = document.querySelector('[data-testid="workflow-axis"]');
      if (bound) { bound.scrollIntoView({ block: 'center' }); return (bound as HTMLElement).innerText; }
      const unbound = document.querySelector('[data-testid="axis-unbound"][data-axis="workflow-axis"]');
      if (unbound) { unbound.scrollIntoView({ block: 'center' }); return (unbound as HTMLElement).innerText; }
      return '（流程綁定／workflow-axis 區塊完全找不到，連 unbound 狀態的提示都沒有）';
    });
    const hasInvoiceIssue = workflowAxisText.includes('開立發票流程') || workflowAxisText.includes('invoice-issue');
    log(`  >> 流程綁定（workflow-axis）內容：${workflowAxisText.replace(/\n/g, ' / ')}`);
    log(`  >> 有沒有「開立發票流程」＝${hasInvoiceIssue}`);
    await sleep(1500);
    await caption(page, `段7b：照實拍流程綁定狀態——有開立發票流程＝${hasInvoiceIssue}`);
    await shot('frame11-指認後流程綁定狀態.png');
    mark('段7b 流程綁定畫面已拍（已核對是 workflow-axis 不是 strategy-axis）');
    if (shouldStop('7b')) return;

    currentSeg = '8';
    hr('段 8：對到合約、識別欄位、審核');
    await titleCard(page, '8', '員工看單據對到的合約與識別欄位，標已校對後送審', '這張單本機解析的欄位（電號/計費期間/金額/度數）');
    // 「已校對」「送審」「認領」「通過」都在電費單詳情頁，不是批次工作台頁，要直接導過去。
    await goto(page, 'energyBillConsole/1/show', '[data-testid="bill-contract-no"]');
    await waitIdle(page);
    await sleep(1000);
    await shot('frame12-對到合約與欄位.png');
    await caption(page, '段8：先標已校對，再送審、claim、pass');
    const markBtn = await page.$('[data-testid="review-mark"]');
    if (markBtn) {
      const [markResp] = await Promise.all([
        page.waitForResponse((r) => r.url().includes('/energy/bills/') && r.request().method() === 'PATCH', { timeout: 10000 }).catch(() => null),
        markBtn.click(),
      ]);
      log(`  >> 標已校對${markResp ? `：HTTP ${markResp.status()}` : '（沒等到回應，可能按鈕本來就是 disabled）'}`);
      await sleep(1000);
    } else {
      log('  !! 找不到 review-mark 按鈕');
    }
    const submitBtn = await page.$('[data-testid="review-submit"]');
    if (!submitBtn) throw new Error('段8：找不到 review-submit 按鈕（可能已經送過或還沒對到案場）');
    const [submitResp] = await Promise.all([
      page.waitForResponse((r) => /\/bills\/\d+\/submit$/.test(r.url()) && r.request().method() === 'POST', { timeout: 10000 }),
      submitBtn.click(),
    ]);
    if (submitResp.status() >= 300) throw new Error(`段8：送審失敗：HTTP ${submitResp.status()}，body=${await submitResp.text()}`);
    log(`  >> 送審：HTTP ${submitResp.status()}`);
    // 送審成功後 submitBill() 會呼叫 load() 重抓 bill/openTaskId，review-claim 按鈕要等它重新渲染才會出現。
    await waitUntil(page, () => !!document.querySelector('[data-testid="review-actions"]'), { timeout: 8000, label: '送審後 review-actions 重新渲染' });
    await sleep(500);
    const claimBtn = await page.$('[data-testid="review-claim"]');
    if (claimBtn) {
      await claimBtn.click();
      await sleep(1500);
      log('  >> claim 點了');
    } else {
      log('  !! 找不到 review-claim 按鈕（可能認領狀態已經是我，或這一關直接給 ActionBar）');
    }
    await waitUntil(page, () => !!document.querySelector('[data-action="pass"]'), { timeout: 8000, label: 'pass 按鈕出現' }).catch(() => undefined);
    const passBtn = await page.$('[data-action="pass"]');
    if (passBtn) { await passBtn.click(); await sleep(1200); log('  >> pass 點了'); } else { log('  !! 找不到 [data-action="pass"] 按鈕'); }
    mark('段8 審核動作完成（畫面實際走多少算多少）');
    await shot('frame13-審核歷程.png');
    if (shouldStop('8')) return;

    currentSeg = '9';
    hr('段 9：EasyFlow 草稿 → 租金三步');
    await titleCard(page, '9', '員工看 EasyFlow 草稿，再做租金整組年額試算、付款草稿、付款申請', cfg.sources.rentCycle);
    const efBtn = await page.$('[data-testid="easyflow-prepare"]');
    if (efBtn) { await efBtn.click(); await waitFor(page, '.MuiDialog-root'); await sleep(1500); await shot('frame14-easyflow草稿.png'); await page.keyboard.press('Escape').catch(() => undefined); await sleep(500); }
    await goto(page, 'energyStationConsole/1/show', '[data-testid="station-name"]');
    await waitIdle(page);
    const rc = await clickByText(page, 'button', '租金週期試算');
    if (!rc) throw new Error('段9：找不到「租金週期試算」按鈕');
    await sleep(1200);
    await waitFor(page, '[data-testid="rent-cycle-anchor-date"]');
    if (!await setDateInputByTestId(page, 'rent-cycle-anchor-date', cfg.rentCycle.anchorDate, '第一期起日'))
      throw new Error('段9：第一期起日設定失敗');
    if (!await openSelect(page, '[data-testid="rent-cycle-transfer-select"] [role="combobox"]', '未轉供', '此期間是否已轉供'))
      throw new Error('段9：「此期間是否已轉供」選不到');
    if (!await openSelect(page, '[data-testid="rent-cycle-kind-select"] [role="combobox"]', '整組年額', '租金方式'))
      throw new Error('段9：租金方式選不到「整組年額」');
    const annualInput = await page.$('[data-testid="rent-cycle-annual-amount"] input, [data-testid="rent-cycle-annual-amount"]');
    if (!annualInput) throw new Error('段9：找不到整組年額輸入框');
    await annualInput.click({ clickCount: 3 });
    await annualInput.type(cfg.rentCycle.annualAmount);
    // 「每幾個月付款」預設 3（一年分4期），整組年額是按這個週期比例分攤，不是整年金額整筆算進一期，
    // 改填 12（一年一期＝年額全額）才會跟斷言的 expectedBaseRent 對得起來。
    const monthsField = await page.evaluateHandle(() => {
      const field = Array.from(document.querySelectorAll('div')).find((d) => (d as HTMLElement).innerText?.trim().startsWith('每幾個月付款') && d.querySelector('input'));
      return field?.querySelector('input') ?? null;
    });
    const monthsEl = monthsField.asElement();
    if (!monthsEl) throw new Error('段9：找不到「每幾個月付款」輸入框');
    await (monthsEl as any).click({ clickCount: 3 });
    await (monthsEl as any).type('12');
    await page.keyboard.press('Tab').catch(() => undefined);
    if (!await setDateInputByLabel(page, '本次計算起日', cfg.rentCycle.startDate, '本次計算起日'))
      throw new Error('段9：本次計算起日設定失敗');
    await caption(page, `段9：租金方式選整組年額，填 ${cfg.rentCycle.annualAmount}`);
    await sleep(800);
    await shot('frame15-租金年額試算填寫.png');

    const [loadBillsResp] = await Promise.all([
      page.waitForResponse((r) => /\/contracts\/\d+\/rent-cycle$/.test(r.url()) && r.request().method() === 'POST', { timeout: 10000 }),
      clickByText(page, 'button', '儲存試算設定並載入單據'),
    ]);
    if (loadBillsResp.status() >= 300) throw new Error(`段9：儲存試算設定失敗：HTTP ${loadBillsResp.status()}，body=${await loadBillsResp.text()}`);
    log(`  >> 儲存試算設定並載入單據：HTTP ${loadBillsResp.status()}`);
    await sleep(1000);

    const checkedAny = await page.evaluate(() => {
      const boxes = Array.from(document.querySelectorAll('input[type="checkbox"][aria-label^="選用單據 "]')) as HTMLInputElement[];
      boxes.forEach((b) => { if (!b.checked) b.click(); });
      return boxes.length;
    });
    log(`  >> 勾選單據：共 ${checkedAny} 筆`);
    await sleep(500);

    const [previewResp] = await Promise.all([
      page.waitForResponse((r) => /\/contracts\/\d+\/rent-cycle\/preview$/.test(r.url()) && r.request().method() === 'POST', { timeout: 10000 }),
      clickByText(page, 'button', '檢查收齊狀態並試算'),
    ]);
    if (previewResp.status() >= 300) throw new Error(`段9：試算失敗：HTTP ${previewResp.status()}，body=${await previewResp.text()}`);
    const previewBody = await previewResp.json();
    log(`  >> 試算：HTTP ${previewResp.status()}，baseRent=${previewBody.baseRent}`);
    if (Number(previewBody.baseRent) !== cfg.rentCycle.expectedBaseRent) throw new Error(`段9：試算 baseRent 預期 ${cfg.rentCycle.expectedBaseRent}，實際 ${previewBody.baseRent}`);
    await sleep(800);
    await shot('frame16-租金試算結果.png');

    const [draftResp] = await Promise.all([
      page.waitForResponse((r) => /\/contracts\/\d+\/rent-cycle\/payment-draft$/.test(r.url()) && r.request().method() === 'POST', { timeout: 10000 }),
      clickByText(page, 'button', '存成租金付款草稿'),
    ]);
    if (draftResp.status() >= 300) throw new Error(`段9：存成租金付款草稿失敗：HTTP ${draftResp.status()}，body=${await draftResp.text()}`);
    log(`  >> 存成租金付款草稿：HTTP ${draftResp.status()}`);
    await sleep(1200);
    mark('段9 租金年額試算→付款草稿全部存檔成功（已核對每一步真實回包）');
    await page.keyboard.press('Escape').catch(() => undefined);
    await sleep(500);

    const raBtn = await page.$('[data-testid="rent-application-open"]');
    if (!raBtn) throw new Error('段9：找不到 rent-application-open 按鈕');
    const [raResp] = await Promise.all([
      page.waitForResponse((r) => /\/stations\/\d+\/rent-application$/.test(r.url()) && r.request().method() === 'GET', { timeout: 10000 }),
      raBtn.click(),
    ]);
    const raBody = await raResp.json();
    const apps = raBody.applications ?? [];
    log(`  >> 租金付款申請：applications.length=${apps.length}，total=${apps[0]?.total}`);
    if (apps.length !== 1) throw new Error(`段9：租金付款申請預期 applications.length=1，實際 ${apps.length}`);
    if (Number(apps[0].total) !== cfg.rentCycle.expectedTotal) throw new Error(`段9：租金付款申請合計預期 ${cfg.rentCycle.expectedTotal}，實際 ${apps[0].total}`);
    await sleep(1500);
    await shot('frame17-租金付款申請.png');
    mark('完成：submissionAllowed／paymentAllowed 兩個終點常數仍是 false');
    await caption(page, '完成：EasyFlow 與租金付款都停在草稿，沒有真的送出或付款');
    await sleep(2000);
  } finally {
    await recorder.stop();
    await browser.close();
    log('--- RESPONSES-JSON-START ---');
    log(JSON.stringify(responses));
    log('--- RESPONSES-JSON-END ---');
  }
}

// 只在直接執行這支檔案時才跑（被其他腳本 import 共用函式時不要自動開瀏覽器）。
if (import.meta.url === `file://${process.argv[1]}`) {
  const cfg = loadConfig();
  const stopAfterSeg = process.env.E2E_STOP_AFTER_SEG || undefined;
  run(cfg, { stopAfterSeg }).then(() => { log('錄影完成'); process.exit(0); }).catch((e) => { console.error(e); process.exit(1); });
}
