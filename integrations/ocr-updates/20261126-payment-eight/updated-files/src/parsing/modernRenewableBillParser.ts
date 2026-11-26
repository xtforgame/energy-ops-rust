import {
  BILL_FIELDS,
  createEmptyBillRecord,
  setExtractionMethod,
  type BillFieldKey,
  type BillRecord,
} from '../domain/billSchema';
import { applyTemplateApplicability } from '../domain/billTemplates';
import type { BillFieldValue, ExtractionMethod, FieldEvidence } from '../domain/billTypes';
import type { NormalizedTextToken, PdfPageText } from '../extraction/pdfExtractor';
import { normalizeIdentifier, normalizeMoney, normalizePercent, normalizeRocDate } from './normalizers';

interface PositionedRow {
  y: number;
  tokens: NormalizedTextToken[];
}

type WritableField = BillFieldValue;

const FIELD_KIND = new Map<BillFieldKey, (typeof BILL_FIELDS)[number]['kind']>(
  BILL_FIELDS.map((field) => [field.key, field.kind]),
);

const IDENTIFIER_FIELDS = new Set<BillFieldKey>([
  'accountNumber', 'contractNumber', 'companyTaxId', 'bankAccount', 'serviceTaxId',
  'rentalInvoiceNumber', 'rentalInvoiceTaxId', 'carrierType', 'carrierPeriod',
  'carrierSerial', 'carrierCheckCode', 'carrierIdentity', 'documentId',
]);

function normalizeText(value: string): string {
  return value.normalize('NFKC').replace(/[‐‑‒–—―−﹘﹣－]/gu, '-').trim();
}

function numericToken(value: string): string | null {
  return normalizeText(value).match(/-?(?:\d{1,3}(?:,\d{3})+|\d+)(?:\.\d+)?/u)?.[0] ?? null;
}

function fieldValue(key: BillFieldKey, rawValue: string): string | number | null {
  const kind = FIELD_KIND.get(key);
  if (kind === 'number') {
    if (key === 'lineLossRate') return normalizePercent(normalizeText(rawValue).replace(/\s+/gu, ''));
    const token = numericToken(rawValue);
    return token === null ? null : normalizeMoney(token);
  }
  if (kind === 'date') return normalizeRocDate(rawValue);
  const normalized = IDENTIFIER_FIELDS.has(key) ? normalizeIdentifier(rawValue) : normalizeText(rawValue);
  return normalized.length === 0 ? null : normalized;
}

function assign(
  record: BillRecord,
  key: BillFieldKey,
  rawValue: string | undefined,
  method: ExtractionMethod,
  pageNumber = 1,
  rawText = rawValue ?? '',
): void {
  if (rawValue === undefined || rawValue.trim().length === 0) return;
  const value = fieldValue(key, rawValue);
  const target = record.fields[key] as WritableField;
  target.value = value;
  target.rawValue = rawValue;
  target.status = value === null ? 'ambiguous' : method === 'ocr' ? 'ocr_extracted' : 'extracted';
  target.evidence = {
    pageNumber,
    region: method === 'ocr' ? 'ocr' : 'pdf_text',
    rawText,
  } satisfies FieldEvidence;
}

function assignLiteral(record: BillRecord, key: BillFieldKey, value: string | number): void {
  const target = record.fields[key] as WritableField;
  target.value = value;
  target.rawValue = String(value);
  target.status = 'extracted';
  target.evidence = null;
}

function markNotApplicable(record: BillRecord, keys: readonly BillFieldKey[]): void {
  for (const key of keys) {
    const field = record.fields[key];
    if (field.status === 'missing') field.status = 'not_applicable';
  }
}

