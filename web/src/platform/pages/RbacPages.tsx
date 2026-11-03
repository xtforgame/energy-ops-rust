// RBAC 管理：角色（permission chips、自訂角色 CRUD）、permission（含 complex 組合）、授權（grant／revoke＋原因）。
// 組織頁與超級使用者的全域頁共用同一組面板，差別只在 adapter（打哪一組端點）。
import * as React from 'react';
import {
  Alert, Autocomplete, Box, Button, Card, Checkbox, Chip, Dialog, DialogActions, DialogContent, DialogTitle,
  FormControlLabel, IconButton, InputAdornment, MenuItem, Stack, Tab, Table, TableBody, TableCell, TableHead,
  TableRow, Tabs, TextField, ToggleButton, ToggleButtonGroup, Tooltip, Typography,
} from '@mui/material';
import AddIcon from '@mui/icons-material/Add';
import EditIcon from '@mui/icons-material/Edit';
import DeleteIcon from '@mui/icons-material/Delete';
import GroupIcon from '@mui/icons-material/Group';
import LockIcon from '@mui/icons-material/Lock';
import CancelIcon from '@mui/icons-material/Cancel';
import { useTranslate } from 'react-admin';
import { adminApi, orgApi, orgRbacApi } from '../../api/platform';
import type {
  CreatePermissionInput, CreateRoleInput, GrantView, Id, Items, RbacPermission, RbacRole, RbacScope,
  UpdatePermissionInput, UpdateRoleInput,
} from '../../api/types';
import { roleDisplayName } from '../../admin/domain-logic/components/energyOps/roleDisplay';
import {
  ConfirmDialog, ErrorAlert, fmtDate, Loading, PageHeader, RequireSelection, useAction, useAsync,
} from './common';

export const CUSTOM_PREFIX = 'custom:';
const SCOPES: RbacScope[] = ['organization', 'project', 'resource'];

export interface RbacAdapter {
  roles: () => Promise<Items<RbacRole>>;
  createRole: (input: CreateRoleInput) => Promise<RbacRole>;
  updateRole: (id: Id, input: UpdateRoleInput) => Promise<RbacRole>;
  deleteRole: (id: Id) => Promise<unknown>;
  permissions: () => Promise<Items<RbacPermission>>;
  createPermission: (input: CreatePermissionInput) => Promise<RbacPermission>;
  updatePermission: (id: Id, input: UpdatePermissionInput) => Promise<RbacPermission>;
  deletePermission: (id: Id) => Promise<unknown>;
  /** 只有組織頁才有「持有者」。 */
  holders?: (roleId: Id) => Promise<Items<GrantView>>;
}

export const orgAdapter = (orgId: Id): RbacAdapter => ({
  roles: () => orgRbacApi.roles(orgId),
  createRole: (i) => orgRbacApi.createRole(orgId, i),
  updateRole: (id, i) => orgRbacApi.updateRole(orgId, id, i),
  deleteRole: (id) => orgRbacApi.deleteRole(orgId, id),
  permissions: () => orgRbacApi.permissions(orgId),
  createPermission: (i) => orgRbacApi.createPermission(orgId, i),
  updatePermission: (id, i) => orgRbacApi.updatePermission(orgId, id, i),
  deletePermission: (id) => orgRbacApi.deletePermission(orgId, id),
  holders: (roleId) => orgRbacApi.roleHolders(orgId, roleId),
});

export const globalAdapter: RbacAdapter = {
  roles: adminApi.roles,
  createRole: adminApi.createRole,
  updateRole: adminApi.updateRole,
  deleteRole: adminApi.deleteRole,
  permissions: adminApi.permissions,
  createPermission: adminApi.createPermission,
  updatePermission: adminApi.updatePermission,
  deletePermission: adminApi.deletePermission,
};

/** 自訂名稱：使用者只打後半段，送出時補上 custom: 前綴。 */
export const withCustomPrefix = (s: string) => {
  const t = s.trim();
  return t.startsWith(CUSTOM_PREFIX) ? t : `${CUSTOM_PREFIX}${t}`;
};
export const isValidCustomName = (s: string) => /^[A-Za-z0-9:\-_.]+$/.test(s.trim()) && s.trim().length > 0;
export const isEditable = (row: { type: string }) => row.type === 'user-defined';

