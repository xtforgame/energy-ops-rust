import * as React from 'react';
import {
  Box, Card, CardContent, Typography, Table, TableHead, TableBody, TableRow, TableCell,
  Stack, Chip, IconButton, Button, Alert,
} from '@mui/material';
import ArrowBackIcon from '@mui/icons-material/ArrowBack';
import RefreshIcon from '@mui/icons-material/Refresh';
import { energyHttp, energyBase } from '@/api/energyLegacy';
import { useNotify, useTranslate } from 'react-admin';
import { useParams, useNavigate } from 'react-router-dom';
import { useSelection } from '@/platform/selection/store';
import { apiErrorCode } from '../../../dataProvider/apiError';
import {
  EnergyPageFrame, EmptyRow, StatusChip, Money, Period, jvTokens,
} from './shared';
import { billRoute, executionListRoute, landownerRoute, myWorkRoute } from './routes';
// 綁定的三個具名動作（`FRONTEND.md §2.84`）——這一頁是動作的家。
import BindingActions, { type BindingKind } from './BindingActions';

interface AxisSeg {
  id: number;
  validFrom: string;
  validTo: string | null;
  /** 現在生效的那一段 —— **後端算的**，前端不比日期。 */
  active: boolean;
  name?: string | null;
  version?: number | null;
  type?: string | null;
  definitionId?: number | null;
  strategyId?: number | null;
}

/**
 * P11 案場詳情（`FRONTEND.md §2 P11`）。
 *
 * **它回答的問題**：這個案場**適用哪版流程、哪版策略**，最近幾張單怎麼了？
 *
 * **兩條效期軸就是這頁存在的理由**。
 * **必須看得出「現在生效的是哪一段」** ——
 * 光列一堆日期區間看不出重點（規格原文）。
 * 所以生效那段用實心主色 ＋「生效中」標籤，其餘淡化。
 *
 * `active` 由**後端**算（`stationOverview`）——
 * 前端自己比日期，比錯了會指向錯的版本而且完全不會報錯。
 */
