#!/usr/bin/env node
// 產生規格文件網站：node docs/spec/tools/build.mjs
//   1. 重畫 diagrams/*.svg（diagrams.mjs）
//   2. 每份文件一個 HTML（index.html 與各資料夾下的頁面），彼此以相對路徑連結
//   3. dist/ 下一份單檔可攜版（所有頁面與圖都內嵌，可直接寄送或開啟）
// 內容來源：src/registers.mjs（清冊）、src/pages.mjs（敘述）。改內容改那兩份，不要改產出的 HTML。
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { CLAR, DATE, IMPL, VERSION, esc } from './lib.mjs';
import { diagrams, writeDiagrams } from './diagrams.mjs';
import * as R from '../src/registers.mjs';
import { PAGES, ARCH } from '../src/pages.mjs';

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..');
const PRODUCT = '電費單與租金管理系統';
const DEADLINE = '2026-11-30';

// ───────────────────────── 連結與標籤 ─────────────────────────
const PAGE_OF = {
  Q: '40-清冊/待釐清問題.html', B: '20-驗收/合約驗收對照表.html', R: '20-驗收/合約驗收對照表.html', D: '20-驗收/合約驗收對照表.html',
  FR: '30-需求/需求總表.html', NFR: '30-需求/需求總表.html', US: '30-需求/需求總表.html',
  DATA: '40-清冊/假資料與不確定資料.html', DEC: '40-清冊/決策紀錄.html', TD: '40-清冊/測試資料盤點.html',
};
const archPath = (id) => `10-架構/${id}-${ARCH[id].slug}.html`;

/** 產生連結時的情境：多頁版用相對路徑；單檔版用頁內錨點。 */
class Ctx {
  constructor(mode, current) { this.mode = mode; this.current = current; }
  href(path, anchor) {
    if (this.mode === 'single') return `#${anchorOf(path)}${anchor ? `--${anchor}` : ''}`;
    const depth = this.current.split('/').length - 1;
    const rel = `${'../'.repeat(depth)}${path}`;
    return encodeURI(rel) + (anchor ? `#${anchor}` : '');
  }
  asset(path) {
    if (this.mode === 'single') {
      const svg = readFileSync(join(ROOT, path));
      return `data:image/svg+xml;base64,${svg.toString('base64')}`;
    }
    const depth = this.current.split('/').length - 1;
    return encodeURI(`${'../'.repeat(depth)}${path}`);
  }
  anchor(id) { return this.mode === 'single' ? `${anchorOf(this.current)}--${id}` : id; }
}
const anchorOf = (path) => `p-${path.replace(/\.html$/, '').replace(/[^\w一-鿿-]+/g, '-')}`;

function refPath(id) {
  if (/^A[0-7]$/.test(id)) return [archPath(id), null];
  const m = id.match(/^(DATA|DEC|NFR|FR|US|TD|Q|B|R|D)-?/);
  if (!m) return null;
  // 範圍標籤（例如 B04–B08）連到第一項。
  return [PAGE_OF[m[1]], id.split('–')[0]];
}

const IMPL_TEXT = Object.fromEntries(Object.entries(IMPL).map(([k, v]) => [k, v.label]));
const CLAR_TEXT = Object.fromEntries(Object.entries(CLAR).map(([k, v]) => [k, v.label]));

export const H = {
  st: (k) => `<span class="st st-${k}" title="實作狀態">${esc(IMPL_TEXT[k] || k)}</span>`,
  cl: (k) => `<span class="cl cl-${k}" title="釐清狀態">${esc(CLAR_TEXT[k] || k)}</span>`,
  dt: (k) => `<span class="dt dt-${k}">${k === 'mock' ? '假資料' : '資料不確定'}</span>`,
  tag: (t) => `<span class="tag">${esc(t)}</span>`,
  av: (k) => `<span class="st st-${{ yes: 'done', partial: 'partial', no: 'todo' }[k]}" title="測試資料">${{ yes: '已有', partial: '部分', no: '沒有' }[k]}</span>`,
};

