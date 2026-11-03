/**
 * 台灣 3D 地圖 —— 配色主題（唯一色票來源）。
 *
 * 原作（`temp/sc-datav-main` Demo2 / `temp/taiwan-scifi-map.html`）是**黑底加法發光**：
 * 靠 AdditiveBlending 把青色打亮到近白。那一套搬到白底會整個消失 —— 加法混合的上限
 * 就是白，底已經是白就等於沒畫（切頁雲霧那次踩過同一個坑）。
 *
 * 所以白底版的規則是**減法**，三條：
 *   1. 所有 AdditiveBlending → NormalBlending，發光改成「上墨」。
 *   2. 亮度階梯整個翻面：原本「越高越亮」的側牆，改成「越低越深」（貼地＝接觸陰影）。
 *   3. ⭐ 顏色是稀缺資源。
 *
 * 兩套主題共用上面的骨架，差在**顏色用得多兇**與**背景物件的存在感**：
 *
 *   'jv'  —— 對齊 `themes/jvTheme.ts` 的 JV 1C 設計。紫色給掃描光帶與標記，
 *            背景物件（格線／光柱／光環／飛線）也帶淡紫。
 *   'lab' —— 對齊 `src/web/src/futuristic-white`（NULLARC / HUD LAB）。
 *            ⭐ 整張圖走**白色色階**：島是白→灰→黑細線的墨色階梯，
 *            背景物件壓到 `--azfw-grid-line`（rgba(10,10,10,.045)）那個量級，
 *            只是紋理、不是內容。顏色只剩選取態的那一點主色紫
 *            （借 canvas L375 的規則："One red carrier... the only colour permitted
 *            inside." —— 借的是「只有一個載體色」，載體本身換成系統主色）。
 */
import { jvTokens } from '../../themes/jvTheme';

/** 各層的目標不透明度 —— 兩套主題的「存在感」差別主要就在這一組數字 */
export interface MapAlpha {
  /** 縣市側牆 */
  sideWall: number;
  /** 縣市頂面輪廓線 */
  edge: number;
  /** 海岸線 */
  coast: number;
  /** 外輪廓光牆 */
  boundaryWall: number;
  trail: number;
  trailHead: number;
  cone: number;
  halo: number;
  flyLine: number;
  /** 底部刻度盤（內圈 / 外圈） */
  ring1: number;
  ring2: number;
  /** 上升光柱 */
  beam: number;
}

export interface MapPalette {
  name: MapThemeName;
  /** 卡片內的畫布底色（HTML 端也要用同一個值，霧色跟著它走） */
  bg: string;
  fog: string;
  fogNear: number;
  fogFar: number;

  /** 頂面底色（貼圖是這個顏色加墨格） */
  topFace: string;
  /** 頂面輪廓線 */
  edge: string;
  /** 側牆漸層：上緣（近頂面）／下緣（貼地，較深＝接觸陰影） */
  sideTop: string;
  sideBottom: string;
  /** 沿側牆上掃的光帶 */
  scan: string;
  /** ⭐ hover 的焦點色 —— 全場唯一允許出現的「顏色」 */
  focus: string;

