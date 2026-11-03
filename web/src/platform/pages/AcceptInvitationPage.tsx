// 公開頁：用邀請連結的 token 看邀請內容；已登入就直接接受，未登入的人可登入，未註冊的人先註冊再接受。
import * as React from 'react';
import {
  Alert, Box, Button, Card, CardActions, CardContent, CircularProgress, Stack, TextField, Typography,
} from '@mui/material';
import { useTranslate } from 'react-admin';
import { useLocation } from 'react-router-dom';
import { authApi, invitationApi } from '../../api/platform';
import { isApiError } from '../../api/http';
import type { InvitationSummary, SessionInfo } from '../../api/types';
import { useErrorText } from './common';
import { presetLabel, usePresets } from './InvitationsPage';

const sameEmail = (a?: string | null, b?: string | null) => !!a && !!b && a.trim().toLowerCase() === b.trim().toLowerCase();

/** 接受完整頁重載回首頁：讓 react-admin 的登入狀態、權限與組織清單全部重新取得。 */
function goHome(inv?: { organizationId?: number; projectId?: number | null }) {
  const q = inv?.organizationId ? `?org=${inv.organizationId}${inv.projectId ? `&project=${inv.projectId}` : ''}` : '';
  window.location.hash = `#/${q}`;
  window.location.reload();
}

