/**
 * energy-ops-console 的 RBAC seed 宣告（permissions / roles）。
 *
 * 設計出處：`docs/features/energy-ops-console/PERMISSIONS.md` §2.5（五角色）／§2.6（permission 分層）。
 *
 * 為什麼放在 **common** 而不是 server（對照 `RyxisPlugin/rbacSeeds.ts` 放 server）：
 * tenant-console 前端的角色下拉與邀請 dialog 也要這份角色清單，前後端共用同一份真值。
 *
 * 三個必須守住的不變量（`RbacSeedManager.validateSeeds`）：
 * 1. complex permission 的 `includedPermissionUniqueNames` **不可為空**（`:103–107`）
 * → 所以 `energy:ops:view` / `energy:ops:edit` 這兩個 complex **本檔刻意不宣告**：
 * 它們的成員是領域 permission（landowner/station/contract/bill/payout 的 `:view`/`:edit`），
 * 那些 model 要到 Slice 5 才存在。現在宣告＝空 complex＝seed 驗證直接掛。
 * 2. complex **不可包 complex**（`:115–119`）→ 每個 complex 各自列全 simple。
 * 3. role 綁的 permission 必須與 role **同 scope** 宣告（`:124–137`）
 * → 本 feature 全部 `scope: 'organization'`（雲豹是單租戶內的組織治理，不切 project）。
 *
 * seed 只 upsert 不刪除（`RbacSeedManager.ts:154–191`）：uniqueName 一次定對，
 * 日後改名／砍角色都需要一次性清理腳本（`PERMISSIONS.md §2.7`）。
 */

import {
  BuiltInPermission, BuiltInRole, RbacScope, RbacSeed,
} from 'common/plugins/RbacCommonPlugin/types';

export const ENERGY_OPS_PLUGIN_NAME = 'energyOps';

/** 本 feature 的 permission / role 一律只在 organization scope 宣告。 */
const SCOPE: RbacScope = 'organization';

// ---------------------------------------------------------------------------
// simple permissions（細粒度，`PERMISSIONS.md §2.6` 表格右欄展開後的項目）
// ---------------------------------------------------------------------------

interface EnergySimpleDef {
  uniqueName: string;
  /** UI 顯示名（中文）。不可把 uniqueName 當顯示名用。 */
  name: string;
}

const ENERGY_SIMPLE_DEFS: EnergySimpleDef[] = [
  // ── 組織治理 ──
  { uniqueName: 'energy:member:manage', name: '能源營運：成員與角色指派' },
  { uniqueName: 'energy:department:manage', name: '能源營運：部門管理' },
  { uniqueName: 'energy:delegation:manage', name: '能源營運：代理規則管理' },
  // ── 稽核 ──
  { uniqueName: 'energy:audit:view', name: '能源營運：稽核紀錄檢視' },
  // ── 流程 ──
  { uniqueName: 'energy:flow:view', name: '能源營運：流程檢視（我參與的）' },
  { uniqueName: 'energy:flow:view-all', name: '能源營運：流程檢視（全組織）' },
  { uniqueName: 'energy:flow:act', name: '能源營運：流程操作（認領／核可）' },
  { uniqueName: 'energy:flow:admin', name: '能源營運：流程強制處置（解除副作用鎖／強制推進／終止）' },
  { uniqueName: 'energy:flow:abort-approve', name: '能源營運：流程中斷核可' },
  // ── 系統設定 ──
  { uniqueName: 'energy:workflow:manage', name: '能源營運：流程定義管理' },
  { uniqueName: 'energy:rating:manage', name: '能源營運：計算策略管理' },
  { uniqueName: 'energy:role:assign-privileged', name: '能源營運：指派管理者角色' },
  // ── 附件 ──
  { uniqueName: 'energy:attachment:upload', name: '能源營運：附件上傳' },
  { uniqueName: 'energy:attachment:download', name: '能源營運：附件下載' },
  { uniqueName: 'energy:attachment:unlink', name: '能源營運：附件解除連結' },
  // ── 應付 ──
  { uniqueName: 'energy:payout:view', name: '能源營運：應付紀錄檢視' },
  { uniqueName: 'energy:payout:recompute', name: '能源營運：應付紀錄重算' },
];

// ---------------------------------------------------------------------------
// complex permissions（角色只綁這一層，＝「改得動」的關鍵，§2.6）
// ---------------------------------------------------------------------------

interface EnergyComplexDef {
  uniqueName: string;
  name: string;
  /** 只能是 simple（不可包 complex）；不可為空。 */
  includes: string[];
}

const ENERGY_COMPLEX_DEFS: EnergyComplexDef[] = [
  {
    uniqueName: 'energy:govern:org',
    name: '能源營運：組織治理',
    includes: ['energy:member:manage', 'energy:department:manage', 'energy:delegation:manage'],
  },
  {
    uniqueName: 'energy:govern:system',
    name: '能源營運：系統設定',
    includes: [
      'energy:workflow:manage', 'energy:rating:manage',
      'energy:flow:admin', 'energy:role:assign-privileged',
    ],
  },
  {
    uniqueName: 'energy:oversight',
    name: '能源營運：全域檢視與稽核',
    includes: ['energy:flow:view-all', 'energy:audit:view', 'energy:attachment:download'],
  },
  {
    uniqueName: 'energy:flow:participate',
    name: '能源營運：流程參與',
    includes: [
      'energy:flow:view', 'energy:flow:act',
      'energy:attachment:upload', 'energy:attachment:download',
    ],
  },
  {
    uniqueName: 'energy:finance:act',
    name: '能源營運：財務作業',
    includes: ['energy:payout:view', 'energy:payout:recompute'],
  },
];

