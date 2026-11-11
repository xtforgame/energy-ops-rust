// 規格文件的敘述內容。清冊資料在 registers.mjs；這裡只寫說明文字與版面。
// 每個函式收到 ctx（產生連結用）與 H（狀態標籤），回傳 HTML 片段。
import { esc } from '../tools/lib.mjs';
import * as R from './registers.mjs';

const P = {
  summary: '00-總覽/01-專案摘要.html', sources: '00-總覽/02-規格來源與世代.html', legend: '00-總覽/03-圖例與編號規則.html', glossary: '00-總覽/04-名詞表.html',
  acceptance: '20-驗收/合約驗收對照表.html', requirements: '30-需求/需求總表.html', questions: '40-清冊/待釐清問題.html', testdata: '40-清冊/測試資料盤點.html',
  inventory: '40-清冊/實作現況.html', data: '40-清冊/假資料與不確定資料.html', decisions: '40-清冊/決策紀錄.html', compare: '40-清冊/程式碼比對紀錄.html',
};

export const ARCH = {
  A0: {
    slug: '系統情境', short: '系統情境',
    intro: () => `<p class="lead">最外層的一張圖：系統在雲豹內部的位置。中間是要交付的「${'電費單與租金管理系統'}」，左邊是使用者，右邊是外部系統與往來對象，下方是現在靠 Excel 維護、要被系統匯入或取代的資料。</p>
<p>每條連線都標了它對應的合約驗收項。可以看出：系統跟外部之間「要往來」的線幾乎都是虛線（未實作）或點線（待釐清）——驗收的工作量主要落在介接，而不是系統內部。</p>`,
    points: [
      '進出系統的資料有三個入口（信箱、手動上傳、Excel 匯入）與四個出口（公槽存檔、EasyFlow 申請單、付款通知、報表）；目前只有手動上傳一條是通的。',
      '公用信箱、公槽、EasyFlow、使用者連線方式都要雲豹 IT 開通，四題都在第一批問題（Q-05、Q-06、Q-07、Q-09）。',
      '辨識服務是外部交付物，隨離線包部署；文字層 PDF 已驗證，影像與新樣態尚未驗證（Q-15）。',
      '會計系統與銀行明確不串接（DEC-06）；LINE 群組歸檔與發票寄送台電是現行作法，合約未列（Q-07、Q-27）。',
    ],
  },
  A1: {
    slug: '業務流程', short: '業務流程全景',
    intro: () => `<p class="lead">把雲豹現在的人工作業（SOP、超強表格、資產管理清單）攤成四條業務線，逐步標出系統化之後由誰做、現在做到哪。</p>
<p>讀法：每一列從左到右是作業順序；方塊填色是 energy-ops-rust 現在能不能做，邊框是規格定了沒有，標籤是對應的驗收項與問題。跨線的箭頭是資料依賴——租金需要台電的餘電度數與天能的轉供度數。</p>`,
    points: [
      '① 台電電費單：現行 SOP 的 9 個步驟中，系統只完整具備「人工覆核」；辨識與對案場是部分具備（缺子公司、區處與正式欄位）。',
      '① 下方藍色方塊是系統內既有的多關卡核可，合約沒有要求；它跟 EasyFlow 的關係是本期最大的設計決策（Q-04）。',
      '② 天能轉供單整條線都還沒有：來源、合併、辨識、開票申請都未實作。',
      '③ 租金：只有計算引擎可沿用，但現在是「每張電費單算一次」，實際需要「每期×每份合約×每位地主」（R01、R02）。',
      '④ 資產與合約管理：稽核與權限可沿用，其餘（匯入、維護、提醒、報表）都未實作；履保金與保險是否本期交付待定（Q-02）。',
    ],
  },
  A2: {
    slug: '部署與容器', short: '部署與容器',
    intro: () => `<p class="lead">系統實際怎麼跑。客戶端是 Windows Server 2022 上的 WSL 發行版，裡面跑 platform（energy-ops-rust）、postgres、minio、ocr 四個容器；右側是我方開發環境與不再採用的舊版。</p>
<p>依安裝指南，離線安裝包（2026-11-04）可在主機上安裝並通過健康檢查，且已驗證 4 份 PDF 的 54 項預期值；但打包原始碼不在 energy-ops-rust 裡，指南也只說明在主機本機以 127.0.0.1 使用。雲豹主機的實際安裝狀態，本次沒有確認。內網資源（信箱、公槽、EasyFlow、同仁電腦）全部還沒接。</p>`,
    points: [
      '離線包只做過單筆試跑（4 vCPU／8 GB），批次與容量未實測（Q-32）。',
      '目前要在主機的遠端桌面內開 127.0.0.1:11950，與需求總表 NFR-002「內部網路登入與操作」不符（Q-09）。',
      '開機自動啟動、備份、監控、日誌都未規劃，但合約維護期有 4 小時回應與逾期罰款（Q-31、D08）。',
      '依部署紀錄，App 5 線上 Demo（Vercel＋Supabase）在線上（本次未確認），去留待定（Q-10）。',
    ],
  },
  A3: {
    slug: '應用元件', short: '應用元件',
    intro: () => `<p class="lead">energy-ops-rust 的每個元件，以及它對合約的意義。綠色可以直接用於驗收；藍色已實作但合約沒有要求；白色虛框是驗收需要、目前沒有的元件。</p>
<p>既有元件的品質不差（有測試、有稽核、有型別閘），問題在方向：投資最多的系統內簽核引擎、多組織與 RBAC 管理不在驗收表上，而驗收表需要的十個元件都還沒有。</p>`,
    points: [
      '可直接沿用：收件與辨識驅動、電費單三層值與校對台、附件、稽核、權限、部門。',
      '要擴充：主檔（只有讀取）、計費（改成每期租金）、工作台（加通知與報表）、抽取（天能與新樣態）。',
      '要新增：郵件收件器、公槽歸檔器、天能單解析、紀錄單產生器、EasyFlow 連接器、發票管理、租金期別計算、提醒排程、主檔匯入編修匯出、報表。',
      '合約外：系統內簽核引擎、流程單與流程總覽、效期綁定、多組織與邀請、RBAC 管理頁、3D 地圖——建議依 Q-04、Q-34 決定隱藏或保留。',
    ],
  },
  A4: {
    slug: '資料模型', short: '資料模型',
    intro: () => `<p class="lead">左邊是 energy-ops-rust 現有的 41 張資料表（13 張平台、28 張能源），依主題分組；右邊是 DB 欄位字典（DB-001～153）與合約驗收要求、但現在沒有的資料。</p>
<p>右側每個方塊都寫了「由哪張現有表擴充」或「新表」。最關鍵的模型差異有三個：一個案場要能有多個電號、地主之外要有子公司與一般受款人、租金要從「每張電費單」改成「每期×合約×地主」。</p>`,
    points: [
      '案場只連一位地主、只有一個電號；子公司（持有公司）與區處都沒有欄位，B04 因此只能部分具備。',
      '電費單的最終值只有期間、總額、度數等少數欄位；B05、B06 要的欄位辨識服務已輸出，但只存在辨識原始資料中。',
      '現有的「期數」是建設期保證金付給地主的分期，與資產表的「履保金退回」語意可能相反（Q-18）。',
      '系統內流程 11 張表屬於合約外功能，去留跟著 Q-04。',
    ],
  },
  A5: {
    slug: '電費單處理', short: '電費單處理細部',
    intro: () => `<p class="lead">表一（B01～B15）的完整管線：A 收件與歸檔 → B 辨識與覆核 → C 電費紀錄單與 EasyFlow 申請單 → D 發票開立之後。</p>
<p>綠色是現在就能用的部分（手動上傳、例外件、人工覆核）；中段的辨識是部分具備；前段的自動收件與後段的紀錄單、申請單、發票全部要補。</p>`,
    points: [
      'B 區的辨識與覆核是既有系統最強的部分，可以直接成為新管線的中段。',
      '自動收件要先解決 PDF 密碼：SOP 現在把密碼寫在文件裡，系統若要自動解密，需要安全的保管方式（Q-14）。',
      '檔名規則可以從 SOP 與超強表格的公式整理出來，但仍需雲豹簽認（Q-07）。',
      '電費紀錄單是 B09、B10、B15 的共同核心，但現行範本沒有提供（Q-08）。',
      'EasyFlow 的送單方式未定；最保守的做法是系統產出申請內容與附件包，由人在 EasyFlow 送出（Q-05）。',
    ],
  },
  A6: {
    slug: '租金與資產', short: '租金與資產細部',
    intro: () => `<p class="lead">表二（R01～R08）的完整管線：A 資產主檔取代 Excel 總表 → B 每期租金計算 → C 通知、發票與付款申請 → D 報表與可沿用的既有元件。</p>
<p>依租金計算範例，購電電費（未稅）＝餘電度數×餘電費率＋轉供度數×轉供費率，租金＝購電電費×租金率，再加計所得稅、補充保費、營業稅。範例活頁簿本身有錯，不能直接當標準答案（DATA-11）。</p>`,
    points: [
      '可沿用：計費引擎（策略即資料、效期、不可變紀錄）、行事曆、稽核。',
      '租金計算需要兩種度數來源：台電單的餘電度數與天能單的轉供度數；天能單目前完全不支援。',
      '多地主、繼承異動、特殊年租、階段式租金都需要雲豹提供規則與標準答案（Q-11、Q-22）。',
      '付款通知、受款人發票、付款申請單都依賴外部（紙本、Email、EasyFlow），做法待定（Q-19、Q-23、Q-05）。',
    ],
  },
  A7: {
    slug: '簽核分工', short: '簽核分工（決策點）',
    intro: () => `<p class="lead">energy-ops-rust 有完整的系統內簽核引擎與四條預設流程；合約要求的簽核則是在 EasyFlow 建單、加簽、由會計群組核准。這張圖並列兩邊，右下是三個可選方案。</p>
<p>這是本期最需要先決定的一題：它決定 B11～B13、R08 的做法、付款鎖是否保留、角色與權限怎麼設計，以及教育訓練與驗收時要展示哪些畫面。</p>`,
    points: [
      '四條流程與加簽門檻 100,000 都是設計時的預設值，從未經雲豹確認（DATA-07）。',
      '方案 A（簽核全在 EasyFlow）最貼合合約、驗收面最小；方案 B 保留一關覆核與稽核；方案 C 會讓同一張單簽兩次。',
      'EasyFlow 的會計群組是「任一人核准即通過」，與系統內的「先搶先贏」語意相同。',
      '無論選哪個方案，核准日與付款日能否讀回都決定付款追蹤能不能自動化（Q-20）。',
    ],
  },
};

