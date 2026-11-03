import {
    RaThemeOptions,
    defaultLightTheme,
    defaultDarkTheme,
    nanoDarkTheme,
    nanoLightTheme,
    radiantDarkTheme,
    radiantLightTheme,
    houseDarkTheme,
    houseLightTheme,
} from 'react-admin';

import { softDarkTheme, softLightTheme } from './softTheme';
import { chiptuneTheme } from './chiptuneTheme';
import { jvLightTheme } from './jvTheme';

export type ThemeName =
    | 'jv'
    | 'soft'
    | 'default'
    | 'nano'
    | 'radiant'
    | 'house'
    | 'chiptune';

export interface Theme {
    name: ThemeName;
    light: RaThemeOptions;
    dark?: RaThemeOptions;
}

const sidebarWidth = 280;
softLightTheme.sidebar = { ...softLightTheme.sidebar, width: sidebarWidth };
softDarkTheme.sidebar = { ...softDarkTheme.sidebar, width: sidebarWidth };
defaultLightTheme.sidebar = { ...defaultLightTheme.sidebar, width: sidebarWidth };
defaultDarkTheme.sidebar = { ...defaultDarkTheme.sidebar, width: sidebarWidth };
nanoLightTheme.sidebar = { ...nanoLightTheme.sidebar, width: sidebarWidth };
nanoDarkTheme.sidebar = { ...nanoDarkTheme.sidebar, width: sidebarWidth };
radiantLightTheme.sidebar = { ...radiantLightTheme.sidebar, width: sidebarWidth };
radiantDarkTheme.sidebar = { ...radiantDarkTheme.sidebar, width: sidebarWidth };
houseLightTheme.sidebar = { ...houseLightTheme.sidebar, width: sidebarWidth };
houseDarkTheme.sidebar = { ...houseDarkTheme.sidebar, width: sidebarWidth };
chiptuneTheme.sidebar = { ...chiptuneTheme.sidebar, width: sidebarWidth };
// jv（1C）刻意用設計稿的窄側欄 238px，不吃上面的 280

export const themes: Theme[] = [
    // JV 1C（設計稿 temp/JV控股管理平台設計；只有亮色）
    { name: 'jv', light: jvLightTheme },
    { name: 'soft', light: softLightTheme, dark: softDarkTheme },
    { name: 'default', light: defaultLightTheme, dark: defaultDarkTheme },
    { name: 'nano', light: nanoLightTheme, dark: nanoDarkTheme },
    { name: 'radiant', light: radiantLightTheme, dark: radiantDarkTheme },
    { name: 'house', light: houseLightTheme, dark: houseDarkTheme },
    { name: 'chiptune', light: chiptuneTheme },
];