function linkRefs(ctx, text) {
  // 把文字中的編號（Q-05、B01、FR-012、A3…）變成連結；只處理純文字，不碰已含 HTML 的字串。
  return esc(text).replace(/\b(Q-\d{2}|B\d{2}|R0\d|D0\d|FR-\d{3}|NFR-\d{3}|US-\d{1,2}|DATA-\d{2}|DEC-\d{2}|TD-\d{2}|A[0-7])\b/g, (m) => refLink(ctx, m));
}
function refLink(ctx, id) {
  const p = refPath(id);
  if (!p || !p[0]) return esc(id);
  const cls = id.startsWith('Q-') ? 'ref ref-q' : 'ref';
  return `<a class="${cls}" href="${ctx.href(p[0], p[1])}">${esc(id)}</a>`;
}
const refs = (ctx, ids) => (ids && ids.length ? ids.map((i) => refLink(ctx, i)).join(' ') : '<span class="muted">—</span>');

// ───────────────────────── 共用元件 ─────────────────────────
function legend() {
  const impl = Object.keys(IMPL).map((k) => `<li>${H.st(k)}<span>${esc(LEGEND_IMPL[k])}</span></li>`).join('');
  const clar = Object.keys(CLAR).map((k) => `<li>${H.cl(k)}<span>${esc(LEGEND_CLAR[k])}</span></li>`).join('');
  return `<div class="legend" role="note" aria-label="狀態圖例">
  <div><h4>實作狀態</h4><ul>${impl}</ul></div>
  <div><h4>釐清狀態</h4><ul>${clar}</ul><h4>資料標記</h4><ul><li>${H.dt('mock')}<span>示範資料、mock 實作或假服務</span></li><li>${H.dt('uncertain')}<span>範例有錯、未驗證或來源不可靠</span></li></ul></div>
</div>`;
}
const LEGEND_IMPL = {
  done: 'energy-ops-rust 已實作，可直接用於驗收',
  partial: '有基礎，但欄位、流程或格式還不夠',
  todo: '驗收需要，目前沒有',
  beyond: '已實作，但合約沒有要求（去留待定）',
  external: '不在本系統內，需要介接',
  dropped: '舊版或範圍外，不採用',
};
const LEGEND_CLAR = {
  confirmed: '規格已由合約或使用者確認',
  assumed: '預設值或設計者代決，尚未經雲豹確認',
  open: '有待釐清問題，答案會改變做法',
};

/** 驗收覆蓋的分段條：已具備、部分具備為實心，未實作為底軌。每段有文字、提示與可存取名稱。 */
function coverageBar(items, caption) {
  const total = items.length;
  const c = R.countBy(items);
  const parts = [['done', '已具備'], ['partial', '部分具備'], ['todo', '未實作']].map(([k, label]) => ({ k, label, n: c[k] || 0 }));
  const segs = parts.filter((p) => p.n > 0).map((p) => {
    const pct = (p.n / total) * 100;
    const text = pct >= 9 ? `<span>${p.label} ${p.n}</span>` : '';
    return `<div class="seg seg-${p.k}" style="flex:${p.n}" tabindex="0" title="${p.label}：${p.n} 項（${Math.round(pct)}%）" aria-label="${p.label} ${p.n} 項，占 ${Math.round(pct)}%">${text}</div>`;
  }).join('');
  const leg = parts.map((p) => `<li><i class="sw sw-${p.k}"></i>${p.label} ${p.n}</li>`).join('');
  return `<figure class="cover"><figcaption>${esc(caption)}（共 ${total} 項）</figcaption><div class="bar" role="img" aria-label="${esc(caption)}：${parts.map((p) => `${p.label} ${p.n} 項`).join('、')}">${segs}</div><ul class="bar-legend">${leg}</ul></figure>`;
}

function tiles(list) {
  return `<div class="tiles">${list.map((t) => `<div class="tile"><div class="t-label">${esc(t.label)}</div><div class="t-value">${esc(t.value)}</div>${t.sub ? `<div class="t-sub">${t.sub}</div>` : ''}</div>`).join('')}</div>`;
}

