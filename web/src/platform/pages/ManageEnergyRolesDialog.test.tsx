import { afterEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { AdminContext } from 'react-admin';
import polyglotI18nProvider from 'ra-i18n-polyglot';
import zhTw from '../../admin/i18n/zh-tw';
import { setEnergyAdapter } from '../../api/energy';
import { fetchAssignableRoles, ManageEnergyRolesDialog } from './ManageEnergyRolesDialog';

const i18nProvider = polyglotI18nProvider(() => zhTw as any, 'zh-TW');

const ROLES = [
  { id: 1, uniqueName: 'energy:admin', name: '系統管理員', description: '全部', privileged: true },
  { id: 2, uniqueName: 'energy:cxo', name: '高階主管', description: '加簽', privileged: true },
  { id: 3, uniqueName: 'energy:operator', name: '業務承辦', description: '送件', privileged: false },
  { id: 4, uniqueName: 'energy:finance', name: '財務', description: '放行', privileged: false },
  { id: 5, uniqueName: 'energy:viewer', name: '唯讀', description: '只看', privileged: false },
];

const reply = (status: number, data: unknown) => ({ status, statusText: '', headers: {}, data, config: {} as any });

afterEach(() => setEnergyAdapter(undefined));

describe('ManageEnergyRolesDialog（成員頁的「管理權限」）', () => {
  it('只列五個中文角色、沒有特權指派權時特權角色選不下去，送出打 role-assignments', async () => {
    const calls: Array<{ method?: string; url?: string; data?: any }> = [];
    setEnergyAdapter(async (config: any) => {
      calls.push({ method: config.method, url: config.url, data: config.data && JSON.parse(config.data) });
      return reply(200, { created: true });
    });
    const onDone = vi.fn();
    render(
      <AdminContext i18nProvider={i18nProvider}>
        <ManageEnergyRolesDialog open onClose={() => undefined} orgId={7} orgName="雲豹能源" granteeUserId={42} granteeName="王承辦"
          roles={{ items: ROLES, canAssignPrivileged: false }} onDone={onDone} />
      </AdminContext>,
    );
    fireEvent.mouseDown(await screen.findByTestId('manage-roles-role'));
    const listbox = await screen.findByRole('listbox');
    const options = within(listbox).getAllByRole('option');
    expect(options.map((o) => o.textContent)).toEqual(expect.arrayContaining([expect.stringContaining('業務承辦')]));
    expect(options).toHaveLength(5);
    expect(listbox.textContent).not.toMatch(/energy:/);
    expect(options[0].getAttribute('aria-disabled')).toBe('true');
    fireEvent.click(options[2]);
    fireEvent.click(screen.getByTestId('manage-roles-submit'));
    await waitFor(() => expect(onDone).toHaveBeenCalled());
    expect(calls).toEqual([{ method: 'post', url: '/api/v1/orgs/7/energy/role-assignments', data: { userId: 42, roleUniqueName: 'energy:operator' } }]);
  });

  it('不是能源組織或沒有指派權時不給入口', async () => {
    setEnergyAdapter(async () => Promise.reject(Object.assign(new Error('forbidden'), { response: reply(403, { code: 'forbidden' }) })));
    expect(await fetchAssignableRoles(7)).toBeNull();
    setEnergyAdapter(async () => reply(200, { items: ROLES, canAssignPrivileged: true }));
    expect(await fetchAssignableRoles(7)).toEqual({ items: ROLES, canAssignPrivileged: true });
  });
});