const archPath = (id) => `10-架構/${id}-${ARCH[id].slug}.html`;

function daysLeft() {
  const a = Date.UTC(2026, 10, 12);
  const b = Date.UTC(2026, 10, 30);
  return Math.round((b - a) / 86400000);
}

function findings(ctx) {
  return `<div class="callout risk"><h4>一、驗收基準與既有實作錯位</h4>
<p>合約驗收表的重心是「自動收件、電費紀錄單、EasyFlow 申請單、發票存檔、租金與資產管理」；energy-ops-rust 投資最多的是系統內多關卡簽核、多組織與完整 RBAC——這些都不在驗收表上。23 個驗收項中，沒有一項完整具備。</p></div>
<div class="callout warn"><h4>二、外部介接全部未定</h4>
<p>公用信箱、公槽、EasyFlow、同仁電腦連線、PDF 密碼保管——每一項都要雲豹 IT 開通或決定，而且都還沒有測試環境。它們是時程上最大的不確定。</p></div>
<div class="callout warn"><h4>三、規格資料不完整，部分有錯</h4>
<p>電費紀錄單的範本沒有提供；租金計算範例有 #VALUE! 與錯位公式；需求總表是機器轉錄的 v0.1；合約附件版本待核；辨識樣本只驗證過 4 種。這些都要先補齊，驗收才判得出合不合格。</p></div>`;
}