function table(head, rows, opts = {}) {
  const th = head.map((h) => `<th${h.w ? ` style="width:${h.w}"` : ''}>${esc(h.t || h)}</th>`).join('');
  const body = rows.map((r) => `<tr${r.id ? ` id="${r.id}"` : ''}>${r.cells.map((c) => `<td>${c}</td>`).join('')}</tr>`).join('\n');
  const filter = opts.filter ? `<div class="filter"><label>篩選 <input type="search" placeholder="輸入關鍵字、編號或狀態" data-filter></label></div>` : '';
  return `${filter}<div class="tbl"><table class="reg${opts.compact ? ' compact' : ''}"><thead><tr>${th}</tr></thead><tbody>\n${body}\n</tbody></table></div>`;
}

function diagramFigure(ctx, id) {
  const d = diagrams.find((x) => x.id === id);
  return `<figure class="diagram"><a href="${ctx.asset(`diagrams/${id}.svg`)}" data-zoom target="_blank" rel="noopener"><img src="${ctx.asset(`diagrams/${id}.svg`)}" alt="${esc(`${id} ${d.title}`)}" loading="lazy"></a><figcaption>圖 ${esc(id)}　${esc(d.title)}。點圖放大；圖例見圖底部。</figcaption></figure>`;
}

// ───────────────────────── 各清冊頁 ─────────────────────────
function acceptancePage(ctx) {
  const groups = [['表一', '表一　電費單管理系統需求暨驗收表'], ['表二', '表二　租金管理系統需求暨驗收表'], ['交付', '其他交付與契約義務（附件一、第二、三、九條）']];
  const ab = R.acceptance.filter((a) => a.table !== '交付');
  let out = PAGES.acceptanceIntro(ctx, H);
  out += tiles([
    { label: '表一　電費單', value: '15 項', sub: summarize(R.acceptance.filter((a) => a.table === '表一')) },
    { label: '表二　租金', value: '8 項', sub: summarize(R.acceptance.filter((a) => a.table === '表二')) },
    { label: '交付與義務', value: `${R.acceptance.filter((a) => a.table === '交付').length} 項`, sub: summarize(R.acceptance.filter((a) => a.table === '交付')) },
  ]);
  out += coverageBar(ab, '表一＋表二驗收項的實作覆蓋');
  out += '<h2 id="summary">總覽</h2>';
  out += table([{ t: '編號', w: '64px' }, '項目', { t: '實作', w: '96px' }, { t: '釐清', w: '84px' }, { t: '待釐清問題', w: '150px' }],
    R.acceptance.map((a) => ({ cells: [`<a class="ref" href="#${ctx.anchor(a.id)}">${a.id}</a>`, `${esc(a.table)}｜${esc(a.item)}`, H.st(a.impl), H.cl(a.clar), refs(ctx, a.qs)] })), { compact: true });
  for (const [g, title] of groups) {
    out += `<h2 id="${ctx.anchor(`g-${g}`)}">${esc(title)}</h2>`;
    for (const a of R.acceptance.filter((x) => x.table === g)) {
      out += `<article class="item" id="${ctx.anchor(a.id)}">
<header><span class="item-id">${a.id}</span><h3>${esc(a.item)}</h3><span class="item-area">${esc(a.area)}</span><span class="badges">${H.st(a.impl)} ${H.cl(a.clar)}</span></header>
<dl>
<dt>合約原文</dt><dd class="quote">${esc(a.contract)}</dd>
<dt>現況（energy-ops-rust）</dt><dd>${linkRefs(ctx, a.current)}</dd>
<dt>缺口</dt><dd>${linkRefs(ctx, a.gap)}</dd>
<dt>驗收方法（草案）</dt><dd>${linkRefs(ctx, a.verify)}</dd>
<dt>對應需求</dt><dd>${refs(ctx, a.fr)}</dd>
<dt>待釐清</dt><dd>${refs(ctx, a.qs)}</dd>
<dt>測試資料</dt><dd>${tdFor(ctx, a.id)}</dd>
<dt>證據</dt><dd>${a.evidence && a.evidence !== '—' ? `<code>${esc(a.evidence)}</code>` : '<span class="muted">—</span>'}</dd>
${a.note ? `<dt>備註</dt><dd>${linkRefs(ctx, a.note)}</dd>` : ''}
</dl></article>`;
    }
  }
  return out;
}
function summarize(list) {
  const c = R.countBy(list);
  return ['done', 'partial', 'todo'].filter((k) => c[k]).map((k) => `${IMPL_TEXT[k]} ${c[k]}`).join('、') || '—';
}

