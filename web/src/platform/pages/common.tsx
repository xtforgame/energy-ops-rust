// 平台頁共用的小元件與 hook：載入狀態、錯誤顯示、確認框、需要先選組織。
import * as React from 'react';
import {
  Alert, Box, Button, CircularProgress, Dialog, DialogActions, DialogContent, DialogTitle, IconButton, Stack,
  TextField, Tooltip, Typography,
} from '@mui/material';
import RefreshIcon from '@mui/icons-material/Refresh';
import { Title, useNotify, useTranslate } from 'react-admin';
import { errorMessage, isApiError } from '../../api/http';
import { Selection, useSelection } from '../selection/store';

export interface AsyncState<T> {
  data: T | undefined;
  error: unknown;
  loading: boolean;
  reload: () => void;
}

/** 依 deps 載入資料；deps 有 null/undefined 時不載。 */
export function useAsync<T>(fn: () => Promise<T>, deps: React.DependencyList, enabled = true): AsyncState<T> {
  const [state, setState] = React.useState<{ data?: T; error: unknown; loading: boolean }>({ error: null, loading: enabled });
  const [tick, setTick] = React.useState(0);
  React.useEffect(() => {
    if (!enabled) {
      setState({ data: undefined, error: null, loading: false });
      return undefined;
    }
    let alive = true;
    setState((s) => ({ ...s, loading: true, error: null }));
    fn().then(
      (data) => alive && setState({ data, error: null, loading: false }),
      (error) => alive && setState({ data: undefined, error, loading: false }),
    );
    return () => {
      alive = false;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [...deps, tick, enabled]);
  return { ...state, data: state.data, reload: () => setTick((t) => t + 1) };
}

/** 錯誤碼 → 畫面訊息：eo.error.<code> 有翻譯就用，否則顯示後端 message。 */
export function useErrorText() {
  const translate = useTranslate();
  return React.useCallback((e: unknown) => {
    if (isApiError(e)) {
      const key = `eo.error.${e.code}`;
      const t = translate(key, { _: '' });
      if (t && t !== key) return t;
    }
    return errorMessage(e);
  }, [translate]);
}

/** 執行一個動作：成功顯示訊息、失敗顯示錯誤；回傳是否成功。 */
export function useAction() {
  const notify = useNotify();
  const errorText = useErrorText();
  return React.useCallback(async (fn: () => Promise<unknown>, successKey?: string) => {
    try {
      await fn();
      if (successKey) notify(successKey, { type: 'success' });
      return true;
    } catch (e) {
      notify(errorText(e), { type: 'error' });
      return false;
    }
  }, [notify, errorText]);
}

export const ErrorAlert: React.FC<{ error: unknown }> = ({ error }) => {
  const errorText = useErrorText();
  const translate = useTranslate();
  if (!error) return null;
  const denied = isApiError(error) && error.status === 403;
  return (
    <Alert severity={denied ? 'warning' : 'error'} sx={{ mb: 2 }} data-testid="page-error">
      {denied ? translate('eo.common.noAccess') : errorText(error)}
    </Alert>
  );
};

export const Loading: React.FC = () => (
  <Box display="flex" justifyContent="center" p={3}><CircularProgress size={26} /></Box>
);

export const PageHeader: React.FC<{
  title: string;
  subtitle?: string | null;
  onRefresh?: () => void;
  actions?: React.ReactNode;
}> = ({ title, subtitle, onRefresh, actions }) => {
  const translate = useTranslate();
  return (
    <Stack direction="row" alignItems="center" justifyContent="space-between" mb={2} mt={1} gap={2} flexWrap="wrap">
      <Title title={title} />
      <Box>
        <Typography variant="h6" data-testid="page-title">{title}</Typography>
        {subtitle && <Typography variant="body2" color="text.secondary">{subtitle}</Typography>}
      </Box>
      <Stack direction="row" gap={1} alignItems="center">
        {onRefresh && (
          <Tooltip title={translate('ra.action.refresh')}>
            <IconButton onClick={onRefresh}><RefreshIcon /></IconButton>
          </Tooltip>
        )}
        {actions}
      </Stack>
    </Stack>
  );
};

/** 需要先選組織的頁面：沒選時顯示提示。 */
export const RequireSelection: React.FC<{
  needProject?: boolean;
  children: (sel: Selection) => React.ReactNode;
}> = ({ needProject, children }) => {
  const sel = useSelection();
  const translate = useTranslate();
  if (!sel) return <Box p={3}><Alert severity="info">{translate('eo.common.selectOrganization')}</Alert></Box>;
  if (needProject && !sel.projectId) {
    return <Box p={3}><Alert severity="info">{translate('eo.common.selectProject')}</Alert></Box>;
  }
  return <>{children(sel)}</>;
};

export const ConfirmDialog: React.FC<{
  open: boolean;
  title: string;
  content?: React.ReactNode;
  confirmLabel?: string;
  /** 要求填原因（撤銷授權）時給 label。 */
  reasonLabel?: string;
  danger?: boolean;
  onClose: () => void;
  onConfirm: (reason: string) => Promise<unknown> | void;
}> = ({ open, title, content, confirmLabel, reasonLabel, danger, onClose, onConfirm }) => {
  const translate = useTranslate();
  const [reason, setReason] = React.useState('');
  const [busy, setBusy] = React.useState(false);
  React.useEffect(() => {
    if (open) setReason('');
  }, [open]);
  return (
    <Dialog open={open} onClose={busy ? undefined : onClose} fullWidth maxWidth="xs">
      <DialogTitle>{title}</DialogTitle>
      <DialogContent>
        {content}
        {reasonLabel && (
          <TextField
            fullWidth
            margin="dense"
            label={reasonLabel}
            value={reason}
            onChange={(e) => setReason(e.target.value)}
            inputProps={{ 'data-testid': 'confirm-reason' }}
          />
        )}
      </DialogContent>
      <DialogActions>
        <Button onClick={onClose} disabled={busy}>{translate('ra.action.cancel')}</Button>
        <Button
          variant="contained"
          color={danger ? 'error' : 'primary'}
          disabled={busy}
          data-testid="confirm-ok"
          onClick={async () => {
            setBusy(true);
            try {
              await onConfirm(reason.trim());
            } finally {
              setBusy(false);
            }
          }}
        >
          {confirmLabel || translate('ra.action.confirm')}
        </Button>
      </DialogActions>
    </Dialog>
  );
};

/** 日期時間的簡短顯示。 */
export const fmtDate = (iso?: string | null) => {
  if (!iso) return '—';
  const d = new Date(iso);
  return Number.isNaN(d.getTime()) ? iso : d.toLocaleString();
};

/** 受控表單欄位的小工具。 */
export function useFields<T extends Record<string, string>>(initial: T) {
  const [values, setValues] = React.useState<T>(initial);
  const bind = (k: keyof T) => ({
    value: values[k],
    onChange: (e: React.ChangeEvent<HTMLInputElement>) => setValues((v) => ({ ...v, [k]: e.target.value })),
  });
  return { values, setValues, bind, reset: () => setValues(initial) };
}
