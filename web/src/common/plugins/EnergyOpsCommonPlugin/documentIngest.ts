/**
 * 文件分類器 —— 「這份掃描件屬於哪一個工作流？」（`IA.md §7-2`）
 *
 * **與 OCR 邊界（`billExtractor.ts`）、geocode 邊界（`geocoder.ts`）同一招**：
 * 介面是我們的，實作是可換的。不要在攝取路徑裡直接寫死判斷。
 *
 * 為什麼需要它：原本的 `billExtractor` **只處理電費單** ——
 * 整條攝取路徑隱含假設「丟進來的一定是電費單」。
 * 使用者要的是「丟一批混合的檔案進來，系統自己分」，那需要在抽取**之前**多一層。
 *
 * **判不出來就標 `unclassified`，絕不猜**（`IA.md §8-5` 拍板）：
 * 猜錯的成本（整張單走錯流程、算錯錢、找不回來）**遠高於**多請人指定一次。
 * 這與 geocoder 的「查不到回 null 不回退成行政區中心」是同一條原則。
 */

/**
 * 認得的文件型別。新增型別時**同時**要決定它產生什麼列（見 `docTypeToWorkflow`）。
 *
 * ### 為什麼是四種單而不是兩種
 *
 * 一開始只分「躉購（台電付我們）」與「用電（我們付台電）」兩種，因為那是
 * **錢的方向**，而錢的方向決定要不要算分潤。但客戶手上的四份範本，
 * 每一份的後續作業都不一樣：
 *
 * | 範本 | 錢的方向 | 後續作業 |
 * |---|---|---|
 * | 躉購·有餘電 | 台電付我們 | 核可 ＋ **開發票給台電** |
 * | 躉購·無餘電 | 我們付台電（只剩電表租費） | 付款，**不會有分潤** |
 * | 逆變器·僅設備發電使用電費 | 我們付台電 | 付款 |
 * | 逆變器·含電器租費 | 我們付台電 | 付款（租費要另外對帳） |
 *
 * 兩種單別表達不出這些差別 —— 而「要開哪些流程」正是照這個差別決定的。
 * 所以單別細到四種，錢的方向退成單別的**屬性**（`isPayable`）。
 *
 * 舊的兩個值（`electricityBill` / `consumptionBill`）**留著**：
 * 既有資料是那樣寫的，而且它們仍然是合法的「粗分」答案 ——
 * 綁定解析時粗分與細分都會被試（見 `docTypeFamily`）。
 */
export type EnergyDocType =
  /** 躉購·**有餘電**（再生能源購售電通知單，購電電費 > 0）。台電付我們，我們要開發票。 */
  | 'renewableSurplus'
  /** 躉購·**無餘電**（購電電費 ＝ 0，只剩電表租費要付）。**我們付台電**。 */
  | 'renewableNoSurplus'
  /** 逆變器·**僅設備發電使用電費**（高壓用戶繳費通知單，沒有電器租費那一列）。我們付台電。 */
  | 'consumptionDevice'
  /** 逆變器·**含電器租費**（多了「電器租費」那一列）。我們付台電。 */
  | 'consumptionDeviceRental'
  /**
   * 舊的粗分：躉購電費單 → `bill-approval`。台電付我們。
   * **新資料不會再產生這個值**，但舊列與外部管線還在用。
   */
  | 'electricityBill'
  /**
   * 舊的粗分：用電電費單 → `utility-bill`。**我們付台電**。
   *
   * 錢的方向與 `electricityBill` **相反**，所以它走的流程**沒有 `calc` 節點** ——
   * 對一張「我們要付錢給台電」的單去算「該付地主多少分潤」是沒有意義的
   * （`RECOGNITION.md §9-1`）。這件事由**流程定義**保證，不是由 if 保證。
   */
  | 'consumptionBill'
  /** 保證金請款 → `deposit-request` */
  | 'depositRequest'
  /** 售電業（陽光伏特家／阿波羅／英業達）賣綠電給我們的購售電單——我們要開發票。
   * 不併進 `renewableSurplus`：那是台電躉購單，RentCycle 會把它當台電躉購單處理。 */
  | 'retailerSale'
  /** 判不出來 —— 這是一個**合法答案**，不是錯誤 */
  | 'unclassified';