export const PAGES = {
  home(ctx, H, { tiles, coverageBar, legend }) {
    const ab = R.acceptance.filter((a) => a.table !== '交付');
    const c1 = R.countBy(R.acceptance.filter((a) => a.table === '表一'));
    const c2 = R.countBy(R.acceptance.filter((a) => a.table === '表二'));
    const b1 = R.questions.filter((q) => q.batch === 1).length;
    const docs = [
      [P.summary, '專案摘要', '合約要點、現況一句話、關鍵發現、主要風險、建議的下一步。給決策者的第一頁。'],
      [P.sources, '規格來源與世代', '三個來源、四個世代、衝突時以誰為準、遠端抓取結果、來源檔案清冊（含機敏標示）。'],
      [P.legend, '圖例與編號規則', '狀態的顏色與樣式、編號規則、架構圖的讀法。'],
      [P.glossary, '名詞表', '電號、躉購、轉供、子公司、區處、BBR、EasyFlow、公槽等名詞。'],
      [archPath('A0'), '架構圖 A0～A7', '從系統情境、業務流程、部署、元件、資料，到電費單與租金的細部，以及簽核分工的決策點。'],
      [P.acceptance, '合約驗收對照表', '表一 B01～B15、表二 R01～R08、交付 D01～D08：合約原文、現況、缺口、驗收方法草案、待釐清問題。'],
      [P.requirements, '需求總表與使用者故事', 'FR-001～035、NFR-001～003 的現況與對應驗收；原版設計的 US-1～21。'],
      [P.questions, '待釐清問題（分四批）', `Q-01～Q-${R.questions.length}；第一批 ${b1} 題是排工前必須決定的；第四批是測試資料索取。`],
      [P.testdata, '測試資料盤點', '每個驗收項需要哪些測試資料、現在有沒有、在哪裡（含各 branch）；案場與地主資料的現況。'],
      [P.inventory, '實作現況清冊', 'energy-ops-rust 每個元件的狀態、對合約的意義與證據路徑。'],
      [P.data, '假資料與不確定資料', '示範資料、mock、預設值、範例錯誤、未驗證的辨識與容量。'],
      [P.decisions, '決策紀錄', '已經決定的事、誰決定的、在新合約脈絡下是否要重新確認。'],
      [P.compare, '程式碼比對紀錄', '三個 repo 的遠端抓取結果；App 5 未併修正與 Rust 版的逐項比對。'],
    ];
    return `<p class="lead">這是「${'電費單與租金管理系統'}」的規格文件系統（${'v0.1'} 盤點版）。它以雲豹委託開發合約的驗收表為基準，盤點三個來源（原版設計文件、App 5、energy-ops-rust）的規格與實作，用顏色與樣式標出每個部分「做到哪」與「定了沒」，並把所有不清楚的地方整理成分批的待釐清問題。</p>
<div class="hero"><span class="num">${daysLeft()}</span><span class="unit">天</span><span class="what">截至 2026-11-12，距合約約定的完成並驗收期限（民國 115 年 11 月 30 日）。</span></div>
${tiles([
  { label: '表一　電費單驗收項', value: '15 項', sub: `已具備 ${c1.done || 0}、部分 ${c1.partial || 0}、未實作 ${c1.todo || 0}` },
  { label: '表二　租金驗收項', value: '8 項', sub: `已具備 ${c2.done || 0}、部分 ${c2.partial || 0}、未實作 ${c2.todo || 0}` },
  { label: '待釐清問題', value: `${R.questions.length} 題`, sub: `第一批 ${b1} 題要先回覆` },
  { label: '假資料與不確定資料', value: `${R.dataIssues.length} 筆`, sub: '示範資料、mock、範例錯誤、未驗證' },
  { label: '測試資料', value: `${R.testData.length} 類`, sub: `已有 ${R.countBy(R.testData, 'have').yes || 0}、部分 ${R.countBy(R.testData, 'have').partial || 0}、沒有 ${R.countBy(R.testData, 'have').no || 0}` },
])}
${coverageBar(ab, '合約驗收項的實作覆蓋')}
<h2>關鍵發現</h2>
${findings(ctx)}
<h2>閱讀路徑</h2>
<div class="tbl"><table class="reg"><thead><tr><th style="width:150px">讀者</th><th>建議順序</th></tr></thead><tbody>
<tr><td>決策者（甲方 PM）</td><td><a href="${ctx.href(P.summary)}">專案摘要</a> → <a href="${ctx.href(archPath('A0'))}">A0 系統情境</a> → <a href="${ctx.href(archPath('A7'))}">A7 簽核分工</a> → <a href="${ctx.href(P.questions, 'batch-1')}">第一批問題</a></td></tr>
<tr><td>開發者</td><td><a href="${ctx.href(archPath('A3'))}">A3 應用元件</a> → <a href="${ctx.href(archPath('A4'))}">A4 資料模型</a> → <a href="${ctx.href(P.acceptance)}">合約驗收對照表</a> → <a href="${ctx.href(P.inventory)}">實作現況清冊</a> → <a href="${ctx.href(P.compare)}">程式碼比對紀錄</a></td></tr>
<tr><td>與雲豹確認時</td><td><a href="${ctx.href(archPath('A1'))}">A1 業務流程全景</a> → <a href="${ctx.href(archPath('A5'))}">A5 電費單處理</a> → <a href="${ctx.href(archPath('A6'))}">A6 租金與資產</a> → <a href="${ctx.href(P.questions, 'batch-2')}">第二批問題</a> → <a href="${ctx.href(P.testdata)}">測試資料盤點</a> → <a href="${ctx.href(P.questions, 'batch-4')}">第四批問題</a>（注意：來源檔案清冊標示為機敏的內容不要外流）</td></tr>
</tbody></table></div>
<h2>文件目錄</h2>
<div class="tbl"><table class="reg"><thead><tr><th style="width:200px">文件</th><th>內容</th></tr></thead><tbody>
${docs.map(([p, t, d]) => `<tr><td><a href="${ctx.href(p)}">${esc(t)}</a></td><td>${esc(d)}</td></tr>`).join('\n')}
</tbody></table></div>
<h2>架構圖（全局 → 細節）</h2>
<div class="thumbs">${Object.keys(ARCH).map((id) => `<a class="thumb" href="${ctx.href(archPath(id))}"><img src="${ctx.asset(`diagrams/${id}.svg`)}" alt="${esc(`${id} ${ARCH[id].short}`)}" loading="lazy"><div><strong>${esc(id)}　${esc(ARCH[id].short)}</strong></div></a>`).join('')}</div>
<h2>圖例</h2>
${legend()}`;
  },

  summary(ctx, H, { tiles, coverageBar, linkRefs, refLink }) {
    const ab = R.acceptance.filter((a) => a.table !== '交付');
    const risks = [
      ['期限', '合約約定 115/11/30 前完成建置並驗收合格；目前 23 個驗收項沒有一項完整具備。', '第二期款依驗收交付；逾期可能被催告終止。', '申請書面展延，或與雲豹書面約定本期縮小範圍。', ['Q-01', 'Q-02']],
      ['外部介接', '信箱、公槽、EasyFlow、同仁電腦連線都要 IT 開通，目前沒有測試環境。', 'B01、B02、B10、B11～B14、R08 無法開工或驗收。', '第一批問題先送 IT；EasyFlow 先採最保守做法（系統產出申請內容，人工送出）。', ['Q-05', 'Q-06', 'Q-07', 'Q-09', 'Q-38']],
      ['簽核重複', '系統內多關卡簽核與 EasyFlow 並存。', '使用者同一張單要簽兩次；驗收時說不清哪一套才算數。', '先決定 Q-04（建議評估方案 A）。', ['Q-04']],
      ['計算正確性', '租金沒有標準答案，範例活頁簿有錯。', 'R01、R02 無法判定合格。', '請雲豹提供已實際付款的案例當標準答案。', ['Q-11', 'Q-22']],
      ['辨識準確度', '只有 4 種樣態驗證過；天能、影像、傳真影本與新樣態未驗證；合約沒有正確率門檻。', 'B04～B08 的驗收爭議。', '擴充樣本與標準答案，並約定門檻。', ['Q-03', 'Q-15']],
      ['機敏資料', 'SOP 寫著台電網站帳密與 PDF 密碼；資產表含個資與銀行帳號；示範資料用了真實電號。', '資安與個資風險；合約有保密條款。', '密碼改由系統安全保管；銀行帳號加密；示範資料改用虛構電號。', ['Q-14', 'Q-29', 'Q-36']],
      ['維運', '開機自啟、備份、監控、日誌都未規劃。', '維護期 4 小時回應、逾期每日罰款。', '在交付前把維運手冊與工具補齊。', ['Q-31', 'Q-32']],
      ['測試資料不足', '22 類測試資料中，沒有任何真實地主、合約、天能轉供單、已開發票與租金標準答案；案場只有名單、尚未匯入。', '表二幾乎全部、表一的天能與發票各項無法開發或驗收。', '第四批問題與第一批同時發出；建議約定一個「驗收基準月」資料集。', ['Q-40', 'Q-42', 'Q-48']],
    ];
    return `<p class="lead">一頁看懂：合約要什麼、現在做到哪、最大的落差與風險、下一步該先決定什麼。</p>
<h2>一、合約要點</h2>
<div class="tbl"><table class="reg"><tbody>
<tr><th style="width:150px">標的</th><td>電費單與租金管理系統（合約檔名：綠電轉供組態管理系統－電費單管理委託合約，修改附件驗收項目版）</td></tr>
<tr><th>當事人</th><td>甲方（開發）：煦策顧問有限公司；乙方（委託）：雲豹能源科技股份有限公司</td></tr>
<tr><th>期限</th><td>民國 115 年 11 月 30 日前完成建置，並經乙方依驗收準則驗收合格（第二條）</td></tr>
<tr><th>驗收</th><td>依附件一表一（電費單，15 項）、表二（租金，8 項）逐項驗收；完成建置與教育訓練後通知驗收；不符者 7 日內改善後重新驗收（第三條）</td></tr>
<tr><th>付款</th><td>分兩期，第二期於驗收交付後支付（第四條；金額不在本文件轉載）</td></tr>
<tr><th>交付</th><td>原始碼、範例、投影片（PPT）、文件（DOC）；安裝、編譯、操作與演算法手冊；主程式、資料庫模組、使用者界面、電費單生成工具、Windows 安裝包；一週教育訓練（附件一）</td></tr>
<tr><th>維護</th><td>驗收合格翌日起一年除錯；4 小時內確認問題；緊急 1 個工作天、非緊急 3 個工作天；逾期每日罰款專案費用 0.1%（第九條）</td></tr>
<tr><th>其他</th><td>智慧財產權歸乙方（第五條）；保密義務含受僱人（第六條）；第一審管轄臺灣臺北地方法院</td></tr>
</tbody></table></div>
<h2>二、現況</h2>
<p>energy-ops-rust 已有「手動收件 → 辨識 → 校對 → 系統內簽核 → 計算 → 稽核」的完整骨幹，以及多組織與完整的權限管理；但合約驗收的重心——自動收件、EasyFlow 申請單、電費紀錄單、發票存檔、租金與資產管理——大多尚未實作。</p>
${tiles([
  { label: '表一　電費單', value: '15 項', sub: '已具備 0、部分 3、未實作 12' },
  { label: '表二　租金', value: '8 項', sub: '已具備 0、部分 2、未實作 6' },
  { label: '交付與義務', value: '8 項', sub: '部分 4、未實作 4' },
])}
${coverageBar(ab, '表一＋表二驗收項的實作覆蓋')}
<p class="muted">判定依據：energy-ops-rust cf7a330 的程式與文件（靜態閱讀）。逐項明細見 <a href="${ctx.href(P.acceptance)}">合約驗收對照表</a>。</p>
<h2>三、關鍵發現</h2>
${findings(ctx)}
<h2>四、主要風險</h2>
<div class="tbl"><table class="reg"><thead><tr><th style="width:96px">風險</th><th>說明</th><th>影響</th><th>建議對策</th><th style="width:150px">相關問題</th></tr></thead><tbody>
${risks.map(([t, d, i, a, q]) => `<tr><td><strong>${esc(t)}</strong></td><td>${esc(d)}</td><td>${esc(i)}</td><td>${esc(a)}</td><td>${q.map(refLink).join(' ')}</td></tr>`).join('\n')}
</tbody></table></div>
<h2>五、建議的下一步</h2>
<ol>
<li>回覆<a href="${ctx.href(P.questions, 'batch-1')}">第一批待釐清問題</a>（13 題），其中 ${refLink('Q-01')} 期限與 ${refLink('Q-04')} 簽核分工最優先。</li>
<li>與雲豹簽一份「驗收測試清單」：每個驗收項列出測試資料、步驟、預期結果、判定人（${refLink('Q-03')}）。</li>
<li>請雲豹 IT 開出測試信箱、測試公槽路徑、EasyFlow 測試環境與帳號（${refLink('Q-38')}）。</li>
<li>同時發出<a href="${ctx.href(P.questions, 'batch-4')}">第四批問題</a>（測試資料索取），並與雲豹約定一個驗收基準月（${refLink('Q-48')}）。</li>
<li>依上述答案，進入第二階段：把每個驗收項展開成可逐條驗收的規格（情境、步驟、預期結果、資料），並更新本文件的狀態。</li>
</ol>`;
  },

  sources(ctx, H, { table, linkRefs }) {
    const gens = [
      ['原版', 'Node／Koa／Sequelize（az monorepo 的 EnergyOpsPlugin＋tenant-console）', 'az repo feat-tenant-console-route-transition（0afe402aba，2026-09-15）', '不採用', '設計文件最完整（需求、領域模型、流程、權限、稽核、附件、計費、辨識、前端、生命週期）；本文件引用時標「RT」。'],
      ['energy-ops-next', 'Next.js 伺服器端 API＋Supabase', 'az repo archive/2026-10-21-feat-energy-ops-next（fa29f3b42b）', '已下線（2026-10-17 當天被 App 5 取代）', '只有 SPEC／DESIGN 參考價值。'],
      ['App 5', 'Supabase Edge Functions＋RLS＋Vercel 前端，單一組織', 'potted-dev-team/workspaces/5-2fv26mb84qh6（main 85631c6）', '依部署紀錄為線上 Demo（energy-ops.vercel.app；本次未確認），去留待定', 'Rust 版的直接移植來源；另有一條未併修正分支。'],
      ['energy-ops-rust', 'Rust 單一執行檔＋Postgres＋MinIO＋React（Vite）', 'energy-ops-rust（master＝cf7a330，2026-11-04）', '最終版本（DEC-01）', '本文件判定「實作狀態」的唯一依據。'],
    ];
    const prec = [
      '合約正式簽署版（含附件一表一、表二）——目前拿到的是「修改附件驗收項目」版，正式版待核（Q-01）。',
      '雙方簽認的驗收測試清單——尚未建立（Q-03）。',
      '需求總表 v0.1（2026-11-08）——機器轉錄整理，明註「不代表已簽核規格」。',
      '現行作業：SOP、電費單超強表格、資產管理清單、租金計算範例——反映實務，但範例有錯。',
      '原版設計文件（RT）與 App 5 文件——內部設計，許多決定是設計者代決。',
      '程式現況（energy-ops-rust）——只說明「做了什麼」，不代表「應該做什麼」。',
    ];
    return `<p class="lead">規格散在三個 repo、四個世代與一批使用者提供的檔案裡。這一頁說明每個來源是什麼、衝突時以誰為準，以及本次抓取與比對的結果。</p>
<h2>衝突時以誰為準</h2>
<ol>${prec.map((p) => `<li>${linkRefs(p)}</li>`).join('')}</ol>
<h2>四個世代</h2>
${table([{ t: '世代', w: '120px' }, '技術', '位置', { t: '狀態', w: '170px' }, '與本文件的關係'], gens.map((g) => ({ cells: g.map((x) => linkRefs(x)) })))}
<p class="muted">使用者指定的三個來源：az repo 的 feat-energy-ops-next 與 feat-tenant-console-route-transition，以及最終版本 energy-ops-rust。feat-energy-ops-next 目前的分支頭已重整，能源相關檔案只剩部署紀錄（deployments/energy-ops.md）、辨識服務（serverless-apps/taipower-recognition）與開發流程的測試夾具；能源設計文件在 archive/2026-10-21 分支。</p>
<h2>遠端抓取結果（2026-11-12）</h2>
<p>三個 repo 的遠端都沒有新提交；App 5 有一條未併入 main、也未被 Rust 版移植的修正分支。詳見 <a href="${ctx.href(P.compare)}">程式碼比對紀錄</a>。</p>
<h2>來源檔案清冊</h2>
<div class="callout warn"><h4>機敏資料處理原則</h4><p>使用者提供的檔案不放進版控，本文件只引用檔名、雜湊與結構，不轉載任何密碼、帳號、銀行資料、個人聯絡方式與合約金額。SOP 內含台電網站帳密與各子公司 PDF 密碼，建議改由系統安全保管並從文件移除（Q-14）。</p></div>
${table([{ t: '類別', w: '150px' }, '檔案', { t: '大小', w: '80px' }, { t: 'sha256（前 16 碼）', w: '150px' }, '用途', { t: '機敏', w: '120px' }],
  R.sources.map((s) => ({ cells: [esc(s.group), esc(s.name), esc(s.size), `<code>${esc(s.sha)}</code>`, linkRefs(s.use), s.sensitive ? `<span class="dt dt-uncertain">${esc(s.sensitive)}</span>` : '<span class="muted">—</span>'] })))}`;
  },

  legendPage(ctx, H, { legend, table }) {
    const ids = [
      ['B01～B15', '合約附件一 表一（電費單管理系統需求暨驗收表）的驗收項', P.acceptance],
      ['R01～R08', '合約附件一 表二（租金管理系統需求暨驗收表）的驗收項', P.acceptance],
      ['D01～D08', '其他交付與契約義務（期限、訓練、手冊、交付物、安裝包、維護）', P.acceptance],
      ['FR-001～035、NFR-001～003', '需求總表 v0.1 的條目（沿用原編號）', P.requirements],
      ['US-1～21', '原版設計文件的使用者故事（沿用原編號）', P.requirements],
      ['Q-01～48', '待釐清問題；第一批 Q-01～13、第二批 Q-14～28、第三批 Q-29～38、第四批（測試資料索取）Q-39～48', P.questions],
      ['TD-01～22', '測試資料盤點的項目', P.testdata],
      ['DATA-01～18', '假資料與不確定資料', P.data],
      ['DEC-01～20', '決策紀錄', P.decisions],
      ['A0～A7', '架構圖，由全局到細節', archPath('A0')],
    ];
    return `<p class="lead">整套文件用同一組狀態與編號。圖與網頁的顏色、邊框完全一致：填色看「做到哪」，邊框看「定了沒」，標籤看「關聯到哪裡」。</p>
<h2>狀態圖例</h2>
${legend()}
<h2>判定原則</h2>
<ul>
<li><strong>實作狀態</strong>以 energy-ops-rust（cf7a330）為準，對照的是「合約與需求要的樣子」，不是「原設計的樣子」。原設計做完、但合約沒有要求的，標「合約外既有」。</li>
<li><strong>部分具備</strong>指已有可沿用的基礎，但欄位、流程、格式或介接至少缺一項；缺口逐項寫在驗收對照表。</li>
<li><strong>釐清狀態</strong>看規格本身：合約原文或使用者明確決定的為「已確認」；設計時的預設值或設計者代決為「預設／假設」；有待釐清問題、答案會改變做法的為「待釐清」。</li>
<li><strong>資料標記</strong>與前兩者獨立：一個元件可以「已具備」但用的是假資料。</li>
<li>本版判定以靜態閱讀程式與文件為主，沒有重跑測試或實機操作；需要實測才能確定的地方會寫明「待實測」。</li>
</ul>
<h2>編號規則</h2>
${table([{ t: '編號', w: '220px' }, '意義', { t: '所在文件', w: '200px' }], ids.map(([a, b, p]) => ({ cells: [`<span class="ref">${esc(a)}</span>`, esc(b), `<a href="${ctx.href(p)}">開啟</a>`] })))}
<h2>架構圖怎麼讀</h2>
<ul>
<li>每張圖左上角有編號、標題、說明與依據來源；底部有完整圖例。</li>
<li>方塊右上角的小標籤直接寫出狀態（例如「未實作・待釐清」），不需要對照顏色。</li>
<li>方塊左下角的標籤是關聯編號：B／R 驗收項、FR 需求、Q 問題；「MOCK」與「不確定」是資料標記。</li>
<li>實線是已具備的連線，虛線是未實作，橘色點線是待釐清。</li>
<li>每張圖的頁面下方有「圖中元素一覽」表格，內容由圖的定義自動產生，與圖面一致。</li>
</ul>
<h2>版本</h2>
<p>v0.1（2026-11-12）：盤點版。只盤點現況、標出狀態與問題，不展開逐條驗收規格。第一批問題回覆後出 v0.2，開始把每個驗收項展開成可逐條驗收的規格。</p>
<h2>修訂紀錄</h2>
<div class="tbl"><table class="reg"><thead><tr><th style="width:120px">版本</th><th style="width:110px">日期</th><th>內容</th></tr></thead><tbody>
<tr><td>v0.1</td><td>2026-11-12</td><td>初版盤點：架構圖 A0～A7、合約驗收對照、需求總表、待釐清問題 Q-01～38、實作現況、假資料、決策紀錄、程式碼比對。</td></tr>
<tr><td>v0.1（同日補充）</td><td>2026-11-12</td><td>加 PDF 輸出；加「測試資料盤點」（TD-01～22，含各 branch 的搜尋結果）與第四批問題 Q-39～48；驗收對照表每項加「測試資料」一列。</td></tr>
</tbody></table></div>`;
  },

  glossary(ctx, H, { table }) {
    return `<p class="lead">文件中常用的業務與系統名詞。</p>${table([{ t: '名詞', w: '200px' }, '說明'], R.glossary.map(([a, b]) => ({ cells: [`<strong>${esc(a)}</strong>`, esc(b)] })))}`;
  },

  acceptanceIntro(ctx) {
    return `<p class="lead">合約附件一的每一個驗收項，對照 energy-ops-rust 的現況：合約原文、現在有什麼、缺什麼、怎麼驗收（草案）、對應哪些需求、還有哪些問題沒答。</p>
<div class="callout note"><p>驗收程序（合約第三條）：完成建置與教育訓練後通知乙方驗收；不符驗收準則者，甲方於收到通知翌日起 7 日內改善並重新提交。附件一的勾選欄只有「合格／不合格」，判定方法需雙方另行約定（<a class="ref ref-q" href="${ctx.href(P.questions, 'Q-03')}">Q-03</a>）。各項「驗收方法」均為草案。</p></div>
<p class="muted">「備註」欄的「整併版對照」是雲豹提供的驗收對照表（電費單掃描資料庫欄位與合約驗收對照_整併版）對需求覆蓋的判斷，與本文件的實作判定是兩件事。</p>`;
  },
  requirementsIntro() {
    return `<p class="lead">需求總表 v0.1（2026-11-08）整理自 08/18 與 11/04 的會議轉錄，共 35 條功能需求與 3 條非功能需求。本頁沿用原編號與原表的「確認狀態」，另加上 energy-ops-rust 的現況與對應的合約驗收項。</p>
<div class="callout warn"><p>原表明註「此為待確認需求，不代表已簽核規格」，且來源轉錄誤辨較多（例如「工潮」疑為「公槽」）。條目文字需與雲豹逐條確認（DATA-13）。</p></div>`;
  },
  storiesIntro() {
    return `<p>以下是原版設計（2026-08）定義的使用者故事，energy-ops-rust 就是照它們做出來的。對照合約可以看出：多數故事已實作，但其中一半屬於系統內簽核，合約沒有要求。</p>`;
  },
  questionsIntro(ctx) {
    return `<p class="lead">所有不清楚的地方都整理成問題，分四批問：第一批是排工前必須決定的範圍、架構與外部系統；第二批是業務規則與資料；第三批是非功能、維運與交付；第四批是測試資料索取，可與第一批同時發出。</p>
<div class="callout note"><p>回覆方式：依批次回覆，每題註明編號即可（例如「Q-04：方案 A」）。不確定的可以回「轉問雲豹」，我們會把該題整理成給雲豹的版本。回覆後本頁的「回覆」欄與相關文件的狀態會一併更新。</p></div>
<p class="muted">問題已與雲豹提供的兩份清單合併去重（需求總表的 Q-01～Q-10、整併版的待確認事項與驗收缺口），「出處」欄註明原編號。</p>`;
  },
  testDataIntro(ctx, H, { refLink }) {
    return `<p class="lead">驗收要能判定合格，每一項都需要測試資料與標準答案。這一頁盤點每類測試資料現在有沒有、在哪裡（包含各 repo 的 branch），沒有的都轉成第四批問題向雲豹索取。</p>
<div class="callout warn"><h4>案場與地主的資料有了嗎？</h4>
<p><strong>案場：有名單，還沒進系統。</strong>使用者提供的「1.發電案場清單」有 98 個真實案場（11 家子公司、16 個區處、電號、容量、發票寄送方式）；系統內與各 branch 只有同一套示範種子的 6 個虛構案場，其中 4 個電號與真實案場相同。名單缺專案編號、型別、併網日、案場分類與合約關聯（${refLink('TD-01')}、${refLink('Q-39')}）。</p>
<p><strong>地主：沒有。</strong>資產管理清單的地主頁只有表頭與一列「我是範例」；原版、App 5、energy-ops-next、energy-ops-rust 的示範種子都是同兩位虛構地主（含假身分證字號與銀行帳號）。多地主比例、繼承異動、公司或機關受款人都沒有資料（${refLink('TD-02')}、${refLink('Q-40')}）。</p></div>
<div class="callout note"><h4>搜尋範圍（2026-11-12）</h4>
<p>az repo 全部 branch 與歷史（含 feat-energy-ops-next、feat-tenant-console-route-transition、archive 與 backup 分支）、App 5 的全部 branch、energy-ops-rust（master、rust-port）、辨識服務的 samples 與 fixtures、az repo 的 temp 與本機上傳區。除了使用者這次提供的附件，找到的案場與地主資料都是示範種子；沒有任何真實地主名冊、合約檔、天能電費單或發票檔。</p></div>
<p>建議做法：與雲豹約定一個「驗收基準月」（${refLink('Q-48')}），由雲豹提供該月全部輸入與人工結果，同一批資料同時當作開發測試資料與驗收的標準答案。含個資的資料可以先去識別化，但要保留結構、比例與金額。</p>`;
  },
  inventoryIntro() {
    return `<p class="lead">energy-ops-rust（cf7a330）每個元件的狀態，以及它對合約的意義：驗收需要、可沿用、要擴充、要改，或屬於合約外。</p>
<p class="muted">依據：程式碼與 docs/OVERVIEW.md、PLAN.md、API-ENERGY.md 的靜態閱讀；測試數字取自 2026-11-04 的文件，本次未重跑。</p>`;
  },
  dataIntro() {
    return `<p class="lead">畫面或程式中「看起來像真的、其實不是」的資料，以及來源本身就不可靠的資料。正式驗收前，每一筆都要有處理結論。</p>`;
  },
  decisionsIntro() {
    return `<p class="lead">已經決定的事。「由誰決定」分五類：合約約定、使用者決定、客戶確認、設計者代決（原設計時依「小事你決定就好」代為決定）、文件推定（從文件內容推得，沒有明確的決定紀錄）。</p>
<p>在合約驗收的新脈絡下，許多原設計的決定需要重新確認；「需重新確認」欄標出這些項目。</p>`;
  },
  compareIntro() {
    return `<p class="lead">2026-11-12 對三個 repo 執行遠端抓取並比對，確認有沒有尚未納入 energy-ops-rust 的新程式碼。</p>
<p class="muted">方法：energy-ops-rust、az repo 以 git fetch／ls-remote 比對分支頭；App 5 以本機保存的 repo 帳密（只在指令中讀取，不寫入設定、不輸出）執行 ls-remote。比對 Rust 版的部分是靜態閱讀程式碼，尚未實測。</p>`;
  },
  compareApp5() {
    return `<p>App 5 的 <code>fix/local-workflow-docker-20261019</code> 分支（56894ad「修正多流程權限、原檔存取與退回操作」、8c99fe2 本地 Docker 驗證紀錄）是在 Rust 版移植之後才出現的修正，沒有被移植。以下逐項比對 Rust 版是否有同樣的問題。</p>`;
  },
  compareSamples() {
    return `<h2>範例電費單與現有測試範本</h2>
<p>使用者這次提供的 8 份範例電費單中，4 份與 energy-ops-rust 測試用的範本完全相同（sha256 一致），另外 4 份是新樣態，辨識尚未驗證。</p>
<div class="tbl"><table class="reg"><thead><tr><th>範例電費單</th><th style="width:170px">sha256（前 16 碼）</th><th>與現有範本</th></tr></thead><tbody>
<tr><td>電費單樣態_1（有餘電需開發票）</td><td><code>bdb4305e73254df2</code></td><td>相同（電費單(有餘電需開發票).pdf）</td></tr>
<tr><td>電費單樣態_2（無餘電，含電表租費及設備發電使用電費）</td><td><code>7a910cdb145d1de3</code></td><td><span class="st st-todo">新樣態</span></td></tr>
<tr><td>電費單樣態_3（無餘電需付款）</td><td><code>388982ae35824bde</code></td><td>相同（電費單(無餘電需付款).pdf）</td></tr>
<tr><td>電費單樣態_4（電表租費有退款情形）</td><td><code>bcfcb4b930e9e9a4</code></td><td><span class="st st-todo">新樣態</span>（掃描影本，有手寫註記）</td></tr>
<tr><td>電費單樣態_5（電表租費有補收款情形）</td><td><code>fc88fb66382a4935</code></td><td><span class="st st-todo">新樣態</span></td></tr>
<tr><td>逆變器電費單樣態_1（含電表租費＆設備發電使用電費）</td><td><code>037a2a7d3b170fa5</code></td><td>相同</td></tr>
<tr><td>逆變器電費單樣態_2（含電表租費＆監控及攝影機使用電費）</td><td><code>f26f8db41b1759d9</code></td><td><span class="st st-todo">新樣態</span></td></tr>
<tr><td>逆變器電費單樣態_3（僅設備發電使用電費）</td><td><code>2fea70b3e8a2f947</code></td><td>相同</td></tr>
</tbody></table></div>
<h2>其他觀察</h2>
<ul>
<li>az repo 遠端另有 <code>feat-rust-ontology-poc</code>（Object Explorer 示範）與 <code>try-rust</code>（暫存），與能源業務無關。</li>
<li>離線安裝包的打包程式在本機與 GitHub 都找不到；若在另一台電腦，建議推上 energy-ops-rust（Q-09）。</li>
</ul>`;
  },
};
