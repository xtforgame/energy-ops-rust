import * as React from 'react';
import { Layout } from 'react-admin';
import AppBar from './AppBar';
import Menu from './Menu';
import RouteTransition from './RouteTransition';
// 登入後依角色導向（只導第一次）
import LandingRedirect from '../domain-logic/components/energyOps/LandingRedirect';

// 切頁效果的開關：'none' | 'enter' | 'crossfade' | 'cloud'（見 ./RouteTransition/README.md）。
const ROUTE_TRANSITION_MODE = 'cloud' as const;

export default ({ children }: { children: React.ReactNode }) => (
  <Layout appBar={AppBar} menu={Menu}>
    {/* 副作用元件掛在 Layout 裡，不參與路由（catch-all 路由會吃掉所有頁面）。 */}
    <LandingRedirect />
    <RouteTransition mode={ROUTE_TRANSITION_MODE}>{children}</RouteTransition>
  </Layout>
);
