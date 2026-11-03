import * as React from 'react';
import {
  Box, Table, TableHead, TableBody, TableRow, TableCell, TextField, Typography, Chip, Stack,
} from '@mui/material';
import { useTranslate } from 'react-admin';
import { billReviewFieldsFor, extractedValueOf } from 'common/plugins/EnergyOpsCommonPlugin/taipowerExtractor';
import { jvTokens } from './shared';
import { colorForField, fieldFill } from './fieldColors';

/**
 * **「辨識值 vs 最終值」—— 全站唯一一份。**
 *
 * ### 為什麼要共用（不只是版面重複）
 *
 * 這一格編碼了三條**規則**，不是三段樣式：
 *
 * | 規則 | 出處 |
 * |---|---|
 * | 辨識值**永遠原樣顯示**，不被最終值覆蓋 | `DESIGN.md §1.4` 三層值 |
 * | 被人工改過的欄位**要標出來** | `FRONTEND.md §2-P3` |
 * | 可編輯性**完全由 `canEditSubject` 決定**，不自己推導 | `LIFECYCLE.md §6.8` |
 *
 * 各寫一份的話，「**OCR 準不準**」這個問題的答案會依你從哪一頁看而不同 ——
 * 而那正是當初把 `ocrExtracted` 與最終值分開存的整個目的。
 *
 * ### 它**不**負責的事
 *
 * · 「哪些欄位算被改過」的判斷 —— 那是後端 `BillIngestService.changedFields`，
 *   前端一律**消費** `bill.changedFields`，不自己算第二份。
 * · 兩個頁面本身的差異（送件詳情沒有 execution／流程圖／應付／血緣）——
 *   那些本來就不同，不該為了共用這一格而硬合併整頁。
 *
 * ### 用它的地方
 *
 * | 頁面 | 差別 |
 * |---|---|
 * | `BillDetailBody`（單據詳情） | 點列會高亮左欄原檔；金額那一列可能多一個「改了會多一關」的標記 |
 * | `IngestBatchDetailPage`（送件的就地校對區） | 兩者都沒有 |
 *
 * `data-testid` 做成參數是刻意的：兩邊既有的 crawler 各認一組，
 * **為了統一命名去改驗收腳本，是拿守衛換整齊**。
 */

/**
 * 預設要並排的五個欄位。
 *
 * **實際顯示哪幾個由版型決定**（`billReviewFieldsFor`）—— 高壓用電單看
 * 「繳費期限」、躉購單看「台電開單日」。這裡留一組預設是給
 * 「還沒辨識／認不出版型」的單用的。
 *
 * 後端寫入與「已修正」標記掃的是**兩組的聯集**（`ALL_BILL_REVIEW_FIELDS`），
 * 比這裡多 —— 多出來的欄位不會被渲染，但改過就一定存得進去、也一定標得出來。
 */
export const REVIEW_FIELDS = billReviewFieldsFor(null);
export type ReviewField = string;

/**
 * 欄位標題的 i18n key。
 *
 * `generationKwh` 這一格在兩種單上是**不同的東西**：躉購單是「發電度數」
 * （我們賣出去的），高壓用電單是「用電度數」（我們買進來的）。
 * 同一個欄位名配同一個標題，會讓一張 800 度的**用電**單看起來像發了 800 度。
 */
export const billFieldLabelKey = (field: string, templateId?: string | null): string => {
  const isConsumption = templateId === 'high_voltage';
  if (field === 'generationKwh' && isConsumption) {
    return 'energy.bills.field.consumptionKwh';
  }
  /**
   * **金額跟度數是同一件事，而這裡原本只做了度數。**
   *
   * `canonicalKeyFor('totalAmount')` 依版型取**不同的 canonical 欄位**
   * （躉購→`purchaseEnergyFee` 購電電費未稅；高壓→`payableTotal` 應繳總金額），
   * 但標籤兩邊都印「總金額」。於是躉購那張單上：
   * 畫面「總金額 127,848」，紙上「應付總金額 133,744」——
   * 看的人會以為辨識讀錯了（使用者 2026-09-15 回報）。
   *
   * 標籤跟著取值走，畫面就不會說謊。取哪一個當分潤基數是另一件事（§9-2 未決）。
   */
  if (field === 'totalAmount') {
    return isConsumption
      ? 'energy.bills.field.payableTotal'
      : 'energy.bills.field.purchaseEnergyFee';
  }
  return `energy.bills.field.${field}`;
};

