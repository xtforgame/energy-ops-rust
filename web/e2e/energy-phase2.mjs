// 第二階段驗收：能源頁對新後端（/api/v1/orgs/{orgId}/energy）。
// 流程：API 建組織與兩個 energy:admin → 畫面上按「初始化能源設定」→ 部門頁把甲加進業務部 →
// SQL 灌主檔與一張電費單 → API 綁流程、由乙開單 → 逐頁打開（記未捕捉錯誤與失敗的 API）→
// 我的工作點開待辦 → 認領 → 核准，流程推進到下一關。截圖存 e2e/screenshots/phase2/。
//
// 收件那一組（附件／bills／ingest／recognition／uploads）後端另一條分支在做：第一輪照實記下 404，
// 走流程那一段才用 App 5 形狀的假資料攔下 `GET E/bills/{id}`（抽屜要靠它畫出來）。
//
// 用法：先起後端與 vite，再 `E2E_BASE_URL=http://localhost:5182 node e2e/energy-phase2.mjs`。
// 主檔沒有建立 API，用 `docker exec <E2E_PG_CONTAINER> psql` 灌（預設 energy-ops-rust-postgres-1）。
import puppeteer from 'puppeteer';
import { execFileSync } from 'node:child_process';
import { mkdirSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const BASE = process.env.E2E_BASE_URL || 'http://localhost:5173';
const ADMIN = process.env.E2E_ADMIN_USER || 'admin@example.com';
const PASSWORD = process.env.E2E_ADMIN_PASSWORD || 'admin12345';
const PG = process.env.E2E_PG_CONTAINER || 'energy-ops-rust-postgres-1';
const OUT = join(dirname(fileURLToPath(import.meta.url)), 'screenshots', 'phase2');
mkdirSync(OUT, { recursive: true });

const stamp = Date.now().toString(36);
const INTAKE = /\/energy\/(bills|ingest-batches|ingest-items|recognition|uploads|attachments|attachment-links|executions\/\d+\/attachments)(\/|\?|$)/;

async function api(method, path, body, cookie) {
  const res = await fetch(`${BASE}${path}`, {
    method,
    headers: { 'content-type': 'application/json', ...(cookie ? { cookie } : {}) },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const text = await res.text();
  const data = text ? JSON.parse(text) : undefined;
  if (!res.ok) throw new Error(`${method} ${path} → ${res.status} ${text}`);
  return { data, res };
}

async function login(user, password) {
  const r = await api('POST', '/api/v1/auth/login', { username: user, password });
  return r.res.headers.get('set-cookie').split(';')[0];
}

function sql(text) {
  return execFileSync('docker', ['exec', '-i', PG, 'sh', '-c', 'psql -U "$POSTGRES_USER" -d energy_ops -At -v ON_ERROR_STOP=1'], {
    input: text,
    encoding: 'utf8',
  }).trim();
}

const settle = (ms = 2600) => new Promise((r) => setTimeout(r, ms));

function assert(cond, msg) {
  if (!cond) throw new Error(`assertion failed: ${msg}`);
  console.log(`  ok  ${msg}`);
}

// ── 準備資料（API）──
const cookie = await login(ADMIN, PASSWORD);
const { data: org } = await api('POST', '/api/v1/orgs', { uniqueName: `e2e2-${stamp}`, name: `能源二階 ${stamp}` }, cookie);
const O = org.id;
const E = `/api/v1/orgs/${O}/energy`;
const { data: me } = await api('GET', '/api/v1/auth/session', undefined, cookie);
await api('POST', `/api/v1/orgs/${O}/rbac/grants`, { userId: me.user.id, roleUniqueName: 'energy:admin' }, cookie);
const approverEmail = `approver-${stamp}@example.com`;
const { data: approver } = await api('POST', `/api/v1/orgs/${O}/members`, { email: approverEmail, name: `開單員 ${stamp}`, password: 'member12345' }, cookie);
await api('POST', `/api/v1/orgs/${O}/rbac/grants`, { userId: approver.id, roleUniqueName: 'energy:admin' }, cookie);
const { data: orgs } = await api('GET', '/api/v1/me/organizations', undefined, cookie);
const lobby = orgs.items.find((o) => o.id === O).projects.find((p) => p.isDefault);
console.log(`org #${O}, approver #${approver.id}`);

const browser = await puppeteer.launch({ headless: true, args: ['--no-sandbox'] });
const errors = [];
const failed = []; // { status, method, url, phase }
let phase = 'setup';
let execId;
try {
  const page = await browser.newPage();
  await page.setViewport({ width: 1440, height: 900 });
  page.on('pageerror', (e) => errors.push(`[${phase}] ${e}`));
  page.on('response', (res) => {
    const url = res.url();
    if (!url.includes('/api/') || res.status() < 400) return;
    failed.push({ status: res.status(), method: res.request().method(), url: url.replace(BASE, ''), phase });
  });

  await page.goto(`${BASE}/#/login`, { waitUntil: 'networkidle0' });
  await page.waitForSelector('input[name="email"]');
  await page.type('input[name="email"]', ADMIN);
  await page.type('input[name="password"]', PASSWORD);
  await page.click('button[type="submit"]');
  await page.waitForSelector('[data-testid="org-selector"]', { timeout: 20000 });
  await page.click('[data-testid="org-selector"]');
  await page.waitForSelector(`[data-testid="org-option-${O}-${lobby.id}"]`);
  await page.click(`[data-testid="org-option-${O}-${lobby.id}"]`);
  await page.waitForFunction((name) => document.querySelector('[data-testid="org-selector"]')?.value?.startsWith(name), {}, org.name);
  assert(true, `selected org ${org.name}`);

  // ── 流程總覽空狀態 → 初始化能源設定 ──
  phase = 'install';
  await page.goto(`${BASE}/#/energyWorkflowConsole`, { waitUntil: 'networkidle0' });
  await page.waitForSelector('[data-testid="install-defaults"]', { timeout: 20000 });
  await settle();
  await page.screenshot({ path: join(OUT, '01-workflows-empty-install.png') });
  await page.click('[data-testid="install-defaults"]');
  await page.waitForSelector('[data-testid="workflow-row"]', { timeout: 20000 });
  const defCount = await page.$$eval('[data-testid="workflow-row"]', (n) => n.length);
  assert(defCount === 4, `install-defaults installed the 4 built-in workflows (${defCount} rows)`);
  await settle(1200);
  await page.screenshot({ path: join(OUT, '02-workflows-installed.png') });

  // ── 部門頁：把甲加進業務部（PUT E/departments/{id}/members/{userId}）──
  // 候選池在關卡 ready 的當下快照，所以要在開單之前加。
  phase = 'departments';
  await page.goto(`${BASE}/#/departments`, { waitUntil: 'networkidle0' });
  await page.waitForSelector('[data-testid="department-row"][data-key="sales"]', { timeout: 20000 });
  await page.click('[data-testid="department-row"][data-key="sales"] [data-testid="department-assign"]');
  await page.waitForSelector('[data-testid="department-assign-user"]');
  await page.click('[data-testid="department-assign-user"] [role="combobox"]');
  await page.waitForSelector(`[data-testid="member-option-${me.user.id}"]`);
  const pickerSize = await page.$$eval('[data-testid^="member-option-"]', (n) => n.length);
  assert(pickerSize === 2, `member picker lists both org members from E/members (${pickerSize})`);
  await page.screenshot({ path: join(OUT, '03-department-assign-picker.png') });
  await page.click(`[data-testid="member-option-${me.user.id}"]`);
  await settle(400);
  await page.click('[data-testid="department-assign-submit"]');
  await page.waitForSelector(`[data-testid="department-row"][data-key="sales"] [data-testid="department-member"][data-user="${me.user.id}"]`, { timeout: 10000 });
  assert(true, 'admin assigned to 業務部 through the dialog');
  await settle(1200);
  await page.screenshot({ path: join(OUT, '04-department-assigned.png') });

  // ── 主檔、綁定、開單（SQL＋API）──
  phase = 'seed';
  const ids = sql(`
    with l1 as (insert into tbl_energy_landowner (name, tax_id, contact_name, contact_phone, organization_id)
                values ('王大明 ${stamp}', '12345678', '王大明', '0912-000-111', ${O}) returning id),
         l2 as (insert into tbl_energy_landowner (name, tax_id, contact_name, contact_phone, organization_id)
                values ('陳美華 ${stamp}', '87654321', '陳小姐', '0922-333-444', ${O}) returning id),
         s1 as (insert into tbl_energy_station (customer_number, customer_number_key, name, address, latitude, longitude, organization_id, landowner_id)
                select '07-${stamp}-1', '07${stamp}1', '屏東一號案場', '屏東縣屏東市自由路 1 號', 22.6727, 120.4880, ${O}, id from l1 returning id),
         s2 as (insert into tbl_energy_station (customer_number, customer_number_key, name, address, organization_id, landowner_id)
                select '06-${stamp}-2', '06${stamp}2', '台南二號案場', '台南市安平區安平路 2 號', ${O}, id from l2 returning id)
    select (select id from s1) || ',' || (select id from s2) || ',' || (select id from l1);
  `).split('\n').pop();
  const [stationId, , landownerId] = ids.split(',').map(Number);
  const billId = Number(sql(`
    insert into tbl_energy_bill (customer_number_raw, doc_type, billing_period_start, billing_period_end, total_amount,
      generation_kwh, issued_at, due_date, review_status, organization_id, station_id)
    values ('07-${stamp}-1', 'bill', '2026-09-01', '2026-10-31', 4321.50, 12000.5, '2026-11-01', '2026-11-20', 'reviewed', ${O}, ${stationId})
    returning id;
  `).split('\n')[0]);
  sql(`insert into tbl_energy_calendar_event (event_date, kind, title, organization_id, landowner_id, station_id)
       values (current_date + 3, 'payout', '屏東一號案場 付款', ${O}, ${landownerId}, ${stationId});`);
  const { data: opts } = await api('GET', `${E}/binding-options?kind=workflow`, undefined, cookie);
  const billFlow = opts.items.find((o) => o.key === 'bill-approval');
  await api('POST', `${E}/stations/${stationId}/bindings`, { kind: 'workflow', versionId: billFlow.id, validFrom: '2026-01-01', docType: null }, cookie);
  const { data: rate } = await api('GET', `${E}/binding-options?kind=rating`, undefined, cookie);
  await api('POST', `${E}/stations/${stationId}/bindings`, { kind: 'rating', versionId: rate.items[0].id, validFrom: '2026-01-01', docType: null }, cookie);
  const approverCookie = await login(approverEmail, 'member12345');
  const { data: exec } = await api('POST', `${E}/executions`, {
    stationKey: `07-${stamp}-1`, workflowKey: 'bill-approval', subjectType: 'energyBill', subjectId: billId,
    businessDate: '2026-11-01', attributes: { totalAmount: 4321.5 },
  }, approverCookie);
  execId = exec.id;
  assert(exec.currentNodeKey === 'clerk-review', `approver started execution #${execId} at clerk-review (bill #${billId}, station #${stationId})`);

  // ── 逐頁：不得有未捕捉錯誤；核心 API 不得失敗 ──
  phase = 'pages';
  const pages = [
    ['05-my-work', 'energyMyWorkConsole'],
    ['06-ingest', 'energyIngestConsole'],
    ['07-bills', 'energyBillConsole'],
    ['08-executions', 'energyExecutionConsole'],
    ['09-execution-detail', `energyExecutionConsole/${execId}/show`],
    ['10-workflows', 'energyWorkflowConsole'],
    ['11-landowners', 'energyLandownerConsole'],
    ['12-landowner-detail', `energyLandowners/${landownerId}`],
    ['13-station-detail', `energyStationConsole/${stationId}/show`],
    ['14-dashboard', 'energyDashboardConsole'],
    ['15-calendar', 'energyCalendarConsole'],
    ['16-audit', 'energyAuditConsole'],
    ['17-departments', 'departments'],
  ];
  for (const [shot, route] of pages) {
    const before = errors.length;
    phase = `page:${route}`;
    await page.goto(`${BASE}/#/${route}`, { waitUntil: 'networkidle0' });
    await settle();
    const blank = await page.evaluate(() => (document.querySelector('#main-content')?.textContent || '').trim().length === 0);
    await page.screenshot({ path: join(OUT, `${shot}.png`) });
    if (errors.length > before || blank) throw new Error(`route ${route} failed: ${blank ? 'blank page' : errors.slice(before).join(' | ')}`);
  }
  assert(true, `${pages.length} energy pages render without uncaught errors`);
  const execRows = await (async () => {
    await page.goto(`${BASE}/#/energyExecutionConsole`, { waitUntil: 'networkidle0' });
    await settle(1500);
    return page.evaluate(() => document.body.innerText.includes('電費單核可流程'));
  })();
  assert(execRows, 'executions list shows the running bill-approval execution');

  // ── 我的工作 → 待辦 → 認領 → 核准 ──
  // 電費單本體（收件）尚未上線：只為這一段攔 GET E/bills/{id}，回 App 5 形狀的假單。
  phase = 'workflow';
  await page.setRequestInterception(true);
  const mockIntake = (req) => {
    if (req.isInterceptResolutionHandled()) return;
    const u = new URL(req.url());
    const mockBill = new RegExp(`^/api/v1/orgs/${O}/energy/bills/${billId}$`);
    if (req.method() === 'GET' && mockBill.test(u.pathname)) {
      req.respond({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({
          id: billId, stationId, customerNumberRaw: `07-${stamp}-1`, sourceAttachmentId: null, ingest: null,
          ocrRaw: {}, ocrExtracted: {}, changedFields: [], reviewStatus: 'reviewed',
          station: { id: stationId, name: '屏東一號案場', customerNumber: `07-${stamp}-1` },
          executions: [{ id: execId, status: 'running', workflowKey: 'bill-approval' }],
          templateId: null, docType: 'bill', recognizedDocType: 'bill', docTypeEditable: false,
          binding: { bound: true, stationName: '屏東一號案場', customerNumber: `07-${stamp}-1`, flows: [{ workflowKey: 'bill-approval', workflowName: '電費單核可流程', version: 1, contractNo: null }] },
          businessKey: null, documentNo: null, mode: 'act', modeReason: null, canEditSubject: false, canAddAttachment: true,
          billingPeriodStart: '2026-09-01', billingPeriodEnd: '2026-10-31', totalAmount: '4321.50', generationKwh: '12000.500',
          issuedAt: '2026-11-01', dueDate: '2026-11-20',
        }),
      });
      return;
    }
    req.continue();
  };
  page.on('request', mockIntake);
  await page.goto(`${BASE}/#/energyMyWorkConsole`, { waitUntil: 'networkidle0' });
  const rowSel = `[data-testid="workbench-row"][data-execution="${execId}"]`;
  await page.waitForSelector(rowSel, { timeout: 20000 });
  assert(true, `my work lists the clerk-review task of execution #${execId}`);
  await settle(1200);
  await page.screenshot({ path: join(OUT, '18-mywork-task.png') });
  await page.click(rowSel);
  await page.waitForSelector('[data-testid="queue-claim"]', { timeout: 20000 });
  await settle(1500);
  await page.screenshot({ path: join(OUT, '19-drawer-claim.png') });
  await page.click('[data-testid="queue-claim"]');
  await page.waitForSelector('[data-testid="queue-act-close"], [data-testid="queue-act-next"]', { timeout: 20000 });
  await settle(1200);
  await page.screenshot({ path: join(OUT, '20-drawer-claimed-actions.png') });
  const actionKey = await page.$eval('[data-testid="queue-act-close"], [data-testid="queue-act-next"]', (n) => n.getAttribute('data-action'));
  await page.click('[data-testid="queue-act-close"], [data-testid="queue-act-next"]');
  await page.waitForFunction(async (url) => {
    const r = await fetch(url, { credentials: 'include' });
    const d = await r.json();
    return d.currentNodeKey && d.currentNodeKey !== 'clerk-review';
  }, { timeout: 20000, polling: 500 }, `${E}/executions/${execId}`);
  const { data: after } = await api('GET', `${E}/executions/${execId}`, undefined, cookie);
  assert(after.currentNodeKey === 'manager-approve', `"${actionKey}" through the UI moved execution #${execId} to ${after.currentNodeKey}`);
  await settle(1500);
  await page.screenshot({ path: join(OUT, '21-after-approve.png') });
  page.off('request', mockIntake);
  await page.setRequestInterception(false);
  await page.goto(`${BASE}/#/energyExecutionConsole/${execId}/show`, { waitUntil: 'networkidle0' });
  await settle();
  await page.screenshot({ path: join(OUT, '22-execution-detail-after.png') });

  // ── 401：session 失效時能源呼叫把人送回登入頁 ──
  phase = '401';
  const client = await page.createCDPSession();
  await client.send('Network.clearBrowserCookies');
  await page.goto(`${BASE}/#/energyDashboardConsole`, { waitUntil: 'networkidle0' });
  await page.waitForSelector('input[name="email"]', { timeout: 20000 });
  assert(true, '401 from the session lands on the login page');
  await page.screenshot({ path: join(OUT, '23-401-login.png') });
} finally {
  await browser.close();
}

const core = failed.filter((f) => !INTAKE.test(f.url) && f.phase !== '401');
const intake = failed.filter((f) => INTAKE.test(f.url));
const uniq = (list) => [...new Set(list.map((f) => `${f.status} ${f.method} ${f.url.replace(/\?.*$/, '').replace(/\/\d+(?=\/|$)/g, '/{id}')}`))];
const report = { org: O, execution: execId, errors, coreFailures: uniq(core), intakePending: uniq(intake) };
writeFileSync(join(OUT, 'report.json'), JSON.stringify(report, null, 2));
console.log('\nintake (pending backend):');
for (const l of report.intakePending) console.log(`  ${l}`);
console.log('core failures:');
for (const l of report.coreFailures) console.log(`  ${l}`);
assert(errors.length === 0, `no uncaught page errors (${errors.join(' | ')})`);
assert(core.length === 0, 'no failing core energy API calls');
console.log(`\nall good — screenshots in ${OUT}`);
