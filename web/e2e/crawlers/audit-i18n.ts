/* eslint-disable no-console, no-await-in-loop */
/**
 * **稽核頁的「事件」那一欄要讀得懂 —— 兩種語言都是。**（LIFECYCLE 【7】C）
 *
 * 這一支存在的理由是一個很好認的空洞：`auditEvents.ts` 的 registry 有 51 個
 * `summaryKey`，而 `zh-tw.ts` 裡 `audit.type.*` **一個都沒有**。
 * 結果是稽核頁那一欄**每一列都印破折號** —— 而那是那一頁最主要的一欄。
 *
 * 缺 key 的兩種後果不一樣，兩種都要擋：
 *
 * | 語言 | 缺 key 的樣子 | 為什麼糟 |
 * |---|---|---|
 * | 中文 | `—`（有 `_` fallback） | 看起來像「沒有資料」，其實是沒翻譯 |
 * | 英文 | `audit.type.flow.taskClaimed` | **直接印 raw key**，更糟 |
 *
 * `auditI18n.spec.ts` 已經在**檔案層**比對過 registry ↔ 翻譯了。
 * 這一支問的是另一件事：**畫面上真的印出來了嗎** ——
 * 中間還隔著 `summaryOf()` 的插值、`translate()` 的 fallback、
 * 以及後端有沒有把 `payload` 一起送出來。那三處任何一處斷掉，
 * 檔案層全綠、畫面照樣是破折號。
 *
 * ### 這一支怎麼跑
 *
 * 移植自 az `scripts/crawler-energy-ops-audit-i18n`。`web/e2e/run-crawlers.sh audit-i18n`
 * （示範資料＋四份範本電費單；示範資料本身就會產生稽核事件，不必另外製造）。
 * 斷言全部是**相對值**（「破折號的比例」「raw key 的筆數」），
 * 所以資料多寡不影響結果，可以重複跑。
 *
 * 帳號用 `viewer@demo.energy`：`manager@` 是 `energy:operator`，**沒有 `energy:audit:view`**，
 * 進去只會拿到 403 —— 那會變成一支永遠綠不了、原因還跟翻譯無關的 crawler。
 */
import type { Page } from 'puppeteer';
import {
  Scoreboard, hr, log, sleep, launch, shooter, loginAs, selectOrg, goto, retryOnDetach, hardenPage,
  waitIdle,
  waitUntil,
  demoEmail,
} from '../lib/energyOpsHarness.ts';

const board = new Scoreboard();
const check = board.check.bind(board);
const shot = shooter('energy-audit-i18n');

/**
 * 把「對象」那一欄整欄讀回來。
 *
 * 這一欄的來源跟「事件」欄**完全不同**：它是 `targetLabel`，
 * 一個**寫入當下就固定下來的字串快照**（不是 i18n key，也不是讀取時 join）。
 * 所以它不會因為切語言而變，也不會因為節點改名而變 ——
 * 後者是刻意的：稽核記的是當時那一關叫什麼。
 */
async function readTargetColumn(page: Page) {
  return retryOnDetach('readTargetColumn', () => page.evaluate(() => {
    const cells = Array.from(document.querySelectorAll('[data-testid="audit-target"]'))
      .map((e) => (e as HTMLElement).innerText.trim());
    return { cells };
  }), 3, page);
}

/**
 * 這一輪稽核裡**有可能漏出來的那組識別碼** —— 從稽核列自己的 `payload` 推。
 *
 * 前兩版都選錯來源：
 * · 用**形狀**猜（kebab-case ／ 帶點）→ 把附件檔名 `bill-small-review.png`
 *   判成違規，而那是正確的 `targetLabel`。誤報跟真發現長得一樣。
 * · 去問 `/workflows` → `viewer@` 拿不到節點，字彙只剩一個，
 *   於是「沒有 raw key」變成跟一個幾乎空的清單比對，**然後綠燈**。
 *
 * `payload.nodeKey` / `payload.sideEffectKey` 是**同一批列自己帶的**：
 * 一筆稽核如果把識別碼寫進了 `targetLabel`，那個識別碼一定也在它的 payload 裡。
 * 來源跟被檢查的對象同源，不會有權限問題，也不用猜形狀。
 */
async function rawKeyVocabulary(page: Page): Promise<string[]> {
  return retryOnDetach('rawKeyVocabulary', () => page.evaluate(async () => {
    const org = (window as any).__demoOrgId;
    const r = await fetch(
      `/api/v1/orgs/${org}/energy/audit-events?limit=500`, { credentials: 'include' },
    );
    const rows = r.ok ? ((await r.json())?.items ?? []) : [];
    const out = new Set<string>();
    for (const row of (Array.isArray(rows) ? rows : [])) {
      const p = row?.payload ?? {};
      for (const k of [p.nodeKey, p.sideEffectKey, p.workflowKey, p.targetNodeKey]) {
        if (typeof k === 'string' && k) out.add(k);
      }
      for (const g of ([] as string[]).concat(p.gatesBefore ?? [], p.gatesAfter ?? [])) {
        if (typeof g === 'string' && g) out.add(g);
      }
    }
    return [...out];
  }), 3, page);
}