const StationDetailPage: React.FC = () => {
  const notify = useNotify();
  const translate = useTranslate();
  const navigate = useNavigate();
  const { id } = useParams();
  const orgId = useSelection()?.organizationId;

  const [loading, setLoading] = React.useState(false);
  const [denied, setDenied] = React.useState(false);
  const [data, setData] = React.useState<any>(null);
  /** 綁定段 ＋ **後端說的** `canManage`（前端不自己推權限）。 */
  const [bindings, setBindings] = React.useState<any>(null);

  const base = energyBase(orgId);

  const load = React.useCallback(async () => {
    if (!orgId || !id) return;
    setLoading(true); setDenied(false);
    try {
      const { data: d } = await energyHttp.get(`${base}/stations/${id}/overview`);
      setData(d);
    } catch (e: any) {
      if (e?.response?.status === 403) setDenied(true);
      else notify(apiErrorCode(e) || e?.message || 'load failed', { type: 'error' });
    } finally { setLoading(false); }
  }, [orgId, id, base, notify]);

  /** 綁定段（含 `editable`：已結束的段唯讀）。 */
  const loadBindings = React.useCallback(async () => {
    if (!orgId || !id) return;
    try {
      const { data: b } = await energyHttp.get(`${base}/stations/${id}/bindings`);
      setBindings(b);
    } catch { setBindings(null); }
  }, [orgId, id, base]);

  React.useEffect(() => { load(); }, [load]);
  React.useEffect(() => { loadBindings(); }, [loadBindings]);

  /**
   * 一條效期軸。
   * 生效中的那一段**視覺上要跳出來** —— 這是這頁唯一真正重要的資訊。
   */
  const renderAxis = (titleKey: string, segs: AxisSeg[], testid: string, kind: BindingKind) => {
    // 綁定段用**綁定端點**回來的那一份（它帶 `editable`／`isCurrent`）——
    // 不要拿 overview 的 `active` 來推：那是兩份真值。
    const bSegs: any[] = bindings?.[kind] ?? [];
    const current = bSegs.find((x) => x.isCurrent) ?? null;
    /**
     * 綁定分「線」：一條線 ＝ (適用單別, 流程 key)。
     * 每條線各自有自己的現行段 —— 整個案場只算一段的話，
     * 第二條流程會看起來像過期的歷史，而它其實正在跑。
     */
    const laneMap = new Map<string, { docType: string | null; targetKey: string; current: any }>();
    bSegs.forEach((seg) => {
      const key = `${seg.docType ?? ''}|${seg.targetKey ?? ''}`;
      const prev = laneMap.get(key);
      laneMap.set(key, {
        docType: seg.docType ?? null,
        targetKey: String(seg.targetKey ?? ''),
        current: seg.isCurrent ? seg : (prev?.current ?? null),
      });
    });
    const lanes = [...laneMap.values()];
    return (
    <Card sx={{ mb: 2 }}>
      <CardContent>
        <Typography variant="subtitle1" gutterBottom>{translate(titleKey)}</Typography>
        {segs.length ? (
          <Stack direction="row" gap={1} flexWrap="wrap" data-testid={testid}>
            {segs.map((seg) => (
              <Box
                key={seg.id}
                data-testid="axis-segment"
                data-active={seg.active ? '1' : '0'}
                sx={{
                  flex: '1 1 200px',
                  minWidth: 200,
                  p: 1.25,
                  borderRadius: 1,
                  // 生效中＝實心主色；其餘淡化（不是同一種灰）。
                  border: `2px solid ${seg.active ? jvTokens.primary : jvTokens.border}`,
                  background: seg.active ? jvTokens.hoverBg : 'transparent',
                  opacity: seg.active ? 1 : 0.65,
                }}
              >
                <Stack direction="row" alignItems="center" gap={0.75} mb={0.5}>
                  <Typography
                    variant="body2"
                    sx={{ fontWeight: seg.active ? 700 : 500, color: seg.active ? jvTokens.primaryDark : 'text.secondary' }}
                  >
                    {seg.name ?? '—'}
                    {seg.version != null ? ` v${seg.version}` : ''}
                  </Typography>
                  {seg.active && (
                    <Chip
                      size="small"
                      data-testid="axis-active-badge"
                      label={translate('energy.stations.activeNow')}
                      sx={{ background: jvTokens.primary, color: '#fff', fontWeight: 600, height: 20 }}
                    />
                  )}
                </Stack>
                <Typography variant="caption" color="text.secondary" sx={{ fontVariantNumeric: 'tabular-nums' }}>
                  {String(seg.validFrom).slice(0, 10)}
                  {' ~ '}
                  {seg.validTo ? String(seg.validTo).slice(0, 10) : translate('energy.stations.openEnded')}
                </Typography>
                {/* 每一段都可點 → 流程版本（P9 是 C7c；先連到定義本身）。 */}
                {seg.definitionId != null && (
                  <Button
                    size="small"
                    sx={{ p: 0, minWidth: 0, mt: 0.5 }}
                    data-testid="axis-open-workflow"
                    onClick={() => navigate(myWorkRoute())}
                  >
                    {translate('energy.stations.viewWorkflow')}
                  </Button>
                )}
              </Box>
            ))}
          </Stack>
        ) : (
          /* `FRONTEND.md §2.85.1`：空軸不是留白 —— 要說出**後果**與**誰能修**。 */
          <Stack gap={0.5} data-testid="axis-unbound" data-axis={testid}>
            <Typography variant="body2" sx={{ color: jvTokens.warn, fontWeight: 600 }}>
              {translate('energy.stations.noBinding')}
            </Typography>
            <Typography variant="caption" color="text.secondary">
              {translate('energy.stations.noBindingHint')}
            </Typography>
            {/* 指出誰能綁：管理員在 P9 流程總覽。 */}
            <Button
              size="small"
              sx={{ p: 0, minWidth: 0, alignSelf: 'flex-start' }}
              data-testid="axis-open-workflows"
              onClick={() => navigate('/energyWorkflowConsole')}
            >
              {translate('energy.stations.gotoWorkflows')}
            </Button>
          </Stack>
        )}

        {/**
          * **一座案場現在可以綁多條流程**（躉購·有餘電要核可 ＋ 開發票）。
          *
          * 所以動作不是「這個案場的那一段綁定」，而是**每條線各一組**：
          * 一條線 ＝ (適用單別, 流程)。少了這個分組，換版與解除都只能猜
          * 要動哪一條 —— 而解除是 critical 動作。
          */}
        {kind === 'workflow' ? (
          <Stack gap={1} mt={1} data-testid="binding-lanes" data-count={String(lanes.length)}>
            {lanes.map((lane) => (
              <Stack
                key={`${lane.docType ?? ''}|${lane.targetKey}`}
                direction="row"
                alignItems="center"
                gap={1}
                flexWrap="wrap"
                data-testid="binding-lane"
                data-doc-type={lane.docType ?? ''}
                data-workflow={lane.targetKey}
              >
                <Chip
                  size="small"
                  data-testid="lane-doc-type"
                  label={translate(`energy.docType.${lane.docType ?? 'anyDocType'}`, { _: lane.docType ?? '' })}
                  sx={{ borderColor: jvTokens.border, color: jvTokens.caption }}
                  variant="outlined"
                />
                <Typography variant="body2" sx={{ fontWeight: 600 }}>
                  {lane.current
                    ? `${lane.current.name ?? lane.targetKey} v${lane.current.version}`
                    : translate('energy.stations.noBinding')}
                </Typography>
                <BindingActions
                  orgId={orgId}
                  stationId={Number(id)}
                  kind={kind}
                  docType={lane.docType}
                  targetKey={lane.targetKey}
                  hasCurrent={!!lane.current}
                  currentLabel={lane.current ? `${lane.current.name} v${lane.current.version}` : null}
                  canManage={!!bindings?.canManage?.[kind]}
                  onDone={() => { loadBindings(); load(); }}
                />
              </Stack>
            ))}
            {/* 綁**新的一條**：選單別 ＋ 選版本。既有的線在上面各自處理。 */}
            <BindingActions
              orgId={orgId}
              stationId={Number(id)}
              kind={kind}
              allowDocTypeSelect
              hasCurrent={false}
              canManage={!!bindings?.canManage?.[kind]}
              onDone={() => { loadBindings(); load(); }}
            />
          </Stack>
        ) : (
          /* 計價策略不分單別：一座案場的錢怎麼算，與收到哪一種單無關。 */
          <BindingActions
            orgId={orgId}
            stationId={Number(id)}
            kind={kind}
            hasCurrent={!!current}
            currentLabel={current ? `${current.name} v${current.version}` : null}
            canManage={!!bindings?.canManage?.[kind]}
            onDone={() => { loadBindings(); load(); }}
          />
        )}
      </CardContent>
    </Card>
    );
  };

  if (!data && !loading) {
    return (
      <EnergyPageFrame orgId={orgId} denied={denied} loading={loading}>
        <Alert severity="info">{translate('energy.stations.notFound')}</Alert>
      </EnergyPageFrame>
    );
  }

  return (
    <EnergyPageFrame orgId={orgId} denied={denied} loading={loading}>
      <Stack direction="row" alignItems="center" justifyContent="space-between" mb={2}>
        <Stack direction="row" alignItems="center" gap={1}>
          <IconButton onClick={() => navigate('/energyLandownerConsole')}><ArrowBackIcon /></IconButton>
          <Typography variant="h6" data-testid="station-name">{data?.name}</Typography>
          <StatusChip i18nKey="energy.stationStatus" value={data?.status} />
        </Stack>
        <IconButton onClick={load} disabled={loading}><RefreshIcon /></IconButton>
      </Stack>

      <Card sx={{ mb: 2 }}>
        <CardContent>
          <Stack direction="row" gap={4} flexWrap="wrap">
            <Box>
              <Typography variant="caption" color="text.secondary">
                {translate('energy.bills.customerNumber')}
              </Typography>
              <Typography variant="body2" sx={{ fontVariantNumeric: 'tabular-nums' }}>
                {data?.customerNumber}
              </Typography>
            </Box>
            <Box>
              <Typography variant="caption" color="text.secondary">
                {translate('energy.landowners.name')}
              </Typography>
              {/* 跨頁：案場 → 地主（P5）。 */}
              <Button
                size="small"
                sx={{ p: 0, minWidth: 0, display: 'block' }}
                data-testid="station-open-landowner"
                onClick={() => (data?.landowner
                  ? navigate(landownerRoute(data.landowner.id)) : undefined)}
              >
                {data?.landowner?.name ?? '—'}
              </Button>
            </Box>
            <Box>
              <Typography variant="caption" color="text.secondary">
                {translate('energy.dashboard.coords')}
              </Typography>
              <Typography variant="body2" sx={{ fontVariantNumeric: 'tabular-nums' }}>
                {data?.latitude && data?.longitude
                  ? `${Number(data.latitude).toFixed(4)}, ${Number(data.longitude).toFixed(4)}` : '—'}
              </Typography>
            </Box>
          </Stack>
        </CardContent>
      </Card>

      {/* 兩條效期軸 */}
      {renderAxis('energy.stations.workflowAxis', data?.workflowAxis ?? [], 'workflow-axis', 'workflow')}
      {/* **策略軸也要有** —— 綁定缺口有兩份，只補流程那一份等於只做一半。 */}
      {renderAxis('energy.stations.strategyAxis', data?.strategyAxis ?? [], 'strategy-axis', 'rating')}

      <Card>
        <CardContent>
          <Stack direction="row" alignItems="center" justifyContent="space-between" mb={1}>
            <Typography variant="subtitle1">
              {translate('energy.landowners.tabBills')}（{data?.recentBills?.length ?? 0}）
            </Typography>
            {/* 「最近的單」只是**最近的** —— 不能讓人以為這個案場就這幾張。
                「看全部」帶著 `stationId` 去 P2（那裡才看得到保證金那類）。 */}
            <Button
              size="small"
              data-testid="station-all-executions"
              onClick={() => navigate(executionListRoute({ status: 'running', stationId: id }))}
            >
              {translate('energy.workflows.seeAll')}
            </Button>
          </Stack>
          <Table size="small">
            <TableHead>
              <TableRow>
                <TableCell>{translate('energy.bills.period')}</TableCell>
                <TableCell align="right">{translate('energy.bills.amount')}</TableCell>
                <TableCell>{translate('energy.bills.reviewStatus')}</TableCell>
              </TableRow>
            </TableHead>
            <TableBody>
              {(data?.recentBills ?? []).length ? data.recentBills.map((b: any) => (
                <TableRow
                  key={b.id}
                  hover
                  sx={{ cursor: 'pointer' }}
                  data-testid="station-bill"
                  onClick={() => navigate(billRoute(b.id))}
                >
                  <TableCell><Period from={b.billingPeriodStart} to={b.billingPeriodEnd} /></TableCell>
                  <TableCell align="right"><Money value={b.totalAmount} /></TableCell>
                  <TableCell><StatusChip i18nKey="energy.reviewStatus" value={b.reviewStatus} /></TableCell>
                </TableRow>
              )) : <EmptyRow colSpan={3} />}
            </TableBody>
          </Table>
        </CardContent>
      </Card>
    </EnergyPageFrame>
  );
};

export default StationDetailPage;
