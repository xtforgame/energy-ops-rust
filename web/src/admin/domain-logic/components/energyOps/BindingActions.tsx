import * as React from 'react';
import {
  Stack, Button, Dialog, DialogTitle, DialogContent, DialogActions,
  TextField, MenuItem, Typography, Alert,
} from '@mui/material';
import { energyHttp, energyBase } from '@/api/energy';
import { useNotify, useTranslate } from 'react-admin';
import { BILL_DOC_TYPES } from 'common/plugins/EnergyOpsCommonPlugin/documentIngest';
import { apiErrorCode } from '../../../dataProvider/apiError';
import { jvTokens } from './shared';

export type BindingKind = 'workflow' | 'rating';

interface VersionOption {
  id: number; key: string; name: string; version: number; type: string | null;
}

interface Preview {
  isBackdated: boolean;
  affectedDraftBills: number;
  runningExecutions: number;
}

/**
 * 效期綁定的三個動作（`FRONTEND.md §2.84`、`WORKFLOW.md §3.2.6`）。
 *
 * **不做 CRUD** —— 效期不重疊是硬約束，暴露 `validFrom`/`validTo` 給人編輯，
 * 第一件事就是撞上約束而且錯誤訊息看不懂。
 * 把危險的狀態轉移包成**具名動作**：綁定 / 換版 / 解除。
 *
 * **動作的家是 P11（案場詳情）而不是 P9**：真實的觸發時機是「新案場上線」，
 * 那個人正在看**案場**。而且綁定的語意主詞就是案場：
 * 「**這個案場**，從哪天起，用哪一版」。
 *
 * 按鈕出不出現由**後端** `canManage` 決定（前端不自己推）——
 * 只有 admin（`energy:govern:system`），**CXO 也不行**。
 */
