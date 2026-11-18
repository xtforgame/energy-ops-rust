import * as React from 'react';
import {
  Box, Card, CardContent, Typography, Button, Table, TableHead, TableBody, TableRow, TableCell,
  Chip, CircularProgress, Alert, Stack, Tooltip, IconButton, MenuItem, Tabs, Tab,
  Dialog, DialogTitle, DialogContent, DialogActions, TextField,
} from '@mui/material';
import AddIcon from '@mui/icons-material/Add';
import DeleteIcon from '@mui/icons-material/Delete';
import RefreshIcon from '@mui/icons-material/Refresh';
import PersonAddIcon from '@mui/icons-material/PersonAdd';
import AccountTreeIcon from '@mui/icons-material/AccountTree';
import { energyHttp, energyBase, fetchMemberOptions } from '@/api/energy';
import { useNotify, useTranslate } from 'react-admin';
import { useSelection } from '@/platform/selection/store';
import { apiErrorCode } from '../../../dataProvider/apiError';
import OrgChart from './OrgChart';

interface DepartmentMember { userId: number; name: string | null; role: 'manager' | 'member' }
interface Department {
  id: number; key: string; name: string; sortOrder: number; parentId: number | null; members: DepartmentMember[];
}

/** 這個部門自己與它所有下層的 id（含自己）——「改上層」的下拉要排除這一整組，不然會造出循環。 */
function selfAndDescendants(departments: Department[], id: number): Set<number> {
  const out = new Set<number>([id]);
  let grew = true;
  while (grew) {
    grew = false;
    departments.forEach((d) => {
      if (d.parentId != null && out.has(d.parentId) && !out.has(d.id)) {
        out.add(d.id);
        grew = true;
      }
    });
  }
  return out;
}

/**
 * 部門管理頁（org-scope）。
 *
 * 設計出處：`docs/features/energy-ops-console/WORKFLOW.md §2.4`
 * —— 組織是**扁平兩層**（部門 ＋ 主管/同仁），沒有上下層部門。
 * 主責者規則（「發起人的部門主管」「指定部門收單」）就是靠這張表解析的。
 *
 * 📌 一個人可以在多個部門、一個部門可以有多位主管 —— 刻意允許
 * （多人＝候選池，任一人核即可）。
 *
 * gating 交給伺服器：能 GET departments（需 `energy:department:manage`）才看得到內容。
 * 前端 gating 只是 UX，後端 BFF 才是真相。
 */
