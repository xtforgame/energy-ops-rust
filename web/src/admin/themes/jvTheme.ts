import { defaultTheme } from 'react-admin';

/**
 * JV 主題 —— 依 `temp/JV控股管理平台設計` 的 **1C「紫底大區塊 · 主管視角」** 落地。
 *
 * 設計語言（來自設計稿的自述）：全直角、深紫 #5B4A96 / 淺紫 #7E6DB6、白底留白、
 * 1px 細線、寬字距英文小標（Archivo）、錯位色塊；警示用燒橘 #B4522C。
 *
 * 這裡只放 MUI/RA 主題能表達的部分（顏色、直角、細線、字體、選單/表格/按鈕的形）。
 * 1C 的「紫底大區塊反白數字」是頁面內容層的版型，之後做 Dashboard 時再用
 * `jvTokens` 取色，不要在頁面裡重新 hardcode 色碼。
 */

/** 設計稿色票 —— 頁面層要用 1C 的顏色一律從這裡拿 */
export const jvTokens = {
  /** 深紫（主色、選取態、實色按鈕、紫底大區塊） */
  primary: '#5B4A96',
  /** 淺紫（次要色塊、hero 的분隔線） */
  primaryLight: '#7E6DB6',
  /** 更深的紫（logo、AppBar 文字、強調） */
  primaryDark: '#3E3470',
  /** hero 紫底裡的細線 */
  heroHairline: '#7565AC',
  /** hero 紫底裡的次要文字 */
  heroMuted: '#CFC8E6',
  /** 淡紫底（hover、選取列、底部 token 區塊） */
  hoverBg: '#F4F3F8',
  /** 主要文字 */
  text: '#2B2A33',
  /** 次要文字 */
  textSecondary: '#43414F',
  /** 弱化文字 */
  muted: '#8B8797',
  /** 更弱（英文小標） */
  faint: '#9D99AA',
  /** 說明文字 */
  caption: '#6A6578',
  /** 主要框線 */
  border: '#DDDDE3',
  /** 面板細線 */
  hairline: '#E4E4EA',
  /** 表格列細線 */
  rowHairline: '#F0EFF4',
  /** 警示（高風險、待辦角標）—— 燒橘，不是紅 */
  warn: '#B4522C',
  /** 中風險金 */
  gold: '#C8A24A',
  /** 成功綠（狀態 pill） */
  ok: '#2F6B41',
  okBorder: '#C6DCCD',
  /** 進度條軌道 */
  track: '#E2DFF0',
} as const;

/** 英文小標 / 數字用字體（設計稿的 Archivo；中文回退 Noto Sans TC） */
export const jvMonoLabel = {
  fontFamily: "'Archivo','Noto Sans TC',sans-serif",
  letterSpacing: '.14em',
} as const;

const fontFamily = "'Noto Sans TC','Archivo',system-ui,-apple-system,sans-serif";

