import { describe, expect, it } from 'vitest';

import type { NormalizedTextToken, PdfPageText } from '../src/extraction/pdfExtractor';
import { groupTokensIntoLines, groupTokensIntoRegions } from '../src/extraction/pdfExtractor';
import { parseModernRenewableBill } from '../src/parsing/modernRenewableBillParser';

type TokenSpec = readonly [text: string, x: number, y: number];

function modernPage(rows: readonly TokenSpec[]): PdfPageText {
  const tokens: NormalizedTextToken[] = rows.map(([text, x, y]) => ({ text, x, y, width: text.length * 0.012 }));
  const lines = groupTokensIntoLines(tokens);
  return {
    pageNumber: 1,
    text: lines.join('\n'),
    lines,
    tokens,
    regions: groupTokensIntoRegions(tokens),
  };
}

const BASE_ROWS: readonly TokenSpec[] = [
  ['115', 0.365, 0.055], ['07', 0.457, 0.055],
  ['115', 0.627, 0.088], ['06', 0.673, 0.088], ['01', 0.709, 0.088],
  ['115', 0.764, 0.088], ['06', 0.809, 0.088], ['30', 0.846, 0.088],
  ['123', 0.099, 0.107],
  ['台北市測試路１號', 0.099, 0.123],
  ['測試能源股份有限公司', 0.099, 0.140],
  ['01-23-4567-89-0', 0.130, 0.210],
  ['01-PV-234-5678', 0.421, 0.210],
  ['-243 元', 0.739, 0.210],
  ['0', 0.266, 0.231],
  ['測試銀行測試分行', 0.066, 0.247], ['000123456789', 0.268, 0.247],
  ['統一編號', 0.060, 0.305], ['01234567', 0.220, 0.305],
  ['併聯總裝置容量', 0.060, 0.323], ['454.0050 瓩', 0.220, 0.323],
  ['併聯總購售電容量', 0.060, 0.337], ['454.0050 瓩', 0.220, 0.337],
  ['機組1', 0.489, 0.337], ['4.8111', 0.566, 0.337], ['0', 0.752, 0.337], ['0元', 0.877, 0.337],
  ['購電電費', 0.489, 0.366], ['0元', 0.877, 0.366],
  ['營業稅', 0.489, 0.380], ['0元', 0.877, 0.380],
  ['115年06月01日', 0.176, 0.392], ['本期電表租費', 0.489, 0.392], ['-163元', 0.850, 0.392],
  ['115年07月01日', 0.176, 0.408], ['使用電能費用', 0.489, 0.408], ['-80元', 0.859, 0.408],
  ['115年08月01日', 0.176, 0.422], ['應付總金額', 0.489, 0.422], ['-243元', 0.850, 0.422],
  ['度數', 0.056, 0.462], ['0', 0.220, 0.462], ['1', 0.393, 0.462],
  ['線路損失率', 0.056, 0.499], ['0.0066 %', 0.206, 0.499],
  ['購電度數', 0.056, 0.516], ['0x(1-0.0066%)= 0度', 0.142, 0.516],
  ['發票資訊', 0.446, 0.746],
  ['統一編號', 0.648, 0.766], ['01234567', 0.831, 0.766],
  ['電表租費', 0.648, 0.797], ['155元', 0.858, 0.797],
  ['使用電能費用', 0.648, 0.828], ['76元', 0.867, 0.828],
  ['營業稅', 0.648, 0.859], ['12元', 0.867, 0.859],
  ['金額', 0.648, 0.889], ['243元', 0.858, 0.889],
];

