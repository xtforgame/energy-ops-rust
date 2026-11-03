import * as React from 'react';
import { useState } from 'react';
import { Box } from '@mui/material';

import {
    useTranslate,
    usePermissions,
    DashboardMenuItem,
    MenuItemLink,
    MenuProps,
    useSidebarState,
} from 'react-admin';
import MailIcon from '@mui/icons-material/Mail';
import PublicIcon from '@mui/icons-material/Public';
import PersonIcon from '@mui/icons-material/Person';
import { useSelection } from '@/platform/selection/store';

import { getMenuGroups } from '../domain-logic/components/getMenuItems';
import SubMenu from './SubMenu';

// 不屬於任何群組的項目：首頁、我的邀請、個人資料一律顯示；台灣圖層沿用 App 5 隱藏（路由仍在）。
const SHOW_STANDALONE_ITEMS = true;
const SHOW_TAIWAN_MAP = false;

const Menu = ({ dense = false }: MenuProps) => {
    const translate = useTranslate();
    const { permissions } = usePermissions();
    const [open] = useSidebarState();
    // 各 group 預設展開；key = group.key（global / organization / project）。
    const [openGroups, setOpenGroups] = useState<Record<string, boolean>>({});
    const isGroupOpen = (k: string) => openGroups[k] ?? true;
    const toggleGroup = (k: string) => setOpenGroups(s => ({ ...s, [k]: !(s[k] ?? true) }));

    // 切換組織時重算選單
    useSelection();

    const groups = getMenuGroups({ dense }, translate, permissions as string[] | undefined);

    return (
        <Box
            sx={{
                // ⚠️ 寬度必須跟著主題的 sidebar 寬（jv=238、其他=280），寫死會爆版：
                //    選取態的整塊底色比側欄寬，就會凸出去（實測 250 vs 238 凸 12px）
                width: (theme: any) => (open ? (theme.sidebar?.width ?? 250) : 50),
                marginTop: 1,
                marginBottom: 1,
                transition: theme =>
                    theme.transitions.create('width', {
                        easing: theme.transitions.easing.sharp,
                        duration: theme.transitions.duration.leavingScreen,
                    }),
            }}
        >
            {/* 見檔頭 `SHOW_STANDALONE_ITEMS`：這三項不在任何 group 裡，要單獨收。 */}
            {SHOW_STANDALONE_ITEMS && <DashboardMenuItem />}
            {/* 個人收件匣（不分 org/project；任何登入者都看得到自己的邀請）。 */}
            {SHOW_STANDALONE_ITEMS && (
                <MenuItemLink
                    to="/my-invitations"
                    data-testid="menu-my-invitations"
                    primaryText={translate('tc.inbox.title')}
                    leftIcon={<MailIcon />}
                    dense={dense}
                />
            )}
            {/* 自訂頁：台灣行政區 3D 圖層（不屬於任何 org/project 資源群） */}
            {SHOW_STANDALONE_ITEMS && (
                <MenuItemLink
                    to="/profile"
                    primaryText={translate('eo.profile.title')}
                    leftIcon={<PersonIcon />}
                    dense={dense}
                    data-testid="menu-profile"
                />
            )}
            {SHOW_TAIWAN_MAP && (
                <MenuItemLink
                    to="/taiwan-map"
                    primaryText={translate('tc.taiwanMap.title')}
                    leftIcon={<PublicIcon />}
                    dense={dense}
                />
            )}
            {groups.map(g => (
                <SubMenu
                    key={g.key}
                    handleToggle={() => toggleGroup(g.key)}
                    isOpen={isGroupOpen(g.key)}
                    name={g.labelKey}
                    icon={g.icon}
                    dense={dense}
                >
                    {g.items}
                </SubMenu>
            ))}
        </Box>
    );
};

export default Menu;
