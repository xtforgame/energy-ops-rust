import * as React from 'react';
import { Box, Stack, Typography, CircularProgress } from '@mui/material';
import { useTranslate } from 'react-admin';
import { jvTokens } from './shared';

/**
 * **「它正在跑，你不用做事」** —— 辨識中的等待指示。
 *
 * ### 為什麼需要它
 *
 * 辨識是非同步的，而且**慢**（L2 三來源，一頁十幾秒；一批 50 份就是十幾分鐘）。
 * 在此之前畫面上只有一個靜止的「辨識中」文字標籤 —— 那看起來與
 * **卡住**、**壞掉**、**忘了送出**完全一樣，於是使用者會做三件多餘的事：
 * 一直按重新整理、重新上傳一次、或者跑來問人。
 *
 * 一個**會動的**東西回答的是另一個問題：不是「現在是什麼狀態」（標籤已經說了），
 * 而是「**這件事還活著嗎**」。所以這裡的動畫不是裝飾，它是那句話唯一的載體。
 *
 * ### 為什麼還要一句話
 *
 * 動畫只說「在跑」，沒說「要不要等」。使用者真正的問題是
 * 「我現在該做什麼」——答案是**什麼都不用做**，好了畫面會自己更新
 * （`IngestBatchDetailPage` 在 `recognizing` 時每 3 秒重讀）。
 * 那句話一定要寫出來，否則他還是會盯著看。
 *
 * ### 為什麼不用不定量進度條
 *
 * 進度條暗示「有進度可看」，但我們**問不到**服務跑到第幾頁 ——
 * 一條永遠在跑的假進度條，比一個誠實的轉圈更容易讓人覺得被騙。
 * 批次那一層有真的分母（`done / total`）時才用數字，那是另一回事。
 */
const RecognizingIndicator: React.FC<{
  /** `inline` 給表格的一列用（只有一顆小轉圈）；`block` 給區塊標題下方用。 */
  variant?: 'inline' | 'block';
  /** 批次層可以給「第幾份 / 共幾份」—— 有真的分母時才給，不要編。 */
  done?: number | null;
  total?: number | null;
  size?: number;
}> = ({
  variant = 'inline', done, total, size = 14,
}) => {
  const translate = useTranslate();
  const hasCount = typeof done === 'number' && typeof total === 'number' && total > 0;

  const spinner = (
    <CircularProgress
      size={size}
      thickness={5}
      // 不確定進度就**不要**假裝有進度：轉圈說的是「還活著」，不是「跑到哪」。
      sx={{ color: jvTokens.primary }}
    />
  );

  if (variant === 'inline') {
    return (
      <Stack
        direction="row"
        alignItems="center"
        gap={0.5}
        data-testid="recognizing-indicator"
        data-variant="inline"
      >
        {spinner}
        <Typography variant="caption" sx={{ color: jvTokens.primaryDark, fontWeight: 600 }}>
          {hasCount
            ? translate('energy.intake.recognizingCount', { done, total })
            : translate('energy.itemStatus.recognizing')}
        </Typography>
      </Stack>
    );
  }

  return (
    <Box
      data-testid="recognizing-indicator"
      data-variant="block"
      sx={{
        mt: 1,
        p: 1,
        borderRadius: 1,
        border: `1px solid ${jvTokens.hairline}`,
        background: jvTokens.hoverBg,
      }}
    >
      <Stack direction="row" alignItems="center" gap={1}>
        {spinner}
        <Typography variant="body2" sx={{ fontWeight: 600, color: jvTokens.primaryDark }}>
          {hasCount
            ? translate('energy.intake.recognizingCount', { done, total })
            : translate('energy.itemStatus.recognizing')}
        </Typography>
      </Stack>
      {/* 使用者真正的問題是「我要做什麼」—— 答案是什麼都不用做。 */}
      <Typography variant="caption" color="text.secondary" display="block" mt={0.25}>
        {translate('energy.intake.recognizingHint')}
      </Typography>
    </Box>
  );
};

export default RecognizingIndicator;
