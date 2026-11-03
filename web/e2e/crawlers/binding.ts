/* eslint-disable no-console, no-await-in-loop */
/**
 * **效期綁定管理**（C7e）—— 從 `crawler-energy-ops-roles` 的 ⑮ 拆出來。
 *
 * ### 為什麼要拆
 *
 * 原本這一段住在一支 12 分鐘的全套回歸裡，而那支跑到 101/131 就被
 * 瀏覽器崩掉帶走了兩次（`TargetCloseError`，0 failed）——
 * **「沒跑到」跟「跑過而且綠」在報表上長得一模一樣**。
 * 一段從來沒被驗證過的程式碼，掛在一個看起來全綠的數字底下。
 *
 * 拆完之後這一段自己有出口碼、自己有截圖，綠不綠一眼看得到。
 *
 * ### 驗的是什麼
 *
 * 綁定決定**這個案場收不收得了單、錢怎麼算**。它有三個動作：
 *
 * | 動作 | 為什麼要單獨驗 |
 * |---|---|
 * | 建立 | 漏綁不會報錯，只會讓案場安靜地收不了單 —— P9 的「尚未綁定」就是為此存在 |
 * | 換版（回溯） | 對話框要印**兩行**後果：N 張未送審會改走新版、M 筆執行不受影響。少了第二行沒人敢按 |
 * | 解除 | 之後這個案場**收不了單** —— 稽核必須是 `critical`，理由必填 |
 *
 * 外加一條權限：`energy:govern:system` 以外的人（**含 CXO**）
 * 連那三顆按鈕都不該看得到。
 *
 * ### 這一支怎麼跑
 *
 * 移植自 az `scripts/crawler-energy-ops-binding`：`web/e2e/run-crawlers.sh binding`
 * （示範資料、不灌範本電費單；每次都重建資料庫）。
 *
 * **要重灌 seed 才能重跑**：這一支會把台南七股一號綁起來、
 * 又把屏東枋寮一號的流程綁定解除掉 —— 兩者都是**不可逆的資料變更**。
 * 斷言本身是相對值（「軸上多一段」「從清單裡消失」），
 * 但起點狀態（台南未綁、屏東有兩段）是 seed 給的。
 */
import type { Page } from 'puppeteer';
import {
  Scoreboard, hr, log, sleep, launch, shooter, loginAs, loginAsAdmin, selectOrg, goto, hardenPage,
  waitIdle,
  eapi, demoEmail,
} from '../lib/energyOpsHarness.ts';

const board = new Scoreboard();
const check = board.check.bind(board);
const shot = shooter('energy-binding');

/** 開 MUI Select 一定要**真滑鼠** —— 頁內 `element.click()` 開不了選單。 */
async function pickVersion(page: Page): Promise<boolean> {
  const sel = await page.$('[data-testid="binding-version"]');
  if (!sel) return false;
  await page.click('[data-testid="binding-version"]');
  await sleep(900);
  const opt = await page.$('li[data-testid="binding-version-option"]');
  if (!opt) return false;
  await opt.click();
  await sleep(700);
  return true;
}

/** 填對話框的日期（`type="date"` 要送 input ＋ change）。 */
async function setBindingDate(page: Page, value: string) {
  await page.evaluate((v: string) => {
    const el = document.querySelector('[data-testid="binding-date"]') as HTMLInputElement | null;
    if (!el) return;
    const setter = Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, 'value')!.set!;
    setter.call(el, v);
    el.dispatchEvent(new Event('input', { bubbles: true }));
    el.dispatchEvent(new Event('change', { bubbles: true }));
  }, value);
  await waitIdle(page);
}

