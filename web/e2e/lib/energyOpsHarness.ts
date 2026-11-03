/* eslint-disable no-console, no-await-in-loop */
/**
 * energy-ops crawler 的共用 harness（移植自 az `scripts/shared/puppeteer/energyOpsHarness.ts`）。
 *
 * 匯出名稱與原版相同，crawler 與之後的示範影片腳本都從這裡取用：
 *
 * | 名稱 | 用途 |
 * |---|---|
 * | `APP`、`BASE` | 前端網址（`${APP}#/<route>`；`BASE` 是後端本身，前端由它供應）；`E2E_BASE_URL` 指定 |
 * | `DEMO_PW`、`demoEmail(role)` | 示範帳號 `<role>@demo.energy`／`demo12345`（`EO_DEMO_SEED=1`） |
 * | `Scoreboard` | 計分板：`check(name, ok, detail?)`、`finish(label)`（最後印一行 `E2E_RESULT pass= fail=` 給 run-crawlers.sh） |
 * | `launch`、`hardenPage` | 起 headless Chrome；把 `page.evaluate` 包上 detach 復原、裝上在途請求計數 |
 * | `loginAs(page, email, pw?)`、`loginAsAdmin`、`loginAsSuperUser` | 走真的登入頁換身分（先登出、清 localStorage） |
 * | `selectOrg(page, orgId?)` | 找出有能源資料的組織，寫進選擇（localStorage `eo.selection`）並記成 `window.__demoOrgId` |
 * | `goto(page, hash, selector)`、`waitFor`、`waitUntil`、`waitIdle` | 導頁並等內容；等條件不等時間 |
 * | `eapi(page, method, path, body?)` | 在頁面裡用目前的 session 打 `/api/v1/orgs/{org}/energy{path}`，回 `{ status, data }` |
 * | `seedBatch`、`uploadNewBatch`、`writeUploadFiles`、`placeholderPdf` | 造送件批次（真上傳＋假辨識結果，或從收件台丟檔） |
 * | `SAMPLE_DIR`、`SAMPLE_FILES`、`samplePath(name)` | 四份真範本 PDF（`tests/fixtures/samples/`，檔名＝假辨識服務認的名字） |
 * | `sql(text)` | 對 crawler 專用資料庫跑 SQL（`docker compose exec -T postgres psql`，`E2E_DB`） |
 * | `shooter(dir)` | 截圖存到 `E2E_SHOTS_DIR`（run-crawlers.sh 給 `web/e2e/shots/<crawler>/`） |
 */
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import puppeteer from 'puppeteer';
import type { Browser, Page } from 'puppeteer';
import { mockOcrForUpload } from './ingestFixtures.ts';

const HERE = path.dirname(fileURLToPath(import.meta.url));
export const REPO_ROOT = path.resolve(HERE, '../../..');

export const BASE = (process.env.E2E_BASE_URL || 'http://127.0.0.1:10950').replace(/\/$/, '');
/** hash 路由的前綴：`${APP}#/energyMyWorkConsole`。 */
export const APP = `${BASE}/`;
export const DEMO_PW = process.env.ENERGY_DEMO_PASSWORD || 'demo12345';
export const demoEmail = (role: string) => `${role}@demo.energy`;
/** 平台超級使用者（後端 `EO_BOOTSTRAP_ADMIN_USER`）；跨組織、平台層的操作才用它。 */
export const SUPER_USER = process.env.E2E_SUPER_USER || 'admin@example.com';
export const SUPER_PW = process.env.E2E_SUPER_PASSWORD || 'admin12345';

export const SAMPLE_DIR = process.env.E2E_SAMPLE_DIR || path.join(REPO_ROOT, 'tests/fixtures/samples');
export const SAMPLE_FILES = {
  renewableSurplus: '電費單(有餘電需開發票).pdf',
  renewableNoSurplus: '電費單(無餘電需付款).pdf',
  consumptionDevice: '逆變器電費單(僅設備發電使用電費).pdf',
  consumptionDeviceRental: '逆變器電費單(含電表租費&設備發電使用電費).pdf',
} as const;
export const samplePath = (name: string) => path.join(SAMPLE_DIR, name);

