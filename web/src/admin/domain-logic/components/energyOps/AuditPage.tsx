import * as React from 'react';
import {
  Box, Card, CardContent, Typography, Table, TableHead, TableBody, TableRow, TableCell,
  Stack, Tooltip, IconButton, TextField, MenuItem, Collapse, Chip, Button,
} from '@mui/material';
import RefreshIcon from '@mui/icons-material/Refresh';
import ExpandMoreIcon from '@mui/icons-material/ExpandMore';
import { energyHttp, energyBase } from '@/api/energy';
import { useNotify, useTranslate } from 'react-admin';
import { useNavigate } from 'react-router-dom';
import { useSelection } from '@/platform/selection/store';
import { apiErrorCode } from '../../../dataProvider/apiError';
import { EnergyPageFrame, EmptyRow, jvTokens } from './shared';
// 「哪個資源的哪一筆 → 去哪一頁」的**唯一一份對照**（各頁不自己拼字串）。
import { auditTargetRoute } from './routes';

interface AuditRow {
  id: number; type: string; severity: string; summaryKey: string | null;
  actorUserId: number | null; actorLabel: string | null; actorName: string | null;
  onBehalfOfUserId: number | null; onBehalfOfName: string | null;
  targetType: string | null; targetId: string | null; targetLabel: string | null;
  /**
   * 反跳實體所需的**關聯**（不是路由）——
   * 後端只回「是哪個資源的哪一筆」，「去哪」由 `routes.ts` 決定。
   * 這兩個是前端查不到的：task 屬於哪個 execution、應付掛在哪張單。
   */
  targetExecutionId: number | null;
  targetBillId: number | null;
  /** 這一筆是**被哪一筆觸發的**（`AUDIT.md §7.5` 的因果鏈）。 */
  causedBy: {
    id: number; type: string; summary: string; summaryKey: string | null; payload?: any;
  } | null;
  outcome: string; summary: string | null; payload: any; traceId: string | null; occurredAt: string;
}

/**
 * 事件類型的**中文分組**（`FRONTEND.md §2-P6`）——
 * 篩選下拉不可以列 raw type 字串（`flow.task_completed` 對使用者毫無意義）。
 *
 * 這裡列的是**前綴**，不是完整型別：型別會一直增加（目前 40 個），
 * 逐一列舉的話每加一個事件就要回來改前端。分組是穩定的，型別不是。
 */
const TYPE_GROUPS: Array<{ key: string; prefix: string }> = [
  { key: 'department', prefix: 'department.' },
  { key: 'member', prefix: 'member.' },
  { key: 'flow', prefix: 'flow.' },
  { key: 'delegation', prefix: 'delegation.' },
  { key: 'lineage', prefix: 'lineage.' },
  { key: 'attachment', prefix: 'attachment.' },
  { key: 'bill', prefix: 'bill.' },
  { key: 'payout', prefix: 'payout.' },
  { key: 'strategy', prefix: 'strategy.' },
];

const OUTCOMES = ['success', 'denied', 'failed'];

/** 結果 → 顏色。失敗用**燒橘**不是紅。 */
const OUTCOME_COLOR: Record<string, string> = {
  success: jvTokens.ok,
  denied: jvTokens.gold,
  failed: jvTokens.warn,
};

/**
 * P6 稽核紀錄（`FRONTEND.md §2-P6`）。
 *
 * 三條硬規矩：
 * 1. **事件類型用中文分組**，篩選下拉不列 raw type。
 * 2. **操作者是人名下拉**，不是叫使用者去打 user id。
 * 3. `onBehalfOfUser` 有值時要顯示「A 以 B 的代理人身分」——
 * 代理是稽核最需要看得出來的東西，藏起來等於沒有稽核。
 *
 * 展開後顯示 `payload` 的**差異對**（欄位／原值／新值）。
 * `AuditLogger.diffChanges` 寫進去的形狀是 `{ field: { before, after } }`。
 */
