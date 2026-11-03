/**
 * 角色的**顯示名**（畫面上不得出現 `energy:` / `ryxis:` 這類 uniqueName）。
 *
 * energy 那半邊直接吃 `EnergyOpsCommonPlugin/rbacSeeds` 的常數 —— 與後端 seed 同一份真值，
 * 改角色名只改一個檔案（`PERMISSIONS.md §2.7`）。
 * 既有 ryxis 角色沒有這份常數（它們的 seed 在 server 端），這裡補一張小對照表；
 * 查不到就退回「把 uniqueName 的最後一段拿來顯示」，永遠不整串印出來。
 */

import { ENERGY_ROLE_DEFS } from 'common/plugins/EnergyOpsCommonPlugin/rbacSeeds';

const ENERGY_LABELS: Record<string, string> = ENERGY_ROLE_DEFS.reduce(
  (acc, r) => { acc[r.uniqueName] = r.name; return acc; },
  {} as Record<string, string>,
);

/** 既有 ryxis 角色（DB 的 name 帶 "Ryxis: " 前綴，這裡給乾淨的顯示名）。 */
const LEGACY_LABELS: Record<string, string> = {
  'platform:org-admin': '組織管理員',
  'platform:project-admin': '專案管理員',
  'ryxis:org-admin': '組織管理員',
  'ryxis:project-admin': '專案管理員',
  'ryxis:agent-editor': '代理編輯者',
  'ryxis:viewer': '唯讀檢視者',
  'ryxis:dynamic-mcp-user': '工具使用者',
};

/** uniqueName 查不到對照時的最後手段：取最後一段、把連字號換成空白。 */
const humanize = (uniqueName: string): string => {
  const tail = uniqueName.split(':').pop() || uniqueName;
  return tail.replace(/-/g, ' ');
};

/**
 * @param uniqueName 角色的 uniqueName（可為空）
 * @param fallbackName 後端帶回的 `role.name`（只有在它本身不含 `:` 時才採用）
 */
export const roleDisplayName = (
  uniqueName?: string | null,
  fallbackName?: string | null,
): string => {
  if (uniqueName) {
    const known = ENERGY_LABELS[uniqueName] || LEGACY_LABELS[uniqueName];
    if (known) return known;
  }
  if (fallbackName && !fallbackName.includes(':')) return fallbackName;
  if (uniqueName) return humanize(uniqueName);
  return fallbackName || '—';
};

export default roleDisplayName;