export const sleep = (ms: number) => new Promise((r) => { setTimeout(r, ms); });
export const log = (...a: any[]) => console.log(`[${new Date().toISOString().slice(11, 23)}]`, ...a);
export const hr = (t: string) => console.log(`\n========== ${t} ==========`);

/** 最後一次 `selectOrg` 選到的組織；reload 之後要補回 `window.__demoOrgId`。 */
let lastOrgId: number | null = null;
const ORG_KEY = 'eo.e2e.org';

/**
 * detach 類錯誤的統一處置：包住一整個操作，只對 detach 類錯誤重試（其他錯誤照拋，
 * 免得把真正的產品失敗變成無聲重試）。main frame detach 之後原地重試沒用，要 reload 建新的 frame。
 */
export async function retryOnDetach<T>(
  label: string, fn: () => Promise<T>, attempts = 3, page?: Page,
): Promise<T> {
  for (let i = 0; i < attempts; i += 1) {
    try {
      return await fn();
    } catch (e: any) {
      const msg = `${e?.name ?? ''} ${e?.message ?? e}`;
      const transient = /detached|Target closed|Session closed|Connection closed|Execution context/i.test(msg);
      if (!transient || i === attempts - 1) throw e;
      log(`    ${label}：${String(e?.message ?? e).slice(0, 60)} → 重試 ${i + 1}/${attempts - 1}`);
      await sleep(1500 * (i + 1));
      if (page) {
        try {
          await page.reload({ waitUntil: 'domcontentloaded' });
          await sleep(1000);
          if (lastOrgId != null) {
            await page.evaluate((id: number) => { (window as any).__demoOrgId = id; }, lastOrgId);
          }
        } catch {
          // reload 也可能撞上同一個競態，留給下一輪
        }
      }
    }
  }
  throw new Error('unreachable');
}

/** 一支 crawler 的計分板。`failures` 決定 process 的 exit code。 */
export class Scoreboard {
  failures = 0;

  passes = 0;

  check(name: string, ok: boolean, detail?: unknown) {
    if (ok) { this.passes += 1; log(`  ✅ ${name}`); } else {
      this.failures += 1;
      log(`  ❌ ${name}`, detail === undefined ? '' : JSON.stringify(detail));
    }
  }

  finish(label: string) {
    hr('結果');
    if (this.failures) {
      log(`❌ ${label}：${this.passes} 通過 / ${this.failures} 未通過`);
      process.exitCode = 1;
    } else {
      log(`✅ ${label}：全部通過（${this.passes}）`);
    }
    console.log(`E2E_RESULT pass=${this.passes} fail=${this.failures}`);
  }
}

export async function launch(): Promise<Browser> {
  return puppeteer.launch({
    headless: process.env.CRAWLER_HEADFUL !== '1',
    args: ['--window-size=1440,1100', '--no-sandbox'],
    defaultViewport: { width: 1440, height: 1100 },
  });
}

/**
 * 把 `page.evaluate` 換成撞到 detach 就 reload 再試的版本，並裝上在途請求計數（`waitIdle` 靠它）。
 * 每個新文件載入時從 localStorage 補回 `window.__demoOrgId`（頁面內的 fetch 靠它決定組織）。
 */
export function hardenPage(page: Page): Page {
  trackRequests(page);
  page.evaluateOnNewDocument((key: string) => {
    try {
      const v = Number(localStorage.getItem(key));
      if (v) (window as any).__demoOrgId = v;
    } catch {
      // about:blank 沒有 localStorage
    }
  }, ORG_KEY).catch(() => undefined);
  const raw = page.evaluate.bind(page);
  (page as any).evaluate = (...args: any[]) => retryOnDetach(
    'evaluate', () => (raw as any)(...args), 3, page,
  );
  return page;
}