const BindingActions: React.FC<{
  orgId?: number | null;
  stationId: number;
  kind: BindingKind;
  /**
   * **這一組動作作用在哪一條線上**（單別 × 流程）。
   *
   * 一座案場現在可以同時綁多條流程（躉購·有餘電要核可 ＋ 開發票），
   * 所以「換版／解除」必須說得出改的是哪一條 ——
   * 沒有這兩個值的話後端只能猜，而「解除綁定」是 critical 動作，猜錯代價很高。
   * 新綁一條時 `targetKey` 由選到的版本決定，這裡只需要單別。
   */
  docType?: string | null;
  targetKey?: string | null;
  /** 新綁一條線時讓人選單別（只有 workflow 有這一維）。 */
  allowDocTypeSelect?: boolean;
  /** 目前有沒有生效中的段 —— 決定顯示「綁定」還是「換版／解除」。 */
  hasCurrent: boolean;
  /** 現行段的顯示名（對話框要印「目前：…」）。 */
  currentLabel?: string | null;
  canManage: boolean;
  onDone: () => void;
}> = ({
  orgId, stationId, kind, docType, targetKey, allowDocTypeSelect,
  hasCurrent, currentLabel, canManage, onDone,
}) => {
  const notify = useNotify();
  const translate = useTranslate();
  const base = energyBase(orgId);

  const [mode, setMode] = React.useState<null | 'create' | 'switch' | 'end'>(null);
  const [options, setOptions] = React.useState<VersionOption[]>([]);
  const [versionId, setVersionId] = React.useState<number | ''>('');
  const [effectiveFrom, setEffectiveFrom] = React.useState(() => new Date().toISOString().slice(0, 10));
  const [reason, setReason] = React.useState('');
  const [docTypeDraft, setDocTypeDraft] = React.useState<string>(docType ?? '');
  const [preview, setPreview] = React.useState<Preview | null>(null);
  const [busy, setBusy] = React.useState(false);

  const open = (m: 'create' | 'switch' | 'end') => {
    setMode(m); setVersionId(''); setReason(''); setPreview(null);
    setDocTypeDraft(docType ?? '');
    setEffectiveFrom(new Date().toISOString().slice(0, 10));
    if (m !== 'end') {
      energyHttp.get(`${base}/binding-options`, { params: { kind } })
        .then(({ data }) => setOptions(Array.isArray(data) ? data : []))
        .catch(() => setOptions([]));
    }
  };

  /**
   * 後果要在**按下之前**看得到（`FRONTEND.md §2.84.2`）——
   * 所以 `preview` 是**獨立一支端點**，不是 switch 回應的一部分。
   * 日期一改就重算（debounce），不要等到按下去才問。
   */
  React.useEffect(() => {
    if (!mode || !orgId || !/^\d{4}-\d{2}-\d{2}$/.test(effectiveFrom)) { setPreview(null); return; }
    let alive = true;
    const timer = setTimeout(() => {
      energyHttp.get(`${base}/stations/${stationId}/bindings/preview`, {
        params: { kind, effectiveFrom },
      })
        .then(({ data }) => { if (alive) setPreview(data); })
        .catch(() => { if (alive) setPreview(null); });
    }, 350);
    return () => { alive = false; clearTimeout(timer); };
  }, [mode, effectiveFrom, kind, orgId, stationId, base]);

  const submit = async () => {
    setBusy(true);
    try {
      if (mode === 'create') {
        await energyHttp.post(`${base}/stations/${stationId}/bindings`, {
          kind, versionId, validFrom: effectiveFrom, docType: docTypeDraft || null,
        });
      } else if (mode === 'switch') {
        await energyHttp.post(`${base}/stations/${stationId}/bindings/switch`, {
          kind, toVersionId: versionId, effectiveFrom, docType: docTypeDraft || null,
        });
      } else {
        await energyHttp.post(`${base}/stations/${stationId}/bindings/end`, {
          // 解除要說得出是**哪一條線** —— 這座案場可能同時綁著好幾條流程。
          kind, validTo: effectiveFrom, reason, docType: docType ?? null, targetKey: targetKey ?? null,
        });
      }
      notify('energy.bindings.done', { type: 'success' });
      setMode(null);
      onDone();
    } catch (e: any) {
      notify(apiErrorCode(e) || 'binding failed', { type: 'error' });
    } finally { setBusy(false); }
  };

  // 不畫按不動的按鈕 —— 沒有權限就整組不出現（同 `actionBlocked` 的道理）。
  if (!canManage) return null;

  const canSubmit = mode === 'end'
    ? !!reason.trim()
    : versionId !== '' && /^\d{4}-\d{2}-\d{2}$/.test(effectiveFrom);

  return (
    <>
      <Stack direction="row" gap={1} mt={1} data-testid="binding-actions" data-kind={kind}>
        {!hasCurrent && (
          <Button
            size="small" variant="outlined"
            data-testid="binding-create"
            onClick={() => open('create')}
          >
            {translate('energy.bindings.create')}
          </Button>
        )}
        {hasCurrent && (
          <>
            <Button
              size="small" variant="outlined"
              data-testid="binding-switch"
              onClick={() => open('switch')}
            >
              {translate('energy.bindings.switch')}
            </Button>
            {/* 解除＝之後這個案場收不了單／算不出錢 —— 用**燒橘**標出來。 */}
            <Button
              size="small" variant="outlined"
              data-testid="binding-end"
              onClick={() => open('end')}
              sx={{ color: jvTokens.warn, borderColor: jvTokens.warn }}
            >
              {translate('energy.bindings.end')}
            </Button>
          </>
        )}
      </Stack>

      <Dialog open={!!mode} onClose={() => setMode(null)} fullWidth maxWidth="sm">
        <DialogTitle data-testid="binding-dialog" data-mode={mode ?? ''}>
          {translate(`energy.bindings.title.${mode ?? 'create'}`)}
        </DialogTitle>
        <DialogContent>
          <Stack gap={2} mt={1}>
            {currentLabel && (
              <Typography variant="body2" color="text.secondary">
                {translate('energy.bindings.current', { name: currentLabel })}
              </Typography>
            )}

            {/* 新綁一條線時要先說**這一段適用於哪一種單** ——
                「要開哪些流程」就是靠這一維：同一座案場的躉購單與用電單
                可以綁不同的流程，而躉購·有餘電那種可以同時綁兩條。 */}
            {mode === 'create' && allowDocTypeSelect && (
              <TextField
                select size="small"
                label={translate('energy.bindings.docType')}
                value={docTypeDraft}
                onChange={(e) => setDocTypeDraft(String(e.target.value))}
                inputProps={{ 'data-testid': 'binding-doc-type' }}
                helperText={translate('energy.bindings.docTypeHint')}
              >
                <MenuItem value="" data-testid="binding-doc-type-option">
                  {translate('energy.docType.anyDocType')}
                </MenuItem>
                {BILL_DOC_TYPES.map((t) => (
                  <MenuItem key={t} value={t} data-testid="binding-doc-type-option">
                    {translate(`energy.docType.${t}`)}
                  </MenuItem>
                ))}
              </TextField>
            )}

            {mode !== 'end' && (
              <TextField
                select size="small"
                label={translate('energy.bindings.version')}
                value={versionId}
                onChange={(e) => setVersionId(Number(e.target.value))}
                inputProps={{ 'data-testid': 'binding-version' }}
              >
                {options.map((o) => (
                  <MenuItem key={o.id} value={o.id} data-testid="binding-version-option">
                    {o.name} v{o.version}
                  </MenuItem>
                ))}
              </TextField>
            )}

            <TextField
              size="small" type="date"
              label={translate(mode === 'end' ? 'energy.bindings.validTo' : 'energy.bindings.effectiveFrom')}
              InputLabelProps={{ shrink: true }}
              value={effectiveFrom}
              onChange={(e) => setEffectiveFrom(e.target.value)}
              inputProps={{ 'data-testid': 'binding-date' }}
            />

            {mode === 'end' && (
              <TextField
                size="small" multiline minRows={2}
                label={translate('energy.bindings.reason')}
                value={reason}
                onChange={(e) => setReason(e.target.value)}
                inputProps={{ 'data-testid': 'binding-reason' }}
                helperText={translate('energy.bindings.endWarn')}
              />
            )}

            {/* 回溯生效的兩行後果（`FRONTEND.md §2.84.2`）。
                **第二行一定要在** —— 沒有它管理員不敢按：
                他會以為自己要把進行中的單全弄亂。 */}
            {preview?.isBackdated && (
              <Alert
                severity="warning"
                data-testid="binding-backdated"
                data-affected={preview.affectedDraftBills}
                data-running={preview.runningExecutions}
                sx={{
                  color: jvTokens.warn, border: `1px solid ${jvTokens.warn}`,
                  backgroundColor: 'transparent',
                }}
              >
                <Typography variant="body2" fontWeight={600}>
                  {translate('energy.bindings.backdated')}
                </Typography>
                <Typography variant="caption" display="block">
                  · {translate('energy.bindings.affectedDrafts', { count: preview.affectedDraftBills })}
                </Typography>
                <Typography variant="caption" display="block" sx={{ color: jvTokens.primaryDark, fontWeight: 600 }}>
                  · {translate('energy.bindings.runningSafe', { count: preview.runningExecutions })}
                </Typography>
              </Alert>
            )}
          </Stack>
        </DialogContent>
        <DialogActions>
          <Button onClick={() => setMode(null)}>{translate('ra.action.cancel')}</Button>
          <Button
            variant="contained"
            disabled={busy || !canSubmit}
            data-testid="binding-submit"
            onClick={submit}
            sx={mode === 'end' ? { background: jvTokens.warn } : undefined}
          >
            {translate(`energy.bindings.confirm.${mode ?? 'create'}`)}
          </Button>
        </DialogActions>
      </Dialog>
    </>
  );
};

export default BindingActions;
