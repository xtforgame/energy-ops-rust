/**
 * **真辨識接上之後的兩個實作**（`RECOGNITION.md §6.2`）。
 *
 * 介面一行都沒改：
 * - `DocumentClassifier`：`mockClassifier`（關鍵詞規則）→ 這裡的 `taipowerClassifier`
 * - `BillExtractor`：`mockBillExtractor` → 這裡的 `taipowerBillExtractor`
 *
 * 兩者吃的都是**服務回來的 canonical document**（存進 `energyBill.ocrRaw`）。
 * 那個形狀由 `taipower-recognition/api/canonical-schema.json` 定義，
 * 我們只讀真的會用到的那幾個 key（`taipowerCanonical.ts` 的理由）。
 */

import { mockBillExtractor, type BillExtractor, type ExtractedBill } from './billExtractor';
import type { ClassifyResult, DocumentClassifier, EnergyDocType } from './documentIngest';
import { DOC_TYPE_WORKFLOW, docTypeToWorkflow, mockClassifier } from './documentIngest';
import {
  blockingIssues,
  dateOf,
  numberOf,
  textOf,
  type CanonicalDocument,
  type CanonicalRecord,
  type CanonicalTemplateId,
} from './taipowerCanonical';

/**
 * 版型 → 我們的單別。**一對一，不猜**。
 *
 * 認不得的版型（`unknown` / `null`）一律 `unclassified` ——
 * 那是一個合法答案，人可以在送件詳情頁手動指定流程。
 * 絕不 fallback 成電費單：猜錯會讓一張「我們要付錢」的單走進躉購流程，
 * 算出一筆**看起來完全正常**的假分潤。
 */
export const TEMPLATE_DOC_TYPE: Record<string, EnergyDocType> = {
  renewable_purchase: 'electricityBill',
  legacy_renewable_purchase: 'electricityBill',
  high_voltage: 'consumptionBill',
};

/** 單別的中文名 —— 畫面與稽核都不可以出現 raw docType。 */
export const DOC_TYPE_LABEL: Record<string, string> = {
  renewableSurplus: '躉購電費單（有餘電·需開發票）',
  renewableNoSurplus: '躉購電費單（無餘電·需付款）',
  consumptionDevice: '逆變器電費單（僅設備發電使用電費）',
  consumptionDeviceRental: '逆變器電費單（含電器租費）',
  electricityBill: '電費單（躉購）',
  consumptionBill: '用電電費單',
  depositRequest: '保證金請款單',
  unclassified: '未判定',
};

const TEMPLATE_LABEL: Record<string, string> = {
  renewable_purchase: '再生能源購售電通知單',
  legacy_renewable_purchase: '再生能源電能躉購電費通知單（舊版）',
  high_voltage: '繳費通知單（高壓電力用戶）',
};

/** 從我們存下來的 `ocrRaw` 取回 canonical document（存的時候整包存，形狀不 pin）。 */
export function asCanonicalDocument(ocrRaw: unknown): CanonicalDocument | null {
  const raw = ocrRaw as any;
  if (!raw || typeof raw !== 'object') return null;
  if (raw.status === 'succeeded' && raw.record && typeof raw.record === 'object') {
    return raw as CanonicalDocument;
  }
  if (raw.status === 'failed' && raw.error) return raw as CanonicalDocument;
  return null;
}

export function asCanonicalRecord(ocrRaw: unknown): CanonicalRecord | null {
  const doc = asCanonicalDocument(ocrRaw);
  return doc && doc.status === 'succeeded' ? doc.record : null;
}

const templateIdOf = (record: CanonicalRecord): CanonicalTemplateId => {
  const declared = record.templateId;
  if (declared && declared !== 'unknown') return declared;
  // `templateId` 也會以欄位的形式出現（parser 會 assign 它）——兩邊取其一。
  const asField = textOf(record, 'templateId');
  return (asField && asField !== 'unknown' ? asField : null) as CanonicalTemplateId;
};

