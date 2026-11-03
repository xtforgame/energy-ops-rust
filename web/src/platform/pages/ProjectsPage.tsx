// 專案：列表、建立、改名、刪除（lobby 不能刪）；每個專案可開成員管理。
import * as React from 'react';
import {
  Box, Button, Card, Chip, Dialog, DialogActions, DialogContent, DialogTitle, IconButton, MenuItem, Table,
  TableBody, TableCell, TableHead, TableRow, TextField, Tooltip, Typography,
} from '@mui/material';
import AddIcon from '@mui/icons-material/Add';
import EditIcon from '@mui/icons-material/Edit';
import DeleteIcon from '@mui/icons-material/Delete';
import GroupIcon from '@mui/icons-material/Group';
import PersonAddIcon from '@mui/icons-material/PersonAdd';
import { useGetIdentity, useTranslate } from 'react-admin';
import { orgApi, projectApi } from '../../api/platform';
import type { Id, Project, ProjectMember } from '../../api/types';
import { roleDisplayName } from '../../admin/domain-logic/components/energyOps/roleDisplay';
import { selectionStore } from '../selection/store';
import {
  ConfirmDialog, ErrorAlert, fmtDate, Loading, PageHeader, RequireSelection, useAction, useAsync,
} from './common';

const UNIQUE_RE = /^[a-z0-9][a-z0-9-]{0,63}$/;

const ProjectFormDialog: React.FC<{
  open: boolean;
  project?: Project | null;
  orgId: Id;
  onClose: () => void;
  onDone: () => void;
}> = ({ open, project, orgId, onClose, onDone }) => {
  const translate = useTranslate();
  const run = useAction();
  const [name, setName] = React.useState('');
  const [uniqueName, setUniqueName] = React.useState('');
  const [busy, setBusy] = React.useState(false);
  React.useEffect(() => {
    if (open) {
      setName(project?.name || '');
      setUniqueName(project?.uniqueName || '');
    }
  }, [open, project]);
  const editing = !!project;
  const uniqueOk = editing || (UNIQUE_RE.test(uniqueName) && uniqueName !== 'lobby');
  const submit = async () => {
    setBusy(true);
    const ok = await run(
      () => (editing
        ? projectApi.rename(project!.id, name.trim())
        : orgApi.createProject(orgId, { uniqueName, name: name.trim() })),
      editing ? 'eo.projects.renamed' : 'eo.projects.created',
    );
    setBusy(false);
    if (ok) {
      onDone();
      onClose();
    }
  };
  return (
    <Dialog open={open} onClose={onClose} fullWidth maxWidth="xs">
      <DialogTitle>{translate(editing ? 'eo.projects.rename' : 'eo.projects.create')}</DialogTitle>
      <DialogContent>
        {!editing && (
          <TextField
            fullWidth
            margin="dense"
            label={translate('eo.common.uniqueName')}
            helperText={translate('eo.common.uniqueNameHelp')}
            error={!!uniqueName && !uniqueOk}
            value={uniqueName}
            onChange={(e) => setUniqueName(e.target.value.toLowerCase())}
            inputProps={{ 'data-testid': 'project-unique-name' }}
          />
        )}
        <TextField
          fullWidth
          margin="dense"
          label={translate('eo.common.name')}
          value={name}
          onChange={(e) => setName(e.target.value)}
          inputProps={{ 'data-testid': 'project-name' }}
        />
      </DialogContent>
      <DialogActions>
        <Button onClick={onClose}>{translate('ra.action.cancel')}</Button>
        <Button variant="contained" disabled={busy || !name.trim() || !uniqueOk} onClick={submit} data-testid="project-submit">
          {translate('ra.action.save')}
        </Button>
      </DialogActions>
    </Dialog>
  );
};

