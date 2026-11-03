import * as React from 'react';
import {
  Box, Card, CardContent, Typography, Table, TableHead, TableBody, TableRow, TableCell,
  Stack, Tooltip, IconButton, Chip, TableSortLabel, Button,
  Dialog, DialogTitle, DialogContent, DialogActions, TextField, Alert,
} from '@mui/material';
import RefreshIcon from '@mui/icons-material/Refresh';
import { energyHttp, energyBase } from '@/api/energy';
import { useNotify, useTranslate } from 'react-admin';
import { useSearchParams } from 'react-router-dom';
import { useSelection } from '@/platform/selection/store';
import { apiErrorCode } from '../../../dataProvider/apiError';
import {
  EnergyPageFrame, EmptyRow, Money, Period, jvTokens,
} from './shared';
import ActionBar, { type TaskAction, type ActionBlocked } from './ActionBar';
import BillDetailDrawer from './BillDetailDrawer';

interface Row {
  id: number;
  executionId: number;
  nodeKey: string;
  nodeName: string | null;
  waitingDays: number | null;
  claimable: boolean;
  claimedByMe: boolean;
  /** `completed` ＝ 這一列已經處理過了 —— **原地變灰保留**，不是移除。 */
  state?: string;
  subject: any;
  availableActions: TaskAction[];
  actionBlocked: ActionBlocked | null;
  /** US-15：退回理由與退回者，**直接印在列上**。 */
  rejectReason?: string | null;
  rejectedBy?: string | null;
}
interface Section {
  key: string; titleKey: string; sort: string; overdueDays?: number; rows: Row[];
}

/**
 * P1 角色工作台 —— **依角色變形，不是一個通用清單**（`FRONTEND.md §2 P1`）。
 *
 * **分區、排序、退回理由全部由後端算**（`GET .../workbench`）——
 * 只有後端知道呼叫者的 RBAC 角色**與部門身分**
 * （「部門主管」是組織事實不是 RBAC 角色）。前端不拼。
 *
 * 每一列的資訊密度：案場 · 地主 · 金額 · 目前關卡 · **等了幾天** · 動作。
 * 不要只有單號 —— 那會逼使用者每一列都點開。
 *
 * **不畫按不動的按鈕**（C7 增補 A）：`availableActions` 空時改顯示
 * 「你是這張單的發起人，不能核自己送出的單…」那一句（`ActionBar`）。
 *
 * ---
 *
 * ## 這是一條**佇列**，不是一張清單（`LIFECYCLE.md §8`）
 *
 * | | 目標 | 動線特性 |
 * |---|---|---|
 * | 清單 | 找到東西 | 可隨時重排、點進去就離開、回來重找 |
 * | **佇列** | **把它清空** | **位置必須穩定**、就地檢視、裁斷完直接接下一件 |
 *
 * 使用者實測回報的四件事全部是「用清單的做法服務佇列的目的」長出來的，
 * 所以這一頁有三條規則：
 *
 * 1. **動作不重排。** `claim` / `actions` 回更新後的那一列，就地替換 ——
 *    **不呼叫 `load()`**。重抓會讓那一列換區又依 `waitingDays` 重排，
 *    使用者剛按的那一列就跳到最下面去了。
 * 2. **檢視在抽屜裡，不換頁。** 換頁 ＝ 離開佇列，那一頁還有它自己的返回邏輯。
 * 3. **認領不在列上。** 它是「開始處理」的第一步，屬於抽屜。
 *    留在列上就會回到「認領了卻不知道去哪看」。
 */
export interface WorkbenchPageProps {
  /**
   * 內嵌在「我的工作」的頁籤裡時，**不要再印一次自己的標題** ——
   * 頁籤已經寫著「等我處理」，底下再來一個「工作台」是同一件事說兩次，
   * 而且那兩個名字還不一樣（使用者會以為是兩層東西）。
   * 重新整理那顆留著：它是這一頁真的會用到的動作。
   */
  embedded?: boolean;
}