function rowsFromTokens(tokens: readonly NormalizedTextToken[]): PositionedRow[] {
  const sorted = tokens.slice().sort((left, right) => left.y - right.y || left.x - right.x);
  const rows: PositionedRow[] = [];
  for (const token of sorted) {
    const current = rows.at(-1);
    if (!current || Math.abs(current.y - token.y) > 0.006) {
      rows.push({ y: token.y, tokens: [token] });
      continue;
    }
    current.tokens.push(token);
    current.y = current.tokens.reduce((sum, item) => sum + item.y, 0) / current.tokens.length;
  }
  for (const row of rows) row.tokens.sort((left, right) => left.x - right.x);
  return rows;
}

function rowText(row: PositionedRow): string {
  return row.tokens.map((token) => normalizeText(token.text)).join(' ');
}

function rowWithLabel(rows: readonly PositionedRow[], label: string, minX = 0, maxX = 1): PositionedRow | undefined {
  const expected = normalizeText(label).replace(/\s+/gu, '');
  return rows.find((row) => row.tokens.some((token) => {
    const text = normalizeText(token.text).replace(/\s+/gu, '');
    return token.x >= minX && token.x <= maxX && text.includes(expected);
  }));
}

function rightmostValue(row: PositionedRow | undefined, label: string, minX = 0, maxX = 1): string | undefined {
  if (!row) return undefined;
  const expected = normalizeText(label).replace(/\s+/gu, '');
  const labelToken = row.tokens.find((token) => {
    const text = normalizeText(token.text).replace(/\s+/gu, '');
    return token.x >= minX && token.x <= maxX && text.includes(expected);
  });
  if (!labelToken) return undefined;
  return row.tokens.filter((token) => token.x > labelToken.x + 0.015).at(-1)?.text;
}

function labeledValue(rows: readonly PositionedRow[], label: string, minX = 0, maxX = 1): string | undefined {
  return rightmostValue(rowWithLabel(rows, label, minX, maxX), label, minX, maxX);
}

function tokensMatching(page: PdfPageText, pattern: RegExp): NormalizedTextToken[] {
  return page.tokens.filter((token) => pattern.test(normalizeText(token.text)));
}

function rocDateFromParts(parts: readonly string[]): string | undefined {
  if (parts.length !== 3) return undefined;
  return `${parts[0]}年${parts[1]}月${parts[2]}日`;
}

function invoiceValue(rows: readonly PositionedRow[], label: string): string | undefined {
  return labeledValue(rows, label, 0.6, 0.75);
}

