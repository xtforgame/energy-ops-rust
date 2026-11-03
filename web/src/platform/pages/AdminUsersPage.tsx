// 超級使用者的帳號管理：搜尋、建立、改名／權限等級／停用、重設密碼。
import * as React from 'react';
import {
  Box, Button, Card, Chip, Dialog, DialogActions, DialogContent, DialogTitle, IconButton, MenuItem, Switch,
  Table, TableBody, TableCell, TableHead, TablePagination, TableRow, TextField, Tooltip,
} from '@mui/material';
import AddIcon from '@mui/icons-material/Add';
import EditIcon from '@mui/icons-material/Edit';
import { useGetIdentity, useTranslate } from 'react-admin';
import { adminApi } from '../../api/platform';
import type { Privilege, User } from '../../api/types';
import { ErrorAlert, fmtDate, Loading, PageHeader, useAction, useAsync } from './common';

const PRIVILEGES: Array<Privilege | ''> = ['', 'user', 'manager', 'admin'];

const UserDialog: React.FC<{ open: boolean; user: User | null; onClose: () => void; onDone: () => void }> = ({ open, user, onClose, onDone }) => {
  const translate = useTranslate();
  const run = useAction();
  const [name, setName] = React.useState('');
  const [email, setEmail] = React.useState('');
  const [password, setPassword] = React.useState('');
  const [privilege, setPrivilege] = React.useState<Privilege | ''>('');
  React.useEffect(() => {
    if (!open) return;
    setName(user?.name || '');
    setEmail(user?.email || '');
    setPassword('');
    setPrivilege((user?.privilege as Privilege) || '');
  }, [open, user]);
  const editing = !!user;
  const valid = !!name.trim() && (editing ? (!password || password.length >= 8) : !!email.trim() && password.length >= 8);
  return (
    <Dialog open={open} onClose={onClose} fullWidth maxWidth="xs">
      <DialogTitle>{translate(editing ? 'eo.admin.editUser' : 'eo.admin.createUser')}</DialogTitle>
      <DialogContent>
        <TextField fullWidth margin="dense" label={translate('eo.common.name')} value={name} onChange={(e) => setName(e.target.value)} />
        <TextField fullWidth margin="dense" label="Email" value={email} disabled={editing} onChange={(e) => setEmail(e.target.value)} />
        <TextField
          fullWidth
          margin="dense"
          type="password"
          label={translate(editing ? 'eo.admin.resetPassword' : 'ra.auth.password')}
          helperText={translate('eo.common.passwordHelp')}
          value={password}
          onChange={(e) => setPassword(e.target.value)}
        />
        <TextField select fullWidth margin="dense" label={translate('eo.admin.privilege')} value={privilege} onChange={(e) => setPrivilege(e.target.value as Privilege | '')}>
          {PRIVILEGES.map((p) => <MenuItem key={p || 'none'} value={p}>{p ? translate(`eo.admin.privileges.${p}`) : '—'}</MenuItem>)}
        </TextField>
      </DialogContent>
      <DialogActions>
        <Button onClick={onClose}>{translate('ra.action.cancel')}</Button>
        <Button
          variant="contained"
          disabled={!valid}
          onClick={async () => {
            const ok = await run(() => (editing
              ? adminApi.updateUser(user!.id, {
                name: name.trim(), privilege: privilege || null, ...(password ? { password } : {}),
              })
              : adminApi.createUser({
                name: name.trim(), email: email.trim(), password, ...(privilege ? { privilege } : {}),
              })), 'eo.admin.userSaved');
            if (ok) {
              onDone();
              onClose();
            }
          }}
        >
          {translate('ra.action.save')}
        </Button>
      </DialogActions>
    </Dialog>
  );
};

const AdminUsersPage: React.FC = () => {
  const translate = useTranslate();
  const run = useAction();
  const { identity } = useGetIdentity();
  const [q, setQ] = React.useState('');
  const [query, setQuery] = React.useState('');
  const [page, setPage] = React.useState(0);
  const [pageSize, setPageSize] = React.useState(50);
  const list = useAsync(() => adminApi.users({ q: query || undefined, page: page + 1, pageSize }), [query, page, pageSize]);
  const [editing, setEditing] = React.useState<{ open: boolean; user: User | null }>({ open: false, user: null });
  React.useEffect(() => {
    const t = setTimeout(() => { setQuery(q.trim()); setPage(0); }, 300);
    return () => clearTimeout(t);
  }, [q]);
  return (
    <Box p={2}>
      <PageHeader
        title={translate('eo.admin.usersTitle')}
        onRefresh={list.reload}
        actions={<Button variant="contained" startIcon={<AddIcon />} onClick={() => setEditing({ open: true, user: null })}>{translate('eo.admin.createUser')}</Button>}
      />
      <TextField size="small" label={translate('ra.action.search')} value={q} onChange={(e) => setQ(e.target.value)} sx={{ mb: 2, minWidth: 280 }} />
      <ErrorAlert error={list.error} />
      {list.loading && <Loading />}
      {list.data && (
        <Card>
          <Table size="small" data-testid="admin-users-table">
            <TableHead>
              <TableRow>
                <TableCell>ID</TableCell>
                <TableCell>{translate('eo.common.name')}</TableCell>
                <TableCell>Email</TableCell>
                <TableCell>{translate('eo.admin.privilege')}</TableCell>
                <TableCell>{translate('eo.admin.enabled')}</TableCell>
                <TableCell>{translate('eo.common.createdAt')}</TableCell>
                <TableCell align="right" />
              </TableRow>
            </TableHead>
            <TableBody>
              {list.data.items.map((u) => (
                <TableRow key={u.id}>
                  <TableCell>{u.id}</TableCell>
                  <TableCell>{u.name}</TableCell>
                  <TableCell>{u.email}</TableCell>
                  <TableCell>{u.privilege ? <Chip size="small" label={translate(`eo.admin.privileges.${u.privilege}`, { _: u.privilege })} /> : '—'}</TableCell>
                  <TableCell>
                    <Switch
                      size="small"
                      checked={!u.disabled}
                      disabled={u.id === identity?.id}
                      onChange={async (e) => {
                        if (await run(() => adminApi.updateUser(u.id, { disabled: !e.target.checked }), 'eo.admin.userSaved')) list.reload();
                      }}
                    />
                  </TableCell>
                  <TableCell>{fmtDate(u.createdAt)}</TableCell>
                  <TableCell align="right">
                    <Tooltip title={translate('ra.action.edit')}>
                      <IconButton size="small" onClick={() => setEditing({ open: true, user: u })}><EditIcon fontSize="small" /></IconButton>
                    </Tooltip>
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
          <TablePagination
            component="div"
            count={list.data.total}
            page={page}
            rowsPerPage={pageSize}
            rowsPerPageOptions={[20, 50, 100, 200]}
            onPageChange={(_e, p) => setPage(p)}
            onRowsPerPageChange={(e) => { setPageSize(Number(e.target.value)); setPage(0); }}
          />
        </Card>
      )}
      <UserDialog open={editing.open} user={editing.user} onClose={() => setEditing({ open: false, user: null })} onDone={list.reload} />
    </Box>
  );
};

export default AdminUsersPage;
