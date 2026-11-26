import * as React from 'react';
import {
  Dialog, DialogTitle, DialogContent, DialogActions, Stack, Button, Typography, Table, TableHead,
  TableBody, TableRow, TableCell, Chip, ToggleButtonGroup, ToggleButton,
} from '@mui/material';
import { useNotify, useTranslate } from 'react-admin';
import { energyHttp } from '@/api/energy';
import { apiErrorCode } from '../../../dataProvider/apiError';

interface RowResult {
  row: number;
  dataId: string;
  station: string;
  skipped: boolean;
  pending: string[];
  stationAction?: string;
  contractAction?: string;
  landowners?: Array<{ name: string; action: string }>;
}

interface ImportResult {
  dryRun: boolean;
  planHash: string;
  summary: {
    stations: { created: number; updated: number; unchanged: number; pending: number };
    landowners: { created: number; updated: number; unchanged: number; pending: number };
    contracts: { created: number; updated: number; unchanged: number; pending: number };
  };
  rows: RowResult[];
}

interface Props {
  open: boolean;
  onClose: () => void;
  base: string;
  onDone: () => void;
}

/**
 * 卡 20261126-2300：核心大表匯入正式資料。選檔 → 預覽（只試算、不寫入，即使用者說的「測試模式」）
 * → 可以只看更新／只看待補 → 下載待補 CSV → 確認寫入（planHash 對不上伺服器會回 409）。
 */
