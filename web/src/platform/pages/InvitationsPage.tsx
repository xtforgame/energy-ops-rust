// 邀請：組織或目前專案的入口；選 preset 送出、列表、撤銷；建立時拿到一次 token，給出可複製的接受連結。
import * as React from 'react';
import {
  Alert, Box, Button, Card, Chip, Dialog, DialogActions, DialogContent, DialogTitle, IconButton, MenuItem,
  Stack, Table, TableBody, TableCell, TableHead, TableRow, TextField, ToggleButton, ToggleButtonGroup, Tooltip,
  Typography,
} from '@mui/material';
import SendIcon from '@mui/icons-material/Send';
import CancelIcon from '@mui/icons-material/Cancel';
import ContentCopyIcon from '@mui/icons-material/ContentCopy';
import { useNotify, useTranslate } from 'react-admin';
import { invitationApi, orgApi, projectApi } from '../../api/platform';
import type { Id, Invitation, InvitePreset } from '../../api/types';
import { roleDisplayName } from '../../admin/domain-logic/components/energyOps/roleDisplay';
import { Selection } from '../selection/store';
import {
  ConfirmDialog, ErrorAlert, fmtDate, Loading, PageHeader, RequireSelection, useAction, useAsync,
} from './common';

type Scope = 'organization' | 'project';

const statusColor = (s: string): 'warning' | 'success' | 'default' => (
  s === 'pending' ? 'warning' : s === 'accepted' ? 'success' : 'default'
);

/** 接受邀請頁的網址（hash 路由）。 */
export const acceptLink = (token: string) => {
  const { origin, pathname } = window.location;
  return `${origin}${pathname}#/accept-invitation?token=${encodeURIComponent(token)}`;
};

export const usePresets = () => useAsync(() => invitationApi.presets().then((r) => r.items), []);

export const presetLabel = (translate: (k: string, o?: any) => string, presets: InvitePreset[] | undefined, key: string) => {
  const p = presets?.find((x) => x.key === key);
  return p ? translate(p.labelKey, { _: key }) : key;
};

