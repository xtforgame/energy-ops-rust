import * as React from 'react';
import {
  Box, Card, CardContent, Typography, Table, TableHead, TableBody, TableRow, TableCell,
  Stack, Chip, IconButton, Button, Divider, Alert,
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
import { executionListRoute, executionRoute, stationRoute } from './routes';

/**
 * P5 地主整合頁 —— **「一個地主的所有錢與時間」**（`FRONTEND.md §2 P5`、US-13/21）。
 *
 * **獨立路由 `#/energyLandowners/:id`，不是 drawer**（2026-08-27 拍板）：
 * drawer 適合「看一眼就關掉」的次要資訊；地主頁是**入口頁**，
 * 使用者會從這裡出發去很多地方 —— 它需要完整的一頁**與可分享的網址**。
 *
 * **「現在正在發生」是最重要的一區**：他的單卡在哪、卡幾天。
 * 舊版只有欄位、看不到任何流程 —— 那正是被點名的核心缺失。
 *
 * 資料來自**一支端點**（`/landowners/:id/overview`）——
 * 不打五支自己拼，那會有五種 loading 狀態。
 */
const LandownerDetailPage: React.FC = () => {
  const notify = useNotify();
  const translate = useTranslate();
  const navigate = useNavigate();
  const { id } = useParams();
  const orgId = useSelection()?.organizationId;

  const [loading, setLoading] = React.useState(false);
  const [denied, setDenied] = React.useState(false);
  const [data, setData] = React.useState<any>(null);

  const base = energyBase(orgId);

  const load = React.useCallback(async () => {
    if (!orgId || !id) return;
    setLoading(true); setDenied(false);
    try {
      const { data: d } = await energyHttp.get(`${base}/landowners/${id}/overview`);
      setData(d);
    } catch (e: any) {
      if (e?.response?.status === 403) setDenied(true);
      else notify(apiErrorCode(e) || e?.message || 'load failed', { type: 'error' });
    } finally { setLoading(false); }
  }, [orgId, id, base, notify]);

  React.useEffect(() => { load(); }, [load]);

  const field = (labelKey: string, value: React.ReactNode) => (
    <Stack direction="row" gap={2} py={0.4}>
      <Typography variant="body2" color="text.secondary" sx={{ minWidth: 96 }}>
        {translate(labelKey)}
      </Typography>
      <Typography variant="body2" component="div">{value || '—'}</Typography>
    </Stack>
  );

  if (!data && !loading) {
    return (
      <EnergyPageFrame orgId={orgId} denied={denied} loading={loading}>
        <Alert severity="info">{translate('energy.landowners.notFound')}</Alert>
      </EnergyPageFrame>
    );
  }

  return (
    <EnergyPageFrame orgId={orgId} denied={denied} loading={loading}>
      {/* ---- 標頭 ---- */}
      <Stack direction="row" alignItems="center" justifyContent="space-between" mb={2}>
        <Stack direction="row" alignItems="center" gap={1}>
          <IconButton onClick={() => navigate('/energyLandownerConsole')}><ArrowBackIcon /></IconButton>
          <Typography variant="h6" data-testid="landowner-name">{data?.name}</Typography>
          <StatusChip i18nKey="energy.landownerStatus" value={data?.status} />
        </Stack>
        <Stack direction="row" gap={1}>
          {/* 跨頁：看他的所有單。用路由不是 modal。
              **一定要帶 `landownerId`** —— 導到一張沒篩選的清單
              等於叫使用者自己再找一次，那個按鈕就是在騙人。 */}
          <Button
            size="small"
            data-testid="landowner-all-bills"
            onClick={() => navigate(executionListRoute({ status: 'running', landownerId: id }))}
          >
            {translate('energy.landowners.allBills')}
          </Button>
          <IconButton onClick={load} disabled={loading}><RefreshIcon /></IconButton>
        </Stack>
      </Stack>

      <Card sx={{ mb: 2 }}>
        <CardContent>
          <Stack direction={{ xs: 'column', md: 'row' }} gap={4}>
            <Box>
              {field('energy.landowners.taxId', data?.taxId)}
              {field('energy.landowners.contact', [data?.contactName, data?.contactPhone, data?.contactEmail]
                .filter(Boolean).join('　'))}
            </Box>
            <Box>
              {field('energy.landowners.bank', [data?.bankName, data?.bankBranch, data?.bankAccountName]
                .filter(Boolean).join('　'))}
              {field('energy.landowners.bankAccount', data?.bankAccount)}
            </Box>
          </Stack>
        </CardContent>
      </Card>

      <Box sx={{ display: { xs: 'block', md: 'grid' }, gridTemplateColumns: '1fr 1fr', gap: 2 }}>
        {/* ---- 左：案場 ＋ 合約 ---- */}
        <Card>
          <CardContent>
            <Typography variant="subtitle1" gutterBottom>
              {translate('energy.landowners.tabStations')}（{data?.stations?.length ?? 0}）
            </Typography>
            <Table size="small">
              <TableBody>
                {(data?.stations ?? []).length ? data.stations.map((s: any) => (
                  <TableRow key={s.id} hover data-testid="landowner-station">
                    <TableCell>
                      {/* 跨頁：地主 → 案場詳情（P11）。 */}
                      <Button
                        size="small"
                        sx={{ p: 0, minWidth: 0, justifyContent: 'flex-start' }}
                        data-testid="landowner-open-station"
                        onClick={() => navigate(stationRoute(s.id))}
                      >
                        {s.name}
                      </Button>
                      <Typography variant="caption" color="text.secondary" display="block">
                        {s.customerNumber}
                      </Typography>
                      {/* 「適用哪版流程／哪版策略」—— 舊版完全看不到。 */}
                      <Stack direction="row" gap={0.5} mt={0.5} flexWrap="wrap">
                        {/* chip **本身可點** → 案場的兩條效期軸（`FRONTEND.md §10 P5`：
                            「顯示適用的流程版本與策略，**且可點**」）。
                            只印文字＝使用者看到版本卻走不過去。 */}
                        {s.workflow && (
                          <Chip
                            size="small" variant="outlined" clickable data-testid="landowner-station-workflow"
                            label={`${s.workflow.name} v${s.workflow.version}`}
                            onClick={() => navigate(stationRoute(s.id))}
                            sx={{ color: jvTokens.primary, borderColor: jvTokens.primary }}
                          />
                        )}
                        {s.strategy && (
                          <Chip
                            size="small" variant="outlined" clickable data-testid="landowner-station-strategy"
                            label={s.strategy.name}
                            onClick={() => navigate(stationRoute(s.id))}
                            sx={{ color: jvTokens.caption, borderColor: jvTokens.border }}
                          />
                        )}
                        {/* `FRONTEND.md §2.85.1`：**未綁定不是「沒設定」，是「收不了單」**。
                            不可以只留白或印「—」—— 那看起來像資料還沒填，
                            但實際上這個案場送進來的單**會被退回**。 */}
                        {!s.workflow && (
                          <Chip
                            size="small" variant="outlined" clickable
                            data-testid="landowner-station-unbound"
                            label={translate('energy.stations.unboundShort')}
                            onClick={() => navigate(stationRoute(s.id))}
                            sx={{ color: jvTokens.warn, borderColor: jvTokens.warn, fontWeight: 600 }}
                          />
                        )}
                      </Stack>
                    </TableCell>
                    <TableCell align="right">
                      <StatusChip i18nKey="energy.stationStatus" value={s.status} />
                    </TableCell>
                  </TableRow>
                )) : <EmptyRow colSpan={2} />}
              </TableBody>
            </Table>

            <Divider sx={{ my: 2 }} />
            <Typography variant="subtitle1" gutterBottom>
              {translate('energy.landowners.tabContracts')}（{data?.contracts?.length ?? 0}）
            </Typography>
            <Table size="small">
              <TableBody>
                {(data?.contracts ?? []).length ? data.contracts.map((c: any) => (
                  <TableRow key={c.id}>
                    <TableCell>
                      <Typography variant="body2">{c.contractNo}</Typography>
                      <Period from={c.validFrom} to={c.validTo} />
                    </TableCell>
                    <TableCell align="right">
                      <StatusChip i18nKey="energy.contractStage" value={c.stage} />
                      {c.stage === 'construction' && c.installmentTotal ? (
                        <Typography variant="caption" color="text.secondary" display="block">
                          {translate('energy.contracts.installmentProgress', {
                            paid: c.installmentPaid, total: c.installmentTotal,
                          })}
                        </Typography>
                      ) : null}
                    </TableCell>
                  </TableRow>
                )) : <EmptyRow colSpan={2} />}
              </TableBody>
            </Table>
          </CardContent>
        </Card>

        {/* ---- 右：現在正在發生 ＋ 近期應付 ＋ 近期事件 ---- */}
        <Box>
          <Card sx={{ mb: 2 }}>
            <CardContent>
              <Typography variant="subtitle1" gutterBottom>
                {translate('energy.landowners.inFlight')}（{data?.inFlight?.length ?? 0}）
              </Typography>
              <Typography variant="caption" color="text.secondary" display="block" mb={1}>
                {translate('energy.landowners.inFlightHint')}
              </Typography>
              <Table size="small">
                <TableHead>
                  <TableRow>
                    <TableCell>{translate('energy.bills.station')}</TableCell>
                    <TableCell align="right">{translate('energy.bills.amount')}</TableCell>
                    <TableCell>{translate('energy.myTasks.node')}</TableCell>
                    <TableCell>{translate('energy.myTasks.waiting')}</TableCell>
                  </TableRow>
                </TableHead>
                <TableBody>
                  {(data?.inFlight ?? []).length ? data.inFlight.map((f: any) => (
                    <TableRow key={f.executionId} hover data-testid="landowner-inflight">
                      <TableCell>
                        {/* 這一區列的是 **execution**（「現在正在發生」）——
                            所以點下去去 **P12 流程單詳情**，不是它底下那張 bill。
                            前一版 `f.billId ? ... : undefined`：
                            保證金那類**點了沒反應**。 */}
                        <Button
                          size="small"
                          sx={{ p: 0, minWidth: 0, justifyContent: 'flex-start' }}
                          data-testid="landowner-open-bill"
                          onClick={() => (f.executionId
                            ? navigate(executionRoute(f.executionId)) : undefined)}
                        >
                          {f.stationName ?? `#${f.executionId}`}
                        </Button>
                      </TableCell>
                      <TableCell align="right"><Money value={f.totalAmount} /></TableCell>
                      <TableCell>
                        {/* 印節點名不印 nodeKey。 */}
                        <Typography variant="body2">{f.nodeName ?? '—'}</Typography>
                      </TableCell>
                      <TableCell>
                        {/* 卡太久用**燒橘**（不是紅）。門檻由後端回。 */}
                        <Typography
                          variant="body2"
                          data-stuck={f.stuck ? '1' : '0'}
                          sx={{ color: f.stuck ? jvTokens.warn : 'text.secondary', fontWeight: f.stuck ? 700 : 400 }}
                        >
                          {translate('energy.common.days', {
                            smart_count: f.waitingDays ?? 0, days: f.waitingDays ?? 0,
                          })}
                        </Typography>
                      </TableCell>
                    </TableRow>
                  )) : <EmptyRow colSpan={4} />}
                </TableBody>
              </Table>
            </CardContent>
          </Card>

          <Card sx={{ mb: 2 }}>
            <CardContent>
              <Typography variant="subtitle1" gutterBottom>
                {translate('energy.landowners.tabBills')}
              </Typography>
              <Table size="small">
                <TableBody>
                  {(data?.recentBills ?? []).length ? data.recentBills.map((b: any) => (
                    <TableRow key={b.id} hover>
                      <TableCell>
                        <Period from={b.billingPeriodStart} to={b.billingPeriodEnd} />
                      </TableCell>
                      <TableCell align="right"><Money value={b.totalAmount} /></TableCell>
                      <TableCell align="right">
                        {/* 沒算出來是 `—`，不顯示 0。 */}
                        <Money value={b.payoutAmount} />
                      </TableCell>
                    </TableRow>
                  )) : <EmptyRow colSpan={3} />}
                </TableBody>
              </Table>
            </CardContent>
          </Card>

          <Card>
            <CardContent>
              <Typography variant="subtitle1" gutterBottom>
                {translate('energy.landowners.tabEvents')}
              </Typography>
              <Table size="small">
                <TableBody>
                  {(data?.calendarEvents ?? []).length ? data.calendarEvents.map((ev: any) => (
                    <TableRow key={ev.id}>
                      <TableCell sx={{ whiteSpace: 'nowrap' }}>{String(ev.eventDate).slice(0, 10)}</TableCell>
                      <TableCell>{ev.title}</TableCell>
                      <TableCell align="right">
                        <StatusChip i18nKey="energy.eventStatus" value={ev.status} />
                      </TableCell>
                    </TableRow>
                  )) : <EmptyRow colSpan={3} />}
                </TableBody>
              </Table>
            </CardContent>
          </Card>
        </Box>
      </Box>
    </EnergyPageFrame>
  );
};

export default LandownerDetailPage;
