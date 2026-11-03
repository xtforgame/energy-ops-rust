import * as React from 'react';
import { Badge } from '@mui/material';
import ReceiptLongIcon from '@mui/icons-material/ReceiptLong';
import { energyHttp, energyBase } from '@/api/energy';
import { MenuItemLink, useTranslate } from 'react-admin';
import { useSelection } from '@/platform/selection/store';
import { jvTokens } from './shared';

/**
 * **選單上的「電費單」—— 帶待指認角標**（`LIFECYCLE.md §7.4`）。
 *
 * ### 為什麼角標非有不可
 *
 * 例外件從獨立的一頁併回電費單的一個頁籤。合併本身是對的
 * （那本來就是同一個資源的一個篩選），但少了角標就會**變得更難被看見**：
 * 原本選單上「例外件」那一項至少會提醒承辦「有這個東西」，
 * 併進去之後如果沒有數字，等於要求他**記得**去點那個頁籤。
 *
 * > 合併不可以讓一件需要人處理的事變得更安靜。
 *
 * ### 數字從哪來
 *
 * `GET /bills/unmatched` —— 與電費單頁那個頁籤上的數字**同一支端點**，
 * 所以兩邊不可能對不起來。不要在這裡自己數一次
 * （前端數的版本會在後端改了「什麼算待指認」的那天默默偏掉）。
 *
 * 沒有權限（403）時就當作 0：不要把錯誤畫成角標 ——
 * 一個永遠亮著紅點又點不出東西的選單比沒有角標更糟。
 */
const BillsMenuItem: React.FC<{ dense?: boolean }> = ({ dense }) => {
  const translate = useTranslate();
  const orgId = useSelection()?.organizationId;
  const [count, setCount] = React.useState(0);

  React.useEffect(() => {
    if (!orgId) { setCount(0); return undefined; }
    let alive = true;
    energyHttp.get(`${energyBase(orgId)}/bills/unmatched`)
      .then(({ data }) => { if (alive) setCount(Array.isArray(data) ? data.length : 0); })
      .catch(() => { if (alive) setCount(0); });
    return () => { alive = false; };
  }, [orgId]);

  return (
    <MenuItemLink
      to="/energyBillConsole"
      state={{ _scrollToTop: true }}
      // 與 `getMenuItems` 的其他項目同一套命名（解說影片的 spotlight 目標）。
      data-testid="menu-energyBillConsole"
      primaryText={translate('resources.energyBillConsole.name', { smart_count: 2 })}
      leftIcon={(
        <Badge
          // 燒橘，不是紅 —— 紅是「壞掉」，這裡是「等人處理」。
          badgeContent={count}
          data-testid="menu-bill-badge"
          data-count={count}
          sx={{ '& .MuiBadge-badge': { backgroundColor: jvTokens.warn, color: '#fff' } }}
        >
          <ReceiptLongIcon />
        </Badge>
      )}
      dense={dense}
    />
  );
};

export default BillsMenuItem;
