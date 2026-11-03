import * as React from 'react';
import { Box, Tabs, Tab } from '@mui/material';
import { useTranslate } from 'react-admin';
import { useSearchParams } from 'react-router-dom';
import WorkbenchPage from './WorkbenchPage';
import ExecutionsPage from './ExecutionsPage';
import BillDetailDrawer from './BillDetailDrawer';

type TabKey = 'todo' | 'mine';
const TABS: TabKey[] = ['todo', 'mine'];

/**
 * **「我的工作」—— 兩個頁籤，不是兩頁**（`LIFECYCLE.md §7.3`）。
 *
 * ### 這一頁是把「原本要新增的那一頁」收進既有的頁裡
 *
 * 設計初稿把「我的流程單」畫成**獨立頁面**。那是錯的：
 * 它與工作台都是「**我的單**」，只是**時間切面**不同 ——
 * 一個是「現在等我做的」，一個是「我碰過的全部」。
 *
 * | 頁籤 | 內容 | 主體 | 端點 |
 * |---|---|---|---|
 * | **等我處理**（預設，登入落點） | 依角色變形的分區 | `energyWorkflowTask` | `GET /workbench` |
 * | **我參與過的** | 我發起／曾被指派／曾在候選池／代理過的**全部**，含已完成 | `energyWorkflowExecution` | `GET /executions?scope=mine` |
 *
 * ### 不要在 UI 上洩漏 task 與 execution 的差別
 *
 * **`task` 是 `execution` 的內部結構** —— 使用者不該需要知道那個差別。
 * 畫面上兩者都是「一列＝一張單」。那正是合併的理由：
 * 分成兩頁等於要求使用者先理解引擎的資料模型，才找得到自己的單。
 *
 * ### 「參與過」的定義（`LIFECYCLE.md §6.6.2`）
 *
 * 發起人 ∨ 曾被指派（**含已完成的 task**）∨ **曾在候選池**（即使沒認領）∨ 代理人。
 * **一旦成立就永久成立。**
 *
 * 不可以只看「目前的」task —— 「我曾被要求處理」本身就是參與，
 * 而只看目前 task 的實作**在畫面上長得一模一樣**（有資料、能點、不報錯），
 * 只是少了那幾筆。判定的唯一實作在後端 `visibleExecutionIds()`。
 */
const MyWorkConsolePage: React.FC = () => {
  const translate = useTranslate();
  const [searchParams, setSearchParams] = useSearchParams();
  const raw = searchParams.get('tab');
  const tab: TabKey = TABS.includes(raw as TabKey) ? (raw as TabKey) : 'todo';

  const setTab = (v: TabKey) => {
    const next = new URLSearchParams(searchParams);
    // 預設頁籤不寫進網址 —— 登入落點的網址要是乾淨的 `#/energyMyWorkConsole`。
    if (v === 'todo') next.delete('tab'); else next.set('tab', v);
    setSearchParams(next, { replace: true });
  };

  /**
   * 「我參與過的」也用**同一個抽屜、同一個元件**（`LIFECYCLE.md §8.7`）——
   * 不要為那個頁籤另做一套檢視方式。
   *
   * 差別只在那邊的 `mode` 幾乎都是 `readonly`，
   * 所以抽屜底部自然印的是 `modeReason` 而不是動作列 —— 那是 `BillDetailBody`
   * 本來就有的行為，這裡不需要為它加任何分支。
   *
   * 網址參數用 `?bill=` 而不是「等我處理」那邊的 `?task=`：
   * 這個頁籤的主體是 **execution**，它沒有「目前這一關的 task」可以指。
   * 兩個頁籤各自記住自己開在哪一筆，切回來還在。
   */
  const billParam = searchParams.get('bill');
  const openBillId = billParam ? Number(billParam) : null;
  const setOpenBillId = (id: number | null) => {
    const next = new URLSearchParams(searchParams);
    if (id == null) next.delete('bill'); else next.set('bill', String(id));
    setSearchParams(next, { replace: true });
  };

  return (
    <Box>
      <Box px={2} pt={2}>
        <Tabs value={tab} onChange={(_, v) => setTab(v)} data-testid="mywork-tabs" data-active={tab}>
          {TABS.map((t) => (
            <Tab key={t} value={t} data-testid={`mywork-tab-${t}`} label={translate(`energy.myWork.tab.${t}`)} />
          ))}
        </Tabs>
      </Box>
      {/*
        兩個頁籤各自是**既有的整頁元件** —— 刻意不把它們拆開重組：
        「等我處理」那一格的規格（依角色變形的分區、四層優先序、退回理由印在列上）
        一個字都沒有變，重寫一份只會讓兩邊慢慢長歪。
      */}
      {tab === 'todo' ? <WorkbenchPage embedded /> : (
        <Box px={2} pb={2}>
          <ExecutionsPage
            scope="mine"
            embedded
            onOpenBill={(billId) => { setOpenBillId(billId); return true; }}
          />
          <BillDetailDrawer
            open={openBillId != null}
            billId={openBillId}
            // 這個頁籤沒有佇列語意（它是「我碰過的全部」不是「等我做的」）——
            // `queue=null` 讓 `BillDetailBody` 走一般的頁面模式。
            queue={null}
            onClose={() => setOpenBillId(null)}
          />
        </Box>
      )}
    </Box>
  );
};

export default MyWorkConsolePage;
