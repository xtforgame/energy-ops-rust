// 後台入口：react-admin＋hash 路由；登入走新後端的 cookie session，組織／專案由頂欄切換器決定。
import * as React from 'react';
import { QueryClient } from '@tanstack/react-query';
import polyglotI18nProvider from 'ra-i18n-polyglot';
import { Admin, CustomRoutes, localStorageStore, useStore } from 'react-admin';
import { Route } from 'react-router-dom';
import authProvider from './authProvider';
import dataProvider from './dataProvider';
import zhTwMessages from './i18n/zh-tw';
import { Layout, Login } from './layout';
import TaiwanMapPage from './taiwan-map/TaiwanMapPage';
import { themes, ThemeName } from './themes/themes';
import getResources from './domain-logic/components/getResources';
import LandownerDetailPage from './domain-logic/components/energyOps/LandownerDetailPage';
import { selectionStore } from '../platform/selection/store';
import HomePage from '../platform/pages/HomePage';
import ProfilePage from '../platform/pages/ProfilePage';
import MyInvitationsPage from '../platform/pages/MyInvitationsPage';
import AcceptInvitationPage from '../platform/pages/AcceptInvitationPage';

const i18nProvider = polyglotI18nProvider(
  (locale) => {
    if (locale === 'en') return import('./i18n/en').then((m) => m.default);
    return zhTwMessages as any;
  },
  'zh-TW',
  [
    { locale: 'zh-TW', name: '繁體中文' },
    { locale: 'en', name: 'English' },
  ],
);

const store = localStorageStore(undefined, 'EnergyOps');

const App = () => {
  const [themeName] = useStore<ThemeName>('themeName', 'jv');
  const lightTheme = themes.find((t) => t.name === themeName)?.light;
  const darkTheme = themes.find((t) => t.name === themeName)?.dark;
  const queryClient = React.useMemo(() => new QueryClient({
    defaultOptions: { queries: { staleTime: 60 * 1000, refetchOnWindowFocus: false } },
  }), []);

  // 換組織／專案 → 重抓有效權限（選單與路由跟著變）
  React.useEffect(() => {
    let last = selectionStore.getSelection();
    return selectionStore.subscribe(() => {
      const next = selectionStore.getSelection();
      if (next?.organizationId === last?.organizationId && next?.projectId === last?.projectId) return;
      last = next;
      queryClient.invalidateQueries({ queryKey: ['auth', 'getPermissions'] });
    });
  }, [queryClient]);

  return (
    <Admin
      title=""
      dataProvider={dataProvider}
      queryClient={queryClient}
      store={store}
      authProvider={authProvider}
      dashboard={HomePage}
      loginPage={Login}
      layout={Layout}
      i18nProvider={i18nProvider}
      disableTelemetry
      lightTheme={lightTheme}
      darkTheme={darkTheme}
      defaultTheme="light"
    >
      {/* 公開頁：未登入（甚至未註冊）的人也要能打開邀請連結 */}
      <CustomRoutes noLayout>
        <Route path="/accept-invitation" element={<AcceptInvitationPage />} />
      </CustomRoutes>
      {(permissions: string[]) => (
        <>
          <CustomRoutes>
            <Route path="/my-invitations" element={<MyInvitationsPage />} />
            <Route path="/profile" element={<ProfilePage />} />
            {/* 地主整合頁：獨立路由＋可分享的網址 */}
            <Route path="/energyLandowners/:id" element={<LandownerDetailPage />} />
            <Route path="/taiwan-map" element={<TaiwanMapPage />} />
          </CustomRoutes>
          {getResources(permissions)}
        </>
      )}
    </Admin>
  );
};

export default App;
