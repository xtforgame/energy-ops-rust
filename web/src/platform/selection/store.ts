// 目前選定的組織／專案（多組織）。全站唯一來源：能源頁的 orgId、getPermissions、選單都讀這裡。
// 選擇的優先序：網址 ?org=／?project= → localStorage 上次的選擇 → 第一個非個人組織的 lobby。
import * as React from 'react';
import { meApi } from '../../api/platform';
import type { Id, MyOrganization, MyProject } from '../../api/types';

export const STORAGE_KEY = 'eo.selection';

export interface Selection {
  organizationId: Id;
  organizationName: string;
  organizationKind: string;
  projectId: Id | null;
  projectName: string | null;
  projectIsDefault: boolean;
}

export interface SelectionOption extends Selection {
  isPersonal: boolean;
}

interface State {
  loaded: boolean;
  orgs: MyOrganization[];
  selection: Selection | null;
}

type Listener = () => void;

/** 只讀網址參數裡的數字 id；hash 路由的 query（#/x?org=1）與一般 query 都收。 */
export function readUrlSelection(loc: { search?: string; hash?: string } = window.location): { org?: Id; project?: Id } {
  const parse = (s: string) => new URLSearchParams(s.startsWith('?') ? s : `?${s}`);
  const sources = [parse(loc.search || '')];
  const hash = loc.hash || '';
  const qi = hash.indexOf('?');
  if (qi >= 0) sources.push(parse(hash.slice(qi + 1)));
  const out: { org?: Id; project?: Id } = {};
  for (const p of sources) {
    const o = Number(p.get('org'));
    const pr = Number(p.get('project'));
    if (Number.isInteger(o) && o > 0) out.org = o;
    if (Number.isInteger(pr) && pr > 0) out.project = pr;
  }
  return out;
}

export function readStoredSelection(storage: Storage | undefined = safeStorage()): { org?: Id; project?: Id | null } {
  try {
    const raw = storage?.getItem(STORAGE_KEY);
    if (!raw) return {};
    const v = JSON.parse(raw);
    return {
      org: typeof v?.organizationId === 'number' ? v.organizationId : undefined,
      project: typeof v?.projectId === 'number' ? v.projectId : null,
    };
  } catch {
    return {};
  }
}

function safeStorage(): Storage | undefined {
  try {
    return typeof window !== 'undefined' ? window.localStorage : undefined;
  } catch {
    return undefined;
  }
}

/** 個人組織排最後（穩定排序，不動同類的原有順序）。 */
export function sortOrgs(orgs: MyOrganization[]): MyOrganization[] {
  return orgs
    .map((o, i) => ({ o, i }))
    .sort((a, b) => Number(a.o.kind === 'personal') - Number(b.o.kind === 'personal') || a.i - b.i)
    .map((x) => x.o);
}

const lobbyOf = (o: MyOrganization): MyProject | undefined => o.projects.find((p) => p.isDefault) ?? o.projects[0];

function toSelection(o: MyOrganization, p: MyProject | undefined | null): Selection {
  return {
    organizationId: o.id,
    organizationName: o.name,
    organizationKind: o.kind,
    projectId: p?.id ?? null,
    projectName: p?.name ?? null,
    projectIsDefault: !!p?.isDefault,
  };
}

/** 依網址、上次選擇、預設順序決定要選哪一個；找不到的 id 一律忽略。 */
export function resolveSelection(
  orgs: MyOrganization[],
  url: { org?: Id; project?: Id },
  stored: { org?: Id; project?: Id | null },
): Selection | null {
  const sorted = sortOrgs(orgs);
  const findProject = (pid: Id) => {
    for (const o of sorted) {
      const p = o.projects.find((x) => x.id === pid);
      if (p) return { o, p };
    }
    return null;
  };
  const pick = (org?: Id, project?: Id | null): Selection | null => {
    if (project) {
      const hit = findProject(project);
      if (hit && (!org || hit.o.id === org)) return toSelection(hit.o, hit.p);
    }
    if (org) {
      const o = sorted.find((x) => x.id === org);
      if (o) return toSelection(o, lobbyOf(o));
    }
    return null;
  };
  return (
    pick(url.org, url.project)
    ?? pick(stored.org, stored.project)
    ?? (sorted[0] ? toSelection(sorted[0], lobbyOf(sorted[0])) : null)
  );
}

