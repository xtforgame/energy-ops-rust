import * as React from 'react';
import {
  Card, CardContent, Typography, Table, TableHead, TableBody, TableRow, TableCell,
  Stack, Tooltip, IconButton, TextField, MenuItem, Chip, Button, Box, Tabs, Tab, Alert,
} from '@mui/material';
import RefreshIcon from '@mui/icons-material/Refresh';
import { energyHttp, energyBase } from '@/api/energy';
import { useNotify, useTranslate } from 'react-admin';
import { useNavigate, useSearchParams } from 'react-router-dom';
import { useSelection } from '@/platform/selection/store';
import { apiErrorCode } from '../../../dataProvider/apiError';
import {
  EnergyPageFrame, EmptyRow, StatusChip, Money, Period, jvTokens,
} from './shared';
import { billRoute, executionRoute } from './routes';

export interface BillRow {
  id: number;
  stationId: number | null;
  customerNumberRaw: string | null;
  ocrExtracted: Record<string, any>;
  billingPeriodStart: string | null;
  billingPeriodEnd: string | null;
  totalAmount: string | null;
  generationKwh: string | null;
  issuedAt: string | null;
  reviewStatus: string;
  changedFields: string[];
  /**
   * 這張單的流程單 id —— `null` ＝ **尚未送審**。
   * 這是後端帶過來的**唯一一個**工作流欄位（`FRONTEND.md §1.4.4` 第 1 條明文允許）：
   * 關卡、等待天數、流程版本**都不在這裡**，它們的家是 P12。
   */
  executionId: number | null;
}

/** 校對狀態的可選值 —— 與後端 `energyBill.reviewStatus` 的 enum 對齊。 */
const REVIEW_STATUSES = ['pendingMatch', 'pendingContract', 'pendingReview', 'reviewed'];

/**
 * `2026-02` → `2026-03-01`（**下個月一號**）。
 * 不要自己算「該月最後一天」——2 月沒有 31 號，湊出來的日期會讓後端 400。
 */
const monthEndExclusive = (month: string) => {
  const [y, m] = month.split('-').map(Number);
  const d = new Date(Date.UTC(y, m, 1));
  return d.toISOString().slice(0, 10);
};

/**
 * **P2 `energyBillConsole`：電費單。就只是電費單。**（`FRONTEND.md §1.4`、`§2 P2`）
 *
 * **它只回答一個問題**：這個組織有哪些電費單，辨識與校對的狀況如何。
 *
 * ### 這一頁被改錯過一次，留著當教訓
 *
 * 前一版把它改成「列 execution」。那是錯的 —— 它把**領域資料**（電號、
 * 計費期間、發電度數）與**工作流狀態**（關卡、等了幾天）混成一團。
 * 使用者的原話：「**搞清楚每個資源不准任何混用**。」
 *
 * 分層判準：**把「雲豹能源」換成任何一家公司，這張表還在嗎？**
 * 不在 —— 電號與發電度數只有雲豹需要。所以這裡是**領域層**（② 層），
 * 而 execution 是**引擎層**（① 層，P12 `energyExecutionConsole`）。
 *
 * ### P2 不做的事（每一條都有它自己的家）
 *
 * | 不放在這裡 | 它該在哪 |
 * |---|---|
 * | 目前卡在哪一關、等了幾天 | **P12 `energyExecutionConsole`** |
 * | 流程版本、節點、動作 | P12 / P9 |
 * | 誰在什麼時候上傳的、辨識原檔 | **`energyIngestConsole`** |
 * | 保證金請款單 | **它根本不是電費單** → P12 |
 * | 「進行中／已完成」頁籤 | 那是 execution 的狀態，不是電費單的 |
 *
 * **唯一的流程出口**：每一列一個「流程單」連結 ——
 * 有 execution → 連到 **P12 詳情**；沒有 → 標「**尚未送審**」，**可直接送**。
 * 那一格屬於這裡，因為「校對完卻沒送出」是**電費單的狀態**。
 *
 * ### 例外件是這一頁的**一個頁籤**，不是一個資源（`LIFECYCLE.md §7.4`）
 *
 * `energyExceptionConsole` 列的是 `energyBill WHERE reviewStatus='pendingMatch'` ——
 * **那是這一頁的一個篩選**。獨立成一頁有兩個代價：
 *
 * 1. 一個資源兩個 Console（§7.1 的判準，與收件台／送單紀錄同一個錯）
 * 2. **更難被看見** —— 例外件本來就該在電費單的脈絡裡出現；
 * 藏在另一個選單項，等於要求承辦**記得**去看。
 * 所以選單上改成**電費單那一項掛角標**。
 *
 * 指認的動作在**電費單詳情頁**（`intake` 模式）。
 * 送件詳情頁的 dialog 指認保留 —— 兩個脈絡各有一份入口是對的，
 * 它們回答的是不同的問題（「這一批怎麼處理」vs「這一張怎麼處理」）。
 */