const WorkbenchPage: React.FC<WorkbenchPageProps> = ({ embedded }) => {
  const notify = useNotify();
  const translate = useTranslate();
  const [searchParams, setSearchParams] = useSearchParams();
  const orgId = useSelection()?.organizationId;

  const [loading, setLoading] = React.useState(false);
  const [denied, setDenied] = React.useState(false);
  const [sections, setSections] = React.useState<Section[]>([]);
  /**
   * 門檻由**後端**回（`PAYOUT_OVERDUE_DAYS`，seed 常數）——
   * 前端這個 5 只是還沒載入時的暫代值，**不是規則**。客戶調門檻不必改前端。
   */
  const [overdueDays, setOverdueDays] = React.useState(5);
  /** 每一區各自的排序方向（「等了幾天」要能排序）。 */
  const [descBySection, setDescBySection] = React.useState<Record<string, boolean>>({});
  const [pending, setPending] = React.useState<{ row: Row; action: TaskAction } | null>(null);
  const [reason, setReason] = React.useState('');
  /**
   * 這一輪**已經處理掉**的 task id。
   *
   * 它們的列**原地變灰保留**，按「重新整理」才真的移除（`LIFECYCLE.md §8.3`）。
   * 立即消失會讓下一筆跳上來佔住滑鼠位置 —— 那是誤按的主要來源；
   * 也讓人失去「我剛做了什麼」的確認。
   */
  const [handled, setHandled] = React.useState<Set<number>>(new Set());

  const base = energyBase(orgId);

  /** 抽屜開在哪一筆 —— 寫在**網址**裡（`?task=`），refresh 回得來、貼給同事打得開。 */
  const openTaskId = searchParams.get('task') ? Number(searchParams.get('task')) : null;
  const setOpenTaskId = React.useCallback((id: number | null) => {
    const next = new URLSearchParams(searchParams);
    if (id == null) next.delete('task'); else next.set('task', String(id));
    // `replace` ＝ 開關抽屜不該塞滿上一頁的歷史；返回鍵要能一次離開這一頁。
    setSearchParams(next, { replace: true });
  }, [searchParams, setSearchParams]);

  const load = React.useCallback(async () => {
    if (!orgId) { setSections([]); setDenied(false); return; }
    setLoading(true); setDenied(false);
    try {
      const { data } = await energyHttp.get(`${base}/workbench`);
      setSections(Array.isArray(data?.sections) ? data.sections : []);
      // 重抓 ＝ 使用者明確要求的重排。已處理的列這時才真的消失。
      setHandled(new Set());
      if (data?.payoutOverdueDays) setOverdueDays(Number(data.payoutOverdueDays));
    } catch (e: any) {
      if (e?.response?.status === 403) setDenied(true);
      else notify(apiErrorCode(e) || e?.message || 'load failed', { type: 'error' });
    } finally { setLoading(false); }
  }, [orgId, base, notify]);

  React.useEffect(() => { load(); }, [load]);

  /**
   * 把**更新後的那一列**就地換掉（`LIFECYCLE.md §8.3`）。
   *
   * **這一支存在的全部理由就是「不要呼叫 `load()`」。**
   * 重抓整頁會讓那一列換區、又依 `waitingDays` 重排 ——
   * 使用者剛按的那一列跳到最下面，而他的注意力還停在原位。
   *
   * 只換內容、**不動順序**：`map` 保持原本的陣列位置。
   */
  const replaceRow = React.useCallback((row: any) => {
    if (!row?.id) return;
    setSections((prev) => prev.map((sec) => ({
      ...sec,
      rows: sec.rows.map((r) => (r.id === Number(row.id) ? { ...r, ...row } : r)),
    })));
    // 做完了的列標成「已處理」—— 灰掉留在原位，等使用者按重新整理才移除。
    if (row.state === 'completed') {
      setHandled((prev) => new Set(prev).add(Number(row.id)));
    }
  }, []);

  const runAction = async (row: Row, action: TaskAction, withReason?: string) => {
    try {
      const { data } = await energyHttp.post(`${base}/tasks/${row.id}/actions`, {
        actionKey: action.actionKey, ...(withReason ? { reason: withReason } : {}),
      });
      notify('energy.myTasks.actionDone', { type: 'success' });
      setPending(null); setReason('');
      if (data?.row) replaceRow(data.row);
      // 後端沒回列（舊版或流程整個結束）才退回重抓 —— 那是相容路徑，不是常態。
      else load();
    } catch (e: any) { notify(apiErrorCode(e) || 'action failed', { type: 'error' }); }
  };

  const onAction = (row: Row, a: TaskAction) => {
    // 需要理由的動作先開對話框；不送空理由（稽核的理由欄不能是空的）。
    if (a.requiresReason) { setReason(''); setPending({ row, action: a }); return; }
    runAction(row, a);
  };

  /**
   * **畫面上看到的順序**攤平成一條佇列。
   *
   * 「下一筆」的定義是「目前排序下的下一列」（`LIFECYCLE.md §8.5`），
   * 所以它必須跟畫面一致 —— 用後端回的原始順序算會跟使用者看到的不同，
   * 而「下一筆」跳到一個他沒看到的地方，比沒有這顆按鈕更糟。
   */
  const flatQueue = React.useMemo(() => {
    const out: Row[] = [];
    for (const sec of sections) {
      const desc = descBySection[sec.key] ?? true;
      const rows = [...sec.rows].sort(
        (a, b) => ((b.waitingDays ?? 0) - (a.waitingDays ?? 0)) * (desc ? 1 : -1),
      );
      out.push(...rows);
    }
    return out;
  }, [sections, descBySection]);

  const openRow = openTaskId != null ? flatQueue.find((r) => r.id === openTaskId) ?? null : null;
  const openIndex = openRow ? flatQueue.findIndex((r) => r.id === openRow.id) : -1;

  /** 下一筆＝往後找第一個**還沒處理**的（`LIFECYCLE.md §8.5`：跳過已處理的）。 */
  const gotoNext = React.useCallback(() => {
    if (openIndex < 0) { setOpenTaskId(null); return; }
    const next = flatQueue.slice(openIndex + 1).find((r) => !handled.has(r.id) && r.state !== 'completed');
    /**
     * **這裡是「接下一筆」那條路上唯一發通知的地方。**
     *
     * `BillDetailBody` 刻意不發 —— RA 的通知是佇列式的，
     * 兩句連著發，第二句會被第一句擋住四秒，
     * 而「這一區處理完了」正是那個被擋住的第二句。
     */
    if (next) {
      setOpenTaskId(next.id);
      notify('energy.myTasks.actionDone', { type: 'success' });
      return;
    }
    // 到底了 —— 關掉抽屜並說一聲。沒有這一句使用者會以為抽屜壞了。
    setOpenTaskId(null);
    notify('energy.myWork.sectionDone', { type: 'info' });
  }, [openIndex, flatQueue, handled, setOpenTaskId, notify]);

  const renderSection = (sec: Section) => {
    const desc = descBySection[sec.key] ?? true;
    const rows = [...sec.rows].sort(
      (a, b) => ((b.waitingDays ?? 0) - (a.waitingDays ?? 0)) * (desc ? 1 : -1),
    );
    // 只有「等我付款」那一區有門檻；其餘區沒有「逾期」的概念。
    const overdue = sec.overdueDays ?? (sec.key === 'toPay' ? overdueDays : undefined);
    const overdueCount = overdue != null
      ? rows.filter((r) => (r.waitingDays ?? 0) >= overdue).length : 0;

    return (
      <Card sx={{ mb: 2 }} key={sec.key} data-testid="workbench-section" data-section={sec.key}>
        <CardContent>
          <Stack direction="row" alignItems="center" gap={1} mb={1}>
            <Typography variant="subtitle1">
              {translate(sec.titleKey)}（{rows.length}）
            </Typography>
            {/* 「已放行超過 N 天未付」的專屬計數 ＋ 燒橘（不是紅）。 */}
            {overdue != null && overdueCount > 0 && (
              <Chip
                size="small" variant="outlined"
                data-testid="workbench-overdue"
                label={translate('energy.workbench.overdue', { days: overdue, count: overdueCount })}
                sx={{ color: jvTokens.warn, borderColor: jvTokens.warn, fontWeight: 600 }}
              />
            )}
          </Stack>
          <Table size="small">
            <TableHead>
              <TableRow>
                <TableCell>{translate('energy.bills.station')}</TableCell>
                <TableCell>{translate('energy.landowners.name')}</TableCell>
                <TableCell align="right">{translate('energy.bills.amount')}</TableCell>
                <TableCell>{translate('energy.myTasks.node')}</TableCell>
                <TableCell>
                  {/* 「等了幾天」是這一頁最重要的欄位，且**要能排序**。 */}
                  <TableSortLabel
                    active
                    direction={desc ? 'desc' : 'asc'}
                    data-testid="workbench-sort-waiting"
                    onClick={() => setDescBySection({ ...descBySection, [sec.key]: !desc })}
                  >
                    {translate('energy.myTasks.waiting')}
                  </TableSortLabel>
                </TableCell>
                <TableCell align="right" />
              </TableRow>
            </TableHead>
            <TableBody>
              {rows.length ? rows.map((r) => {
                const hot = overdue != null && (r.waitingDays ?? 0) >= overdue;
                const isHandled = handled.has(r.id) || r.state === 'completed';
                return (
                  <TableRow
                    key={r.id}
                    hover
                    data-testid="workbench-row"
                    data-node={r.nodeKey}
                    data-task={r.id}
                    // 這一列是**哪一張單**的 —— 驗收要指名道姓地找某一筆，
                    // 而不是「按第一列」（佇列裡本來就會有別人的東西）。
                    data-bill={r.subject?.billId ?? ''}
                    // 這一筆待辦屬於**哪一條流程** —— 同一張單會同時出現在
                    // 兩條流程的佇列裡，驗收要問「抽屜綁的是不是這一條」。
                    data-execution={r.executionId ?? ''}
                    data-handled={isHandled ? '1' : '0'}
                    /**
                     * **整列可點**（`LIFECYCLE.md §8.2`）——
                     * 不是只有案場名可點。同一個心智動作（「我要看這一筆」）
                     * 以前被拆成兩個控制項：檢視在左邊的案場名、動作在右邊。
                     */
                    onClick={() => setOpenTaskId(r.id)}
                    sx={{
                      cursor: 'pointer',
                      // 已處理：原地變灰保留，按重新整理才移除。
                      opacity: isHandled ? 0.5 : 1,
                    }}
                  >
                    <TableCell>
                      {/*
                        兩行（`LIFECYCLE.md §8.2`）：
                        主行 案場名（粗體）＋ 狀態標記；次行 電號 · 計費期間（次要色小字）。
                        使用者回報「案場跟時間範圍黏在一起」就是因為前一版
                        兩者同行同字級、沒有層級。

                        案場名**不再是按鈕樣式** —— 列本身可點，
                        裡面再放一個看起來可點的東西只會讓人猶豫該點哪個。
                      */}
                      <Stack direction="row" alignItems="center" gap={0.75}>
                        <Typography
                          variant="body2"
                          data-testid="workbench-open"
                          sx={{ fontWeight: 600, fontSize: 14 }}
                        >
                          {r.subject?.stationName || r.subject?.customerNumber || `#${r.executionId}`}
                        </Typography>
                        {isHandled && (
                          <Chip
                            size="small" variant="outlined" data-testid="row-handled"
                            label={translate('energy.myWork.handled')}
                          />
                        )}
                        {!isHandled && r.claimedByMe && (
                          <Chip
                            size="small" variant="outlined" data-testid="row-claimed"
                            label={translate('energy.myWork.claimedByMe')}
                            sx={{ color: jvTokens.primary, borderColor: jvTokens.primary }}
                          />
                        )}
                      </Stack>
                      <Typography
                        variant="caption"
                        data-testid="workbench-subline"
                        sx={{
                          display: 'block',
                          fontSize: 11.5,
                          color: 'text.secondary',
                          fontVariantNumeric: 'tabular-nums',
                          whiteSpace: 'nowrap',
                          overflow: 'hidden',
                          textOverflow: 'ellipsis',
                        }}
                      >
                        {r.subject?.customerNumber ?? '—'}
                        {' · '}
                        <Period from={r.subject?.billingPeriodStart} to={r.subject?.billingPeriodEnd} />
                      </Typography>
                      {/* US-15：退回理由與退回者**直接印在列上**，
                          不收進 tooltip 或詳情頁 —— 不必點三層才知道要改什麼。 */}
                      {r.rejectReason && (
                        <Box
                          data-testid="workbench-reject"
                          sx={{
                            mt: 0.5, px: 1, py: 0.5, borderLeft: `3px solid ${jvTokens.warn}`,
                            background: jvTokens.hoverBg, borderRadius: 0.5,
                          }}
                        >
                          <Typography variant="caption" sx={{ color: jvTokens.warn, fontWeight: 600 }}>
                            {translate('energy.workbench.rejectedBy', { name: r.rejectedBy ?? '—' })}
                          </Typography>
                          <Typography variant="caption" display="block">{r.rejectReason}</Typography>
                        </Box>
                      )}
                    </TableCell>
                    <TableCell>
                      <Typography variant="body2">{r.subject?.landownerName ?? '—'}</Typography>
                    </TableCell>
                    <TableCell align="right"><Money value={r.subject?.totalAmount} /></TableCell>
                    <TableCell>
                      <Typography variant="body2">{r.nodeName ?? '—'}</Typography>
                    </TableCell>
                    <TableCell>
                      <Typography
                        variant="body2"
                        sx={{ color: hot ? jvTokens.warn : 'text.secondary', fontWeight: hot ? 700 : 400 }}
                      >
                        {translate('energy.common.days', {
                          smart_count: r.waitingDays ?? 0, days: r.waitingDays ?? 0,
                        })}
                      </Typography>
                    </TableCell>
                    {/* 動作欄的點擊**不可以**冒泡去開抽屜 —— 按「核准」不是「我要看」。 */}
                    <TableCell align="right" onClick={(e) => e.stopPropagation()}>
                      <Stack direction="row" gap={1} justifyContent="flex-end" alignItems="center">
                        {/*
                          〔認領〕**已經移出列表**（`LIFECYCLE.md §8.5`）——
                          它是「開始處理」的第一步，屬於抽屜。
                          留在列上就會回到「認領了卻不知道去哪看」。

                          動作列留著：已經認領的人可以在列上直接裁斷，
                          不必每一筆都開抽屜。
                        */}
                        {!isHandled && (
                          <ActionBar
                            actions={r.availableActions}
                            blocked={r.actionBlocked}
                            onAction={(a) => onAction(r, a)}
                          />
                        )}
                      </Stack>
                    </TableCell>
                  </TableRow>
                );
              }) : <EmptyRow colSpan={6} loading={loading} />}
            </TableBody>
          </Table>
        </CardContent>
      </Card>
    );
  };

  return (
    <EnergyPageFrame orgId={orgId} denied={denied} loading={loading}>
      <Stack direction="row" alignItems="center" justifyContent="space-between" mb={2}>
        {embedded
          ? <span />
          : <Typography variant="h6">{translate('energy.workbench.title')}</Typography>}
        <Tooltip title={translate('ra.action.refresh')}>
          {/* 解說影片要指著它按：處理完的列會留在原位標「已處理」，
              按了這顆才會移走、也才會把**新輪到我的那一關**抓進來。 */}
          <IconButton data-testid="workbench-refresh" onClick={load} disabled={loading}>
            <RefreshIcon />
          </IconButton>
        </Tooltip>
      </Stack>

      {sections.map(renderSection)}
      {!sections.length && !loading && (
        <Alert severity="info">{translate('energy.workbench.nothing')}</Alert>
      )}

      {/*
        檢視在**抽屜**裡，不換頁（`LIFECYCLE.md §8.4`）。
        換頁 ＝ 離開佇列，而那一頁還有它自己的返回邏輯 ——
        使用者送出後就回到 `energyBillConsole`，佇列的脈絡整個斷掉。
      */}
      <BillDetailDrawer
        open={openTaskId != null}
        billId={openRow?.subject?.billId ?? null}
        onClose={() => setOpenTaskId(null)}
        queue={openRow ? {
          index: openIndex + 1,
          total: flatQueue.length,
          taskId: openRow.id,
          // 這個待辦屬於哪一條流程 —— 一張單可能同時開著好幾條。
          executionId: openRow.executionId,
          claimable: openRow.claimable,
          claimedByMe: openRow.claimedByMe,
          onNext: gotoNext,
          onClose: () => setOpenTaskId(null),
          onRowChanged: replaceRow,
        } : null}
      />

      <Dialog open={!!pending} onClose={() => setPending(null)} fullWidth maxWidth="xs">
        <DialogTitle>
          {pending ? translate(pending.action.labelKey, { _: pending.action.actionKey }) : ''}
        </DialogTitle>
        <DialogContent>
          <Box display="flex" flexDirection="column" gap={2} mt={1}>
            <Typography variant="caption" color="text.secondary">
              {translate('energy.myTasks.reasonRequired')}
            </Typography>
            <TextField
              autoFocus multiline minRows={3}
              label={translate('energy.myTasks.reason')}
              value={reason}
              onChange={(e) => setReason(e.target.value)}
            />
          </Box>
        </DialogContent>
        <DialogActions>
          <Button onClick={() => setPending(null)}>{translate('ra.action.cancel')}</Button>
          <Button
            variant="contained"
            disabled={!reason.trim()}
            onClick={() => pending && runAction(pending.row, pending.action, reason.trim())}
          >
            {translate('ra.action.confirm')}
          </Button>
        </DialogActions>
      </Dialog>
    </EnergyPageFrame>
  );
};

export default WorkbenchPage;
