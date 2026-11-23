import * as React from 'react';
import { Box, Chip, Stack, Typography } from '@mui/material';
import { useTranslate } from 'react-admin';
import type { RecognitionSummary } from 'common/plugins/EnergyOpsCommonPlugin/taipowerExtractor';
import { jvTokens } from './shared';

/**
 * **辨識這件事本身的結果**（`RECOGNITION.md §7.2`）。
 *
 * ### 為什麼這一格非有不可
 *
 * 我們對每一份 PDF 跑**三條路**（文字層 ＋ PaddleOCR ＋ Tesseract）再交叉驗證
 * （`§9-4` 的 L2 裁定）。那件事很貴 —— 但如果結果不上畫面，
 * 「三來源都同意」與「只有一個引擎看到、另外兩個沒看到」在校對台上長得**一模一樣**，
 * 承辦沒有任何線索知道哪一份該多看一眼。那等於白跑。
 *
 * ### 兩件事，兩種份量
 *
 * | | 是什麼 | 畫面上怎麼呈現 |
 * |---|---|---|
 * | `issues`（blocking） | 服務**自己說**核心欄位有問題（勾稽對不起來、必要欄位缺） | 逐條列出來，用警示色 |
 * | `resolutions` | 每個欄位的值**是怎麼決定的** | 一排計數 chip |
 *
 * 這裡**不看** `record.reviewStatus` —— 交付版永遠回 `needs_review`，
 * 拿它當號誌等於每一份都亮紅燈（`taipowerCanonical` 有記這件事）。
 */
const RecognitionNote: React.FC<{
  summary?: RecognitionSummary | null;
  /** 緊湊版（表格列上用）：只留最重要的那一顆 chip。 */
  dense?: boolean;
}> = ({ summary, dense }) => {
  const translate = useTranslate();
  if (!summary) return null;

  // 辨識失敗：錯誤碼是**可行動的資訊**（`password_required` → 去要密碼），
  // 所以它排在最前面，而且不跟 issues 混在一起。
  if (summary.errorCode) {
    return (
      <Chip
        size="small"
        variant="outlined"
        data-testid="recognition-error"
        data-code={summary.errorCode}
        label={translate(`energy.recognition.error.${summary.errorCode}`, {
          _: summary.errorMessage || summary.errorCode,
        })}
        sx={{ color: jvTokens.warn, borderColor: jvTokens.warn }}
      />
    );
  }

  const blocking = summary.issues.filter((i) => i.blocking);
  const nonBlocking = summary.issues.filter((i) => !i.blocking);

  if (dense) {
    // 列上只講**要不要人看**。細節在展開的校對區裡，講兩次只會把那一列撐爆。
    if (!blocking.length) return null;
    return (
      <Chip
        size="small"
        variant="outlined"
        data-testid="recognition-blocking"
        data-count={blocking.length}
        label={translate('energy.recognition.blocking', { count: blocking.length })}
        sx={{ color: jvTokens.warn, borderColor: jvTokens.warn }}
      />
    );
  }

  const resolutionKeys = Object.keys(summary.resolutions).sort();

  return (
    <Box data-testid="recognition-note" sx={{ mb: 2 }}>
      <Stack direction="row" gap={1} alignItems="center" flexWrap="wrap" sx={{ mb: 1 }}>
        <Typography variant="caption" sx={{ color: jvTokens.muted }}>
          {translate('energy.recognition.title')}
        </Typography>
        {summary.templateLabel ? (
          <Chip
            size="small"
            variant="outlined"
            data-testid="recognition-template"
            data-template={summary.templateId ?? ''}
            label={summary.templateLabel}
          />
        ) : null}
        {summary.extractionMethod ? (
          <Chip
            size="small"
            variant="outlined"
            data-testid="recognition-method"
            data-method={summary.extractionMethod}
            label={translate(`energy.recognition.method.${summary.extractionMethod}`, {
              _: summary.extractionMethod,
            })}
            // 文字層＝從 PDF 裡拆出來的精確字串，比 OCR 認出來的可信 ——
            // 兩者同色的話這句話就沒有被說出來。
            sx={summary.extractionMethod === 'pdf_text'
              ? { color: jvTokens.ok, borderColor: jvTokens.ok }
              : undefined}
          />
        ) : null}
      </Stack>

      {resolutionKeys.length ? (
        <Stack direction="row" gap={0.5} flexWrap="wrap" sx={{ mb: 1 }}>
          {resolutionKeys.map((k) => (
            <Chip
              key={k}
              size="small"
              variant="outlined"
              data-testid="recognition-resolution"
              data-resolution={k}
              data-count={summary.resolutions[k]}
              label={`${translate(`energy.recognition.resolution.${k}`, { _: k })} ${summary.resolutions[k]}`}
              sx={k === 'conflict'
                ? { color: jvTokens.warn, borderColor: jvTokens.warn }
                : { color: jvTokens.muted }}
            />
          ))}
        </Stack>
      ) : null}

      {/* 逐條列出來，**不要只說「有 3 項問題」** ——
          問題本身（哪個欄位、為什麼）才是承辦能拿去對紙的東西。 */}
      {summary.issues.length ? (
        <Stack gap={0.5}>
          {[...blocking, ...nonBlocking].map((i, idx) => (
            <Typography
              // 同一個 code 可能出現多次（不同欄位），所以 key 帶 index。
              key={`${i.code}-${idx}`}
              variant="caption"
              display="block"
              data-testid="recognition-issue"
              data-code={i.code}
              data-blocking={i.blocking ? '1' : '0'}
              sx={{ color: i.blocking ? jvTokens.warn : jvTokens.muted }}
            >
              {i.message}
              {i.fields.length ? `（${i.fields.join('、')}）` : ''}
            </Typography>
          ))}
        </Stack>
      ) : summary.templateId ? (
        <Typography variant="caption" sx={{ color: jvTokens.muted }} data-testid="recognition-clean">
          {translate('energy.recognition.noIssues')}
        </Typography>
      ) : null}
    </Box>
  );
};

export default RecognitionNote;
