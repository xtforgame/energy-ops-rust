import * as React from 'react';
import {
  Card, CardContent, Typography, Table, TableHead, TableBody, TableRow, TableCell,
  Stack, Tooltip, IconButton, TextField, MenuItem, Chip, Tabs, Tab, TableSortLabel, Box, Alert,
} from '@mui/material';
import RefreshIcon from '@mui/icons-material/Refresh';
import { energyHttp, energyBase } from '@/api/energyLegacy';
import { useNotify, useTranslate } from 'react-admin';
import { useNavigate, useSearchParams } from 'react-router-dom';
import { useSelection } from '@/platform/selection/store';
import { apiErrorCode } from '../../../dataProvider/apiError';
import {
  EnergyPageFrame, EmptyRow, Money, Period, WaitingDays, jvTokens, useCanViewAllExecutions,
} from './shared';
import { executionRoute } from './routes';

/** 後端 `ExecutionListService` 回的一列。 */
export interface ExecutionRow {
  id: number | null;
  /**
   * 對外指認用的**單號**（`displayId.ts`）。
   * **純顯示** —— 連結一律走 `id`（`/energyExecutionConsole/:id/show`）。
   */
  displayId: string;
  status: string;
  workflowKey: string | null;
  workflowName: string | null;
  version: number | null;
  nodeKey: string | null;
  nodeName: string | null;
  waitingDays: number | null;
  startedAt: string | null;
  startedByName: string | null;
  subject: {
    kind: string;
    billId?: number | null;
    installmentId?: number | null;
    contractNo?: string | null;
    seq?: number | null;
    stationId?: number | null;
    stationName?: string | null;
    landownerId?: number | null;
    landownerName?: string | null;
    amount?: string | null;
    periodStart?: string | null;
    periodEnd?: string | null;
    dueDate?: string | null;
  };
}

/**
 * **P12 `energyExecutionConsole`：流程單（執行）**（`FRONTEND.md §1.4`、`§2 P12`）。
 *
 * **它只回答一個問題**：每一筆跑在流程上的單，現在在哪一關、等多久、輪到誰。
 *
 * ### 它是**工作流引擎層**，不是電費單清單
 *
 * 判準：**把「雲豹能源」換成任何一家公司，這張表還在嗎？**
 * 在 —— execution 不在乎自己跑的是什麼，`subjectType` 今天是 `energyBill`、
 * 明天是 `energyInstallment`。所以「類別」是**一欄**，不是兩頁。
 *
 * 而這正是它非存在不可的理由：保證金請款的 subject 是 `energyInstallment`，
 * **沒有 bill** —— **我們做了第二個流程，然後把它做成看不見的。**
 *
 * ### 頁籤只有兩個
 *
 * | 頁籤 | 是什麼 |
 * |---|---|
 * | 進行中 | `running` ＋ `abortPending`（待中斷核可也還在跑） |
 * | 已完成 | `completed` ＋ `aborted` ＋ `terminated`（中斷掉的不可以消失） |
 *
 * **這裡沒有「尚未送審」**。前一版有過，那是錯的：
 *
 * > **當一個清單需要「這東西不存在」的頁籤時，那個清單的主體就選錯了。**
 *
 * 「校對完但沒送審」是**電費單的狀態** —— 它的家是 P2 `energyBillConsole`，
 * 那裡每一列有一個「流程單」出口，沒有 execution 就標「尚未送審」並可直接送。
 *
 * 可視度照舊由後端過濾（`WORKFLOW.md §7.3`）——
 * **前端不做第二套過濾**。兩套規則遲早不一致，然後沒人知道誰該看到什麼。
 */

type TabKey = 'running' | 'completed';
const TABS: TabKey[] = ['running', 'completed'];

