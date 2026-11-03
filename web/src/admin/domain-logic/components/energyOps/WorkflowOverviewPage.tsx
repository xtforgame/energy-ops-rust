import * as React from 'react';
import {
  Box, Card, CardContent, Typography, Table, TableHead, TableBody, TableRow, TableCell,
  Stack, Chip, IconButton, Drawer, Divider, Button, Alert,
} from '@mui/material';
import RefreshIcon from '@mui/icons-material/Refresh';
import CloseIcon from '@mui/icons-material/Close';
import { energyHttp, energyBase } from '@/api/energyLegacy';
import { useNotify, useTranslate } from 'react-admin';
import { useNavigate } from 'react-router-dom';
import { useSelection } from '@/platform/selection/store';
import { apiErrorCode } from '../../../dataProvider/apiError';
import {
  EnergyPageFrame, EmptyRow, Money, WaitingDays, jvTokens,
} from './shared';
import { executionListRoute, stationRoute, taskWorkRoute } from './routes';
import FlowGraph, { type ExecutionGraph } from './FlowGraph';
import ActionBar, { type TaskAction, type ActionBlocked } from './ActionBar';

interface DefinitionRow {
  id: number;
  key: string;
  name: string;
  version: number;
  parentVersion: number | null;
  status: string;
  runningCount: number;
  bindings: Array<{
    id: number; stationKey: string | null; stationId: number | null;
    /** 契約層的綁定沒有電號 —— 顯示契約編號。 */
    contractId?: number | null; contractNo?: string | null; docType?: string | null;
    validFrom: string; validTo: string | null;
  }>;
  nodes: Array<{ nodeKey: string; name: string; nodeType: string; waiting: number; stuck: number }>;
}

/**
 * P9 流程總覽 —— **管理員的家**（`FRONTEND.md §2 P9`、US-18）。
 *
 * **這一頁存在的理由是「沒有任何 execution 時也看得到流程圖」。**
 * `WORKFLOW.md §0.4` 當初把 react-flow 排除在設計外，導致流程圖
 * **只活在 execution 的脈絡裡** —— 沒有執行就永遠看不到流程長什麼樣，
 * 等於流程設定無法檢查。一個剛發佈、還沒人送過單的新版本，
 * 管理員必須也看得到它長什麼樣、綁了哪些案場。
 *
 * 每個節點標「現在積了幾張」—— 這把一張說明圖變成**營運儀表**。
 * 點節點 → 右側 drawer 列出卡在那一關的單，**可直接處理**
 * （`ActionBar` 的**第三處落點**，`FRONTEND.md §2.8.3`）。
 *
 * 動作按鈕一律來自後端的 `availableActions`／`actionBlocked` ——
 * 前端不自己判斷誰能按（那就是「按鈕會說謊」的來源）。
 */
