import * as React from 'react';
import { AppBar, TitlePortal, Logout, UserMenu, useUserMenu } from 'react-admin';
import { Box, ListItemIcon, ListItemText, MenuItem, useMediaQuery, Theme } from '@mui/material';
import PersonIcon from '@mui/icons-material/Person';
import { Link } from 'react-router-dom';
import { useTranslate } from 'react-admin';

import Logo from './Logo';
import { AppBarToolbar } from './AppBarToolbar';
import ProjectSelector from '@/platform/selection/ProjectSelector';

// 使用者選單裡的「個人資料」
const ProfileMenuItem = React.forwardRef<HTMLAnchorElement>((props, ref) => {
  const translate = useTranslate();
  const userMenu = useUserMenu();
  return (
    <MenuItem ref={ref} component={Link} to="/profile" onClick={() => userMenu?.onClose()} {...props}>
      <ListItemIcon><PersonIcon fontSize="small" /></ListItemIcon>
      <ListItemText>{translate('eo.profile.title')}</ListItemText>
    </MenuItem>
  );
});

const CustomAppBar = () => {
  const isLargeEnough = useMediaQuery<Theme>((theme) => theme.breakpoints.up('sm'));
  return (
    <AppBar
      color="secondary"
      toolbar={<AppBarToolbar />}
      userMenu={(
        // 外包一層只為了掛 data-testid（UserMenu 不轉傳多餘 props）
        <Box component="span" data-testid="user-menu" sx={{ display: 'inline-flex' }}>
          <UserMenu>
            <ProfileMenuItem />
            <Logout data-testid="logout" />
          </UserMenu>
        </Box>
      )}
    >
      {isLargeEnough && <Logo sx={{ mr: 1 }} />}
      <ProjectSelector />
      <TitlePortal sx={{ ml: 1 }} />
    </AppBar>
  );
};

export default CustomAppBar;