const AuditPage: React.FC = () => {
  const notify = useNotify();
  const translate = useTranslate();
  const navigate = useNavigate();
  const orgId = useSelection()?.organizationId;

  const [loading, setLoading] = React.useState(false);
  const [denied, setDenied] = React.useState(false);
  const [rows, setRows] = React.useState<AuditRow[]>([]);
  const [actors, setActors] = React.useState<Array<{ id: number; name: string }>>([]);
  const [group, setGroup] = React.useState('');
  const [actorUserId, setActorUserId] = React.useState('');
  const [outcome, setOutcome] = React.useState('');
  const [from, setFrom] = React.useState('');
  const [to, setTo] = React.useState('');
  const [open, setOpen] = React.useState<number | null>(null);

  const base = energyBase(orgId);

  const load = React.useCallback(async () => {
    if (!orgId) { setRows([]); setDenied(false); return; }
    setLoading(true); setDenied(false);
    try {
      const { data } = await energyHttp.get(`${base}/audit-events`, {
        params: {
          ...(actorUserId ? { actorUserId } : {}),
          ...(outcome ? { outcome } : {}),
          ...(from ? { from } : {}),
          ...(to ? { to } : {}),
          limit: 300,
        },
      });
      setRows(Array.isArray(data) ? data : []);
    } catch (e: any) {
      if (e?.response?.status === 403) setDenied(true);
      else notify(apiErrorCode(e) || e?.message || 'load failed', { type: 'error' });
    } finally {
      setLoading(false);
    }
    try {
      const { data } = await energyHttp.get(`${base}/audit-actors`);
      setActors(Array.isArray(data) ? data : []);
    } catch { setActors([]); }
  }, [orgId, base, actorUserId, outcome, from, to, notify]);

  React.useEffect(() => { load(); }, [load]);

  // 分組在前端過濾（因為它是「前綴」不是欄位值，不值得為它多開一個查詢參數）。
  const visible = group ? rows.filter((r) => r.type.startsWith(group)) : rows;

  /**
   * 事件那一欄的**一句話**。
   *
   * 有幾句要靠 `payload` 撐起來 —— 少了插值它們就變成
   * 「修正金額後少了一關」，而**少了哪一關**才是重點。
   * 這裡把 payload 裡幾個具名欄位一起傳進 `translate`；
   * 用不到的句子拿到多餘的參數不會有事。
   *
   * 翻不到就顯示 `—`，**不** fallback 成 raw type
   * （`energy:` / `flow.` 這種字串不該出現在畫面上）。
   */
  const summaryOf = (row: { summaryKey: string | null; payload?: any }) => {
    if (!row.summaryKey) return '—';
    const p = row.payload ?? {};
    /**
     * 插值的來源是 registry 的 `payloadSchema`，**不是**隨便猜欄位名：
     *
     * · `bill.route_gate_*` → `gatesDelta.{added,removed}`（nodeKey）
     *   ＋ `gateNames`（key → 顯示名）。**印名稱不印 key**。
     *   一次只會有一邊非空，所以兩邊接起來就是那一關。
     * · `flow.side_effect_*` → `sideEffectKey`（例如 `payment.sent`）。
     */
    const delta = p.gatesDelta ?? {};
    const gateKeys = [...(delta.added ?? []), ...(delta.removed ?? [])];
    const gate = gateKeys.map((k: string) => p.gateNames?.[k] ?? k).join('、');
    const effectKey = p.sideEffectKey ?? '';
    /**
     * **payload 的純量欄位全部當成插值來源。**
     *
     * 原本這裡只餵 `gate` 與 `effect` 兩個具名欄位，於是每新增一種事件、
     * 只要它的句子引用了自己 payload 的欄位（`%{documents}`／`%{fileCount}`…），
     * 畫面上就會**原樣印出樣板洞** —— 而寫那句話的人不會知道還要回來改這個檔。
     * 實際踩到：辨識那四個事件（`ingest.recognition_*`）全部印成
     * 「把 %{fileCount} 份檔案送去辨識」。
     *
     * 只帶純量：物件／陣列插進句子會變成 `[object Object]`，
     * 那比樣板洞更難看懂。要用結構化的值就自己算一個具名欄位（像下面兩個）。
     */
    const scalars: Record<string, string | number | boolean> = {};
    Object.entries(p).forEach(([k, v]) => {
      if (typeof v === 'string' || typeof v === 'number' || typeof v === 'boolean') scalars[k] = v;
    });
    return translate(row.summaryKey, {
      ...scalars,
      _: '—',
      gate: gate || '—',
      // 單別要印**中文名**，不是 `electricityBill`（畫面上不出現 raw docType）。
      docType: p.docType ? translate(`energy.docType.${p.docType}`, { _: p.docType }) : '—',
      effect: effectKey ? translate(`energy.sideEffect.${effectKey}`, { _: effectKey }) : '—',
    });
  };

  /** `payload` 的差異對。不是每個事件都有 `changes`，沒有就顯示整包 JSON。 */
  const renderPayload = (payload: any) => {
    const changes = payload?.changes;
    if (changes && typeof changes === 'object' && !Array.isArray(changes)) {
      return (
        <Table size="small">
          <TableHead>
            <TableRow>
              <TableCell>{translate('energy.audit.field')}</TableCell>
              <TableCell>{translate('energy.audit.before')}</TableCell>
              <TableCell>{translate('energy.audit.after')}</TableCell>
            </TableRow>
          </TableHead>
          <TableBody>
            {Object.entries(changes).map(([field, v]: [string, any]) => (
              <TableRow key={field}>
                <TableCell>{translate(`energy.bills.field.${field}`, { _: field })}</TableCell>
                <TableCell sx={{ color: jvTokens.muted }}>{String(v?.before ?? '—')}</TableCell>
                <TableCell>{String(v?.after ?? '—')}</TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      );
    }
    return (
      <Box
        component="pre"
        sx={{
          m: 0, p: 1, fontSize: 12, overflowX: 'auto',
          background: jvTokens.hoverBg, borderRadius: 1,
        }}
      >
        {JSON.stringify(payload ?? {}, null, 2)}
      </Box>
    );
  };

  return (
    <EnergyPageFrame orgId={orgId} denied={denied} loading={loading}>
      <Stack direction="row" alignItems="center" justifyContent="space-between" mb={2}>
        <Typography variant="h6">{translate('energy.audit.title')}</Typography>
        <Tooltip title={translate('ra.action.refresh')}>
          <IconButton onClick={load} disabled={loading}><RefreshIcon /></IconButton>
        </Tooltip>
      </Stack>

      <Card>
        <CardContent>
          <Stack direction="row" gap={2} mb={2} flexWrap="wrap">
            {/* 這裡列的是中文分組，不是 raw type。 */}
            <TextField
              select size="small" sx={{ minWidth: 160 }}
              label={translate('energy.audit.group')}
              value={group}
              onChange={(e) => setGroup(e.target.value)}
              inputProps={{ 'data-testid': 'audit-group' }}
            >
              <MenuItem value="">{translate('energy.common.all')}</MenuItem>
              {TYPE_GROUPS.map((g) => (
                <MenuItem key={g.key} value={g.prefix}>{translate(`energy.audit.group.${g.key}`)}</MenuItem>
              ))}
            </TextField>
            {/* 人名下拉，不是 id 輸入框。 */}
            <TextField
              select size="small" sx={{ minWidth: 160 }}
              label={translate('energy.audit.actor')}
              value={actorUserId}
              onChange={(e) => setActorUserId(e.target.value)}
              inputProps={{ 'data-testid': 'audit-actor' }}
            >
              <MenuItem value="">{translate('energy.common.all')}</MenuItem>
              {actors.map((a) => (
                <MenuItem key={a.id} value={String(a.id)}>{a.name || `#${a.id}`}</MenuItem>
              ))}
            </TextField>
            <TextField
              select size="small" sx={{ minWidth: 140 }}
              label={translate('energy.audit.outcome')}
              value={outcome}
              onChange={(e) => setOutcome(e.target.value)}
              inputProps={{ 'data-testid': 'audit-outcome' }}
            >
              <MenuItem value="">{translate('energy.common.all')}</MenuItem>
              {OUTCOMES.map((o) => (
                <MenuItem key={o} value={o}>{translate(`energy.audit.outcomeValue.${o}`)}</MenuItem>
              ))}
            </TextField>
            <TextField
              size="small" type="date" label={translate('energy.audit.from')}
              InputLabelProps={{ shrink: true }}
              value={from} onChange={(e) => setFrom(e.target.value)}
            />
            <TextField
              size="small" type="date" label={translate('energy.audit.to')}
              InputLabelProps={{ shrink: true }}
              value={to} onChange={(e) => setTo(e.target.value)}
            />
          </Stack>

          <Table size="small">
            <TableHead>
              <TableRow>
                <TableCell sx={{ width: 40 }} />
                <TableCell>{translate('energy.review.when')}</TableCell>
                <TableCell>{translate('energy.review.what')}</TableCell>
                <TableCell>{translate('energy.review.who')}</TableCell>
                <TableCell>{translate('energy.audit.target')}</TableCell>
                <TableCell>{translate('energy.audit.outcome')}</TableCell>
              </TableRow>
            </TableHead>
            <TableBody>
              {visible.length ? visible.map((r) => (
                <React.Fragment key={r.id}>
                  <TableRow
                    hover
                    data-testid="audit-row"
                    // 讓斷言**指名得到**某一種列 —— 不要靠「第一筆剛好是對的」。
                    // target 是多型的（bill / execution / task / station …），
                    // 而它們現在各自跳到**各自的資源頁**（流程事件 → P12）。
                    data-target-type={r.targetType ?? ''}
                    data-type={r.type}
                  >
                    <TableCell>
                      <IconButton size="small" onClick={() => setOpen(open === r.id ? null : r.id)}>
                        <ExpandMoreIcon
                          fontSize="small"
                          sx={{ transform: open === r.id ? 'rotate(180deg)' : undefined }}
                        />
                      </IconButton>
                    </TableCell>
                    <TableCell sx={{ whiteSpace: 'nowrap' }}>
                      <Typography variant="caption">
                        {String(r.occurredAt ?? '').slice(0, 19).replace('T', ' ')}
                      </Typography>
                    </TableCell>
                    <TableCell>
                      {/* 一律翻成中文；翻不到顯示 `—`，不 fallback 成 raw type。 */}
                      <Typography variant="body2" data-testid="audit-what">
                        {summaryOf(r)}
                      </Typography>
                    </TableCell>
                    <TableCell>
                      <Typography variant="body2">
                        {r.actorName || r.actorLabel || `#${r.actorUserId ?? '—'}`}
                      </Typography>
                      {/* 因果鏈：「← 由 <某筆> 觸發」（`AUDIT.md §7.5`）。
                          沒有它，「這張單少了一關」那筆看起來像憑空發生 ——
                          而稽核要答的正是**是誰的哪個動作造成的**。 */}
                      {r.causedBy && (
                        <Typography
                          variant="caption"
                          display="block"
                          data-testid="audit-caused-by"
                          data-cause-id={r.causedBy.id}
                          sx={{ color: jvTokens.caption }}
                        >
                          {translate('energy.audit.causedBy', {
                            what: r.causedBy.summaryKey
                              ? summaryOf(r.causedBy)
                              : r.causedBy.type,
                          })}
                        </Typography>
                      )}
                      {/* 代理身分 —— 稽核最需要看得出來的東西。 */}
                      {r.onBehalfOfUserId ? (
                        <Typography variant="caption" sx={{ color: jvTokens.primary }} data-testid="audit-delegate">
                          {translate('energy.audit.onBehalfOf', {
                            actor: r.actorName || r.actorLabel || `#${r.actorUserId}`,
                            principal: r.onBehalfOfName || `#${r.onBehalfOfUserId}`,
                          })}
                        </Typography>
                      ) : null}
                    </TableCell>
                    {/* `data-testid` 掛在**整格**上：這一格有兩種長相
                        （連得過去＝按鈕、連不過去＝純文字），
                        探針只認其中一種的話會漏掉另一半，
                        而「只量到一半」跟「整欄都對」在報表上長得一樣。 */}
                    <TableCell data-testid="audit-target">
                      {/* 反跳實體（`IA.md §5` 矩陣的 P6 那一列）——
                          沒有它稽核頁是一條死路：看到「某某核准了某張單」卻跳不過去看那張單。
                          路由由**前端**的 `routes.ts` 決定（後端不回路由字串）；
                          流程類事件連 **P12 流程單**，不繞去它底下那張 bill。
                          **連不到就不畫按鈕**，不畫一顆按不動的。 */}
                      {auditTargetRoute(r.targetType, r.targetId, {
                        executionId: r.targetExecutionId, billId: r.targetBillId,
                      }) ? (
                        <Button
                          size="small"
                          sx={{ p: 0, minWidth: 0, justifyContent: 'flex-start' }}
                          data-testid="audit-open"
                          onClick={() => navigate(auditTargetRoute(r.targetType, r.targetId, {
                            executionId: r.targetExecutionId, billId: r.targetBillId,
                          })!)}
                        >
                          {r.targetLabel || (r.targetId ? `#${r.targetId}` : '—')}
                        </Button>
                      ) : (
                        <Typography variant="caption" color="text.secondary">
                          {r.targetLabel || (r.targetId ? `#${r.targetId}` : '—')}
                        </Typography>
                      )}
                    </TableCell>
                    <TableCell>
                      <Chip
                        size="small"
                        variant="outlined"
                        label={translate(`energy.audit.outcomeValue.${r.outcome}`, { _: r.outcome })}
                        sx={{
                          color: OUTCOME_COLOR[r.outcome] ?? jvTokens.muted,
                          borderColor: OUTCOME_COLOR[r.outcome] ?? jvTokens.border,
                        }}
                      />
                    </TableCell>
                  </TableRow>
                  <TableRow>
                    <TableCell colSpan={6} sx={{ py: 0, border: 0 }}>
                      <Collapse in={open === r.id} unmountOnExit>
                        <Box py={2}>{renderPayload(r.payload)}</Box>
                      </Collapse>
                    </TableCell>
                  </TableRow>
                </React.Fragment>
              )) : <EmptyRow colSpan={6} loading={loading} />}
            </TableBody>
          </Table>
        </CardContent>
      </Card>
    </EnergyPageFrame>
  );
};

export default AuditPage;
