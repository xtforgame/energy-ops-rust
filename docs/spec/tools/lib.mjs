// 規格文件的共用設定與 SVG 畫圖小工具（零相依）。
// 狀態的顏色、邊框、標記只定義在這裡：圖（diagrams.mjs）與網頁（build.mjs）都讀這一份，兩邊不會各說各話。

export const VERSION = 'v0.1';
export const DATE = '2026-11-12';
export const FONT = "'PingFang TC','Noto Sans TC','Microsoft JhengHei','Heiti TC',sans-serif";

/** 實作狀態（填色）：對照合約與需求，energy-ops-rust 現在做到哪。 */
export const IMPL = {
  done: { fill: '#E3F1E6', stroke: '#2F6B41', text: '#173A22', label: '已具備' },
  partial: { fill: '#FFF1CC', stroke: '#B7791F', text: '#4A3208', label: '部分具備' },
  todo: { fill: '#FFFFFF', stroke: '#6B7280', text: '#1F2937', label: '未實作' },
  beyond: { fill: '#E2EBF6', stroke: '#3A5F8A', text: '#1B3150', label: '合約外既有' },
  external: { fill: '#EEE9F7', stroke: '#5B4A96', text: '#2A2150', label: '外部系統' },
  dropped: { fill: '#F1F1F1', stroke: '#A3A3A3', text: '#6B6B6B', label: '不採用／範圍外' },
};

/** 釐清狀態（邊框）：規格本身定了沒有。 */
export const OPEN = '#C2410C';
export const CLAR = {
  confirmed: { dash: null, width: 1.6, label: '已確認' },
  assumed: { dash: '7 4', width: 2, label: '預設／假設' },
  open: { dash: '1.5 3.5', width: 2.6, label: '待釐清' },
};

/** 資料標記。 */
export const DATA = {
  mock: { label: '假資料／mock', tag: 'MOCK' },
  uncertain: { label: '資料不確定', tag: '不確定' },
};

export const esc = (s) => String(s)
  .replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');

const CJK = /[⺀-鿿豈-﫿＀-￯　-〿]/;
const NO_LINE_START = /^[，。、；：）」』！？…％%,.;:)\]]$/;

