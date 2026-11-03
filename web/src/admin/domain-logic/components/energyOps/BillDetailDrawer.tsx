import * as React from 'react';
import {
  Drawer, Box, Stack, Typography, IconButton, Divider,
} from '@mui/material';
import CloseIcon from '@mui/icons-material/Close';
import { useTranslate } from 'react-admin';
import BillDetailBody, { type BillDetailQueue } from './BillDetailBody';
import { jvTokens } from './shared';

/**
 * 「我的工作」的單據抽屜（`LIFECYCLE.md §8.4`）。
 *
 * **抽屜不是彈窗。** 彈窗遮住整個列表，佇列的脈絡就沒了 ——
 * 而這一頁的目的是**把佇列清空**，不是「找到某一張單」。
 * 抽屜留著左側，處理完關掉位置還在、下一筆就在眼前。
 *
 * 寬度 56%：原檔需要空間，但列表必須留得住。
 *
 * 標頭的「第 N / M 筆」是**佇列的進度感** —— 沒有它使用者不知道還要多久。
 *
 * 內容是 `BillDetailBody`，跟路由頁 `#/energyBillConsole/:id` **同一份實作**。
 */
const BillDetailDrawer: React.FC<{
  open: boolean;
  billId: number | null;
  queue: BillDetailQueue | null;
  onClose: () => void;
}> = ({ open, billId, queue, onClose }) => {
  const translate = useTranslate();
  return (
    <Drawer
      anchor="right"
      open={open && billId != null}
      onClose={onClose}
      /**
       * `keepMounted={false}`（預設）＝ 關掉就卸載。
       * 這一頁很重，留著它會讓下一次開啟拿到**上一筆的殘影**
       * （舊 bill 先閃一下才換成新的）。
       */
      PaperProps={{
        sx: { width: '56%', minWidth: 560 },
        'data-testid': 'mywork-drawer',
      } as any}
    >
      <Stack
        direction="row"
        alignItems="center"
        justifyContent="space-between"
        sx={{ px: 2, py: 1.5, borderBottom: `1px solid ${jvTokens.border}` }}
      >
        <Typography variant="subtitle2" data-testid="drawer-progress" sx={{ fontWeight: 600 }}>
          {queue
            ? translate('energy.myWork.queueProgress', {
              index: queue.index, total: queue.total,
            })
            : ''}
        </Typography>
        {/* Esc 由 MUI 的 Drawer 處理；這顆是滑鼠的出口。 */}
        <IconButton size="small" data-testid="drawer-close" onClick={onClose}>
          <CloseIcon fontSize="small" />
        </IconButton>
      </Stack>
      <Divider />
      <Box sx={{ overflowY: 'auto' }}>
        {billId != null && (
          <BillDetailBody
            // `key` 讓換下一筆時整塊重新掛載 —— 不然上一筆的草稿會留在欄位裡。
            key={billId}
            billId={billId}
            queue={queue}
          />
        )}
      </Box>
    </Drawer>
  );
};

export default BillDetailDrawer;
