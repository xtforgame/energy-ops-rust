import { describe, expect, it, vi } from 'vitest';

import type { ValidationIssue } from '../src/domain/billTypes';
import type { OcrEngineDocument } from '../src/extraction/ocrTypes';
import type { PdfExtraction, PdfPageText } from '../src/extraction/pdfExtractor';
import {
  BillProcessingError,
  InvalidPdfFileError,
  OcrProcessingError,
  PdfProcessingError,
  processBill,
  type ProcessBillDependencies,
} from '../src/extraction/processBill';

const EMPTY_REGIONS = {
  header: '', basic: '', charges: '', readings: '', service: '', rentalInvoice: '', footer: '',
} as const;

function pdfFile(name = 'bill.pdf'): File {
  return new File(['%PDF-test'], name, { type: 'application/pdf' });
}

function imageFile(name = 'bill.jpg'): File {
  return new File([new Uint8Array([0xff, 0xd8, 0xff])], name, { type: 'image/jpeg' });
}

function page(pageNumber: number, text: string): PdfPageText {
  return {
    pageNumber,
    text,
    lines: text.split('\n'),
    tokens: [],
    regions: { ...EMPTY_REGIONS },
  };
}

function pdfExtraction(pages: readonly PdfPageText[], sufficient: boolean): PdfExtraction {
  const canvases = pages.map(() => document.createElement('canvas'));
  return {
    regions: { ...EMPTY_REGIONS },
    sufficient,
    pageCanvas: canvases[0] ?? document.createElement('canvas'),
    pageCanvases: sufficient ? undefined : canvases,
    preview: new Blob(['preview'], { type: 'image/png' }),
    pages: [...pages],
    pageCount: pages.length,
    documentId: 'sha256:test-document',
  };
}

function paddleDocument(pages: readonly PdfPageText[]): OcrEngineDocument {
  return {
    engine: 'paddleocr',
    pages: [...pages],
    observations: [],
    metrics: { initializeMs: 1, recognizeMs: 2, totalMs: 3, recognizedCount: 0 },
  };
}