/**
 * **辨識服務收得下的檔案型別**（`taipower-recognition/server/app.ts` 的 `validFile`）。
 *
 * 與附件的 `ALLOWED_MIME_TYPES` **不一樣**，而那個差別會咬人：
 * 附件那一份還收 `image/heic`（iPhone 拍的照片），辨識服務不收 ——
 * 送過去會拿到 415，而 415 是**永久**錯誤（重試一百次也一樣）。
 * 把它當成暫時錯誤退回重送的話，那一份會被**無限重試**，
 * 畫面上永遠停在「辨識中」：使用者看到的就是「卡死」。
 *
 * 所以要在送出去**之前**就用這張表擋下來，並且告訴使用者能做什麼。
 */
export const RECOGNIZABLE_MIME_TYPES = ['application/pdf', 'image/png', 'image/jpeg'];

export const isRecognizableMime = (mimeType: string): boolean => (
  RECOGNIZABLE_MIME_TYPES.includes(String(mimeType).toLowerCase())
);

/** 人可以在校對台上指定的單別（不含 `unclassified` 與保證金）。順序＝下拉的順序。 */
export const BILL_DOC_TYPES: readonly EnergyDocType[] = [
  'renewableSurplus', 'renewableNoSurplus', 'consumptionDevice', 'consumptionDeviceRental', 'retailerSale',
];

/**
 * 細分 → 粗分（錢的方向）。
 *
 * 綁定解析要用它做**回退**：客戶只綁了「躉購」那一條時，
 * 一張 `renewableSurplus` 的單仍然找得到那段綁定 ——
 * 否則細分單別的第一天，所有既有綁定會全部失效，而畫面只會說「尚未綁定流程」。
 */
export const DOC_TYPE_FAMILY: Partial<Record<EnergyDocType, EnergyDocType>> = {
  renewableSurplus: 'electricityBill',
  renewableNoSurplus: 'consumptionBill',
  consumptionDevice: 'consumptionBill',
  consumptionDeviceRental: 'consumptionBill',
  retailerSale: 'electricityBill',
};

/** 這種單**錢的方向**：true ＝ 我們要付出去（不該算分潤）。 */
export const DOC_TYPE_IS_PAYABLE: Partial<Record<EnergyDocType, boolean>> = {
  renewableSurplus: false,
  renewableNoSurplus: true,
  consumptionDevice: true,
  consumptionDeviceRental: true,
  electricityBill: false,
  consumptionBill: true,
  retailerSale: false,
};

/**
 * 這個單別**會產生一張電費單**嗎（相對於保證金請款那種只是觸發器的單）。
 *
 * 寫成 `d === 'electricityBill' || d === 'consumptionBill'` 的地方有好幾處，
 * 而細分成四種的那一天，**每一處都要記得加四個值** ——
 * 漏掉任何一處，那種單就會安靜地不產生電費單（收件台顯示「已抽取」，但沒有單）。
 */
export const isBillDocType = (t: string): boolean => (
  BILL_DOC_TYPES.includes(t as EnergyDocType) || t === 'electricityBill' || t === 'consumptionBill'
);

/**
 * 解析綁定時要試的單別，**由細到粗**。
 *
 * `renewableSurplus` → 先找它自己的綁定，找不到才回頭找 `electricityBill` 的。
 * 順序不可以顛倒：細分綁定的存在就代表「這座案場對這種單有特別安排」。
 */
export const docTypeLookupChain = (t: EnergyDocType): EnergyDocType[] => (
  DOC_TYPE_FAMILY[t] ? [t, DOC_TYPE_FAMILY[t]!] : [t]
);

export interface ClassifyResult {
  docType: EnergyDocType;
  /**
   * 對應的流程 key。`unclassified` 時為 `null`。
   * 不要在這裡回「預設流程」——那就是猜。
   */
  workflowKey: string | null;
  /**
   * 0–1。**僅供顯示與排序，不可拿來當門檻**。
   *
   * 這條與 geocoder 的教訓同源（`DESIGN.md §1.2.1` 約束 1）：實測 ArcGIS 回
   * score 76.4 但座標是區公所 —— **分數高不代表對**。
   * 這裡真正的訊號是 `docType` 本身：分類器要嘛認得，要嘛老實說不認得。
   */
  confidence: number;
  /** 為什麼這樣判 —— 給人看的，畫面上要能解釋「它憑什麼說這是電費單」。 */
  reason: string;
  /** 命中的特徵（除錯與日後訓練用）。不 pin shape。 */
  signals?: unknown;
}

/** 介面：真實作（模型／規則引擎／外部服務）換掉時，只換這一個函式。 */
export type DocumentClassifier = (ocrRaw: unknown) => Promise<ClassifyResult>;

