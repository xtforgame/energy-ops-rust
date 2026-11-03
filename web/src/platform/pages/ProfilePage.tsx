// 個人資料（名稱、頭像網址）與改密碼；改密碼後其他裝置的 session 會被後端撤掉。
import * as React from 'react';
import { Alert, Box, Button, Card, CardActions, CardContent, Chip, Stack, TextField, Typography } from '@mui/material';
import { useGetIdentity, useTranslate } from 'react-admin';
import { authApi, meApi } from '../../api/platform';
import { ErrorAlert, Loading, PageHeader, useAction, useAsync } from './common';

const ProfilePage: React.FC = () => {
  const translate = useTranslate();
  const run = useAction();
  const { refetch } = useGetIdentity();
  const session = useAsync(() => authApi.session(), []);
  const [name, setName] = React.useState('');
  const [picture, setPicture] = React.useState('');
  const [current, setCurrent] = React.useState('');
  const [next, setNext] = React.useState('');
  const [next2, setNext2] = React.useState('');
  React.useEffect(() => {
    if (session.data) {
      setName(session.data.user.name || '');
      setPicture(session.data.user.picture || '');
    }
  }, [session.data]);
  const mismatch = !!next2 && next !== next2;
  const u = session.data?.user;
  return (
    <Box p={2} maxWidth={640}>
      <PageHeader title={translate('eo.profile.title')} />
      <ErrorAlert error={session.error} />
      {session.loading && <Loading />}
      {u && (
        <Stack gap={2}>
          <Card>
            <CardContent>
              <Typography variant="subtitle1" gutterBottom>{translate('eo.profile.basic')}</Typography>
              <Typography variant="body2" color="text.secondary" gutterBottom>
                Email：{u.email || '—'}
                {session.data!.superUser && <Chip size="small" color="secondary" label={translate('eo.profile.superUser')} sx={{ ml: 1 }} />}
              </Typography>
              <TextField fullWidth margin="dense" label={translate('eo.common.name')} value={name} onChange={(e) => setName(e.target.value)} inputProps={{ 'data-testid': 'profile-name' }} />
              <TextField fullWidth margin="dense" label={translate('eo.profile.picture')} value={picture} onChange={(e) => setPicture(e.target.value)} />
            </CardContent>
            <CardActions sx={{ px: 2, pb: 2 }}>
              <Button
                variant="contained"
                disabled={!name.trim()}
                data-testid="profile-save"
                onClick={async () => {
                  const ok = await run(() => meApi.updateProfile({ name: name.trim(), ...(picture.trim() ? { picture: picture.trim() } : {}) }), 'eo.profile.saved');
                  if (ok) {
                    session.reload();
                    refetch();
                  }
                }}
              >
                {translate('ra.action.save')}
              </Button>
            </CardActions>
          </Card>
          <Card>
            <CardContent>
              <Typography variant="subtitle1" gutterBottom>{translate('eo.profile.changePassword')}</Typography>
              <Alert severity="info" sx={{ mb: 1 }}>{translate('eo.profile.otherSessionsRevoked')}</Alert>
              <TextField fullWidth margin="dense" type="password" label={translate('eo.profile.currentPassword')} value={current} onChange={(e) => setCurrent(e.target.value)} />
              <TextField fullWidth margin="dense" type="password" label={translate('eo.profile.newPassword')} helperText={translate('eo.common.passwordHelp')} value={next} onChange={(e) => setNext(e.target.value)} />
              <TextField
                fullWidth
                margin="dense"
                type="password"
                label={translate('eo.profile.confirmPassword')}
                error={mismatch}
                helperText={mismatch ? translate('eo.profile.passwordMismatch') : ' '}
                value={next2}
                onChange={(e) => setNext2(e.target.value)}
              />
            </CardContent>
            <CardActions sx={{ px: 2, pb: 2 }}>
              <Button
                variant="contained"
                disabled={!current || next.length < 8 || next !== next2}
                onClick={async () => {
                  if (await run(() => meApi.changePassword(current, next), 'eo.profile.passwordChanged')) {
                    setCurrent('');
                    setNext('');
                    setNext2('');
                  }
                }}
              >
                {translate('eo.profile.changePassword')}
              </Button>
            </CardActions>
          </Card>
        </Stack>
      )}
    </Box>
  );
};

export default ProfilePage;
