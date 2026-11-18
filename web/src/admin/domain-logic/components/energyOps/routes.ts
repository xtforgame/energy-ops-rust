/**
 * **「哪個資源的哪一筆 → 去哪一頁」的唯一一份對照**（`FRONTEND.md §1.4.5`）。
 *
 * ### 為什麼非要收斂成一份不可
 *
 * 這份表是被**兩次真實錯誤**逼出來的：
 *
 * 1. **後端在產生前端路由**：`ExecutionListService` 每列回一個
 * `route: '/energyBillConsole/:id/show'`，`withTargetLinks()` 也回 `targetLink`。
 * 後端知道前端長什麼樣 → 改一個路由名要回頭改後端，
 * 而且**後端替前端做了導覽決策**，於是 P12 的每一列變成電費單的捷徑。
 * 2. **多型只處理了一半**：四個頁面各自寫
 * `subject.billId ? navigate(bill) : undefined` ——
 * 保證金那類（subject 是 `energyInstallment`，**沒有 bill**）
 * 點下去**什麼都不會發生，也沒有任何提示**。
 * 那正是專案記過的「按鈕會說謊」：症狀跟系統壞掉一模一樣。
 *
 * 收斂成一份之後，「多型少處理一支」這種錯**只會發生在這個檔案裡**，
 * 不會散在六個頁面各犯一次。
 *
 * ### 分層規則
 *
 * **後端只回「是哪個資源的哪一筆」**（`targetType` / `targetId` / `subject.kind`）；
 * **「要跳去哪」永遠是前端的決定** —— 這個檔案就是那個決定。
 */

/** 稽核事件的目標型別（與後端 `energyAuditEvent.targetType` 對齊）。 */
export type TargetType =
  | 'energyBill'
  | 'energyStation'
  | 'energyLandowner'
  | 'energyWorkflowExecution'
  | 'energyWorkflowTask'
  | 'energyPayoutRecord'
  | 'energyWorkflowDefinition'
  | 'energyRatingStrategy'
  | string;

/** 一筆流程單的家 ＝ **P12 詳情**（不是它的 subject 的家）。 */
export const executionRoute = (executionId: number | string) => `/energyExecutionConsole/${executionId}/show`;

/** 一張電費單的家 ＝ P3 校對台（`FRONTEND.md §1.1`：P3 是 P2 的 detail）。 */
export const billRoute = (billId: number | string) => `/energyBillConsole/${billId}/show`;

export const stationRoute = (stationId: number | string) => `/energyStationConsole/${stationId}/show`;

/**
 * 卡 E S4：通知頁「點一筆跳到行事曆那一天」。
 * `CalendarPage` 讀 `month`／`view` 兩個 query 參數，沒帶就照自己原本的預設（本月／月曆檢視）。
 */
export const calendarRoute = (eventDate?: string | null) => {
  if (!eventDate) return '/energyCalendarConsole';
  const month = eventDate.slice(0, 7);
  return `/energyCalendarConsole?month=${encodeURIComponent(month)}&view=list`;
};

/**
 * 「我的工作」（`LIFECYCLE.md §7.3`）—— 兩個頁籤共用一條路由。
 *
 * `tab` 是**頁籤不是路由**：`todo`（等我處理，登入落點）／`mine`（我參與過的）。
 * 預設那一格不寫進網址 —— 落點的網址要乾淨。
 */
export const myWorkRoute = (tab?: 'todo' | 'mine') => (
  tab && tab !== 'todo' ? `/energyMyWorkConsole?tab=${tab}` : '/energyMyWorkConsole'
);

/** 送件清單（`LIFECYCLE.md §7.2`：收件台 ＋ 送單紀錄合併後的家）。 */
export const ingestListRoute = () => '/energyIngestConsole';

/**
 * **一批送件的家 ＝ 送件詳情（校對工作區）**。
 *
 * `itemId` 只是**展開哪一列**，不是另一個路由 ——
 * 校對是在詳情頁裡就地展開的（`LIFECYCLE.md §6.5.2`），
 * 草稿屬於批次，離開批次就沒有草稿的家。
 */
export const ingestBatchRoute = (batchId: number | string, itemId?: number | string | null) => (
  itemId != null
    ? `/energyIngestConsole/${batchId}?item=${encodeURIComponent(String(itemId))}`
    : `/energyIngestConsole/${batchId}`
);
export const landownerRoute = (landownerId: number | string) => `/energyLandowners/${landownerId}`;

/**
 * P2 電費單清單，帶篩選（`month` 是**計費期間**，不是流程的業務日期）。
 *
 * `tab='pendingMatch'` ＝ 例外件那一格（`LIFECYCLE.md §7.4`：
 * 例外件是這一頁的**一個頁籤**，不是一個資源）。
 */