/**
 * 分類器：**版型就是答案**，不再做關鍵詞計分。
 *
 * `confidence` 一律 1 或 0：服務要嘛認得這個版型（它有整組欄位錨點與勾稽），
 * 要嘛老實說不認得。給一個 0.73 只會讓人以為那是機率。
 */
/**
 * **版型不等於單別**（2026-09-09）。
 *
 * 四份範本只有兩種版型，但**每一份的後續作業都不一樣**，
 * 而「要開哪些流程」是照後續作業決定的。所以單別要比版型細一階，
 * 分辨的依據是**紙上真的印了什麼**：
 *
 * | 範本 | 版型 | 判別 | 單別 |
 * |---|---|---|---|
 * | 電費單(有餘電需開發票) | `renewable_purchase` | 購電電費 127,848 > 0 | `renewableSurplus` |
 * | 電費單(無餘電需付款) | `renewable_purchase` | 購電電費 0、應付總金額 −132 | `renewableNoSurplus` |
 * | 逆變器(僅設備發電使用電費) | `high_voltage` | **沒有**「電器租費」那一列 | `consumptionDevice` |
 * | 逆變器(含電表租費…) | `high_voltage` | 電器租費 1,549 | `consumptionDeviceRental` |
 *
 * 判別欄位都是**已經在抽的欄位**，不需要為了分類多抽東西。
 *
 * ### 判錯了會怎樣，以及為什麼那還好
 *
 * 「電器租費」那一列如果沒抽到，一張含租費的單會被判成 `consumptionDevice`。
 * 這種錯**必然存在**（辨識不是保證），所以真正的保險不是把規則寫得更複雜，
 * 而是**讓人改得動** —— 單別在校對台上是一個可以改的欄位，
 * 而且改完之後「會開哪些流程」當場就重算給人看（`billBindingStatus`）。
 */
export function docTypeFromRecord(record: CanonicalRecord): EnergyDocType | null {
  const templateId = templateIdOf(record);
  if (!templateId) return null;

  if (templateId === 'high_voltage') {
    // 「有沒有電器租費那一列」——抽不到就是沒有那一列（另一張範本連欄位都不存在）。
    const rental = numberOf(record, 'meterRentalFee');
    return rental !== undefined && rental !== 0 ? 'consumptionDeviceRental' : 'consumptionDevice';
  }

  // 躉購：有沒有賣出去的電。
  const fee = numberOf(record, 'purchaseEnergyFee');
  const kwh = numberOf(record, 'purchaseKwh');
  const surplus = (fee !== undefined && fee > 0) || (kwh !== undefined && kwh > 0);
  if (surplus) return 'renewableSurplus';
  // 兩個都抽不到時**不要猜「無餘電」**：那會把一張躉購單送進付款流程。
  if (fee === undefined && kwh === undefined) return TEMPLATE_DOC_TYPE[templateId] ?? null;
  return 'renewableNoSurplus';
}

export const taipowerClassifier: DocumentClassifier = async (ocrRaw: unknown): Promise<ClassifyResult> => {
  const doc = asCanonicalDocument(ocrRaw);
  if (!doc) {
    return {
      docType: 'unclassified', workflowKey: null, confidence: 0,
      reason: '沒有辨識結果可判讀',
    };
  }
  if (doc.status === 'failed') {
    return {
      docType: 'unclassified', workflowKey: null, confidence: 0,
      reason: `辨識失敗：${doc.error.message}`,
      signals: { errorCode: doc.error.code },
    };
  }

  const templateId = templateIdOf(doc.record);
  const docType = docTypeFromRecord(doc.record);
  if (!templateId || !docType) {
    return {
      docType: 'unclassified', workflowKey: null, confidence: 0,
      reason: '辨識服務認不出這是哪一種單',
      signals: { templateId },
    };
  }

  const blocking = blockingIssues(doc.issues);
  return {
    docType,
    // 這裡回的是**預設**流程；真正要開哪些由綁定決定（`DOC_TYPE_WORKFLOW` 的說明）。
    workflowKey: docTypeToWorkflow(docType),
    confidence: 1,
    reason: `辨識為「${DOC_TYPE_LABEL[docType] ?? TEMPLATE_LABEL[templateId] ?? templateId}」`,
    signals: {
      templateId,
      extractionMethod: doc.record.extractionMethod,
      // 判別單別時看的兩個數字 —— 判錯時人要看得出它憑什麼那樣判。
      purchaseEnergyFee: numberOf(doc.record, 'purchaseEnergyFee') ?? null,
      meterRentalFee: numberOf(doc.record, 'meterRentalFee') ?? null,
      blockingIssues: blocking.map((issue) => issue.code),
    },
  };
};