/** 專案成員（給專案 id）：列表、從組織成員加入、移除（lobby 不能移除）。 */
export const ProjectMembersPanel: React.FC<{ project: Pick<Project, 'id' | 'name' | 'isDefault' | 'organizationId'> }> = ({ project }) => {
  const translate = useTranslate();
  const run = useAction();
  const { identity } = useGetIdentity();
  const members = useAsync(() => projectApi.members(project.id), [project.id]);
  const orgMembers = useAsync(() => orgApi.members(project.organizationId), [project.organizationId]);
  const [pick, setPick] = React.useState('');
  const [removing, setRemoving] = React.useState<ProjectMember | null>(null);
  const current = new Set((members.data?.items || []).map((m) => m.userId));
  const candidates = (orgMembers.data?.items || []).filter((m) => !current.has(m.userId));
  return (
    <Box>
      <ErrorAlert error={members.error} />
      {members.loading && <Loading />}
      {!project.isDefault && (
        <Box display="flex" gap={1} mb={2} alignItems="center">
          <TextField
            select
            size="small"
            sx={{ minWidth: 260 }}
            label={translate('eo.projects.addMember')}
            value={pick}
            onChange={(e) => setPick(e.target.value)}
            disabled={!candidates.length}
            helperText={orgMembers.error ? translate('eo.projects.orgMembersUnavailable') : undefined}
            SelectProps={{ SelectDisplayProps: { 'data-testid': 'project-member-pick' } as any }}
          >
            {candidates.map((m) => <MenuItem key={m.userId} value={String(m.userId)}>{m.name} {m.email ? `<${m.email}>` : ''}</MenuItem>)}
          </TextField>
          <Button
            variant="contained"
            startIcon={<PersonAddIcon />}
            disabled={!pick}
            onClick={async () => {
              if (await run(() => projectApi.addMember(project.id, Number(pick)), 'eo.members.added')) {
                setPick('');
                members.reload();
              }
            }}
          >
            {translate('eo.members.add')}
          </Button>
        </Box>
      )}
      {project.isDefault && <Typography variant="body2" color="text.secondary" mb={1}>{translate('eo.projects.lobbyNote')}</Typography>}
      {members.data && (
        <Table size="small" data-testid="project-members-table">
          <TableHead>
            <TableRow>
              <TableCell>{translate('eo.common.name')}</TableCell>
              <TableCell>Email</TableCell>
              <TableCell>{translate('eo.members.membership')}</TableCell>
              <TableCell>{translate('eo.members.roles')}</TableCell>
              <TableCell align="right" />
            </TableRow>
          </TableHead>
          <TableBody>
            {members.data.items.map((m) => (
              <TableRow key={m.userId}>
                <TableCell>{m.name}</TableCell>
                <TableCell>{m.email || '—'}</TableCell>
                <TableCell>{m.membershipRole}</TableCell>
                <TableCell>{m.roles.map((r) => <Chip key={r} size="small" label={roleDisplayName(r)} title={r} sx={{ mr: 0.5 }} />)}</TableCell>
                <TableCell align="right">
                  {!project.isDefault && m.userId !== identity?.id && (
                    <IconButton size="small" onClick={() => setRemoving(m)}><DeleteIcon fontSize="small" /></IconButton>
                  )}
                </TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      )}
      <ConfirmDialog
        open={!!removing}
        danger
        title={translate('eo.members.removeTitle', { name: removing?.name || '' })}
        onClose={() => setRemoving(null)}
        onConfirm={async () => {
          if (!removing) return;
          if (await run(() => projectApi.removeMember(project.id, removing.userId), 'eo.members.removed')) {
            setRemoving(null);
            members.reload();
          }
        }}
      />
    </Box>
  );
};

const Projects: React.FC<{ orgId: Id; orgName: string }> = ({ orgId, orgName }) => {
  const translate = useTranslate();
  const run = useAction();
  const list = useAsync(() => orgApi.projects(orgId), [orgId]);
  const [form, setForm] = React.useState<{ open: boolean; project?: Project | null }>({ open: false });
  const [deleting, setDeleting] = React.useState<Project | null>(null);
  const [membersOf, setMembersOf] = React.useState<Project | null>(null);
  // 專案有異動時，切換器的清單也要更新
  const changed = () => {
    list.reload();
    selectionStore.load(true).catch(() => undefined);
  };
  return (
    <Box p={2}>
      <PageHeader
        title={translate('eo.projects.title')}
        subtitle={orgName}
        onRefresh={list.reload}
        actions={(
          <Button variant="contained" startIcon={<AddIcon />} onClick={() => setForm({ open: true, project: null })} data-testid="project-create">
            {translate('eo.projects.create')}
          </Button>
        )}
      />
      <ErrorAlert error={list.error} />
      {list.loading && <Loading />}
      {list.data && (
        <Card>
          <Table size="small" data-testid="projects-table">
            <TableHead>
              <TableRow>
                <TableCell>{translate('eo.common.name')}</TableCell>
                <TableCell>{translate('eo.common.uniqueName')}</TableCell>
                <TableCell>{translate('eo.common.type')}</TableCell>
                <TableCell>{translate('eo.common.createdAt')}</TableCell>
                <TableCell align="right" />
              </TableRow>
            </TableHead>
            <TableBody>
              {list.data.items.map((p) => (
                <TableRow key={p.id} data-testid={`project-row-${p.id}`}>
                  <TableCell>
                    {p.name}
                    {p.isDefault && <Chip size="small" label="lobby" sx={{ ml: 1 }} />}
                  </TableCell>
                  <TableCell><code>{p.uniqueName}</code></TableCell>
                  <TableCell>{p.type}</TableCell>
                  <TableCell>{fmtDate(p.createdAt)}</TableCell>
                  <TableCell align="right">
                    <Tooltip title={translate('eo.projects.members')}>
                      <IconButton size="small" onClick={() => setMembersOf(p)}><GroupIcon fontSize="small" /></IconButton>
                    </Tooltip>
                    <Tooltip title={translate('eo.projects.rename')}>
                      <IconButton size="small" onClick={() => setForm({ open: true, project: p })}><EditIcon fontSize="small" /></IconButton>
                    </Tooltip>
                    {!p.isDefault && (
                      <Tooltip title={translate('ra.action.delete')}>
                        <IconButton size="small" onClick={() => setDeleting(p)} data-testid={`project-delete-${p.id}`}>
                          <DeleteIcon fontSize="small" />
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
      <ProjectFormDialog open={form.open} project={form.project} orgId={orgId} onClose={() => setForm({ open: false })} onDone={changed} />
      <ConfirmDialog
        open={!!deleting}
        danger
        title={translate('eo.projects.deleteTitle', { name: deleting?.name || '' })}
        content={<Typography variant="body2">{translate('eo.projects.deleteHelp')}</Typography>}
        onClose={() => setDeleting(null)}
        onConfirm={async () => {
          if (!deleting) return;
          if (await run(() => projectApi.delete(deleting.id), 'eo.projects.deleted')) {
            setDeleting(null);
            changed();
          }
        }}
      />
      <Dialog open={!!membersOf} onClose={() => setMembersOf(null)} fullWidth maxWidth="md">
        <DialogTitle>{translate('eo.projects.members')} — {membersOf?.name}</DialogTitle>
        <DialogContent>{membersOf && <ProjectMembersPanel project={membersOf} />}</DialogContent>
        <DialogActions><Button onClick={() => setMembersOf(null)}>{translate('ra.action.close')}</Button></DialogActions>
      </Dialog>
    </Box>
  );
};

const ProjectsPage: React.FC = () => (
  <RequireSelection>{(sel) => <Projects key={sel.organizationId} orgId={sel.organizationId} orgName={sel.organizationName} />}</RequireSelection>
);

/** 目前選擇的專案的成員頁（選單「專案成員」）。 */
export const ProjectMembersPage: React.FC = () => {
  const translate = useTranslate();
  return (
    <RequireSelection needProject>
      {(s) => (
        <Box p={2}>
          <PageHeader title={translate('eo.projects.members')} subtitle={`${s.organizationName} / ${s.projectName}`} />
          <Card sx={{ p: 2 }}>
            <ProjectMembersPanel
              key={s.projectId}
              project={{ id: s.projectId!, name: s.projectName || '', isDefault: s.projectIsDefault, organizationId: s.organizationId }}
            />
          </Card>
        </Box>
      )}
    </RequireSelection>
  );
};

export default ProjectsPage;