export const billListRoute = (params: Record<string, string | number | undefined> = {}) => {
  const qs = Object.entries(params)
    .filter(([, v]) => v !== undefined && v !== null && v !== '')
    .map(([k, v]) => `${k}=${encodeURIComponent(String(v))}`)
    .join('&');
  return qs ? `/energyBillConsole?${qs}` : '/energyBillConsole';
};

/** P12 清單，帶篩選。 */
export const executionListRoute = (params: Record<string, string | number | undefined>) => {
  const qs = Object.entries(params)
    .filter(([, v]) => v !== undefined && v !== null && v !== '')
    .map(([k, v]) => `${k}=${encodeURIComponent(String(v))}`)
    .join('&');
  return qs ? `/energyExecutionConsole?${qs}` : '/energyExecutionConsole';
};

/** execution 清單／詳情回的 subject 摘要（多型）。 */
export interface SubjectRef {
  kind?: string | null;
  billId?: number | null;
  installmentId?: number | null;
  landownerId?: number | null;
  stationId?: number | null;
}

/**
 * **「它的對象」的家** —— 多型，**每一支都要處理**。
 *
 * | subject | 去哪 | 為什麼 |
 * |---|---|---|
 * | `energyBill` | P3 校對台 | 電費單的 detail 就是校對台 |
 * | `energyInstallment` | 地主頁（合約與期數區） | 保證金**沒有校對台** |
 *
 * 回 `null` 代表**真的沒有可去的地方** —— 呼叫端要**不畫按鈕**，
 * 不要畫一顆按了沒反應的（那比沒有按鈕更糟）。
 */
export const subjectRoute = (subject?: SubjectRef | null): string | null => {
  if (!subject) return null;
  if (subject.kind === 'energyBill' && subject.billId) return billRoute(subject.billId);
  // 保證金沒有自己的單據頁 —— 它的內容在合約的期數裡，家是地主頁。
  if (subject.kind === 'energyInstallment' && subject.landownerId) {
    return landownerRoute(subject.landownerId);
  }
  return null;
};

/**
 * **稽核事件的目標 → 路由**。
 *
 * 前一版這段住在**後端**（`withTargetLinks.linkOf()`），而且有兩個錯：
 * 1. 後端產生前端路由。
 * 2. `energyWorkflowExecution` / `energyWorkflowTask` 被轉成
 * **那筆流程底下那張 bill 的校對台** —— 於是「流程」類的稽核事件
 * 點下去卻到了單據頁；而**保證金那類直接回 `null`**（整類連不出去）。
 *
 * 現在：流程事件連 **P12**，不再繞去 bill；多型無關，任何 subject 都連得到。
 *
 * @param executionIdOfTask `energyWorkflowTask` 需要它所屬的 execution
 * （後端回 `targetExecutionId`，前端無從自己查）。
 */
export const auditTargetRoute = (
  targetType: TargetType | null | undefined,
  targetId: number | string | null | undefined,
  extra?: { executionId?: number | null; billId?: number | null },
): string | null => {
  const id = Number(targetId);
  const ok = Number.isInteger(id) && id > 0;
  switch (targetType) {
    case 'energyBill': return ok ? billRoute(id) : null;
    case 'energyStation': return ok ? stationRoute(id) : null;
    case 'energyLandowner': return ok ? landownerRoute(id) : null;
    // 流程事件 → 流程單（不是它底下那張 bill）。
    case 'energyWorkflowExecution': return ok ? executionRoute(id) : null;
    case 'energyWorkflowTask':
      return extra?.executionId ? executionRoute(extra.executionId) : null;
    // 應付紀錄掛在單上 —— 它的算式在校對台的應付頁籤。
    case 'energyPayoutRecord':
      return extra?.billId ? billRoute(extra.billId) : null;
    // 流程定義／策略版本 → P9（治理層的家）。
    case 'energyWorkflowDefinition':
    case 'energyRatingStrategy':
      return '/energyWorkflowConsole';
    default:
      // 連不到就回 null —— 畫面上**不畫按不動的按鈕**。
      return null;
  }
};

/**
 * **一筆待辦（task）點下去要去哪**（工作台 P1 / P9 節點 drawer）。
 *
 * 這裡的語意跟上面不同：待辦是「**我要做事**」，
 * 所以有單據頁的就去單據頁（那裡才有欄位可以改、按鈕可以按）。
 * 但**沒有單據頁的不可以回 null** —— 前一版就是
 * `billId ? navigate(bill) : undefined`，於是保證金的待辦**點了沒反應**。
 * 退而求其次去 **P12 詳情**：至少看得到它卡在哪、誰在處理。
 */
export const taskWorkRoute = (
  subject: SubjectRef | null | undefined,
  executionId: number | null | undefined,
): string | null => {
  const s = subjectRoute(subject);
  if (s) return s;
  return executionId ? executionRoute(executionId) : null;
};