/** 輪詢到條件成立或逾時；detach 之類的暫時錯誤當成還沒好。 */
export async function waitUntil(
  page: Page,
  fn: () => boolean | Promise<boolean>,
  opts: { timeout?: number; poll?: number; label?: string } = {},
): Promise<boolean> {
  const timeout = opts.timeout ?? 20000;
  const poll = opts.poll ?? 150;
  const deadline = Date.now() + timeout;
  for (;;) {
    const ok = await page.evaluate(fn as any).catch(() => false);
    if (ok) return true;
    if (Date.now() > deadline) {
      if (opts.label) log(`    等不到「${opts.label}」（${timeout}ms）`);
      return false;
    }
    await sleep(poll);
  }
}

const inflight = new WeakMap<Page, { n: number; lastChange: number }>();

/** 裝上請求計數 —— `waitIdle` 靠它判斷這一波請求跑完了沒。 */
export function trackRequests(page: Page): Page {
  if (inflight.has(page)) return page;
  const state = { n: 0, lastChange: Date.now() };
  inflight.set(page, state);
  const bump = (d: number) => { state.n = Math.max(0, state.n + d); state.lastChange = Date.now(); };
  page.on('request', () => bump(1));
  page.on('requestfinished', () => bump(-1));
  page.on('requestfailed', () => bump(-1));
  // 換頁會把在途請求全部作廢，不歸零的話計數會永遠停在 >0。
  page.on('framenavigated', (f) => { if (f === page.mainFrame()) { state.n = 0; state.lastChange = Date.now(); } });
  return page;
}

/** 等到網路安靜且 DOM 不再變（取代固定的 sleep）。 */
export async function waitIdle(
  page: Page,
  opts: { quietMs?: number; timeout?: number } = {},
): Promise<void> {
  const quietMs = opts.quietMs ?? 400;
  const timeout = opts.timeout ?? 15000;
  const deadline = Date.now() + timeout;
  const state = inflight.get(page);
  let lastHtmlLen = -1;
  let quietSince = 0;
  for (;;) {
    const busy = state ? state.n > 0 : false;
    const len = await page.evaluate(() => document.body?.innerHTML.length ?? 0).catch(() => -1);
    const stable = !busy && len === lastHtmlLen && len >= 0;
    lastHtmlLen = len;
    if (stable) {
      if (!quietSince) quietSince = Date.now();
      if (Date.now() - quietSince >= quietMs) return;
    } else {
      quietSince = 0;
    }
    if (Date.now() > deadline) return;
    await sleep(100);
  }
}

export function shooter(dir: string) {
  const outDir = process.env.E2E_SHOTS_DIR || path.join(REPO_ROOT, 'web/e2e/shots', dir);
  return async (page: Page, name: string) => {
    fs.mkdirSync(outDir, { recursive: true });
    await page.screenshot({ path: path.join(outDir, `${name}.png`) as `${string}.png` });
  };
}

/** 頁面上目前有沒有有效的 session（cookie 是 httpOnly，只能問後端）。 */
async function hasSession(page: Page): Promise<boolean> {
  return page.evaluate(async () => {
    const r = await fetch('/api/v1/auth/session', { credentials: 'include' });
    return r.ok;
  }).catch(() => false);
}