const DepartmentsPage: React.FC = () => {
  const notify = useNotify();
  const translate = useTranslate();
  const selected = useSelection();
  const orgId = selected?.organizationId;
  const orgName = selected?.organizationName;

  const [loading, setLoading] = React.useState(false);
  const [denied, setDenied] = React.useState(false);
  const [departments, setDepartments] = React.useState<Department[]>([]);
  const [members, setMembers] = React.useState<any[]>([]);
  const [tab, setTab] = React.useState<'list' | 'chart'>('list');

  const [createOpen, setCreateOpen] = React.useState(false);
  const [newKey, setNewKey] = React.useState('');
  const [newName, setNewName] = React.useState('');
  const [newParentId, setNewParentId] = React.useState('');
  const [createError, setCreateError] = React.useState<string | null>(null);

  const [assignTarget, setAssignTarget] = React.useState<Department | null>(null);
  const [assignUserId, setAssignUserId] = React.useState('');
  const [assignRole, setAssignRole] = React.useState<'manager' | 'member'>('member');
  const [assignError, setAssignError] = React.useState<string | null>(null);

  const [reparentTarget, setReparentTarget] = React.useState<Department | null>(null);
  const [reparentValue, setReparentValue] = React.useState('');
  const [reparentError, setReparentError] = React.useState<string | null>(null);

  const friendlyError = (code: string) => {
    const map: Record<string, string> = {
      department_key_taken: translate('tc.departments.keyTaken'),
      parent_not_found: translate('tc.departments.parentNotFound'),
      department_cycle: translate('tc.departments.cycleError'),
    };
    return map[code] || code || 'failed';
  };

  const base = energyBase(orgId);

  const load = React.useCallback(async () => {
    if (!orgId) { setDepartments([]); setDenied(false); return; }
    setLoading(true); setDenied(false);
    try {
      const { data } = await energyHttp.get(`${energyBase(orgId)}/departments`);
      setDepartments(Array.isArray(data) ? data : []);
    } catch (e: any) {
      if (e?.response?.status === 403) setDenied(true);
      else notify(e?.response?.data?.message || e?.message || 'load failed', { type: 'error' });
    } finally {
      setLoading(false);
    }
    // 指派對話框要有人可選；沒權限讀成員就留空（手動輸入 id），不擋整頁。
    setMembers(await fetchMemberOptions(orgId));
  }, [orgId, notify]);

  React.useEffect(() => { load(); }, [load]);

  const createDepartment = async () => {
    setCreateError(null);
    try {
      await energyHttp.post(`${base}/departments`, {
        key: newKey.trim(), name: newName.trim(), parentId: newParentId ? Number(newParentId) : null,
      });
      notify('tc.departments.created', { type: 'success' });
      setCreateOpen(false); setNewKey(''); setNewName(''); setNewParentId('');
      load();
    } catch (e: any) {
      setCreateError(friendlyError(apiErrorCode(e)));
    }
  };

  const submitReparent = async () => {
    if (!reparentTarget) return;
    setReparentError(null);
    try {
      await energyHttp.patch(`${base}/departments/${reparentTarget.id}`, {
        parentId: reparentValue ? Number(reparentValue) : null,
      });
      notify('tc.departments.parentChanged', { type: 'success' });
      setReparentTarget(null); setReparentValue('');
      load();
    } catch (e: any) {
      setReparentError(friendlyError(apiErrorCode(e)));
    }
  };

  const removeDepartment = async (dept: Department) => {
    try {
      await energyHttp.delete(`${base}/departments/${dept.id}`);
      notify('tc.departments.deleted', { type: 'success' });
      load();
    } catch (e: any) {
      notify(apiErrorCode(e) || 'delete failed', { type: 'error' });
    }
  };

  const submitAssign = async () => {
    if (!assignTarget) return;
    setAssignError(null);
    if (!assignUserId) { setAssignError(translate('tc.departments.pickUser')); return; }
    try {
      await energyHttp.put(`${base}/departments/${assignTarget.id}/members/${Number(assignUserId)}`, {
        role: assignRole,
      });
      notify('tc.departments.memberAssigned', { type: 'success' });
      setAssignTarget(null); setAssignUserId(''); setAssignRole('member');
      load();
    } catch (e: any) {
      setAssignError(apiErrorCode(e) || 'assign failed');
    }
  };

  const removeMember = async (dept: Department, userId: number) => {
    try {
      await energyHttp.delete(`${base}/departments/${dept.id}/members/${userId}`);
      notify('tc.departments.memberRemoved', { type: 'success' });
      load();
    } catch (e: any) {
      notify(apiErrorCode(e) || 'remove failed', { type: 'error' });
    }
  };

  if (!orgId) {
    return <Box p={3}><Alert severity="info">{translate('tc.departments.selectScope')}</Alert></Box>;
  }
  if (denied) {
    return <Box p={3}><Alert severity="warning">{translate('tc.departments.noAccess')}</Alert></Box>;
  }

  return (
    // `position: relative` ＝ 下面那個 loading overlay 的錨點（見它的註解）。
    <Box p={2} position="relative">
      <Stack direction="row" alignItems="center" justifyContent="space-between" mb={2}>
        <Typography variant="h6">
          {translate('tc.departments.title')}
          {orgName ? ` — ${orgName}` : ''}
        </Typography>
        <Stack direction="row" gap={1}>
          <Tooltip title={translate('ra.action.refresh')}>
            <IconButton onClick={load} disabled={loading}><RefreshIcon /></IconButton>
          </Tooltip>
          <Button variant="contained" startIcon={<AddIcon />} onClick={() => { setCreateError(null); setCreateOpen(true); }}>
            {translate('tc.departments.create')}
          </Button>
        </Stack>
      </Stack>

      <Tabs value={tab} onChange={(_e, v) => setTab(v)} sx={{ mb: 1 }} data-testid="departments-tabs">
        <Tab value="list" label={translate('tc.departments.tabList')} data-testid="departments-tab-list" />
        <Tab value="chart" icon={<AccountTreeIcon fontSize="small" />} iconPosition="start" label={translate('tc.departments.tabChart')} data-testid="departments-tab-chart" />
      </Tabs>

      {/*
        **載入指示不可以佔版面空間。**
        原本這是一個正常元素，`loading` 一 true 就多出 56px、
        底下所有內容往下掉，載完又彈回來 —— 那就是「跳 popup 時畫面抖一下」。
        這一頁沒有走 `EnergyPageFrame`，所以它自己刻了一份同樣的東西，
        也要自己修一次（`shared.tsx` 的 `EnergyPageFrame` 是另外那 15 頁的）。
      */}
      {loading && (
        <Box
          data-testid="page-loading"
          sx={{
            position: 'absolute',
            top: 8,
            left: 0,
            right: 0,
            display: 'flex',
            justifyContent: 'center',
            zIndex: 2,
            pointerEvents: 'none',
          }}
        >
          <CircularProgress size={24} />
        </Box>
      )}

      {tab === 'chart' ? (
        <Card>
          <CardContent>
            <OrgChart orgName={orgName} departments={departments} />
          </CardContent>
        </Card>
      ) : (
      <Card>
        <CardContent>
          <Typography variant="subtitle1" gutterBottom>
            {translate('tc.departments.list')}（{departments.length}）
          </Typography>
          <Table size="small">
            <TableHead>
              <TableRow>
                <TableCell>{translate('tc.departments.name')}</TableCell>
                <TableCell>{translate('tc.departments.key')}</TableCell>
                <TableCell>{translate('tc.departments.parent')}</TableCell>
                <TableCell>{translate('tc.departments.managers')}</TableCell>
                <TableCell>{translate('tc.departments.members')}</TableCell>
                <TableCell align="right" />
              </TableRow>
            </TableHead>
            <TableBody>
              {departments.map((d) => {
                const managers = d.members.filter((m) => m.role === 'manager');
                const plain = d.members.filter((m) => m.role !== 'manager');
                const chip = (m: DepartmentMember, color: any) => (
                  <Chip
                    key={m.userId}
                    size="small"
                    color={color}
                    label={m.name || `#${m.userId}`}
                    data-testid="department-member"
                    data-user={m.userId}
                    data-role={m.role}
                    onDelete={() => removeMember(d, m.userId)}
                    sx={{ mr: 0.5, mb: 0.5 }}
                  />
                );
                const parentName = d.parentId != null ? departments.find((p) => p.id === d.parentId)?.name : null;
                return (
                  <TableRow key={d.id} data-testid="department-row" data-key={d.key}>
                    <TableCell>{d.name}</TableCell>
                    <TableCell><Typography variant="caption" color="text.secondary">{d.key}</Typography></TableCell>
                    <TableCell data-testid="department-parent">
                      {parentName || <Typography variant="caption" color="text.secondary">—</Typography>}
                    </TableCell>
                    <TableCell>
                      {managers.length
                        ? managers.map((m) => chip(m, 'primary'))
                        : <Typography variant="caption" color="text.secondary">—</Typography>}
                    </TableCell>
                    <TableCell>
                      {plain.length
                        ? plain.map((m) => chip(m, 'default'))
                        : <Typography variant="caption" color="text.secondary">—</Typography>}
                    </TableCell>
                    <TableCell align="right">
                      <Tooltip title={translate('tc.departments.changeParent')}>
                        <IconButton
                          size="small"
                          data-testid="department-reparent"
                          onClick={() => { setReparentError(null); setReparentValue(d.parentId != null ? String(d.parentId) : ''); setReparentTarget(d); }}
                        >
                          <AccountTreeIcon fontSize="small" />
                        </IconButton>
                      </Tooltip>
                      <Tooltip title={translate('tc.departments.assign')}>
                        <IconButton size="small" data-testid="department-assign" onClick={() => { setAssignError(null); setAssignTarget(d); }}>
                          <PersonAddIcon fontSize="small" />
                        </IconButton>
                      </Tooltip>
                      <Tooltip title={translate('tc.departments.delete')}>
                        <IconButton size="small" data-testid="department-delete" onClick={() => removeDepartment(d)}>
                          <DeleteIcon fontSize="small" />
                        </IconButton>
                      </Tooltip>
                    </TableCell>
                  </TableRow>
                );
              })}
              {!departments.length && !loading && (
                <TableRow>
                  <TableCell colSpan={6}>
                    <Typography variant="body2" color="text.secondary">{translate('tc.departments.none')}</Typography>
                  </TableCell>
                </TableRow>
              )}
            </TableBody>
          </Table>
        </CardContent>
      </Card>
      )}

      {/* 新增部門 */}
      <Dialog open={createOpen} onClose={() => setCreateOpen(false)} fullWidth maxWidth="xs">
        <DialogTitle>{translate('tc.departments.create')}</DialogTitle>
        <DialogContent>
          <Box display="flex" flexDirection="column" gap={2} mt={1}>
            <TextField
              autoFocus
              label={translate('tc.departments.name')}
              value={newName}
              onChange={(e) => setNewName(e.target.value)}
            />
            <TextField
              label={translate('tc.departments.key')}
              value={newKey}
              onChange={(e) => setNewKey(e.target.value)}
              helperText={translate('tc.departments.keyHelp')}
            />
            <TextField
              select
              data-testid="department-create-parent"
              label={translate('tc.departments.parent')}
              value={newParentId}
              onChange={(e) => setNewParentId(e.target.value)}
            >
              <MenuItem value="">{translate('tc.departments.topLevel')}</MenuItem>
              {departments.map((d) => (
                <MenuItem key={d.id} value={String(d.id)} data-testid={`parent-option-${d.id}`}>{d.name}</MenuItem>
              ))}
            </TextField>
            {createError && <Alert severity="error">{createError}</Alert>}
          </Box>
        </DialogContent>
        <DialogActions>
          <Button onClick={() => setCreateOpen(false)}>{translate('ra.action.cancel')}</Button>
          <Button variant="contained" onClick={createDepartment}>{translate('ra.action.create')}</Button>
        </DialogActions>
      </Dialog>

      {/* 改上層 */}
      <Dialog open={!!reparentTarget} onClose={() => setReparentTarget(null)} fullWidth maxWidth="xs">
        <DialogTitle>{translate('tc.departments.changeParentTitle', { name: reparentTarget?.name })}</DialogTitle>
        <DialogContent>
          <Box display="flex" flexDirection="column" gap={2} mt={1}>
            <TextField
              select
              data-testid="department-reparent-select"
              label={translate('tc.departments.parent')}
              value={reparentValue}
              onChange={(e) => setReparentValue(e.target.value)}
            >
              <MenuItem value="">{translate('tc.departments.topLevel')}</MenuItem>
              {departments
                .filter((d) => !reparentTarget || !selfAndDescendants(departments, reparentTarget.id).has(d.id))
                .map((d) => (
                  <MenuItem key={d.id} value={String(d.id)} data-testid={`reparent-option-${d.id}`}>{d.name}</MenuItem>
                ))}
            </TextField>
            {reparentError && <Alert severity="error" data-testid="reparent-error">{reparentError}</Alert>}
          </Box>
        </DialogContent>
        <DialogActions>
          <Button onClick={() => setReparentTarget(null)}>{translate('ra.action.cancel')}</Button>
          <Button variant="contained" onClick={submitReparent} data-testid="department-reparent-submit">{translate('ra.action.save')}</Button>
        </DialogActions>
      </Dialog>

      {/* 指派成員 */}
      <Dialog open={!!assignTarget} onClose={() => setAssignTarget(null)} fullWidth maxWidth="xs">
        <DialogTitle>{translate('tc.departments.assignTitle', { name: assignTarget?.name })}</DialogTitle>
        <DialogContent>
          <Box display="flex" flexDirection="column" gap={2} mt={1}>
            <TextField
              select={members.length > 0}
              data-testid="department-assign-user"
              label={translate('tc.departments.user')}
              value={assignUserId}
              onChange={(e) => setAssignUserId(e.target.value)}
              helperText={members.length ? undefined : translate('tc.departments.userIdHelp')}
            >
              {members.map((m) => (
                <MenuItem key={m.id} value={String(m.id)} data-testid={`member-option-${m.id}`}>{m.name || `#${m.id}`}</MenuItem>
              ))}
            </TextField>
            <TextField
              select
              data-testid="department-assign-role"
              label={translate('tc.departments.role')}
              value={assignRole}
              onChange={(e) => setAssignRole(e.target.value as any)}
            >
              <MenuItem value="manager">{translate('tc.departments.roleManager')}</MenuItem>
              <MenuItem value="member">{translate('tc.departments.roleMember')}</MenuItem>
            </TextField>
            {assignError && <Alert severity="error">{assignError}</Alert>}
          </Box>
        </DialogContent>
        <DialogActions>
          <Button onClick={() => setAssignTarget(null)}>{translate('ra.action.cancel')}</Button>
          <Button variant="contained" onClick={submitAssign} data-testid="department-assign-submit">{translate('tc.departments.assign')}</Button>
        </DialogActions>
      </Dialog>
    </Box>
  );
};

export default DepartmentsPage;