describe('modern renewable purchase bill parser', () => {
  it('extracts the payment variant including distinct bill and invoice tax contexts', () => {
    const record = parseModernRenewableBill(
      'modern.pdf',
      [modernPage(BASE_ROWS)],
      'sha256:abc',
      'pdf_text',
    );

    expect(record.templateId).toBe('renewable_purchase');
    expect(record.fields.accountNumber.value).toBe('01-23-4567-89-0');
    expect(record.fields.contractNumber.value).toBe('01-PV-234-5678');
    expect(record.fields.companyTaxId.value).toBe('01234567');
    expect(record.fields.billingStartDate.value).toBe('2026-06-01');
    expect(record.fields.billingEndDate.value).toBe('2026-06-30');
    expect(record.fields.purchaseKwh.value).toBe(0);
    expect(record.fields.purchaseEnergyFee.value).toBe(0);
    expect(record.fields.businessTax.value).toBe(0);
    expect(record.fields.meterRentalSubtotal.value).toBe(-163);
    expect(record.fields.usageEnergyFee.value).toBe(-80);
    expect(record.fields.payableTotal.value).toBe(-243);
    expect(record.fields.rentalNetAmount.value).toBe(155);
    expect(record.fields.invoiceUsageEnergyFee.value).toBe(76);
    expect(record.fields.invoiceBusinessTax.value).toBe(12);
    expect(record.fields.rentalTotal.value).toBe(243);
    expect(record.fields.billNumber.status).toBe('not_applicable');
    expect(record.fields.carrierIdentity.status).toBe('not_applicable');
    expect(record.fields.documentId.value).toBe('sha256:abc');
  });

  it('extracts the invoice carrier identity and purchase values when present', () => {
    const carrierRows: readonly TokenSpec[] = [
      ...BASE_ROWS.filter(([text, x, y]) => !(
        ['-243 元', '0', '購電度數', '0x(1-0.0066%)= 0度'].includes(text)
        || (x >= 0.45 && y >= 0.33 && y <= 0.43)
        || (x >= 0.6 && y >= 0.79)
      )),
      ['133,744 元', 0.724, 0.210],
      ['134,240', 0.237, 0.231],
      ['機組1', 0.489, 0.337], ['6.0672', 0.566, 0.337], ['21072', 0.715, 0.337], ['127,848元', 0.822, 0.337],
      ['購電電費', 0.489, 0.366], ['127,848元', 0.822, 0.366],
      ['營業稅', 0.489, 0.380], ['6,392元', 0.840, 0.380],
      ['本期電表租費', 0.489, 0.392], ['-496元', 0.850, 0.392],
      ['應付總金額', 0.489, 0.408], ['133,744元', 0.822, 0.408],
      ['購電度數', 0.056, 0.603], ['21,080x(1-0.0397%)-0= 21,072度', 0.142, 0.603],
      ['115年07-08月', 0.099, 0.788],
      ['ED0003', 0.324, 0.797], ['載具類別(6位)', 0.389, 0.797], ['電表租費', 0.648, 0.797], ['472元', 0.858, 0.797],
      ['DJ15394346', 0.114, 0.830],
      ['11508BBR1491646', 0.324, 0.850], ['年期別(5位)', 0.498, 0.850],
      ['496元', 0.153, 0.858], ['營業稅', 0.648, 0.858], ['24元', 0.867, 0.858],
      ['001000601600795', 0.324, 0.902], ['檢核碼(15位)', 0.498, 0.902],
    ];
    const record = parseModernRenewableBill('carrier.pdf', [modernPage(carrierRows)], 'sha256:def', 'pdf_text');

    expect(record.fields.purchaseKwh.value).toBe(21072);
    expect(record.fields.purchaseEnergyFee.value).toBe(127848);
    expect(record.fields.businessTax.value).toBe(6392);
    expect(record.fields.electricityFeeTotal.value).toBe(134240);
    expect(record.fields.payableTotal.value).toBe(133744);
    expect(record.fields.rentalInvoiceNumber.value).toBe('DJ15394346');
    expect(record.fields.carrierType.value).toBe('ED0003');
    expect(record.fields.carrierPeriod.value).toBe('11508');
    expect(record.fields.carrierSerial.value).toBe('BBR1491646');
    expect(record.fields.carrierIdentity.value).toBe('ED0003-11508-BBR1491646');
    expect(record.fields.carrierCheckCode.value).toBe('001000601600795');
  });

  it('extracts a renewable contract number without the PV segment', () => {
    const rows = BASE_ROWS.map(([text, x, y]) => (
      text === '01-PV-234-5678' ? ['11-112-0037', x, y] as const : [text, x, y] as const
    ));
    const record = parseModernRenewableBill('non-pv.pdf', [modernPage(rows)], 'sha256:nonpv', 'pdf_text');

    expect(record.fields.contractNumber.value).toBe('11-112-0037');
  });

  it('uses OCR evidence for OCR-derived values and raw source text', () => {
    const record = parseModernRenewableBill('modern-ocr.pdf', [modernPage(BASE_ROWS)], 'sha256:ocr', 'ocr');
    const withoutDocumentId = parseModernRenewableBill('modern-no-id.pdf', [modernPage(BASE_ROWS)], '', 'pdf_text');

    expect(record.fields.businessTax.evidence?.region).toBe('ocr');
    expect(record.fields.rawSourceText).toMatchObject({ status: 'ocr_extracted', evidence: { region: 'ocr' } });
    expect(withoutDocumentId.fields.documentId).toMatchObject({ value: null, rawValue: '', status: 'missing', evidence: null });
  });

});

it('retains explicit payment directions instead of relying on absent barcode detection',()=>{
 const r=parseModernRenewableBill('payment.pdf',[modernPage([...BASE_ROWS,['備註：表租132元請臨櫃繳納或匯款至本處',0.48,0.45]])],'hash','pdf_text');
 expect(r.fields.paymentNote.value).toContain('匯款至本處');
});