const TypeChip: React.FC<{ row: { type: string; createdByPlugin: string | null } }> = ({ row }) => {
  const translate = useTranslate();
  const editable = isEditable(row);
  return (
    <Tooltip title={editable ? '' : translate('eo.rbac.readOnly', { plugin: row.createdByPlugin || 'core' })}>
      <Chip
        size="small"
        variant="outlined"
        color={editable ? 'success' : 'info'}
        icon={editable ? undefined : <LockIcon fontSize="small" />}
        label={translate(`eo.rbac.type.${row.type}`, { _: row.type })}
      />
    </Tooltip>
  );
};

const PermissionChips: React.FC<{ names: string[]; perms?: RbacPermission[] }> = ({ names, perms }) => {
  const byName = React.useMemo(() => new Map((perms || []).map((p) => [p.uniqueName, p])), [perms]);
  return (
    <Box display="flex" gap={0.5} flexWrap="wrap">
      {names.map((n) => {
        const p = byName.get(n);
        return (
          <Tooltip key={n} title={p ? `${p.name}${p.complex ? ` ⊃ ${p.included.join(', ')}` : ''}` : n}>
            <Chip size="small" label={n} variant={p?.complex ? 'filled' : 'outlined'} color={p?.complex ? 'secondary' : 'default'} />
          </Tooltip>
        );
      })}
      {!names.length && <Typography variant="caption" color="text.secondary">—</Typography>}
    </Box>
  );
};

// ── 角色 ──

const RoleDialog: React.FC<{
  open: boolean;
  role: RbacRole | null;
  perms: RbacPermission[];
  adapter: RbacAdapter;
  onClose: () => void;
  onDone: () => void;
}> = ({ open, role, perms, adapter, onClose, onDone }) => {
  const translate = useTranslate();
  const run = useAction();
  const [scope, setScope] = React.useState<RbacScope>('organization');
  const [suffix, setSuffix] = React.useState('');
  const [name, setName] = React.useState('');
  const [selected, setSelected] = React.useState<string[]>([]);
  const [busy, setBusy] = React.useState(false);
  React.useEffect(() => {
    if (!open) return;
    setScope(role?.scope || 'organization');
    setSuffix(role ? role.uniqueName.slice(CUSTOM_PREFIX.length) : '');
    setName(role?.name || '');
    setSelected(role?.permissions || []);
  }, [open, role]);
  const options = perms.filter((p) => p.scope === scope).map((p) => p.uniqueName);
  const editing = !!role;
  const unique = withCustomPrefix(suffix);
  const valid = !!name.trim() && (editing || (isValidCustomName(suffix) && !suffix.startsWith(':')));
  const submit = async () => {
    setBusy(true);
    const ok = await run(
      () => (editing
        ? adapter.updateRole(role!.id, { name: name.trim(), permissions: selected })
        : adapter.createRole({ scope, uniqueName: unique, name: name.trim(), permissions: selected })),
      editing ? 'eo.rbac.roleSaved' : 'eo.rbac.roleCreated',
    );
    setBusy(false);
    if (ok) {
      onDone();
      onClose();
    }
  };
  return (
    <Dialog open={open} onClose={onClose} fullWidth maxWidth="sm">
      <DialogTitle>{translate(editing ? 'eo.rbac.editRole' : 'eo.rbac.createRole')}</DialogTitle>
      <DialogContent>
        <TextField
          select
          fullWidth
          margin="dense"
          label={translate('eo.common.scope')}
          value={scope}
          disabled={editing}
          onChange={(e) => {
            setScope(e.target.value as RbacScope);
            setSelected([]);
          }}
        >
          {SCOPES.map((s) => <MenuItem key={s} value={s}>{translate(`eo.rbac.scope.${s}`)}</MenuItem>)}
        </TextField>
        <TextField
          fullWidth
          margin="dense"
          label={translate('eo.common.uniqueName')}
          value={suffix}
          disabled={editing}
          onChange={(e) => setSuffix(e.target.value.replace(/^custom:/, ''))}
          helperText={translate('eo.rbac.customPrefixHelp')}
          InputProps={{ startAdornment: <InputAdornment position="start">{CUSTOM_PREFIX}</InputAdornment> }}
          inputProps={{ 'data-testid': 'role-unique-name' }}
        />
        <TextField
          fullWidth
          margin="dense"
          label={translate('eo.common.name')}
          value={name}
          onChange={(e) => setName(e.target.value)}
          inputProps={{ 'data-testid': 'role-name' }}
        />
        <Autocomplete
          multiple
          options={options}
          value={selected}
          onChange={(_e, v) => setSelected(v)}
          renderInput={(params) => <TextField {...params} margin="dense" label={translate('eo.rbac.permissions')} />}
        />
      </DialogContent>
      <DialogActions>
        <Button onClick={onClose}>{translate('ra.action.cancel')}</Button>
        <Button variant="contained" disabled={busy || !valid} onClick={submit} data-testid="role-submit">
          {translate('ra.action.save')}
        </Button>
      </DialogActions>
    </Dialog>
  );
};

