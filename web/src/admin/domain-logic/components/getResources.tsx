import { Resource } from 'react-admin';
// 平台頁（新後端 /api/v1）
import MembersPage from '@/platform/pages/MembersPage';
import InvitationsPage from '@/platform/pages/InvitationsPage';
import ProjectsPage, { ProjectMembersPage } from '@/platform/pages/ProjectsPage';
import { AdminRbacPage, RbacGrantsPage, RbacPermissionsPage, RbacRolesPage } from '@/platform/pages/RbacPages';
import AdminUsersPage from '@/platform/pages/AdminUsersPage';
import OrganizationSettingsPage from '@/platform/pages/OrganizationSettingsPage';
import { canAccessResource } from '@/platform/permissions';
// 部門（energy-ops-console 的組織模型；custom page，走 /api/energy-ops/*）
import DepartmentsPage from './energyOps/DepartmentsPage';
// energy-ops-console 營運頁（C1 骨架：P1 待辦、P2 單據、P4 例外件、P5 地主/發電站）
// P1 角色工作台（C7 步驟 3）—— 舊的 MyTasksPage 已刪除（沒有分區的通用清單）
import MyWorkConsolePage from './energyOps/MyWorkConsolePage';
import BillsPage from './energyOps/BillsPage';
// P12 流程單（執行）—— **工作流引擎層**，與 P2 電費單是兩個資源（`FRONTEND.md §1.4`）。
import ExecutionsPage from './energyOps/ExecutionsPage';
// P12 詳情 —— **execution 自己的家**。
// 少了它，「點一列去哪」就沒有答案，而上一版是拿 subject 的家湊
// （點 execution 卻跳到電費單校對台）—— 那是合併，不是連結。
import ExecutionDetailPage from './energyOps/ExecutionDetailPage';
import LandownersPage from './energyOps/LandownersPage';
// C7b：實體頁。地主整合頁走 **App.tsx 的 CustomRoute**
// （`#/energyLandowners/:id`），不是 RA resource —— 所以不在這裡 import。
import StationDetailPage from './energyOps/StationDetailPage';
// 卡 20261118-0427-a S1：案場清單頁（新增案場、算專案編號）。
import StationsPage from './energyOps/StationsPage';
// 卡 20261118-0810-a2 S1：合約清單頁（新增編輯、合約檔上傳）。
import ContractsPage from './energyOps/ContractsPage';
import IngestConsolePage from './energyOps/IngestConsolePage';
import IngestBatchDetailPage from './energyOps/IngestBatchDetailPage';
// P9 流程總覽（管理員的家）—— 治理層。
import WorkflowOverviewPage from './energyOps/WorkflowOverviewPage';
// P3 校對台（全案最重的一頁）——掛成 energyBillConsole 的 show
import ReviewConsolePage from './energyOps/ReviewConsolePage';
// C4：稽核頁 P6 / Dashboard P7 / 行事曆 P8
// P0 收件台 —— 單子進系統的唯一入口（C6）
import AuditPage from './energyOps/AuditPage';
import DashboardPage from './energyOps/DashboardPage';
import CalendarPage from './energyOps/CalendarPage';
// 卡 E：提醒設定（規則參數＋假日表）、通知（組織內共用，不寄信）。
import ReminderSettingsPage from './energyOps/ReminderSettingsPage';
import NotificationsPage from './energyOps/NotificationsPage';
// 權限閘（RESOURCE_PERMISSION_MAP／MENU_PERMISSION_MAP）集中在 @/platform/permissions。
export { canAccessResource, canSeeInMenu, RESOURCE_PERMISSION_MAP, MENU_PERMISSION_MAP } from '@/platform/permissions';

const getResources = (permissions?: string[] | null) => {
  const all = [
    <Resource key="organizationMembers" name="organizationMembers" list={MembersPage} />,
    <Resource key="organizationInvitations" name="organizationInvitations" list={InvitationsPage} />,
    <Resource key="organizationSettings" name="organizationSettings" list={OrganizationSettingsPage} />,
    <Resource key="projects" name="projects" list={ProjectsPage} />,
    <Resource key="projectMembers" name="projectMembers" list={ProjectMembersPage} />,
    <Resource key="rbacRoles" name="rbacRoles" list={RbacRolesPage} />,
    <Resource key="rbacPermissions" name="rbacPermissions" list={RbacPermissionsPage} />,
    <Resource key="rbacGrants" name="rbacGrants" list={RbacGrantsPage} />,
    <Resource key="adminUsers" name="adminUsers" list={AdminUsersPage} />,
    <Resource key="adminRbac" name="adminRbac" list={AdminRbacPage} />,
    <Resource key="departments" name="departments" list={DepartmentsPage} />,
    // 送件：清單 ＋ 詳情（校對工作區）。
    // 詳情掛在 `edit` 而不是 `show` —— RA 的 `show` 路由是 `/:id/show`，
    // 而規格要的路由是 `#/energyIngestConsole/:batchId`；語意上這一頁也確實是
    // 「編輯這一批」而不是「檢視這一批」。
    <Resource
      key="energyIngestConsole"
      name="energyIngestConsole"
      list={IngestConsolePage}
      edit={IngestBatchDetailPage}
    />,
    <Resource key="energyMyWorkConsole" name="energyMyWorkConsole" list={MyWorkConsolePage} />,
    <Resource key="energyBillConsole" name="energyBillConsole" list={BillsPage} show={ReviewConsolePage} />,
    <Resource
      key="energyExecutionConsole"
      name="energyExecutionConsole"
      list={ExecutionsPage}
      show={ExecutionDetailPage}
    />,
    <Resource key="energyLandownerConsole" name="energyLandownerConsole" list={LandownersPage} />,
    // P11 案場詳情；卡 20261118-0427-a 起加 `list`（案場清單頁，進選單）。
    // 這裡是**陣列字面值** —— 不可以用 `{/* */}` 註解，
    // 那會變成一個空物件被塞進 resources（React render 時才炸）。
    <Resource key="energyStationConsole" name="energyStationConsole" list={StationsPage} show={StationDetailPage} />,
    <Resource key="energyContractConsole" name="energyContractConsole" list={ContractsPage} />,
    <Resource key="energyWorkflowConsole" name="energyWorkflowConsole" list={WorkflowOverviewPage} />,
    <Resource key="energyDashboardConsole" name="energyDashboardConsole" list={DashboardPage} />,
    <Resource key="energyCalendarConsole" name="energyCalendarConsole" list={CalendarPage} />,
    <Resource key="energyReminderConsole" name="energyReminderConsole" list={ReminderSettingsPage} />,
    <Resource key="energyNotificationConsole" name="energyNotificationConsole" list={NotificationsPage} />,
    <Resource key="energyAuditConsole" name="energyAuditConsole" list={AuditPage} />,
  ];
  return all.filter((r) => canAccessResource((r as any).props.name, permissions));
};

export default getResources;
