#!/usr/bin/env node
// 把單檔版輸出成 PDF：node docs/spec/tools/pdf.mjs [輸出路徑]
// 先跑 build.mjs 產生 dist/energy-ops-spec-<版本>.html。PDF 用 web/ 已有的 puppeteer（cd web && npm install）。
// A4 橫向；頁首是文件名與版本，頁尾是頁碼；架構圖以向量輸出，放大不糊。PDF 不進版控（見 docs/spec/.gitignore）。
import { existsSync } from 'node:fs';
import { createRequire } from 'node:module';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { DATE, VERSION } from './lib.mjs';

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..');
const require = createRequire(join(ROOT, '..', '..', 'web', 'package.json'));
const puppeteer = require('puppeteer');

const src = join(ROOT, 'dist', `energy-ops-spec-${VERSION}.html`);
const out = resolve(process.argv[2] || join(ROOT, 'dist', `energy-ops-spec-${VERSION}.pdf`));
if (!existsSync(src)) {
  console.error(`找不到 ${src}，請先執行 node docs/spec/tools/build.mjs`);
  process.exit(1);
}

const FONT = "'PingFang TC','Noto Sans TC','Microsoft JhengHei',sans-serif";
const header = `<div style="font-family:${FONT};font-size:8px;color:#6b7280;width:100%;padding:0 12mm;display:flex;justify-content:space-between;">
<span>電費單與租金管理系統　規格文件</span><span>${VERSION} 盤點版・${DATE}・以 energy-ops-rust 為主</span></div>`;
const footer = `<div style="font-family:${FONT};font-size:8px;color:#6b7280;width:100%;padding:0 12mm;display:flex;justify-content:space-between;">
<span>機敏資料未轉載；原始檔案不在本文件內</span><span>第 <span class="pageNumber"></span> 頁，共 <span class="totalPages"></span> 頁</span></div>`;

const browser = await puppeteer.launch({ headless: true });
try {
  const page = await browser.newPage();
  await page.setViewport({ width: 1200, height: 900 });
  await page.goto(pathToFileURL(src).href, { waitUntil: 'load' });
  // 圖片預設延遲載入；輸出前全部改成立即載入並等它們解碼完。
  await page.evaluate(async () => {
    const imgs = [...document.images];
    imgs.forEach((i) => { i.loading = 'eager'; });
    await Promise.all(imgs.map((i) => (i.complete && i.naturalWidth ? Promise.resolve() : i.decode().catch(() => {}))));
  });
  await page.emulateMediaType('print');
  await page.pdf({
    path: out,
    format: 'A4',
    landscape: true,
    printBackground: true,
    displayHeaderFooter: true,
    headerTemplate: header,
    footerTemplate: footer,
    margin: { top: '15mm', bottom: '14mm', left: '12mm', right: '12mm' },
    outline: true,
    tagged: true,
  });
  console.log(`已輸出 ${out}`);
} finally {
  await browser.close();
}