/**
 * 抽取器：canonical → `ExtractedBill`。
 *
 * **`totalAmount` 依單別取不同欄位**，因為兩種單的「主要金額」不是同一件事：
 *
 * | | 躉購（台電付我們） | 高壓用電（我們付台電） |
 * |---|---|---|
 * | `totalAmount` | `purchaseEnergyFee` 購電電費（**未稅售電收入**） | `payableTotal` 應繳總金額 |
 * | `generationKwh` | `purchaseKwh` 購電度數（已扣線損） | `consumptionKwh` 用電度數 |
 *
 * ⏳ 躉購那一格是**預設值不是需求**：分潤的基數到底是購電電費（未稅）、
 * 電費合計（含稅）還是應付總金額（扣表租後），要客戶的實際算式
 * （`REQUIREMENTS.md §九-2`／`RECOGNITION.md §9-2`）。
 * 三個數字都會進 `ocrExtracted`，所以答案回來時**不必重跑辨識**。
 */
export const taipowerBillExtractor: BillExtractor = (ocrJson: unknown): ExtractedBill[] => {
  const record = asCanonicalRecord(ocrJson);
  if (!record) return [];

  const templateId = templateIdOf(record) ?? undefined;
  const isConsumption = templateId === 'high_voltage';

  const out: ExtractedBill = {
    customerNumber: textOf(record, 'accountNumber'),
    billingPeriodStart: dateOf(record, 'billingStartDate'),
    billingPeriodEnd: dateOf(record, 'billingEndDate'),
    totalAmount: isConsumption
      ? numberOf(record, 'payableTotal')
      : numberOf(record, 'purchaseEnergyFee'),
    generationKwh: isConsumption
      ? numberOf(record, 'consumptionKwh')
      : numberOf(record, 'purchaseKwh'),
    // 兩種單都沒有「台電開單日」這個欄位；不要拿通知單年月去造一個假日期。
    issuedAt: undefined,
    // 躉購新版單**沒有單號**（服務明確回 not_applicable）——那時候就沒有，
    // 不自己編一個（`FRONTEND.md §10` 已經記過這個決定）。
    documentNo: textOf(record, 'billNumber'),
    templateId,
    dueDate: dateOf(record, 'dueDate'),
    contractNumberRaw: textOf(record, 'contractNumber'),
  };

  const cleaned = Object.fromEntries(
    Object.entries(out).filter(([, v]) => v !== undefined),
  ) as ExtractedBill;

  return Object.keys(cleaned).length > 0 ? [cleaned] : [];
};

/**
 * 校對台要並排的欄位，**依版型而異**（`RECOGNITION.md §7.4`）。
 *
 * 沿用服務自己的 `reviewPolicy.CORE_REVIEW_FIELDS` 概念，
 * 不另立一套：兩套「哪些欄位重要」遲早會不一致。
 */