/** 下拉選單的選項：每個組織的每個專案（lobby 在前），個人組織最後。 */
export function buildOptions(orgs: MyOrganization[]): SelectionOption[] {
  return sortOrgs(orgs).flatMap((o) => {
    const ps = [...o.projects].sort((a, b) => Number(b.isDefault) - Number(a.isDefault));
    const list = ps.length ? ps : [null];
    return list.map((p) => ({ ...toSelection(o, p), isPersonal: o.kind === 'personal' }));
  });
}

export class SelectionStore {
  private state: State = { loaded: false, orgs: [], selection: null };

  private listeners = new Set<Listener>();

  private loading: Promise<State> | null = null;

  private urlConsumed = false;

  constructor(
    private readonly fetchOrgs: () => Promise<{ items: MyOrganization[] }> = meApi.organizations,
    private readonly storage: () => Storage | undefined = safeStorage,
    private readonly location: () => { search?: string; hash?: string } = () => window.location,
  ) {}

  getState = (): State => this.state;

  getSelection = (): Selection | null => this.state.selection;

  subscribe = (fn: Listener): (() => void) => {
    this.listeners.add(fn);
    return () => {
      this.listeners.delete(fn);
    };
  };

  private set(next: Partial<State>) {
    this.state = { ...this.state, ...next };
    this.listeners.forEach((fn) => fn());
  }

  /** 載入我的組織並決定選擇；force 時重抓（新建組織、接受邀請後）。 */
  load(force = false): Promise<State> {
    if (this.loading && !force) return this.loading;
    if (this.state.loaded && !force) return Promise.resolve(this.state);
    this.loading = this.fetchOrgs()
      .then(({ items }) => {
        const orgs = sortOrgs(items || []);
        const current = this.state.selection;
        // 網址參數只在第一次載入時生效；之後以畫面上的選擇為準（切換後再重抓不會被網址拉回去）。
        const url = this.urlConsumed ? {} : readUrlSelection(this.location());
        this.urlConsumed = true;
        const selection = resolveSelection(
          orgs,
          url,
          current ? { org: current.organizationId, project: current.projectId } : readStoredSelection(this.storage()),
        );
        this.persist(selection);
        this.set({ loaded: true, orgs, selection });
        return this.state;
      })
      .finally(() => {
        this.loading = null;
      });
    return this.loading;
  }

  select(organizationId: Id, projectId?: Id | null) {
    const sel = resolveSelection(this.state.orgs, {}, { org: organizationId, project: projectId ?? null });
    if (!sel) return;
    const cur = this.state.selection;
    if (cur && cur.organizationId === sel.organizationId && cur.projectId === sel.projectId) return;
    this.persist(sel);
    this.set({ selection: sel });
  }

  /** 已載入後網址帶了 ?org=／?project=（例如點了分享連結）就切過去。 */
  applyUrl(loc: { search?: string; hash?: string } = this.location()) {
    if (!this.state.loaded) return;
    const url = readUrlSelection(loc);
    if (!url.org && !url.project) return;
    const sel = resolveSelection(this.state.orgs, url, {});
    if (sel && (url.project ? sel.projectId === url.project : sel.organizationId === url.org)) {
      this.select(sel.organizationId, sel.projectId);
    }
  }

  reset() {
    this.loading = null;
    this.urlConsumed = false;
    this.set({ loaded: false, orgs: [], selection: null });
  }

  private persist(sel: Selection | null) {
    try {
      const s = this.storage();
      if (!s) return;
      if (sel) s.setItem(STORAGE_KEY, JSON.stringify({ organizationId: sel.organizationId, projectId: sel.projectId }));
    } catch {
      // 無痕模式等情況寫不進去就算了
    }
  }
}

export const selectionStore = new SelectionStore();

export function useSelectionState(store: SelectionStore = selectionStore): State {
  return React.useSyncExternalStore(store.subscribe, store.getState, store.getState);
}

/** 目前的選擇（未載入或沒有任何組織時為 null）。 */
export function useSelection(store: SelectionStore = selectionStore): Selection | null {
  return useSelectionState(store).selection;
}

export function useSelectedOrgId(store: SelectionStore = selectionStore): Id | undefined {
  return useSelection(store)?.organizationId;
}