const CoreTableImportDialog: React.FC<Props> = ({ open, onClose, base, onDone }) => {
  const notify = useNotify();
  const translate = useTranslate();
  const [file, setFile] = React.useState<File | null>(null);
  const [result, setResult] = React.useState<ImportResult | null>(null);
  const [filter, setFilter] = React.useState<'all' | 'updated' | 'pending'>('all');
  const [busy, setBusy] = React.useState(false);

  const reset = () => { setFile(null); setResult(null); setFilter('all'); };

  const run = async (dryRun: boolean) => {
    if (!file) return;
    setBusy(true);
    try {
      const data = new FormData();
      data.append('file', file, file.name);
      data.append('dryRun', dryRun ? '1' : '0');
      if (result?.planHash) data.append('planHash', result.planHash);
      const { data: out } = await energyHttp.post(`${base}/imports/core-table`, data);
      setResult(out);
      if (!dryRun) {
        notify('ra.notification.updated', { type: 'info', messageArgs: { smart_count: 1 } });
        onDone();
      }
    } catch (e: any) {
      if (apiErrorCode(e) === 'plan_stale') {
        notify(translate('energy.coreTableImport.stale'), { type: 'warning' });
        setResult(null);
      } else {
        notify(apiErrorCode(e) || e?.message || 'import failed', { type: 'error' });
      }
    } finally {
      setBusy(false);
    }
  };

  const visibleRows = (result?.rows ?? []).filter((r) => {
    if (filter === 'updated') return r.stationAction === 'update' || r.contractAction === 'update';
    if (filter === 'pending') return r.pending.length > 0;
    return true;
  });

  const downloadPendingCsv = () => {
    const pendingRows = (result?.rows ?? []).filter((r) => r.pending.length > 0);
    if (!pendingRows.length) return;
    const esc = (s: string) => `"${String(s ?? '').replace(/"/g, '""')}"`;
    const lines = [
      ['row', 'dataId', 'station', 'pending'].map(esc).join(','),
      ...pendingRows.map((r) => [r.row, r.dataId, r.station, r.pending.join('；')].map(esc).join(',')),
    ];
    const csv = `﻿${lines.join('\r\n')}`;
    const blob = new Blob([csv], { type: 'text/csv;charset=utf-8;' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = 'core-table-import-pending.csv';
    a.click();
    URL.revokeObjectURL(url);
  };

  return (
    <Dialog open={open} onClose={() => { onClose(); reset(); }} fullWidth maxWidth="md" data-testid="core-table-import-dialog">
      <DialogTitle>{translate('energy.coreTableImport.title')}</DialogTitle>
      <DialogContent>
        <Stack gap={2} mt={1}>
          <Button variant="outlined" component="label" data-testid="core-table-import-choose">
            {file ? file.name : translate('energy.coreTableImport.chooseFile')}
            <input
              type="file"
              hidden
              accept=".xlsx"
              onChange={(e) => { setFile(e.target.files?.[0] ?? null); setResult(null); }}
            />
          </Button>

          {result && (
            <>
              <Stack direction="row" gap={2} data-testid="core-table-import-summary">
                {(['stations', 'landowners', 'contracts'] as const).map((k) => (
                  <Typography key={k} variant="body2">
                    {translate(`energy.coreTableImport.${k}`)}：
                    {translate('energy.coreTableImport.created')} {result.summary[k].created} ·
                    {' '}{translate('energy.coreTableImport.updated')} {result.summary[k].updated} ·
                    {' '}{translate('energy.coreTableImport.unchanged')} {result.summary[k].unchanged} ·
                    {' '}{translate('energy.coreTableImport.pending')} {result.summary[k].pending}
                  </Typography>
                ))}
              </Stack>

              <ToggleButtonGroup size="small" exclusive value={filter} onChange={(_, v) => v && setFilter(v)}>
                <ToggleButton value="all" data-testid="core-table-import-filter-all">{translate('energy.coreTableImport.filterAll')}</ToggleButton>
                <ToggleButton value="updated" data-testid="core-table-import-filter-updated">{translate('energy.coreTableImport.filterUpdated')}</ToggleButton>
                <ToggleButton value="pending" data-testid="core-table-import-filter-pending">{translate('energy.coreTableImport.filterPending')}</ToggleButton>
              </ToggleButtonGroup>

              <Table size="small" data-testid="core-table-import-rows">
                <TableHead>
                  <TableRow>
                    <TableCell>{translate('energy.coreTableImport.row')}</TableCell>
                    <TableCell>{translate('energy.coreTableImport.station')}</TableCell>
                    <TableCell>{translate('energy.coreTableImport.stations')}</TableCell>
                    <TableCell>{translate('energy.coreTableImport.contracts')}</TableCell>
                    <TableCell>{translate('energy.coreTableImport.pending')}</TableCell>
                  </TableRow>
                </TableHead>
                <TableBody>
                  {visibleRows.map((r) => (
                    <TableRow key={r.row} data-testid="core-table-import-row">
                      <TableCell>{r.row}</TableCell>
                      <TableCell>{r.station}</TableCell>
                      <TableCell>{r.skipped ? '—' : r.stationAction}</TableCell>
                      <TableCell>{r.skipped ? '—' : r.contractAction}</TableCell>
                      <TableCell>
                        {r.pending.map((p) => <Chip key={p} size="small" label={p} sx={{ mr: 0.5 }} />)}
                      </TableCell>
                    </TableRow>
                  ))}
                </TableBody>
              </Table>

              <Button size="small" onClick={downloadPendingCsv} data-testid="core-table-import-download-pending">
                {translate('energy.coreTableImport.downloadPending')}
              </Button>
            </>
          )}
        </Stack>
      </DialogContent>
      <DialogActions>
        <Button onClick={() => { onClose(); reset(); }}>{translate('ra.action.cancel')}</Button>
        <Button onClick={() => run(true)} disabled={!file || busy} data-testid="core-table-import-preview">
          {translate('energy.coreTableImport.preview')}
        </Button>
        <Button variant="contained" onClick={() => run(false)} disabled={!file || busy || !result} data-testid="core-table-import-confirm">
          {translate('energy.coreTableImport.confirm')}
        </Button>
      </DialogActions>
    </Dialog>
  );
};

export default CoreTableImportDialog;