// ---------------------------------------------------------------------------
// 保證金請款單的抽取器（`DESIGN.md §1.8.2`）
// ---------------------------------------------------------------------------

/**
 * 注意這個介面**刻意很窮** —— 它抽的東西**不會變成系統裡的事實**。
 *
 * 電費單帶來**新事實**（度數、金額只有台電知道）→ 產生 `energyBill`。
 * 保證金請款單**什麼都沒帶來**：期數、金額、到期日全部來自合約
 * （`energyInstallment` 建約時就產好了）。它只是「第 3 期該付了」的**觸發器**。
 *
 * 所以 `amount` **只用來交叉驗證，絕不覆蓋合約的金額** ——
 * 否則錢就有兩個來源，哪天不一樣時沒有人知道該信誰。
 * 合約是唯一真相，掃描件是觸發與佐證。
 */
export interface ExtractedDepositRequest {
  /** 抽出來的合約編號**原樣**。對不上也要留著 —— 人工指認要靠它（同電號）。 */
  contractNoRaw: string;
  /** 第幾期。 */
  seq?: number;
  /** 只用來交叉驗證，不用來覆蓋 `energyInstallment.amount`。 */
  amount?: string;
  dueDate?: string;
}

export type DepositRequestExtractor = (ocrRaw: unknown) => Promise<ExtractedDepositRequest>;

/**
 * `docType` → 流程 key 的**唯一對照表**。
 *
 * 分成兩層（分類 vs 對照）是刻意的：分類器回答「這是什麼文件」，
 * 對照表回答「這種文件走哪條流程」。後者是**營運設定**，會因客戶而異；
 * 不要把它燒進分類器裡。
 */
/**
 * **這是預設值，不是答案**。
 *
 * 「這張單要開哪些流程」的真正答案在**綁定**上（`energyWorkflowBinding`
 * 的 `(案場, 單別)`）—— 一張單可以開多條，而且不同案場可以不一樣。
 * 這張表只回答「這種單**通常**走哪一條」，用在兩個地方：
 * 收件台顯示「判定的流程」，以及還沒有任何綁定時的說明文字。
 * **送審一律以綁定為準**（`IngestService.submitItem`）。
 */
export const DOC_TYPE_WORKFLOW: Record<EnergyDocType, string | null> = {
  renewableSurplus: 'bill-approval',
  // 無餘電那張是**我們付台電**（只剩電表租費）—— 走付款流程，不是躉購核可流程。
  renewableNoSurplus: 'utility-bill',
  consumptionDevice: 'utility-bill',
  consumptionDeviceRental: 'utility-bill',
  electricityBill: 'bill-approval',
  consumptionBill: 'utility-bill',
  depositRequest: 'deposit-request',
  retailerSale: 'bill-approval',
  unclassified: null,
};

export const docTypeToWorkflow = (t: EnergyDocType): string | null => DOC_TYPE_WORKFLOW[t] ?? null;

// ---------------------------------------------------------------------------
// v1 的 mock（規則式）
// ---------------------------------------------------------------------------

/** 把 OCR 輸出攤平成一段可搜尋的文字。不假設 provider 的 JSON 形狀。 */
function flatten(ocrRaw: unknown, depth = 0): string {
  if (ocrRaw == null || depth > 6) return '';
  if (typeof ocrRaw === 'string') return ocrRaw;
  if (typeof ocrRaw === 'number' || typeof ocrRaw === 'boolean') return String(ocrRaw);
  if (Array.isArray(ocrRaw)) return ocrRaw.map((x) => flatten(x, depth + 1)).join(' ');
  if (typeof ocrRaw === 'object') {
    return Object.entries(ocrRaw as Record<string, unknown>)
      .map(([k, v]) => `${k} ${flatten(v, depth + 1)}`)
      .join(' ');
  }
  return '';
}

interface Rule {
  docType: EnergyDocType;
  /** 命中任一即算一分。 */
  patterns: RegExp[];
  label: string;
}

/**
 * 規則刻意保守：寧可判不出來，也不要判錯。
 * 真實作（例如版面模型）接上時，這整段會被換掉 —— 但**介面與「不猜」的規則不變**。
 */