const HoldersDialog: React.FC<{ role: RbacRole | null; adapter: RbacAdapter; onClose: () => void }> = ({ role, adapter, onClose }) => {
  const translate = useTranslate();
  const holders = useAsync(() => adapter.holders!(role!.id), [role?.id], !!role && !!adapter.holders);
  return (
    <Dialog open={!!role} onClose={onClose} fullWidth maxWidth="sm">
      <DialogTitle>{translate('eo.rbac.holders')} — {role?.name}</DialogTitle>
      <DialogContent>
        <ErrorAlert error={holders.error} />
        {holders.loading && <Loading />}
        {holders.data && (
          <Table size="small">
            <TableBody>
              {holders.data.items.map((g) => (
                <TableRow key={g.id}>
                  <TableCell>{g.userName}</TableCell>
                  <TableCell>{g.userEmail}</TableCell>
                  <TableCell>{g.projectName || translate('eo.common.organization')}</TableCell>
                </TableRow>
              ))}
              {!holders.data.items.length && <TableRow><TableCell>{translate('eo.rbac.noHolders')}</TableCell></TableRow>}
            </TableBody>
          </Table>
        )}
      </DialogContent>
      <DialogActions><Button onClick={onClose}>{translate('ra.action.close')}</Button></DialogActions>
    </Dialog>
  );
};