export const energyPermissions: BuiltInPermission[] = [
  ...ENERGY_SIMPLE_DEFS.map((d): BuiltInPermission => ({
    scope: SCOPE,
    uniqueName: d.uniqueName,
    name: d.name,
    complex: false,
  })),
  ...ENERGY_COMPLEX_DEFS.map((d): BuiltInPermission => ({
    scope: SCOPE,
    uniqueName: d.uniqueName,
    name: d.name,
    complex: true,
    includedPermissionUniqueNames: d.includes,
  })),
];

// ---------------------------------------------------------------------------
// roles（`PERMISSIONS.md §2.5.1` 五角色）
// ---------------------------------------------------------------------------

/** 前端下拉／邀請 preset 用的角色描述（uniqueName 不進 UI 文案）。 */
export interface EnergyRoleDef {
  uniqueName: string;
  /** UI 顯示名（＝ rbacRole.name，seed 進 DB 的也是這個字串）。 */
  name: string;
  /** 一句話說明，下拉選項的副標。 */
  description: string;
  permissionUniqueNames: string[];
}

export const ENERGY_ROLE_DEFS: EnergyRoleDef[] = [
  {
    uniqueName: 'energy:admin',
    name: '系統管理員',
    description: '全部權限，含系統設定與指派管理者角色',
    permissionUniqueNames: [
      'energy:govern:system', 'energy:govern:org', 'energy:oversight',
      'energy:flow:participate', 'energy:finance:act', 'energy:flow:abort-approve',
    ],
  },
  {
    uniqueName: 'energy:cxo',
    name: '高階主管',
    description: '可調整組織架構與他人權限，但不能指派管理者角色',
    permissionUniqueNames: [
      'energy:govern:org', 'energy:oversight',
      'energy:flow:participate', 'energy:finance:act', 'energy:flow:abort-approve',
    ],
  },
  {
    uniqueName: 'energy:operator',
    name: '業務承辦',
    description: '參與流程：建檔、送審、核可自己關卡的單',
    permissionUniqueNames: ['energy:flow:participate'],
  },
  {
    uniqueName: 'energy:finance',
    name: '財務',
    description: '參與流程，另可檢視與重算應付紀錄',
    permissionUniqueNames: ['energy:flow:participate', 'energy:finance:act'],
  },
  {
    uniqueName: 'energy:viewer',
    name: '唯讀',
    description: '看得到全組織的單與稽核紀錄，不能操作',
    permissionUniqueNames: ['energy:oversight'],
  },
];

export const energyRoles: BuiltInRole[] = ENERGY_ROLE_DEFS.map((r): BuiltInRole => ({
  scope: SCOPE,
  uniqueName: r.uniqueName,
  name: r.name,
  permissionUniqueNames: r.permissionUniqueNames,
}));

/**
 * `PERMISSIONS.md §2.5.3`：只有持 `energy:role:assign-privileged` 的人能指派／移除這些角色。
 * 「誰能指派管理者」要改，就改這個陣列（不做角色階層引擎）。
 */
export const PRIVILEGED_ROLES: string[] = ['energy:admin', 'energy:cxo'];

/**
 * `PERMISSIONS.md §2.5.4-I7`：不能移除最後一個持有組織治理權的人。
 * 判定用 complex permission 而不是硬編角色名 —— 日後加角色不必改防呆邏輯。
 */
export const ORG_GOVERNANCE_PERMISSION = 'energy:govern:org';

const ROLE_BY_UNIQUE_NAME: Record<string, EnergyRoleDef> = ENERGY_ROLE_DEFS.reduce(
  (acc, r) => { acc[r.uniqueName] = r; return acc; },
  {} as Record<string, EnergyRoleDef>,
);

/** 取單一角色定義；找不到回 undefined（呼叫端須擋）。 */
export const getEnergyRoleDef = (uniqueName: string): EnergyRoleDef | undefined => (
  ROLE_BY_UNIQUE_NAME[uniqueName]
);

/** 是否為本 feature 管的角色（BFF 的 role-assignments 端點只認這五個）。 */
export const isEnergyRole = (uniqueName: string): boolean => !!ROLE_BY_UNIQUE_NAME[uniqueName];

/** 是否為需要 `energy:role:assign-privileged` 才能指派的角色。 */
export const isPrivilegedEnergyRole = (uniqueName: string): boolean => (
  PRIVILEGED_ROLES.includes(uniqueName)
);

/** `EnergyOpsPlugin.getRbacSeeds()` 的回傳內容。 */
export function buildEnergyOpsRbacSeed(): RbacSeed {
  return {
    createdByPlugin: ENERGY_OPS_PLUGIN_NAME,
    permissions: energyPermissions,
    roles: energyRoles,
  };
}
