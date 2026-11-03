import { describe, expect, it, vi } from 'vitest';
import type { MyOrganization } from '../../api/types';
import {
  buildOptions, readStoredSelection, readUrlSelection, resolveSelection, SelectionStore, sortOrgs, STORAGE_KEY,
} from './store';

const org = (id: number, kind: string, projects: Array<[number, boolean]>): MyOrganization => ({
  id,
  uniqueName: `o${id}`,
  name: `Org ${id}`,
  kind,
  membershipRole: 'member',
  joinedAt: '',
  projects: projects.map(([pid, isDefault]) => ({
    id: pid, organizationId: id, name: isDefault ? 'Lobby' : `P${pid}`, uniqueName: isDefault ? 'lobby' : `p${pid}`, isDefault, membershipRole: 'member',
  })),
});

// 個人組織刻意排在第一個，驗證會被移到最後
const ORGS = [org(1, 'personal', [[11, true]]), org(2, 'organization', [[21, true], [22, false]]), org(3, 'organization', [[31, true]])];

class MemStorage implements Storage {
  private m = new Map<string, string>();

  get length() { return this.m.size; }

  clear() { this.m.clear(); }

  getItem(k: string) { return this.m.has(k) ? this.m.get(k)! : null; }

  key(i: number) { return [...this.m.keys()][i] ?? null; }

  removeItem(k: string) { this.m.delete(k); }

  setItem(k: string, v: string) { this.m.set(k, v); }
}

const makeStore = (loc: { search?: string; hash?: string } = {}, storage = new MemStorage(), orgs = ORGS) => {
  const fetchOrgs = vi.fn(async () => ({ items: orgs }));
  const store = new SelectionStore(fetchOrgs, () => storage, () => loc);
  return { store, storage, fetchOrgs };
};

describe('sortOrgs / buildOptions', () => {
  it('個人組織排最後、其餘保持原順序', () => {
    expect(sortOrgs(ORGS).map((o) => o.id)).toEqual([2, 3, 1]);
  });

  it('選項：每個專案一項、lobby 在前、個人組織標記', () => {
    const opts = buildOptions(ORGS);
    expect(opts.map((o) => [o.organizationId, o.projectId])).toEqual([[2, 21], [2, 22], [3, 31], [1, 11]]);
    expect(opts[3].isPersonal).toBe(true);
    expect(opts[0].projectIsDefault).toBe(true);
  });
});

describe('readUrlSelection', () => {
  it('一般 query 與 hash 內的 query 都讀', () => {
    expect(readUrlSelection({ search: '?org=3', hash: '' })).toEqual({ org: 3 });
    expect(readUrlSelection({ search: '', hash: '#/organizationMembers?org=2&project=22' })).toEqual({ org: 2, project: 22 });
  });

  it('非正整數忽略', () => {
    expect(readUrlSelection({ search: '?org=abc&project=-1', hash: '' })).toEqual({});
  });
});

describe('resolveSelection', () => {
  it('預設：第一個非個人組織的 lobby', () => {
    expect(resolveSelection(ORGS, {}, {})).toMatchObject({ organizationId: 2, projectId: 21, projectIsDefault: true });
  });

  it('網址優先於上次的選擇', () => {
    expect(resolveSelection(ORGS, { org: 3 }, { org: 2, project: 22 })).toMatchObject({ organizationId: 3, projectId: 31 });
  });

  it('?project= 會帶出所屬組織', () => {
    expect(resolveSelection(ORGS, { project: 22 }, {})).toMatchObject({ organizationId: 2, projectId: 22, projectName: 'P22' });
  });

  it('組織與專案不相符時退回組織的 lobby', () => {
    expect(resolveSelection(ORGS, { org: 3, project: 22 }, {})).toMatchObject({ organizationId: 3, projectId: 31 });
  });

  it('找不到的 id 一律忽略、往下一個來源', () => {
    expect(resolveSelection(ORGS, { org: 999 }, { org: 3 })).toMatchObject({ organizationId: 3 });
    expect(resolveSelection(ORGS, { org: 999 }, { org: 888 })).toMatchObject({ organizationId: 2 });
  });

  it('沒有任何組織 → null', () => {
    expect(resolveSelection([], {}, {})).toBeNull();
  });
});

describe('SelectionStore', () => {
  it('載入後寫入 localStorage；下次啟動沿用', async () => {
    const storage = new MemStorage();
    const a = makeStore({}, storage).store;
    await a.load();
    a.select(2, 22);
    expect(JSON.parse(storage.getItem(STORAGE_KEY)!)).toEqual({ organizationId: 2, projectId: 22 });
    expect(readStoredSelection(storage)).toEqual({ org: 2, project: 22 });

    const b = makeStore({}, storage).store;
    await b.load();
    expect(b.getSelection()).toMatchObject({ organizationId: 2, projectId: 22 });
  });

  it('localStorage 裡的組織已不屬於我 → 回預設', async () => {
    const storage = new MemStorage();
    storage.setItem(STORAGE_KEY, JSON.stringify({ organizationId: 404, projectId: null }));
    const { store } = makeStore({}, storage);
    await store.load();
    expect(store.getSelection()?.organizationId).toBe(2);
  });

  it('壞掉的 localStorage 內容不影響啟動', async () => {
    const storage = new MemStorage();
    storage.setItem(STORAGE_KEY, '{not json');
    const { store } = makeStore({}, storage);
    await store.load();
    expect(store.getSelection()?.organizationId).toBe(2);
  });

  it('網址參數只在第一次載入生效；之後重抓不會被拉回去', async () => {
    const { store } = makeStore({ search: '?org=3' });
    await store.load();
    expect(store.getSelection()?.organizationId).toBe(3);
    store.select(2);
    await store.load(true);
    expect(store.getSelection()?.organizationId).toBe(2);
  });

  it('applyUrl：已載入後網址帶 ?project= 就切過去；無效 id 不動', async () => {
    const { store } = makeStore();
    await store.load();
    store.applyUrl({ search: '?project=22' });
    expect(store.getSelection()).toMatchObject({ organizationId: 2, projectId: 22 });
    store.applyUrl({ search: '?org=999' });
    expect(store.getSelection()).toMatchObject({ organizationId: 2, projectId: 22 });
  });

  it('select 通知訂閱者；選同一個不重複通知', async () => {
    const { store } = makeStore();
    await store.load();
    const fn = vi.fn();
    store.subscribe(fn);
    store.select(3);
    store.select(3);
    expect(fn).toHaveBeenCalledTimes(1);
  });

  it('同時多次 load 只打一次 API；reset 後重新載入', async () => {
    const { store, fetchOrgs } = makeStore();
    await Promise.all([store.load(), store.load()]);
    expect(fetchOrgs).toHaveBeenCalledTimes(1);
    store.reset();
    expect(store.getSelection()).toBeNull();
    await store.load();
    expect(fetchOrgs).toHaveBeenCalledTimes(2);
  });

  it('只有個人組織時選它', async () => {
    const { store } = makeStore({}, new MemStorage(), [org(1, 'personal', [[11, true]])]);
    await store.load();
    expect(store.getSelection()).toMatchObject({ organizationId: 1, organizationKind: 'personal' });
  });
});
