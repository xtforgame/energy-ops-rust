import * as React from 'react';
import { afterEach, describe, expect, it } from 'vitest';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { AdminContext } from 'react-admin';
import polyglotI18nProvider from 'ra-i18n-polyglot';
import { setFetch } from '../../api/http';
import { mergeMessages, platformZhTw } from '../i18n';
import ProjectSelector from './ProjectSelector';
import { selectionStore, STORAGE_KEY } from './store';

const i18nProvider = polyglotI18nProvider(() => mergeMessages({}, platformZhTw) as any, 'zh-TW');

const ORGS = {
  items: [
    { id: 1, uniqueName: 'me', name: 'Ann 的空間', kind: 'personal', membershipRole: 'owner', joinedAt: '', projects: [{ id: 11, organizationId: 1, name: 'Lobby', uniqueName: 'lobby', isDefault: true, membershipRole: 'member' }] },
    { id: 2, uniqueName: 'acme', name: 'Acme', kind: 'organization', membershipRole: 'member', joinedAt: '', projects: [
      { id: 21, organizationId: 2, name: 'Lobby', uniqueName: 'lobby', isDefault: true, membershipRole: 'member' },
      { id: 22, organizationId: 2, name: '北區案場', uniqueName: 'north', isDefault: false, membershipRole: 'member' },
    ] },
  ],
};

afterEach(() => {
  selectionStore.reset();
  localStorage.clear();
});

describe('ProjectSelector', () => {
  it('顯示目前選擇（個人組織排最後），切換後寫入 localStorage', async () => {
    setFetch((async () => new Response(JSON.stringify(ORGS), { status: 200, headers: { 'content-type': 'application/json' } })) as any);
    render(<AdminContext i18nProvider={i18nProvider}><ProjectSelector /></AdminContext>);
    const input = await screen.findByTestId('org-selector') as HTMLInputElement;
    await waitFor(() => expect(input.value).toBe('Acme / 整個組織'));

    fireEvent.mouseDown(input);
    fireEvent.keyDown(input, { key: 'ArrowDown' });
    const listbox = await screen.findByRole('listbox');
    // 群組順序：Acme 在前、個人組織在後
    const groups = within(listbox).getAllByText(/Acme|Ann 的空間/).map((n) => n.textContent);
    expect(groups[0]).toBe('Acme');
    expect(groups[groups.length - 1]).toContain('Ann 的空間');

    fireEvent.click(within(listbox).getByTestId('org-option-2-22'));
    await waitFor(() => expect(input.value).toBe('Acme / 北區案場'));
    expect(JSON.parse(localStorage.getItem(STORAGE_KEY)!)).toEqual({ organizationId: 2, projectId: 22 });
    expect(selectionStore.getSelection()).toMatchObject({ organizationId: 2, projectId: 22 });
  });
});