export interface ExecutionsPageProps {
  /**
   * **誰的單**（`LIFECYCLE.md §6.6.1`）。
   *
   * · 不給（P12「營運 › 流程單」）：有 `view-all` 就問 `all`，沒有就問 `mine`。
   * 這**不是**靜默降級 —— 降級發生在**伺服器**才危險
   * （使用者以為看到全部）。這裡是 client **明確**問自己看得到的那一份，
   * 而且畫面上那一句 `visibilityNote` 就是在講這件事。
   * 伺服器對 `scope=all` 沒有 `view-all` 一律 403，不降級。
   *
   * · `'mine'`（「我的工作 › 我參與過的」）：**永遠**問 `mine`，
   * 即使他有 `view-all` —— 那一格問的是「我碰過什麼」不是「我看得到什麼」。
   */
  scope?: 'mine';
  /** 內嵌在別的頁裡（`MyWorkConsolePage`）時不要再畫一層外框與標題。 */
  embedded?: boolean;
  /**
   * 有給 ＝ 容器想**自己**接手「打開這一筆」（「我的工作」用抽屜，`LIFECYCLE.md §8.7`）。
   * 回 `true` 表示已處理，本頁就不再導航。
   *
   * 只有**有 bill** 的那類接得住 —— 保證金沒有 bill，
   * 單據詳情這個元件對它無話可說，所以照舊導去 P12 執行詳情。
   * 「多型只做一半」在這個 feature 已經出現過三次，這裡不再犯第四次。
   */
  onOpenBill?: (billId: number) => boolean;
}

