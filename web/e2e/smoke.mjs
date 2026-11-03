// 對真後端的冒煙測試：經 Vite dev server 登入 bootstrap admin，用 API 建組織與成員，
// 在畫面上用切換器切過去、打開成員頁與角色頁，存截圖到 e2e/screenshots/。
// 用法：先起後端與 `npm run dev`，再 `npm run e2e`（E2E_BASE_URL 預設 http://localhost:5173）。
import puppeteer from 'puppeteer';
import { mkdirSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const BASE = process.env.E2E_BASE_URL || 'http://localhost:5173';
const ADMIN = process.env.E2E_ADMIN_USER || 'admin@example.com';
const PASSWORD = process.env.E2E_ADMIN_PASSWORD || 'admin12345';
const OUT = join(dirname(fileURLToPath(import.meta.url)), 'screenshots');
mkdirSync(OUT, { recursive: true });

const stamp = Date.now().toString(36);

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

// 切頁的雲霧轉場要等它掃完再截圖
const settle = (ms = 2600) => new Promise((r) => setTimeout(r, ms));

function assert(cond, msg) {
  if (!cond) throw new Error(`assertion failed: ${msg}`);
  console.log(`  ok  ${msg}`);
}

// ── 準備資料（API）──
const login = await api('POST', '/api/v1/auth/login', { username: ADMIN, password: PASSWORD });
const cookie = login.res.headers.get('set-cookie').split(';')[0];
assert(cookie.startsWith('eo_session='), 'login through the Vite proxy sets the eo_session cookie');
const { data: org } = await api('POST', '/api/v1/orgs', { uniqueName: `e2e-${stamp}`, name: `E2E 能源 ${stamp}` }, cookie);
const memberEmail = `member-${stamp}@example.com`;
await api('POST', `/api/v1/orgs/${org.id}/members`, { email: memberEmail, name: `E2E 成員 ${stamp}`, password: 'member12345' }, cookie);
const { data: orgs } = await api('GET', '/api/v1/me/organizations', undefined, cookie);
const mine = orgs.items.find((o) => o.id === org.id);
const lobby = mine.projects.find((p) => p.isDefault);
assert(!!lobby, `org #${org.id} has a lobby project (#${lobby?.id})`);
assert(orgs.items[orgs.items.length - 1].kind === 'personal', 'personal org is listed last');

// ── 畫面 ──
const browser = await puppeteer.launch({ headless: true, args: ['--no-sandbox'] });
const errors = [];
try {
  const page = await browser.newPage();
  await page.setViewport({ width: 1400, height: 860 });
  page.on('pageerror', (e) => errors.push(String(e)));

  await page.goto(`${BASE}/#/login`, { waitUntil: 'networkidle0' });
  await page.waitForSelector('input[name="email"]');
  await page.type('input[name="email"]', ADMIN);
  await page.type('input[name="password"]', PASSWORD);
  await Promise.all([page.click('button[type="submit"]')]);
  await page.waitForSelector('[data-testid="home-greeting"]', { timeout: 20000 });
  await page.waitForSelector('[data-testid="org-selector"]');
  const greeting = await page.$eval('[data-testid="home-greeting"]', (n) => n.textContent);
  assert(greeting.includes('Administrator'), `home shows the signed-in user (${greeting})`);
  await settle();
  await page.screenshot({ path: join(OUT, '01-home-after-login.png') });

  // 切換器：打開下拉、選剛建的組織
  await page.click('[data-testid="org-selector"]');
  await page.waitForSelector(`[data-testid="org-option-${org.id}-${lobby.id}"]`);
  await page.screenshot({ path: join(OUT, '02-org-selector-open.png') });
  await page.click(`[data-testid="org-option-${org.id}-${lobby.id}"]`);
  await page.waitForFunction(
    (name) => document.querySelector('[data-testid="org-selector"]')?.value?.startsWith(name),
    {},
    org.name,
  );
  const stored = await page.evaluate(() => localStorage.getItem('eo.selection'));
  assert(JSON.parse(stored).organizationId === org.id, `selection persisted to localStorage (${stored})`);

  // 成員頁
  await page.goto(`${BASE}/#/organizationMembers`, { waitUntil: 'networkidle0' });
  await page.waitForSelector('[data-testid="members-table"]', { timeout: 20000 });
  await page.waitForFunction((email) => document.body.innerText.includes(email), {}, memberEmail);
  const rows = await page.$$eval('[data-testid^="member-row-"]', (ns) => ns.length);
  assert(rows === 2, `members page lists the creator and the new member (${rows} rows)`);
  const hasAdminChip = await page.evaluate(() => document.body.innerText.includes('組織管理員'));
  assert(hasAdminChip, 'creator shows the org-admin role chip');
  await settle();
  await page.screenshot({ path: join(OUT, '03-organization-members.png') });

  // 重新整理後仍在同一個組織（localStorage）
  await page.reload({ waitUntil: 'networkidle0' });
  await page.waitForSelector('[data-testid="members-table"]', { timeout: 20000 });
  const after = await page.$eval('[data-testid="org-selector"]', (n) => n.value);
  assert(after.startsWith(org.name), `selection survives a reload (${after})`);

  // 角色頁：permission chips
  await page.goto(`${BASE}/#/rbacRoles`, { waitUntil: 'networkidle0' });
  await page.waitForSelector('[data-testid="role-row-platform:org-admin"]', { timeout: 20000 });
  assert(true, 'RBAC roles page lists platform:org-admin');
  await settle();
  await page.screenshot({ path: join(OUT, '04-rbac-roles.png') });

  // ?org= 參數：直接用網址切到個人組織
  const personal = orgs.items.find((o) => o.kind === 'personal');
  await page.goto(`${BASE}/#/?org=${personal.id}`, { waitUntil: 'networkidle0' });
  await page.waitForFunction(
    (name) => document.querySelector('[data-testid="org-selector"]')?.value?.startsWith(name),
    { timeout: 10000 },
    personal.name,
  );
  assert(true, '?org= in the URL switches the selector');

  // 能源頁（第一階段 API 尚未對到新後端）：每頁都要能渲染、不得有未捕捉的錯誤
  const energyRoutes = [
    'energyMyWorkConsole', 'energyIngestConsole', 'energyBillConsole', 'energyExecutionConsole',
    'energyLandownerConsole', 'energyCalendarConsole', 'energyDashboardConsole', 'energyWorkflowConsole',
    'energyAuditConsole', 'departments', 'projects', 'organizationInvitations', 'rbacPermissions', 'rbacGrants',
    'adminUsers', 'adminRbac', 'organizationSettings', 'profile', 'my-invitations',
  ];
  for (const r of energyRoutes) {
    const before = errors.length;
    await page.goto(`${BASE}/#/${r}`, { waitUntil: 'networkidle0' });
    await settle(400);
    const blank = await page.evaluate(() => (document.querySelector('#main-content')?.textContent || '').trim().length === 0);
    if (errors.length > before || blank) throw new Error(`route ${r} failed: ${blank ? 'blank page' : errors.slice(before).join(' | ')}`);
  }
  assert(true, `${energyRoutes.length} other routes render without uncaught errors`);

  // 公開的接受邀請頁：未註冊的人經連結註冊並接受
  const inviteeEmail = `invitee-${stamp}@example.com`;
  const { data: inv } = await api('POST', `/api/v1/orgs/${org.id}/invitations`, { email: inviteeEmail, inviteRoleKey: 'org_member' }, cookie);
  assert(typeof inv.token === 'string', 'creating an invitation returns its token once');
  const guestCtx = await browser.createBrowserContext();
  const guest = await guestCtx.newPage();
  await guest.setViewport({ width: 1400, height: 860 });
  guest.on('pageerror', (e) => errors.push(String(e)));
  await guest.goto(`${BASE}/#/accept-invitation?token=${encodeURIComponent(inv.token)}`, { waitUntil: 'networkidle0' });
  await guest.waitForSelector('[data-testid="accept-summary"]', { timeout: 20000 });
  await guest.type('[data-testid="accept-register-name"]', `受邀者 ${stamp}`);
  await guest.type('[data-testid="accept-register-password"]', 'invitee12345');
  await guest.type('[data-testid="accept-register-password2"]', 'invitee12345');
  await guest.screenshot({ path: join(OUT, '05-accept-invitation-register.png') });
  await guest.click('[data-testid="accept-register-submit"]');
  await guest.waitForSelector('[data-testid="home-greeting"]', { timeout: 20000 });
  await guest.waitForFunction(
    (name) => document.querySelector('[data-testid="org-selector"]')?.value?.startsWith(name),
    { timeout: 10000 },
    org.name,
  );
  assert(true, 'invitee registered, accepted and landed in the inviting org');
  // 一般成員沒有平台管理權限：選單看不到成員／角色
  await guest.waitForSelector('[data-testid="menu-my-invitations"]');
  const menu = await guest.evaluate(() => [...document.querySelectorAll('a[data-testid^="menu-"]')].map((a) => a.getAttribute('data-testid')));
  assert(!menu.includes('menu-organizationMembers') && !menu.includes('menu-rbacRoles'), `plain member menu is gated (${menu.join(',')})`);
  await guestCtx.close();

  assert(errors.length === 0, `no uncaught page errors (${errors.join(' | ')})`);
  console.log(`\nall good — screenshots in ${OUT}`);
} finally {
  await browser.close();
}