export const REVIEW_FIELDS_BY_TEMPLATE: Record<string, readonly string[]> = {
  renewable_purchase: [
    'accountNumber', 'contractNumber', 'companyTaxId',
    'billingStartDate', 'billingEndDate',
    'purchaseKwh', 'purchaseEnergyFee', 'businessTax',
    'electricityFeeTotal', 'meterRentalSubtotal', 'payableTotal',
  ],
  legacy_renewable_purchase: [
    'billNumber', 'accountNumber', 'siteName', 'companyTaxId',
    'billingStartDate', 'billingEndDate', 'contractNumber',
    'purchaseKwh', 'purchaseEnergyFee', 'businessTax',
    'electricityFeeTotal', 'meterRentalSubtotal', 'payableTotal',
  ],
  high_voltage: [
    'billNumber', 'accountNumber', 'companyTaxId',
    'billingStartDate', 'billingEndDate', 'dueDate',
    'consumptionKwh', 'businessTax', 'taxBeforeAmount', 'payableTotal',
  ],
};

export const reviewFieldsFor = (templateId?: string | null): readonly string[] => (
  templateId ? REVIEW_FIELDS_BY_TEMPLATE[templateId] ?? [] : []
);

// ---------------------------------------------------------------------------
// 校對台要並排哪幾個**我們的**欄位（`RECOGNITION.md §7.1`）
// ---------------------------------------------------------------------------
//
// 上面那張表講的是 canonical 的欄位（單據語彙），這一段講的是 `energyBill`
// 的欄位（業務語彙）—— 校對台的「辨識值 vs 最終值」並排的就是後者。
//
// **兩種單要看的日期不一樣**，這不是版面偏好：
// 高壓用電單有「繳費期限」（會計依它排付款），躉購單沒有；
// 而**兩種單都沒有台電開單日**（`canonicalKeyFor('issuedAt')` 回 null），
// `issuedAt` 留在躉購那一組是因為它仍是人可以手填的欄位。

/** 高壓用電單（我們付台電）看繳費期限。 */
const CONSUMPTION_REVIEW_FIELDS = [
  'customerNumberRaw',
  'billingPeriodStart', 'billingPeriodEnd', 'totalAmount', 'generationKwh', 'dueDate',
] as const;

/** 躉購單（台電付我們）與所有認不出版型的單。 */
/**
 * **電號排第一個**（`customerNumberRaw`）。
 *
 * 它決定這張單配到哪一座案場，而案場決定走哪一條流程 ——
 * 讀錯的後果不是「有個欄位錯了」，是**整張單進到別人的流程**。
 * 其他欄位錯了頂多金額不對；這一個錯了，後面每一關都在處理錯的東西。
 * 所以它必須跟其他欄位一樣：辨識值與最終值並排、看得到、改得動。
 */
const DEFAULT_REVIEW_FIELDS = [
  'customerNumberRaw',
  'billingPeriodStart', 'billingPeriodEnd', 'totalAmount', 'generationKwh', 'issuedAt',
] as const;

/**
 * 後端**寫入**與**變更標記**掃的是這一組（兩組的聯集）。
 *
 * 不要讓 `correct()` 與 `changedFields()` 只掃版型那一組：
 * 版型判錯（或事後改版型）時，一個已經被人改過的欄位會**靜默地**
 * 既存不進去、也不顯示「已修正」—— 而畫面上不會有任何訊息。
 * 畫面只渲染版型那一組，多出來的欄位不會露出來。
 */
export const ALL_BILL_REVIEW_FIELDS = [
  'customerNumberRaw',
  'billingPeriodStart', 'billingPeriodEnd', 'totalAmount', 'generationKwh', 'issuedAt', 'dueDate',
] as const;

/**
 * 欄位名 → `ocrExtracted` 裡的鍵。
 *
 * 兩邊不同名是歷史的結果：`energyBill` 的欄位叫 `customerNumberRaw`
 * （「單子上原樣印的電號」），而抽取器產出的 `ExtractedBill` 叫 `customerNumber`。
 *
 * 沒有這張對照表的話，`changedFields` 會拿 `extracted.customerNumberRaw`
 * （永遠是 undefined）去比 `bill.customerNumberRaw`（有值）——
 * 於是**每一張單**都被標成「電號已人工修正」，那個標記就完全失去意義。
 * 前端的並排也一樣：辨識值那一欄會永遠空白。
 */