  /** 海岸線 */
  coast: string;
  /**
   * 背景物件的主色（光柱、飛線、刻度盤、拖尾）。
   *
   * ⛔ 試過也否決掉的方案：把這一組換成**加法混合的純白**。理由聽起來成立 ——
   *    lab 的底色是 `#f2f2f0` 不是純白，上面還有 13 階 headroom，島被打光後約在 230，
   *    所以純白的加法光畫得出來、而且會是全場最亮，等於還原原作「發光」的語意。
   *    實際上螢幕：13 階的動態範圍太窄，光看起來只是「霧掉」，反而不如墨色乾淨。
   *    **白底就是走減法，這條結論不要再翻。**
   */
  bgInk: string;
  /** 背景物件更淡的一階 */
  bgInkSoft: string;
  /** 縣市標記（菱錐）—— 22 個一起出現，所以要比 bgInk 再退一階 */
  marker: string;
  /** 標記的尺寸倍率。MONO 只在特寫（鏡頭很近）時才出現，所以要比總覽用的小 */
  markerScale: number;
  /** 標記上下浮動的振幅（地圖局部單位）。近距離下原作的 0.22 晃得太明顯 */
  markerBob: number;
  /**
   * 標籤浮在頂面之上多高（地圖局部單位）。原作固定 0.62。
   * ⚠️ 不能直接乘 `markerScale` —— 指標縮小後算出來的 0.24 剛好落在菱錐尖端，字會被壓到。
   *    這個值要能同時讓開「菱錐頂端 + 浮動振幅」，所以獨立成參數。兩邊都撞過。
   */
  labelLift: number;
  /**
   * 標記是否常態顯示。
   * false = 只有被特寫的那一個會出現（其餘連畫都不畫，不是靠透明度藏）——
   * 22 個指標同時在畫面上太雜。
   */
  markersAlwaysOn: boolean;
  /** 縣市標籤字級（px）：平時 / hover / 被特寫選中 */
  labelSize: { base: number; hot: number; sel: number };
  /** hover 時焦點色滲進側牆的量。lab 版壓低，讓顏色停在輪廓線上而不是變成填色 */
  focusMix: number;
  /**
   * 焦點景深 —— 沒 hover 時整座島退到中景，hover 到的縣市才回到前景。
   *
   * ⛔ **不是用不透明度做的。** 第一版把島的 opacity 壓到 0.38，結果底下的地板格線
   *    與其他縣市的掃描光帶會透出來，比原本更雜亂（使用者實地指出）。
   *    `DepthField.tsx` 早就寫過同一件事：深度來自**受光的面 + 遮擋 + 尺度遞減 +
   *    大氣透視**，不是靠透明度疊。所以這裡改成**顏色往背景色退**（aerial perspective），
   *    材質一律維持不透明 —— `island` = 0 全退成背景色、1 = 原色。
   *
   * 級距仍照 `futuristic-white/styles/atmos.scss` 量到的三階
   * （量測對象 to-portfolio.com）：**背景 0.003–0.019 / 中景 0.18–0.4 / 前景 0.9–1**。
   * 對應到這張圖：
   *   前景 = hover 的縣市（島層 ×1）
   *   中景 = 其餘縣市 + 海岸線（島層 × `island`）
   *   背景 = 地板格線 / 飛線 / 光圈 / 光牆 / 標記（alpha 本身就壓在 0.02~0.05）
   *
   * ⛔ 那份文件的硬規則要一起守：**大氣只發生在背景層與容器層，資訊層一律銳利** ——
   *    所以左上標題、讀數面板、HUD 文字都不吃這組值。
   */
  focusDepth: { island: number; label: number };

  floorBase: string;
  floorLine: string;
  /** 地板 shader 的格線強度（細格 / 粗格） */
  floorGrid: [number, number];
  /**
   * 外輪廓光牆的高度（地圖局部單位，原作 2.6）。
   * ⚠️ 這道牆是**每個外環**都會擠出來的，包含釣魚臺那種小島礁 —— 高度一大，
   *    白底上就會在離島位置變成一根根漂浮的板子（實測誤判成「光柱」查了一輪）。
   */
  boundaryHeight: number;
  /** 鏡面倒影：反射率（<1 = 壓暗）與混合強度 */
  reflect: { tint: [number, number, number]; mix: number };

  /** 頂面貼圖用的 rgba 字串 */
  grid: { base: string; noise: string; fine: string; bold: string; block: string };

  /**
   * 底部刻度盤要不要轉。
   * ⚠️ 之前把整層關掉是因為「有一道光沿著圓跑」—— 那其實是**旋轉**造成的
   *    （盤面貼圖上有四段掃描弧，一轉就變成跑動的光），不是圓圈本身的問題。
   *    所以圈圈留著、把 spin 關掉就好。
   */
  ringSpin: boolean;
  /**
   * 哪些層要建。
   *
   * ⚠️ 這件事**不能**用「alpha === 0 就不建」判斷 —— 指標（cone/halo）平時的 alpha
   *    就是 0，但特寫時要靠 `focusTarget` 浮出來。兩種「看不見」的意思不一樣：
   *      alpha 0  = 現在不顯示，之後可能顯示
   *      layers   = 這個主題根本不要這個東西（連 geometry 與每幀更新都省掉）
   */
  layers: { trail: boolean; beams: boolean; flyLines: boolean; rings: boolean };
  /** 標籤是否常態顯示。false = 只顯示被特寫選中的那一個 */
  labelsAlwaysOn: boolean;
  /**
   * 底部刻度盤貼圖的 alpha 級距壓縮量 0~1。
   *
   * ⚠️ 盤面貼圖內部的 alpha 從 0.10（細刻度）到 0.42（掃描弧）差了 4 倍。整層不透明度
   *    調到 MONO 的量級（0.05）之後，0.10 那批只剩 0.005 ＝ 直接消失，看起來就像
   *    「MONO 的圓環裝飾比較少」。這裡把貼圖**內部**的級距壓平，讓所有元素一起浮現，
   *    而不是把整層調亮（那會讓背景又變吵）。0 = 照原樣。
   */
  plateFlatten: number;
  /**
   * 是否開放滑鼠 hover 拾取。
   * MONO 關掉 —— 這一版的互動只有「從清單選一個縣市」，滑過去就抬升/變色反而
   * 讓「安靜的背景」名不副實（也省掉每幀的 raycast）。
   */
  hoverEnabled: boolean;
  lights: { ambient: number; dir: number; fill: number };
  alpha: MapAlpha;
}