const ExecutionsPage: React.FC<ExecutionsPageProps> = ({ scope: forcedScope, embedded, onOpenBill }) => {
  const notify = useNotify();
  const translate = useTranslate();
  const navigate = useNavigate();
  const selected = useSelection();
  const orgId = selected?.organizationId;
  const canViewAll = useCanViewAllExecutions();
  const scope: 'mine' | 'all' = forcedScope ?? (canViewAll ? 'all' : 'mine');
  /** 「我參與過的」不分進行中／已完成 —— 它問的是「我碰過什麼」。 */
  const showTabs = scope !== 'mine' || !forcedScope;

  const [loading, setLoading] = React.useState(false);
  const [denied, setDenied] = React.useState(false);
  const [rows, setRows] = React.useState<ExecutionRow[]>([]);
  const [total, setTotal] = React.useState(0);

  /**
   * 篩選從**網址**吃初始值 —— 這一頁是四個地方的落點：
   * · P7 Dashboard「幾件卡關」→ `?status=running&waitingDaysMin=N`
   * · P9 節點 drawer「看全部」→ `?workflowKey=&nodeKey=`
   * · P5 地主「看他的所有單」→ `?landownerId=`
   * · P11 案場「最近的單」    → `?stationId=`
   * 沒有這個的話那些連結只會到一張沒篩選的清單 ——
   * 使用者還要自己再篩一次，那個數字就等於一張海報。
   * 只吃**初始值**：使用者接著自己改篩選時 不要把網址搶回去。
   */
  const [searchParams] = useSearchParams();
  const initial = React.useRef({
    status: (searchParams.get('status') as TabKey) || 'running',
    workflowKey: searchParams.get('workflowKey') ?? '',
    nodeKey: searchParams.get('nodeKey') ?? '',
    stationId: searchParams.get('stationId') ?? '',
    landownerId: searchParams.get('landownerId') ?? '',
    waitingDaysMin: searchParams.get('waitingDaysMin') ?? '',
    month: searchParams.get('month') ?? '',
    // P2 的「流程單」連結帶過來的：只看那一筆（不套 status）。
    executionId: searchParams.get('executionId') ?? '',
  }).current;

  const [tab, setTab] = React.useState<TabKey>(
    TABS.includes(initial.status) ? initial.status : 'running',
  );
  const [workflowKey, setWorkflowKey] = React.useState(initial.workflowKey);
  const [nodeKey, setNodeKey] = React.useState(initial.nodeKey);
  const [waitingDaysMin, setWaitingDaysMin] = React.useState(initial.waitingDaysMin);
  // 案場／地主由連結帶進來，頁面上不另外做兩個下拉（那是 P5/P11 的工作）。
  const [stationId] = React.useState(initial.stationId);
  const [landownerId] = React.useState(initial.landownerId);
  const [month] = React.useState(initial.month);
  const [executionId] = React.useState(initial.executionId);
  const [keyword, setKeyword] = React.useState('');
  const [sortDir, setSortDir] = React.useState<'asc' | 'desc'>('desc');

  /** 流程種類的選項 —— 從後端拿（不 hardcode「電費單/保證金」兩個字串）。 */
  const [flows, setFlows] = React.useState<{ key: string; name: string }[]>([]);

  const load = React.useCallback(async () => {
    if (!orgId) { setRows([]); setDenied(false); return; }
    setLoading(true); setDenied(false);
    try {
      const { data } = await energyHttp.get(`${energyBase(orgId)}/executions`, {
        params: {
          scope,
          // `scope=mine` 且是「我參與過的」那一格 → **不送 status**，
          // 讓後端回全部（含已完成）。送 `running` 的話那一格永遠看不到
          // 我核准完、已經跑完的單 —— 而那正是它最主要的用途。
          ...(forcedScope === 'mine' ? {} : { status: tab }),
          ...(workflowKey ? { workflowKey } : {}),
          ...(nodeKey ? { nodeKey } : {}),
          ...(stationId ? { stationId } : {}),
          ...(landownerId ? { landownerId } : {}),
          ...(waitingDaysMin ? { waitingDaysMin } : {}),
          ...(month ? { month } : {}),
          ...(executionId ? { executionId } : {}),
          sort: `waitingDays:${sortDir}`,
          pageSize: 200,
        },
      });
      setRows(Array.isArray(data?.rows) ? data.rows : []);
      setTotal(Number(data?.total ?? 0));
    } catch (e: any) {
      if (e?.response?.status === 403) setDenied(true);
      else notify(apiErrorCode(e) || e?.message || 'load failed', { type: 'error' });
    } finally {
      setLoading(false);
    }
  }, [orgId, tab, scope, forcedScope, workflowKey, nodeKey, stationId, landownerId, waitingDaysMin,
    month, executionId, sortDir, notify]);

  React.useEffect(() => { load(); }, [load]);

  React.useEffect(() => {
    if (!orgId) { setFlows([]); return; }
    // P9 那支回的是 `{ definitions, stuckDays }`，而且**同一個 key 有多個版本** ——
    // 這裡要的是「幾種流程」，所以按 key 收斂（不要列出 v1/v2 兩個選項）。
    energyHttp.get(`${energyBase(orgId)}/workflows`)
      .then(({ data }) => {
        const seen = new Map<string, string>();
        (Array.isArray(data?.definitions) ? data.definitions : []).forEach((d: any) => {
          if (d?.key && !seen.has(d.key)) seen.set(d.key, d.name ?? d.key);
        });
        setFlows([...seen].map(([key, name]) => ({ key, name })));
      })
      .catch(() => setFlows([]));
  }, [orgId]);

  // 關鍵字在前端過濾（清單有上限，不為了搜尋多開一支端點）。
  const kw = keyword.trim().toLowerCase();
  const visible = kw
    ? rows.filter((r) => [
      r.subject.stationName, r.subject.landownerName, r.subject.contractNo,
      r.workflowName, r.nodeName,
    ].filter(Boolean).join(' ').toLowerCase().includes(kw))
    : rows;

  /**
   * 一列點下去去 **execution 自己的詳情**（P12 的 show）。
   *
   * 前一版是跳到 `subject.route`（後端算好的電費單校對台）——
   * 那讓 P12 的每一列變成**電費單的捷徑**，
   * 而保證金那類（沒有 bill）被導去**地主頁**，跟那筆流程根本沒關係。
   * 「它的對象」是**詳情頁上的一個具名連結**，
   * 不是把整列變成對象的入口（`FRONTEND.md §1.4.5`：連結，不是合併）。
   */
  const open = (r: ExecutionRow) => {
    // 容器要自己開（抽屜）而且這一筆有 bill —— 交給它，不換頁。
    const billId = r.subject?.billId;
    if (onOpenBill && billId != null && onOpenBill(Number(billId))) return;
    if (r.id != null) navigate(executionRoute(r.id));
  };

  const body = (
    <>
      {!embedded && (
      <Stack direction="row" alignItems="center" justifyContent="space-between" mb={1}>
        <Typography variant="h6">{translate('energy.executions.title')}</Typography>
        <Tooltip title={translate('ra.action.refresh')}>
          <IconButton onClick={load} disabled={loading}><RefreshIcon /></IconButton>
        </Tooltip>
      </Stack>
      )}

      {/* 指名某一筆時把頁籤收起來 —— 不然使用者會以為「這一格只有一筆」，
          而其實是被 `executionId` 釘住了。 */}
      {!showTabs ? null : executionId ? (
        <Chip
          size="small" variant="outlined" sx={{ mb: 2 }}
          data-testid="exec-pinned"
          label={translate('energy.executions.pinned')}
        />
      ) : (
      /* 兩個頁籤 —— 這裡**沒有**「尚未送審」（那是電費單的狀態，在 P2）。 */
      <Tabs
        value={tab}
        onChange={(_, v) => setTab(v)}
        sx={{ mb: 2 }}
        data-testid="exec-tabs"
        data-active={tab}
      >
        {TABS.map((t) => (
          <Tab
            key={t}
            value={t}
            data-testid={`exec-tab-${t}`}
            label={translate(`energy.executions.tab.${t}`)}
          />
        ))}
      </Tabs>
      )}

      <Card>
        <CardContent>
          <Stack direction="row" gap={2} mb={2} flexWrap="wrap" alignItems="center">
            <TextField
              select size="small" sx={{ minWidth: 180 }}
              label={translate('energy.executions.flow')}
              value={workflowKey}
              onChange={(e) => { setWorkflowKey(e.target.value); setNodeKey(''); }}
              inputProps={{ 'data-testid': 'exec-flow' }}
            >
              <MenuItem value="">{translate('energy.common.all')}</MenuItem>
              {flows.map((f) => (
                <MenuItem key={f.key} value={f.key}>{f.name}</MenuItem>
              ))}
            </TextField>
            <TextField
              size="small" type="number" sx={{ minWidth: 140 }}
              label={translate('energy.executions.waitingDaysMin')}
              value={waitingDaysMin}
              onChange={(e) => setWaitingDaysMin(e.target.value)}
              inputProps={{ 'data-testid': 'exec-waiting-min', min: 0 }}
            />
            <TextField
              size="small" sx={{ minWidth: 220 }}
              label={translate('energy.executions.search')}
              value={keyword}
              onChange={(e) => setKeyword(e.target.value)}
              inputProps={{ 'data-testid': 'exec-search' }}
            />
            {/* 由連結帶進來的篩選要**看得見**，不然使用者會以為清單就這麼少。 */}
            {nodeKey && (
              <Chip
                size="small" variant="outlined" data-testid="exec-node-filter"
                label={translate('energy.executions.nodeFilter', { node: nodeKey })}
                onDelete={() => setNodeKey('')}
              />
            )}
            {stationId && (
              <Chip size="small" variant="outlined" data-testid="exec-station-filter"
                label={translate('energy.executions.stationFilter')} />
            )}
            {landownerId && (
              <Chip size="small" variant="outlined" data-testid="exec-landowner-filter"
                label={translate('energy.executions.landownerFilter')} />
            )}
          </Stack>

          <Stack direction="row" justifyContent="space-between" alignItems="center" mb={1}>
            <Typography variant="caption" color="text.secondary" data-testid="exec-total">
              {translate('energy.executions.count', { count: total })}
            </Typography>
            {/* 「你現在看到的是哪一份」要**寫在畫面上** ——
                看到的比實際少而不自知，是這一頁最危險的失效方式。 */}
            <Typography variant="caption" color="text.secondary" data-testid="exec-scope-note" data-scope={scope}>
              {translate(scope === 'mine'
                ? 'energy.executions.scopeMine'
                : 'energy.executions.scopeAll')}
            </Typography>
          </Stack>

          <Box sx={{ overflowX: 'auto' }}>
            {/* 整張表也給一個 testid：標註要一次指「這幾筆」時，得指得到整份清單。 */}
            <Table size="small" data-testid="exec-table">
              <TableHead>
                <TableRow>
                  {/* 單號在第一欄 —— 清單的第一欄就是「這一列叫什麼」。 */}
                  <TableCell>{translate('energy.executions.displayId')}</TableCell>
                  {/* 「類別」是一欄 —— 保證金與電費單在**同一張表**上。 */}
                  <TableCell>{translate('energy.executions.kind')}</TableCell>
                  <TableCell>{translate('energy.executions.subject')}</TableCell>
                  <TableCell>{translate('energy.executions.station')}</TableCell>
                  <TableCell>{translate('energy.executions.landowner')}</TableCell>
                  <TableCell align="right">{translate('energy.executions.amount')}</TableCell>
                  <TableCell>{translate('energy.executions.node')}</TableCell>
                  <TableCell align="right">
                    {/* 工作流的痛點永遠是「誰卡住了」—— 所以這一欄可排序。 */}
                    <TableSortLabel
                      active
                      direction={sortDir}
                      onClick={() => setSortDir(sortDir === 'desc' ? 'asc' : 'desc')}
                      data-testid="exec-sort-waiting"
                    >
                      {translate('energy.executions.waitingDays')}
                    </TableSortLabel>
                  </TableCell>
                  <TableCell>{translate('energy.executions.startedBy')}</TableCell>
                </TableRow>
              </TableHead>
              <TableBody>
                {visible.length ? visible.map((r) => (
                  <TableRow
                    key={`${r.id ?? 'u'}-${r.subject.billId ?? r.subject.installmentId}`}
                    hover
                    data-testid="exec-row"
                    data-kind={r.subject.kind}
                    data-status={r.status}
                    // 排序驗證要看**數列本身**，不是「順序有沒有變」——
                    // 全部同一天送進來時值會相等，「順序沒變」是**正確**的。
                    data-waiting={r.waitingDays ?? ''}
                    // 讓斷言**指名檢查這一欄**（不要去掃整列的 innerText：
                    // 「等了幾天」那欄空的時候也是 `—`，會誤判成案場沒填）。
                    data-station-name={r.subject?.stationName ?? ''}
                    sx={{ cursor: r.id != null ? 'pointer' : 'default' }}
                    onClick={() => open(r)}
                  >
                    <TableCell>
                      <Typography variant="body2" sx={{ fontVariantNumeric: 'tabular-nums' }}>
                        {r.displayId || '—'}
                      </Typography>
                    </TableCell>
                    <TableCell>
                      <Stack direction="row" gap={0.5} alignItems="center">
                        {/* 印名稱不印 key —— 畫面上不出現 `deposit-request` 這種字串。 */}
                        <Typography variant="body2">
                          {r.workflowName || translate(`energy.executions.kindOf.${r.subject.kind}`)}
                        </Typography>
                        {/* 版本可點到 P9 —— 「它為什麼走這條路」的答案在那裡。
                            `stopPropagation`：不然會被整列的 onClick 蓋掉，
                            點版本卻跳去校對台。 */}
                        {r.version != null && (
                          <Typography
                            variant="caption"
                            data-testid="exec-version"
                            sx={{ color: jvTokens.primary, cursor: 'pointer', textDecoration: 'underline' }}
                            onClick={(e) => { e.stopPropagation(); navigate('/energyWorkflowConsole'); }}
                          >
                            {`v${r.version}`}
                          </Typography>
                        )}
                        {/* 待中斷核可：它還在跑，但已經有人要求停 —— 用燒橘點出來。 */}
                        {r.status === 'abortPending' && (
                          <Chip
                            size="small" variant="outlined"
                            data-testid="exec-abort-pending"
                            label={translate('energy.executions.abortPending')}
                            sx={{ color: jvTokens.warn, borderColor: jvTokens.warn }}
                          />
                        )}
                      </Stack>
                    </TableCell>
                    <TableCell>
                      {r.subject.kind === 'energyInstallment' ? (
                        <Typography variant="body2">
                          {translate('energy.executions.installmentOf', {
                            contract: r.subject.contractNo ?? '—', seq: r.subject.seq ?? '—',
                          })}
                        </Typography>
                      ) : (
                        <Period from={r.subject.periodStart} to={r.subject.periodEnd} />
                      )}
                    </TableCell>
                    <TableCell>
                      <Typography variant="body2">{r.subject.stationName || '—'}</Typography>
                    </TableCell>
                    <TableCell>
                      <Typography variant="body2">{r.subject.landownerName || '—'}</Typography>
                    </TableCell>
                    <TableCell align="right"><Money value={r.subject.amount} /></TableCell>
                    <TableCell>
                      <Typography variant="body2">{r.nodeName || r.nodeKey || '—'}</Typography>
                    </TableCell>
                    <TableCell align="right"><WaitingDays days={r.waitingDays} /></TableCell>
                    <TableCell>
                      <Typography variant="body2">{r.startedByName || '—'}</Typography>
                      <Typography variant="caption" color="text.secondary">
                        {r.startedAt ? String(r.startedAt).slice(0, 10) : ''}
                      </Typography>
                    </TableCell>
                  </TableRow>
                )) : <EmptyRow colSpan={9} loading={loading} />}
              </TableBody>
            </Table>
          </Box>

        </CardContent>
      </Card>
    </>
  );

  if (embedded) {
    // 內嵌時外框由呼叫端畫，但**「沒權限」不可以變成一片空白** ——
    // 空白跟壞掉長得一樣。
    if (denied) return <Alert severity="warning">{translate('energy.common.noAccess')}</Alert>;
    return body;
  }
  return (
    <EnergyPageFrame orgId={orgId} denied={denied} loading={loading}>
      {body}
    </EnergyPageFrame>
  );
};

export default ExecutionsPage;
