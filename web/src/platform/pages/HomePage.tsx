// 首頁：目前的組織／專案與有效權限摘要；沒有任何組織時引導到「我的邀請」或（超級使用者）建立組織。
import * as React from 'react';
import { Alert, Box, Button, Card, CardContent, Chip, Stack, Typography } from '@mui/material';
import { Link } from 'react-router-dom';
import { Title, useGetIdentity, usePermissions, useTranslate } from 'react-admin';
import { useSelectionState } from '../selection/store';
import { isSuperUser } from '../permissions';
import { CreateOrganizationCard } from './OrganizationSettingsPage';

const HomePage: React.FC = () => {
  const translate = useTranslate();
  const { identity } = useGetIdentity();
  const { permissions } = usePermissions();
  const { loaded, selection, orgs } = useSelectionState();
  const perms = (permissions as string[] | undefined) || [];
  const superUser = isSuperUser(perms);
  return (
    <Box p={2} maxWidth={900}>
      <Title title={translate('eo.home.title')} />
      <Typography variant="h5" gutterBottom data-testid="home-greeting">
        {translate('eo.home.hello', { name: identity?.fullName || '' })}
      </Typography>
      {loaded && !orgs.length && (
        <Alert severity="info" sx={{ mb: 2 }} action={<Button component={Link} to="/my-invitations">{translate('tc.inbox.title')}</Button>}>
          {translate('eo.home.noOrganizations')}
        </Alert>
      )}
      {selection && (
        <Card sx={{ mb: 2 }}>
          <CardContent>
            <Typography variant="overline">{translate('eo.home.current')}</Typography>
            <Typography variant="h6" data-testid="home-current-org">
              {selection.organizationName}
              {selection.projectName && !selection.projectIsDefault ? ` / ${selection.projectName}` : ''}
            </Typography>
            <Typography variant="body2" color="text.secondary" gutterBottom>{translate('eo.home.permissions')}</Typography>
            <Stack direction="row" gap={0.5} flexWrap="wrap" data-testid="home-permissions">
              {superUser
                ? <Chip size="small" color="secondary" label={translate('eo.profile.superUser')} />
                : perms.map((p) => <Chip key={p} size="small" variant="outlined" label={p} />)}
              {!superUser && !perms.length && <Typography variant="caption" color="text.secondary">—</Typography>}
            </Stack>
          </CardContent>
        </Card>
      )}
      {superUser && <CreateOrganizationCard />}
    </Box>
  );
};

export default HomePage;