export type MapThemeName = 'jv' | 'lab';

/* ---------------- JV 1C：白底 + 紫 ---------------- */
const jv: MapPalette = {
  name: 'jv',
  bg: '#ffffff',
  fog: '#ffffff',
  fogNear: 18,
  fogFar: 62,

  topFace: '#ffffff',
  edge: jvTokens.primaryDark, // #3E3470
  sideTop: '#ece9f4',
  sideBottom: '#867ea6',
  scan: jvTokens.primary, // #5B4A96
  focus: jvTokens.primary,

  coast: jvTokens.primary,
  bgInk: jvTokens.primary,
  bgInkSoft: jvTokens.primaryLight, // #7E6DB6
  marker: jvTokens.primary,
  markerScale: 1,
  markerBob: 0.22,
  labelLift: 0.62,
  markersAlwaysOn: true,
  labelSize: { base: 11, hot: 12.5, sel: 15 },
  focusMix: 0.5,
  // VIOLET 維持既有樣貌：不做景深退場
  focusDepth: { island: 1, label: 1 },

  floorBase: '#ffffff',
  floorLine: jvTokens.primary,
  floorGrid: [0.05, 0.12],
  boundaryHeight: 1.5,
  reflect: { tint: [0.55, 0.5, 0.68], mix: 0.7 },

  grid: {
    base: '#ffffff',
    noise: '43,42,51',
    fine: 'rgba(43,42,51,.045)',
    bold: 'rgba(91,74,150,.13)',
    block: '91,74,150',
  },

  ringSpin: true,
  layers: { trail: true, beams: true, flyLines: true, rings: true },
  labelsAlwaysOn: true,
  plateFlatten: 0,
  hoverEnabled: true,
  lights: { ambient: 1.5, dir: 1.1, fill: 0.55 },
  alpha: {
    sideWall: 1,
    edge: 0.55,
    coast: 0.7,
    boundaryWall: 0.13,
    trail: 1,
    trailHead: 0.9,
    cone: 0.85,
    halo: 0.55,
    flyLine: 0.45,
    ring1: 0.3,
    ring2: 0.14,
    beam: 0.09,
  },
};

/* ---------------- LAB：futuristic-white（NULLARC / HUD LAB） ----------------
   色碼逐項對應 `src/web/src/futuristic-white/styles/tokens.scss`：
     --azfw-bg #f2f2f0 / --azfw-ink #0a0a0a / --azfw-text-faint #8d8d8d
     --azfw-inert #b9b9b9 / --azfw-accent #d81324
     --azfw-grid-line rgba(10,10,10,.045) / --azfw-border-faint rgba(10,10,10,.16)

   ⭐ 這一版的重點不是換色，是**降存在感**：背景物件（光柱／飛線／刻度盤／光牆）
     全部壓到 0.03~0.10，跟參考頁的 plexus 網同一個量級 —— 有紋理，但不搶戲。
     力氣全部押在島本身的墨色階梯與黑色細線上（＝參考頁那組黑弧與刻度）。 */
