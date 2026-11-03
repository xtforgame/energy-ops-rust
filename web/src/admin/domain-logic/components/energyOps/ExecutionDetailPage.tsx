import * as React from 'react';
import {
  Box, Card, CardContent, Typography, Table, TableHead, TableBody, TableRow, TableCell,
  Stack, Chip, IconButton, Button, Divider,
} from '@mui/material';
import ArrowBackIcon from '@mui/icons-material/ArrowBack';
import RefreshIcon from '@mui/icons-material/Refresh';
import { energyHttp, energyBase } from '@/api/energyLegacy';
import { useNotify, useTranslate } from 'react-admin';
import { useParams, useNavigate } from 'react-router-dom';
import { useSelection } from '@/platform/selection/store';
import { apiErrorCode } from '../../../dataProvider/apiError';
import {
  EnergyPageFrame, EmptyRow, Money, WaitingDays, jvTokens,
} from './shared';
import FlowGraph, { type ExecutionGraph } from './FlowGraph';
import { subjectRoute, stationRoute } from './routes';

/**
 * **P12 詳情：一筆流程單自己的家**（`FRONTEND.md §1.4`）。
 *
 * ### 這一頁是補上一個**缺件**，而那個缺件造成了一次真正的混用
 *
 * 前一版的 P12 **只有清單、沒有詳情**。於是「點一列要去哪」沒有答案，
 * 我就讓後端替每一列算一個 `subject.route`，整列點下去跳到**電費單校對台**。
 * 那有兩個錯，而且第二個很嚴重：
 *
 * 1. 後端**不該知道前端路由**（改個路由名要回頭改後端）。
 * 2. 它讓 P12 的每一列變成**電費單的捷徑** ——
 * 而保證金那類（沒有 bill）當時被導去**地主頁**，跟那筆流程根本沒關係。
 * 那不是「連結」，那是**把 P12 併進 P2**（`§1.4.5` 明文禁止：
 * 資源之間**用連結相通，不是合併**）。
 *
 * 使用者的原話：「**搞清楚每個資源不准任何混用**。」
 *
 * ### 這一頁回答什麼
 *
 * 「**這一筆流程單**現在在哪一關、等多久、走過哪些關卡、輪到誰。」
 *
 * 它是**引擎層**的頁：不印電號／發電度數（那是電費單的欄位，在 P2）。
 * 對象只以**一個具名連結**呈現 —— 「它的對象」，不是整頁變成單據頁。
 */

interface TaskRow {
  id: number; seq: number; nodeKey: string; nodeName: string | null;
  nodeType: string; state: string; outcome: string | null; reason: string | null;
  assigneeName: string | null; createdAt: string | null; completedAt: string | null;
}
interface ExecutionDetail {
  id: number;
  /** 對外單號（純顯示，路由走 id）。 */
  displayId: string;
  status: string;
  currentNodeKey: string | null;
  subjectType: string;
  subjectId: string;
  businessDate: string | null;
  workflowKey: string | null;
  workflowName: string | null;
  definitionId: number | null;
  version: number | null;
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
  } | null;
  tasks: TaskRow[];
}