export const jvLightTheme = {
  palette: {
    mode: 'light' as const,
    primary: { main: jvTokens.primary, dark: jvTokens.primaryDark, light: jvTokens.primaryLight, contrastText: '#fff' },
    secondary: { main: jvTokens.primaryDark, contrastText: '#fff' },
    error: { main: jvTokens.warn },
    warning: { main: jvTokens.gold },
    success: { main: jvTokens.ok },
    // 設計是「白底留白」：頁面與面板同為純白，層次交給 1px 細線，不靠底色差
    background: { default: '#ffffff', paper: '#ffffff' },
    text: { primary: jvTokens.text, secondary: jvTokens.caption },
    divider: jvTokens.hairline,
  },
  // 全直角 —— 這一條是整個設計語言的骨架
  shape: { borderRadius: 0 },
  typography: {
    fontFamily,
    fontSize: 13,
    h6: { fontWeight: 700, letterSpacing: '.06em', color: jvTokens.text },
    button: { textTransform: 'none' as const, letterSpacing: '.04em', fontWeight: 500 },
    caption: { color: jvTokens.muted },
  },
  sidebar: { width: 238 },
  components: {
    ...defaultTheme.components,

    /* ---------- AppBar：白底、下緣 1px 細線、深紫文字、零陰影 ---------- */
    MuiAppBar: {
      defaultProps: { elevation: 0 },
      styleOverrides: {
        colorSecondary: {
          color: jvTokens.primaryDark,
          backgroundColor: '#ffffff',
          borderBottom: `1px solid ${jvTokens.hairline}`,
        },
      },
    },

    /* ---------- 側欄：右緣細線；⛔ 不給背景色 ----------
       側欄必須維持透明：切頁的雲霧轉場（CloudTransition）把雲畫在側欄後面、
       選單浮在雲上 —— 側欄一有底色，雲就被擋住，那個「雲從選單底下飄過」的
       效果就沒了。頁面底色本來就是白，平時視覺無差。 */
    RaSidebar: {
      styleOverrides: {
        root: {
          borderRight: `1px solid ${jvTokens.hairline}`,
        },
      },
    },

    /* ---------- 選單：選取＝整塊紫底反白（1C 側欄的核心視覺） ---------- */
    RaMenuItemLink: {
      styleOverrides: {
        root: {
          color: jvTokens.textSecondary,
          fontSize: '13.5px',
          paddingTop: 10,
          paddingBottom: 10,
          borderLeft: 'none',
          '& .RaMenuItemLink-icon': { color: jvTokens.textSecondary, minWidth: 38 },
          '&:hover': { backgroundColor: jvTokens.hoverBg },
          '&.RaMenuItemLink-active': {
            backgroundColor: jvTokens.primary,
            color: '#ffffff',
            fontWeight: 500,
            '& .RaMenuItemLink-icon': { color: '#ffffff' },
            '&:hover': { backgroundColor: jvTokens.primary },
          },
        },
      },
    },

    /* ---------- 面板：1px 細線取代陰影 ---------- */
    MuiPaper: {
      styleOverrides: {
        root: { backgroundClip: 'padding-box' },
        outlined: { borderColor: jvTokens.hairline },
        elevation1: { boxShadow: 'none', border: `1px solid ${jvTokens.hairline}` },
        elevation2: { boxShadow: 'none', border: `1px solid ${jvTokens.hairline}` },
        // 下拉選單／彈窗維持一點浮起感，但直角＋細線
        elevation8: { boxShadow: '0 10px 30px rgba(43,42,51,.14)', border: `1px solid ${jvTokens.hairline}` },
      },
    },
    MuiCard: {
      styleOverrides: {
        root: { boxShadow: 'none', border: `1px solid ${jvTokens.hairline}` },
      },
    },

    /* ---------- 按鈕：直角、實紫零陰影、外框用 #DDDDE3 ---------- */
    MuiButton: {
      defaultProps: { disableElevation: true },
      styleOverrides: {
        root: { borderRadius: 0 },
        containedPrimary: {
          '&:hover': { backgroundColor: jvTokens.primaryDark },
        },
        outlined: {
          borderColor: jvTokens.border,
          color: jvTokens.textSecondary,
          '&:hover': { borderColor: jvTokens.primary, backgroundColor: jvTokens.hoverBg },
        },
        outlinedPrimary: { borderColor: jvTokens.primary, color: jvTokens.primary },
      },
    },
    MuiIconButton: { styleOverrides: { root: { borderRadius: 0 } } },

    /* ---------- Datagrid：表頭＝弱化寬字距小標，列＝更細的 hairline ---------- */
    RaDatagrid: {
      styleOverrides: {
        root: {
          '& .RaDatagrid-headerCell': {
            backgroundColor: '#ffffff',
            color: jvTokens.muted,
            fontSize: '11px',
            fontWeight: 500,
            letterSpacing: '.12em',
            borderBottom: `1px solid ${jvTokens.hairline}`,
          },
          '& .RaDatagrid-rowCell': {
            fontSize: '12.5px',
            color: jvTokens.textSecondary,
            borderBottom: `1px solid ${jvTokens.rowHairline}`,
          },
          '& .RaDatagrid-row:hover': { backgroundColor: jvTokens.hoverBg },
        },
      },
    },
    MuiTableRow: {
      styleOverrides: {
        root: { '&:last-child td': { borderBottom: 0 } },
      },
    },
    // 自訂頁面直接用 MUI Table 的也要吃同一套語言（RaDatagrid 之外的表格）
    MuiTableCell: {
      styleOverrides: {
        root: { borderBottom: `1px solid ${jvTokens.rowHairline}`, fontSize: '12.5px', color: jvTokens.textSecondary },
        head: {
          color: jvTokens.muted,
          fontSize: '11px',
          fontWeight: 500,
          letterSpacing: '.12em',
          borderBottom: `1px solid ${jvTokens.hairline}`,
        },
      },
    },

    /* ---------- 輸入框：直角外框式，聚焦轉紫 ---------- */
    MuiTextField: { defaultProps: { variant: 'outlined' as const, size: 'small' as const } },
    MuiOutlinedInput: {
      styleOverrides: {
        root: {
          borderRadius: 0,
          '& .MuiOutlinedInput-notchedOutline': { borderColor: jvTokens.border },
          '&:hover .MuiOutlinedInput-notchedOutline': { borderColor: jvTokens.muted },
          '&.Mui-focused .MuiOutlinedInput-notchedOutline': { borderColor: jvTokens.primary, borderWidth: 1 },
        },
      },
    },
    MuiFilledInput: {
      styleOverrides: {
        root: {
          borderRadius: 0,
          borderTopLeftRadius: 0,
          borderTopRightRadius: 0,
        },
      },
    },

    /* ---------- Chip / 狀態 pill：直角細框 ---------- */
    MuiChip: {
      styleOverrides: {
        root: { borderRadius: 0, fontSize: '11px' },
        outlined: { borderColor: jvTokens.border },
      },
    },

    /* ---------- 其他小件 ---------- */
    MuiLinearProgress: {
      styleOverrides: {
        root: { borderRadius: 0, backgroundColor: jvTokens.track, height: 5 },
        bar: { borderRadius: 0 },
      },
    },
    MuiTooltip: {
      styleOverrides: {
        tooltip: { borderRadius: 0, backgroundColor: jvTokens.text, fontSize: '11px' },
      },
    },
    MuiMenuItem: {
      styleOverrides: {
        root: {
          fontSize: '13px',
          '&.Mui-selected': { backgroundColor: jvTokens.hoverBg, color: jvTokens.primary },
          '&.Mui-selected:hover': { backgroundColor: jvTokens.hoverBg },
        },
      },
    },
    MuiTabs: {
      styleOverrides: {
        indicator: { height: 3, backgroundColor: jvTokens.primary },
      },
    },
    MuiTab: {
      styleOverrides: {
        root: { textTransform: 'none' as const, letterSpacing: '.04em' },
      },
    },
  },
};

export default jvLightTheme;