export const EXTRACTED_KEY_ALIAS: Record<string, string> = {
  customerNumberRaw: 'customerNumber',
};

/** 從 `ocrExtracted` 取某個欄位的辨識值（處理上面那張對照表）。 */
export function extractedValueOf(extracted: unknown, field: string): unknown {
  const e = (extracted ?? {}) as Record<string, unknown>;
  const key = EXTRACTED_KEY_ALIAS[field] ?? field;
  return e[key];
}

/**
 * 這張單是**哪一種單**（決定走哪一條流程）。
 *
 * 來源是 **`templateId`**（辨識判出來的版型，`high_voltage` ＝ 用電單）。
 *
 * `energyBill` **沒有** `docType` 欄位。`BillIngestService.ingest()` 一度在寫
 * `docType: ...`，而那個欄位不存在 —— Sequelize 靜靜丟掉，沒有任何錯誤。
 * 所以「單別記在單上」從來沒有成立過；真正記著這件事的是 `templateId`。
 * （要讓 docType 成為第一級事實得先加欄位＋`init:hasura`，
 * 但它完全能從版型推得，多一欄只是多一個會不一致的地方。）
 *
 * 推不出來就回 `electricityBill`（躉購）—— 那是舊資料唯一存在的形態。
 *
 * **不要寫死 `electricityBill`**。寫死的話高壓用電單會被送進躉購的審批流程，
 * 而那條流程有「計算應付分潤」那一關 —— 一張我們付錢給台電的單，
 * 會反過來算出要付給地主多少（`RECOGNITION.md §9-1`）。
 * 而且畫面會說它「尚未綁定流程」，即使綁定明明存在。
 */
export function billDocType(bill: {
  get?: (k: string) => unknown; docType?: unknown; templateId?: unknown; ocrRaw?: unknown;
}): EnergyDocType {
  const read = (k: string) => (typeof bill?.get === 'function' ? bill.get(k) : (bill as any)?.[k]);
  // ① 人改過（或收件時就存好）的值最大。
  const stored = String(read('docType') ?? '');
  if (stored && stored !== 'unclassified' && stored in DOC_TYPE_WORKFLOW) return stored as EnergyDocType;
  // ② 沒存值的舊單：從辨識結果現算（細分）。
  const recognized = recognizedDocType(bill);
  if (recognized) return recognized;
  // ③ 連版型都沒有：躉購是舊資料唯一存在的形態。
  return 'electricityBill';
}

/**
 * **辨識判出來的**單別（不看人工修正）。
 *
 * `billDocType()` 回的是「現在算哪一種」（人改過就是人改的那個）；
 * 這一支回的是「辨識當初說是哪一種」。校對台要把兩者並排 ——
 * 單別跟金額、電號一樣是辨識結果，所以它也要看得到「原本判成什麼」，
 * 否則「已人工修正」這件事就無從顯示，也無從復原。
 *
 * 認不出版型時回 `null`（**不是** `electricityBill`）：
 * 「辨識沒判出來」與「辨識判成躉購」是兩件不同的事，
 * 混成同一個值會讓畫面對著一張根本沒判出版型的單說「辨識值：躉購單」。
 */
export function recognizedDocType(bill: {
  get?: (k: string) => unknown; templateId?: unknown; ocrRaw?: unknown;
}): EnergyDocType | null {
  const read = (k: string) => (typeof bill?.get === 'function' ? bill.get(k) : (bill as any)?.[k]);
  // 細分要看辨識結果本身（購電電費／電器租費），版型只分得出粗的兩類。
  const record = asCanonicalRecord(read('ocrRaw'));
  if (record) {
    const fine = docTypeFromRecord(record);
    if (fine) return fine;
  }
  const template = String(read('templateId') ?? '');
  return TEMPLATE_DOC_TYPE[template] ?? null;
}