function requirementsPage(ctx) {
  let out = PAGES.requirementsIntro(ctx, H);
  out += tiles([
    { label: '功能需求 FR', value: '35 條', sub: summarize(R.requirements.filter((r) => r.id.startsWith('FR'))) },
    { label: '非功能需求 NFR', value: '3 條', sub: summarize(R.requirements.filter((r) => r.id.startsWith('NFR'))) },
    { label: '原表標示待確認', value: `${R.requirements.filter((r) => !/^已討論$/.test(r.confirm)).length} 條`, sub: '欄位、公式、規則、介接、報表樣式、選配' },
  ]);
  out += '<h2>需求總表（v0.1，2026-11-08）</h2>';
  out += table([{ t: '編號', w: '78px' }, { t: '模組', w: '96px' }, '需求說明', { t: '優先', w: '44px' }, { t: '原表確認狀態', w: '96px' }, '初步驗收條件', { t: '現況', w: '92px' }, { t: '對應驗收', w: '110px' }, '備註'],
    R.requirements.map((r) => ({ id: ctx.anchor(r.id), cells: [`<span class="ref">${r.id}</span>`, esc(r.module), linkRefs(ctx, r.desc), esc(r.pri), `<span class="${/已討論|驗收確認/.test(r.confirm) ? 'muted' : 'warn-text'}">${esc(r.confirm)}</span>`, esc(r.accept), H.st(r.impl), refs(ctx, r.ac), linkRefs(ctx, r.note)] })), { filter: true });
  out += '<h2 id="stories">早期設計的使用者故事（原版設計文件，2026-08）</h2>';
  out += PAGES.storiesIntro(ctx, H);
  out += table([{ t: '編號', w: '64px' }, { t: '角色', w: '96px' }, '需求', { t: '現況', w: '96px' }, '與合約的關係'],
    R.stories.map((s) => ({ id: ctx.anchor(s.id), cells: [`<span class="ref">${s.id}</span>`, esc(s.role), esc(s.need), H.st(s.impl), linkRefs(ctx, s.rel)] })));
  return out;
}

function questionsPage(ctx) {
  let out = PAGES.questionsIntro(ctx, H);
  const batches = Object.keys(R.BATCHES).map(Number);
  out += tiles(batches.map((b) => ({ label: R.BATCHES[b].title.split('：')[0], value: `${R.questions.filter((q) => q.batch === b).length} 題`, sub: esc(R.BATCHES[b].who) })));
  out += '<h2 id="summary">總覽</h2>';
  out += table([{ t: '編號', w: '64px' }, { t: '批次', w: '64px' }, { t: '主題', w: '150px' }, '問題', { t: '建議回答者', w: '130px' }, { t: '關聯', w: '160px' }, { t: '狀態', w: '64px' }],
    R.questions.map((q) => ({ cells: [`<a class="ref ref-q" href="#${ctx.anchor(q.id)}">${q.id}</a>`, `第 ${q.batch} 批`, esc(q.topic), esc(q.q), esc(q.who), refs(ctx, q.refs), '<span class="cl cl-open">未回覆</span>'] })), { filter: true, compact: true });
  for (const b of batches) {
    out += `<h2 id="${ctx.anchor(`batch-${b}`)}">${esc(R.BATCHES[b].title)}</h2><p class="muted">建議回答者：${esc(R.BATCHES[b].who)}</p>`;
    for (const q of R.questions.filter((x) => x.batch === b)) {
      out += `<article class="item q" id="${ctx.anchor(q.id)}">
<header><span class="item-id q-id">${q.id}</span><h3>${esc(q.topic)}</h3><span class="badges"><span class="cl cl-open">未回覆</span></span></header>
<p class="q-text">${linkRefs(ctx, q.q)}</p>
<dl>
<dt>為什麼要問</dt><dd><div class="why"><div class="why-row"><span class="st st-done">已有</span><ul>${(q.have || []).map((x) => `<li>${linkRefs(ctx, x)}</li>`).join('')}</ul></div><div class="why-row"><span class="st st-todo">沒有</span><ul>${(q.lack || []).map((x) => `<li>${linkRefs(ctx, x)}</li>`).join('')}</ul></div><p>${linkRefs(ctx, q.why)}</p></div></dd>
<dt>選項／建議</dt><dd>${linkRefs(ctx, q.options)}</dd>
<dt>建議回答者</dt><dd>${esc(q.who)}</dd>
<dt>關聯</dt><dd>${refs(ctx, q.refs)}</dd>
<dt>出處</dt><dd>${esc(q.from)}</dd>
<dt>回覆</dt><dd class="answer">（待填）</dd>
</dl></article>`;
    }
  }
  return out;
}

