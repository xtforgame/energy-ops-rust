import * as React from 'react';
import {
  Dialog, DialogTitle, DialogContent, DialogActions, Stack, TextField, Button, MenuItem,
  Select, InputLabel, FormControl, Typography, Alert, Divider, ToggleButtonGroup, ToggleButton, IconButton,
} from '@mui/material';
import DeleteIcon from '@mui/icons-material/Delete';
import AddIcon from '@mui/icons-material/Add';
import { useNotify, useTranslate } from 'react-admin';
import { energyHttp } from '@/api/energy';
import { apiErrorCode } from '../../../dataProvider/apiError';

interface LandownerOption { id: number; name: string }

interface LandownerEntry {
  mode: 'existing' | 'new';
  existingId: string;
  newName: string;
  ratio: string;
}

const emptyEntry = (): LandownerEntry => ({ mode: 'existing', existingId: '', newName: '', ratio: '' });

interface OnboardDialogProps {
  open: boolean;
  onClose: () => void;
  base: string;
  landowners: LandownerOption[];
  onCreated: () => void;
}

/**
 * 卡 20261124-2316 S3：三合一入口——一次建案場、地主（既有或新增，多位、各填分配比例）、合約。
 * 一次存檔全成或全敗（`POST {base}/onboard`）；案場可以不填電號（沒有電號標籤）。
 */
