import * as React from 'react';
import {
  Box, Card, CardContent, Typography, Stack, Button, TextField,
} from '@mui/material';
import { energyHttp, energyBase } from '@/api/energy';
import { useNotify, useTranslate } from 'react-admin';
import { apiErrorCode } from '../../../dataProvider/apiError';

/** 卡 20261126-0214：案場層 EasyFlow 預設值（`data.easyflowDefaults`）。
 *  後端只收 9 個 key；`expectedReceiptDays` 是 0～365 的整數，其餘是字串（上限 200 字）。
 *  403（沒有 master:manage）或讀取失敗時整張卡不顯示——不佔案場詳情頁版面。 */
const FIELDS: Array<{ key: string; i18n: string }> = [
  { key: 'companyName', i18n: 'energy.easyflowDefaults.companyName' },
  { key: 'companyCode', i18n: 'energy.easyflowDefaults.companyCode' },
  { key: 'applicant', i18n: 'energy.easyflowDefaults.applicant' },
  { key: 'department', i18n: 'energy.easyflowDefaults.department' },
  { key: 'expectedReceiptDays', i18n: 'energy.easyflowDefaults.expectedReceiptDays' },
  { key: 'paymentMethod', i18n: 'energy.easyflowDefaults.paymentMethod' },
  { key: 'paymentTerms', i18n: 'energy.easyflowDefaults.paymentTerms' },
  { key: 'voucherStatus', i18n: 'energy.easyflowDefaults.voucherStatus' },
  { key: 'contractOption', i18n: 'energy.easyflowDefaults.contractOption' },
];

const EasyFlowDefaultsSection: React.FC<{ orgId?: number | null; stationId: number }> = ({ orgId, stationId }) => {
  const notify = useNotify();
  const translate = useTranslate();
  const base = energyBase(orgId);
  const [values, setValues] = React.useState<Record<string, string>>({});
  const [canManage, setCanManage] = React.useState(false);
  const [visible, setVisible] = React.useState(false);
  const [saving, setSaving] = React.useState(false);
  const endpoint = `${base}/stations/${stationId}/easyflow-defaults`;

  const load = React.useCallback(async () => {
    if (!orgId) return;
    try {
      const { data } = await energyHttp.get(endpoint);
      setValues(Object.fromEntries(FIELDS.map((f) => [f.key, data.easyflowDefaults?.[f.key] != null ? String(data.easyflowDefaults[f.key]) : ''])));
      setCanManage(!!data.canManage);
      setVisible(true);
    } catch {
      // 403／讀取失敗：這張卡不是必要區塊，不擋案場詳情頁其他部分。
      setVisible(false);
    }
  }, [orgId, endpoint]);
  React.useEffect(() => { load(); }, [load]);

  const save = async () => {
    setSaving(true);
    try {
      const body: Record<string, string | number | null> = {};
      for (const f of FIELDS) {
        const raw = values[f.key] ?? '';
        if (f.key === 'expectedReceiptDays') {
          body[f.key] = raw.trim() === '' ? null : Number(raw);
        } else {
          body[f.key] = raw.trim() === '' ? null : raw;
        }
      }
      const { data } = await energyHttp.put(endpoint, body);
      setValues(Object.fromEntries(FIELDS.map((f) => [f.key, data.easyflowDefaults?.[f.key] != null ? String(data.easyflowDefaults[f.key]) : ''])));
      notify('ra.notification.updated', { type: 'info', messageArgs: { smart_count: 1 } });
    } catch (e: any) {
      notify(apiErrorCode(e) || e?.message || 'save failed', { type: 'error' });
    } finally {
      setSaving(false);
    }
  };

  if (!visible) return null;
  return (
    <Card sx={{ mb: 2 }} data-testid="easyflow-defaults-section">
      <CardContent>
        <Typography variant="subtitle1" mb={1}>{translate('energy.easyflowDefaults.title')}</Typography>
        <Box sx={{ display: 'grid', gridTemplateColumns: { xs: '1fr', sm: '1fr 1fr', md: '1fr 1fr 1fr' }, gap: 1.5 }}>
          {FIELDS.map((f) => (
            <TextField
              key={f.key}
              label={translate(f.i18n)}
              value={values[f.key] ?? ''}
              disabled={!canManage || saving}
              type={f.key === 'expectedReceiptDays' ? 'number' : 'text'}
              inputProps={f.key === 'expectedReceiptDays' ? { min: 0, max: 365 } : undefined}
              onChange={(e) => setValues((v) => ({ ...v, [f.key]: e.target.value }))}
              data-testid={`easyflow-defaults-${f.key}`}
              size="small"
              fullWidth
            />
          ))}
        </Box>
        {canManage && (
          <Stack direction="row" justifyContent="flex-end" mt={2}>
            <Button variant="contained" onClick={save} disabled={saving} data-testid="easyflow-defaults-save">
              {translate('ra.action.save')}
            </Button>
          </Stack>
        )}
      </CardContent>
    </Card>
  );
};

export default EasyFlowDefaultsSection;