/** 某個驗收項要用到的測試資料（由 testData.need 反查）。 */
function tdFor(ctx, id) {
  const list = R.testData.filter((t) => t.need.includes(id));
  if (!list.length) return '<span class="muted">—</span>';
  return list.map((t) => `${refLink(ctx, t.id)} ${H.av(t.have)} ${esc(t.item)}`).join('<br>');
}

function testDataPage(ctx) {
  let out = PAGES.testDataIntro(ctx, H, { refLink: (id) => refLink(ctx, id) });
  const c = R.countBy(R.testData, 'have');
  out += tiles([
    { label: '已有，可用', value: `${c.yes || 0} 類`, sub: '可直接當測試輸入' },
    { label: '部分，不足或不可靠', value: `${c.partial || 0} 類`, sub: '有來源，但缺欄位、缺答案或對不上' },
    { label: '沒有', value: `${c.no || 0} 類`, sub: '需要向雲豹索取（第四批問題）' },
  ]);
  out += '<h2 id="list">測試資料清單</h2>';
  out += table([{ t: '編號', w: '70px' }, { t: '資料', w: '170px' }, { t: '現況', w: '64px' }, '來源與位置（含 branch）', '缺什麼', { t: '用於驗收', w: '130px' }, { t: '問題', w: '120px' }],
    R.testData.map((t) => ({ id: ctx.anchor(t.id), cells: [`<span class="ref">${t.id}</span>`, esc(t.item), H.av(t.have), linkRefs(ctx, t.where), linkRefs(ctx, t.gap), refs(ctx, t.need), refs(ctx, t.qs)] })), { filter: true });
  out += '<h2 id="by-acceptance">依驗收項看測試資料</h2><p class="muted">每個驗收項需要哪些測試資料、現在有沒有。任一項標「沒有」，該驗收項就無法完整測試。</p>';
  out += table([{ t: '驗收項', w: '64px' }, { t: '項目', w: '260px' }, '需要的測試資料', { t: '可測程度', w: '96px' }],
    R.acceptance.filter((a) => a.table !== '交付').map((a) => {
      const list = R.testData.filter((t) => t.need.includes(a.id));
      const worst = list.some((t) => t.have === 'no') ? 'no' : list.some((t) => t.have === 'partial') ? 'partial' : list.length ? 'yes' : null;
      const verdict = worst === 'no' ? '<span class="st st-todo">無法完整測試</span>' : worst === 'partial' ? '<span class="st st-partial">可部分測試</span>' : worst === 'yes' ? '<span class="st st-done">可測試</span>' : '<span class="muted">—</span>';
      return { cells: [refLink(ctx, a.id), esc(a.item), list.length ? list.map((t) => `${refLink(ctx, t.id)} ${H.av(t.have)} ${esc(t.item)}`).join('<br>') : '<span class="muted">—</span>', verdict] };
    }), { compact: true });
  return out;
}