const BillsPage: React.FC = () => {
  const notify = useNotify();
  const translate = useTranslate();
  const navigate = useNavigate();
  const selected = useSelection();
  const orgId = selected?.organizationId;

  const [loading, setLoading] = React.useState(false);
  const [denied, setDenied] = React.useState(false);
  const [bills, setBills] = React.useState<BillRow[]>([]);
  const [busyId, setBusyId] = React.useState<number | null>(null);

  /**
   * 從網址吃初始篩選（`?reviewStatus=`）。
   * 只吃**初始值**：使用者接著自己改篩選時 不要把網址搶回去。
   */
  const [searchParams, setSearchParams] = useSearchParams();
  /**
   * 頁籤 ＝ 「全部」／「待指認」。
   *
   * 它是 `reviewStatus` 的**捷徑**不是第二套狀態：選了「待指認」就等於
   * `reviewStatus='pendingMatch'`。做成兩份狀態的話，
   * 下拉與頁籤遲早會互相打架（選了頁籤又改下拉，畫面說不出自己在篩什麼）。
   */
  const tabParam = searchParams.get('tab');
  const tab: 'all' | 'pendingMatch' | 'pendingContract' =
    tabParam === 'pendingMatch' ? 'pendingMatch' : tabParam === 'pendingContract' ? 'pendingContract' : 'all';
  const [reviewStatus, setReviewStatus] = React.useState(() => searchParams.get('reviewStatus') ?? '');
  const effectiveReviewStatus = tab !== 'all' ? tab : reviewStatus;
  const setTab = (v: 'all' | 'pendingMatch' | 'pendingContract') => {
    const next = new URLSearchParams(searchParams);
    if (v === 'all') next.delete('tab'); else next.set('tab', v);
    setSearchParams(next, { replace: true });
  };
  /** 角標的數字 —— 與選單上那個是**同一支端點**（不可能對不起來）。 */
  const [unmatchedCount, setUnmatchedCount] = React.useState(0);
  /** S2：找不到生效合約那一格的角標。 */
  const [noContractCount, setNoContractCount] = React.useState(0);
  // Dashboard「本月應付」帶過來的計費期間；本頁沒有月份輸入框 → 不需要 setter。
  const month = searchParams.get('month') ?? '';
  const [keyword, setKeyword] = React.useState('');
  /** 「是否被人工修正過」—— 規格明列的篩選之一。 */
  const [correctedOnly, setCorrectedOnly] = React.useState(false);
  /**
   * 電號 → 案場名 ＋ **地主名**。
   * 地主是**規格明列的欄位**（`FRONTEND.md §2 P2`）——
   * `GET /bills` 只回 `station_id`，名字要靠 `station-list` 補
   * （那支本來就一併回 `landownerName`，不必為此多開一支端點）。
   */
  const [stationName, setStationName] = React.useState<Record<number, string>>({});
  const [ownerName, setOwnerName] = React.useState<Record<number, string>>({});

  const load = React.useCallback(async () => {
    if (!orgId) { setBills([]); setDenied(false); return; }
    setLoading(true); setDenied(false);
    try {
      const { data } = await energyHttp.get(`${energyBase(orgId)}/bills`, {
        params: {
          ...(effectiveReviewStatus ? { reviewStatus: effectiveReviewStatus } : {}),
          // 期間用**半開區間**送（`from` 含、`to` 不含）——不要送 `-31`：
          // 2 月沒有 31 號，後端會回 400 `invalid_date`（踩過）。
          ...(month ? { from: `${month}-01`, to: monthEndExclusive(month) } : {}),
        },
      });
      setBills(Array.isArray(data) ? data : []);
    } catch (e: any) {
      if (e?.response?.status === 403) setDenied(true);
      else notify(apiErrorCode(e) || e?.message || 'load failed', { type: 'error' });
    } finally {
      setLoading(false);
    }
    // 電號 → 案場名：清單要顯示人看得懂的名字，不是只有 id。
    try {
      const { data } = await energyHttp.get(`${energyBase(orgId)}/station-list`);
      const map: Record<number, string> = {};
      const owners: Record<number, string> = {};
      (Array.isArray(data) ? data : []).forEach((s: any) => {
        map[Number(s.id)] = s.name;
        if (s.landownerName) owners[Number(s.id)] = s.landownerName;
      });
      setStationName(map); setOwnerName(owners);
    } catch { setStationName({}); setOwnerName({}); }
    // 待指認的筆數：頁籤與選單角標共用這一支（`GET /bills/unmatched`）。
    try {
      const { data } = await energyHttp.get(`${energyBase(orgId)}/bills/unmatched`);
      setUnmatchedCount(Array.isArray(data) ? data.length : 0);
    } catch { setUnmatchedCount(0); }
    // 找不到生效合約那一格 —— 沒有獨立端點，用同一支 `/bills` 篩選後數長度。
    try {
      const { data } = await energyHttp.get(`${energyBase(orgId)}/bills`, { params: { reviewStatus: 'pendingContract', limit: 500 } });
      setNoContractCount(Array.isArray(data?.items) ? data.items.length : 0);
    } catch { setNoContractCount(0); }
  }, [orgId, effectiveReviewStatus, month, notify]);

  React.useEffect(() => { load(); }, [load]);

  /**
   * 直接送審 —— 前端**不組** stationKey / workflowKey / 分支屬性，
   * 那條推導在後端（與收件台送審**同一份**）。
   * 前端自己組一份的話，兩條路送出來的單會走不同流程。
   */
  const submit = async (billId: number) => {
    setBusyId(billId);
    try {
      await energyHttp.post(`${energyBase(orgId)}/bills/${billId}/submit`);
      notify('energy.bills.submitted', { type: 'success' });
      load();
    } catch (e: any) {
      notify(apiErrorCode(e) || 'submit failed', { type: 'error' });
    } finally { setBusyId(null); }
  };

  // 關鍵字在前端過濾（清單上限 100 筆，不為了搜尋多開一支端點）。
  const kw = keyword.trim().toLowerCase();
  const rows = bills.filter((b) => {
    if (correctedOnly && !(b.changedFields?.length)) return false;
    if (!kw) return true;
    const name = b.stationId != null ? (stationName[b.stationId] ?? '') : '';
    const owner = b.stationId != null ? (ownerName[b.stationId] ?? '') : '';
    return `${b.customerNumberRaw ?? ''}${name}${owner}`.toLowerCase().includes(kw);
  });

  return (
    <EnergyPageFrame orgId={orgId} denied={denied} loading={loading}>
      <Stack direction="row" alignItems="center" justifyContent="space-between" mb={2}>
        <Typography variant="h6">{translate('energy.bills.title')}</Typography>
        <Tooltip title={translate('ra.action.refresh')}>
          <IconButton onClick={load} disabled={loading}><RefreshIcon /></IconButton>
        </Tooltip>
      </Stack>

      {/* 例外件併進來的那一格（§7.4）。數字寫在頁籤上 ——
          「有幾件等我指認」是承辦每天第一個要知道的事。 */}
      <Tabs
        value={tab}
        onChange={(_, v) => setTab(v)}
        sx={{ mb: 2 }}
        data-testid="bills-tabs"
        data-active={tab}
      >
        <Tab value="all" data-testid="bills-tab-all" label={translate('energy.bills.tabAll')} />
        <Tab
          value="pendingMatch"
          data-testid="bills-tab-pendingMatch"
          data-count={unmatchedCount}
          label={translate('energy.bills.tabPendingMatch', { count: unmatchedCount })}
          sx={unmatchedCount > 0 ? { color: jvTokens.warn } : undefined}
        />
        <Tab
          value="pendingContract"
          data-testid="bills-tab-pendingContract"
          data-count={noContractCount}
          label={translate('energy.bills.tabPendingContract', { count: noContractCount })}
          sx={noContractCount > 0 ? { color: jvTokens.warn } : undefined}
        />
      </Tabs>

      <Card>
        <CardContent>
          {tab === 'pendingMatch' && (
            // 這一格堆積＝整條流程停住。用**燒橘**（紅是「壞掉」，這裡是「等人處理」）。
            <Alert
              severity="warning"
              icon={false}
              data-testid="bills-pendingmatch-note"
              sx={{
                mb: 2,
                color: jvTokens.warn,
                borderColor: jvTokens.warn,
                border: '1px solid',
                backgroundColor: 'transparent',
              }}
            >
              {translate('energy.unmatched.blockingNote')}
            </Alert>
          )}
          {tab === 'pendingContract' && (
            <Alert
              severity="warning"
              icon={false}
              data-testid="bills-pendingcontract-note"
              sx={{
                mb: 2,
                color: jvTokens.warn,
                borderColor: jvTokens.warn,
                border: '1px solid',
                backgroundColor: 'transparent',
              }}
            >
              {translate('energy.unmatched.noContractBlockingNote')}
            </Alert>
          )}
          <Stack direction="row" gap={2} mb={2} flexWrap="wrap" alignItems="center">
            <TextField
              select
              size="small"
              sx={{ minWidth: 180 }}
              label={translate('energy.bills.reviewStatus')}
              value={effectiveReviewStatus}
              // 頁籤已經釘住 `pendingMatch`／`pendingContract` 時不讓下拉再改一次 ——
              // 兩個控制項同時改同一件事，畫面就說不出自己在篩什麼。
              disabled={tab !== 'all'}
              onChange={(e) => setReviewStatus(e.target.value)}
              inputProps={{ 'data-testid': 'bills-review-status' }}
            >
              <MenuItem value="">{translate('energy.common.all')}</MenuItem>
              {REVIEW_STATUSES.map((s) => (
                <MenuItem key={s} value={s}>{translate(`energy.reviewStatus.${s}`)}</MenuItem>
              ))}
            </TextField>
            <TextField
              size="small"
              sx={{ minWidth: 220 }}
              label={translate('energy.bills.search')}
              value={keyword}
              onChange={(e) => setKeyword(e.target.value)}
            />
            <Chip
              size="small"
              variant={correctedOnly ? 'filled' : 'outlined'}
              data-testid="bills-corrected-filter"
              label={translate('energy.bills.correctedOnly')}
              onClick={() => setCorrectedOnly(!correctedOnly)}
              sx={correctedOnly
                ? { background: jvTokens.primary, color: '#fff' }
                : { color: jvTokens.primary, borderColor: jvTokens.primary }}
            />
          </Stack>

          <Box sx={{ overflowX: 'auto' }}>
            <Table size="small">
              <TableHead>
                <TableRow>
                  <TableCell>{translate('energy.bills.station')}</TableCell>
                  <TableCell>{translate('energy.bills.customerNumber')}</TableCell>
                  <TableCell>{translate('energy.bills.landowner')}</TableCell>
                  <TableCell>{translate('energy.bills.period')}</TableCell>
                  <TableCell align="right">{translate('energy.bills.amount')}</TableCell>
                  <TableCell align="right">{translate('energy.bills.kwh')}</TableCell>
                  <TableCell>{translate('energy.bills.issuedAt')}</TableCell>
                  <TableCell>{translate('energy.bills.reviewStatus')}</TableCell>
                  {/* 唯一的流程出口 —— 這一欄是「連結」，不是「關卡」。 */}
                  <TableCell>{translate('energy.bills.execution')}</TableCell>
                </TableRow>
              </TableHead>
              <TableBody>
                {rows.length ? rows.map((b) => (
                  <TableRow
                    key={b.id}
                    hover
                    data-testid="bill-row"
                    data-submitted={b.executionId != null ? '1' : '0'}
                    sx={{ cursor: 'pointer' }}
                    onClick={() => navigate(billRoute(b.id))}
                  >
                    <TableCell>
                      {b.stationId != null && stationName[b.stationId]
                        ? <Typography variant="body2">{stationName[b.stationId]}</Typography>
                        : (
                          // 沒有指認到發電站＝例外件；用**燒橘**點出來（不是紅）。
                          <Chip
                            size="small"
                            variant="outlined"
                            label={translate('energy.bills.unmatchedTag')}
                            sx={{ color: jvTokens.warn, borderColor: jvTokens.warn }}
                          />
                        )}
                    </TableCell>
                    <TableCell>
                      <Typography variant="body2" sx={{ fontVariantNumeric: 'tabular-nums' }}>
                        {b.customerNumberRaw || '—'}
                      </Typography>
                    </TableCell>
                    <TableCell>
                      <Typography variant="body2">
                        {b.stationId != null ? (ownerName[b.stationId] ?? '—') : '—'}
                      </Typography>
                    </TableCell>
                    <TableCell><Period from={b.billingPeriodStart} to={b.billingPeriodEnd} /></TableCell>
                    <TableCell align="right"><Money value={b.totalAmount} /></TableCell>
                    <TableCell align="right"><Money value={b.generationKwh} /></TableCell>
                    <TableCell>
                      <Typography variant="body2">
                        {b.issuedAt ? String(b.issuedAt).slice(0, 10) : '—'}
                      </Typography>
                    </TableCell>
                    <TableCell>
                      <Stack direction="row" gap={0.5} alignItems="center">
                        <StatusChip i18nKey="energy.reviewStatus" value={b.reviewStatus} />
                        {/* 辨識值 vs 最終值：被改過要標記（`DESIGN.md §1.4` 三層值）。 */}
                        {b.changedFields?.length ? (
                          <Tooltip
                            title={b.changedFields
                              .map((f) => translate(`energy.bills.field.${f}`, { _: f }))
                              .join('、')}
                          >
                            <Chip
                              size="small"
                              data-testid="bill-corrected"
                              label={translate('energy.bills.corrected')}
                              sx={{ color: jvTokens.primary, borderColor: jvTokens.primary }}
                              variant="outlined"
                            />
                          </Tooltip>
                        ) : null}
                      </Stack>
                    </TableCell>
                    {/* `stopPropagation`：不然會被整列的 onClick 蓋掉。 */}
                    <TableCell onClick={(e) => e.stopPropagation()}>
                      {b.executionId != null ? (
                        <Button
                          size="small"
                          data-testid="bill-open-execution"
                          /**
                           * 連到 **P12 詳情**，不是「只有一列的 P12 清單」。
                           *
                           * 前一版帶 `?executionId=` 去清單頁。§7.3 之後那一頁
                           * 是「全組織」的視角（需 `view-all`）——
                           * 沒有那個權限的承辦點自己的單會撞牆。
                           * 而且「看這一張的流程」本來就該去它自己的詳情頁：
                           * 詳情是**參與者**就看得到的。
                           */
                          onClick={() => navigate(executionRoute(b.executionId!))}
                        >
                          {translate('energy.bills.openExecution')}
                        </Button>
                      ) : (
                        <Stack direction="row" gap={0.5} alignItems="center">
                          {/* 「尚未送審」的家在這裡 —— 它是**電費單的狀態**。 */}
                          <Chip
                            size="small"
                            variant="outlined"
                            data-testid="bill-unsubmitted"
                            label={translate('energy.bills.unsubmitted')}
                            sx={{ color: jvTokens.warn, borderColor: jvTokens.warn }}
                          />
                          {/* 電號還沒指認的不能送 —— 後端也會擋（`bill_needs_station`），
                              但這裡先不畫按鈕，不要給一顆按了會失敗的按鈕。 */}
                          {b.stationId != null && (
                            <Button
                              size="small"
                              disabled={busyId === b.id}
                              data-testid="bill-submit"
                              onClick={() => submit(b.id)}
                            >
                              {translate('energy.bills.submit')}
                            </Button>
                          )}
                        </Stack>
                      )}
                    </TableCell>
                  </TableRow>
                )) : <EmptyRow colSpan={9} loading={loading} />}
              </TableBody>
            </Table>
          </Box>
        </CardContent>
      </Card>
    </EnergyPageFrame>
  );
};

export default BillsPage;