/** 把「事件」那一欄整欄讀回來。 */
async function readWhatColumn(page: Page) {
  return retryOnDetach('readWhatColumn', () => page.evaluate(() => {
    const cells = Array.from(document.querySelectorAll('[data-testid="audit-what"]'))
      .map((e) => (e as HTMLElement).innerText.trim());
    return {
      total: cells.length,
      dashes: cells.filter((t) => t === '—' || t === '' || t === '-').length,
      // raw key 的特徵：中間有點、而且沒有空白（`audit.type.flow.taskClaimed`）。
      rawKeys: cells.filter((t) => /^[a-z][\w]*(\.[\w]+){2,}$/.test(t)),
      // 沒被插進去的樣板洞（`%{gate}` 原封不動印出來）。
      unfilled: cells.filter((t) => t.includes('%{')),
      // 沒被插值的洞也可能長成「多了一關「—」」—— 那句話等於沒說。
      emptySlot: cells.filter((t) => t.includes('「—」')),
      sample: cells.slice(0, 12),
    };
  }), 3, page);
}

/**
 * 用 AppBar 上真正的語言選單切語言（不是塞 localStorage）。
 *
 * `LocalesMenuButton` 的按鈕文字是**目前語言的名字**（「繁體中文」），
 * 所以是靠它認按鈕，不是靠位置 —— header 上還有別的按鈕，
 * 「第一顆有文字的」會抓到別人，然後選單根本沒開、
 * 而下一步的「找不到 English」看起來就像翻譯壞了。
 */