/** 這張單在校對台上要並排哪幾個欄位。 */
export const billReviewFieldsFor = (templateId?: string | null): readonly string[] => (
  templateId === 'high_voltage' ? CONSUMPTION_REVIEW_FIELDS : DEFAULT_REVIEW_FIELDS
);

// ---------------------------------------------------------------------------
// 兩條進來的路
// ---------------------------------------------------------------------------

/**
 * 收件台走辨識服務，**外部管線**則可以直接灌一份「整理好的 JSON」
 * （`DESIGN.md §1.8` 的原始設計，`POST /ingest-batches` 帶 `files[].ocrRaw`）。
 *
 * 兩者的形狀完全不同，所以由**內容**決定用哪一個判讀器：
 * 是 canonical document 就用版型判，不是就退回規則式的關鍵詞判讀。
 *
 * 這不是「猜」：規則式那一份本來就保守（要命中兩項特徵、平手不判），
 * 而且它只會在**根本不是 canonical** 的輸入上被用到。
 * 真正要避免的是相反的事 —— 把一份辨識失敗的 canonical 拿去關鍵詞硬湊出一個型別。
 */
export const energyDocumentClassifier: DocumentClassifier = async (ocrRaw: unknown) => {
  if (asCanonicalDocument(ocrRaw)) return taipowerClassifier(ocrRaw);
  return mockClassifier(ocrRaw);
};

/** 同上：canonical 走 taipower 映射，其餘走原本的欄位名相容抽取。 */
export const energyBillExtractor: BillExtractor = (ocrJson: unknown): ExtractedBill[] => {
  if (asCanonicalDocument(ocrJson)) return taipowerBillExtractor(ocrJson);
  return mockBillExtractor(ocrJson);
};

// ---------------------------------------------------------------------------
// 欄位在原檔上的位置（`RECOGNITION.md §9-5`）
// ---------------------------------------------------------------------------

/**
 * 我們的欄位名 → canonical 的欄位名。
 *
 * 兩邊的名字不一樣是歷史的結果（我們的是業務語彙、它的是單據語彙），
 * 對照表放這裡是為了**只有一份** —— 抽取、校對台高亮、日後的差異提示都讀它。
 */
export function canonicalKeyFor(field: string, templateId?: string | null): string | null {
  const isConsumption = templateId === 'high_voltage';
  switch (field) {
    // 兩個名字都要認：`customerNumber` 是抽取器產出的名字，
    // `customerNumberRaw` 是 `energyBill` 的欄位名（校對台用的是後者）。
    // 只認一個的話，點「電號」那一列不會有任何反應 ——
    // 而電號正是最需要拿去對紙的那一格（讀錯就配到別的案場）。
    case 'customerNumber':
    case 'customerNumberRaw':
      return 'accountNumber';
    case 'billingPeriodStart': return 'billingStartDate';
    case 'billingPeriodEnd': return 'billingEndDate';
    case 'totalAmount': return isConsumption ? 'payableTotal' : 'purchaseEnergyFee';
    case 'generationKwh': return isConsumption ? 'consumptionKwh' : 'purchaseKwh';
    case 'documentNo': return 'billNumber';
    case 'dueDate': return 'dueDate';
    case 'contractNumberRaw': return 'contractNumber';
    // 兩種單都沒有「台電開單日」——不要對到一個看起來很像的欄位。
    case 'issuedAt': return null;
    default: return null;
  }
}

export interface SourceFieldBox {
  field: string;
  page: number;
  x: number;
  y: number;
  width: number;
  height: number;
}

/**
 * 從辨識結果取出「欄位在紙上的位置」。
 *
 * **沒有座標就不回** —— 服務只在能唯一定位到那個 token 時才給 box，
 * 而指錯位置比沒有位置更糟（覆核的整件事就是拿螢幕對紙）。
 * 回空陣列時畫面就不畫框，這是正確行為，不是缺陷。
 *
 * `fields` 指定要哪幾個（校對台只高亮它在編輯的那幾格）。
 */
