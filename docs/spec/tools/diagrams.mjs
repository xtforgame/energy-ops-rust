#!/usr/bin/env node
// 圖即程式：node docs/spec/tools/diagrams.mjs → docs/spec/diagrams/A0～A7.svg
// 每個方塊的狀態（impl 實作、clar 釐清、data 資料）與標記（B／R 驗收項、FR 需求、Q 問題）都寫在這裡；
// 改了狀態就重跑，再跑 build.mjs 更新 index.html。
import { mkdirSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { Diagram } from './lib.mjs';
import { acceptance, countBy } from '../src/registers.mjs';

const OUT = join(dirname(fileURLToPath(import.meta.url)), '..', 'diagrams');
/** 全部的圖（build.mjs 也讀它來產生「圖中元素一覽」）。 */
export const diagrams = [];
const make = (o) => { const d = new Diagram(o); diagrams.push(d); return d; };
const t1 = countBy(acceptance.filter((a) => a.table === '表一'));
const t2 = countBy(acceptance.filter((a) => a.table === '表二'));
const cover = (c) => ['done', 'partial', 'todo'].map((k) => `${{ done: '已具備', partial: '部分', todo: '未實作' }[k]} ${c[k] || 0}`).join('、');

// ───────────────────────────── A0 系統情境 ─────────────────────────────
{
  const d = make({
    id: 'A0', title: '系統情境圖（誰用、跟誰往來）', width: 1680, height: 1090,
    subtitle: '最外層：系統在雲豹的位置。中間是我們要交付的系統，左邊是使用者，右邊是外部系統與往來對象；每條線標著它對應的合約驗收項。',
    basis: '委託開發契約書附件一（表一、表二）、電費單付款申請及開立發票 SOP、需求總表 v0.1',
  });
  d.group({ id: 'gUsers', x: 30, y: 120, w: 340, h: 660, title: '雲豹能源內部使用者', sub: '角色對應為暫定（Q-30）' });
  const actors = [
    ['a1', '總務／電費單承辦', '收件、對電號、歸檔、填 EasyFlow（SOP 執行者）'],
    ['a2', '各子公司財會窗口', '依子公司分工收單據、處理付款'],
    ['a3', '會計／財務', '發票開立核准、付款'],
    ['a4', '資產管理', '地主、租約、租金、履保、保險'],
    ['a5', '簽核主管', 'EasyFlow 加簽'],
    ['a6', 'IT／系統管理員', '主機、帳號、信箱、公槽權限'],
  ];
  actors.forEach(([id, title, sub], i) => d.node({ id, x: 50, y: 178 + i * 98, w: 300, h: 84, shape: 'actor', title, sub, impl: 'external', chip: false }));
  d.node({
    id: 'sys', x: 560, y: 300, w: 480, h: 300, impl: 'partial', titleFs: 19,
    title: '電費單與租金管理系統',
    sub: `以 energy-ops-rust 為主（Rust 單一執行檔＋Postgres＋MinIO＋React 前端），部署在雲豹內網 Windows Server（離線安裝包）。\n\n已具備：手動上傳、辨識、校對、例外件、稽核、權限、系統內簽核。\n未實作：自動收件、未收單警示、天能轉供單、電費紀錄單、EasyFlow 申請單、發票存檔、租金與資產管理、報表。\n\n合約驗收覆蓋：表一 15 項（${cover(t1)}）；表二 8 項（${cover(t2)}）。`,
  });
  d.node({ id: 'ocr', x: 640, y: 690, w: 400, h: 100, impl: 'partial', data: 'uncertain', tags: ['B04–B08', 'Q-15'], title: 'OCR 辨識服務（taipower-recognition）', sub: '外部交付物，隨離線包以容器部署；4 份 PDF 範本 54 項已驗證，影像辨識仍有誤' });
  d.node({ id: 'tp', x: 1190, y: 112, w: 215, h: 74, impl: 'external', title: '台灣電力公司', sub: '電子電費單（PDF，依子公司加密）' });
  d.node({ id: 'gn', x: 1425, y: 112, w: 215, h: 74, impl: 'external', title: '天能綠電', sub: '轉供電費單（一張多個電號）' });
  d.node({ id: 'mail', x: 1190, y: 214, w: 450, h: 86, impl: 'external', clar: 'open', tags: ['Q-06'], title: '公用信箱群組', sub: '系統要自動收台電、天能電費單與財務開好的發票' });
  d.node({ id: 'ef', x: 1190, y: 326, w: 450, h: 100, impl: 'external', clar: 'open', tags: ['Q-04', 'Q-05'], title: 'EasyFlow／EIP 簽核系統', sub: '付款申請單、開立發票申請單；加簽；會計群組核准' });
  d.node({ id: 'share', x: 1190, y: 452, w: 450, h: 84, impl: 'external', clar: 'open', tags: ['Q-07'], title: '公槽（共用磁碟 Z:\\）', sub: '依指定檔名與路徑存放電費單、紀錄單、發票' });
  d.node({ id: 'payee', x: 1190, y: 562, w: 450, h: 88, impl: 'external', title: '地主／受款人', sub: '收租金付款通知（紙本或 Email）；公司或機關要提供發票或收據' });
  d.node({ id: 'outlook', x: 1190, y: 676, w: 450, h: 84, impl: 'external', clar: 'open', tags: ['FR-027', 'Q-19'], title: 'Outlook', sub: '批次寄送付款通知（技術與資安待驗證）' });
  d.node({ id: 'acct', x: 1190, y: 786, w: 450, h: 70, impl: 'dropped', title: '會計系統／銀行', sub: '不串接：付款以人工為準（合約未要求）' });
  d.node({ id: 'line', x: 1190, y: 880, w: 450, h: 80, impl: 'dropped', clar: 'open', tags: ['Q-07'], title: 'LINE 群組「電費單小圈圈」', sub: '現行歸檔管道；系統化後是否停用' });
  d.group({ id: 'gExcel', x: 30, y: 820, w: 1010, h: 160, title: '現行 Excel 與檔案（要匯入或被系統取代）' });
  d.row({
    x: 50, y: 858, w: 235, h: 108, gap: 13, shape: 'doc', impl: 'todo',
    items: [
      { id: 'e1', title: '電費單超強表格', sub: '電號→案場、檔名規則、付款申請內容', data: 'uncertain' },
      { id: 'e2', title: '電費單紀錄', sub: '每日收件、開票金額、計費期間（未提供範本）', clar: 'open', tags: ['Q-08'] },
      { id: 'e3', title: '資產管理清單', sub: '案場、SPV、合約、地主、履保、保險', data: 'uncertain' },
      { id: 'e4', title: '租金計算（範例）', sub: '度數×費率×租金率；範例含 #VALUE!', data: 'uncertain', tags: ['Q-11'] },
    ],
  });
  d.edge({ from: 'tp', to: 'mail', fromSide: 'b', toSide: 't', toAt: 0.24, impl: 'external', label: 'Email' });
  d.edge({ from: 'gn', to: 'mail', fromSide: 'b', toSide: 't', toAt: 0.76, impl: 'external', label: 'Email' });
  d.edge({ from: 'mail', to: 'sys', fromSide: 'l', toSide: 'r', toAt: 0.06, impl: 'todo', clar: 'open', label: '自動收件 B01／B02／B14' });
  d.edge({ from: 'sys', to: 'ef', fromSide: 'r', toSide: 'l', fromAt: 0.2, toAt: 0.34, impl: 'todo', clar: 'open', label: '建立申請單 B11–B13、R08' });
  d.edge({ from: 'ef', to: 'sys', fromSide: 'l', toSide: 'r', fromAt: 0.78, toAt: 0.34, impl: 'todo', clar: 'open', label: '核准日／付款日回傳？FR-030', labelAt: [1114, 404] });
  d.edge({ from: 'sys', to: 'share', fromSide: 'r', toSide: 'l', fromAt: 0.5, toAt: 0.3, impl: 'todo', clar: 'open', label: '命名存檔 B01／B02／B10／B14' });
  d.edge({ from: 'sys', to: 'payee', fromSide: 'r', toSide: 'l', fromAt: 0.7, toAt: 0.3, impl: 'todo', label: '付款通知 R03' });
  d.edge({ from: 'payee', to: 'sys', fromSide: 'l', toSide: 'r', fromAt: 0.75, toAt: 0.85, impl: 'todo', label: '發票／收據 R04', labelAt: [1114, 604] });
  d.edge({ from: 'sys', to: 'outlook', fromSide: 'r', toSide: 'l', fromAt: 0.97, impl: 'todo', clar: 'open', label: '寄送（研究）' });
  d.edge({ from: 'sys', to: 'ocr', fromSide: 'b', toSide: 't', fromAt: 0.62, toAt: 0.45, impl: 'partial', label: '辨識（HTTP）', both: true });
  d.edge({ from: 'gUsers', to: 'sys', fromSide: 'r', toSide: 'l', fromAt: 0.5, toAt: 0.5, impl: 'partial', clar: 'open', label: '瀏覽器操作（安裝指南只寫主機本機 127.0.0.1，Q-09）', labelW: 160 });
  d.edge({ from: 'gExcel', to: 'sys', fromSide: 't', toSide: 'b', fromAt: 0.5644, toAt: 0.0833, impl: 'todo', label: '匯入 R05／FR-006', labelAt: [560, 742] });
  d.legend(30, 1004, 1620);
}

// ───────────────────────────── A1 業務流程全景 ─────────────────────────────
{
  const d = make({
    id: 'A1', title: '業務流程全景（現行人工作業 → 系統化後）', width: 1720, height: 1250,
    subtitle: '四條業務線。每個方塊是一個作業步驟，填色是 energy-ops-rust 現在能不能做；標籤是合約驗收項（B＝表一電費單、R＝表二租金）與待釐清問題。',
    basis: '電費單付款申請及開立發票 SOP、委託開發契約書附件一、需求總表 v0.1、資產管理清單（範例）',
  });
  const X = (i) => 40 + i * 180;
  const W = 164;
  d.group({ id: 'L1', x: 20, y: 110, w: 1680, h: 380, title: '① 台電電費單（約 98 個發電案場，另有逆變器用電單）' });
  d.row({
    x: 40, y: 152, w: W, h: 126, gap: 16,
    items: [
      { id: 's1', title: '收信下載', sub: '公用信箱自動取附件；現況只能手動上傳', impl: 'todo', clar: 'open', tags: ['B01', 'Q-06'] },
      { id: 's2', title: '解密 PDF', sub: '依子公司密碼；現況可逐檔輸入、不保存', impl: 'partial', clar: 'open', tags: ['Q-14'] },
      { id: 's3', title: '命名＋歸檔', sub: '日期_編號_(子公司)案場；存公槽（現況存 MinIO）', impl: 'todo', clar: 'open', tags: ['B01', 'Q-07'] },
      { id: 's4', title: '辨識欄位', sub: '電號、期間、度數、金額（4 版型已驗證）', impl: 'partial', data: 'uncertain', tags: ['B05', 'B06'] },
      { id: 's5', title: '對案場／子公司／區處', sub: '電號→案場已具備；子公司、區處缺', impl: 'partial', tags: ['B04'] },
      { id: 's6', title: '人工覆核', sub: '校對台：原檔高亮＋三層值，改值留痕', impl: 'done', tags: ['FR-008'] },
      { id: 's7', title: '電費紀錄單', sub: '彙整各案場並依指定檔名路徑存檔', impl: 'todo', clar: 'open', tags: ['B09', 'B10', 'Q-08'] },
      { id: 's8', title: 'EasyFlow 付款申請', sub: '電表租費、使用電能費；附件＋加簽', impl: 'todo', clar: 'open', tags: ['B11', 'Q-05'] },
      { id: 's9', title: 'EasyFlow 開票申請', sub: '有餘電才開；收款對象＝台電區處', impl: 'todo', clar: 'open', tags: ['B12'] },
    ],
  });
  d.chain(['s1', 's2', 's3', 's4', 's5', 's6', 's7', 's8', 's9'], { impl: 'todo' });
  d.node({ id: 'own', x: 40, y: 330, w: 524, h: 140, impl: 'beyond', clar: 'open', tags: ['Q-04'], title: '系統內多關卡核可（energy-ops-rust 既有，合約外）', sub: '承辦確認 → 主管核准 →（≥10 萬）高層加簽 → 計算應付 → 財務放行 → 付款。合約要求的簽核在 EasyFlow，兩者分工待決。' });
  d.node({ id: 'alert', x: 580, y: 330, w: 344, h: 140, impl: 'todo', clar: 'open', tags: ['B03', 'Q-16'], title: '未收單警示', sub: '指定時間沒收到電費單就通知（系統內；Email 群組為選配）。現況：沒有預計到件日的資料，也沒有通知機制。' });
  d.node({ id: 's12', x: X(6), y: 330, w: W, h: 140, impl: 'dropped', clar: 'open', tags: ['Q-27'], title: '發票寄送台電', sub: '區處 Email；雲林、南投寄紙本。合約未列' });
  d.node({ id: 's11', x: X(7), y: 330, w: W, h: 140, impl: 'todo', tags: ['B15'], title: '回寫開立日期', sub: '更新電費紀錄單的發票開立日期' });
  d.node({ id: 's10', x: X(8), y: 330, w: W, h: 140, impl: 'todo', tags: ['B14'], title: '發票收件存檔', sub: '財務開立後，自信箱取發票附件並命名存檔' });
  d.edge({ from: 's9', to: 's10', fromSide: 'b', toSide: 't', impl: 'todo' });
  d.edge({ from: 's10', to: 's11', impl: 'todo' });
  d.edge({ from: 's11', to: 's12', impl: 'dropped' });

  d.group({ id: 'L2', x: 20, y: 510, w: 1680, h: 190, title: '② 天能轉供電費單（綠電轉供：天能向子公司購電，子公司開發票給天能）' });
  d.row({
    x: 40, y: 552, w: W, h: 130, gap: 16,
    items: [
      { id: 't1', title: '收信（天能）', sub: '與台電來源分流', impl: 'todo', clar: 'open', tags: ['B02', 'Q-06'] },
      { id: 't2', title: '合併檔案＋歸檔', sub: '合併規則未定', impl: 'todo', clar: 'open', tags: ['B02', 'Q-12'] },
      { id: 't3', title: '辨識電號→案場、子公司', sub: '一張單多個電號；含計費期間', impl: 'todo', tags: ['B07'] },
      { id: 't4', title: '辨識未稅電能費、營業稅', sub: '現有辨識服務不支援天能版型', impl: 'todo', tags: ['B08'] },
      { id: 't5', title: 'EasyFlow 開票申請', sub: '收款對象＝天能；項目說明有固定句型', impl: 'todo', clar: 'open', tags: ['B13', 'Q-05'] },
      { id: 't6', title: '發票存檔', sub: '同台電發票的收件存檔', impl: 'todo', tags: ['B14'] },
    ],
  });
  d.chain(['t1', 't2', 't3', 't4', 't5', 't6'], { impl: 'todo' });
  d.note({ x: X(6), y: 552, w: 3 * 180 - 16, h: 130, tone: 'info', fs: 12, text: '天能電費單上同時列「轉供度數」與「購售電費率」，這兩個數字也是變動租金的輸入（下方 ③ 取度數）。現有系統只認台電四種版型，天能整條線都還沒有。' });

  d.group({ id: 'L3', x: 20, y: 720, w: 1680, h: 200, title: '③ 租金（付給地主或受款人）' });
  d.row({
    x: 40, y: 762, w: W, h: 136, gap: 16,
    items: [
      { id: 'r1', title: '付款期程到期', sub: '固定租金直接產出；變動租金等度數', impl: 'todo', clar: 'open', tags: ['R02', 'Q-24'] },
      { id: 'r2', title: '取度數', sub: '台電餘電＋天能轉供（指定資料夾）', impl: 'todo', tags: ['R02'] },
      { id: 'r3', title: '租金計算表', sub: '(餘電度×費率＋轉供度×費率)×租金率；稅', impl: 'partial', clar: 'open', tags: ['R01', 'Q-11'] },
      { id: 'r4', title: '多地主分配', sub: '比例或固定金額；繼承異動', impl: 'todo', clar: 'open', tags: ['FR-019', 'Q-22'] },
      { id: 'r5', title: '付款通知', sub: '罐頭訊息；紙本或 Email；附電費單', impl: 'todo', clar: 'open', tags: ['R03', 'Q-19'] },
      { id: 'r6', title: '發票／收據', sub: '公司或機關受款人；先附或後補；未收提醒', impl: 'todo', clar: 'open', tags: ['R04', 'Q-23'] },
      { id: 'r7', title: '付款申請單', sub: '附租金計算表、電費單、發票；檢核簽准', impl: 'todo', clar: 'open', tags: ['R08', 'Q-05'] },
      { id: 'r8', title: '付款追蹤', sub: 'EIP 單號、核准日、實際付款日', impl: 'todo', clar: 'open', tags: ['FR-030', 'Q-20'] },
    ],
  });
  d.chain(['r1', 'r2', 'r3', 'r4', 'r5', 'r6', 'r7', 'r8'], { impl: 'todo' });

  d.group({ id: 'L4', x: 20, y: 940, w: 1680, h: 210, title: '④ 資產與合約管理（取代「資產管理清單」Excel）' });
  d.row({
    x: 40, y: 982, w: W, h: 146, gap: 16,
    items: [
      { id: 'm1', title: '資產管理表匯入', sub: '共用總表：查詢、篩選、修正、匯出', impl: 'todo', tags: ['R05', 'FR-006'] },
      { id: 'm2', title: '操作紀錄與權限', sub: '稽核與權限機制已具備，要接上主檔', impl: 'partial', tags: ['R05'] },
      { id: 'm3', title: '合約上傳＋到期判定', sub: '到期日、租金付款日', impl: 'todo', clar: 'open', tags: ['R06', 'Q-25'] },
      { id: 'm4', title: '續約／付款日提醒', sub: '到期前 6／3／1 個月；付款日待辦', impl: 'todo', tags: ['R06', 'FR-015'] },
      { id: 'm5', title: '履保金退還追蹤', sub: '分期退還、提醒；本期是否交付？', impl: 'todo', clar: 'open', tags: ['FR-033', 'Q-02'] },
      { id: 'm6', title: '保險承保追蹤', sub: '續保與保單提供；本期是否交付？', impl: 'todo', clar: 'open', tags: ['FR-034', 'Q-02'] },
      { id: 'm7', title: '報表', sub: '案場／年／月：發電度數、租金支出', impl: 'todo', tags: ['R07'] },
    ],
  });
  d.chain(['m1', 'm2', 'm3', 'm4'], { impl: 'todo' });
  d.edge({ from: 's4', to: 'r2', fromSide: 'b', toSide: 't', fromAt: 0.5, toAt: 0.18, impl: 'todo', points: [[X(3) + 82, 314], [28, 314], [28, 744], [X(1) + 29.5, 744]], label: '台電餘電度數', labelAt: [X(1) + 160, 314] });
  d.edge({ from: 't4', to: 'r2', fromSide: 'b', toSide: 't', fromAt: 0.5, toAt: 0.35, impl: 'todo', points: [[X(3) + 82, 710], [X(1) + 57, 710]], label: '轉供度數', labelAt: [X(2) + 82, 710] });
  d.edge({ from: 'm3', to: 'r1', fromSide: 't', toSide: 'b', fromAt: 0.5, toAt: 0.5, impl: 'todo', points: [[X(2) + 82, 930], [X(0) + 82, 930]], label: '合約條款與付款期程', labelAt: [X(1) + 82, 930] });
  d.legend(20, 1168, 1680);
}

// ───────────────────────────── A2 部署與容器 ─────────────────────────────
{
  const d = make({
    id: 'A2', title: '部署與容器（客戶主機、開發環境、舊版）', width: 1700, height: 1100,
    subtitle: '系統實際怎麼跑：客戶端是 Windows Server 上的 WSL 與四個容器；右側是我方開發環境與不再採用的舊版。內網資源都還沒接。',
    basis: '雲豹離線安裝逐步操作指南（2026-11-04）、energy-ops-rust README／docker-compose、deployments/energy-ops.md',
  });
  d.group({ id: 'gC', x: 20, y: 110, w: 1060, h: 880, title: '雲豹內網（客戶主機）', sub: 'NFR-002：部署於客戶主機，配合內部網路限制' });
  d.group({ id: 'gVM', x: 40, y: 170, w: 640, h: 650, title: 'Windows Server 2022 VM（VMware，需巢狀虛擬化）', sub: '4 vCPU／8 GB；目前只做過單筆試跑，批次速度與容量待實測', fill: '#F1F5F9' });
  d.group({ id: 'gWSL', x: 60, y: 238, w: 600, h: 400, title: 'WSL 發行版「EnergyOps」＋ 4 個容器', fill: '#FFFFFF' });
  d.node({ id: 'c1', x: 80, y: 280, w: 270, h: 120, impl: 'done', title: 'platform：energy-ops', sub: 'Rust 單一執行檔＋前端；:11950；開機自動套 migration、建 bucket' });
  d.node({ id: 'c2', x: 370, y: 280, w: 270, h: 120, shape: 'db', impl: 'done', title: 'postgres', sub: '業務資料、稽核、session' });
  d.node({ id: 'c3', x: 80, y: 420, w: 270, h: 120, shape: 'db', impl: 'done', title: 'minio', sub: '原檔與附件（S3 相容）；不對外開放' });
  d.node({ id: 'c4', x: 370, y: 420, w: 270, h: 120, impl: 'partial', data: 'uncertain', title: 'ocr：taipower-recognition', sub: 'PDF 文字層＋本機 OCR；影像辨識有誤（營業稅誤取）' });
  d.node({ id: 'pkg', x: 80, y: 552, w: 560, h: 78, impl: 'partial', clar: 'open', tags: ['Q-09'], title: '安裝包 EnergyOps_Offline_20261104', sub: 'Prepare／Install／Start-Windows.ps1；打包原始碼不在 energy-ops-rust repo' });
  d.node({ id: 'ops1', x: 60, y: 660, w: 290, h: 140, impl: 'todo', clar: 'open', tags: ['Q-31'], title: '開機自動啟動', sub: '需 IT 設定工作排程，並驗證重開機、遠端登出後仍持續運作' });
  d.node({ id: 'ops2', x: 370, y: 660, w: 290, h: 140, impl: 'todo', clar: 'open', tags: ['Q-31'], title: '備份／監控／日誌', sub: '尚未規劃。維護期合約要求 4 小時內確認問題' });
  d.group({ id: 'gNet', x: 700, y: 170, w: 360, h: 650, title: '內網資源（都需 IT 開通）', fill: '#F1F5F9' });
  d.node({ id: 'i1', x: 720, y: 222, w: 320, h: 120, impl: 'partial', clar: 'open', tags: ['Q-09', 'Q-30'], title: '使用者電腦（瀏覽器）', sub: '安裝指南要求在主機遠端桌面內開 127.0.0.1:11950；同仁自己的電腦能否連線未定' });
  d.node({ id: 'i2', x: 720, y: 368, w: 320, h: 120, impl: 'external', clar: 'open', tags: ['Q-06'], title: '公用信箱伺服器', sub: 'Exchange／Microsoft 365／IMAP？帳號與連線方式未定' });
  d.node({ id: 'i3', x: 720, y: 514, w: 320, h: 120, impl: 'external', clar: 'open', tags: ['Q-07'], title: '公槽（SMB 共用磁碟）', sub: '指定路徑與寫入權限未定；指南提醒不要把程式放在公槽執行' });
  d.node({ id: 'i4', x: 720, y: 660, w: 320, h: 140, impl: 'external', clar: 'open', tags: ['Q-05'], title: 'EasyFlow 伺服器', sub: '有無 API、能否以模擬操作建單、測試帳號，皆未定' });
  d.node({ id: 'deliv', x: 40, y: 840, w: 1020, h: 130, impl: 'todo', clar: 'open', tags: ['Q-33'], title: '交付與維運（合約第三、九條與附件一）', sub: '教育訓練 1 週；使用手冊（安裝、功能、錯誤排除）；原始碼、範例、PPT、DOC；Windows 安裝包；維護 1 年：4 小時內確認、緊急 1 個工作天、非緊急 3 個工作天，逾期每日罰款。' });
  d.edge({ from: 'c1', to: 'c2', impl: 'done', both: true });
  d.edge({ from: 'c1', to: 'c3', fromSide: 'b', toSide: 't', impl: 'done', both: true });
  d.edge({ from: 'c1', to: 'c4', fromSide: 'r', toSide: 'l', fromAt: 0.85, toAt: 0.2, impl: 'partial' });
  d.edge({ from: 'i1', to: 'gWSL', fromSide: 'l', toSide: 'r', toAt: 0.12, impl: 'partial', clar: 'open', label: '瀏覽器' });
  d.edge({ from: 'gWSL', to: 'i2', fromSide: 'r', toSide: 'l', fromAt: 0.42, toAt: 0.317, impl: 'todo', clar: 'open', label: '收件' });
  d.edge({ from: 'gWSL', to: 'i3', fromSide: 'r', toSide: 'l', fromAt: 0.8, toAt: 0.367, impl: 'todo', clar: 'open', label: '存檔' });
  d.edge({ from: 'gWSL', to: 'i4', fromSide: 'r', toSide: 'l', fromAt: 0.97, toAt: 0.2, impl: 'todo', clar: 'open', route: 'h', mid: 690, label: '建單', labelAt: [690, 660] });
  d.group({ id: 'gDev', x: 1100, y: 110, w: 580, h: 560, title: '開發環境（我方）', fill: '#F8FAFC' });
  d.row({ x: 1120, y: 166, w: 262, h: 100, gap: 16, items: [
    { id: 'd1', impl: 'done', title: 'docker compose', sub: 'Postgres 18 :56432、MinIO :56900' },
    { id: 'd2', impl: 'done', title: 'dev-run.sh', sub: '後端 :10950＋Vite :5190；--demo 灌示範資料' },
  ] });
  d.row({ x: 1120, y: 290, w: 262, h: 100, gap: 16, items: [
    { id: 'd3', impl: 'done', data: 'mock', title: 'fake-tpr :18080', sub: '依檔名回 4 份範本的辨識結果' },
    { id: 'd4', impl: 'done', title: 'gate.sh／e2e crawler', sub: 'Rust 266＋vitest 173；12 支 crawler、595 條斷言' },
  ] });
  d.node({ id: 'd5', x: 1120, y: 414, w: 540, h: 100, impl: 'todo', clar: 'open', tags: ['Q-09'], title: '打包腳本（產出離線安裝包）', sub: '本機與 GitHub 都找不到；離線包可能在另一台電腦打包' });
  d.node({ id: 'd6', x: 1120, y: 538, w: 540, h: 112, impl: 'done', title: 'GitHub：xtforgame/energy-ops-rust', sub: 'master＝rust-port＝cf7a330（2026-11-04）。2026-11-12 抓取：遠端沒有新提交' });
  d.group({ id: 'gOld', x: 1100, y: 690, w: 580, h: 300, title: '舊版（不採用；去留待定）', fill: '#FAFAFA' });
  d.node({ id: 'l1', x: 1120, y: 744, w: 540, h: 104, impl: 'dropped', clar: 'open', tags: ['Q-10'], title: 'App 5 線上 Demo（energy-ops.vercel.app）', sub: 'Vercel＋Supabase Edge Functions＋平台 OCR；單一組織；有一條未併修正分支' });
  d.node({ id: 'l2', x: 1120, y: 866, w: 540, h: 104, impl: 'dropped', title: 'az repo 原版（Node／Sequelize）', sub: 'feat-tenant-console-route-transition；設計文件與 crawler 的來源' });
  d.edge({ from: 'd5', to: 'pkg', fromSide: 'l', toSide: 'r', impl: 'external', points: [[1090, 464], [1090, 160], [690, 160], [690, 591]], label: 'zip 人工搬到主機', labelAt: [890, 160] });
  d.legend(20, 1012, 1660);
}

// ───────────────────────────── A3 應用元件 ─────────────────────────────
{
  const d = make({
    id: 'A3', title: '應用元件（energy-ops-rust 的前端、後端、平台、純邏輯）', width: 1720, height: 1240,
    subtitle: '最終版本的每個元件與它對合約的意義：綠＝驗收用得上、藍＝已做但合約沒要求、白虛框＝驗收需要但還沒有的元件。',
    basis: 'energy-ops-rust（cf7a330）docs/OVERVIEW.md、crates/*、web/src；委託開發契約書附件一',
  });
  const P = (i) => 40 + i * 207;
  d.group({ id: 'gFE', x: 20, y: 110, w: 1680, h: 290, title: '前端 web（Vite＋React 19＋react-admin＋MUI；建置後由後端供應）' });
  d.row({ x: 40, y: 154, w: 191, h: 100, gap: 16, items: [
    { id: 'p1', impl: 'done', title: '我的工作', sub: '待辦佇列＋我參與過的' },
    { id: 'p2', impl: 'done', title: '送件（收件台）', sub: '批次上傳、辨識進度、就地校對' },
    { id: 'p3', impl: 'done', title: '電費單／校對台', sub: '原檔＋三層值；五種模式' },
    { id: 'p4', impl: 'beyond', clar: 'open', tags: ['Q-04'], title: '流程單', sub: '系統內簽核的執行' },
    { id: 'p5', impl: 'partial', title: '地主／案場', sub: '只能看，不能新增、編輯' },
    { id: 'p6', impl: 'partial', title: '行事曆／儀表板', sub: '只有期數到期；無應收' },
    { id: 'p7', impl: 'done', clar: 'open', tags: ['Q-34'], title: '流程總覽／稽核', sub: '選單隱藏，網址可進' },
    { id: 'p8', impl: 'beyond', data: 'mock', tags: ['Q-28'], title: '台灣 3D 地圖', sub: '示範座標' },
  ] });
  d.row({ x: 40, y: 278, w: 191, h: 104, gap: 16, items: [
    { id: 'p9', impl: 'beyond', clar: 'open', tags: ['Q-34'], title: '組織／專案／邀請', sub: '多組織（合約外）' },
    { id: 'p10', impl: 'beyond', clar: 'open', tags: ['Q-34'], title: 'RBAC 管理', sub: '角色、permission、授權' },
    { id: 'p11', impl: 'done', title: '部門與能源角色', sub: '五角色指派、部門主管' },
    { id: 'p12', impl: 'done', title: '個人資料／帳號管理', sub: '改密碼、超級使用者' },
    { id: 'p13', impl: 'todo', tags: ['R05'], title: '資產主檔維護', sub: '匯入、修正、匯出' },
    { id: 'p14', impl: 'todo', tags: ['R01', 'R03'], title: '租金計算／通知', sub: '每期計算、通知' },
    { id: 'p15', impl: 'todo', tags: ['B14', 'R04'], title: '發票管理', sub: '收件、配對、漏件' },
    { id: 'p16', impl: 'todo', tags: ['B09', 'R07'], title: '紀錄單／報表', sub: '查詢、匯出' },
  ] });
  d.group({ id: 'gBE', x: 20, y: 420, w: 1680, h: 300, title: '後端 eo-energy（能源 plugin；/api/v1/orgs/{orgId}/energy）' });
  d.row({ x: 40, y: 464, w: 191, h: 110, gap: 16, items: [
    { id: 'b1', impl: 'done', title: 'ingest 收件', sub: '批次、項目、去重、例外件、送審' },
    { id: 'b2', impl: 'done', title: 'recognition 辨識驅動', sub: '背景 tick、租約、每批 10 份' },
    { id: 'b3', impl: 'done', title: 'bills 電費單', sub: '三層值、校對、分支預覽' },
    { id: 'b4', impl: 'done', title: 'attachments 附件', sub: 'MinIO、sha256、預簽直傳' },
    { id: 'b5', impl: 'partial', data: 'mock', title: 'master_data 主檔', sub: '只有讀取；geocode 是 mock' },
    { id: 'b6', impl: 'beyond', title: 'bindings 效期綁定', sub: '流程／計費綁案場' },
    { id: 'b7', impl: 'partial', clar: 'open', tags: ['Q-11'], title: 'rating 計費', sub: '三種策略、不可變應付紀錄' },
    { id: 'b8', impl: 'beyond', clar: 'open', tags: ['Q-04'], title: 'workflow 引擎', sub: '多關卡簽核、回退鎖、代理' },
  ] });
  d.row({ x: 40, y: 592, w: 191, h: 110, gap: 16, items: [
    { id: 'b9', impl: 'partial', title: 'workbench', sub: '登入落點、儀表板（只有應付）、行事曆' },
    { id: 'b10', impl: 'done', title: 'org 部門與角色', sub: '五能源角色、最後一位治理者防呆' },
    { id: 'b11', impl: 'done', tags: ['R05'], title: 'audit 稽核', sub: '56 種事件，與業務同交易寫入' },
    { id: 'b12', impl: 'done', data: 'mock', tags: ['Q-36'], title: 'demo_seed', sub: '虛構主檔，但用了真實電號' },
    { id: 'b13', impl: 'done', data: 'mock', title: 'fake_tpr', sub: '開發測試用假辨識服務' },
    { id: 'b14', impl: 'external', data: 'uncertain', title: 'tpr_client → OCR', sub: '講 taipower-recognition 協定' },
  ] });
  d.group({ id: 'gNew', x: 20, y: 740, w: 1680, h: 166, title: '要新增的元件（合約驗收需要，目前沒有）', clar: 'open', fill: '#FFFBF5' });
  d.row({ x: 40, y: 782, w: 150, h: 108, gap: 15, impl: 'todo', small: true, items: [
    { id: 'n1', title: '郵件收件器', tags: ['B01', 'B02'] },
    { id: 'n2', title: '公槽歸檔器', tags: ['B01', 'B10'] },
    { id: 'n3', title: '天能單解析', tags: ['B07', 'B08'] },
    { id: 'n4', title: '紀錄單產生器（Excel）', tags: ['B09'] },
    { id: 'n5', title: 'EasyFlow 連接器（API／模擬操作）', tags: ['B11', 'R08'] },
    { id: 'n6', title: '發票管理', tags: ['B14', 'R04'] },
    { id: 'n7', title: '租金期別計算', tags: ['R01', 'R02'] },
    { id: 'n8', title: '提醒排程＋通知中心', tags: ['B03', 'R06'] },
    { id: 'n9', title: '主檔匯入／編修／匯出', tags: ['R05'] },
    { id: 'n10', title: '報表與匯出', tags: ['R07'] },
  ] });
  d.group({ id: 'gPL', x: 20, y: 926, w: 1120, h: 210, title: '平台 eo-platform（azldi plugin 框架）' });
  d.row({ x: 40, y: 970, w: 206, h: 146, gap: 15, items: [
    { id: 'q1', impl: 'done', title: 'authn 登入', sub: 'bcrypt；DB session（閒置 14 天、絕對 183 天）；無忘記密碼' },
    { id: 'q2', impl: 'done', title: 'user 帳號', sub: '個資、改密碼、停用' },
    { id: 'q3', impl: 'beyond', clar: 'open', tags: ['Q-34'], title: 'multitenancy', sub: '組織、專案、邀請（不寄信）' },
    { id: 'q4', impl: 'done', title: 'rbac 權限', sub: '三層 scope、自訂角色、快取（功能遠多於合約）' },
    { id: 'q5', impl: 'done', title: '基礎', sub: 'database／storage（SigV4）／health／static_web' },
  ] });
  d.group({ id: 'gDM', x: 1160, y: 926, w: 540, h: 210, title: '純邏輯 eo-domain（不碰 IO）' });
  d.row({ x: 1180, y: 970, w: 160, h: 146, gap: 10, items: [
    { id: 'r1', impl: 'partial', data: 'mock', title: '抽取與分類', sub: 'canonical 119 欄；非 canonical 退回 mock' },
    { id: 'r2', impl: 'done', title: '計費與條件', sub: '定點數計算、條件求值' },
    { id: 'r3', impl: 'done', title: '業務鍵與模式', sub: '去重鍵、單據五種模式' },
  ] });
  d.edge({ from: 'gFE', to: 'gBE', fromSide: 'b', toSide: 't', fromAt: 0.93, toAt: 0.93, impl: 'done', label: 'REST /api/v1' });
  d.edge({ from: 'gNew', to: 'gBE', fromSide: 't', toSide: 'b', fromAt: 0.97, toAt: 0.97, impl: 'todo', clar: 'open', label: '擴充' });
  d.legend(20, 1156, 1680);
}

// ───────────────────────────── A4 資料模型 ─────────────────────────────
{
  const d = make({
    id: 'A4', title: '資料模型（現有資料表 → 合約與需求還需要的資料）', width: 1720, height: 1220,
    subtitle: '左：energy-ops-rust 現有 41 張表（13 平台＋28 能源）依主題分組；右：DB 欄位字典（DB-001～153）要求、但現在沒有的資料。箭頭是「可以由左邊擴充」。',
    basis: 'energy-ops-rust migrations 0001～0102；電費單掃描資料庫欄位與合約驗收對照（整併版）資料庫欄位字典',
  });
  d.group({ id: 'gL', x: 20, y: 110, w: 820, h: 1000, title: '現有資料表（migrations 0001～0102）' });
  d.node({ id: 't1', x: 40, y: 160, w: 380, h: 172, impl: 'partial', clar: 'open', tags: ['FR-001', 'FR-002', 'FR-014'], title: '主檔', sub: 'landowner：一案場只連一位地主\nstation：一案場一個電號；沒有子公司、區處、容量、併網日\ncontract：沒有類型、到期提醒、付款規則、合約檔\ncontract_station：契約×案場配對與參數' });
  d.node({ id: 't2', x: 440, y: 160, w: 380, h: 172, impl: 'partial', clar: 'open', tags: ['Q-18'], title: '期數與行事曆', sub: 'installment：建設期保證金分期（付給地主？）\ncalendar_event：只產「期數到期」一種事件；不會由合約自動產生' });
  d.node({ id: 't3', x: 40, y: 352, w: 380, h: 188, impl: 'partial', tags: ['B05', 'B06'], title: '收件與單據', sub: 'ingest_batch／ingest_item\nbill：三層值（辨識原始／抽取／最終）\n最終值只有期間、總額、度數、開單日、繳費期限；電表租費、使用電能費、BBR、下次抄表日仍只在辨識原始資料裡' });
  d.node({ id: 't4', x: 440, y: 352, w: 380, h: 188, impl: 'done', title: '附件', sub: 'attachment：MinIO、sha256、換版\nattachment_link：掛在哪個關卡\n（多流程時原檔歸屬會被後開的流程蓋掉，見程式碼比對紀錄）' });
  d.node({ id: 't5', x: 40, y: 560, w: 380, h: 190, impl: 'beyond', clar: 'open', tags: ['Q-04'], title: '系統內流程（11 張，合約外）', sub: 'workflow_definition／node／transition／action\nworkflow_binding／execution／task／task_candidate\ndelegation／side_effect／execution_lineage' });
  d.node({ id: 't6', x: 440, y: 560, w: 380, h: 190, impl: 'partial', clar: 'open', tags: ['R01', 'Q-11'], title: '計費與應付', sub: 'rating_strategy：固定、售電比例、度數單價\nrating_strategy_binding：效期\npayout_record：每張單算一次、不可變\n（實際租金是「每期×每份合約×每位地主」）' });
  d.node({ id: 't7', x: 40, y: 770, w: 380, h: 120, impl: 'done', title: '組織', sub: 'department／department_member（主管）' });
  d.node({ id: 't8', x: 440, y: 770, w: 380, h: 120, impl: 'done', tags: ['R05'], title: '稽核', sub: 'audit_event：56 種事件、payload 白名單、前後值' });
  d.node({ id: 't9', x: 40, y: 910, w: 780, h: 180, impl: 'done', tags: ['Q-34'], title: '平台（13 張）', sub: 'user／account_link／user_session：登入與 session\norganization／project／invitation：多組織與邀請（合約外）\nrbac_permission／complex／role／role_permission／user_rbac_role：權限' });
  d.group({ id: 'gR', x: 860, y: 110, w: 840, h: 1000, title: '還需要的資料（DB 欄位字典與合約驗收）', clar: 'open', fill: '#FFFBF5' });
  d.node({ id: 'n1', x: 880, y: 160, w: 400, h: 140, impl: 'todo', tags: ['FR-001'], title: '案場主檔（擴充）', sub: '專案編號、簡稱、區域、分類、子公司（SPV）、容量 kWp、型別、併網日、狀態、內部負責人。由現有 station 擴充' });
  d.node({ id: 'n2', x: 1300, y: 160, w: 380, h: 140, impl: 'todo', clar: 'open', tags: ['FR-002', 'Q-17'], title: '案場電號關聯', sub: '一案場多電號；用途（躉購／用電）；台電契約編號；躉售與綠電費率；有效起訖。新表，取代 station 的單一電號' });
  d.node({ id: 'n3', x: 880, y: 320, w: 400, h: 140, impl: 'todo', clar: 'open', tags: ['FR-022', 'Q-29'], title: '子公司與受款人', sub: '對象類型、統編、地址、銀行帳號（需加密）、是否開發票、先開或後補。新表；現有 landowner 只涵蓋個人地主' });
  d.node({ id: 'n4', x: 1300, y: 320, w: 380, h: 140, impl: 'todo', clar: 'open', tags: ['FR-019', 'FR-020', 'Q-22'], title: '多地主分配', sub: '比例或固定金額；有效起訖；繼承與異動文件、原因。新表；效期做法可沿用 bindings' });
  d.node({ id: 'n5', x: 880, y: 480, w: 400, h: 160, impl: 'todo', tags: ['FR-014', 'R06'], title: '合約（多類型）', sub: '租賃、綠電、維運上包、下包；簽約、起訖、續約提醒；條款版本；付款頻率與期限規則；合約檔案。由現有 contract 擴充' });
  d.node({ id: 'n6', x: 1300, y: 480, w: 380, h: 160, impl: 'todo', clar: 'open', tags: ['R01', 'R02', 'Q-11'], title: '租金規則與每期明細', sub: '規則版本；餘電／轉供費率、租金率、固定、保底、年度基準；稅；每期計算明細＋人工覆核。由現有 rating 擴充' });
  d.node({ id: 'n7', x: 880, y: 660, w: 400, h: 140, impl: 'partial', tags: ['B05', 'B06'], title: '電費單明細（依版型）', sub: '通知單號、本次／下次抄表日、繳費期限、電表租費、使用電能費、基本／流動電費、載具與發票號碼。由現有 bill 擴充（辨識服務已輸出）' });
  d.node({ id: 'n8', x: 1300, y: 660, w: 380, h: 140, impl: 'todo', clar: 'open', tags: ['B07', 'B08', 'Q-12'], title: '天能轉供單', sub: '一單多電號；轉供度數；購售電費率；未稅電能費；評鑑費、書審費。新表' });
  d.node({ id: 'n9', x: 880, y: 820, w: 400, h: 130, impl: 'todo', tags: ['B14', 'B15', 'R04'], title: '發票與請款', sub: '類型、號碼、日期、金額、收票日、核對狀態、缺件提醒。新表' });
  d.node({ id: 'n10', x: 1300, y: 820, w: 380, h: 130, impl: 'todo', clar: 'open', tags: ['B11', 'R08', 'Q-20'], title: '付款申請', sub: '申請金額、預計付款日、期限、EIP 單號、狀態、核准日、實際付款日。新表' });
  d.node({ id: 'n11', x: 880, y: 970, w: 400, h: 120, impl: 'todo', clar: 'open', tags: ['FR-033', 'FR-034', 'Q-02'], title: '履保／保險／待辦', sub: '保證金餘額與分期退還、保險有效期、提醒、負責人。新表；與現有 installment 語意不同？' });
  d.node({ id: 'n12', x: 1300, y: 970, w: 380, h: 120, impl: 'todo', clar: 'open', tags: ['B03', 'Q-16'], title: '收件設定與匯入批次', sub: '信箱、允許寄件者、預計到件日、漏件提醒天數；匯入錯誤。新表；可擴充 ingest_batch' });
  d.legend(20, 1130, 1680);
}

// ───────────────────────────── A5 電費單處理細部 ─────────────────────────────
{
  const d = make({
    id: 'A5', title: '電費單處理細部（表一 B01～B15 對到每一步）', width: 1720, height: 1150,
    subtitle: '從信箱到發票回寫的完整管線。A 收件歸檔 → B 辨識覆核 → C 紀錄單與申請單 → D 發票。綠色是現在就能用的部分，其他都要補。',
    basis: '委託開發契約書附件一表一、電費單付款申請及開立發票 SOP、天能轉供電費開立發票欄位對照圖、RECOGNITION.md',
  });
  const S = 235; const W = 215;
  d.group({ id: 'gA', x: 20, y: 110, w: 1680, h: 210, title: 'A. 收件與歸檔' });
  d.row({ x: 40, y: 154, w: W, h: 146, gap: S - W, items: [
    { id: 'a1', impl: 'external', clar: 'open', tags: ['Q-06'], title: '公用信箱', sub: '台電、天能電費單；財務開好的發票' },
    { id: 'a2', impl: 'todo', tags: ['B01', 'B02'], title: '收件器', sub: '排程收信；寄件者與附件規則；sha256 去重' },
    { id: 'a3', impl: 'partial', clar: 'open', tags: ['Q-14'], title: 'PDF 解密', sub: '依子公司密碼；現況只能逐檔輸入、不保存' },
    { id: 'a4', impl: 'todo', clar: 'open', tags: ['B01', 'Q-07'], title: '檔案命名', sub: '日期_案場編號_(子公司)案場名［(逆變器)］' },
    { id: 'a5', impl: 'todo', clar: 'open', tags: ['B01', 'Q-07'], title: '存公槽＋系統', sub: '公槽指定資料夾；系統內同時存 MinIO' },
    { id: 'a6', impl: 'todo', clar: 'open', tags: ['B02', 'Q-12'], title: '天能：合併檔案', sub: '台電轉供單清單＋天能電費單？' },
    { id: 'a7', impl: 'done', tags: ['FR-005'], title: '手動上傳（既有）', sub: '收件台一批 ≤50 檔；密碼可逐檔輸入' },
  ] });
  d.chain(['a1', 'a2', 'a3', 'a4', 'a5'], { impl: 'todo' });
  d.edge({ from: 'a2', to: 'a6', fromSide: 'b', toSide: 'b', fromAt: 0.5, toAt: 0.5, impl: 'todo', points: [[S + 147, 312], [5 * S + 147, 312]], label: '天能來源', labelAt: [3 * S + 147, 312] });
  d.group({ id: 'gB', x: 20, y: 340, w: 1680, h: 210, title: 'B. 辨識與覆核' });
  d.row({ x: 40, y: 384, w: W, h: 146, gap: S - W, items: [
    { id: 'b1', impl: 'partial', data: 'uncertain', tags: ['B04–B08', 'Q-15'], title: 'OCR 辨識', sub: '文字層 PDF 4 範本已驗證；影像、傳真影本有誤' },
    { id: 'b2', impl: 'partial', clar: 'open', tags: ['Q-15'], title: '分類單別', sub: '躉購有餘電／無餘電／逆變器；天能、退款補收未支援' },
    { id: 'b3', impl: 'partial', tags: ['B05', 'B06'], title: '抽取欄位', sub: '電號、期間、度數、金額已有；BBR、下次抄表日未成一級欄位' },
    { id: 'b4', impl: 'partial', tags: ['B04'], title: '對案場', sub: '電號正規化比對已有；子公司、區處要主檔' },
    { id: 'b5', impl: 'done', tags: ['FR-009'], title: '例外件', sub: '對不上或辨識失敗：人工指認（既有）' },
    { id: 'b6', impl: 'done', tags: ['FR-008'], title: '人工覆核', sub: '校對台：原檔高亮、改值留痕（既有）' },
    { id: 'b7', impl: 'todo', clar: 'open', tags: ['B03', 'Q-16'], title: '未收單警示', sub: '比對預計到件日；系統內通知，Email 選配' },
  ] });
  d.edge({ from: 'a5', to: 'b1', fromSide: 'b', toSide: 't', impl: 'todo', route: 'v', mid: 330 });
  d.edge({ from: 'a7', to: 'b1', fromSide: 'b', toSide: 't', fromAt: 0.5, toAt: 0.8, impl: 'done', points: [[6 * S + 147, 336], [40 + W * 0.8, 336]], label: '既有路徑', labelAt: [5 * S + 40, 336] });
  d.chain(['b1', 'b2', 'b3', 'b4'], { impl: 'partial' });
  d.edge({ from: 'b4', to: 'b5', impl: 'done', label: '對不上' });
  d.edge({ from: 'b5', to: 'b6', impl: 'done' });
  d.group({ id: 'gC', x: 20, y: 570, w: 1680, h: 230, title: 'C. 電費紀錄單與 EasyFlow 申請單' });
  d.row({ x: 40, y: 614, w: W, h: 166, gap: S - W, items: [
    { id: 'c1', impl: 'todo', clar: 'open', tags: ['B09', 'Q-08'], title: '電費紀錄單', sub: '各案場電費、開票金額、計費期間、下次抄表日、收件日' },
    { id: 'c2', impl: 'todo', clar: 'open', tags: ['B10', 'Q-07'], title: '紀錄單存檔', sub: '指定檔名與路徑' },
    { id: 'c3', impl: 'todo', tags: ['B11'], title: '付款申請內容', sub: '公司代碼、費用種類、摘要、付款對象、方式、憑證（BBR）、事由、金額' },
    { id: 'c4', impl: 'todo', tags: ['B12'], title: '開票申請（台電）', sub: '收款對象＝區處；統編；購電電費；預計收款日' },
    { id: 'c5', impl: 'todo', tags: ['B13'], title: '開票申請（天能）', sub: '收款對象＝天能；未稅電能費；項目說明句型' },
    { id: 'c6', impl: 'todo', clar: 'open', tags: ['Q-05'], title: '送進 EasyFlow', sub: '附件＋加簽；API 或模擬操作' },
    { id: 'c7', impl: 'external', title: 'EasyFlow 簽核', sub: '員工關 → 會計群組（任一人核准）' },
  ] });
  d.edge({ from: 'b6', to: 'c1', fromSide: 'b', toSide: 't', impl: 'todo', route: 'v', mid: 560 });
  d.edge({ from: 'c1', to: 'c2', impl: 'todo' });
  d.edge({ from: 'c2', to: 'c3', impl: 'todo' });
  d.edge({ from: 'c3', to: 'c6', fromSide: 'b', toSide: 'b', impl: 'todo', points: [[2 * S + 147, 792], [5 * S + 147, 792]] });
  d.edge({ from: 'c4', to: 'c6', fromSide: 'b', toSide: 'b', fromAt: 0.5, toAt: 0.3, impl: 'todo', points: [[3 * S + 147, 788], [5 * S + 104, 788]] });
  d.edge({ from: 'c5', to: 'c6', impl: 'todo' });
  d.edge({ from: 'c6', to: 'c7', impl: 'todo', clar: 'open' });
  d.group({ id: 'gD', x: 20, y: 820, w: 1680, h: 210, title: 'D. 發票開立之後' });
  d.row({ x: 40, y: 864, w: W, h: 146, gap: S - W, items: [
    { id: 'd1', impl: 'external', title: '財務開立發票', sub: 'EasyFlow 核准後由財務開立' },
    { id: 'd2', impl: 'todo', tags: ['B14'], title: '發票收件', sub: '自信箱取發票附件' },
    { id: 'd3', impl: 'todo', tags: ['B14'], title: '發票存檔', sub: '(區域)子公司_發票號碼_案場-電號' },
    { id: 'd4', impl: 'todo', tags: ['B15'], title: '回寫紀錄單', sub: '更新發票開立日期' },
    { id: 'd5', impl: 'dropped', clar: 'open', tags: ['Q-27'], title: '寄送台電區處', sub: '現行人工 Email／紙本；合約未列' },
  ] });
  d.chain(['d1', 'd2', 'd3', 'd4', 'd5'], { impl: 'todo' });
  d.edge({ from: 'c7', to: 'd1', fromSide: 'b', toSide: 't', impl: 'external', route: 'v', mid: 812 });
  d.note({ x: 5 * S + 40, y: 864, w: 2 * S - 20, h: 146, tone: 'warn', fs: 12, text: '注意：SOP 現在把台電電子帳單網站帳密與各子公司 PDF 密碼寫在文件裡。系統若要自動解密，密碼要由系統安全保管（Q-14），本文件不轉載任何密碼。' });
  d.legend(20, 1050, 1680);
}

// ───────────────────────────── A6 租金與資產細部 ─────────────────────────────
{
  const d = make({
    id: 'A6', title: '租金與資產管理細部（表二 R01～R08）', width: 1720, height: 1150,
    subtitle: 'A 資產主檔取代 Excel 總表；B 每期租金計算；C 通知、發票與付款申請；D 報表。可以沿用的既有元件標在 D 區。',
    basis: '委託開發契約書附件一表二、資產管理清單（範例）、租金計算（範例）、需求總表 v0.1',
  });
  const S = 235; const W = 215;
  d.group({ id: 'gA', x: 20, y: 110, w: 1680, h: 210, title: 'A. 資產主檔（取代「資產管理清單」Excel）' });
  d.row({ x: 40, y: 154, w: W, h: 146, gap: S - W, items: [
    { id: 'x1', shape: 'doc', impl: 'external', data: 'uncertain', title: '資產管理清單', sub: '9 個工作表；公式有 #N/A、#REF!' },
    { id: 'x2', impl: 'todo', tags: ['R05', 'FR-006'], title: '匯入＋驗證', sub: '主鍵、電號、子公司對應；錯誤報告' },
    { id: 'x3', impl: 'todo', tags: ['R05'], title: '主檔維護', sub: '查詢、篩選、修正、匯出' },
    { id: 'x4', impl: 'partial', tags: ['R05'], title: '操作紀錄＋權限', sub: '稽核與權限已具備，要接到主檔' },
    { id: 'x5', impl: 'todo', clar: 'open', tags: ['R06', 'Q-25'], title: '合約上傳', sub: '判定到期日與付款日：OCR 或人工？' },
    { id: 'x6', impl: 'todo', tags: ['R06', 'FR-015'], title: '提醒', sub: '續約（到期前 6／3／1 月）、付款日待辦' },
    { id: 'x7', impl: 'todo', clar: 'open', tags: ['FR-033', 'Q-02'], title: '履保金／保險', sub: '分期退還、續保；本期是否交付？' },
  ] });
  d.chain(['x1', 'x2', 'x3', 'x4'], { impl: 'todo' });
  d.edge({ from: 'x5', to: 'x6', impl: 'todo' });
  d.group({ id: 'gB', x: 20, y: 340, w: 1680, h: 230, title: 'B. 每期租金計算（R01、R02）' });
  d.row({ x: 40, y: 384, w: W, h: 166, gap: S - W, items: [
    { id: 'y1', impl: 'todo', clar: 'open', tags: ['R02', 'Q-24'], title: '付款期程', sub: '每月／每季／每年；固定或變動；公證日起 20 工作天等規則' },
    { id: 'y2', impl: 'todo', tags: ['R02'], title: '度數來源', sub: '台電餘電度數＋天能轉供度數（指定資料夾）' },
    { id: 'y3', impl: 'partial', clar: 'open', tags: ['R01', 'FR-016', 'FR-018'], title: '租金規則（版本）', sub: '固定、比例、階段、特殊年租；既有計費策略可擴充' },
    { id: 'y4', impl: 'partial', clar: 'open', tags: ['R01', 'Q-11'], title: '計算', sub: '購電電費(未稅)＝餘電度×餘電費率＋轉供度×轉供費率；租金＝購電電費×租金率' },
    { id: 'y5', impl: 'todo', clar: 'open', tags: ['Q-11'], title: '稅與扣繳', sub: '所得稅、補充保費、營業稅；四捨五入' },
    { id: 'y6', impl: 'todo', clar: 'open', tags: ['FR-019', 'Q-22'], title: '多地主分配', sub: '比例或固定金額；繼承異動生效日' },
    { id: 'y7', impl: 'todo', tags: ['FR-017'], title: '人工覆核', sub: '手動調整＋原因；確認金額；保留規則版本' },
  ] });
  d.chain(['y1', 'y2', 'y3', 'y4', 'y5', 'y6', 'y7'], { impl: 'todo' });
  d.edge({ from: 'x3', to: 'y1', fromSide: 'b', toSide: 't', impl: 'todo', route: 'v', mid: 330, label: '合約條款', labelAt: [S + 100, 330] });
  d.group({ id: 'gC', x: 20, y: 590, w: 1680, h: 230, title: 'C. 通知、發票與付款（R03、R04、R08）' });
  d.row({ x: 40, y: 634, w: W, h: 166, gap: S - W, items: [
    { id: 'z1', impl: 'todo', clar: 'open', tags: ['R03', 'Q-19'], title: '付款通知', sub: '依受款人的罐頭訊息；紙本或 Email；附電費單' },
    { id: 'z2', impl: 'todo', clar: 'open', tags: ['FR-027'], title: 'Outlook 批次寄送', sub: '技術與資安待驗證' },
    { id: 'z3', impl: 'todo', clar: 'open', tags: ['R04', 'Q-23'], title: '受款人發票／收據', sub: '公司或機關才需要；先附或後補' },
    { id: 'z4', impl: 'todo', tags: ['R04', 'FR-025'], title: '未收提醒', sub: '後補沒收到就提醒' },
    { id: 'z5', impl: 'todo', clar: 'open', tags: ['R08'], title: '付款申請單', sub: '系統資訊帶入；附租金計算表、電費單、發票' },
    { id: 'z6', impl: 'external', clar: 'open', tags: ['Q-05'], title: 'EasyFlow／EIP', sub: '檢核與簽准' },
    { id: 'z7', impl: 'todo', clar: 'open', tags: ['FR-030', 'Q-20'], title: '付款追蹤', sub: '單號、核准日、實際付款日：能否讀回？' },
  ] });
  d.chain(['z1', 'z2'], { impl: 'todo' });
  d.chain(['z3', 'z4', 'z5', 'z6', 'z7'], { impl: 'todo' });
  d.edge({ from: 'y7', to: 'z1', fromSide: 'b', toSide: 't', impl: 'todo', points: [[6 * S + 147, 582], [147, 582]], label: '確認金額後', labelAt: [3 * S + 147, 582] });
  d.group({ id: 'gD', x: 20, y: 840, w: 1680, h: 190, title: 'D. 報表與可沿用的既有元件' });
  d.row({ x: 40, y: 884, w: 395, h: 126, gap: 20, items: [
    { id: 'w1', impl: 'todo', tags: ['R07'], title: '租金與發電報表', sub: '依案場、年份、月份篩選：發電度數、租金支出；可匯出' },
    { id: 'w2', impl: 'partial', title: '既有計費引擎 rating（可沿用）', sub: '策略即資料、效期綁定、不可變紀錄、定點數；要改成每期×合約×地主' },
    { id: 'w3', impl: 'partial', title: '既有行事曆 calendar_event（可沿用）', sub: '目前只有期數到期；可擴充成合約到期、付款日、未收單' },
    { id: 'w4', impl: 'done', title: '既有稽核 audit（可沿用）', sub: '所有異動同交易留痕，滿足「操作紀錄」' },
  ] });
  d.legend(20, 1050, 1680);
}

// ───────────────────────────── A7 簽核分工決策點 ─────────────────────────────
{
  const d = make({
    id: 'A7', title: '系統內簽核引擎 與 EasyFlow 的分工（待決策）', width: 1720, height: 1020,
    subtitle: 'energy-ops-rust 已有完整的系統內簽核引擎與四條預設流程，但合約要求的簽核是在 EasyFlow 建單加簽。這張圖並列兩邊，右下是三個可選方案（Q-04）。',
    basis: 'energy-ops-rust workflow_defs.rs、WORKFLOW.md；SOP 第 5、6 節；天能轉供電費開立發票申請（EasyFlow 單號 202609080026）',
  });
  d.group({ id: 'gL', x: 20, y: 110, w: 860, h: 800, title: 'energy-ops-rust 系統內簽核引擎（合約外既有）' });
  d.node({ id: 'eng', x: 40, y: 156, w: 820, h: 112, impl: 'beyond', title: '引擎能力（已實作、有測試）', sub: '定義有版本；五種動作（推進、回退、設狀態、副作用、中斷）；主責者規則 R1～R4；並行先搶先贏；回退鎖（已付款不可退）；中斷要核可；代理、轉移、強制推進、終止；血緣' });
  const flow = (y, label, nodes, extra = {}) => {
    d.note({ x: 40, y, w: 820, h: 26, tone: 'info', fs: 12, text: label });
    const w = Math.min(120, (820 - (nodes.length - 1) * 12) / nodes.length);
    nodes.forEach((n, i) => d.node({ id: n.id, x: 40 + i * (w + 12), y: y + 34, w, h: n.tags ? 82 : 64, small: true, align: 'center', center: true, chip: false, impl: n.impl || 'beyond', clar: n.clar || 'assumed', title: n.t, sub: n.s, tags: n.tags, ...extra }));
    d.chain(nodes.map((n) => n.id), { impl: 'beyond' });
  };
  flow(290, 'bill-approval：躉購電費單核可（7 關，預設值）', [
    { id: 'f1a', t: '開始' }, { id: 'f1b', t: '承辦確認', s: '業務部' }, { id: 'f1c', t: '主管核准', s: '發起人主管' },
    { id: 'f1d', t: '高層加簽', s: '≥100,000', clar: 'open', tags: ['Q-21'] }, { id: 'f1e', t: '計算應付', s: '計費引擎', impl: 'partial', clar: 'open', tags: ['Q-11'] },
    { id: 'f1f', t: '財務放行', s: '財務' }, { id: 'f1g', t: '付款', s: '會計部；不可退' }, { id: 'f1h', t: '完成' },
  ]);
  flow(430, 'utility-bill：用電電費單付款（無計算節點）', [
    { id: 'f2a', t: '開始' }, { id: 'f2b', t: '承辦確認' }, { id: 'f2c', t: '主管核准' }, { id: 'f2d', t: '財務放行' }, { id: 'f2e', t: '付款' }, { id: 'f2f', t: '完成' },
  ]);
  flow(560, 'deposit-request：保證金請款（subject＝期數）', [
    { id: 'f3a', t: '送審' }, { id: 'f3b', t: '財務放行' }, { id: 'f3c', t: '付款', clar: 'open', tags: ['Q-18'] }, { id: 'f3d', t: '完成' },
  ]);
  flow(680, 'invoice-issue：開立發票（與核可平行）', [
    { id: 'f4a', t: '開始' }, { id: 'f4b', t: '開立發票', s: '會計部' }, { id: 'f4c', t: '完成' },
  ]);
  d.note({ x: 40, y: 806, w: 820, h: 84, tone: 'warn', fs: 12.5, text: '四條流程都是設計時的 seed 預設值（DESIGN.md §3「預設值不是需求」），從未經雲豹確認；加簽門檻 100,000 也是預設。合約驗收表沒有任何一項要求系統內簽核。' });
  d.group({ id: 'gR', x: 900, y: 110, w: 800, h: 450, title: 'EasyFlow（雲豹現行簽核系統，合約要求在這裡建單）' });
  d.node({ id: 'ef1', x: 920, y: 156, w: 370, h: 150, impl: 'external', tags: ['B11', 'R08'], title: '付款申請單', sub: '公司代碼、費用種類（有／無合約）、申請摘要、付款對象、付款方式、憑證（BBR）、申請事由、金額；附件' });
  d.node({ id: 'ef4', x: 1310, y: 156, w: 370, h: 150, impl: 'external', tags: ['B12', 'B13'], title: '開立（作廢）發票申請單', sub: '單別 ODMMFA012：收款對象、統編、開立金額（未稅）、項目說明、預計收款日、相關憑證' });
  d.node({ id: 'ef2', x: 920, y: 340, w: 370, h: 130, impl: 'external', title: '加簽', sub: 'SOP：重設流程 → 指定人員串簽至最上方；開票加簽工程部' });
  d.node({ id: 'ef3', x: 1310, y: 340, w: 370, h: 130, impl: 'external', title: '會計群組核准', sub: '標準群組「會計」：任一人核准即通過（其他人顯示他人已簽核）' });
  d.edge({ from: 'ef1', to: 'ef2', impl: 'external' });
  d.edge({ from: 'ef2', to: 'ef3', impl: 'external' });
  d.edge({ from: 'ef4', to: 'ef3', impl: 'external' });
  d.note({ x: 920, y: 490, w: 760, h: 52, tone: 'info', fs: 12, text: '簽核結果（核准日、付款日）能否從 EasyFlow／EIP 讀回，決定付款追蹤能不能自動化（Q-20）。' });
  d.group({ id: 'gD', x: 900, y: 580, w: 800, h: 330, title: '要決定：兩套簽核怎麼分工（Q-04）', clar: 'open', fill: '#FFFBF5' });
  d.row({ x: 920, y: 626, w: 245, h: 190, gap: 12, impl: 'todo', clar: 'open', items: [
    { id: 'o1', title: '方案 A：簽核全在 EasyFlow', sub: '系統只做收件、辨識、覆核、產生申請單內容並送 EasyFlow；系統內引擎停用或隱藏。最貼合合約、驗收面最小、教育訓練最單純。' },
    { id: 'o2', title: '方案 B：系統內一關覆核＋EasyFlow', sub: '承辦覆核（或主管確認）後才送 EasyFlow；保留稽核、回退鎖與退回理由。使用者多一步，但資料進 EasyFlow 前有人把關。' },
    { id: 'o3', title: '方案 C：維持內部多關卡＋EasyFlow', sub: '現有 7 關流程照跑，最後再送 EasyFlow。同一張單要簽兩次，需要能向雲豹說明額外價值。' },
  ] });
  d.note({ x: 920, y: 830, w: 760, h: 64, tone: 'warn', fs: 12, text: '影響範圍：B11～B13、R08 的做法；「付款」關卡的已付款鎖是否保留；角色與權限設計；教育訓練與使用手冊內容；驗收時要展示哪些畫面。' });
  d.edge({ from: 'gL', to: 'gD', fromSide: 'r', toSide: 'l', fromAt: 0.86, toAt: 0.5, impl: 'beyond', clar: 'open', label: '去留？' });
  d.legend(20, 930, 1680);
}

export function writeDiagrams() {
  mkdirSync(OUT, { recursive: true });
  const summary = {};
  for (const d of diagrams) {
    writeFileSync(join(OUT, `${d.id}.svg`), d.render());
    summary[d.id] = { title: d.title, stats: d.stats() };
  }
  writeFileSync(join(OUT, 'stats.json'), `${JSON.stringify(summary, null, 2)}\n`);
  return diagrams.length;
}

if (import.meta.url === pathToFileURL(process.argv[1]).href) {
  console.log(`已產生 ${writeDiagrams()} 張圖 → ${OUT}`);
}