/** 換身分：先登出、清 localStorage（不然沿用上一個人的選擇），再走登入頁。 */
export async function loginAs(page: Page, email: string, password = DEMO_PW) {
  await retryOnDetach('loginAs', async () => {
    await page.goto(`${APP}#/login`, { waitUntil: 'networkidle2' });
    await page.evaluate(async () => {
      // 沒有 session 時不打登出（那一發必然 401，會混進 crawler 的「非預期 401」檢查）
      const s = await fetch('/api/v1/auth/session', { credentials: 'include' }).catch(() => null);
      if (s?.ok) await fetch('/api/v1/auth/logout', { method: 'POST', credentials: 'include' }).catch(() => undefined);
      localStorage.clear();
    });
    await page.goto(`${APP}#/login`, { waitUntil: 'networkidle2' });
    await page.reload({ waitUntil: 'networkidle2' });
    await page.waitForSelector('input[name="email"]', { timeout: 30000 });
    await page.type('input[name="email"]', email, { delay: 8 });
    await page.type('input[name="password"]', password, { delay: 8 });
    await page.click('button[type=submit]');
  }, 3, page);
  for (let i = 0; i < 80; i += 1) {
    const away = await page.evaluate(() => !/^#\/login/.test(window.location.hash)).catch(() => false);
    if (away && await hasSession(page)) { lastOrgId = null; log(`  → ${email}`); return; }
    await sleep(250);
  }
  throw new Error(`登入失敗：${email}（停在 ${page.url()}）`);
}

/**
 * 示範組織的管理員（`energy:admin`，含 `energy:govern:system`：綁定管理要它）。
 * 原版用平台 super-user；本版的示範資料把同一組職責給了 `admin@demo.energy`（同時是三個部門的成員）。
 */
export async function loginAsAdmin(page: Page) {
  await loginAs(page, demoEmail('admin'));
}

/** 平台超級使用者（bootstrap admin）。 */
export async function loginAsSuperUser(page: Page) {
  await loginAs(page, SUPER_USER, SUPER_PW);
}

/**
 * 找出有能源資料的組織（查得到案場的那個），寫進選擇並記在 `window.__demoOrgId`。
 * 判準用資料不用名字：每個使用者都有個人組織，問錯組織的症狀是「查不到資料」。
 */
export async function selectOrg(page: Page, orgId?: number): Promise<number | null> {
  const id = await retryOnDetach('selectOrg', async () => {
    if (!page.url().startsWith(BASE)) await page.goto(`${APP}#/`, { waitUntil: 'domcontentloaded' });
    return page.evaluate(async (want: number | null) => {
      const r = await fetch('/api/v1/me/organizations', { credentials: 'include' });
      if (!r.ok) return null;
      const me = await r.json();
      const ids: number[] = (me?.items ?? []).map((o: any) => Number(o.id)).filter((n: number) => Number.isFinite(n));
      const candidates = want ? ids.filter((x) => x === want) : ids;
      for (const oid of candidates) {
        const s = await fetch(`/api/v1/orgs/${oid}/energy/station-list`, { credentials: 'include' });
        if (!s.ok) continue;
        const list = await s.json();
        if (want || (Array.isArray(list?.items) && list.items.length)) return oid;
      }
      return null;
    }, orgId ?? null);
  }, 3, page);
  if (id == null) return null;
  lastOrgId = Number(id);
  await page.evaluate((oid: number, key: string) => {
    localStorage.setItem('eo.selection', JSON.stringify({ organizationId: oid, projectId: null }));
    localStorage.setItem(key, String(oid));
    (window as any).__demoOrgId = oid;
  }, lastOrgId, ORG_KEY);
  await page.goto(`${APP}#/energyMyWorkConsole`, { waitUntil: 'networkidle2' });
  await page.reload({ waitUntil: 'networkidle2' });
  await waitIdle(page);
  log(`    org = ${id}`);
  return lastOrgId;
}

export const currentOrgId = () => lastOrgId;

export async function waitFor(page: Page, selector: string, ms = 30000) {
  const end = Date.now() + ms;
  while (Date.now() < end) {
    try {
      if (await page.evaluate((s) => !!document.querySelector(s), selector)) return true;
    } catch {
      // 輪詢期間 frame 可能正在換
    }
    await sleep(250);
  }
  return false;
}

/**
 * 導到某個 hash 路由並等它的內容出現。前三次 goto＋reload（要乾淨的 React state），
 * 第四次只改 hash（不重建 frame），最後純 goto。
 */
export async function goto(page: Page, hash: string, selector: string) {
  const LAST = 4;
  for (let attempt = 0; attempt <= LAST; attempt += 1) {
    try {
      if (attempt < 3) {
        await page.goto(`${APP}#/${hash}`, { waitUntil: 'networkidle2' });
        await page.reload({ waitUntil: 'networkidle2' });
      } else if (attempt === 3) {
        if (!page.url().startsWith(BASE)) await page.goto(APP, { waitUntil: 'networkidle2' });
        await page.evaluate((h: string) => {
          if (window.location.hash === h) window.location.hash = '#/';
          window.location.hash = h;
        }, `#/${hash}`);
        await waitIdle(page);
      } else {
        await page.goto(`${APP}#/${hash}`, { waitUntil: 'domcontentloaded' });
        await waitIdle(page);
      }
      break;
    } catch (e: any) {
      const msg = `${e?.name ?? ''} ${e?.message ?? e}`;
      const transient = /detached|Target closed|Session closed|Connection closed|Navigation|Execution context/i.test(msg);
      if (!transient || attempt === LAST) throw e;
      await sleep(1500 * (attempt + 1));
    }
  }
  const ok = await waitFor(page, selector);
  await sleep(300);
  return ok;
}

/**
 * 在頁面裡用目前的 session 打能源 API（`/api/v1/orgs/{org}/energy{path}`）。
 * `path` 以 `/` 開頭；`body` 是物件就送 JSON。回 `{ status, data }`，不丟例外（斷言自己看 status）。
 */
export async function eapi(page: Page, method: string, apiPath: string, body?: unknown): Promise<{ status: number; data: any }> {
  return page.evaluate(async (m: string, p: string, b: string | null) => {
    const org = (window as any).__demoOrgId;
    const r = await fetch(`/api/v1/orgs/${org}/energy${p}`, {
      method: m,
      credentials: 'include',
      headers: b != null ? { 'content-type': 'application/json' } : {},
      body: b ?? undefined,
    });
    const text = await r.text();
    let data: any = text;
    try { data = text ? JSON.parse(text) : null; } catch { /* 不是 JSON 就回原文 */ }
    return { status: r.status, data };
  }, method, apiPath, body === undefined ? null : JSON.stringify(body));
}

/** 在頁面裡打平台 API（`/api/v1{path}`）。 */
export async function papi(page: Page, method: string, apiPath: string, body?: unknown): Promise<{ status: number; data: any }> {
  return page.evaluate(async (m: string, p: string, b: string | null) => {
    const r = await fetch(`/api/v1${p}`, {
      method: m,
      credentials: 'include',
      headers: b != null ? { 'content-type': 'application/json' } : {},
      body: b ?? undefined,
    });
    const text = await r.text();
    let data: any = text;
    try { data = text ? JSON.parse(text) : null; } catch { /* 不是 JSON 就回原文 */ }
    return { status: r.status, data };
  }, method, apiPath, body === undefined ? null : JSON.stringify(body));
}

/**
 * 對 crawler 專用資料庫跑 SQL，回 psql `-At` 的輸出（欄以 `|` 分隔）。
 * 只連 `E2E_DB`（run-crawlers.sh 給 `eo_e2e`），不碰別的資料庫。
 */
export function sql(text: string): string {
  const db = process.env.E2E_DB || 'eo_e2e';
  if (!db.startsWith('eo_e2e')) throw new Error(`E2E_DB 必須以 eo_e2e 開頭：${db}`);
  return execFileSync('docker', ['compose', 'exec', '-T', 'postgres', 'psql', '-U', 'energy', '-d', db, '-At', '-v', 'ON_ERROR_STOP=1'], {
    cwd: REPO_ROOT,
    input: text,
    encoding: 'utf8',
  }).trim();
}

/**
 * 看得出是什麼的佔位 PDF（不是 1×1 PNG）：它會留在資料庫裡，使用者打開那張單看到的就是它。
 * 寫明「不是真的電費單」、檔名（假辨識結果靠檔名挑）與出處。base-14 Helvetica 只能放 ASCII。
 */
export function placeholderPdf(fileName: string): Buffer {
  const lines = [
    ['TEST PLACEHOLDER - NOT A REAL BILL', 20, 780],
    [`file: ${fileName.replace(/[^\x20-\x7e]/g, '?')}`, 12, 740],
    ['Generated by web/e2e/lib/energyOpsHarness.ts (seedBatch).', 10, 715],
    ['The recognition values on this bill come from a fixture', 10, 698],
    ['(mockOcrForUpload), NOT from this page.', 10, 683],
    ['Real-document recognition is verified by', 10, 655],
    ['web/e2e/crawlers/recognition.ts with the four sample PDFs.', 10, 640],
  ] as Array<[string, number, number]>;
  const esc = (t: string) => t.replace(/\\/g, '\\\\').replace(/\(/g, '\\(').replace(/\)/g, '\\)');
  const content = [
    '1 w 0.6 0.6 0.6 RG 40 40 515 762 re S',
    ...lines.map(([t, size, y]) => `BT /F1 ${size} Tf 60 ${y} Td (${esc(t)}) Tj ET`),
  ].join('\n');
  const objs = [
    '<< /Type /Catalog /Pages 2 0 R >>',
    '<< /Type /Pages /Kids [3 0 R] /Count 1 >>',
    '<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] '
      + '/Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>',
    `<< /Length ${content.length} >>\nstream\n${content}\nendstream`,
    '<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>',
  ];
  // xref 的位移必須是實際 byte 位置，算錯 pdf.js 會拒收
  let pdf = '%PDF-1.4\n';
  const offsets: number[] = [];
  objs.forEach((objBody, i) => {
    offsets.push(Buffer.byteLength(pdf, 'binary'));
    pdf += `${i + 1} 0 obj\n${objBody}\nendobj\n`;
  });
  const xrefAt = Buffer.byteLength(pdf, 'binary');
  pdf += `xref\n0 ${objs.length + 1}\n0000000000 65535 f \n`;
  offsets.forEach((o) => { pdf += `${String(o).padStart(10, '0')} 00000 n \n`; });
  pdf += `trailer\n<< /Size ${objs.length + 1} /Root 1 0 R >>\nstartxref\n${xrefAt}\n%%EOF\n`;
  return Buffer.from(pdf, 'binary');
}

/** 1x1 PNG。 */
export const PNG_1x1 = Buffer.from(
  'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==',
  'base64',
);

/**
 * 從收件台開一批新送件並停在它的詳情頁：丟檔進 dropzone，等路由換到 `energyIngestConsole/<id>`。
 * 多個路徑＝同一批多份檔。
 */
export async function uploadNewBatch(page: Page, ...files: string[]): Promise<{ ok: boolean; batchId: number }> {
  await goto(page, 'energyIngestConsole', '[data-testid="intake-dropzone"]');
  const input = await page.$('[data-testid="intake-file-input"]');
  if (!input) return { ok: false, batchId: 0 };
  await (input as any).uploadFile(...files);

  const settle = async <T>(fn: () => Promise<T>, fallback: T): Promise<T> => {
    try { return await fn(); } catch (e: any) {
      const msg = `${e?.name ?? ''} ${e?.message ?? e}`;
      if (!/detached|Execution context|Navigation/i.test(msg)) throw e;
      return fallback;
    }
  };

  let batchId = 0;
  for (let i = 0; i < 60; i += 1) {
    const hash = await settle(() => page.evaluate(() => window.location.hash), '');
    const m = /#\/energyIngestConsole\/(\d+)/.exec(hash);
    if (m) { batchId = Number(m[1]); break; }
    await sleep(500);
  }
  if (!batchId) return { ok: false, batchId: 0 };
  for (let i = 0; i < 60; i += 1) {
    const n = await settle(() => page.$$eval('[data-testid="intake-item"]', (e) => e.length), 0);
    if (n >= 1) return { ok: true, batchId };
    await sleep(500);
  }
  return { ok: false, batchId };
}

/**
 * 造一批送件：真的開批、真的上傳佔位 PDF、辨識結果由我們給（`POST E/ingest-items/{id}/recognition`）。
 * 每張單都有原檔，只有「辨識結果從哪來」不同。要對得上的那份電號用這個組織第一座運轉中案場的。
 * 同檔名＝同一張紙。
 */
export async function seedBatch(
  page: Page,
  names: string[],
  opts: { customerNumber?: string; contractNo?: string } = {},
): Promise<{ ok: boolean; batchId: number }> {
  const customerNumber = opts.customerNumber ?? await (async () => {
    const r = await eapi(page, 'GET', '/stations');
    const rows: any[] = r.status === 200 ? (r.data?.items ?? []) : [];
    const operating = rows.filter((x: any) => x.status === 'operating')
      .sort((a: any, b: any) => Number(a.id) - Number(b.id));
    return operating.length ? String(operating[0].customerNumber ?? '') : '';
  })().catch(() => '');

  const files = names.map((name) => {
    // 副檔名換成 .pdf（內容是佔位 PDF）；stem 不動，fixture 與斷言都看 stem。
    const originalFilename = name.replace(/\.(png|jpe?g)$/i, '.pdf');
    return {
      originalFilename,
      pdfBase64: placeholderPdf(originalFilename).toString('base64'),
      ocrRaw: mockOcrForUpload(name, {
        ...(customerNumber ? { customerNumber } : {}),
        ...(opts.contractNo ? { contractNo: opts.contractNo } : {}),
      }).ocrRaw,
    };
  });

  const batchId = await page.evaluate(async (payload: any) => {
    const org = (window as any).__demoOrgId;
    const E = `/api/v1/orgs/${org}/energy`;
    const post = (url: string, body: any) => fetch(url, {
      method: 'POST',
      credentials: 'include',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify(body),
    });
    const created = await post(`${E}/ingest-batches`, { fileCount: payload.length });
    if (!created.ok) return 0;
    const batch = await created.json();
    const bid = Number(batch?.id ?? 0);
    if (!bid) return 0;
    for (const f of payload) {
      const bytes = Uint8Array.from(atob(f.pdfBase64), (c) => c.charCodeAt(0));
      const fd = new FormData();
      fd.append('file', new Blob([bytes], { type: 'application/pdf' }), f.originalFilename);
      const up = await fetch(`${E}/ingest-batches/${bid}/attachments`, { method: 'POST', credentials: 'include', body: fd });
      if (!up.ok) return 0;
      const uploaded = await up.json();
      const itemId = Number(uploaded?.itemId ?? 0);
      if (!itemId) return 0;
      const applied = await post(`${E}/ingest-items/${itemId}/recognition`, { ocrRaw: f.ocrRaw });
      if (!applied.ok) return 0;
    }
    return bid;
  }, files as any);

  if (!batchId) return { ok: false, batchId: 0 };
  const ok = await goto(page, `energyIngestConsole/${batchId}`, '[data-testid="intake-item"]');
  return { ok, batchId };
}

/** 寫一組上傳用的佔位 PDF，回絕對路徑。同檔名＝同一張紙。 */
export function writeUploadFiles(dir: string, names: string[]): string[] {
  const base = path.join(REPO_ROOT, 'web/e2e/.tmp', dir);
  fs.mkdirSync(base, { recursive: true });
  return names.map((n) => {
    const name = n.replace(/\.(png|jpe?g)$/i, '.pdf');
    const p = path.join(base, name);
    fs.writeFileSync(p, placeholderPdf(name));
    return p;
  });
}