const ExecutionDetailPage: React.FC = () => {
  const { id } = useParams();
  const notify = useNotify();
  const translate = useTranslate();
  const navigate = useNavigate();
  const orgId = useSelection()?.organizationId;

  const [loading, setLoading] = React.useState(false);
  const [denied, setDenied] = React.useState(false);
  const [data, setData] = React.useState<ExecutionDetail | null>(null);
  const [graph, setGraph] = React.useState<ExecutionGraph | null>(null);

  const base = energyBase(orgId);

  const load = React.useCallback(async () => {
    if (!orgId || !id) { setData(null); return; }
    setLoading(true); setDenied(false);
    try {
      const { data: d } = await energyHttp.get(`${base}/executions/${id}`);
      setData(d);
    } catch (e: any) {
      if (e?.response?.status === 403) setDenied(true);
      else notify(apiErrorCode(e) || e?.message || 'load failed', { type: 'error' });
    } finally { setLoading(false); }
    try {
      const { data: g } = await energyHttp.get(`${base}/executions/${id}/graph`);
      setGraph(g);
    } catch { setGraph(null); }
  }, [orgId, id, base, notify]);

  React.useEffect(() => { load(); }, [load]);

  /**
   * 「它的對象」—— `§1.4.5` 那條連結（**前端自己決定去哪**，
   * 不是後端塞一個 route 字串過來）。
   * 多型：電費單 → P3 校對台；保證金**沒有校對台** → 去地主頁的合約區。
   */
  const subjectLink = (): { label: string; to: string } | null => {
    const s = data?.subject;
    const to = subjectRoute(s);
    if (!s || !to) return null;
    return {
      label: translate(s.kind === 'energyInstallment'
        ? 'energy.executions.openContract' : 'energy.executions.openBill'),
      to,
    };
  };
  const link = subjectLink();

  const field = (labelKey: string, value: React.ReactNode) => (
    <Box sx={{ minWidth: 160 }}>
      <Typography variant="caption" color="text.secondary" display="block">
        {translate(labelKey)}
      </Typography>
      <Typography variant="body2" component="div">{value}</Typography>
    </Box>
  );

  return (
    <EnergyPageFrame orgId={orgId} denied={denied} loading={loading}>
      <Stack direction="row" alignItems="center" justifyContent="space-between" mb={2}>
        <Stack direction="row" alignItems="center" gap={1}>
          <IconButton onClick={() => navigate('/energyExecutionConsole')}><ArrowBackIcon /></IconButton>
          <Typography variant="h6" data-testid="exec-detail-title">
            {/* 單號放在最前面 —— 使用者要對外講的就是這個。 */}
            <Typography
              component="span" variant="h6"
              data-testid="exec-detail-display-id"
              sx={{ fontVariantNumeric: 'tabular-nums', mr: 1 }}
            >
              {data?.displayId || ''}
            </Typography>
            {data?.workflowName || translate('energy.executions.title')}
            {data?.version != null && (
              <Typography component="span" variant="caption" color="text.secondary" sx={{ ml: 0.5 }}>
                {`v${data.version}`}
              </Typography>
            )}
          </Typography>
        </Stack>
        <IconButton onClick={load} disabled={loading}><RefreshIcon /></IconButton>
      </Stack>

      <Card sx={{ mb: 2 }}>
        <CardContent>
          <Stack direction="row" gap={3} flexWrap="wrap" data-testid="exec-detail-head">
            {field('energy.executions.node', (
              <Stack direction="row" gap={0.5} alignItems="center">
                {/* 印名稱不印 key。 */}
                <span>{data?.nodeName || data?.currentNodeKey || '—'}</span>
                {data?.status === 'abortPending' && (
                  <Chip
                    size="small" variant="outlined"
                    data-testid="exec-detail-abort-pending"
                    label={translate('energy.executions.abortPending')}
                    sx={{ color: jvTokens.warn, borderColor: jvTokens.warn }}
                  />
                )}
              </Stack>
            ))}
            {field('energy.executions.waitingDays', <WaitingDays days={data?.waitingDays} />)}
            {field('energy.executions.amount', <Money value={data?.subject?.amount} />)}
            {field('energy.executions.startedBy', (
              <>
                {data?.startedByName || '—'}
                <Typography variant="caption" color="text.secondary" sx={{ ml: 0.5 }}>
                  {data?.startedAt ? String(data.startedAt).slice(0, 10) : ''}
                </Typography>
              </>
            ))}
            {field('energy.executions.businessDate', data?.businessDate
              ? String(data.businessDate).slice(0, 10) : '—')}
          </Stack>

          <Divider sx={{ my: 1.5 }} />

          {/* 兩條具名連結 —— `§1.4.5`：資源之間**用連結相通，不是合併**。 */}
          <Stack direction="row" gap={1} flexWrap="wrap">
            {link && (
              <Button
                size="small" variant="outlined"
                data-testid="exec-open-subject"
                onClick={() => navigate(link.to)}
              >
                {link.label}
              </Button>
            )}
            {/* 「它跑的是哪一版流程」→ P9 定義頁。 */}
            <Button
              size="small"
              data-testid="exec-open-definition"
              onClick={() => navigate('/energyWorkflowConsole')}
            >
              {translate('energy.executions.openDefinition')}
            </Button>
            {data?.subject?.stationId != null && (
              <Button
                size="small"
                data-testid="exec-open-station"
                onClick={() => navigate(stationRoute(data.subject!.stationId!))}
              >
                {translate('energy.executions.openStation')}
              </Button>
            )}
          </Stack>
        </CardContent>
      </Card>

      {/* 流程圖：這一筆現在走到哪（唯讀，動作在別的地方）。 */}
      {graph && (
        <Card sx={{ mb: 2 }}>
          <CardContent>
            <Typography variant="subtitle1" gutterBottom>
              {translate('energy.executions.graph')}
            </Typography>
            <FlowGraph graph={graph} height={340} />
          </CardContent>
        </Card>
      )}

      {/* 關卡歷程 —— 「走過哪些關、誰處理的、為什麼退回」。 */}
      <Card>
        <CardContent>
          <Typography variant="subtitle1" gutterBottom>
            {translate('energy.executions.timeline')}
          </Typography>
          <Box sx={{ overflowX: 'auto' }}>
            <Table size="small">
              <TableHead>
                <TableRow>
                  <TableCell>#</TableCell>
                  <TableCell>{translate('energy.executions.node')}</TableCell>
                  <TableCell>{translate('energy.executions.taskState')}</TableCell>
                  <TableCell>{translate('energy.executions.assignee')}</TableCell>
                  <TableCell>{translate('energy.executions.outcome')}</TableCell>
                  <TableCell>{translate('energy.myTasks.reason')}</TableCell>
                </TableRow>
              </TableHead>
              <TableBody>
                {(data?.tasks ?? []).length ? (data?.tasks ?? []).map((t) => (
                  <TableRow key={t.id} data-testid="exec-task-row" data-state={t.state}>
                    <TableCell>{t.seq}</TableCell>
                    <TableCell>{t.nodeName || t.nodeKey}</TableCell>
                    <TableCell>
                      <Chip
                        size="small"
                        variant="outlined"
                        label={translate(`energy.executions.state.${t.state}`, { _: t.state })}
                      />
                    </TableCell>
                    <TableCell>{t.assigneeName || '—'}</TableCell>
                    <TableCell>{t.outcome || '—'}</TableCell>
                    {/* 退回理由要看得見 —— 那是最常被問的一格。 */}
                    <TableCell>{t.reason || '—'}</TableCell>
                  </TableRow>
                )) : <EmptyRow colSpan={6} loading={loading} />}
              </TableBody>
            </Table>
          </Box>
        </CardContent>
      </Card>
    </EnergyPageFrame>
  );
};

export default ExecutionDetailPage;