function inventoryPage(ctx) {
  let out = PAGES.inventoryIntro(ctx, H);
  const c = R.countBy(R.inventory);
  out += tiles(['done', 'partial', 'beyond'].map((k) => ({ label: IMPL_TEXT[k], value: `${c[k] || 0} 個元件`, sub: esc(LEGEND_IMPL[k]) })));
  for (const layer of [...new Set(R.inventory.map((x) => x.layer))]) {
    out += `<h2>${esc(layer)}</h2>`;
    out += table([{ t: '元件', w: '180px' }, '內容', { t: '狀態', w: '96px' }, { t: '對合約的意義', w: '170px' }, { t: '證據', w: '240px' }],
      R.inventory.filter((x) => x.layer === layer).map((x) => ({ cells: [esc(x.name), esc(x.desc), H.st(x.impl), linkRefs(ctx, x.use), `<code>${esc(x.evidence)}</code>`] })));
  }
  return out;
}

function dataPage(ctx) {
  let out = PAGES.dataIntro(ctx, H);
  out += table([{ t: '編號', w: '78px' }, { t: '類型', w: '86px' }, { t: '項目', w: '150px' }, '內容', '影響', '處理建議', { t: '問題', w: '80px' }],
    R.dataIssues.map((d) => ({ id: ctx.anchor(d.id), cells: [`<span class="ref">${d.id}</span>`, H.dt(d.kind), `${esc(d.item)}<div class="where"><code>${esc(d.where)}</code></div>`, linkRefs(ctx, d.detail), linkRefs(ctx, d.impact), linkRefs(ctx, d.action), refs(ctx, d.qs)] })), { filter: true });
  return out;
}

function decisionsPage(ctx) {
  let out = PAGES.decisionsIntro(ctx, H);
  out += table([{ t: '編號', w: '74px' }, { t: '日期', w: '92px' }, { t: '議題', w: '110px' }, '決定', { t: '由誰決定', w: '96px' }, { t: '出處', w: '200px' }, { t: '需重新確認', w: '96px' }],
    R.decisions.map((d) => ({ id: ctx.anchor(d.id), cells: [`<span class="ref">${d.id}</span>`, esc(d.date), esc(d.topic), `${linkRefs(ctx, d.decision)}${d.note ? `<div class="muted">${linkRefs(ctx, d.note)}</div>` : ''}`, `<span class="by by-${byClass(d.by)}">${esc(d.by)}</span>`, esc(d.source), d.recheck ? '<span class="cl cl-open">需重新確認</span>' : '<span class="cl cl-confirmed">維持</span>'] })));
  return out;
}
const byClass = (b) => ({ 使用者決定: 'user', 客戶確認: 'client', 設計者代決: 'proxy', 文件推定: 'doc', 合約約定: 'contract' }[b] || 'doc');

function comparePage(ctx) {
  let out = PAGES.compareIntro(ctx, H);
  out += '<h2>遠端抓取結果（2026-11-12）</h2>';
  out += table([{ t: '對象', w: '200px' }, { t: '遠端', w: '190px' }, '本機', '遠端最新', '結果'],
    R.fetchResults.map((f) => ({ cells: [esc(f.repo), `<code>${esc(f.remote)}</code>`, esc(f.local), esc(f.remoteTip), linkRefs(ctx, f.result)] })));
  out += '<h2>App 5 未併修正分支 對 energy-ops-rust</h2>';
  out += PAGES.compareApp5(ctx, H);
  out += table([{ t: '#', w: '32px' }, { t: '問題', w: '180px' }, 'App 5 的修法', 'energy-ops-rust 現況', { t: '判定', w: '130px' }, { t: '證據', w: '200px' }],
    R.app5Fixes.map((f) => ({ cells: [String(f.no), esc(f.issue), esc(f.app5), esc(f.rust), `<span class="verdict v-${verdictClass(f.verdict)}">${esc(f.verdict)}</span>`, `<code>${esc(f.evidence)}</code>`] })));
  out += PAGES.compareSamples(ctx, H);
  return out;
}
const verdictClass = (v) => (/高度疑似|疑似同樣/.test(v) ? 'bad' : /部分|疑似/.test(v) ? 'mid' : 'ok');