const AcceptInvitationPage: React.FC = () => {
  const translate = useTranslate();
  const errorText = useErrorText();
  const location = useLocation();
  const token = new URLSearchParams(location.search).get('token') || '';
  const presets = usePresets();
  const [summary, setSummary] = React.useState<InvitationSummary | null>(null);
  const [session, setSession] = React.useState<SessionInfo | null>(null);
  const [loading, setLoading] = React.useState(true);
  const [loadError, setLoadError] = React.useState<unknown>(null);
  const [busy, setBusy] = React.useState(false);
  const [error, setError] = React.useState<string | null>(null);
  const [name, setName] = React.useState('');
  const [password, setPassword] = React.useState('');
  const [password2, setPassword2] = React.useState('');

  const refresh = React.useCallback(async () => {
    setLoading(true);
    setLoadError(null);
    try {
      const [s, me] = await Promise.all([
        invitationApi.byToken(token),
        authApi.session().catch(() => null),
      ]);
      setSummary(s);
      setSession(me);
    } catch (e) {
      setLoadError(e);
    } finally {
      setLoading(false);
    }
  }, [token]);

  React.useEffect(() => {
    if (token) refresh();
    else setLoading(false);
  }, [token, refresh]);

  const act = async (fn: () => Promise<void>) => {
    setBusy(true);
    setError(null);
    try {
      await fn();
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(false);
    }
  };

  const accept = () => act(async () => {
    const inv = await invitationApi.acceptByToken(token);
    goHome(inv);
  });

  const loginAndAccept = () => act(async () => {
    await authApi.login(summary!.inviteeEmail, password);
    const inv = await invitationApi.acceptByToken(token);
    goHome(inv);
  });

  const registerAndAccept = () => act(async () => {
    if (password !== password2) throw new Error(translate('eo.profile.passwordMismatch'));
    await authApi.register({ name: name.trim(), email: summary!.inviteeEmail, password, invitationToken: token });
    const inv = await invitationApi.acceptByToken(token);
    goHome(inv);
  });

  const target = summary
    ? (summary.projectName ? `${summary.organizationName} / ${summary.projectName}` : summary.organizationName)
    : '';

  let body: React.ReactNode = null;
  if (!token) {
    body = <Alert severity="error">{translate('tc.accept.noToken')}</Alert>;
  } else if (loading) {
    body = <Box display="flex" justifyContent="center" p={3}><CircularProgress size={26} /></Box>;
  } else if (loadError || !summary) {
    body = (
      <Alert severity="error" data-testid="accept-error">
        {isApiError(loadError) && loadError.status === 404 ? translate('tc.accept.notFound') : errorText(loadError)}
      </Alert>
    );
  } else {
    const pending = summary.status === 'pending';
    const mine = session && sameEmail(session.user.email, summary.inviteeEmail);
    body = (
      <>
        <CardContent>
          <Typography variant="h6" gutterBottom>{translate('tc.accept.title')}</Typography>
          <Typography variant="body1" data-testid="accept-summary">
            {translate('tc.accept.line', {
              inviter: summary.inviterName,
              role: presetLabel(translate, presets.data, summary.inviteRoleKey),
              target,
            })}
          </Typography>
          <Typography variant="body2" color="text.secondary" gutterBottom>
            {translate('tc.accept.forEmail', { email: summary.inviteeEmail })}
          </Typography>
          {!pending && (
            <Alert severity="info" sx={{ mt: 2 }}>
              {translate('tc.accept.alreadyResolved', {
                status: translate(`eo.invitationStatus.${summary.status}`, { _: summary.status }),
              })}
            </Alert>
          )}
          {pending && session && !mine && (
            <Alert severity="warning" sx={{ mt: 2 }}>{translate('tc.accept.wrongAccount', { email: summary.inviteeEmail })}</Alert>
          )}
          {pending && !session && summary.inviteeRegistered && (
            <Stack gap={1} mt={2}>
              <Typography variant="body2">{translate('tc.accept.loginFirst')}</Typography>
              <TextField label="Email" value={summary.inviteeEmail} InputProps={{ readOnly: true }} size="small" />
              <TextField
                label={translate('ra.auth.password')}
                type="password"
                size="small"
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                inputProps={{ 'data-testid': 'accept-login-password' }}
              />
            </Stack>
          )}
          {pending && !session && !summary.inviteeRegistered && (
            <Stack gap={1} mt={2}>
              <Typography variant="body2">{translate('eo.accept.registerFirst')}</Typography>
              <TextField label="Email" value={summary.inviteeEmail} InputProps={{ readOnly: true }} size="small" />
              <TextField
                label={translate('eo.common.name')}
                size="small"
                value={name}
                onChange={(e) => setName(e.target.value)}
                inputProps={{ 'data-testid': 'accept-register-name' }}
              />
              <TextField
                label={translate('ra.auth.password')}
                type="password"
                size="small"
                helperText={translate('eo.common.passwordHelp')}
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                inputProps={{ 'data-testid': 'accept-register-password' }}
              />
              <TextField
                label={translate('eo.profile.confirmPassword')}
                type="password"
                size="small"
                value={password2}
                onChange={(e) => setPassword2(e.target.value)}
                inputProps={{ 'data-testid': 'accept-register-password2' }}
              />
            </Stack>
          )}
          {error && <Alert severity="error" sx={{ mt: 2 }} data-testid="accept-action-error">{error}</Alert>}
        </CardContent>
        <CardActions sx={{ px: 2, pb: 2 }}>
          {pending && mine && (
            <Button variant="contained" disabled={busy} onClick={accept} data-testid="accept-submit">
              {translate('tc.inbox.accept')}
            </Button>
          )}
          {pending && session && !mine && (
            <Button
              variant="outlined"
              disabled={busy}
              onClick={() => act(async () => {
                await authApi.logout();
                await refresh();
              })}
            >
              {translate('ra.auth.logout')}
            </Button>
          )}
          {pending && !session && summary.inviteeRegistered && (
            <Button variant="contained" disabled={busy || !password} onClick={loginAndAccept} data-testid="accept-login-submit">
              {translate('tc.accept.loginToAccept')}
            </Button>
          )}
          {pending && !session && !summary.inviteeRegistered && (
            <Button
              variant="contained"
              disabled={busy || !name.trim() || password.length < 8}
              onClick={registerAndAccept}
              data-testid="accept-register-submit"
            >
              {translate('eo.accept.registerAndAccept')}
            </Button>
          )}
          {!pending && (
            <Button onClick={() => goHome()}>{translate('eo.accept.goHome')}</Button>
          )}
        </CardActions>
      </>
    );
  }

  return (
    <Box
      sx={{
        minHeight: '100vh',
        display: 'flex',
        justifyContent: 'center',
        alignItems: 'flex-start',
        background: 'url(/images/3056533.jpg)',
        backgroundSize: 'cover',
      }}
    >
      <Card sx={{ width: 460, maxWidth: '94vw', mt: '6em' }}>{body}</Card>
    </Box>
  );
};

export default AcceptInvitationPage;
