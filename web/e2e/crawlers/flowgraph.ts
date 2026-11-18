/* eslint-disable no-console, no-await-in-loop */
/**
 * **流程圖上不可以有線壓過節點**（2026-09-09，使用者回報）。
 *
 * > 最後調整一下你的流程圖，現在有一些線會從節點上面穿過去。
 *
 * 這件事**量得出來**，所以不要用眼睛驗：把每個節點的框與每條邊的路徑取樣點
 * 抓下來，算「有沒有邊的取樣點落在別人的框裡」。
 *
 * 修法在 `FlowGraph.tsx` 的 `BowEdge`：退回邊往下繞、會撞到節點的分支邊往上繞。
 * 這一支就是那個修法的守衛 —— 換版型、加關卡、改 layout 都可能讓它再次發生，
 * 而那種退步在截圖上很容易被當成「圖本來就這麼亂」。
 *
 * 移植自 az `scripts/crawler-energy-ops-flowgraph`。跑法：`web/e2e/run-crawlers.sh flowgraph`
 * （示範資料＋四份範本電費單）。
 */
import type { Page } from 'puppeteer';
import {
  Scoreboard, hr, log, launch, shooter, loginAsAdmin, selectOrg, goto,
  hardenPage, waitIdle, waitUntil,
  eapi,
} from '../lib/energyOpsHarness.ts';

const board = new Scoreboard();
const check = board.check.bind(board);
const shot = shooter('energy-flowgraph');

/** 節點的框、邊的取樣點，以及「誰壓到誰」。 */
async function geometry(page: Page) {
  return page.evaluate(() => {
    const nodes = Array.from(document.querySelectorAll('.react-flow__node')).map((n) => {
      const el = n as HTMLElement;
      const b = el.getBoundingClientRect();
      return {
        id: el.dataset.id ?? '', x: b.left, y: b.top, w: b.width, h: b.height,
      };
    });
    const edges = Array.from(document.querySelectorAll('.react-flow__edge')).map((e) => {
      const el = e as HTMLElement;
      const path = el.querySelector('path.react-flow__edge-path') as SVGPathElement | null;
      const len = path?.getTotalLength() ?? 0;
      const pts: Array<{ x: number; y: number }> = [];
      for (let i = 0; i <= 60; i += 1) {
        const p = path?.getPointAtLength((len * i) / 60);
        if (p && path) {
          // SVG 座標 → 畫面座標（節點的框是畫面座標）。
          const m = (path as any).getScreenCTM();
          pts.push({ x: p.x * m.a + p.y * m.c + m.e, y: p.x * m.b + p.y * m.d + m.f });
        }
      }
      return { id: el.dataset.id ?? '', pts };
    });
    const crossings: Array<{ edge: string; through: string }> = [];
    edges.forEach((e) => {
      // 邊的 id 形狀是 `<from>-><to>-<kind>-<i>`：`->` 之後、最後兩段之前的就是終點。
      const [from, rest] = String(e.id).split('->');
      const parts = String(rest ?? '').split('-');
      const to = parts.slice(0, Math.max(parts.length - 2, 1)).join('-');
      nodes.forEach((n) => {
        if (!n.id || n.id === from || n.id === to) return;
        const inside = e.pts.some((p) => p.x > n.x + 2 && p.x < n.x + n.w - 2
          && p.y > n.y + 2 && p.y < n.y + n.h - 2);
        if (inside) crossings.push({ edge: e.id, through: n.id });
      });
    });
    return {
      nodeCount: nodes.length,
      edgeCount: edges.length,
      nodes: nodes.map((n) => ({ id: n.id, x: Math.round(n.x), y: Math.round(n.y) })),
      crossings,
    };
  });
}

async function run() {
  const browser = await launch();
  const page = hardenPage(await browser.newPage());
  try {
    hr('① 用電電費單付款流程：有退回邊的那一版（承辦確認／主管核准／財務放行都可退回）');
    await loginAsAdmin(page);
    check('選得到 demo 組織', (await selectOrg(page)) != null);
    check('進得了流程總覽', await goto(page, 'energyWorkflowConsole', '[data-testid="workflow-row"]'));
    await waitIdle(page);

    // 挑「用電電費單付款流程」—— 6 個節點、5 條順向＋2 條退回邊，退回線最會穿過節點。
    // workflow_defs.rs:764/768/773/775 已把 layout x 座標後移（跟承辦確認的間距從 20 拉開到
    // 220，其餘都 ≥180），退回邊不再畫穿「承辦確認」。
    await page.evaluate(() => {
      const row = Array.from(document.querySelectorAll('[data-testid="workflow-row"]'))
        .find((r) => /用電電費單/.test((r as HTMLElement).innerText)) as HTMLElement | undefined;
      row?.click();
    });
    check('點得開流程圖', await waitUntil(page, () => !!document.querySelector('.react-flow__node'),
      { timeout: 20000, label: '流程圖節點' }));
    await waitIdle(page);
    await page.evaluate(() => {
      document.querySelector('[data-testid="flow-graph"]')?.scrollIntoView({ block: 'center' });
    });
    await new Promise((r) => { setTimeout(r, 800); });
    // commander 代看時發現改前那張截圖裁到只剩前三個節點（用電電費單 6 個節點比
    // 保證金請款 4 個節點寬，預設視角裝不下）——點一下 react-flow 內建的
    // fit-view 鍵，讓六個節點都進畫面，截圖才看得出退回線真的繞開了哪些節點。
    await page.evaluate(() => {
      (document.querySelector('.react-flow__controls-fitview') as HTMLElement | null)?.click();
    });
    await new Promise((r) => { setTimeout(r, 500); });

    const geo = await geometry(page);
    log(`  節點 ${geo.nodeCount} / 邊 ${geo.edgeCount}`);
    log('  穿越：', JSON.stringify(geo.crossings));
    check('圖上有節點與邊（不是空畫布）', geo.nodeCount >= 6 && geo.edgeCount >= 7, geo);
    // **這一條就是使用者回報的那件事**。
    check('沒有任何一條線壓過節點', geo.crossings.length === 0, geo.crossings);
    await shot(page, '1-definition-graph');

    hr('② 執行圖：同一張圖在單據詳情頁上');
    const billId = await (async () => {
      const r = await eapi(page, 'GET', '/executions?status=running&scope=all');
      if (r.status !== 200) return 0;
      const list: any[] = r.data?.items ?? [];
      const hit = list.find((e: any) => e.subject?.kind === 'energyBill' && e.subject?.billId);
      return hit ? Number(hit.subject.billId) : 0;
    })();
    if (billId) {
      await goto(page, `energyBillConsole/${billId}/show`, '[data-testid="flow-graph"], [data-testid="review-field"]');
      await waitUntil(page, () => !!document.querySelector('.react-flow__node'), { timeout: 20000 });
      await waitIdle(page);
      const runGeo = await geometry(page);
      log('  執行圖穿越：', JSON.stringify(runGeo.crossings));
      check('執行圖也沒有線壓過節點', runGeo.crossings.length === 0, runGeo.crossings);
      await shot(page, '2-execution-graph');
    } else {
      check('找得到一筆進行中的單來畫執行圖', false, billId);
    }
  } finally {
    await browser.close();
  }
  board.finish('流程圖：線不可以壓過節點');
}

run().catch((e) => { console.error(e); process.exitCode = 1; });
