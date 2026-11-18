import * as React from 'react';
import {
  TranslateFunction,
  MenuItemLink,
  MenuProps,
} from 'react-admin';
import BusinessIcon from '@mui/icons-material/Business';
import PeopleIcon from '@mui/icons-material/People';
import AccountTreeIcon from '@mui/icons-material/AccountTree';
// 流程單（執行）＝「在跑、等著人動」—— 不與治理層的 AccountTree 撞圖示。
import PendingActionsIcon from '@mui/icons-material/PendingActions';
// energy-ops-console。icon 一律用元件，禁用 emoji（專案鐵律）。
import BoltIcon from '@mui/icons-material/Bolt';
import AssignmentIcon from '@mui/icons-material/Assignment';
import SolarPowerIcon from '@mui/icons-material/SolarPower';
import SpaceDashboardIcon from '@mui/icons-material/SpaceDashboard';
import MoveToInboxIcon from '@mui/icons-material/MoveToInbox';
import EventIcon from '@mui/icons-material/Event';
import NotificationsIcon from '@mui/icons-material/Notifications';
import AlarmIcon from '@mui/icons-material/Alarm';
import HistoryIcon from '@mui/icons-material/History';
import MailOutlineIcon from '@mui/icons-material/MailOutline';
import FolderIcon from '@mui/icons-material/Folder';
import GroupsIcon from '@mui/icons-material/Groups';
import SettingsIcon from '@mui/icons-material/Settings';
import SecurityIcon from '@mui/icons-material/Security';
import BadgeIcon from '@mui/icons-material/Badge';
import KeyIcon from '@mui/icons-material/Key';
import VerifiedUserIcon from '@mui/icons-material/VerifiedUser';
import AdminPanelSettingsIcon from '@mui/icons-material/AdminPanelSettings';
import ManageAccountsIcon from '@mui/icons-material/ManageAccounts';
import { canSeeInMenu } from '@/platform/permissions';
import BillsMenuItem from './energyOps/BillsMenuItem';

export interface MenuGroup {
  key: string;
  labelKey: string;       // i18n key（SubMenu name）
  icon: React.ReactElement;
  items: React.ReactElement[];
}

/** group → entries（[resourceKey, icon]）。org 切換由頂部 selector 處理。 */
// 2026-06-04 編排定案：org 群只放真 org-scoped tenant 資源；rbacRoles/rbacPermissions/
// userRbacRoles（授權）已移除——授權/撤銷併進「成員」頁（單一權限中心）。
const GROUPS: Array<{ key: string; labelKey: string; icon: React.ReactElement; entries: Array<[string, React.ReactNode]> }> = [
  {
    key: 'organization', labelKey: 'pos.menu.organization', icon: <BusinessIcon />,
    entries: [
      ['organizationMembers', <PeopleIcon />],
      ['organizationInvitations', <MailOutlineIcon />],
      ['projects', <FolderIcon />],
      ['projectMembers', <GroupsIcon />],
      ['departments', <AccountTreeIcon />],
      ['organizationSettings', <SettingsIcon />],
    ],
  },
  {
    key: 'rbac', labelKey: 'pos.menu.rbac', icon: <SecurityIcon />,
    entries: [
      ['rbacRoles', <BadgeIcon />],
      ['rbacPermissions', <KeyIcon />],
      ['rbacGrants', <VerifiedUserIcon />],
    ],
  },
  // ─────────────────────────────────────────────────────────────
  // energy-ops-console 分**三層**（`FRONTEND.md §1.2`）——
  // 動線層（角色的家）／實體層（查東西）／治理層（設定與稽核）。
  //
  // **現況曾經是照資料表列的**（一長串平的清單）：那種排法讓
  // 「我每天要做的事」跟「偶爾查一次的設定」混在一起，
  // 使用者每天都要在同一串裡重新找路。
  // 不塞進「組織」群 —— 那群是設定類的，點進去的頻率完全不同。
  // ─────────────────────────────────────────────────────────────
  {
    key: 'energyMyWork', labelKey: 'pos.menu.energyMyWork', icon: <BoltIcon />,
    entries: [
      // 我的工作（`LIFECYCLE.md §7.3`）—— 工作台與「我參與過的」是**兩個頁籤**，
      // 不是兩頁：它們都是「我的單」，只是時間切面不同。
      ['energyMyWorkConsole', <AssignmentIcon />],
      // 送件（§7.2）—— 收件台與送單紀錄合併成的**一個**項目。
      // 那兩項不是被砍掉，是併回它們本來所屬的資源（`energyIngestBatch`）：
      // 「我在收件」與「我在查紀錄」動作感覺很不一樣，但**動作不同不代表資源不同**。
      ['energyIngestConsole', <MoveToInboxIcon />],
    ],
  },
  {
    key: 'energyOperations', labelKey: 'pos.menu.energyOperations', icon: <SolarPowerIcon />,
    entries: [
      // 電費單帶**待指認角標**（§7.4）—— 例外件併進來之後，
      // 沒有這個數字它就變得比獨立一頁**更難被看見**。
      ['energyBillConsole', null],
      // 流程單（執行）與電費單**並列在營運層** ——
      // 兩者都不放治理層：治理層是「設定流程長什麼樣」，
      // 而「哪些單在跑、卡在哪」是每天要看的營運資訊。
      // 這一項的選單閘是 `view-all`（`MENU_PERMISSION_MAP`）：
      // 它問的是「**全組織**現在怎麼了」，沒權限整條隱藏不是空清單。
      ['energyExecutionConsole', <PendingActionsIcon />],
      // 案場詳情（P11）本身不進選單；卡 20261118-0427-a 起案場**清單**進選單
      // （主檔維護：新增案場、算專案編號）。地主詳情同理，不進選單。
      ['energyLandownerConsole', <SolarPowerIcon />],
      ['energyStationConsole', <SolarPowerIcon />],
      // 卡 20261118-0810-a2：合約維護（清單、新增編輯、合約檔）進選單。
      ['energyContractConsole', <AssignmentIcon />],
      ['energyCalendarConsole', <EventIcon />],
      ['energyReminderConsole', <AlarmIcon />],
      ['energyNotificationConsole', <NotificationsIcon />],
      ['energyDashboardConsole', <SpaceDashboardIcon />],
    ],
  },
  {
    key: 'energyGovernance', labelKey: 'pos.menu.energyGovernance', icon: <AccountTreeIcon />,
    entries: [
      // P9 流程總覽 —— 管理員的家。
      ['energyWorkflowConsole', <AccountTreeIcon />],
      ['energyAuditConsole', <HistoryIcon />],
    ],
  },
  {
    key: 'superAdmin', labelKey: 'pos.menu.superAdmin', icon: <AdminPanelSettingsIcon />,
    entries: [
      ['adminUsers', <ManageAccountsIcon />],
      ['adminRbac', <SecurityIcon />],
    ],
  },
];