const RULES: Rule[] = [
  {
    docType: 'electricityBill',
    label: '電費單',
    patterns: [
      /台灣電力|台電|電費通知|電費單/,
      /customerNumber|電號/i,
      /用電度數|kwh|度數/i,
      /計費期間|billingPeriod/i,
    ],
  },
  {
    docType: 'depositRequest',
    label: '保證金請款單',
    patterns: [
      /保證金|履約保證|請款單/,
      /合約編號|contractNo/i,
      /第\s*\d+\s*期|installment|期別/i,
      /應付日|dueDate/i,
    ],
  },
];

/** 至少要命中幾個特徵才敢說認得。是參數不是猜測；調低之前先想清楚誤判的代價。 */
export const CLASSIFY_MIN_HITS = 2;

/**
 * v1 mock 分類器。
 *
 * 三種情況都回 `unclassified`（**不 fallback 成 bill**）：
 * 1. 特徵數不足（< `CLASSIFY_MIN_HITS`）
 * 2. 兩種型別**平手** —— 分不出來就是分不出來
 * 3. OCR 輸出是空的
 */
export const mockClassifier: DocumentClassifier = async (ocrRaw: unknown) => {
  const text = flatten(ocrRaw);
  if (!text.trim()) {
    return {
      docType: 'unclassified', workflowKey: null, confidence: 0,
      reason: 'OCR 輸出是空的，無法判定',
    };
  }

  const scored = RULES.map((r) => {
    const hits = r.patterns.filter((p) => p.test(text));
    return { rule: r, hits: hits.length, matched: hits.map((p) => p.source) };
  }).sort((a, b) => b.hits - a.hits);

  const top = scored[0];
  const runnerUp = scored[1];

  if (!top || top.hits < CLASSIFY_MIN_HITS) {
    return {
      docType: 'unclassified', workflowKey: null, confidence: 0,
      reason: `可辨識的特徵不足（最多只命中 ${top?.hits ?? 0} 項，需要 ${CLASSIFY_MIN_HITS} 項）`,
      signals: scored.map((s) => ({ docType: s.rule.docType, hits: s.hits })),
    };
  }
  // 平手不猜 —— 兩種都像，代表特徵不足以區分。
  if (runnerUp && runnerUp.hits === top.hits) {
    return {
      docType: 'unclassified', workflowKey: null, confidence: 0,
      reason: `「${top.rule.label}」與「${runnerUp.rule.label}」的特徵一樣多，無法區分`,
      signals: scored.map((s) => ({ docType: s.rule.docType, hits: s.hits })),
    };
  }

  return {
    docType: top.rule.docType,
    workflowKey: docTypeToWorkflow(top.rule.docType),
    // 只是「命中幾成特徵」，不是機率，也不該被拿去當門檻。
    confidence: Number((top.hits / top.rule.patterns.length).toFixed(2)),
    reason: `命中「${top.rule.label}」的 ${top.hits} 項特徵`,
    signals: { matched: top.matched, runnerUpHits: runnerUp?.hits ?? 0 },
  };
};

/**
 * v1 mock 抽取器。抽不到合約編號就回空字串（不丟例外）——
 * 「對不上」是**例外件**不是錯誤，跟電號那條完全同構（`DESIGN.md §1.8.2`）。
 */
export const mockDepositRequestExtractor: DepositRequestExtractor = async (ocrRaw: unknown) => {
  const src = (ocrRaw ?? {}) as any;
  const text = flatten(ocrRaw);
  const pick = (...keys: string[]) => {
    for (const k of keys) {
      if (src[k] != null && String(src[k]).trim() !== '') return String(src[k]).trim();
    }
    return '';
  };

  const contractNoRaw = pick('contractNo', 'contractNoRaw', '合約編號')
    || (text.match(/[A-Z]{2}-\d{4}-\d{3,5}/) ?? [''])[0];

  const seqRaw = pick('seq', '期別', 'installmentSeq');
  const seqMatch = seqRaw.match(/\d+/) ?? text.match(/第\s*(\d+)\s*期/);
  const seq = seqMatch ? Number(seqMatch[1] ?? seqMatch[0]) : undefined;

  const amountRaw = pick('amount', '請款金額', '金額');
  // 千分位要吃掉（同 `billExtractor`）。
  const amount = amountRaw ? amountRaw.replace(/,/g, '') : undefined;

  const dueDate = pick('dueDate', '應付日')
    || (text.match(/\d{4}-\d{2}-\d{2}/) ?? [''])[0] || undefined;

  return {
    contractNoRaw,
    ...(seq != null && Number.isFinite(seq) ? { seq } : {}),
    ...(amount ? { amount } : {}),
    ...(dueDate ? { dueDate } : {}),
  };
};
