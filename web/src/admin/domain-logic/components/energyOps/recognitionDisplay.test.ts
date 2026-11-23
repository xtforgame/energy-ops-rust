import { describe, it, expect } from 'vitest';
import { emptyFieldMessage, intakeStageMessage } from './recognitionDisplay';

const raw = (status: string, rawValue = '') => ({status: 'succeeded', record: {fields: {accountNumber: {status, value: null, rawValue}}}});
describe('recognition display evidence boundaries', () => {
  it('does not claim absent source when extraction has no evidence', () => {
    expect(emptyFieldMessage(null, 'customerNumberRaw')).toContain('不代表原單沒有');
    expect(emptyFieldMessage(raw('missing'), 'customerNumberRaw')).toContain('尚未擷取到');
  });
  it('separates confirmed blank from not applicable', () => {
    expect(emptyFieldMessage(raw('source_blank'), 'customerNumberRaw')).toContain('原單未提供');
    expect(emptyFieldMessage(raw('not_applicable'), 'customerNumberRaw')).toContain('不適用');
  });
  it('retains ambiguous candidates instead of reporting absence', () => {
    expect(emptyFieldMessage(raw('ambiguous', '12521400968 | 其他候選'), 'customerNumberRaw')).toContain('12521400968');
  });
  it('distinguishes actual failed and unclassified supplier batches', () => {
    expect(intakeStageMessage({status:'failed'})).toContain('文件解析未完成');
    expect(intakeStageMessage({status:'unclassified', recognition:{extractionMethod:'pdf_text'}})).toContain('已取得辨識結果');
    expect(intakeStageMessage({status:'unclassified'})).not.toContain('已取得辨識結果');
  });
  it('treats unmatched station separately from OCR and preserves pending states', () => {
    expect(intakeStageMessage({status:'extracted',billId:4,stationId:null})).toContain('尚未對應案場');
    expect(intakeStageMessage({status:'recognizing'})).toBeNull();
    expect(intakeStageMessage({status:'extracted',billId:4,stationId:168})).toBeNull();
  });
});