function charW(ch, fs) {
  if (CJK.test(ch)) return fs;
  if (ch === ' ') return fs * 0.32;
  if (/[A-Z0-9]/.test(ch)) return fs * 0.62;
  if (/[il.,:;'|!()[\]]/.test(ch)) return fs * 0.32;
  return fs * 0.55;
}
export function textW(s, fs) {
  let w = 0;
  for (const ch of String(s)) w += charW(ch, fs);
  return w;
}

/** 依寬度斷行：中文逐字可斷，英數整個字不拆；標點不放行首。 */
export function wrap(text, maxW, fs) {
  const out = [];
  for (const para of String(text).split('\n')) {
    const tokens = para.match(/[⺀-鿿豈-﫿＀-￯　-〿]|[^\s⺀-鿿豈-﫿＀-￯　-〿]+|\s+/g) || [''];
    let line = '';
    let w = 0;
    for (const t of tokens) {
      const tw = textW(t, fs);
      if (w + tw > maxW && line.trim()) {
        if (NO_LINE_START.test(t)) { out.push(line + t); line = ''; w = 0; continue; }
        out.push(line.trimEnd());
        line = t.trimStart();
        w = textW(line, fs);
      } else {
        line += t;
        w += tw;
      }
    }
    out.push(line.trimEnd());
  }
  return out;
}

const EDGE = {
  done: { c: '#4B5563', dash: null },
  partial: { c: '#B7791F', dash: null },
  todo: { c: '#6B7280', dash: '6 4' },
  beyond: { c: '#3A5F8A', dash: null },
  external: { c: '#5B4A96', dash: '6 4' },
  dropped: { c: '#A3A3A3', dash: '4 4' },
};

function tagStyle(t) {
  if (/^Q-/.test(t)) return { fill: '#FFF1E6', stroke: OPEN, text: OPEN };
  if (t === 'MOCK') return { fill: '#F3F4F6', stroke: '#6B7280', text: '#374151', hatch: true };
  if (t === '不確定') return { fill: '#FEF3C7', stroke: '#B45309', text: '#92400E' };
  return { fill: '#F3F4F6', stroke: '#9CA3AF', text: '#374151' };
}

export class Diagram {
  constructor({ id, title, subtitle, basis, width, height }) {
    Object.assign(this, { id, title, subtitle, basis, width, height });
    this.items = [];
    this.byId = new Map();
    this.edges = [];
    this.warnings = [];
  }

  group(o) { this.items.push({ kind: 'group', ...o }); if (o.id) this.byId.set(o.id, o); return o; }
  node(o) { const n = { kind: 'node', ...o }; this.items.push(n); this.byId.set(o.id, n); return n; }
  note(o) { this.items.push({ kind: 'note', ...o }); }
  edge(o) { this.edges.push(o); }

  /** 一排等寬的節點；items 只寫每格不同的部分。 */
  row({ x, y, w, h, gap = 16, items, ...common }) {
    items.forEach((it, i) => this.node({ x: x + i * (w + gap), y, w, h, ...common, ...it }));
  }

  chain(ids, opts = {}) {
    for (let i = 0; i < ids.length - 1; i++) this.edge({ from: ids[i], to: ids[i + 1], ...opts });
  }

  legend(x, y, w) { this.legendAt = { x, y, w }; }

  stats() {
    const s = {};
    for (const it of this.items) if (it.kind === 'node') s[it.impl || 'todo'] = (s[it.impl || 'todo'] || 0) + 1;
    return s;
  }

  render() {
    const W = this.width;
    const H = this.height;
    const parts = [];
    const labels = [];
    const markers = new Set();
    parts.push(`<rect x="0" y="0" width="${W}" height="${H}" fill="#FFFFFF"/>`);
    parts.push(this.renderHeader());
    for (const it of this.items) if (it.kind === 'group') parts.push(this.renderGroup(it));
    for (const e of this.edges) {
      const r = this.renderEdge(e);
      parts.push(r.path);
      if (r.label) labels.push(r.label);
      markers.add(r.color);
    }
    for (const it of this.items) {
      if (it.kind === 'node') parts.push(this.renderNode(it));
      if (it.kind === 'note') parts.push(this.renderNote(it));
    }
    parts.push(...labels);
    if (this.legendAt) parts.push(this.renderLegend(this.legendAt));
    const defs = [
      '<pattern id="hatch" width="8" height="8" patternUnits="userSpaceOnUse" patternTransform="rotate(45)">'
        + '<line x1="0" y1="0" x2="0" y2="8" stroke="#6B7280" stroke-width="2" stroke-opacity="0.28"/></pattern>',
      ...[...markers].map((c) => `<marker id="ah-${c.slice(1)}" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse"><path d="M0,0 L10,5 L0,10 z" fill="${c}"/></marker>`),
    ];
    for (const w of this.warnings) console.warn(`[${this.id}] ${w}`);
    return `<svg xmlns="http://www.w3.org/2000/svg" width="${W}" height="${H}" viewBox="0 0 ${W} ${H}" font-family="${esc(FONT)}">\n`
      + `<title>${esc(`${this.id} ${this.title}`)}</title>\n<defs>${defs.join('')}</defs>\n${parts.join('\n')}\n</svg>\n`;
  }

  renderHeader() {
    const W = this.width;
    const sub = wrap(this.subtitle || '', W - 72, 13);
    const metaY = 74 + sub.length * 18;
    const meta = wrap(`${VERSION} · ${DATE}｜依據：${this.basis || ''}`, W - 72, 11.5);
    return [
      `<text x="36" y="48" font-size="24" font-weight="700" fill="#111827">${esc(`${this.id}　${this.title}`)}</text>`,
      ...sub.map((l, i) => `<text x="36" y="${74 + i * 18}" font-size="13" fill="#4B5563">${esc(l)}</text>`),
      ...meta.map((l, i) => `<text x="36" y="${metaY + i * 15}" font-size="11.5" fill="#6B7280">${esc(l)}</text>`),
    ].join('\n');
  }

  renderGroup(g) {
    const open = g.clar === 'open';
    const stroke = open ? OPEN : (g.stroke || '#CBD5E1');
    const dash = open ? ` stroke-dasharray="${CLAR.open.dash}"` : (g.clar === 'assumed' ? ` stroke-dasharray="${CLAR.assumed.dash}"` : '');
    const out = [`<rect x="${g.x}" y="${g.y}" width="${g.w}" height="${g.h}" rx="12" fill="${g.fill || '#F8FAFC'}" stroke="${stroke}" stroke-width="${open ? 2.2 : 1.3}"${dash}/>`];
    out.push(`<text x="${g.x + 14}" y="${g.y + 24}" font-size="14.5" font-weight="700" fill="${g.titleColor || '#334155'}">${esc(g.title)}</text>`);
    if (g.sub) {
      wrap(g.sub, g.w - 28, 11.5).forEach((l, i) => out.push(`<text x="${g.x + 14}" y="${g.y + 42 + i * 15}" font-size="11.5" fill="#64748B">${esc(l)}</text>`));
    }
    if (g.tags) out.push(this.renderTags(g.tags, g.x + g.w - 14, g.y + 10, 'right'));
    return out.join('\n');
  }

  renderTags(tags, x, y, align = 'left') {
    const fs = 10.5;
    const pills = tags.map((t) => ({ t, w: textW(t, fs) + 12, s: tagStyle(t) }));
    let cx = align === 'right' ? x - pills.reduce((a, p) => a + p.w + 4, -4) : x;
    const out = [];
    for (const p of pills) {
      out.push(`<rect x="${cx}" y="${y}" width="${p.w}" height="16" rx="8" fill="${p.s.fill}" stroke="${p.s.stroke}" stroke-width="0.9"/>`);
      if (p.s.hatch) out.push(`<rect x="${cx}" y="${y}" width="${p.w}" height="16" rx="8" fill="url(#hatch)"/>`);
      out.push(`<text x="${cx + p.w / 2}" y="${y + 11.8}" text-anchor="middle" font-size="${fs}" font-weight="600" fill="${p.s.text}">${esc(p.t)}</text>`);
      cx += p.w + 4;
    }
    return out.join('');
  }

  renderNode(n) {
    const S = IMPL[n.impl || 'todo'];
    const clar = n.clar || 'confirmed';
    const C = CLAR[clar];
    const stroke = clar === 'open' ? OPEN : S.stroke;
    const dash = C.dash ? ` stroke-dasharray="${C.dash}"` : '';
    const { x, y, w, h } = n;
    const shape = n.shape || 'rect';
    const out = [];
    const sw = C.width;
    if (shape === 'db') {
      const ry = 9;
      out.push(`<path d="M${x},${y + ry} A${w / 2},${ry} 0 0 1 ${x + w},${y + ry} L${x + w},${y + h - ry} A${w / 2},${ry} 0 0 1 ${x},${y + h - ry} Z" fill="${S.fill}" stroke="${stroke}" stroke-width="${sw}"${dash}/>`);
      out.push(`<path d="M${x},${y + ry} A${w / 2},${ry} 0 0 0 ${x + w},${y + ry}" fill="none" stroke="${stroke}" stroke-width="${sw}"${dash}/>`);
    } else if (shape === 'doc') {
      const f = 16;
      out.push(`<path d="M${x},${y} L${x + w - f},${y} L${x + w},${y + f} L${x + w},${y + h} L${x},${y + h} Z" fill="${S.fill}" stroke="${stroke}" stroke-width="${sw}"${dash}/>`);
      out.push(`<path d="M${x + w - f},${y} L${x + w - f},${y + f} L${x + w},${y + f}" fill="none" stroke="${stroke}" stroke-width="1"/>`);
    } else {
      out.push(`<rect x="${x}" y="${y}" width="${w}" height="${h}" rx="8" fill="${S.fill}" stroke="${stroke}" stroke-width="${sw}"${dash}/>`);
    }
    if (n.data === 'mock') out.push(`<rect x="${x + 1}" y="${y + 1}" width="${w - 2}" height="${h - 2}" rx="7" fill="url(#hatch)"/>`);

    let tx = x + 12;
    let tw = w - 24;
    if (shape === 'actor') {
      const cx = x + 26;
      const cy = y + h / 2;
      out.push(`<circle cx="${cx}" cy="${cy - 10}" r="7.5" fill="none" stroke="${S.stroke}" stroke-width="1.8"/>`);
      out.push(`<path d="M${cx - 12},${cy + 14} Q${cx - 12},${cy + 1} ${cx},${cy + 1} Q${cx + 12},${cy + 1} ${cx + 12},${cy + 14}" fill="none" stroke="${S.stroke}" stroke-width="1.8"/>`);
      tx = x + 50;
      tw = w - 60;
    }
    const tfs = n.titleFs || (n.small ? 12 : 14);
    const sfs = n.subFs || (n.small ? 10.5 : 11.5);
    const tLines = wrap(n.title, tw, tfs);
    const sLines = n.sub ? wrap(n.sub, tw, sfs) : [];
    const tags = [...(n.tags || [])];
    if (n.data === 'mock') tags.push('MOCK');
    if (n.data === 'uncertain') tags.push('不確定');
    const textH = tLines.length * tfs * 1.28 + (sLines.length ? 3 + sLines.length * sfs * 1.36 : 0);
    const tagH = tags.length ? 22 : 0;
    const top = shape === 'db' ? 16 : 9;
    if (textH + tagH + top + 6 > h) this.warnings.push(`節點 ${n.id} 文字放不下（需要 ${Math.ceil(textH + tagH + top + 6)}，高 ${h}）`);
    let cy = n.center ? y + (h - tagH - textH) / 2 + tfs * 0.95 : y + top + tfs;
    const anchor = n.align === 'center' ? 'middle' : 'start';
    const ax = n.align === 'center' ? x + w / 2 : tx;
    for (const l of tLines) { out.push(`<text x="${ax}" y="${cy}" text-anchor="${anchor}" font-size="${tfs}" font-weight="650" fill="${S.text}">${esc(l)}</text>`); cy += tfs * 1.28; }
    cy += 3;
    for (const l of sLines) { out.push(`<text x="${ax}" y="${cy - 2}" text-anchor="${anchor}" font-size="${sfs}" fill="${n.impl === 'dropped' ? '#7B7B7B' : '#4B5563'}">${esc(l)}</text>`); cy += sfs * 1.36; }
    if (tags.length) out.push(this.renderTags(tags, x + 10, y + h - 22));
    if (n.chip !== false) {
      const label = S.label + (clar === 'assumed' ? '·預設' : clar === 'open' ? '·待釐清' : '');
      const cw = textW(label, 10) + 12;
      const col = clar === 'open' ? OPEN : S.stroke;
      out.push(`<rect x="${x + w - cw - 8}" y="${y - 8}" width="${cw}" height="16" rx="8" fill="#FFFFFF" stroke="${col}" stroke-width="1"/>`);
      out.push(`<text x="${x + w - cw / 2 - 8}" y="${y + 3.6}" text-anchor="middle" font-size="10" font-weight="600" fill="${col}">${esc(label)}</text>`);
    }
    return `<g data-id="${esc(n.id)}" data-impl="${n.impl || 'todo'}" data-clar="${clar}">${out.join('')}</g>`;
  }

  renderNote(o) {
    const tones = { warn: ['#FFF7ED', OPEN, '#7C2D12'], info: ['#F1F5F9', '#94A3B8', '#334155'] };
    const [fill, stroke, color] = tones[o.tone || 'info'];
    const fs = o.fs || 12;
    const lines = wrap(o.text, o.w - 20, fs);
    const h = o.h || lines.length * fs * 1.45 + 16;
    const out = [`<rect x="${o.x}" y="${o.y}" width="${o.w}" height="${h}" rx="8" fill="${fill}" stroke="${stroke}" stroke-width="1"/>`];
    lines.forEach((l, i) => out.push(`<text x="${o.x + 10}" y="${o.y + 8 + fs + i * fs * 1.45}" font-size="${fs}" fill="${color}"${o.bold && i === 0 ? ' font-weight="700"' : ''}>${esc(l)}</text>`));
    return out.join('');
  }

  anchor(n, side, at = 0.5) {
    if (side === 'l') return [n.x, n.y + n.h * at];
    if (side === 'r') return [n.x + n.w, n.y + n.h * at];
    if (side === 't') return [n.x + n.w * at, n.y];
    return [n.x + n.w * at, n.y + n.h];
  }

  renderEdge(e) {
    const A = this.byId.get(e.from);
    const B = this.byId.get(e.to);
    if (!A || !B) throw new Error(`[${this.id}] 連線找不到節點：${e.from} → ${e.to}`);
    let [fs, ts] = [e.fromSide, e.toSide];
    if (!fs || !ts) {
      const ax = A.x + A.w / 2; const ay = A.y + A.h / 2; const bx = B.x + B.w / 2; const by = B.y + B.h / 2;
      const yOverlap = Math.min(A.y + A.h, B.y + B.h) - Math.max(A.y, B.y);
      const xOverlap = Math.min(A.x + A.w, B.x + B.w) - Math.max(A.x, B.x);
      let auto;
      if (yOverlap > 10 || (xOverlap <= 0 && Math.abs(bx - ax) >= Math.abs(by - ay))) auto = bx >= ax ? ['r', 'l'] : ['l', 'r'];
      else auto = by >= ay ? ['b', 't'] : ['t', 'b'];
      fs = fs || auto[0]; ts = ts || auto[1];
    }
    const p0 = this.anchor(A, fs, e.fromAt ?? 0.5);
    const p1 = this.anchor(B, ts, e.toAt ?? 0.5);
    let pts;
    if (e.points) pts = [p0, ...e.points, p1];
    else if (e.route === 'v') { const my = e.mid ?? (p0[1] + p1[1]) / 2; pts = [p0, [p0[0], my], [p1[0], my], p1]; }
    else if (e.route === 'h') { const mx = e.mid ?? (p0[0] + p1[0]) / 2; pts = [p0, [mx, p0[1]], [mx, p1[1]], p1]; }
    else pts = [p0, p1];
    const st = EDGE[e.impl || 'done'];
    const color = e.clar === 'open' ? OPEN : st.c;
    const dash = e.clar === 'open' ? '2 4' : st.dash;
    const d = pts.map((p, i) => `${i ? 'L' : 'M'}${p[0].toFixed(1)},${p[1].toFixed(1)}`).join(' ');
    const mk = `url(#ah-${color.slice(1)})`;
    const path = `<path d="${d}" fill="none" stroke="${color}" stroke-width="${e.width || 1.6}"${dash ? ` stroke-dasharray="${dash}"` : ''} marker-end="${mk}"${e.both ? ` marker-start="${mk}"` : ''}/>`;
    let label = '';
    if (e.label) {
      let lx; let ly;
      if (e.labelAt) [lx, ly] = e.labelAt;
      else {
        let best = 0; let bi = 0;
        for (let i = 0; i < pts.length - 1; i++) {
          const len = Math.hypot(pts[i + 1][0] - pts[i][0], pts[i + 1][1] - pts[i][1]);
          if (len > best) { best = len; bi = i; }
        }
        lx = (pts[bi][0] + pts[bi + 1][0]) / 2; ly = (pts[bi][1] + pts[bi + 1][1]) / 2;
      }
      const fs2 = 11;
      const lines = wrap(e.label, e.labelW || 190, fs2);
      const lw = Math.max(...lines.map((l) => textW(l, fs2))) + 10;
      const lh = lines.length * fs2 * 1.3 + 6;
      label = `<rect x="${lx - lw / 2}" y="${ly - lh / 2}" width="${lw}" height="${lh}" rx="4" fill="#FFFFFF" fill-opacity="0.93" stroke="${color}" stroke-width="0.6"/>`
        + lines.map((l, i) => `<text x="${lx}" y="${ly - lh / 2 + 3 + fs2 + i * fs2 * 1.3 - 1}" text-anchor="middle" font-size="${fs2}" fill="${color === OPEN ? OPEN : '#374151'}">${esc(l)}</text>`).join('');
    }
    return { path, label, color };
  }

  renderLegend({ x, y, w }) {
    const out = [`<rect x="${x}" y="${y}" width="${w}" height="66" rx="8" fill="#FFFFFF" stroke="#D1D5DB"/>`];
    let cx = x + 14;
    const r1 = y + 22;
    out.push(`<text x="${cx}" y="${r1 + 4}" font-size="12" font-weight="700" fill="#374151">實作狀態（填色）</text>`);
    cx += 112;
    for (const k of Object.keys(IMPL)) {
      const s = IMPL[k];
      out.push(`<rect x="${cx}" y="${r1 - 8}" width="26" height="16" rx="4" fill="${s.fill}" stroke="${s.stroke}" stroke-width="1.4"/>`);
      out.push(`<text x="${cx + 32}" y="${r1 + 4}" font-size="11.5" fill="#374151">${esc(s.label)}</text>`);
      cx += 32 + textW(s.label, 11.5) + 18;
    }
    cx += 8;
    out.push(`<text x="${cx}" y="${r1 + 4}" font-size="12" font-weight="700" fill="#374151">釐清狀態（邊框）</text>`);
    cx += 112;
    for (const k of Object.keys(CLAR)) {
      const c = CLAR[k];
      const col = k === 'open' ? OPEN : '#4B5563';
      out.push(`<rect x="${cx}" y="${r1 - 8}" width="26" height="16" rx="4" fill="#FFFFFF" stroke="${col}" stroke-width="${c.width}"${c.dash ? ` stroke-dasharray="${c.dash}"` : ''}/>`);
      out.push(`<text x="${cx + 32}" y="${r1 + 4}" font-size="11.5" fill="#374151">${esc(c.label)}</text>`);
      cx += 32 + textW(c.label, 11.5) + 18;
    }
    const r2 = y + 48;
    cx = x + 14;
    out.push(`<text x="${cx}" y="${r2 + 4}" font-size="12" font-weight="700" fill="#374151">標記與連線</text>`);
    cx += 112;
    const tagDemo = [['B01', '合約驗收項（表一 B、表二 R）'], ['FR-012', '需求總表條目'], ['Q-05', '待釐清問題編號'], ['MOCK', '假資料／mock 實作'], ['不確定', '資料或範例不可靠']];
    for (const [t, l] of tagDemo) {
      out.push(this.renderTags([t], cx, r2 - 8));
      cx += textW(t, 10.5) + 18;
      out.push(`<text x="${cx}" y="${r2 + 4}" font-size="11.5" fill="#374151">${esc(l)}</text>`);
      cx += textW(l, 11.5) + 18;
    }
    const lines = [['done', '已具備的連線'], ['todo', '未實作的連線'], ['open', '待釐清的連線']];
    for (const [k, l] of lines) {
      const col = k === 'open' ? OPEN : EDGE[k].c;
      const dash = k === 'open' ? '2 4' : EDGE[k].dash;
      out.push(`<line x1="${cx}" y1="${r2}" x2="${cx + 30}" y2="${r2}" stroke="${col}" stroke-width="1.8"${dash ? ` stroke-dasharray="${dash}"` : ''}/>`);
      out.push(`<text x="${cx + 36}" y="${r2 + 4}" font-size="11.5" fill="#374151">${esc(l)}</text>`);
      cx += 36 + textW(l, 11.5) + 18;
    }
    if (cx > x + w) this.warnings.push(`圖例超出寬度（${Math.round(cx - x)} > ${w}）`);
    return out.join('\n');
  }
}