export function sourceFieldBoxes(ocrRaw: unknown, fields: readonly string[]): SourceFieldBox[] {
  const record = asCanonicalRecord(ocrRaw);
  if (!record) return [];
  const templateId = templateIdOf(record) ?? undefined;
  const out: SourceFieldBox[] = [];
  for (const field of fields) {
    const key = canonicalKeyFor(field, templateId);
    if (!key) continue;
    const box = record.fields[key]?.evidence?.box;
    if (!box) continue;
    out.push({
      field,
      page: Math.max(0, (record.fields[key]?.evidence?.pageNumber ?? 1) - 1),
      x: box.x,
      y: box.y,
      width: box.width,
      height: box.height,
    });
  }
  return out;
}

// ---------------------------------------------------------------------------
// 「這份辨識結果可不可信」的一句話（`RECOGNITION.md §7.2`）
// ---------------------------------------------------------------------------

export interface RecognitionIssueView {
  code: string;
  message: string;
  fields: string[];
  blocking: boolean;
}

export interface RecognitionSummary {
  templateId: string | null;
  templateLabel: string | null;
  /** 這份結果是文字層拆出來的還是 OCR 認出來的（`§9-4` 的融合結果）。 */
  extractionMethod: 'pdf_text' | 'ocr' | null;
  /** 服務自己列出來的問題。**blocking ＝ 核心欄位有問題，不該就這樣送審。** */
  issues: RecognitionIssueView[];
  blockingCount: number;
  /**
   * 每個欄位的值是怎麼被決定的，按 `resolution` 計數。
   *
   * 這是 L2（三來源全跑）唯一能被人看見的地方 —— 少了它，
   * 「跑了三條路」與「只跑文字層」在畫面上完全一樣，
   * 使用者無從判斷該不該多看一眼。
   */
  resolutions: Record<string, number>;
  /** 辨識失敗時的錯誤碼（例如 `password_required`）。 */
  errorCode: string | null;
  errorMessage: string | null;
}

/**
 * 從 `ocrRaw` 整理出畫面要的那幾件事。
 *
 * 不看 `record.reviewStatus` —— 它在交付版永遠是 `needs_review`
 * （`taipowerCanonical` 有寫）。判準是 `issues` 裡有沒有 `blocking`。
 */
export function recognitionSummary(ocrRaw: unknown): RecognitionSummary | null {
  const doc = asCanonicalDocument(ocrRaw);
  if (!doc) return null;
  if (doc.status === 'failed') {
    return {
      templateId: null,
      templateLabel: null,
      extractionMethod: null,
      issues: [],
      blockingCount: 0,
      resolutions: {},
      errorCode: doc.error.code,
      errorMessage: doc.error.message,
    };
  }
  const { record } = doc;
  const templateId = templateIdOf(record);
  const resolutions: Record<string, number> = {};
  Object.values(record.fields ?? {}).forEach((f) => {
    const r = f?.evidence?.resolution;
    // 沒有 evidence 的欄位（`not_applicable` / `missing`）不計 —— 它們不是
    // 「某個來源看到的值」，混進來會讓比例失真。
    if (!r) return;
    resolutions[r] = (resolutions[r] ?? 0) + 1;
  });
  const issues = (doc.issues ?? []).map((i) => ({
    code: i.code, message: i.message, fields: i.fields ?? [], blocking: !!i.blocking,
  }));
  return {
    templateId: templateId ?? null,
    templateLabel: templateId ? TEMPLATE_LABEL[templateId] ?? null : null,
    extractionMethod: record.extractionMethod ?? null,
    issues,
    blockingCount: blockingIssues(doc.issues ?? []).length,
    resolutions,
    errorCode: null,
    errorMessage: null,
  };
}