describe('processBill schema-driven extraction', () => {
  it('uses native PDF text and falls back to the generic parser when no template routes', async () => {
    const pages = [page(1, '電號 01-23-4567-89-0\n應付總金額 5,035 元')];
    const selected = pdfExtraction(pages, true);
    const extractPaddleDocument = vi.fn();

    const result = await processBill(pdfFile('native.pdf'), {
      extractTextRegions: vi.fn().mockResolvedValue(selected),
      extractPaddleDocument,
    });

    expect(extractPaddleDocument).not.toHaveBeenCalled();
    expect(result.record.templateId).toBe('unknown');
    expect(result.record.extractionMethod).toBe('pdf_text');
    expect(result.record.fields.accountNumber.value).toBe('01-23-4567-89-0');
    expect(result.record.fields.payableTotal.value).toBe(5035);
    expect(result.preview).toBe(selected.preview);
  });

  it('uses Paddle as the only OCR engine for every rendered scan page', async () => {
    const selected = pdfExtraction([page(1, ''), page(2, '')], false);
    const recognized = [
      page(1, '電號 01-23-4567-89-0'),
      page(2, '應付總金額 5,035 元'),
    ];
    const extractPaddleDocument = vi.fn().mockResolvedValue(paddleDocument(recognized));
    const onOcrProgress = vi.fn();

    const result = await processBill(
      pdfFile('scan.pdf'),
      { extractTextRegions: vi.fn().mockResolvedValue(selected), extractPaddleDocument },
      { onOcrProgress },
    );

    expect(extractPaddleDocument).toHaveBeenCalledOnce();
    expect(extractPaddleDocument).toHaveBeenCalledWith(selected.pageCanvases, onOcrProgress);
    expect(result.record.extractionMethod).toBe('ocr');
    expect(result.record.fields.accountNumber.value).toBe('01-23-4567-89-0');
    expect(result.record.fields.payableTotal.value).toBe(5035);
    expect(result.record.fields.sourcePageCount.value).toBe(2);
  });

  it('streams rental bundles without claiming attached pages were recognized', async () => {
    const selected=pdfExtraction([page(1,'')],false);selected.pageCount=202;
    let released=false;
    selected.ocrPageStream=async function*(){try{yield document.createElement('canvas');throw Error('must not consume attachments as one bill');}finally{released=true;}};
    const text='測試參考年租金計算表\n台電購電電費 轉供電費';
    const paddle=vi.fn().mockResolvedValue(paddleDocument([page(1,text)]));
    const result=await processBill(pdfFile(),{extractTextRegions:vi.fn().mockResolvedValue(selected),extractPaddleDocument:paddle});
    expect(paddle).toHaveBeenCalledOnce();expect(released).toBe(true);
    expect(result.record.sourcePageCount).toBe(202);expect(result.record.sourceScope).toBe('rentalWorksheetFirstPage');
    expect(result.record.sourcePages?.[0]?.text).toBe(text);
  });

  it('uses Paddle for image input and keeps its values without a second-engine decision', async () => {
    const pageCanvas = document.createElement('canvas');
    const preview = new Blob(['jpg'], { type: 'image/jpeg' });
    const recognized = [page(1, '單據號碼 R2020081800001\n營業稅 4,116 元')];
    const extractPaddleDocument = vi.fn().mockResolvedValue(paddleDocument(recognized));

    const result = await processBill(imageFile(), {
      extractImagePage: vi.fn().mockResolvedValue({ pageCanvas, preview }),
      extractPaddleDocument,
    });

    expect(extractPaddleDocument).toHaveBeenCalledWith([pageCanvas], undefined);
    expect(result.record.fields.billNumber.value).toBe('R2020081800001');
    expect(result.record.fields.businessTax.value).toBe(4116);
    expect(result.record.fields.businessTax.evidence?.source).toBe('paddleocr');
  });

  it('returns the same field values for equivalent PDF text and Paddle text', async () => {
    const text = '電號 01-23-4567-89-0\n營業稅 240 元\n應付總金額 5,035 元';
    const nativePages = [page(1, text)];
    const native = await processBill(pdfFile('native.pdf'), {
      extractTextRegions: vi.fn().mockResolvedValue(pdfExtraction(nativePages, true)),
    });
    const scanExtraction = pdfExtraction([page(1, '')], false);
    const scan = await processBill(pdfFile('scan.pdf'), {
      extractTextRegions: vi.fn().mockResolvedValue(scanExtraction),
      extractPaddleDocument: vi.fn().mockResolvedValue(paddleDocument([page(1, text)])),
    });

    expect(scan.record.fields.accountNumber.value).toBe(native.record.fields.accountNumber.value);
    expect(scan.record.fields.businessTax.value).toBe(native.record.fields.businessTax.value);
    expect(scan.record.fields.payableTotal.value).toBe(native.record.fields.payableTotal.value);
  });

  it('returns a row even when Paddle finds no business fields', async () => {
    const selected = pdfExtraction([page(1, '')], false);
    const result = await processBill(pdfFile('blank-scan.pdf'), {
      extractTextRegions: vi.fn().mockResolvedValue(selected),
      extractPaddleDocument: vi.fn().mockResolvedValue(paddleDocument([page(1, '無可配對欄位')])),
    });

    expect(result.record.fields.accountNumber.status).toBe('missing');
    expect(result.record.fields.rawSourceText.value).toContain('無可配對欄位');
    expect(result.preview).toBe(selected.preview);
  });

  it('retains visible but unreadable Paddle candidates as ambiguous evidence', async () => {
    const selected = pdfExtraction([page(1, '')], false);
    const result = await processBill(pdfFile('uncertain.pdf'), {
      extractTextRegions: vi.fn().mockResolvedValue(selected),
      extractPaddleDocument: vi.fn().mockResolvedValue(paddleDocument([
        page(1, '營業稅 4,1I6 元'),
      ])),
    });

    expect(result.record.fields.businessTax).toMatchObject({
      value: null,
      rawValue: '4,1I6 元',
      status: 'ambiguous',
      evidence: { source: 'paddleocr' },
    });
  });

  it('returns and freezes validator issues without changing extracted values', async () => {
    const selected = pdfExtraction([page(1, '電號 01-23-4567-89-0')], true);
    const issue: ValidationIssue = {
      code: 'review_account', message: 'review', fields: ['accountNumber'], blocking: false,
    };
    const result = await processBill(pdfFile(), {
      extractTextRegions: vi.fn().mockResolvedValue(selected),
      validateBill: vi.fn().mockReturnValue([issue]),
    });

    expect(result.record.fields.accountNumber.value).toBe('01-23-4567-89-0');
    expect(result.issues).toEqual([issue]);
    expect(Object.isFrozen(result.issues)).toBe(true);
  });

  it('wraps Paddle failures in a privacy-safe retryable error retaining the preview', async () => {
    const selected = pdfExtraction([page(1, '')], false);
    const privateCause = new Error('private OCR output');
    const error = await processBill(pdfFile(), {
      extractTextRegions: vi.fn().mockResolvedValue(selected),
      extractPaddleDocument: vi.fn().mockRejectedValue(privateCause),
    }).catch((reason: unknown) => reason);

    expect(error).toBeInstanceOf(OcrProcessingError);
    expect((error as OcrProcessingError).preview).toBe(selected.preview);
    expect((error as OcrProcessingError).cause).not.toBe(privateCause);
    expect(JSON.stringify(error)).not.toContain('private OCR output');
  });

  it('rejects invalid, empty, oversized, and spoofed PDF files before extraction', async () => {
    const dependencies: Partial<ProcessBillDependencies> = {
      extractTextRegions: vi.fn(),
      extractPaddleDocument: vi.fn(),
    };

    await expect(processBill(
      new File(['%PDF-valid-signature'], 'bill.txt', { type: 'text/plain' }),
      dependencies,
    )).rejects.toBeInstanceOf(InvalidPdfFileError);
    await expect(processBill(
      new File([], 'empty.pdf', { type: 'application/pdf' }),
      dependencies,
    )).rejects.toBeInstanceOf(PdfProcessingError);
    await expect(processBill(
      new File(['plain text'], 'spoofed.pdf', { type: 'application/pdf' }),
      dependencies,
    )).rejects.toBeInstanceOf(InvalidPdfFileError);
    await expect(processBill(new File(
      [new Uint8Array(20 * 1024 * 1024 + 1)],
      'oversized.pdf',
      { type: 'application/pdf' },
    ), dependencies)).rejects.toBeInstanceOf(PdfProcessingError);
    expect(dependencies.extractTextRegions).not.toHaveBeenCalled();
    expect(dependencies.extractPaddleDocument).not.toHaveBeenCalled();
  });

  it('wraps damaged PDF extraction failures while retaining the cause', async () => {
    const cause = new Error('Invalid PDF structure');
    const error = await processBill(pdfFile(), {
      extractTextRegions: vi.fn().mockRejectedValue(cause),
    }).catch((reason: unknown) => reason);

    expect(error).toBeInstanceOf(PdfProcessingError);
    expect((error as PdfProcessingError).cause).toBe(cause);
    expect((error as PdfProcessingError).preview).toBeNull();
  });

  it('wraps parser and validator failures while retaining the preview and cause', async () => {
    for (const stage of ['parser', 'validator'] as const) {
      const selected = pdfExtraction([page(1, '電號 01-23-4567-89-0')], true);
      const cause = new Error(`${stage} failed`);
      const overrides: Partial<ProcessBillDependencies> = {
        extractTextRegions: vi.fn().mockResolvedValue(selected),
        ...(stage === 'parser'
          ? { parseRoutedBill: vi.fn(() => { throw cause; }) }
          : { validateBill: vi.fn(() => { throw cause; }) }),
      };

      const error = await processBill(pdfFile(), overrides).catch((reason: unknown) => reason);
      expect(error, stage).toBeInstanceOf(BillProcessingError);
      expect((error as BillProcessingError).preview, stage).toBe(selected.preview);
      expect((error as BillProcessingError).cause, stage).toBe(cause);
    }
  });
});
