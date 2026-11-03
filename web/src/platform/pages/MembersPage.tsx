// 組織成員：列表（含組織層角色）、加入既有使用者或直接建帳號、移除。
import * as React from 'react';
import {
  Box, Button, Card, Chip, Dialog, DialogActions, DialogContent, DialogTitle, IconButton, Tab, Tabs, Table,
  TableBody, TableCell, TableHead, TableRow, TextField, Tooltip, Typography,
} from '@mui/material';
import PersonAddIcon from '@mui/icons-material/PersonAdd';
import DeleteIcon from '@mui/icons-material/Delete';
import { useGetIdentity, useTranslate } from 'react-admin';
import { orgApi } from '../../api/platform';
import type { Id, OrgMember } from '../../api/types';
import { roleDisplayName } from '../../admin/domain-logic/components/energyOps/roleDisplay';
import {
  ConfirmDialog, ErrorAlert, fmtDate, Loading, PageHeader, RequireSelection, useAction, useAsync, useFields,
} from './common';

const AddMemberDialog: React.FC<{ orgId: Id; open: boolean; onClose: () => void; onDone: () => void }> = ({
  orgId, open, onClose, onDone,
}) => {
  const translate = useTranslate();
  const run = useAction();
  const [mode, setMode] = React.useState<'existing' | 'create'>('create');
  const f = useFields({ userId: '', name: '', email: '', password: '' });
  const [busy, setBusy] = React.useState(false);
  React.useEffect(() => {
    if (open) f.reset();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);
  const v = f.values;
  const valid = mode === 'existing'
    ? Number(v.userId) > 0
    : !!v.name.trim() && !!v.email.trim() && v.password.length >= 8;
  const submit = async () => {
    setBusy(true);
    const ok = await run(
      () => orgApi.addMember(orgId, mode === 'existing'
        ? { userId: Number(v.userId) }
        : { name: v.name.trim(), email: v.email.trim(), password: v.password }),
      'eo.members.added',
    );
    setBusy(false);
    if (ok) {
      onDone();
      onClose();
    }
  };
  return (
    <Dialog open={open} onClose={onClose} fullWidth maxWidth="sm">
      <DialogTitle>{translate('eo.members.add')}</DialogTitle>
      <DialogContent>
        <Tabs value={mode} onChange={(_e, m) => setMode(m)} sx={{ mb: 2 }}>
          <Tab value="create" label={translate('eo.members.createAccount')} data-testid="add-member-create-tab" />
          <Tab value="existing" label={translate('eo.members.existingUser')} data-testid="add-member-existing-tab" />
        </Tabs>
        {mode === 'existing' ? (
          <TextField
            fullWidth
            type="number"
            label={translate('eo.members.userId')}
            helperText={translate('eo.members.userIdHelp')}
            {...f.bind('userId')}
            inputProps={{ 'data-testid': 'add-member-user-id' }}
          />
        ) : (
          <>
            <TextField fullWidth margin="dense" label={translate('eo.common.name')} {...f.bind('name')} inputProps={{ 'data-testid': 'add-member-name' }} />
            <TextField fullWidth margin="dense" type="email" label="Email" {...f.bind('email')} inputProps={{ 'data-testid': 'add-member-email' }} />
            <TextField
              fullWidth
              margin="dense"
              type="password"
              label={translate('eo.members.initialPassword')}
              helperText={translate('eo.common.passwordHelp')}
              {...f.bind('password')}
              inputProps={{ 'data-testid': 'add-member-password' }}
            />
          </>
        )}
      </DialogContent>
      <DialogActions>
        <Button onClick={onClose}>{translate('ra.action.cancel')}</Button>
        <Button variant="contained" disabled={!valid || busy} onClick={submit} data-testid="add-member-submit">
          {translate('eo.members.add')}
        </Button>
      </DialogActions>
    </Dialog>
  );
};

export const MembersTable: React.FC<{
  members: OrgMember[];
  meId?: Id;
  onRemove?: (m: OrgMember) => void;
}> = ({ members, meId, onRemove }) => {
  const translate = useTranslate();
  return (
    <Table size="small" data-testid="members-table">
      <TableHead>
        <TableRow>
          <TableCell>{translate('eo.common.name')}</TableCell>
          <TableCell>Email</TableCell>
          <TableCell>{translate('eo.members.membership')}</TableCell>
          <TableCell>{translate('eo.members.roles')}</TableCell>
          <TableCell>{translate('eo.members.joinedAt')}</TableCell>
          {onRemove && <TableCell align="right" />}
        </TableRow>
      </TableHead>
      <TableBody>
        {members.map((m) => (
          <TableRow key={m.userId} data-testid={`member-row-${m.userId}`}>
            <TableCell>
              {m.name || `#${m.userId}`}
              {m.userId === meId && <Chip size="small" label={translate('eo.common.you')} sx={{ ml: 1 }} />}
              {m.disabled && <Chip size="small" color="default" label={translate('eo.common.disabled')} sx={{ ml: 1 }} />}
            </TableCell>
            <TableCell>{m.email || '—'}</TableCell>
            <TableCell>{m.membershipRole}</TableCell>
            <TableCell>
              <Box display="flex" gap={0.5} flexWrap="wrap">
                {m.roles.map((r) => <Chip key={r} size="small" label={roleDisplayName(r)} title={r} />)}
                {!m.roles.length && <Typography variant="caption" color="text.secondary">—</Typography>}
              </Box>
            </TableCell>
            <TableCell>{fmtDate(m.joinedAt)}</TableCell>
            {onRemove && (
              <TableCell align="right">
                {m.userId !== meId && (
                  <Tooltip title={translate('eo.members.remove')}>
                    <IconButton size="small" onClick={() => onRemove(m)} data-testid={`member-remove-${m.userId}`}>
                      <DeleteIcon fontSize="small" />
                    </IconButton>
                  </Tooltip>
                )}
              </TableCell>
            )}
          </TableRow>
        ))}
        {!members.length && (
          <TableRow><TableCell colSpan={6}><Typography variant="body2" color="text.secondary">{translate('eo.members.none')}</Typography></TableCell></TableRow>
        )}
      </TableBody>
    </Table>
  );
};

const OrgMembers: React.FC<{ orgId: Id; orgName: string; personal: boolean }> = ({ orgId, orgName, personal }) => {
  const translate = useTranslate();
  const run = useAction();
  const { identity } = useGetIdentity();
  const list = useAsync(() => orgApi.members(orgId), [orgId]);
  const [addOpen, setAddOpen] = React.useState(false);
  const [removing, setRemoving] = React.useState<OrgMember | null>(null);
  return (
    <Box p={2}>
      <PageHeader
        title={translate('eo.members.title')}
        subtitle={orgName}
        onRefresh={list.reload}
        actions={!personal && (
          <Button variant="contained" startIcon={<PersonAddIcon />} onClick={() => setAddOpen(true)} data-testid="add-member">
            {translate('eo.members.add')}
          </Button>
        )}
      />
      <ErrorAlert error={list.error} />
      {list.loading && <Loading />}
      {list.data && (
        <Card>
          <MembersTable
            members={list.data.items}
            meId={identity?.id as Id | undefined}
            onRemove={personal ? undefined : setRemoving}
          />
        </Card>
      )}
      <AddMemberDialog orgId={orgId} open={addOpen} onClose={() => setAddOpen(false)} onDone={list.reload} />
      <ConfirmDialog
        open={!!removing}
        danger
        title={translate('eo.members.removeTitle', { name: removing?.name || '' })}
        content={<Typography variant="body2">{translate('eo.members.removeHelp')}</Typography>}
        onClose={() => setRemoving(null)}
        onConfirm={async () => {
          if (!removing) return;
          const ok = await run(() => orgApi.removeMember(orgId, removing.userId), 'eo.members.removed');
          if (ok) {
            setRemoving(null);
            list.reload();
          }
        }}
      />
    </Box>
  );
};

const MembersPage: React.FC = () => (
  <RequireSelection>
    {(sel) => (
      <OrgMembers
        key={sel.organizationId}
        orgId={sel.organizationId}
        orgName={sel.organizationName}
        personal={sel.organizationKind === 'personal'}
      />
    )}
  </RequireSelection>
);

export default MembersPage;