const InviteDialog: React.FC<{
  open: boolean;
  scope: Scope;
  sel: Selection;
  presets: InvitePreset[];
  onClose: () => void;
  onCreated: (inv: Invitation) => void;
}> = ({ open, scope, sel, presets, onClose, onCreated }) => {
  const translate = useTranslate();
  const run = useAction();
  const available = presets.filter((p) => p.scope === scope);
  const [email, setEmail] = React.useState('');
  const [preset, setPreset] = React.useState('');
  const [busy, setBusy] = React.useState(false);
  React.useEffect(() => {
    if (open) {
      setEmail('');
      setPreset(available.find((p) => !p.grantRoleUniqueNames.length)?.key || available[0]?.key || '');
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, scope]);
  const chosen = available.find((p) => p.key === preset);
  const submit = async () => {
    setBusy(true);
    let created: Invitation | null = null;
    const input = { email: email.trim(), inviteRoleKey: preset };
    const ok = await run(async () => {
      created = scope === 'project' && sel.projectId
        ? await projectApi.invite(sel.projectId, input)
        : await orgApi.invite(sel.organizationId, input);
    }, 'eo.invitations.sent');
    setBusy(false);
    if (ok && created) onCreated(created);
  };
  return (
    <Dialog open={open} onClose={onClose} fullWidth maxWidth="sm">
      <DialogTitle>
        {translate(scope === 'project' ? 'tc.invite.titleProject' : 'tc.invite.titleOrg')}
        {' — '}
        {scope === 'project' ? sel.projectName : sel.organizationName}
      </DialogTitle>
      <DialogContent>
        <TextField
          fullWidth
          margin="dense"
          type="email"
          label="Email"
          helperText={translate('tc.invite.emailHelp')}
          value={email}
          onChange={(e) => setEmail(e.target.value)}
          inputProps={{ 'data-testid': 'invite-email' }}
        />
        <TextField
          select
          fullWidth
          margin="dense"
          label={translate('tc.invite.role')}
          value={preset}
          onChange={(e) => setPreset(e.target.value)}
          SelectProps={{ SelectDisplayProps: { 'data-testid': 'invite-preset' } as any }}
        >
          {available.map((p) => (
            <MenuItem key={p.key} value={p.key} data-testid={`invite-preset-${p.key}`}>
              {translate(p.labelKey, { _: p.key })}
            </MenuItem>
          ))}
        </TextField>
        {chosen && (
          <Box mt={1}>
            <Typography variant="caption" color="text.secondary">{translate('tc.invite.willGrant')}：</Typography>
            {chosen.grantRoleUniqueNames.length
              ? chosen.grantRoleUniqueNames.map((r) => <Chip key={r} size="small" label={roleDisplayName(r)} sx={{ ml: 0.5 }} />)
              : <Typography variant="caption" color="text.secondary">{translate('tc.invite.noExtra')}</Typography>}
          </Box>
        )}
      </DialogContent>
      <DialogActions>
        <Button onClick={onClose}>{translate('ra.action.cancel')}</Button>
        <Button
          variant="contained"
          disabled={busy || !email.trim() || !preset}
          onClick={submit}
          data-testid="invite-submit"
        >
          {translate('tc.invite.send')}
        </Button>
      </DialogActions>
    </Dialog>
  );
};

const CreatedLinkDialog: React.FC<{ invitation: Invitation | null; onClose: () => void }> = ({ invitation, onClose }) => {
  const translate = useTranslate();
  const notify = useNotify();
  const link = invitation?.token ? acceptLink(invitation.token) : '';
  return (
    <Dialog open={!!invitation} onClose={onClose} fullWidth maxWidth="sm">
      <DialogTitle>{translate('eo.invitations.createdTitle')}</DialogTitle>
      <DialogContent>
        <Alert severity="info" sx={{ mb: 2 }}>{translate('eo.invitations.linkOnce')}</Alert>
        <TextField fullWidth value={link} InputProps={{ readOnly: true }} inputProps={{ 'data-testid': 'invite-link' }} />
      </DialogContent>
      <DialogActions>
        <Button
          startIcon={<ContentCopyIcon />}
          onClick={async () => {
            try {
              await navigator.clipboard.writeText(link);
              notify('tc.invite.linkCopied', { type: 'success' });
            } catch {
              notify(link, { type: 'info', autoHideDuration: 12000 });
            }
          }}
        >
          {translate('tc.invite.copyLink')}
        </Button>
        <Button variant="contained" onClick={onClose}>{translate('ra.action.close')}</Button>
      </DialogActions>
    </Dialog>
  );
};

const Invitations: React.FC<{ sel: Selection }> = ({ sel }) => {
  const translate = useTranslate();
  const run = useAction();
  const canProject = !!sel.projectId && !sel.projectIsDefault;
  const [scope, setScope] = React.useState<Scope>('organization');
  const [showAll, setShowAll] = React.useState(false);
  const presets = usePresets();
  const effectiveScope: Scope = canProject ? scope : 'organization';
  const list = useAsync(
    () => (effectiveScope === 'project' && sel.projectId
      ? projectApi.invitations(sel.projectId)
      : orgApi.invitations(sel.organizationId)),
    [effectiveScope, sel.organizationId, sel.projectId],
  );
  const [inviteOpen, setInviteOpen] = React.useState(false);
  const [created, setCreated] = React.useState<Invitation | null>(null);
  const [revoking, setRevoking] = React.useState<Invitation | null>(null);
  const items = (list.data?.items || []).filter((i) => showAll || i.status === 'pending');
  const personal = sel.organizationKind === 'personal';
  return (
    <Box p={2}>
      <PageHeader
        title={translate('eo.invitations.title')}
        subtitle={effectiveScope === 'project' ? `${sel.organizationName} / ${sel.projectName}` : sel.organizationName}
        onRefresh={list.reload}
        actions={!personal && (
          <Button variant="contained" startIcon={<SendIcon />} onClick={() => setInviteOpen(true)} data-testid="invite-open">
            {translate('tc.invite.send')}
          </Button>
        )}
      />
      {personal && <Alert severity="info" sx={{ mb: 2 }}>{translate('eo.invitations.personalOrg')}</Alert>}
      <Stack direction="row" gap={2} mb={2} alignItems="center">
        {canProject && (
          <ToggleButtonGroup size="small" exclusive value={effectiveScope} onChange={(_e, v) => v && setScope(v)}>
            <ToggleButton value="organization">{translate('eo.common.organization')}</ToggleButton>
            <ToggleButton value="project">{translate('eo.common.project')}</ToggleButton>
          </ToggleButtonGroup>
        )}
        <ToggleButtonGroup size="small" exclusive value={showAll ? 'all' : 'pending'} onChange={(_e, v) => v && setShowAll(v === 'all')}>
          <ToggleButton value="pending">{translate('tc.invite.pending')}</ToggleButton>
          <ToggleButton value="all">{translate('eo.common.all')}</ToggleButton>
        </ToggleButtonGroup>
      </Stack>
      <ErrorAlert error={list.error || presets.error} />
      {list.loading && <Loading />}
      {list.data && (
        <Card>
          <Table size="small" data-testid="invitations-table">
            <TableHead>
              <TableRow>
                <TableCell>{translate('tc.invite.invitee')}</TableCell>
                <TableCell>{translate('tc.invite.role')}</TableCell>
                <TableCell>{translate('eo.common.scope')}</TableCell>
                <TableCell>{translate('tc.invite.inviter')}</TableCell>
                <TableCell>{translate('tc.invite.status')}</TableCell>
                <TableCell>{translate('eo.invitations.expiresAt')}</TableCell>
                <TableCell align="right" />
              </TableRow>
            </TableHead>
            <TableBody>
              {items.map((inv) => (
                <TableRow key={inv.id} data-testid={`invitation-row-${inv.id}`}>
                  <TableCell>{inv.inviteeEmail}</TableCell>
                  <TableCell>{presetLabel(translate, presets.data, inv.inviteRoleKey)}</TableCell>
                  <TableCell>{inv.projectName || inv.organizationName}</TableCell>
                  <TableCell>{inv.inviterName}</TableCell>
                  <TableCell><Chip size="small" color={statusColor(inv.status)} label={translate(`eo.invitationStatus.${inv.status}`, { _: inv.status })} /></TableCell>
                  <TableCell>{fmtDate(inv.expiresAt)}</TableCell>
                  <TableCell align="right">
                    {inv.status === 'pending' && (
                      <Tooltip title={translate('tc.invite.revoke')}>
                        <IconButton size="small" onClick={() => setRevoking(inv)} data-testid={`invitation-revoke-${inv.id}`}>
                          <CancelIcon fontSize="small" />
                        </IconButton>
                      </Tooltip>
                    )}
                  </TableCell>
                </TableRow>
              ))}
              {!items.length && (
                <TableRow><TableCell colSpan={7}><Typography variant="body2" color="text.secondary">{translate('tc.invite.noPending')}</Typography></TableCell></TableRow>
              )}
            </TableBody>
          </Table>
        </Card>
      )}
      {presets.data && (
        <InviteDialog
          open={inviteOpen}
          scope={effectiveScope}
          sel={sel}
          presets={presets.data}
          onClose={() => setInviteOpen(false)}
          onCreated={(inv) => {
            setInviteOpen(false);
            setCreated(inv);
            list.reload();
          }}
        />
      )}
      <CreatedLinkDialog invitation={created} onClose={() => setCreated(null)} />
      <ConfirmDialog
        open={!!revoking}
        danger
        title={translate('eo.invitations.revokeTitle', { email: revoking?.inviteeEmail || '' })}
        onClose={() => setRevoking(null)}
        onConfirm={async () => {
          if (!revoking) return;
          const ok = await run(() => invitationApi.revoke(revoking.id as Id), 'tc.invite.revoked');
          if (ok) {
            setRevoking(null);
            list.reload();
          }
        }}
      />
    </Box>
  );
};

const InvitationsPage: React.FC = () => (
  <RequireSelection>{(sel) => <Invitations key={`${sel.organizationId}:${sel.projectId}`} sel={sel} />}</RequireSelection>
);

export default InvitationsPage;