/**
 * **暫時隱藏**的選單分組（2026-08-26 使用者指示：先只留「我的工作」與「營運」）。
 *
 * ⚠️ 這裡隱藏的只是**選單入口**——`getResources` 的 Resource 與路由都還在：
 * 直接打 URL 仍然進得去，crawler 走 hash 導覽也完全不受影響。
 * 所以這不是「移除功能」，是**收窄每天要看的東西**。
 *
 * 要恢復：把 key 從這個 Set 拿掉即可，`GROUPS` 的定義一行都沒動。
 */
const HIDDEN_GROUP_KEYS = new Set<string>([
  'energyGovernance',   // 設定與稽核（沿用 App 5 的隱藏；路由仍在）
]);

const itemLabel = (translate: TranslateFunction, key: string) =>
  translate(`resources.${key}.name`, { smart_count: 2 });

/** 分組後的選單（gated）。Menu.tsx 以 SubMenu render 每組。 */
export const getMenuGroups = (
  props: MenuProps,
  translate: TranslateFunction,
  permissions?: string[] | null,
): MenuGroup[] => GROUPS.filter((g) => !HIDDEN_GROUP_KEYS.has(g.key)).map((g) => ({
  key: g.key,
  labelKey: g.labelKey,
  icon: g.icon,
  items: g.entries
    .filter(([key]) => canSeeInMenu(String(key), permissions))
    .map(([key, icon]) => (
      // `icon === null` ＝ 這一項自己畫（目前只有電費單，它要帶角標）。
      icon === null ? <BillsMenuItem key={key} dense={props.dense} /> : (
      <MenuItemLink
        key={key}
        to={`/${key}`}
        state={{ _scrollToTop: true }}
        primaryText={itemLabel(translate, key)}
        leftIcon={icon as any}
        dense={props.dense}
        // 解說影片要「指著選單項目再點」（`DEMO_STORYBOARD.md` §1.4）。
        // 靠 `a[href="#/x"]` 也找得到，但頁面上常常有第二個連到同一處的連結
        // （KPI 卡、麵包屑），spotlight 會標到清單裡那個而不是選單上那個。
        data-testid={`menu-${key}`}
      />
      )
    )),
})).filter((g) => g.items.length > 0);

export default getMenuGroups;