async function switchLocale(page: Page, from: string, to: string) {
  const opened = await page.evaluate((cur: string) => {
    const btn = Array.from(document.querySelectorAll('header button, header [role=button]'))
      .find((b) => (b as HTMLElement).innerText.trim() === cur);
    if (!btn) return false;
    (btn as HTMLElement).click();
    return true;
  }, from);
  if (!opened) return { opened, picked: false, applied: false };
  await page.evaluate((t: string) => { (window as any).__wantLocale = t; }, to);
  // 選單是**動畫展開**的：等「那一項真的在 DOM 裡」，不要等時間。
  await waitUntil(page, () => Array.from(document.querySelectorAll('[role=menuitem]')).length > 0,
    { label: '語言選單展開' });
  const picked = await page.evaluate((t: string) => {
    const item = Array.from(document.querySelectorAll('[role=menuitem]'))
      .find((m) => (m as HTMLElement).innerText.trim() === t);
    if (!item) return false;
    (item as HTMLElement).click();
    return true;
  }, to);
  /**
   * **等「語言真的換了」，不是等網路安靜。**
   *
   * 切語言不會發任何請求：RA 是把整棵樹重掛。所以 `waitIdle` 會在
   * locale 還沒套用之前就回來 —— 然後英文那幾條斷言拿到的是中文，
   * 而紅燈長得像「en.ts 沒生效」。判準用**切換鈕自己的文字**：
   * 它顯示的就是目前語言，換完才會變成 `to`。
   */
  const applied = await waitUntil(page, () => {
    const btn = Array.from(document.querySelectorAll('header button, header [role=button]'))
      .find((b) => (b as HTMLElement).innerText.trim() === (window as any).__wantLocale);
    return !!btn;
  }, { label: `語言切換到 ${to}` });
  return { opened, picked, applied };
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
    hr('① 進得了稽核頁（viewer@ 才有 energy:audit:view）');
    await loginAs(page, demoEmail('viewer'));
    check('選得到 demo 組織', (await selectOrg(page)) != null);
    await goto(page, 'energyAuditConsole', '[data-testid="audit-row"], .MuiAlert-root');
    await waitIdle(page);

    const zh = await readWhatColumn(page);
    log('  中文樣本：', JSON.stringify(zh.sample, null, 0));
    /**
     * 先證明**探針量到東西了** —— 零列的話下面每一條都會用 0/0 綠掉，
     * 而「沒跑到」跟「跑過而且綠」在報表上長得一模一樣。
     */
    check('稽核頁有列可以讀（不然下面全部沒有意義）', zh.total > 0, { total: zh.total });

    hr('② 中文：「事件」欄不再是破折號');
    check('破折號 0 筆', zh.dashes === 0, { total: zh.total, dashes: zh.dashes, sample: zh.sample });
    check('沒有 raw key 漏到畫面上', zh.rawKeys.length === 0, zh.rawKeys);
    check('樣板洞都填起來了（沒有 %{...}）', zh.unfilled.length === 0, zh.unfilled);
    check('插值不是空的（沒有「多了一關「—」」這種空話）',
      zh.emptySlot.length === 0, zh.emptySlot);
    /**
     * 「讀得懂」沒辦法完全自動化，但**最沒用的那一種**擋得掉：
     * 一句話短到只剩名詞就是在重述事件名。
     */
    const tooShort = zh.sample.filter((t) => t.length < 5);
    check('句子不是只把事件名翻一次（樣本裡沒有短到 5 個字以下的）',
      tooShort.length === 0, tooShort);
    await page.evaluate(() => {
      document.querySelector('[data-testid="audit-row"]')?.scrollIntoView({ block: 'center' });
    });
    await sleep(500);
    await shot(page, '1-audit-zh');

    hr('②-b 「對象」欄：識別碼不可以漏到畫面上');
    const vocab = await rawKeyVocabulary(page);
    log(`  這個組織的識別碼字彙（${vocab.length} 個）：${JSON.stringify(vocab.slice(0, 12))}`);
    const { cells } = await readTargetColumn(page);
    // 兩種形狀都要認：整格就是識別碼、以及 `bill-approval / 15` 那種前綴。
    const leaked = cells.filter((t) => vocab.some(
      (k) => t === k || t.startsWith(`${k} /`) || t.startsWith(`${k}/`),
    ));
    const tgt = {
      total: cells.length,
      rawKeys: leaked,
      empty: cells.filter((t) => t === '' || t === '—').length,
      sample: cells.slice(0, 12),
    };
    log('  對象樣本：', JSON.stringify(tgt.sample, null, 0));
    check('抓得到識別碼字彙（空的話下面那條會用 0 個 pattern 假裝通過）',
      vocab.length > 6, vocab);
    check('對象欄讀得到（不然下面兩條會用 0 筆假裝通過）', tgt.total > 0, { total: tgt.total });
    /**
     * 這一條就是使用者實測抓到的那件事：同一頁上一半寫「主管核准」、
     * 一半寫 `manager-approve` —— 因為 `WorkflowEngine` 有十來個寫入點
     * 寫的是 `task.get('nodeKey')`，而 `candidates_resolved` 那一處寫的是名字。
     */
    check('對象欄沒有 raw key（clerk-review／payment.sent／bill-approval）',
      tgt.rawKeys.length === 0, [...new Set(tgt.rawKeys)]);
    /**
     * 反向：**不可以整欄都空**。
     * 「沒有 raw key」最省事的過關法就是不寫 label，
     * 而那會讓這一欄變成整排破折號 —— 比印 key 更糟。
     */
    check('而且不是靠「整欄留空」過關的',
      tgt.empty < tgt.total * 0.2, { empty: tgt.empty, total: tgt.total });

    hr('③ 英文：切過去不出現 raw key');
    const sw = await switchLocale(page, '繁體中文', 'English');
    check('AppBar 上開得出語言選單', sw.opened, sw);
    check('選單裡點得到 English', sw.picked, sw);
    /**
     * **切語言會讓 RA 把整棵樹重掛** —— 那一瞬間 main frame 會 detach，
     * 而 detach 之後**重跑同一個 `evaluate` 是救不回來的**
     * （frame 不會自己回來，只有 reload 會建一個新的）。
     * 三個探針都包在 `retryOnDetach(..., page)` 裡就是為了這件事；
     * 這裡再多等一下，讓大部分情況根本走不到重試。
     */
    await waitIdle(page);
    const en = await readWhatColumn(page);
    log('  英文樣本：', JSON.stringify(en.sample, null, 0));
    check('英文也讀得到同樣多的列（切語言不該把表清掉）',
      en.total === zh.total, { zh: zh.total, en: en.total });
    /**
     * 這一條是 `en.ts` 存在的**唯一**理由：ra-i18n-polyglot 對缺 key
     * 沒有 fallback 到中文這回事，它會**原封不動印出 key 本身**。
     */
    check('英文沒有 raw key', en.rawKeys.length === 0, en.rawKeys);
    check('英文也沒有破折號', en.dashes === 0, { total: en.total, dashes: en.dashes, sample: en.sample });
    check('英文的樣板洞也填起來了', en.unfilled.length === 0, en.unfilled);
    // 切了語言但字沒變 → 多半是 `en.ts` 根本沒載到（動態 import 失敗會靜默）。
    check('英文真的跟中文不一樣（不是切了沒生效）',
      JSON.stringify(en.sample) !== JSON.stringify(zh.sample),
      { zh: zh.sample[0], en: en.sample[0] });
    await page.evaluate(() => {
      document.querySelector('[data-testid="audit-row"]')?.scrollIntoView({ block: 'center' });
    });
    await sleep(500);
    await shot(page, '2-audit-en');

    check('沒有 5xx', serverErrors.length === 0, [...new Set(serverErrors)]);
  } finally {
    await browser.close();
  }
  board.finish('稽核頁 i18n');
}

run().catch((e) => { console.error(e); process.exit(1); });