export function parseModernRenewableBill(
  sourceFileName: string,
  pages: readonly PdfPageText[],
  documentId: string,
  method: ExtractionMethod,
): BillRecord {
  const page = pages[0];
  if (!page) throw new Error('新版再生能源通知單沒有第一頁文字。');
  const rows = rowsFromTokens(page.tokens);
  const record = createEmptyBillRecord(sourceFileName);
  setExtractionMethod(record, method);
  applyTemplateApplicability(record, 'renewable_purchase');

  assignLiteral(record, 'templateName', '台電再生能源躉購電費通知單（新版）');
  assignLiteral(record, 'documentType', '再生能源電能躉購電費通知單');
  assignLiteral(record, 'issuerName', '台灣電力公司');
  assignLiteral(record, 'issuerWebsite', 'www.taipower.com.tw');
  if (documentId.trim().length > 0) assignLiteral(record, 'documentId', documentId);
  assignLiteral(record, 'sourcePageCount', pages.length);
  assign(record, 'rawSourceText', pages.map((candidate) => candidate.text).join('\n\n'), method);

  const headerMonth = rows.find((row) => row.y < 0.075 && row.tokens.filter((token) => /^\d{2,3}$/u.test(normalizeText(token.text))).length >= 2);
  const headerMonthNumbers = headerMonth?.tokens.map((token) => normalizeText(token.text)).filter((text) => /^\d{2,3}$/u.test(text));
  if (headerMonth && headerMonthNumbers && headerMonthNumbers.length >= 2) {
    assign(record, 'noticeYearMonth', `${headerMonthNumbers[0]}年${headerMonthNumbers[1]}月`, method, 1, rowText(headerMonth));
  }

  const periodRow = rows.find((row) => row.y < 0.13 && row.tokens.filter((token) => /^\d{1,3}$/u.test(normalizeText(token.text))).length >= 6);
  const periodParts = periodRow?.tokens.map((token) => normalizeText(token.text)).filter((text) => /^\d{1,3}$/u.test(text)).slice(-6);
  if (periodParts?.length === 6) {
    assign(record, 'billingStartDate', rocDateFromParts(periodParts.slice(0, 3)), method, 1, rowText(periodRow!));
    assign(record, 'billingEndDate', rocDateFromParts(periodParts.slice(3, 6)), method, 1, rowText(periodRow!));
  }

  assign(record, 'accountNumber', tokensMatching(page, /^\d{2}-\d{2}-\d{4}-\d{2}-\d$/u)[0]?.text, method);
  assign(record, 'contractNumber', tokensMatching(page, /^\d{2}-(?:PV-\d{3}|\d{3})-\d{4}$/u)[0]?.text, method);
  assign(record, 'companyTaxId', labeledValue(rows, '統一編號', 0, 0.3), method);

  const recipientCompany = page.tokens.find((token) => token.x < 0.45 && /(?:股份有限公司|有限公司)$/u.test(normalizeText(token.text)));
  const recipientAddress = page.tokens.find((token) => token.x < 0.45 && /(?:縣|市).*(?:路|街|段|地號)/u.test(normalizeText(token.text)));
  const recipientPostal = page.tokens.find((token) => token.x < 0.2 && token.y > 0.08 && token.y < 0.13 && /^\d{3,5}$/u.test(normalizeText(token.text)));
  assign(record, 'recipientCompanyName', recipientCompany?.text, method);
  assign(record, 'recipientAddress', recipientAddress?.text, method);
  assign(record, 'recipientPostalCode', recipientPostal?.text, method);

  const bankRow = rows.find((row) => row.tokens.some((token) => /銀行.*分行/u.test(normalizeText(token.text))));
  if (bankRow) {
    assign(record, 'bankBranch', bankRow.tokens.find((token) => /銀行.*分行/u.test(normalizeText(token.text)))?.text, method, 1, rowText(bankRow));
    assign(record, 'bankAccount', bankRow.tokens.find((token) => /^\d{10,16}$/u.test(normalizeText(token.text)))?.text, method, 1, rowText(bankRow));
  }

  const installed = labeledValue(rows, '併聯總裝置容量', 0, 0.3);
  const purchaseCapacity = labeledValue(rows, '併聯總購售電容量', 0, 0.3);
  assign(record, 'installedCapacity', installed, method);
  assign(record, 'installedCapacityUnit', installed?.split(/\s+/u).at(-1), method);
  assign(record, 'purchaseSaleCapacity', purchaseCapacity, method);
  assign(record, 'purchaseSaleCapacityUnit', purchaseCapacity?.split(/\s+/u).at(-1), method);

  const chargeRateRow = rowWithLabel(rows, '機組', 0.45, 0.65);
  if (chargeRateRow) {
    const values = chargeRateRow.tokens.filter((token) => token.x >= 0.48);
    assign(record, 'generatorNumber', values[0]?.text.replace(/^機組/u, ''), method, 1, rowText(chargeRateRow));
    assign(record, 'purchaseRate', values[1]?.text, method, 1, rowText(chargeRateRow));
    assign(record, 'ratePurchaseKwh', values[2]?.text, method, 1, rowText(chargeRateRow));
    assign(record, 'rateCalculatedAmount', values[3]?.text, method, 1, rowText(chargeRateRow));
  }

  const electricityTotalRow = rows.find((row) => Math.abs(row.y - 0.231) <= 0.01);
  const electricityTotal = electricityTotalRow?.tokens.find((token) => token.x > 0.18 && token.x < 0.35 && /^-?[\d,]+$/u.test(normalizeText(token.text)));
  assign(record, 'electricityFeeTotal', electricityTotal?.text, method, 1, electricityTotalRow ? rowText(electricityTotalRow) : '');
  assign(record, 'currentReceivableTotal', electricityTotal?.text, method, 1, electricityTotalRow ? rowText(electricityTotalRow) : '');
  assign(record, 'purchaseEnergyFee', labeledValue(rows, '購電電費', 0.45, 0.65), method);
  assign(record, 'businessTax', labeledValue(rows, '營業稅', 0.45, 0.65), method);
  assign(record, 'meterRentalSubtotal', labeledValue(rows, '本期電表租費', 0.45, 0.7), method);
  assign(record, 'usageEnergyFee', labeledValue(rows, '使用電能費用', 0.45, 0.7), method);
  assign(record, 'payableTotal', labeledValue(rows, '應付總金額', 0.45, 0.7), method);

  const readingDates = page.tokens
    .filter((token) => token.x < 0.45 && /^\d{2,3}年\d{1,2}月\d{1,2}日$/u.test(normalizeText(token.text)))
    .sort((left, right) => left.y - right.y);
  assign(record, 'previousReadingDate', readingDates[0]?.text, method);
  assign(record, 'currentReadingDate', readingDates[1]?.text, method);
  assign(record, 'nextReadingDate', readingDates[2]?.text, method);

  const readingMappings = [
    ['上期指數', 'purchaseRPreviousIndex', 'saleRCh1PreviousIndex'],
    ['拆表指數', 'purchaseRSplitIndex', 'saleRCh1SplitIndex'],
    ['本期指數', 'purchaseRCurrentIndex', 'saleRCh1CurrentIndex'],
    ['指數差', 'purchaseRIndexDifference', 'saleRCh1IndexDifference'],
    ['倍數', 'purchaseRMultiplier', 'saleRCh1Multiplier'],
    ['度數', 'purchaseRBase', 'saleRCh1Base'],
  ] as const;
  for (const [label, purchaseKey, saleKey] of readingMappings) {
    const row = rowWithLabel(rows, label, 0, 0.15);
    if (!row) continue;
    const values = row.tokens.filter((token) => token.x > 0.15 && token.x < 0.45);
    assign(record, purchaseKey, values[0]?.text, method, 1, rowText(row));
    assign(record, saleKey, values[1]?.text, method, 1, rowText(row));
  }
  assign(record, 'lineLossRate', labeledValue(rows, '線路損失率', 0, 0.18), method);
  const purchaseFormulaRow = rowWithLabel(rows, '購電度數', 0, 0.15);
  const purchaseFormula = purchaseFormulaRow?.tokens.filter((token) => token.x > 0.12).map((token) => token.text).join(' ');
  assign(record, 'purchaseKwhFormula', purchaseFormula, method, 1, purchaseFormulaRow ? rowText(purchaseFormulaRow) : '');
  const purchaseKwh = normalizeText(purchaseFormula ?? '').match(/=\s*([\d,]+)度/u)?.[1]
    ?? chargeRateRow?.tokens.find((token) => token.x > 0.68 && token.x < 0.82)?.text;
  assign(record, 'purchaseKwh', purchaseKwh, method, 1, purchaseFormula ?? '');

  const serviceDepartment = page.tokens.find((token) => /營業處核算/u.test(normalizeText(token.text)));
  const servicePhone = page.tokens.find((token) => /^\(\d{2}\)\d+$/u.test(normalizeText(token.text)));
  const serviceTaxId = page.tokens.find((token) => token.x > 0.45 && token.y > 0.6 && token.y < 0.72 && /^\d{8}$/u.test(normalizeText(token.text)));
  const serviceAddress = page.tokens.find((token) => token.x < 0.62 && token.y > 0.64 && token.y < 0.69 && /(?:縣|市).*(?:路|街)/u.test(normalizeText(token.text)));
  const installationAddress = page.tokens.find((token) => token.x < 0.62 && token.y >= 0.68 && token.y < 0.72 && /(?:縣|市).*(?:路|街|段|地號)/u.test(normalizeText(token.text)));
  assign(record, 'serviceDepartment', serviceDepartment?.text, method);
  assign(record, 'servicePhone', servicePhone?.text, method);
  assign(record, 'serviceTaxId', serviceTaxId?.text, method);
  assign(record, 'serviceAddress', serviceAddress?.text, method);
  assign(record, 'installationAddress', installationAddress?.text, method);

  assign(record, 'rentalInvoiceTaxId', labeledValue(rows, '統一編號', 0.6, 0.75), method);
  assign(record, 'rentalNetAmount', invoiceValue(rows, '電表租費'), method);
  assign(record, 'invoiceUsageEnergyFee', invoiceValue(rows, '使用電能費用'), method);
  const invoiceTax = invoiceValue(rows, '營業稅');
  assign(record, 'invoiceBusinessTax', invoiceTax, method);
  assign(record, 'rentalBusinessTax', invoiceTax, method);
  const invoiceTotal = invoiceValue(rows, '金額');
  assign(record, 'rentalTotal', invoiceTotal, method);

  const invoicePeriod = page.tokens.find((token) => /^\d{3}年\d{2}-\d{2}月$/u.test(normalizeText(token.text)));
  const invoiceNumber = page.tokens.find((token) => /^[A-Z]{2}\d{8}$/u.test(normalizeText(token.text)));
  const invoiceAmount = page.tokens.find((token) => token.x < 0.3 && token.y > 0.82 && token.y < 0.89 && /^\d[\d,]*元$/u.test(normalizeText(token.text)));
  assign(record, 'rentalInvoicePeriod', invoicePeriod?.text, method);
  assign(record, 'rentalInvoiceNumber', invoiceNumber?.text, method);
  assign(record, 'rentalInvoiceAmount', invoiceAmount?.text, method);

  const carrierType = page.tokens.find((token) => token.x > 0.25 && token.x < 0.45 && /^[A-Z0-9]{6}$/u.test(normalizeText(token.text)));
  const carrierComposite = page.tokens.find((token) => /^\d{5}[A-Z0-9]{10}$/u.test(normalizeText(token.text)));
  const carrierCheck = page.tokens.find((token) => /^\d{15}$/u.test(normalizeText(token.text)));
  assign(record, 'carrierType', carrierType?.text, method);
  if (carrierComposite) {
    const normalized = normalizeIdentifier(carrierComposite.text);
    assign(record, 'carrierPeriod', normalized.slice(0, 5), method, 1, carrierComposite.text);
    assign(record, 'carrierSerial', normalized.slice(5), method, 1, carrierComposite.text);
    if (carrierType) {
      assign(record, 'carrierIdentity', `${normalizeIdentifier(carrierType.text)}-${normalized.slice(0, 5)}-${normalized.slice(5)}`, method, 1, `${carrierType.text} ${carrierComposite.text}`);
    }
  }
  assign(record, 'carrierCheckCode', carrierCheck?.text, method);
  assign(record, 'carrierBarcode1', carrierType?.text, method);
  assign(record, 'carrierBarcode2', carrierComposite?.text, method);
  assign(record, 'carrierBarcode3', carrierCheck?.text, method);

  const paymentDirections = page.tokens.filter(t => /匯款至|匯款到|臨櫃繳/u.test(normalizeText(t.text))).map(t => t.text).join(' ');
  assign(record, 'paymentNote', paymentDirections || undefined, method);
  markNotApplicable(record, ['billNumber', 'siteName']);
  if (!invoiceNumber) markNotApplicable(record, ['rentalInvoiceNumber']);
  if (!carrierType || !carrierComposite) {
    markNotApplicable(record, [
      'carrierType', 'carrierPeriod', 'carrierSerial', 'carrierIdentity', 'carrierCheckCode',
      'carrierBarcode1', 'carrierBarcode2', 'carrierBarcode3',
    ]);
  }

  return record;
}
