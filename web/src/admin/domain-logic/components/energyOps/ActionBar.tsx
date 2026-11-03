import * as React from 'react';
import { Button, Stack, Typography } from '@mui/material';
import { useTranslate } from 'react-admin';
import { jvTokens } from './shared';

export interface TaskAction {
  actionKey: string; labelKey: string; kind: string; requiresReason: boolean; sortOrder: number;
}
export interface ActionBlocked {
  code: string;
  nodeName?: string | null;
  assigneeHint?: string | null;
}

/**
 * 動作按鈕列 —— **共用元件**（`FRONTEND.md §2.8.3` 規則 3）。
 *
 * 這一句說明必須出現在**三個地方**：P1 工作台的列、P3 校對台的動作列、
 * P9 節點 drawer。只做校對台的話，使用者仍然會在工作台看到假按鈕 ——
 * 所以做成元件，不在三個頁面各寫一份。
 *
 * **不畫按不動的按鈕，也不畫 disabled 的**（disabled 一樣不說明原因）。
 * `availableActions` 空 → 這一區換成**那一句說明**。
 *
 * 判定全部來自後端的 `actionBlocked`，前端不自己算 ——
 * 要自己判斷「我是不是候選人」就得複製一份 R1–R4，兩套遲早不一致。
 */
const ActionBar: React.FC<{
  actions: TaskAction[];
  blocked?: ActionBlocked | null;
  onAction: (a: TaskAction) => void;
  size?: 'small' | 'medium';
}> = ({ actions, blocked, onAction, size = 'small' }) => {
  const translate = useTranslate();

  if (!actions.length) {
    if (!blocked) return null;
    // 顯示句子，**不顯示 code**。
    const msg = translate(`energy.actionBlocked.${blocked.code}`, {
      _: translate('energy.actionBlocked.generic'),
      nodeName: blocked.nodeName ?? '',
      assigneeHint: blocked.assigneeHint ?? translate('energy.actionBlocked.someoneElse'),
    });
    return (
      <Typography
        variant="caption"
        data-testid="action-blocked"
        data-code={blocked.code}
        sx={{ color: jvTokens.caption, display: 'block' }}
      >
        {msg}
      </Typography>
    );
  }

  return (
    <Stack direction="row" gap={1} flexWrap="wrap" data-testid="action-bar">
      {[...actions].sort((a, b) => a.sortOrder - b.sortOrder).map((a) => (
        <Button
          key={a.actionKey}
          size={size}
          variant={a.kind === 'advance' ? 'contained' : 'outlined'}
          data-testid="action-button"
          data-action={a.actionKey}
          // 回退／中斷用**燒橘**（不是紅）。
          sx={a.kind === 'revert' || a.kind === 'abort'
            ? { color: jvTokens.warn, borderColor: jvTokens.warn } : undefined}
          onClick={() => onAction(a)}
        >
          {translate(a.labelKey, { _: a.actionKey })}
        </Button>
      ))}
    </Stack>
  );
};

export default ActionBar;