export interface FieldDiffGridProps {
  /** 要並排哪幾個欄位（依版型）。不給就用預設那一組。 */
  fields?: readonly string[];
  /**
   * 這些欄位**在原檔上找得到位置**（有 `evidence.box`）。
 *
   * 用來畫「這一列的顏色 ＝ 紙上那個框的顏色」；找不到位置的欄位畫成空心，
   * 那是**正確狀態**不是缺陷（服務只在能唯一定位到 token 時才給座標），
   * 但使用者要看得出差別 —— 否則他會一直點一個永遠不會亮的欄位。
   */
  boxedFields?: readonly string[];
  /** 這張單的版型 —— 只用來決定欄位標題（`generationKwh` 兩種單意思不同）。 */
  templateId?: string | null;
  /** 後端回的辨識值（`ocrExtracted`）。 */
  ocrExtracted?: Record<string, any> | null;
  /** 後端算好的「被改過的欄位」。**前端不自己算。** */
  changedFields?: string[] | null;
  /** 最終值的草稿。 */
  draft: Partial<Record<ReviewField, string>>;
  onChange: (f: ReviewField, v: string) => void;
  /** 直接餵 `bill.canEditSubject` —— 不要在這裡再推導一次「送審了沒」。 */
  editable: boolean;

  /** 每一列的 testid（兩邊的 crawler 各認一組）。 */
  rowTestId: string;
  /** 最終值輸入框的 testid。 */
  inputTestId: (f: ReviewField) => string;
  /** 辨識值那一格的 testid（送件頁沒有就不給）。 */
  ocrTestId?: string;

  /** 沒有辨識值時顯示什麼（校對台印「未辨識出」，送件頁印破折號）。 */
  notExtractedLabel?: string;

  /** 點列高亮（校對台有：點欄位 → 左欄原檔框起來；送件頁沒有）。 */
  focusedField?: ReviewField | null;
  onFocusField?: (f: ReviewField) => void;

  /** 標記欄的額外內容（例如「改金額會多一關」）。 */
  extraMark?: (f: ReviewField) => React.ReactNode;

  /** 第一欄與最後一欄的寬度微調（兩邊版面寬度不同）。 */
  labelWidth?: number | string;
  markWidth?: number;
}