function archPage(ctx, id) {
  const d = diagrams.find((x) => x.id === id);
  const meta = ARCH[id];
  let out = meta.intro(ctx, H);
  out += diagramFigure(ctx, id);
  if (meta.points) out += `<h2>重點</h2><ul class="points">${meta.points.map((p) => `<li>${linkRefs(ctx, p)}</li>`).join('')}</ul>`;
  const nodes = d.items.filter((it) => it.kind === 'node');
  const stats = R.countBy(nodes.map((n) => ({ impl: n.impl || 'todo' })));
  out += `<h2>圖中元素一覽</h2><p class="muted">由圖的定義自動產生（tools/diagrams.mjs），與圖面一致。共 ${nodes.length} 個元素：${Object.keys(IMPL).filter((k) => stats[k]).map((k) => `${IMPL_TEXT[k]} ${stats[k]}`).join('、')}。</p>`;
  out += table([{ t: '元素', w: '200px' }, '說明', { t: '實作', w: '96px' }, { t: '釐清', w: '84px' }, { t: '關聯', w: '170px' }],
    nodes.map((n) => ({ cells: [esc(n.title), n.sub ? linkRefs(ctx, n.sub.replace(/\n+/g, ' ')) : '<span class="muted">—</span>', `${H.st(n.impl || 'todo')}${n.data ? ` ${H.dt(n.data)}` : ''}`, H.cl(n.clar || 'confirmed'), refs(ctx, (n.tags || []).filter((t) => /^(Q-|B\d|R0|D0|FR-|NFR-)/.test(t)))] })), { compact: true });
  const qs = [...new Set(nodes.flatMap((n) => (n.tags || []).filter((t) => t.startsWith('Q-'))))].sort();
  if (qs.length) {
    out += `<h2>這一層的待釐清問題</h2><ul class="qlist">${qs.map((q) => { const x = R.questions.find((y) => y.id === q); return `<li>${refLink(ctx, q)} <strong>${esc(x?.topic || '')}</strong>：${esc(x?.q || '')}</li>`; }).join('')}</ul>`;
  }
  return out;
}

// ───────────────────────── 頁面清單與導覽 ─────────────────────────
const SITE = [
  { section: '首頁', pages: [{ path: 'index.html', title: '首頁', h1: `${PRODUCT}　規格文件`, render: (ctx) => PAGES.home(ctx, H, { tiles, coverageBar, legend, refLink: (id) => refLink(ctx, id) }) }] },
  { section: '00 總覽', pages: [
    { path: '00-總覽/01-專案摘要.html', title: '專案摘要', render: (ctx) => PAGES.summary(ctx, H, { tiles, coverageBar, linkRefs: (t) => linkRefs(ctx, t), refLink: (id) => refLink(ctx, id) }) },
    { path: '00-總覽/02-規格來源與世代.html', title: '規格來源與世代', render: (ctx) => PAGES.sources(ctx, H, { table, linkRefs: (t) => linkRefs(ctx, t) }) },
    { path: '00-總覽/03-圖例與編號規則.html', title: '圖例與編號規則', render: (ctx) => PAGES.legendPage(ctx, H, { legend, table }) },
    { path: '00-總覽/04-名詞表.html', title: '名詞表', render: (ctx) => PAGES.glossary(ctx, H, { table }) },
  ] },
  { section: '10 架構（全局→細節）', pages: Object.keys(ARCH).map((id) => ({ path: archPath(id), title: `${id} ${ARCH[id].short}`, render: (ctx) => archPage(ctx, id) })) },
  { section: '20 驗收', pages: [{ path: '20-驗收/合約驗收對照表.html', title: '合約驗收對照表', render: acceptancePage }] },
  { section: '30 需求', pages: [{ path: '30-需求/需求總表.html', title: '需求總表與使用者故事', render: requirementsPage }] },
  { section: '40 清冊', pages: [
    { path: '40-清冊/待釐清問題.html', title: '待釐清問題（分四批）', render: questionsPage },
    { path: '40-清冊/測試資料盤點.html', title: '測試資料盤點', render: testDataPage },
    { path: '40-清冊/實作現況.html', title: '實作現況清冊', render: inventoryPage },
    { path: '40-清冊/假資料與不確定資料.html', title: '假資料與不確定資料', render: dataPage },
    { path: '40-清冊/決策紀錄.html', title: '決策紀錄', render: decisionsPage },
    { path: '40-清冊/程式碼比對紀錄.html', title: '程式碼比對紀錄', render: comparePage },
  ] },
];