const lab: MapPalette = {
  name: 'lab',
  bg: '#f2f2f0',
  fog: '#f2f2f0',
  fogNear: 16,
  fogFar: 54,

  // 頂面比底色再亮一階，島才會從畫布「浮」起來（底色不是純白，所以有空間）
  topFace: '#fbfbfa',
  // 一階灰而不是純黑（--azfw-text-dim）：atmos 那張參考的背景層裡沒有任何純黑，
  // 黑是留給資訊層的。島的輪廓退一階，整體才不會硬。
  edge: '#3d3d3d',
  sideTop: '#eeeeeb',
  // ⚠️ 貼地的深色是「接觸陰影」，不是要它變成一條黑帶。背景模式被大氣透視蓋著看不出來，
  //    一特寫恢復原色就會整片壓出來 —— 從原本的 #8f8f8c 一路提到 #cfcfcc。
  sideBottom: '#cfcfcc',
  // 掃描光帶＝墨色密度掃過，不是顏色。純黑在特寫下太重，退到 --azfw-text-faint
  scan: '#8d8d8d',
  // ⭐ 全場唯一的顏色，只給 hover（選取態）。
  //    參考頁用的是自己的紅 `--azfw-accent #d81324`，這裡換成系統主色 —— 借的是
  //    「顏色極度節制、只標焦點」這條規則，不是那個紅；後台的選取態要跟 JV 一致。
  focus: jvTokens.primary,

  coast: '#3d3d3d',
  bgInk: '#0a0a0a',
  bgInkSoft: '#b9b9b9', // --azfw-inert
  // MONO 的指標只有「被選中的那一個」會出現（markersAlwaysOn: false），
  // 所以它可以直接是主色 —— 全場唯一變紫的東西就是選中的縣市與它的指標。
  marker: jvTokens.primary,
  markerScale: 0.38,
  markerBob: 0.05,
  labelLift: 0.42,
  markersAlwaysOn: false,
  labelSize: { base: 13, hot: 15, sel: 19 },
  // 紅只當「載體線」（參考頁就是一條紅弧 + 一個紅點），不當填色
  focusMix: 0.22,
  focusDepth: { island: 0.38, label: 0.5 },

  floorBase: '#f2f2f0',
  floorLine: '#0a0a0a',
  // 對齊 --azfw-grid-line 的 .045
  floorGrid: [0.02, 0.035],
  // 壓成貼地的矮裙：atmos 那張參考的背景沒有任何「立起來」的量體
  boundaryHeight: 0.45,
  // ⛔ MONO 不做鏡面倒影（mix = 0 ⇒ 連 Reflector 都不建）。
  //    倒影在斜角下會被透視拉成垂直抹痕（島與離島底下那幾道），跟「背景只是紋理」
  //    直接衝突；關掉同時省下整個第二遍場景渲染。
  //    ⭐ 補回原作偷工減料的那個**真鏡面仍在 VIOLET 主題**，沒有被刪掉。
  reflect: { tint: [0.62, 0.62, 0.61], mix: 0 },

  grid: {
    base: '#fbfbfa',
    noise: '10,10,10',
    fine: 'rgba(10,10,10,.045)',
    bold: 'rgba(10,10,10,.09)',
    block: '10,10,10',
  },

  ringSpin: false,
  // 拖尾＝沿島邊緣跑的光、光柱＝天上的小黑條、飛線＝從臺北散射出去的弧 ——
  // 都跟「背景只是紋理」相衝，MONO 一律不建（特寫時也不會回來）。
  layers: { trail: false, beams: false, flyLines: false, rings: true },
  labelsAlwaysOn: false,
  plateFlatten: 0.62,
  hoverEnabled: false,
  lights: { ambient: 1.45, dir: 1.0, fill: 0.35 },
  alpha: {
    sideWall: 1,
    edge: 0.62, // 黑細線是這一版的骨架，要比 jv 更實
    coast: 0.8,
    boundaryWall: 0.02,
    trail: 0,
    trailHead: 0,
    // 指標平時不出現（22 個一起在畫面上太雜），只在特寫時隨 VIOLET 的 alpha 浮出。
    // ⚠️ 0 仍然會建出來 —— 靠 Reveal 的 focusTarget 拉起來，不是「整層不建」那條路。
    cone: 0,
    halo: 0,
    flyLine: 0,
    // 圓圈留著當地板的製圖紋理；會跑的那道光是「旋轉」造成的，靠 ringSpin: false 關掉
    ring1: 0.055,
    ring2: 0.03,
    beam: 0,
  },
};

export const mapThemes: Record<MapThemeName, MapPalette> = { jv, lab };

/** 預設主題。改這裡就換整張圖的配色。 */
export const DEFAULT_MAP_THEME: MapThemeName = 'lab';
