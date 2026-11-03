// 組織設定：改組織名稱（platform:org:manage）；超級使用者另可建立新組織並切過去。
import * as React from 'react';
import { Box, Button, Card, CardActions, CardContent, TextField, Typography } from '@mui/material';
import { usePermissions, useTranslate } from 'react-admin';
import { orgApi } from '../../api/platform';
import type { Id } from '../../api/types';
import { selectionStore } from '../selection/store';
import { isSuperUser } from '../permissions';
import { ErrorAlert, Loading, PageHeader, RequireSelection, useAction, useAsync } from './common';

const UNIQUE_RE = /^[a-z0-9][a-z0-9-]{0,63}$/;

export const CreateOrganizationCard: React.FC = () => {
  const translate = useTranslate();
  const run = useAction();
  const [uniqueName, setUniqueName] = React.useState('');
  const [name, setName] = React.useState('');
  return (
    <Card>
      <CardContent>
        <Typography variant="subtitle1" gutterBottom>{translate('eo.orgs.create')}</Typography>
        <TextField
          fullWidth
          margin="dense"
          label={translate('eo.common.uniqueName')}
          helperText={translate('eo.common.uniqueNameHelp')}
          error={!!uniqueName && !UNIQUE_RE.test(uniqueName)}
          value={uniqueName}
          onChange={(e) => setUniqueName(e.target.value.toLowerCase())}
          inputProps={{ 'data-testid': 'org-create-unique-name' }}
        />
        <TextField fullWidth margin="dense" label={translate('eo.common.name')} value={name} onChange={(e) => setName(e.target.value)} inputProps={{ 'data-testid': 'org-create-name' }} />
      </CardContent>
      <CardActions sx={{ px: 2, pb: 2 }}>
        <Button
          variant="contained"
          disabled={!UNIQUE_RE.test(uniqueName) || !name.trim()}
          data-testid="org-create-submit"
          onClick={async () => {
            let id: Id | null = null;
            const ok = await run(async () => {
              id = (await orgApi.create({ uniqueName, name: name.trim() })).id;
            }, 'eo.orgs.created');
            if (ok && id) {
              setUniqueName('');
              setName('');
              await selectionStore.load(true);
              selectionStore.select(id);
            }
          }}
        >
          {translate('eo.orgs.create')}
        </Button>
      </CardActions>
    </Card>
  );
};

const Settings: React.FC<{ orgId: Id }> = ({ orgId }) => {
  const translate = useTranslate();
  const run = useAction();
  const org = useAsync(() => orgApi.get(orgId), [orgId]);
  const [name, setName] = React.useState('');
  React.useEffect(() => {
    if (org.data) setName(org.data.name);
  }, [org.data]);
  return (
    <>
      <ErrorAlert error={org.error} />
      {org.loading && <Loading />}
      {org.data && (
        <Card sx={{ mb: 2 }}>
          <CardContent>
            <Typography variant="subtitle1" gutterBottom>{translate('eo.orgs.basic')}</Typography>
            <Typography variant="body2" color="text.secondary">
              {translate('eo.common.uniqueName')}：<code>{org.data.uniqueName}</code>（{org.data.kind}）
            </Typography>
            <TextField fullWidth margin="dense" label={translate('eo.common.name')} value={name} onChange={(e) => setName(e.target.value)} />
          </CardContent>
          <CardActions sx={{ px: 2, pb: 2 }}>
            <Button
              variant="contained"
              disabled={!name.trim() || name.trim() === org.data.name}
              onClick={async () => {
                if (await run(() => orgApi.update(orgId, { name: name.trim() }), 'eo.orgs.saved')) {
                  org.reload();
                  selectionStore.load(true).catch(() => undefined);
                }
              }}
            >
              {translate('ra.action.save')}
            </Button>
          </CardActions>
        </Card>
      )}
    </>
  );
};

const OrganizationSettingsPage: React.FC = () => {
  const translate = useTranslate();
  const { permissions } = usePermissions();
  return (
    <Box p={2} maxWidth={640}>
      <PageHeader title={translate('eo.orgs.settingsTitle')} />
      <RequireSelection>{(s) => <Settings key={s.organizationId} orgId={s.organizationId} />}</RequireSelection>
      {isSuperUser(permissions as string[]) && <CreateOrganizationCard />}
    </Box>
  );
};

export default OrganizationSettingsPage;
