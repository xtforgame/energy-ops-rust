// 成員頁的「管理權限」：替一位成員指派能源角色。選項只有後端 `assignable-roles` 給的五個中文具名角色，
// 不露出平台角色語彙；沒有特權指派權的人看得到特權角色但選不下去（真正的閘在後端）。
// 角色一律在組織層授予。
import * as React from 'react';
import {
  Alert, Box, Button, Chip, CircularProgress, Dialog, DialogActions, DialogContent, DialogTitle, MenuItem, Stack,
  TextField, Typography,
} from '@mui/material';
import { useNotify, useTranslate } from 'react-admin';
import { energyHttp, energyPrefix } from '../../api/energy';
import type { Id } from '../../api/types';

export interface AssignableRole {
  id: number;
  uniqueName: string;
  name: string;
  description: string;
  privileged: boolean;
}

export interface AssignableRoles {
  items: AssignableRole[];
  canAssignPrivileged: boolean;
}

/** 這個組織能不能指派能源角色（沒有能源權限或不是能源組織時回 null）。 */
export async function fetchAssignableRoles(orgId: Id): Promise<AssignableRoles | null> {
  try {
    const { data } = await energyHttp.get(`${energyPrefix(orgId)}/assignable-roles`);
    const items = Array.isArray(data?.items) ? data.items : [];
    return items.length ? { items, canAssignPrivileged: !!data?.canAssignPrivileged } : null;
  } catch {
    return null;
  }
}

const errorCodeOf = (e: any): string => e?.response?.data?.code || e?.code || '';

export const ManageEnergyRolesDialog: React.FC<{
  open: boolean;
  onClose: () => void;
  orgId: Id;
  orgName?: string | null;
  roles: AssignableRoles;
  granteeUserId: Id;
  granteeName?: string | null;
  onDone?: () => void;
}> = ({ open, onClose, orgId, orgName, roles, granteeUserId, granteeName, onDone }) => {
  const notify = useNotify();
  const translate = useTranslate();
  const [roleUniqueName, setRoleUniqueName] = React.useState('');
  const [reason, setReason] = React.useState('');
  const [submitting, setSubmitting] = React.useState(false);
  const [error, setError] = React.useState<string | null>(null);

  React.useEffect(() => {
    if (open) { setRoleUniqueName(''); setReason(''); setError(null); }
  }, [open]);

  const isDisabled = (r: AssignableRole) => r.privileged && !roles.canAssignPrivileged;

  const submit = async () => {
    if (!roleUniqueName) { setError(translate('tc.grant.pickRole')); return; }
    setSubmitting(true);
    setError(null);
    try {
      await energyHttp.post(`${energyPrefix(orgId)}/role-assignments`, {
        userId: Number(granteeUserId),
        roleUniqueName,
        ...(reason.trim() ? { reason: reason.trim() } : {}),
      });
      notify('tc.grant.granted', { type: 'success' });
      onDone?.();
      onClose();
    } catch (e: any) {
      const code = errorCodeOf(e);
      const friendly: Record<string, string> = {
        cannot_assign_privileged_role: translate('tc.grant.cannotAssignPrivileged'),
        unknown_role: translate('tc.grant.unknownRole'),
      };
      setError(friendly[code] || e?.response?.data?.message || code || String(e?.message || e));
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <Dialog open={open} onClose={submitting ? undefined : onClose} fullWidth maxWidth="sm" data-testid="manage-roles-dialog">
      <DialogTitle>{translate('tc.grant.title', { name: granteeName || `#${granteeUserId}` })}</DialogTitle>
      <DialogContent>
        <Box display="flex" flexDirection="column" gap={2} mt={1}>
          <Box>
            <Typography variant="caption" color="text.secondary">{translate('tc.grant.scope')}</Typography>
            <Stack direction="row" gap={1} mt={0.5}>
              <Chip label={orgName || String(orgId)} />
            </Stack>
            <Typography variant="caption" color="text.secondary" display="block" mt={0.5}>
              {translate('tc.grant.orgWideNote')}
            </Typography>
          </Box>
          <TextField
            select
            label={translate('tc.invite.role')}
            value={roleUniqueName}
            onChange={(e) => setRoleUniqueName(e.target.value)}
            disabled={submitting}
            helperText={translate('tc.grant.roleHelp', { count: roles.items.length })}
            SelectProps={{ SelectDisplayProps: { 'data-testid': 'manage-roles-role' } as any }}
          >
            {roles.items.map((r) => (
              <MenuItem key={r.uniqueName} value={r.uniqueName} disabled={isDisabled(r)} data-testid={`manage-roles-option-${r.id}`}>
                <Box>
                  <Typography variant="body2">{r.name}</Typography>
                  <Typography variant="caption" color="text.secondary">
                    {isDisabled(r) ? translate('tc.grant.privilegedLocked') : r.description}
                  </Typography>
                </Box>
              </MenuItem>
            ))}
          </TextField>
          <TextField
            label={translate('tc.grant.reason')}
            value={reason}
            onChange={(e) => setReason(e.target.value)}
            disabled={submitting}
            multiline
            minRows={2}
            helperText={translate('tc.grant.reasonHelp')}
          />
          {error && <Alert severity="error">{error}</Alert>}
        </Box>
      </DialogContent>
      <DialogActions>
        <Button onClick={onClose} disabled={submitting}>{translate('ra.action.cancel')}</Button>
        <Button
          variant="contained"
          onClick={submit}
          disabled={submitting}
          data-testid="manage-roles-submit"
          startIcon={submitting ? <CircularProgress size={16} /> : undefined}
        >
          {translate('tc.grant.grant')}
        </Button>
      </DialogActions>
    </Dialog>
  );
};

export default ManageEnergyRolesDialog;
