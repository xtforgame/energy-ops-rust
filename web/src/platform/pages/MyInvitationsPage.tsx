// 我的邀請：寄給我（email 相同）的待回覆邀請，接受或拒絕；接受後重抓組織清單並切過去。
import * as React from 'react';
import { Box, Button, Card, CardActions, CardContent, Stack, Typography } from '@mui/material';
import { useTranslate } from 'react-admin';
import { meApi } from '../../api/platform';
import type { Invitation } from '../../api/types';
import { selectionStore } from '../selection/store';
import { ErrorAlert, fmtDate, Loading, PageHeader, useAction, useAsync } from './common';
import { presetLabel, usePresets } from './InvitationsPage';

/** 接受後：重抓我的組織並切到剛加入的那一個。 */
export async function switchToAccepted(inv: Pick<Invitation, 'organizationId' | 'projectId'>) {
  await selectionStore.load(true);
  selectionStore.select(inv.organizationId, inv.projectId ?? null);
}

const MyInvitationsPage: React.FC = () => {
  const translate = useTranslate();
  const run = useAction();
  const list = useAsync(() => meApi.invitations(), []);
  const presets = usePresets();
  const items = list.data?.items || [];
  return (
    <Box p={2} maxWidth={760}>
      <PageHeader title={translate('tc.inbox.title')} onRefresh={list.reload} />
      <ErrorAlert error={list.error} />
      {list.loading && <Loading />}
      {list.data && !items.length && (
        <Typography color="text.secondary" data-testid="my-invitations-empty">{translate('tc.inbox.empty')}</Typography>
      )}
      <Stack gap={2}>
        {items.map((inv) => (
          <Card key={inv.id} data-testid={`my-invitation-${inv.id}`}>
            <CardContent>
              <Typography variant="subtitle1">
                {inv.projectName ? `${inv.organizationName} / ${inv.projectName}` : inv.organizationName}
              </Typography>
              <Typography variant="body2">
                {translate('tc.inbox.line', {
                  inviter: inv.inviterName,
                  role: presetLabel(translate, presets.data, inv.inviteRoleKey),
                  target: translate(inv.projectId ? 'tc.inbox.targetProject' : 'tc.inbox.targetOrg'),
                })}
              </Typography>
              <Typography variant="caption" color="text.secondary">
                {translate('eo.invitations.expiresAt')}：{fmtDate(inv.expiresAt)}
              </Typography>
            </CardContent>
            <CardActions>
              <Button
                variant="contained"
                data-testid={`my-invitation-accept-${inv.id}`}
                onClick={async () => {
                  const ok = await run(() => meApi.acceptInvitation(inv.id), 'tc.inbox.accepted');
                  if (ok) {
                    await switchToAccepted(inv);
                    list.reload();
                  }
                }}
              >
                {translate('tc.inbox.accept')}
              </Button>
              <Button
                onClick={async () => {
                  if (await run(() => meApi.declineInvitation(inv.id), 'tc.inbox.declined')) list.reload();
                }}
              >
                {translate('tc.inbox.decline')}
              </Button>
            </CardActions>
          </Card>
        ))}
      </Stack>
    </Box>
  );
};

export default MyInvitationsPage;