async function run() {
  const browser = await launch();
  // `hardenPage`：把 `page.evaluate` 包上 detach 復原（見 harness 裡的說明）。
  const page = hardenPage(await browser.newPage());
  const serverErrors: string[] = [];
  page.on('response', (r) => {
    if (r.status() >= 500) serverErrors.push(`${r.status()} ${r.url()}`);
  });

  try {
    // ── ⑮ 效期綁定管理（C7e）—— 使用者實測撞到的缺口 ─────────
    //
    // 用 **admin** 跑：綁定要 `energy:workflow:manage` / `energy:rating:manage`，
    // 兩者都在 `energy:govern:system` 內 —— **CXO 也不行**。
    hr('① P9 列得出「尚未綁定的案場」（admin）');
    await loginAsAdmin(page);
    await selectOrg(page);

    // P9 的「尚未綁定的案場」—— 台南七股一號刻意沒綁，它一定在裡面。
    await goto(page, 'energyWorkflowConsole', '[data-testid="workflow-row"]');
    await waitIdle(page);
    const unboundRows = await page.evaluate(() => {
      const rows = Array.from(document.querySelectorAll('[data-testid="unbound-station"]'));
      return rows.map((r) => ({
        id: Number((r as HTMLElement).dataset.station),
        missing: (r as HTMLElement).dataset.missing,
        text: (r as HTMLElement).innerText.replace(/\n/g, ' ').slice(0, 80),
      }));
    });
    log(`  尚未綁定：${JSON.stringify(unboundRows)}`);
    // 漏綁不會報錯 —— 這一區就是為了讓它被看見。
    check('P9 列得出「尚未綁定的案場」', unboundRows.length > 0, unboundRows);
    const tainan = unboundRows.find((u: any) => /台南七股一號/.test(u.text));
    check('台南七股一號（刻意未綁）在裡面，且兩種都缺',
      !!tainan && /workflow/.test(tainan.missing ?? '') && /rating/.test(tainan.missing ?? ''), tainan);

    // 點得進 P11
    const intoStation = await page.evaluate(async () => {
      const r = document.querySelector('[data-testid="unbound-station"]') as HTMLElement | null;
      if (!r) return { note: 'none' };
      const id = r.dataset.station;
      r.click();
      await new Promise((res) => { setTimeout(res, 2800); });
      return { id, hash: location.hash };
    });
    check('未綁定那一列點得進 P11',
      new RegExp(`energyStationConsole/${(intoStation as any)?.id}/show`).test((intoStation as any)?.hash ?? ''),
      intoStation);

    // ── ② 綁定：台南 → 流程 ＋ 策略 ──────────────────────────
    hr('② 建立綁定：未綁定的案場綁得起來');
    const bindOnce = async (kind: string) => {
      const opened = await page.evaluate((k: string) => {
        const grp = Array.from(document.querySelectorAll('[data-testid="binding-actions"]'))
          .find((g) => (g as HTMLElement).dataset.kind === k);
        const btn = grp?.querySelector('[data-testid="binding-create"]') as HTMLElement | null;
        if (!btn) return false;
        btn.click();
        return true;
      }, kind);
      if (!opened) return { note: `no create button for ${kind}` };
      await waitIdle(page);
      // 真滑鼠開 MUI Select（頁內 click 開不了）。
      if (!await pickVersion(page)) return { note: 'no options' };
      await setBindingDate(page, '2000-01-01');
      await page.click('[data-testid="binding-submit"]');
      await waitIdle(page);
      return { ok: true };
    };

    log(`  綁流程：${JSON.stringify(await bindOnce('workflow'))}`);
    log(`  綁策略：${JSON.stringify(await bindOnce('rating'))}`);
    await waitIdle(page);
    const afterBind = await page.evaluate(() => ({
      wf: document.querySelectorAll('[data-testid="workflow-axis"] [data-testid="axis-segment"]').length,
      st: document.querySelectorAll('[data-testid="strategy-axis"] [data-testid="axis-segment"]').length,
    }));
    log(`  綁完的軸：${JSON.stringify(afterBind)}`);
    // 綁完之後兩條軸都有東西 —— 這個案場**收得了單**了。
    check('未綁定的案場綁得起來（兩條軸都有段了）',
      afterBind.wf > 0 && afterBind.st > 0, afterBind);
    await shot(page, '15-binding-created');

    // 綁完之後，P9 的「尚未綁定」不該再有它
    await goto(page, 'energyWorkflowConsole', '[data-testid="workflow-row"]');
    await waitIdle(page);
    const stillUnbound = await page.evaluate((sid: string) => Array.from(
      document.querySelectorAll('[data-testid="unbound-station"]'),
    ).some((r) => (r as HTMLElement).dataset.station === sid), String((intoStation as any)?.id));
    check('綁完之後它從「尚未綁定」清單裡消失', stillUnbound === false, { stillUnbound });

    // ── ③ 換版（回溯）＋ 舊段唯讀 ───────────────────────────
    hr('③ 回溯換版：兩行後果都要印出來');
    // 找一個**已經有兩段**的案場（屏東枋寮一號）。
    await goto(page, 'energyLandownerConsole', '[data-testid="landowner-row"]');
    await waitIdle(page);
    const toPingtung = await page.evaluate(async () => {
      const wait = (ms: number) => new Promise((res) => { setTimeout(res, ms); });
      const rows = Array.from(document.querySelectorAll('[data-testid="landowner-row"]'));
      for (let i = 0; i < rows.length; i += 1) {
        (document.querySelectorAll('[data-testid="landowner-row"]')[i] as HTMLElement)?.click();
        await wait(2500);
        const chips = Array.from(document.querySelectorAll('[data-testid="landowner-open-station"]'));
        const hit = chips.find((c) => /屏東枋寮一號/.test((c as HTMLElement).innerText));
        if (hit) { (hit as HTMLElement).click(); await wait(3000); return location.hash; }
        window.history.back(); await wait(1800);
      }
      return '';
    });
    log(`  屏東：${toPingtung}`);

    const beforeSwitch = await page.evaluate(() => document
      .querySelectorAll('[data-testid="workflow-axis"] [data-testid="axis-segment"]').length);

    const openedSwitch = await page.evaluate(() => {
      const grp = Array.from(document.querySelectorAll('[data-testid="binding-actions"]'))
        .find((g) => (g as HTMLElement).dataset.kind === 'workflow');
      const btn = grp?.querySelector('[data-testid="binding-switch"]') as HTMLElement | null;
      if (!btn) return false;
      btn.click();
      return true;
    });
    let switched: any = { note: 'no switch button' };
    if (openedSwitch) {
      await waitIdle(page);
      if (!await pickVersion(page)) {
        switched = { note: 'no options' };
      } else {
        // **回溯生效** —— 選一個早於今天的日期。
        await setBindingDate(page, '2026-02-01');
        switched = await page.evaluate(() => {
          const warn = document.querySelector('[data-testid="binding-backdated"]') as HTMLElement | null;
          return {
            shown: !!warn,
            affected: warn?.dataset.affected ?? '',
            running: warn?.dataset.running ?? '',
            text: warn?.innerText.replace(/\n/g, ' ') ?? '',
          };
        });
        await page.click('[data-testid="binding-submit"]');
        await waitIdle(page);
      }
    }
    log(`  回溯換版：${JSON.stringify(switched)}`);
    // 兩行後果都要在 —— 少了第二行管理員不敢按。
    check('回溯換版對話框印出「N 張未送審」', (switched as any)?.shown === true
      && (switched as any)?.affected !== '', switched);
    check('且印出「M 筆執行不受影響」（少了這行沒人敢按）',
      /不受影響/.test((switched as any)?.text ?? ''), (switched as any)?.text);

    await waitIdle(page);
    const afterSwitch = await page.evaluate(() => {
      const segs = Array.from(document.querySelectorAll('[data-testid="workflow-axis"] [data-testid="axis-segment"]'));
      return { count: segs.length };
    });
    const bSegs = await (async () => {
      const m = (await page.evaluate(() => location.hash)).match(/energyStationConsole\/(\d+)/);
      if (!m) return { note: 'no station id' };
      const r = await eapi(page, 'GET', `/stations/${m[1]}/bindings`);
      const d = r.status === 200 ? r.data : null;
      return {
        segs: (d?.workflow ?? []).map((x: any) => ({ v: x.version, from: x.validFrom, to: x.validTo, editable: x.editable, isCurrent: x.isCurrent })),
      };
    })();
    log(`  換版後的段：${JSON.stringify(bSegs)}`);
    // 效期軸**真的多一段**。
    check('換版後效期軸多一段', afterSwitch.count > beforeSwitch,
      { beforeSwitch, after: afterSwitch.count });
    // **舊段變唯讀** —— 那期間已經有單依它跑完了，改它＝改寫歷史。
    const segList: any[] = (bSegs as any)?.segs ?? [];
    /**
     * **「除了最後一段其餘都唯讀」這個形狀已經不成立**（2026-09-09）。
     *
     * 一座案場現在可以同時綁多條流程（躉購·有餘電要核可 ＋ 開發票），
     * 所以清單裡會**交錯**出現不同線的現行段 ——
     * 「最後一段」不再等於「唯一還開著的那一段」。
     *
     * 但要驗的東西沒有變，而且比原本更精確：
     * **已結束的段唯讀、還開著的段可編輯**。
     */
    const today = new Date().toISOString().slice(0, 10);
    const closed = segList.filter((x) => x.to && String(x.to).slice(0, 10) <= today);
    const open = segList.filter((x) => !x.to);
    check('已結束的段變成唯讀（editable=false）',
      closed.length >= 1 && closed.every((x) => x.editable === false), closed);
    check('還開著的段可以編輯（每條線各自有一段）',
      open.length >= 1 && open.every((x) => x.editable === true), open);
    await shot(page, '15-binding-switched');

    // ── ④ 解除 → critical 稽核 ──────────────────────────────
    hr('④ 解除綁定：理由必填，稽核是 critical');
    const ended = await page.evaluate(async () => {
      const wait = (ms: number) => new Promise((res) => { setTimeout(res, ms); });
      const grp = Array.from(document.querySelectorAll('[data-testid="binding-actions"]'))
        .find((g) => (g as HTMLElement).dataset.kind === 'workflow');
      const btn = grp?.querySelector('[data-testid="binding-end"]') as HTMLElement | null;
      if (!btn) return { note: 'no end button' };
      btn.click(); await wait(1500);
      // 理由沒填時送出鈕應該是 disabled。
      const submit = document.querySelector('[data-testid="binding-submit"]') as HTMLButtonElement | null;
      const disabledWithoutReason = !!submit?.disabled;
      const reason = document.querySelector('[data-testid="binding-reason"]') as HTMLTextAreaElement | null;
      const setter = Object.getOwnPropertyDescriptor(window.HTMLTextAreaElement.prototype, 'value')!.set!;
      setter.call(reason!, '案場轉手，合約終止');
      reason!.dispatchEvent(new Event('input', { bubbles: true }));
      await wait(600);
      const date = document.querySelector('[data-testid="binding-date"]') as HTMLInputElement | null;
      const dsetter = Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, 'value')!.set!;
      dsetter.call(date!, '2026-12-01');
      date!.dispatchEvent(new Event('change', { bubbles: true }));
      await wait(900);
      (document.querySelector('[data-testid="binding-submit"]') as HTMLElement)?.click();
      await wait(3500);
      return { disabledWithoutReason };
    });
    log(`  解除：${JSON.stringify(ended)}`);
    // 理由必填（前端也擋）。
    check('沒填理由時「解除」按不下去', (ended as any)?.disabledWithoutReason === true, ended);

    const endAudit = await (async () => {
      const r = await eapi(page, 'GET', '/audit-events?limit=200');
      const rows: any[] = r.status === 200 ? (r.data?.items ?? []) : [];
      const e = rows.find((x: any) => x.type === 'flow.binding_ended');
      return e ? { severity: e.severity, summary: e.summary } : { note: 'not found', types: [...new Set(rows.map((x: any) => x.type))].slice(0, 30) };
    })();
    log(`  解除稽核：${JSON.stringify(endAudit)}`);
    // critical —— 之後這個案場收不了單。
    check('解除綁定的稽核是 **critical**', (endAudit as any)?.severity === 'critical', endAudit);
    check('summary 講後果（收不了單）', /收不了單/.test((endAudit as any)?.summary ?? ''), endAudit);
    await shot(page, '15-binding-ended');

    // ── ⑤ 權限：finance@ 與 cxo@ 都**看不到**這三顆按鈕 ─────────
    hr('⑤ 權限：綁定的三顆按鈕連 CXO 都看不到');
    for (const who of [demoEmail('finance'), demoEmail('cxo'), demoEmail('viewer')]) {
      await loginAs(page, who);
      await selectOrg(page);
      await goto(page, 'energyLandownerConsole', '[data-testid="landowner-row"]');
      await waitIdle(page);
      const seen = await page.evaluate(async () => {
        const wait = (ms: number) => new Promise((res) => { setTimeout(res, ms); });
        (document.querySelector('[data-testid="landowner-row"]') as HTMLElement)?.click();
        await wait(2500);
        const st = document.querySelector('[data-testid="landowner-open-station"]') as HTMLElement | null;
        if (!st) return { note: 'no station' };
        st.click(); await wait(3000);
        return {
          groups: document.querySelectorAll('[data-testid="binding-actions"]').length,
          buttons: document.querySelectorAll('[data-testid="binding-create"],[data-testid="binding-switch"],[data-testid="binding-end"]').length,
        };
      });
      log(`  ${who} 看到的綁定按鈕：${JSON.stringify(seen)}`);
      // 綁定決定錢怎麼算、誰要核准 —— **CXO 也不行**。
      check(`${who} 看不到綁定的三顆按鈕`, ((seen as any)?.buttons ?? 0) === 0, seen);
    }
    await shot(page, '15-binding-no-permission');
    hr('伺服器');
    check('沒有 5xx', serverErrors.length === 0, [...new Set(serverErrors)]);
  } finally {
    await browser.close();
  }
  board.finish('效期綁定管理');
}

run().catch((e) => { console.error(e); process.exitCode = 1; });