const OnboardDialog: React.FC<OnboardDialogProps> = ({ open, onClose, base, landowners, onCreated }) => {
  const notify = useNotify();
  const translate = useTranslate();

  const [stationName, setStationName] = React.useState('');
  const [customerNumber, setCustomerNumber] = React.useState('');
  const [entries, setEntries] = React.useState<LandownerEntry[]>([emptyEntry()]);
  const [contractNo, setContractNo] = React.useState('');
  const [validFrom, setValidFrom] = React.useState('');
  const [validTo, setValidTo] = React.useState('');
  const [saving, setSaving] = React.useState(false);
  const [error, setError] = React.useState<string | null>(null);

  React.useEffect(() => {
    if (open) {
      setStationName(''); setCustomerNumber(''); setEntries([emptyEntry()]);
      setContractNo(''); setValidFrom(''); setValidTo(''); setError(null);
    }
  }, [open]);

  const updateEntry = (idx: number, patch: Partial<LandownerEntry>) => {
    setEntries((prev) => prev.map((e, i) => (i === idx ? { ...e, ...patch } : e)));
  };
  const addEntry = () => setEntries((prev) => [...prev, emptyEntry()]);
  const removeEntry = (idx: number) => setEntries((prev) => prev.filter((_, i) => i !== idx));

  const valid = stationName.trim() !== ''
    && entries.length > 0
    && entries.every((e) => (e.mode === 'existing' ? !!e.existingId : e.newName.trim() !== '') && e.ratio !== '')
    && contractNo.trim() !== '' && validFrom !== '';

  const save = async () => {
    if (!valid) return;
    setSaving(true); setError(null);
    try {
      const body = {
        station: { name: stationName, customerNumber: customerNumber || undefined },
        landowners: entries.map((e) => (e.mode === 'existing'
          ? { id: Number(e.existingId), ratio: Number(e.ratio) }
          : { name: e.newName, ratio: Number(e.ratio) })),
        contract: { contractNo, validFrom, validTo: validTo || undefined },
      };
      await energyHttp.post(`${base}/onboard`, body);
      notify('energy.stations.onboardDone', { type: 'success' });
      onCreated();
      onClose();
    } catch (e: any) {
      setError(apiErrorCode(e) || e?.message || 'save failed');
    } finally {
      setSaving(false);
    }
  };

  return (
    <Dialog open={open} onClose={onClose} fullWidth maxWidth="sm" data-testid="onboard-dialog">
      <DialogTitle>{translate('energy.stations.onboardTitle')}</DialogTitle>
      <DialogContent>
        <Stack gap={2} mt={1}>
          <Typography variant="subtitle2">{translate('energy.stations.onboardStep1')}</Typography>
          <TextField
            label={translate('energy.stations.name')} value={stationName}
            onChange={(e) => setStationName(e.target.value)} data-testid="onboard-station-name" required
          />
          <TextField
            label={translate('energy.bills.customerNumber')} value={customerNumber}
            onChange={(e) => setCustomerNumber(e.target.value)} data-testid="onboard-station-customer-number"
            helperText={translate('energy.stations.noMeterHint')}
          />

          <Divider />
          <Typography variant="subtitle2">{translate('energy.stations.onboardStep2')}</Typography>
          {entries.map((entry, idx) => (
            <Stack key={idx} direction="row" gap={1} alignItems="flex-start" data-testid="onboard-landowner-row">
              <ToggleButtonGroup
                size="small"
                exclusive
                value={entry.mode}
                onChange={(_, v) => v && updateEntry(idx, { mode: v })}
              >
                <ToggleButton value="existing" data-testid="onboard-landowner-mode-existing">
                  {translate('energy.stations.onboardExistingLandowner')}
                </ToggleButton>
                <ToggleButton value="new" data-testid="onboard-landowner-mode-new">
                  {translate('energy.stations.onboardNewLandowner')}
                </ToggleButton>
              </ToggleButtonGroup>
              {entry.mode === 'existing' ? (
                <FormControl size="small" sx={{ minWidth: 160 }} data-testid="onboard-landowner-existing-select">
                  <InputLabel>{translate('energy.landowners.name')}</InputLabel>
                  <Select
                    label={translate('energy.landowners.name')}
                    value={entry.existingId}
                    onChange={(e) => updateEntry(idx, { existingId: e.target.value })}
                  >
                    {landowners.map((l) => <MenuItem key={l.id} value={String(l.id)}>{l.name}</MenuItem>)}
                  </Select>
                </FormControl>
              ) : (
                <TextField
                  size="small" label={translate('energy.stations.onboardLandownerName')} value={entry.newName}
                  onChange={(e) => updateEntry(idx, { newName: e.target.value })} data-testid="onboard-landowner-new-name"
                />
              )}
              <TextField
                size="small" label={translate('energy.stations.onboardRatio')} value={entry.ratio}
                onChange={(e) => updateEntry(idx, { ratio: e.target.value })} sx={{ width: 110 }}
                data-testid="onboard-landowner-ratio"
              />
              {entries.length > 1 && (
                <IconButton size="small" onClick={() => removeEntry(idx)} data-testid="onboard-landowner-remove">
                  <DeleteIcon fontSize="small" />
                </IconButton>
              )}
            </Stack>
          ))}
          <Button size="small" startIcon={<AddIcon />} onClick={addEntry} data-testid="onboard-landowner-add" sx={{ alignSelf: 'flex-start' }}>
            {translate('energy.stations.onboardAddLandowner')}
          </Button>

          <Divider />
          <Typography variant="subtitle2">{translate('energy.stations.onboardStep3')}</Typography>
          <TextField
            label={translate('energy.contracts.no')} value={contractNo}
            onChange={(e) => setContractNo(e.target.value)} data-testid="onboard-contract-no" required
          />
          <TextField
            label={translate('energy.stations.validFrom')} type="date" value={validFrom}
            onChange={(e) => setValidFrom(e.target.value)} InputLabelProps={{ shrink: true }}
            data-testid="onboard-contract-validfrom" required
          />
          <TextField
            label={translate('energy.stations.validTo')} type="date" value={validTo}
            onChange={(e) => setValidTo(e.target.value)} InputLabelProps={{ shrink: true }}
            data-testid="onboard-contract-validto"
          />
          {error && <Alert severity="error">{error}</Alert>}
        </Stack>
      </DialogContent>
      <DialogActions>
        <Button onClick={onClose}>{translate('ra.action.cancel')}</Button>
        <Button variant="contained" disabled={saving || !valid} onClick={save} data-testid="onboard-save">
          {translate('ra.action.save')}
        </Button>
      </DialogActions>
    </Dialog>
  );
};

export default OnboardDialog;