function nav(ctx) {
  return SITE.map((s) => `<div class="nav-sec"><div class="nav-h">${esc(s.section)}</div><ul>${s.pages.map((p) => `<li${p.path === ctx.current ? ' class="on"' : ''}><a href="${ctx.href(p.path)}">${esc(p.title)}</a></li>`).join('')}</ul></div>`).join('');
}

function shell(ctx, page, body, { single = false, navHtml = '' } = {}) {
  const css = readFileSync(join(ROOT, 'tools', 'site.css'), 'utf8');
  const js = readFileSync(join(ROOT, 'tools', 'site.js'), 'utf8');
  const sec = SITE.find((s) => s.pages.includes(page))?.section || '';
  const main = single ? body : `<div class="crumb">${esc(sec)}</div><h1>${esc(page.h1 || page.title)}</h1>\n${body}`;
  return `<!doctype html>
<html lang="zh-Hant">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta name="color-scheme" content="light">
<meta name="generator" content="docs/spec/tools/build.mjs">
<title>${esc(single ? `${PRODUCT} 規格文件（單檔版）` : `${page.title}｜${PRODUCT} 規格文件`)}</title>
<style>${css}</style>
</head>
<body>
<header class="top"><div class="brand">${esc(PRODUCT)}<span>規格文件</span></div><div class="meta">${esc(VERSION)} 盤點版 · ${esc(DATE)} · 以 energy-ops-rust 為主</div></header>
<div class="wrap">
<nav class="side" aria-label="文件目錄">${navHtml || nav(ctx)}</nav>
<main class="doc">
${main}
<footer class="foot">本頁由 <code>docs/spec/tools/build.mjs</code> 產生，內容來源為 <code>docs/spec/src/</code>；請改來源後重新產生，不要直接改 HTML。${esc(VERSION)}・${esc(DATE)}</footer>
</main>
</div>
<div class="zoom" hidden><div class="zoom-bar"><span>拖曳或捲動檢視，按 Esc 關閉</span><button type="button" data-zoom-close>關閉</button></div><div class="zoom-body"><img alt=""></div></div>
<script>${js}</script>
</body>
</html>
`;
}

// ───────────────────────── 產出 ─────────────────────────
const n = writeDiagrams();
let count = 0;
for (const s of SITE) {
  for (const p of s.pages) {
    const ctx = new Ctx('multi', p.path);
    const html = shell(ctx, p, p.render(ctx));
    const out = join(ROOT, p.path);
    mkdirSync(dirname(out), { recursive: true });
    writeFileSync(out, html);
    count += 1;
  }
}
// 單檔版：每頁一個 section，導覽改成頁內錨點，圖改成內嵌。
{
  const sections = [];
  const navItems = [];
  for (const s of SITE) {
    navItems.push(`<div class="nav-sec"><div class="nav-h">${esc(s.section)}</div><ul>`);
    for (const p of s.pages) {
      const ctx = new Ctx('single', p.path);
      navItems.push(`<li><a href="#${anchorOf(p.path)}">${esc(p.title)}</a></li>`);
      sections.push(`<section class="page" id="${anchorOf(p.path)}"><div class="crumb">${esc(s.section)}</div><h1>${esc(p.h1 || p.title)}</h1>\n${p.render(ctx)}</section>`);
    }
    navItems.push('</ul></div>');
  }
  const ctx0 = new Ctx('single', 'index.html');
  const html = shell(ctx0, SITE[0].pages[0], sections.join('\n'), { single: true, navHtml: navItems.join('') });
  mkdirSync(join(ROOT, 'dist'), { recursive: true });
  writeFileSync(join(ROOT, 'dist', `energy-ops-spec-${VERSION}.html`), html);
}
console.log(`已產生 ${n} 張圖、${count} 個頁面，以及 dist/energy-ops-spec-${VERSION}.html（截止日 ${DEADLINE}）`);