const WorkflowOverviewPage: React.FC = () => {
  const notify = useNotify();
  const translate = useTranslate();
  const navigate = useNavigate();
  const orgId = useSelection()?.organizationId;

  const [loading, setLoading] = React.useState(false);
  const [denied, setDenied] = React.useState(false);
  const [defs, setDefs] = React.useState<DefinitionRow[]>([]);
  const [selected, setSelected] = React.useState<number | null>(null);
  const [graph, setGraph] = React.useState<ExecutionGraph | null>(null);
  /** 點開的節點（drawer）。 */
  const [openNode, setOpenNode] = React.useState<string | null>(null);
  const [nodeTasks, setNodeTasks] = React.useState<any[]>([]);
  const [nodeLoading, setNodeLoading] = React.useState(false);
  /** 尚未綁定的案場 —— 漏綁不會報錯，幾週後才變成一張退回的單。 */
  const [unbound, setUnbound] = React.useState<any[]>([]);

  const base = energyBase(orgId);

  const load = React.useCallback(async () => {
    if (!orgId) { setDefs([]); setDenied(false); return; }
    setLoading(true); setDenied(false);
    try {
      const { data } = await energyHttp.get(`${base}/workflows`);
      const rows: DefinitionRow[] = data?.definitions ?? [];
      setDefs(rows);
      // 預設選第一個 —— 不要讓使用者先點一次才看得到圖。
      setSelected((prev) => (prev && rows.some((r) => r.id === prev) ? prev : (rows[0]?.id ?? null)));
    } catch (e: any) {
      if (e?.response?.status === 403) setDenied(true);
      else notify(apiErrorCode(e) || e?.message || 'load failed', { type: 'error' });
    } finally { setLoading(false); }
  }, [orgId, base, notify]);

  React.useEffect(() => { load(); }, [load]);

  React.useEffect(() => {
    if (!orgId) { setUnbound([]); return; }
    let alive = true;
    energyHttp.get(`${base}/unbound-stations`)
      .then(({ data }) => { if (alive) setUnbound(Array.isArray(data) ? data : []); })
      .catch(() => { if (alive) setUnbound([]); });
    return () => { alive = false; };
  }, [orgId, base]);

  // 選定版本 → 抓**定義圖**（不是執行圖：這裡可能一個 execution 都沒有）。
  React.useEffect(() => {
    if (!orgId || !selected) { setGraph(null); return; }
    let alive = true;
    energyHttp.get(`${base}/workflows/${selected}/graph`)
      .then(({ data }) => { if (alive) setGraph(data); })
      .catch(() => { if (alive) setGraph(null); });
    return () => { alive = false; };
  }, [orgId, selected, base]);

  const openNodeDrawer = React.useCallback(async (nodeKey: string) => {
    if (!orgId || !selected) return;
    setOpenNode(nodeKey); setNodeTasks([]); setNodeLoading(true);
    try {
      const { data } = await energyHttp.get(`${base}/workflows/${selected}/nodes/${nodeKey}/tasks`);
      setNodeTasks(Array.isArray(data) ? data : []);
    } catch (e: any) {
      notify(apiErrorCode(e) || 'load failed', { type: 'error' });
    } finally { setNodeLoading(false); }
  }, [orgId, selected, base, notify]);

  /**
   * 在 drawer 裡直接動作 —— 第三處落點。
   * 端點形狀與工作台**一模一樣**（`POST .../tasks/:id/actions`，body 帶 `actionKey`）——
   * 不要為這一頁另發明一條路徑。
   */
  const act = async (taskId: number, action: TaskAction) => {
    // 需要理由的動作（退回）在這裡先擋掉並導去校對台填 ——
    // 不在 drawer 裡再蓋一個對話框（`energy.workflows.needsReasonHint`）。
    if (action.requiresReason) {
      notify('energy.workflows.needsReasonHint', { type: 'info' });
      return;
    }
    try {
      await energyHttp.post(`${base}/tasks/${taskId}/actions`, { actionKey: action.actionKey });
      notify('energy.myTasks.actionDone', { type: 'success' });
      if (openNode) openNodeDrawer(openNode);
      load();
    } catch (e: any) { notify(apiErrorCode(e) || 'action failed', { type: 'error' }); }
  };

  const current = defs.find((d) => d.id === selected) ?? null;

  return (
    <EnergyPageFrame orgId={orgId} denied={denied} loading={loading}>
      <Stack direction="row" alignItems="center" justifyContent="space-between" mb={2}>
        <Typography variant="h6">{translate('energy.workflows.title')}</Typography>
        <IconButton onClick={load} disabled={loading}><RefreshIcon /></IconButton>
      </Stack>

      {/* ---- 尚未綁定的案場 —— **這一區是 P9 真正的價值** ----
          管理員的職責是「流程設定對嗎」，而最常見的設定錯誤就是**漏綁**。
          漏綁不會報錯，它只會在幾週後變成一張退回的單。 */}
      {unbound.length > 0 && (
        <Card sx={{ mb: 2, borderLeft: `4px solid ${jvTokens.warn}` }}>
          <CardContent>
            <Stack direction="row" alignItems="center" gap={1} mb={1}>
              <Typography variant="subtitle1" sx={{ color: jvTokens.warn, fontWeight: 700 }}>
                {translate('energy.workflows.unbound', { count: unbound.length })}
              </Typography>
            </Stack>
            <Typography variant="caption" color="text.secondary" display="block" mb={1}>
              {translate('energy.workflows.unboundHint')}
            </Typography>
            <Table size="small">
              <TableBody>
                {unbound.map((s: any) => (
                  <TableRow
                    key={s.id}
                    hover
                    sx={{ cursor: 'pointer' }}
                    data-testid="unbound-station"
                    data-station={s.id}
                    data-missing={[s.missingWorkflow ? 'workflow' : '', s.missingRating ? 'rating' : ''].filter(Boolean).join(',')}
                    // 點得進 P11 —— 那裡才是綁定動作的家。
                    onClick={() => navigate(stationRoute(s.id))}
                  >
                    <TableCell>
                      <Typography variant="body2" fontWeight={600}>{s.name}</Typography>
                      <Typography variant="caption" color="text.secondary" sx={{ fontVariantNumeric: 'tabular-nums' }}>
                        {s.customerNumber}
                      </Typography>
                    </TableCell>
                    <TableCell>
                      {/* 兩種 kind 分開講 —— 「有綁流程但沒綁策略」也是壞的（算不出錢）。 */}
                      <Stack direction="row" gap={0.5} flexWrap="wrap">
                        {s.missingWorkflow && (
                          <Chip
                            size="small" variant="outlined"
                            label={translate('energy.workflows.missingWorkflow')}
                            sx={{ color: jvTokens.warn, borderColor: jvTokens.warn, fontWeight: 600 }}
                          />
                        )}
                        {s.missingRating && (
                          <Chip
                            size="small" variant="outlined"
                            label={translate('energy.workflows.missingRating')}
                            sx={{ color: jvTokens.warn, borderColor: jvTokens.warn, fontWeight: 600 }}
                          />
                        )}
                      </Stack>
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          </CardContent>
        </Card>
      )}

      {/* ---- 上半：流程清單 ---- */}
      <Card sx={{ mb: 2 }}>
        <CardContent>
          <Typography variant="caption" color="text.secondary" display="block" mb={1}>
            {translate('energy.workflows.hint')}
          </Typography>
          <Table size="small">
            <TableHead>
              <TableRow>
                <TableCell>{translate('energy.workflows.flow')}</TableCell>
                <TableCell>{translate('energy.workflows.version')}</TableCell>
                <TableCell>{translate('energy.workflows.bindings')}</TableCell>
                <TableCell align="right">{translate('energy.workflows.running')}</TableCell>
              </TableRow>
            </TableHead>
            <TableBody>
              {defs.length ? defs.map((d) => (
                <TableRow
                  key={d.id}
                  hover
                  selected={d.id === selected}
                  sx={{ cursor: 'pointer' }}
                  data-testid="workflow-row"
                  data-definition={d.id}
                  onClick={() => { setSelected(d.id); setOpenNode(null); }}
                >
                  <TableCell>
                    <Typography variant="body2" fontWeight={600}>{d.name}</Typography>
                  </TableCell>
                  <TableCell>
                    <Stack direction="row" gap={0.5} alignItems="center">
                      <Chip size="small" label={`v${d.version}`} data-testid="workflow-version" />
                      {/* 譜系：v2 從 v1 分岔 —— 沒有它就看不出「換版」這件事發生過。 */}
                      {d.parentVersion != null && (
                        <Typography variant="caption" color="text.secondary" data-testid="workflow-lineage">
                          {translate('energy.workflows.forkedFrom', { version: d.parentVersion })}
                        </Typography>
                      )}
                    </Stack>
                  </TableCell>
                  <TableCell>
                    {/* 綁定認的是**電號**（陷阱 #27）——印 stationId 會全是空的。 */}
                    {d.bindings.length ? (
                      <Stack gap={0.25}>
                        {d.bindings.map((b) => (
                          <Typography
                            key={b.id}
                            variant="caption"
                            color="text.secondary"
                            data-testid="workflow-binding"
                            sx={{ fontVariantNumeric: 'tabular-nums' }}
                          >
                            {/* 契約層綁定顯示契約編號（它沒有電號）——
                                印 `#—` 的話看起來像資料壞了。 */}
                            {b.contractNo
                              ? `${b.contractNo}${b.docType ? ` · ${translate(`energy.docType.${b.docType}`, { _: b.docType })}` : ''}`
                              : (b.stationKey ?? `#${b.stationId ?? '—'}`)}
                            {'　'}
                            {String(b.validFrom).slice(0, 10)}
                            {' ~ '}
                            {b.validTo ? String(b.validTo).slice(0, 10) : translate('energy.stations.openEnded')}
                          </Typography>
                        ))}
                      </Stack>
                    ) : (
                      <Typography variant="caption" color="text.secondary">
                        {translate('energy.workflows.noBinding')}
                      </Typography>
                    )}
                  </TableCell>
                  <TableCell align="right" data-testid="workflow-running">{d.runningCount}</TableCell>
                </TableRow>
              )) : <EmptyRow colSpan={4} loading={loading} />}
            </TableBody>
          </Table>
        </CardContent>
      </Card>

      {/* ---- 下半：選定版本的流程圖 ---- */}
      <Card>
        <CardContent>
          <Stack direction="row" alignItems="center" gap={1} mb={1}>
            <Typography variant="subtitle1">
              {current ? `${current.name} v${current.version}` : translate('energy.workflows.pickOne')}
            </Typography>
            {/* 這句是這一頁的重點：**一個 execution 都沒有時，圖照樣在**。 */}
            {current?.runningCount === 0 && (
              <Chip
                size="small"
                variant="outlined"
                data-testid="workflow-no-execution"
                label={translate('energy.workflows.noExecutionYet')}
                sx={{ color: jvTokens.caption, borderColor: jvTokens.border }}
              />
            )}
          </Stack>
          {graph ? (
            <FlowGraph graph={graph} height={380} onNodeClick={openNodeDrawer} />
          ) : (
            <Alert severity="info">{translate('energy.workflows.pickOne')}</Alert>
          )}
          <Typography variant="caption" color="text.secondary" display="block" mt={1}>
            {translate('energy.workflows.clickNodeHint')}
          </Typography>
        </CardContent>
      </Card>

      {/* ---- 點節點 → drawer：卡在這一關的那幾張單 ---- */}
      <Drawer anchor="right" open={!!openNode} onClose={() => setOpenNode(null)}>
        <Box sx={{ width: { xs: 360, md: 560 }, p: 2 }} data-testid="node-drawer" data-node={openNode ?? ''}>
          <Stack direction="row" alignItems="center" justifyContent="space-between" mb={1}>
            <Typography variant="subtitle1">
              {current?.nodes.find((n) => n.nodeKey === openNode)?.name ?? openNode}
            </Typography>
            <Stack direction="row" gap={0.5} alignItems="center">
              {/* drawer 只列得下前幾張 —— 不能讓人以為「這一關就這幾件」。
                  「看全部」帶著**同樣的篩選**去 P2（`?workflowKey=&nodeKey=`）。 */}
              <Button
                size="small"
                data-testid="node-drawer-see-all"
                onClick={() => navigate(executionListRoute({
                  status: 'running', workflowKey: current?.key, nodeKey: openNode ?? undefined,
                }))}
              >
                {translate('energy.workflows.seeAll')}
              </Button>
              <IconButton size="small" onClick={() => setOpenNode(null)}><CloseIcon fontSize="small" /></IconButton>
            </Stack>
          </Stack>
          <Divider sx={{ mb: 1 }} />
          {nodeLoading && <Typography variant="caption" color="text.secondary">…</Typography>}
          {!nodeLoading && !nodeTasks.length && (
            <Typography variant="body2" color="text.secondary" data-testid="node-drawer-empty">
              {translate('energy.workflows.nodeEmpty')}
            </Typography>
          )}
          <Stack gap={1.5}>
            {nodeTasks.map((t: any) => (
              <Card key={t.id} variant="outlined" data-testid="node-task">
                <CardContent>
                  <Stack direction="row" justifyContent="space-between" alignItems="flex-start" gap={1}>
                    <Box>
                      {/* 跨頁：drawer → 校對台（不是 modal 疊 modal）。 */}
                      <Button
                        size="small"
                        sx={{ p: 0, minWidth: 0, justifyContent: 'flex-start' }}
                        data-testid="node-task-open"
                        // 同工作台：保證金那類原本點了沒反應（沒有 bill）。
                        onClick={() => {
                          const to = taskWorkRoute(t.subject, t.executionId);
                          if (to) navigate(to);
                        }}
                      >
                        {t.subject?.stationName || t.subject?.customerNumber || `#${t.executionId}`}
                      </Button>
                      <Typography variant="caption" color="text.secondary" display="block">
                        {t.subject?.landownerName ?? '—'}
                      </Typography>
                    </Box>
                    <Stack alignItems="flex-end">
                      <Money value={t.subject?.totalAmount} />
                      <WaitingDays days={t.waitingDays} />
                    </Stack>
                  </Stack>
                  {/* `ActionBar` 的**第三處落點**（`FRONTEND.md §2.8.3`）。
                      這裡不要自己寫一份按鈕邏輯 —— 三處必須是同一個元件。 */}
                  <Box mt={1}>
                    <ActionBar
                      actions={(t.availableActions ?? []) as TaskAction[]}
                      blocked={(t.actionBlocked ?? null) as ActionBlocked | null}
                      onAction={(a) => act(t.id, a)}
                    />
                  </Box>
                </CardContent>
              </Card>
            ))}
          </Stack>
        </Box>
      </Drawer>
    </EnergyPageFrame>
  );
};

export default WorkflowOverviewPage;