export const RolesPanel: React.FC<{ adapter: RbacAdapter; title: string; subtitle?: string }> = ({ adapter, title, subtitle }) => {
  const translate = useTranslate();
  const run = useAction();
  const roles = useAsync(adapter.roles, [adapter]);
  const perms = useAsync(adapter.permissions, [adapter]);
  const [scope, setScope] = React.useState<RbacScope | 'all'>('all');
  const [editing, setEditing] = React.useState<{ open: boolean; role: RbacRole | null }>({ open: false, role: null });
  const [deleting, setDeleting] = React.useState<RbacRole | null>(null);
  const [holdersOf, setHoldersOf] = React.useState<RbacRole | null>(null);
  const items = (roles.data?.items || []).filter((r) => scope === 'all' || r.scope === scope);
  return (
    <Box p={2}>
      <PageHeader
        title={title}
        subtitle={subtitle}
        onRefresh={() => { roles.reload(); perms.reload(); }}
        actions={(
          <Button variant="contained" startIcon={<AddIcon />} onClick={() => setEditing({ open: true, role: null })} data-testid="role-create">
            {translate('eo.rbac.createRole')}
          </Button>
        )}
      />
      <ToggleButtonGroup size="small" exclusive value={scope} onChange={(_e, v) => v && setScope(v)} sx={{ mb: 2 }}>
        <ToggleButton value="all">{translate('eo.common.all')}</ToggleButton>
        {SCOPES.map((s) => <ToggleButton key={s} value={s}>{translate(`eo.rbac.scope.${s}`)}</ToggleButton>)}
      </ToggleButtonGroup>
      <ErrorAlert error={roles.error} />
      {roles.loading && <Loading />}
      {roles.data && (
        <Card>
          <Table size="small" data-testid="roles-table">
            <TableHead>
              <TableRow>
                <TableCell>{translate('eo.common.name')}</TableCell>
                <TableCell>{translate('eo.common.uniqueName')}</TableCell>
                <TableCell>{translate('eo.common.scope')}</TableCell>
                <TableCell>{translate('eo.common.type')}</TableCell>
                <TableCell>{translate('eo.rbac.permissions')}</TableCell>
                <TableCell align="right" />
              </TableRow>
            </TableHead>
            <TableBody>
              {items.map((r) => (
                <TableRow key={r.id} data-testid={`role-row-${r.uniqueName}`}>
                  <TableCell>{roleDisplayName(r.uniqueName, r.name)}</TableCell>
                  <TableCell><code>{r.uniqueName}</code></TableCell>
                  <TableCell>{translate(`eo.rbac.scope.${r.scope}`)}</TableCell>
                  <TableCell><TypeChip row={r} /></TableCell>
                  <TableCell><PermissionChips names={r.permissions} perms={perms.data?.items} /></TableCell>
                  <TableCell align="right" sx={{ whiteSpace: 'nowrap' }}>
                    {adapter.holders && (
                      <Tooltip title={translate('eo.rbac.holders')}>
                        <IconButton size="small" onClick={() => setHoldersOf(r)}><GroupIcon fontSize="small" /></IconButton>
                      </Tooltip>
                    )}
                    {isEditable(r) && (
                      <>
                        <IconButton size="small" onClick={() => setEditing({ open: true, role: r })} data-testid={`role-edit-${r.uniqueName}`}>
                          <EditIcon fontSize="small" />
                        </IconButton>
                        <IconButton size="small" onClick={() => setDeleting(r)} data-testid={`role-delete-${r.uniqueName}`}>
                          <DeleteIcon fontSize="small" />
                        </IconButton>
                      </>
                    )}
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </Card>
      )}
      <RoleDialog
        open={editing.open}
        role={editing.role}
        perms={perms.data?.items || []}
        adapter={adapter}
        onClose={() => setEditing({ open: false, role: null })}
        onDone={roles.reload}
      />
      <HoldersDialog role={holdersOf} adapter={adapter} onClose={() => setHoldersOf(null)} />
      <ConfirmDialog
        open={!!deleting}
        danger
        title={translate('eo.rbac.deleteRoleTitle', { name: deleting?.uniqueName || '' })}
        onClose={() => setDeleting(null)}
        onConfirm={async () => {
          if (!deleting) return;
          if (await run(() => adapter.deleteRole(deleting.id), 'eo.rbac.roleDeleted')) {
            setDeleting(null);
            roles.reload();
          }
        }}
      />
    </Box>
  );
};

// ── permission ──

const PermissionDialog: React.FC<{
  open: boolean;
  perm: RbacPermission | null;
  perms: RbacPermission[];
  adapter: RbacAdapter;
  onClose: () => void;
  onDone: () => void;
}> = ({ open, perm, perms, adapter, onClose, onDone }) => {
  const translate = useTranslate();
  const run = useAction();
  const [scope, setScope] = React.useState<RbacScope>('organization');
  const [suffix, setSuffix] = React.useState('');
  const [name, setName] = React.useState('');
  const [complex, setComplex] = React.useState(false);
  const [included, setIncluded] = React.useState<string[]>([]);
  const [busy, setBusy] = React.useState(false);
  React.useEffect(() => {
    if (!open) return;
    setScope(perm?.scope || 'organization');
    setSuffix(perm ? perm.uniqueName.slice(CUSTOM_PREFIX.length) : '');
    setName(perm?.name || '');
    setComplex(!!perm?.complex);
    setIncluded(perm?.included || []);
  }, [open, perm]);
  const editing = !!perm;
  // complex 只能包含同 scope 的 simple permission
  const options = perms.filter((p) => p.scope === scope && !p.complex).map((p) => p.uniqueName);
  const valid = !!name.trim() && (editing || isValidCustomName(suffix)) && (!complex || included.length > 0);
  const submit = async () => {
    setBusy(true);
    const ok = await run(
      () => (editing
        ? adapter.updatePermission(perm!.id, { name: name.trim(), ...(perm!.complex ? { included } : {}) })
        : adapter.createPermission({
          scope, uniqueName: withCustomPrefix(suffix), name: name.trim(), complex, included: complex ? included : [],
        })),
      'eo.rbac.permissionSaved',
    );
    setBusy(false);
    if (ok) {
      onDone();
      onClose();
    }
  };
  return (
    <Dialog open={open} onClose={onClose} fullWidth maxWidth="sm">
      <DialogTitle>{translate(editing ? 'eo.rbac.editPermission' : 'eo.rbac.createPermission')}</DialogTitle>
      <DialogContent>
        <TextField select fullWidth margin="dense" label={translate('eo.common.scope')} value={scope} disabled={editing}
          onChange={(e) => { setScope(e.target.value as RbacScope); setIncluded([]); }}>
          {SCOPES.map((s) => <MenuItem key={s} value={s}>{translate(`eo.rbac.scope.${s}`)}</MenuItem>)}
        </TextField>
        <TextField
          fullWidth
          margin="dense"
          label={translate('eo.common.uniqueName')}
          value={suffix}
          disabled={editing}
          onChange={(e) => setSuffix(e.target.value.replace(/^custom:/, ''))}
          InputProps={{ startAdornment: <InputAdornment position="start">{CUSTOM_PREFIX}</InputAdornment> }}
        />
        <TextField fullWidth margin="dense" label={translate('eo.common.name')} value={name} onChange={(e) => setName(e.target.value)} />
        <FormControlLabel
          control={<Checkbox checked={complex} disabled={editing} onChange={(e) => setComplex(e.target.checked)} />}
          label={translate('eo.rbac.complex')}
        />
        {complex && (
          <Autocomplete
            multiple
            options={options}
            value={included}
            onChange={(_e, v) => setIncluded(v)}
            renderInput={(params) => <TextField {...params} margin="dense" label={translate('eo.rbac.included')} />}
          />
        )}
      </DialogContent>
      <DialogActions>
        <Button onClick={onClose}>{translate('ra.action.cancel')}</Button>
        <Button variant="contained" disabled={busy || !valid} onClick={submit}>{translate('ra.action.save')}</Button>
      </DialogActions>
    </Dialog>
  );
};

export const PermissionsPanel: React.FC<{ adapter: RbacAdapter; title: string; subtitle?: string }> = ({ adapter, title, subtitle }) => {
  const translate = useTranslate();
  const run = useAction();
  const perms = useAsync(adapter.permissions, [adapter]);
  const [scope, setScope] = React.useState<RbacScope | 'all'>('all');
  const [editing, setEditing] = React.useState<{ open: boolean; perm: RbacPermission | null }>({ open: false, perm: null });
  const [deleting, setDeleting] = React.useState<RbacPermission | null>(null);
  const items = (perms.data?.items || []).filter((p) => scope === 'all' || p.scope === scope);
  return (
    <Box p={2}>
      <PageHeader
        title={title}
        subtitle={subtitle}
        onRefresh={perms.reload}
        actions={(
          <Button variant="contained" startIcon={<AddIcon />} onClick={() => setEditing({ open: true, perm: null })}>
            {translate('eo.rbac.createPermission')}
          </Button>
        )}
      />
      <ToggleButtonGroup size="small" exclusive value={scope} onChange={(_e, v) => v && setScope(v)} sx={{ mb: 2 }}>
        <ToggleButton value="all">{translate('eo.common.all')}</ToggleButton>
        {SCOPES.map((s) => <ToggleButton key={s} value={s}>{translate(`eo.rbac.scope.${s}`)}</ToggleButton>)}
      </ToggleButtonGroup>
      <ErrorAlert error={perms.error} />
      {perms.loading && <Loading />}
      {perms.data && (
        <Card>
          <Table size="small" data-testid="permissions-table">
            <TableHead>
              <TableRow>
                <TableCell>{translate('eo.common.uniqueName')}</TableCell>
                <TableCell>{translate('eo.common.name')}</TableCell>
                <TableCell>{translate('eo.common.scope')}</TableCell>
                <TableCell>{translate('eo.common.type')}</TableCell>
                <TableCell>{translate('eo.rbac.included')}</TableCell>
                <TableCell align="right" />
              </TableRow>
            </TableHead>
            <TableBody>
              {items.map((p) => (
                <TableRow key={p.id}>
                  <TableCell>
                    <code>{p.uniqueName}</code>
                    {p.complex && <Chip size="small" color="secondary" label={translate('eo.rbac.complex')} sx={{ ml: 1 }} />}
                  </TableCell>
                  <TableCell>{p.name}</TableCell>
                  <TableCell>{translate(`eo.rbac.scope.${p.scope}`)}</TableCell>
                  <TableCell><TypeChip row={p} /></TableCell>
                  <TableCell>{p.complex ? <PermissionChips names={p.included} perms={perms.data!.items} /> : '—'}</TableCell>
                  <TableCell align="right" sx={{ whiteSpace: 'nowrap' }}>
                    {isEditable(p) && (
                      <>
                        <IconButton size="small" onClick={() => setEditing({ open: true, perm: p })}><EditIcon fontSize="small" /></IconButton>
                        <IconButton size="small" onClick={() => setDeleting(p)}><DeleteIcon fontSize="small" /></IconButton>
                      </>
                    )}
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </Card>
      )}
      <PermissionDialog
        open={editing.open}
        perm={editing.perm}
        perms={perms.data?.items || []}
        adapter={adapter}
        onClose={() => setEditing({ open: false, perm: null })}
        onDone={perms.reload}
      />
      <ConfirmDialog
        open={!!deleting}
        danger
        title={translate('eo.rbac.deletePermissionTitle', { name: deleting?.uniqueName || '' })}
        onClose={() => setDeleting(null)}
        onConfirm={async () => {
          if (!deleting) return;
          if (await run(() => adapter.deletePermission(deleting.id), 'eo.rbac.permissionDeleted')) {
            setDeleting(null);
            perms.reload();
          }
        }}
      />
    </Box>
  );
};

// ── 授權 ──

const GrantDialog: React.FC<{ open: boolean; orgId: Id; onClose: () => void; onDone: () => void }> = ({ open, orgId, onClose, onDone }) => {
  const translate = useTranslate();
  const run = useAction();
  const members = useAsync(() => orgApi.members(orgId), [orgId, open], open);
  const roles = useAsync(() => orgRbacApi.roles(orgId), [orgId, open], open);
  const projects = useAsync(() => orgApi.projects(orgId), [orgId, open], open);
  const [userId, setUserId] = React.useState('');
  const [roleId, setRoleId] = React.useState('');
  const [projectId, setProjectId] = React.useState('');
  React.useEffect(() => {
    if (open) { setUserId(''); setRoleId(''); setProjectId(''); }
  }, [open]);
  const role = roles.data?.items.find((r) => String(r.id) === roleId);
  const needsProject = role?.scope === 'project';
  const assignable = (roles.data?.items || []).filter((r) => r.scope !== 'resource');
  const valid = !!userId && !!roleId && (!needsProject || !!projectId);
  return (
    <Dialog open={open} onClose={onClose} fullWidth maxWidth="sm">
      <DialogTitle>{translate('eo.rbac.grant')}</DialogTitle>
      <DialogContent>
        <ErrorAlert error={members.error || roles.error} />
        <TextField select fullWidth margin="dense" label={translate('eo.members.user')} value={userId}
          onChange={(e) => setUserId(e.target.value)} SelectProps={{ SelectDisplayProps: { 'data-testid': 'grant-user' } as any }}>
          {(members.data?.items || []).map((m) => <MenuItem key={m.userId} value={String(m.userId)}>{m.name} {m.email ? `<${m.email}>` : ''}</MenuItem>)}
        </TextField>
        <TextField select fullWidth margin="dense" label={translate('eo.rbac.role')} value={roleId}
          onChange={(e) => { setRoleId(e.target.value); setProjectId(''); }} SelectProps={{ SelectDisplayProps: { 'data-testid': 'grant-role' } as any }}>
          {assignable.map((r) => (
            <MenuItem key={r.id} value={String(r.id)}>
              {roleDisplayName(r.uniqueName, r.name)}（{translate(`eo.rbac.scope.${r.scope}`)}）
            </MenuItem>
          ))}
        </TextField>
        {needsProject && (
          <TextField select fullWidth margin="dense" label={translate('eo.common.project')} value={projectId} onChange={(e) => setProjectId(e.target.value)}>
            {(projects.data?.items || []).map((p) => <MenuItem key={p.id} value={String(p.id)}>{p.name}</MenuItem>)}
          </TextField>
        )}
        {needsProject && <Alert severity="info" sx={{ mt: 1 }}>{translate('eo.rbac.projectMemberRequired')}</Alert>}
      </DialogContent>
      <DialogActions>
        <Button onClick={onClose}>{translate('ra.action.cancel')}</Button>
        <Button
          variant="contained"
          disabled={!valid}
          data-testid="grant-submit"
          onClick={async () => {
            const ok = await run(() => orgRbacApi.grant(orgId, {
              userId: Number(userId), roleId: Number(roleId), ...(needsProject ? { projectId: Number(projectId) } : {}),
            }), 'tc.grant.granted');
            if (ok) {
              onDone();
              onClose();
            }
          }}
        >
          {translate('eo.rbac.grant')}
        </Button>
      </DialogActions>
    </Dialog>
  );
};

const GrantsPanel: React.FC<{ orgId: Id; orgName: string }> = ({ orgId, orgName }) => {
  const translate = useTranslate();
  const run = useAction();
  const [includeRevoked, setIncludeRevoked] = React.useState(false);
  const grants = useAsync(() => orgRbacApi.grants(orgId, { includeRevoked }), [orgId, includeRevoked]);
  const [grantOpen, setGrantOpen] = React.useState(false);
  const [revoking, setRevoking] = React.useState<GrantView | null>(null);
  return (
    <Box p={2}>
      <PageHeader
        title={translate('eo.rbac.grantsTitle')}
        subtitle={orgName}
        onRefresh={grants.reload}
        actions={(
          <Button variant="contained" startIcon={<AddIcon />} onClick={() => setGrantOpen(true)} data-testid="grant-open">
            {translate('eo.rbac.grant')}
          </Button>
        )}
      />
      <FormControlLabel
        control={<Checkbox checked={includeRevoked} onChange={(e) => setIncludeRevoked(e.target.checked)} />}
        label={translate('eo.rbac.includeRevoked')}
      />
      <ErrorAlert error={grants.error} />
      {grants.loading && <Loading />}
      {grants.data && (
        <Card>
          <Table size="small" data-testid="grants-table">
            <TableHead>
              <TableRow>
                <TableCell>{translate('eo.members.user')}</TableCell>
                <TableCell>{translate('eo.rbac.role')}</TableCell>
                <TableCell>{translate('eo.common.scope')}</TableCell>
                <TableCell>{translate('eo.common.createdAt')}</TableCell>
                <TableCell>{translate('eo.rbac.revoked')}</TableCell>
                <TableCell align="right" />
              </TableRow>
            </TableHead>
            <TableBody>
              {grants.data.items.map((g) => (
                <TableRow key={g.id} sx={g.revokedAt ? { opacity: 0.55 } : undefined} data-testid={`grant-row-${g.id}`}>
                  <TableCell>{g.userName}<Typography variant="caption" display="block" color="text.secondary">{g.userEmail}</Typography></TableCell>
                  <TableCell><Chip size="small" label={roleDisplayName(g.roleUniqueName, g.roleName)} title={g.roleUniqueName} /></TableCell>
                  <TableCell>
                    {g.projectName ? `${translate('eo.common.project')}：${g.projectName}` : translate('eo.common.organization')}
                    {g.resourceIdentifier && <Typography variant="caption" display="block"><code>{g.resourceIdentifier}</code></Typography>}
                  </TableCell>
                  <TableCell>{fmtDate(g.createdAt)}</TableCell>
                  <TableCell>
                    {g.revokedAt ? `${fmtDate(g.revokedAt)}${g.revokedReason ? `（${g.revokedReason}）` : ''}` : '—'}
                  </TableCell>
                  <TableCell align="right">
                    {!g.revokedAt && (
                      <Tooltip title={translate('tc.grant.revokeThis')}>
                        <IconButton size="small" onClick={() => setRevoking(g)} data-testid={`grant-revoke-${g.id}`}>
                          <CancelIcon fontSize="small" />
                        </IconButton>
                      </Tooltip>
                    )}
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </Card>
      )}
      <GrantDialog open={grantOpen} orgId={orgId} onClose={() => setGrantOpen(false)} onDone={grants.reload} />
      <ConfirmDialog
        open={!!revoking}
        danger
        title={translate('tc.grant.revokeTitle', {
          name: revoking?.userName || '', role: revoking ? roleDisplayName(revoking.roleUniqueName, revoking.roleName) : '',
        })}
        reasonLabel={translate('tc.grant.reason')}
        onClose={() => setRevoking(null)}
        onConfirm={async (reason) => {
          if (!revoking) return;
          if (await run(() => orgRbacApi.revokeGrant(orgId, revoking.id, reason || undefined), 'tc.grant.revoked')) {
            setRevoking(null);
            grants.reload();
          }
        }}
      />
    </Box>
  );
};

// ── 頁面 ──

const useOrgAdapter = (orgId: Id) => React.useMemo(() => orgAdapter(orgId), [orgId]);

const OrgRoles: React.FC<{ orgId: Id; orgName: string }> = ({ orgId, orgName }) => {
  const translate = useTranslate();
  return <RolesPanel adapter={useOrgAdapter(orgId)} title={translate('eo.rbac.rolesTitle')} subtitle={orgName} />;
};

const OrgPermissions: React.FC<{ orgId: Id; orgName: string }> = ({ orgId, orgName }) => {
  const translate = useTranslate();
  return <PermissionsPanel adapter={useOrgAdapter(orgId)} title={translate('eo.rbac.permissionsTitle')} subtitle={orgName} />;
};

export const RbacRolesPage: React.FC = () => (
  <RequireSelection>{(s) => <OrgRoles key={s.organizationId} orgId={s.organizationId} orgName={s.organizationName} />}</RequireSelection>
);

export const RbacPermissionsPage: React.FC = () => (
  <RequireSelection>{(s) => <OrgPermissions key={s.organizationId} orgId={s.organizationId} orgName={s.organizationName} />}</RequireSelection>
);

export const RbacGrantsPage: React.FC = () => (
  <RequireSelection>{(s) => <GrantsPanel key={s.organizationId} orgId={s.organizationId} orgName={s.organizationName} />}</RequireSelection>
);

/** 超級使用者：全域（不屬於任何組織）的自訂角色與 permission。 */
export const AdminRbacPage: React.FC = () => {
  const translate = useTranslate();
  const [tab, setTab] = React.useState<'roles' | 'permissions'>('roles');
  return (
    <Box>
      <Stack px={2} pt={1}>
        <Tabs value={tab} onChange={(_e, v) => setTab(v)}>
          <Tab value="roles" label={translate('eo.rbac.rolesTitle')} />
          <Tab value="permissions" label={translate('eo.rbac.permissionsTitle')} />
        </Tabs>
      </Stack>
      {tab === 'roles'
        ? <RolesPanel adapter={globalAdapter} title={translate('eo.admin.globalRoles')} />
        : <PermissionsPanel adapter={globalAdapter} title={translate('eo.admin.globalPermissions')} />}
    </Box>
  );
};