const FieldDiffGrid: React.FC<FieldDiffGridProps> = ({
  fields, templateId, boxedFields,
  ocrExtracted, changedFields, draft, onChange, editable,
  rowTestId, inputTestId, ocrTestId,
  notExtractedLabel, focusedField, onFocusField, extraMark,
  labelWidth, markWidth = 96,
}) => {
  const translate = useTranslate();
  const clickable = !!onFocusField;
  const rows = fields ?? REVIEW_FIELDS;
  /** 這個欄位在紙上找得到位置嗎 —— 決定色點是實心還是空心。 */
  const hasBox = (f: string) => !!boxedFields?.includes(f);

  return (
    /**
     * **寧可橫捲，也不要把輸入框壓扁。**
     * 沒有 `minWidth` 的話，容器一窄 MUI 就把四欄等比壓縮，
     * 「最終值」那格會掉到 43px —— `60000.00` 只看得見 `60`。
     * 一個看不見自己輸入什麼的欄位，比要左右捲一下更糟。
     */
    <Box sx={{ overflowX: 'auto' }}>
      <Table size="small" sx={{ minWidth: 480 }}>
        <TableHead>
          <TableRow>
            <TableCell sx={labelWidth ? { width: labelWidth } : undefined}>
              {translate('energy.review.field')}
            </TableCell>
            <TableCell>{translate('energy.review.ocrValue')}</TableCell>
            <TableCell sx={{ minWidth: 150 }}>{translate('energy.review.finalValue')}</TableCell>
            <TableCell sx={{ width: markWidth }} />
          </TableRow>
        </TableHead>
        <TableBody>
          {rows.map((f) => {
            // `customerNumberRaw` 在辨識值那一層叫 `customerNumber`（見 `extractedValueOf`）。
            const extracted = extractedValueOf(ocrExtracted, f);
            const changed = !!changedFields?.includes(f);
            const focused = focusedField === f;
            return (
              <TableRow
                key={f}
                data-testid={rowTestId}
                data-field={f}
                data-changed={changed ? '1' : '0'}
                {...(clickable ? { 'data-focused': focused ? '1' : '0' } : {})}
                onClick={clickable ? () => onFocusField!(f) : undefined}
                sx={{
                  ...(clickable ? { cursor: 'pointer' } : {}),
                  // 被點到的那一列用**該欄位的顏色**（不是通用灰底）——
                  // 左邊的框同時變粗變濃，兩邊是同一個動作的兩個表現。
                  ...(focused ? {
                    background: fieldFill(f, '18'),
                    boxShadow: `inset 3px 0 0 ${colorForField(f)}`,
                  } : {}),
                }}
              >
                <TableCell sx={{ whiteSpace: 'nowrap' }}>
                  {/* **顏色是左右兩邊的連結**：這一列的色點 ＝ 紙上那個框的顏色。
                      找不到位置的欄位畫空心 —— 點它不會有東西亮起來，那要看得出來。 */}
                  <Stack direction="row" alignItems="center" gap={0.75}>
                    <Box
                      data-testid="field-color"
                      data-field={f}
                      data-boxed={hasBox(f) ? '1' : '0'}
                      sx={{
                        width: 10,
                        height: 10,
                        borderRadius: '2px',
                        flex: '0 0 auto',
                        border: `2px solid ${colorForField(f)}`,
                        background: hasBox(f) ? colorForField(f) : 'transparent',
                      }}
                    />
                    <Typography variant="body2">
                      {translate(billFieldLabelKey(f, templateId), { _: f })}
                    </Typography>
                  </Stack>
                </TableCell>
                <TableCell>
                  {/* 辨識值**永遠原樣顯示**，不被最終值覆蓋 ——
                      這一整格存在的理由就是這一欄。 */}
                  <Typography
                    variant="body2"
                    {...(ocrTestId ? { 'data-testid': ocrTestId } : {})}
                    sx={{ color: jvTokens.muted, fontVariantNumeric: 'tabular-nums' }}
                  >
                    {extracted === null || extracted === undefined || extracted === ''
                      ? (notExtractedLabel ?? '—')
                      : String(extracted)}
                  </Typography>
                </TableCell>
                <TableCell sx={{ minWidth: 150 }}>
                  <TextField
                    size="small"
                    fullWidth
                    value={draft[f] ?? ''}
                    // 可編輯性**完全由呼叫端的 `canEditSubject` 決定**
                    // （`LIFECYCLE.md §6.8`）。不要在這裡再推導一次 ——
                    // 那會變成第二份判定，而兩份遲早會長不一樣。
                    disabled={!editable}
                    onChange={(e) => onChange(f, e.target.value)}
                    inputProps={{ 'data-testid': inputTestId(f) }}
                  />
                </TableCell>
                <TableCell sx={{ width: markWidth }}>
                  {extraMark?.(f)}
                  {changed ? (
                    <Chip
                      size="small"
                      variant="outlined"
                      label={translate('energy.bills.corrected')}
                      sx={{ color: jvTokens.primary, borderColor: jvTokens.primary }}
                    />
                  ) : null}
                </TableCell>
              </TableRow>
            );
          })}
        </TableBody>
      </Table>
    </Box>
  );
};

export default FieldDiffGrid;
