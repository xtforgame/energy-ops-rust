import { asCanonicalRecord, canonicalKeyFor } from 'common/plugins/EnergyOpsCommonPlugin/taipowerExtractor';

/** A missing value alone never proves that the source document omitted it. */
export function emptyFieldMessage(raw: unknown, field: string, templateId?: string | null): string {
  const record = asCanonicalRecord(raw);
  const key = canonicalKeyFor(field, templateId) ?? field;
  const source = record?.fields?.[key];
  if (source?.status === 'source_blank') return '原單未提供（來源標記為空白）';
  if (source?.status === 'not_applicable') return '不適用（來源規則標記此欄不存在）';
  if (source?.status === 'ambiguous') {
    return source.rawValue
      ? `已讀到候選值，待確認對應：${source.rawValue}`
      : '已讀到候選值，待確認對應';
  }
  return '尚未擷取到（不代表原單沒有）';
}

export function intakeStageMessage(item: { status: string; billId?: number | null; stationId?: number | null; recognition?: { extractionMethod?: string | null } | null }): string | null {
  if (item.status === 'failed') return '文件解析未完成，請查看下方錯誤；不代表原單沒有資料。';
  if (item.status === 'unclassified') return item.recognition?.extractionMethod
    ? '已取得辨識結果，尚未完成單據分類／欄位對應；待補規則或確認單別。'
    : '尚未完成單據分類；目前不能判定原單是否缺少欄位。';
  if (item.status === 'unmatched' || (item.billId != null && item.stationId == null)) return '尚未對應案場；請確認電號或其他案場對照資料，不代表單據無法讀取。';
  return null;
}
