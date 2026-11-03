# 台灣行政區 3D 圖層（tenant-console 自訂頁）

路由 `#/taiwan-map`，選單「台灣圖層」。three.js 場景在 `scene/`，用動態 `import()`
切成獨立 chunk —— 沒開這頁就不該把 three 拉進主 bundle。

**版型**：參考 `src/web/src/pages/futuristic-white-atmos` —— 3D 是**滿版背景**，
資訊層（左上標題、左下讀數、右下提示、右上配色切換）是浮在上面的銳利文字，沒有卡片外框。
對應專案既有結論「大氣只在背景與容器層、資訊層一律銳利」。

⚠️ **高度不寫死**：`#main-content`（`.RaLayout-content`）本身就是
`display:flex; flex-direction:column`、高度已由 appFrame 撐定（實測 915px），
所以頁面根節點只要 `flex:1 + minHeight:0` 就會填滿；左右 `mx:-1` 抵銷
`.RaLayout-content` 的 `padding: 0 8px` 讓畫面真的出血。

```
TaiwanMapPage.tsx        RA 頁殼：JV 語言的卡片 + HUD + 讀數面板，lazy 載入場景
data/taiwan-country.topo.json   來源 temp/taiwan-country.topo.json（22 個縣市，40KB）
scene/
  createScene.ts   ← Demo2 map/index.tsx + lights.tsx（場景/燈/霧/進場/主迴圈）
  palette.ts       ← 兩套配色主題的唯一真值來源（⚠️ 純資料、不 import three）
  glColor.ts       ← three 的顏色 helper（色彩空間慣例寫在這）
  ctx.ts           ← 各層 builder 共用的建構情境（palette/textures/reveal/res/depth）
  geo.ts           ← TopoJSON 解碼 + 麥卡托投影 + 外輪廓（html §2~§4，不依賴 three）
  textures.ts      ← 程序化貼圖（html §5）
  reveal.ts        ← 進場淡入登記簿 + 資源回收
  counties.ts      ← base.tsx + shape.tsx + shaderMaterial.tsx（§7 / §9）
  boundary.ts      ← boundary.tsx（§10）
  trail.ts         ← geoTrail.tsx（§11）
  markers.ts       ← cone.tsx（§12）
  flyLines.ts      ← flyLine.tsx（§13）
  bottomRing.ts    ← bottom.tsx（§14）
  floor.ts         ← mirror.tsx（§15）★ 補回原作的真鏡面
  beams.ts         ← beamLight.tsx（§16）
  controls.ts      ← drei OrbitControls 的等價實作（§17）
  labels.ts        ← label.tsx / drei Html（§20）
  rng.ts           ← 種子亂數（畫面要可重現，截圖驗證才有意義）
```

---

## 一、`temp/taiwan-scifi-map.html` 忠於原作嗎？

跟 `temp/sc-datav-main/src/pages/Demo2/map/*` 逐檔對照過。結論：**絕大多數忠實，
但有一處真的偷工減料。**

### 忠實的部分

| Demo2 | html 版 | 對照結果 |
|---|---|---|
| `shaderMaterial.tsx` | §7 `ScanMaterial` | 幾乎逐行相同：`bandHeight 0.45`、`progress/within/core/edge` 的算式一致。html 多了 hover 用的 `uHighlight` 項，edge 係數 0.45 vs 0.4 |
| `boundary.tsx` | §10 | 相同：丟掉上下蓋（`abs(vNormal.z) > 0.9` discard）、高度衰減 `pow(h, 1.6)` |
| `beamLight.tsx` | §16 | 完全相同：`pow(1-abs(vUv.x-0.5)*2, 2.0) * pow(sin(vUv.y*π), 0.5)`，亮度 `*2.0` |
| `shape.tsx` | §9 `remapUV()` | 相同：依全圖 bbox 重算 uv，讓網格貼圖跨縣市連續 |
| `cone.tsx` | §12 | 相同：菱錐上下浮動 + 光圈旋轉，hover 時加速 |
| `flyLine.tsx` | §13 | 相同：二次貝茲 TubeGeometry + 貼圖 offset 捲動 |
| `bottom.tsx` | §14 | 相同：兩層刻度盤反向轉 |
| `geoTrail.tsx` | §11 | 相同：沿最長環取樣 46 點，`pow(f, 2.6)` 衰減 |
| `index.tsx` gsap timeline | §19 | 等價實作（鏡頭俯衝 → 長高 → 淡入 → 解鎖） |

### ⛔ 真的少做的：`mirror.tsx`

Demo2 用 drei 的 `MeshReflectorMaterial`：

```tsx
<MeshReflectorMaterial blur={[400,100]} resolution={1024}
  mixBlur={10} mixStrength={10} depthScale={1} minDepthThreshold={0.85} />
```

那是**真的把場景重畫進一張 render target 的平面鏡**。html 版沒有做，改成
「把縣市 mesh 沿 y 翻面複製一份、塗成半透明青色 `#2b7f9b` opacity 0.3」再加一層
程序化網格地板。結果是：

- 倒影**只有縣市體塊** —— 光柱、菱錐、拖尾、外輪廓一律不會倒映
- 沒有模糊、沒有深度衰減、沒有距離衰減

**這一頁把它補回來了**：`floor.ts` 改用 three 內建的 `Reflector`（addons），跟
`MeshReflectorMaterial` 同一個原理（鏡射相機 + 斜截頭體裁切 + render target）。
drei 多做的模糊與深度混合，這裡用「距離衰減 + 反射率壓暗」取代 —— 白底地板本來就
不該給到鏡面級的清晰倒影。

> **怎麼確認它真的在跑**：把 `uTint` 暫時改成純紅跑一次。紅色會**準確落在**
> 島的底緣與每根光柱的腳下 —— 那正是平面鏡該有的位置。截圖確認過。
> 另外要有心理準備：島是**平貼在鏡面上**的薄片，所以倒影本來就只有底緣一圈，
> 這是幾何正確的結果（drei 版也一樣），不是沒生效。

### 次要的落差（html 版與這一頁都保留）

- 四張 png 素材（`quan1` / `guangquan01` / `fly_line` / `sc_normal_map1`）沒進 repo，
  一律用 canvas 程序化生成。
- 頂面原本是 `normalMap`，改成 albedo 的網格 `map`（沒有地形資料，法線貼圖沒意義）。
- `Cones` 原本用 drei `<Instances>`，這裡是共用 geometry/material 的個別 mesh
  —— 22 個標記的 draw call 差異無感，但 hover 的個別位移要靠各自的 transform。

---

## 二、兩套配色主題

頁首右上角的 `MONO / VIOLET` 可以即時切換（切換會重建場景、重播進場動畫）。
預設由 `scene/palette.ts` 的 `DEFAULT_MAP_THEME` 決定。

| | `jv` — VIOLET | `lab` — MONO（預設） |
|---|---|---|
| 對齊 | `themes/jvTheme.ts`（JV 1C 設計） | `src/web/src/futuristic-white`；存在感對齊 `pages/futuristic-white-atmos` |
| 底色 | `#ffffff` | `#f2f2f0`（`--azfw-bg`） |
| 島 | 白頂面 + 灰紫側牆 + 深紫輪廓 | 白頂面 + 灰階側牆 + **純黑細線輪廓** |
| 掃描光帶 | 飽和紫 `#5B4A96` | **墨色**（密度掃過，不是顏色） |
| 背景物件 | 帶淡紫，alpha 0.09~0.45 | 一律墨色/灰，**alpha 0.03~0.10** |
| 顏色 | 紫用在掃描帶、標記、飛線 | ⭐ **只有選取態的那一點主色** `#5B4A96` |

`lab` 的重點不是換色，是**降存在感**：光柱／飛線／刻度盤／外輪廓光牆全部壓到
`--azfw-grid-line`（`rgba(10,10,10,.045)`）那個量級 —— 有紋理，但不是內容。
力氣全部押在島本身的墨色階梯與黑細線上（＝參考頁那組黑弧與刻度）。

兩個只有上螢幕才看得出來的調整：

- **菱錐 ×22 不能用純黑** —— 22 個黑色實心會變成全圖最重的東西、壓過島本身。
  退到 `--azfw-text-faint #8d8d8d`（`palette.marker`）。
- **焦點色只當「載體線」，不當填色** —— 參考頁就是一條弧加一個點。
  hover 時滲進側牆的量（`palette.focusMix`）從 jv 的 0.5 壓到 0.22，
  顏色主要停在縣市輪廓線與標籤上。
- **選取色用系統主色，不是參考頁的紅** —— 借 futuristic-white 的是「顏色極度節制、
  只標焦點」這條規則，不是 `--azfw-accent #d81324` 那個紅。後台的選取態要跟側欄的
  選中項一致，所以載體色換成 `jvTokens.primary`。
- **背景層不准出現純黑** —— atmos 那張參考的背景裡沒有任何 `#000`，黑是留給資訊層的。
  島的輪廓與海岸線退到 `--azfw-text-dim #3d3d3d`。
- **地格鋪到地平線** —— 格線的距離尺度從 42 單位開始衰減（倒影另用 15 單位的尺度），
  斜視下自然收斂成一條地平線，這是 atmos 那張「滿版」感的主要來源。
- **外輪廓光牆壓成貼地矮裙**（`palette.boundaryHeight`，1.5 → 0.45）——
  這道牆是**每個外環**都會擠出來的，包含釣魚臺那種小島礁；高度一大，白底上就會在
  離島位置變成一根根漂浮的板子。
- **倒影 mix 壓到 0.18** —— 光柱有 2~6 單位高，倒影在斜角下會被透視拉成長條，
  白底上就是垂直抹痕。同時把倒影的半徑收到島的範圍內（`smoothstep(0.05, 0.55, rr)`）。
### MONO 的減法清單

| 拿掉的東西 | 為什麼 |
|---|---|
| 上升光柱（`layers.beams`） | 墨色版裡就是一堆小黑條在天上飄 |
| 海岸線拖尾（`layers.trail`） | 一道沿著島的邊緣跑的光 |
| 臺北放射的飛線（`layers.flyLines`） | 同上，弧線在背景層太顯眼 |
| 鏡面倒影（`reflect.mix = 0`） | 斜角下被透視拉成垂直抹痕 |
| 常態的縣市標記（`markersAlwaysOn`） | 22 個指標同時在畫面上太雜 |
| 常態的縣市標籤（`labelsAlwaysOn`） | 同上 |
| 滑鼠 hover 拾取（`hoverEnabled`） | 這一版的互動只有「從清單選一個縣市」；滑過去就抬升/變色會讓「安靜的背景」名不副實（順便省掉每幀 raycast） |
| 刻度盤的旋轉（`ringSpin`） | 圓圈本身留著；會跑的那道光是**旋轉**造成的（盤面貼圖有四段掃描弧） |

⚠️ **「不建」與「alpha 0」是兩件事，判準不能混用**：

- `layers.X = false` —— 這個主題根本不要它，連 geometry 與每幀更新都省掉
- `alpha.X = 0` —— 現在不顯示，但特寫時要靠 `focusTarget` 浮出來（指標就是這種）

早期用「alpha === 0 就不建」當判準，等到指標改成「只有特寫才出現」時就誤傷了。

### 特寫（`handle.focus(id)`）

左側清單點一個縣市 → 鏡頭滑過去、地圖從背景模式回到清楚的樣子；點 OVERVIEW 解除。

- **運鏡走計時 tween，不是既有的指數阻尼。** 阻尼（`x += (goal - x) * dt * k`）永遠只是
  「越來越靠近」，沒有到達時間，而且最快的那一刻是第一幀（沒有 ease-in）——當成運鏡看
  就是先猛衝再拖尾。特寫用 `easeInOutCubic` + 固定 1100ms，使用者一動手就取消。
- **取景照縣市自己算**：距離來自它的外接半徑，方位角來自「島心 → 該縣市」的延長線
  （所以每次選取都自帶一段轉身）。⚠️ 半徑只能算面積最大的那一環 —— 掃過所有 polygon
  的話，臺東的蘭嶼綠島、宜蘭的龜山島釣魚臺、屏東的小琉球會把半徑灌大好幾倍。
- **⛔ 特寫改變的是「清晰度」，不是配色。** 曾經整組 morph 成 VIOLET，結果整片側牆變紫，
  干擾性太強。現在全場唯一會變紫的是**被選中的那一個縣市**（走 scan shader 的 `uFocus`，
  那是 per-county 的）與它的指標、標籤。

### 焦點景深（`palette.focusDepth`）

沒 hover 時整座島退到中景、hover 到的縣市回到前景，其餘配件壓在背景階。
級距抄 `futuristic-white/styles/atmos.scss` 量到的三階（量測對象 to-portfolio.com）：
**背景 0.003–0.019 / 中景 0.18–0.4 / 前景 0.9–1**。同時守它的硬規則
「大氣只發生在背景層與容器層、資訊層一律銳利」—— 左上標題、讀數面板、HUD 文字不吃景深。

⛔ **不可以用不透明度做。** 第一版把島的 opacity 壓到 0.38，結果地板格線與其他縣市的
掃描光帶會從島**透出來**，比原本更雜亂。`DepthField.tsx` 早就寫過同一件事：深度來自
受光的面 + 遮擋 + 尺度遞減 + **大氣透視**，不是靠透明度疊。所以實作是
**顏色往背景色 mix**，材質一律維持不透明：

- 側牆（自訂 shader）：多一組 `uHaze` / `uDepth2`，在寫進 `gl_FragColor` 前 mix
- 頂面（`MeshStandardMaterial`）：`onBeforeCompile` 在 `#include <colorspace_fragment>`
  **之前**注入同一條 mix —— 那個位置的 `gl_FragColor` 還在 linear，跟 `uHaze` 同空間
- 輪廓線 / 海岸線：`color.copy(bg).lerp(inkColor, k)`，opacity 不動

### ⛔ 試過並否決的方案：加法混合的純白光

想法是：lab 的底色 `#f2f2f0` 不是純白，上面還有 **13 階 headroom**，而島被打光後
約落在 230 —— 所以純白的加法光畫得出來、而且會是全場最亮，等於還原原作「發光」的語意，
不必把光改成墨。

實際上螢幕後否決：13 階的動態範圍太窄，光看起來只是「霧掉」，不如墨色乾淨。
**白底就是走減法，這條結論不要再翻。**

---

## 三、白底反轉：為什麼不能照抄配色

原作整套是**黑底加法混合發光**（`AdditiveBlending`）。加法混合的上限就是白，
底已經是白就等於沒畫 —— 切頁雲霧那次踩過同一個坑。所以白底版是**減法**，三條規則：

1. 所有 `AdditiveBlending` → `NormalBlending`，發光改成上墨。
2. 亮度階梯翻面：原本「越高越亮」的側牆改成「越低越深」（貼地那圈＝接觸陰影）。
3. ⭐ 飽和紫是稀缺資源，只給掃描光帶與焦點；其餘走墨色/灰紫階梯。
   這條抄自 `src/web/src/futuristic-white`（"One red carrier... the only colour
   permitted inside."）—— 白底科幻的骨架是線，不是顏色。

色票對齊 `src/tenant-console/src/themes/jvTheme.ts` 的 `jvTokens`（JV 1C 設計）。

### 逐項對照

| 元素 | 原作（黑底） | 這一頁（白底） | 理由 |
|---|---|---|---|
| 背景 / 霧 | `#000308`，霧到黑 | renderer `alpha:true`，霧到白 | 讓卡片白底透上來，遠景自然溶進頁面 |
| 側牆漸層 | 下 `#08172c` → 上 `#5fe3ff` | 下 `#867ea6` → 上 `#ece9f4` | 翻面；貼地深色＝接觸陰影，體塊才會站在地板上 |
| 掃描光帶 | `#a8f6ff`（比底色亮） | `#5B4A96`（比底色濃） | 白底上「亮」＝消失 |
| 側牆 alpha | `0.78 + 0.22*h`（上緣實） | `1.0 - 0.16*h`（下緣實） | 同上，梯度翻面 |
| hover | `col * (1.0 + uHighlight*0.5)` 打亮 | 往掃描色多混 + 提高 alpha | 白底乘亮只會洗白 |
| 頂面材質 | metalness .55 / roughness .65 | metalness 0 / roughness .85 | 白底高金屬度＝一片死白 |
| 海岸線 | 加法青光 0.75 | 一般混合深紫 0.7 | 白底版的骨架就是這條線，強度要留住 |
| 外輪廓光牆 | 高 2.6、不透明 0.34 | 高 1.5、不透明 0.13 | 實色遮蔽照抄會糊出一圈紫裙邊，把海岸線壓掉 |
| 拖尾 | 加法 + 逐點顏色衰減 | 一般混合 + **逐點 alpha**（color attribute itemSize 4） | 加法下「變暗」＝「變透明」，白底沒這回事 |
| 光柱 | `vec4(uColor*b*2.0, b*uOpacity)`，0.55 / r=.035 / 26 根 | `vec4(uColor, b*uOpacity)`，0.09 / r=.018 / 18 根 | 照抄會變成一片髒污的垂直抹痕（實測） |
| 底部光環 | 加法 0.55 / 0.28 | 一般混合 0.30 / 0.14 | 白底上實色刻度很搶眼 |
| 地板格線 | `smoothstep(0, .035, abs(fract(p/s)-.5))` | 距離**乘回世界單位**，線寬固定 0.012 / 0.02 | ⚠️ 原式的線寬會跟著格距放大：10 單位的粗格畫出來是 **0.7 單位寬的帶**。黑底看不出來，白底就是滿版斜紋（實測） |
| 地板範圍 | `length(vP)/34.0` | `/15.0`，格距 2/10 → 1/5 | 島只有 5.3 單位，原基準等於格線鋪到天邊 |
| 燈光 | ambient 1.9 + dir 2.6 | ambient 1.5 + dir 1.1 | 白底照抄會整片死白 |
| 鏡面倒影 | 亮的倒影自然浮出 | `uTint` 壓暗到 0.55/0.5/0.68 並偏紫 | 白色的島倒映在白色地板上等於沒畫 |
| 相機 | `r = 11.2`（16:9 滿版） | `r = 8.8` | 這頁 canvas 約 2:1，原距離下島只佔畫面 1/3 |
| 進場時間軸 | 3.6s | **× 0.6**（總長 2.16s、可互動 1.86s） | 後台分頁不該讓人等 3.6 秒；比例與 easing 照原作，只縮時間 |

---

## 四、幾個踩過的坑

- **⚠️ 載入遮罩不要蓋到 `onReady`。** 實測「點下去 → canvas 出現」只有 **721ms**，
  剩下的是進場動畫。遮罩蓋到動畫結束，使用者看到的就是「讀了 3.8 秒」而不是一段動畫。
  所以 `createTaiwanMap` 分成兩個 callback：`onFirstFrame`（收遮罩）與 `onReady`（淡入 HUD）。
- **自訂 shader 的顏色空間**：`new THREE.Color(hex)` 會轉進 linear working space，
  自訂 shader 直接寫 `gl_FragColor` 就會暗掉一大截。解法不是 `NoColorSpace`，而是
  **兩端都留在 linear**：顏色照常轉，fragment shader 最後加 `#include <colorspace_fragment>`。
  額外好處是鏡面的 render target 也是 linear（非 XR 的 RT 一律輸出 linear-sRGB），
  可以直接 mix，不必手刻 gamma。
- **`renderer.setSize` 會清空 drawing buffer**（無條件寫 `canvas.width/height`）。
  `ResizeObserver` 在版面穩定時也會叫 → 尺寸沒變一定要直接 return。
- **`renderer.dispose()` 不會把 canvas 從 DOM 拿掉**，漏掉就會疊出第二張、
  上面那張直接 context lost。`dispose()` 裡要 `renderer.domElement.remove()`。
- 座標一律換算成**容器相對**，不是 `innerWidth/innerHeight` —— 用視窗座標拾取會整個偏掉。
- ⛔ **`new Material({ color: someColorObject })` 不會持有你那個 Color。**
  three 的 `Material.setValues` 遇到 Color 是走 `currentValue.copy(newValue)` ——
  材質最後拿的是它自己 new 出來的 Color，你傳進去的物件直接被孤立。
  特寫的配色 morph 就是這樣壞掉的：登記在 **shader uniform**（`{ value: color }`，直接持有）
  的側牆真的變色了，登記在**材質建構子**的指標／飛線／刻度盤完全沒動 ——
  畫面上就是「一半 MONO 一半 VIOLET」。正確寫法是建完材質再登記它自己的欄位：
  `reveal.color(m.color, from, to)`。
- ⛔ **特寫的配色不要做「只換某幾層」的折衷。** 試過一次（VIOLET 的背景配件 alpha 是配
  遠景調的，近距離看太重，所以只覆寫那幾層），結果是不上不下。
  最後的結論更乾脆：配色根本不隨特寫改變，只有選中的那一個縣市變色。
- ⚠️ **MONO 的色票是在「背景模式」下調的，一定要再回特寫看一次。** 背景模式整座島被
  大氣透視推到 0.38，側牆下緣與掃描光帶怎麼調都看不太出來；一特寫恢復原色就整片壓出來
  （`sideBottom` 從 #8f8f8c 一路提到 #cfcfcc、`scan` 從純黑提到 #8d8d8d 才收得住）。
- ⚠️ **同一張貼圖在不同不透明度下，「內部」的 alpha 級距會決定看得到幾個元素。**
  刻度盤貼圖內部從 0.10（細刻度）到 0.42（掃描弧）差 4.2 倍，乘上 MONO 的 0.055 之後
  0.10 那批只剩 0.005 ＝ 消失，看起來就像「MONO 的圓環裝飾比較少」。解法是壓縮貼圖
  **內部**的級距（`plateFlatten`，`a ** (1 - flatten)`，4.2× → 1.7×），不是把整層調亮。
- ⭐ **查「這塊東西是哪一層畫的」最快的方法：一次把每一層染成不同顏色，跑一張圖。**
  垂直灰條那次先猜光柱（錯）、再猜光牆（只對一半），把 beams/flyLines/boundary/
  bottomRing/trail/markers 各染一色後一眼就看到真兇是**鏡面倒影**。
  ⚠️ 染完務必還原並 grep 確認（`'#ff0000'` 之類）—— 那張圖看起來很像設計稿翻車。
- ⚠️ **探針的取樣框一定要夾在截圖範圍內。** 版面改成滿版後 canvas 是
  `y=48 h=915`，底部超出 960 的視窗；超界的 `getImageData` 回傳透明黑，
  量出來會是「深墨 100%」這種一看就假的數字。

## 五、驗證過的事實（真瀏覽器操作）

`https://localhost:8444/tenant-console#/taiwan-map`，1440×960，走選單真實點擊：

| 項目 | 數字 |
|---|---|
| 點下去 → canvas 出現 / → 可互動 | 721ms / 2537ms |
| 滿版後的 canvas 盒子 | `239,48 1201x915`（`flex:1` 撐出來的，沒有寫死高度） |
| canvas 非白像素占比 | 7.1%（調校前 17.2%，雜訊砍掉後剩地圖本體） |
| 紫像素占比 | 2.7% |
| **青像素占比** | **0.00%**（原作配色殘留的檢查，必須是 0） |
| hover 南投縣 → 讀數面板 | `南投縣` / `ID 10008 · AREA 300.7`；移開回空 |
| 掃描光帶（側牆濃紫% 連續 14 幀） | 1.08 → 3.50 起伏，振幅 2.42 個百分點 |
| 標籤 | 22 個全部 on，同時可見 11 個（讓位規則生效） |
| 切走再切回 | host 底下 canvas 0 → 1（沒有疊加，dispose 正常） |
| console error | 0（既有的 403 與 MUI warning 不算） |

`lab` 主題（同樣走選單真實點擊，取樣框從 canvas rect 推導）：

| 項目 | 數字 |
|---|---|
| **idle 有彩度的像素** | **0.03%**（且那 0.03% 是讀數面板的 2px 主色左框，不是 3D 場景）—— 幾乎純灰階 |
| hover 後有彩度的像素 | 0.11%，全部是主色紫；**紅 0.00%** —— 顏色只在焦點出現 |
| 深墨像素（島的輪廓與側牆） | 0.92% |
| 對照 `jv` 主題 | 有彩度 1.38%、紫 2.33% |
| 兩套主題來回切 | host 底下 canvas 維持 1（沒有疊加） |
| 三角形 / draw call / 幀 | **21,246 / 130.0**，120fps 滿幀（一開始是 60,239 / 447.8） |
| 特寫運鏡 | 速度 頭 0.037 / 中 0.815 / 尾 0.036，相鄰幀跳動 8.7%，0 掉幀 |
| 背景模式可見標籤 / 特寫中 | `[]` / `["臺東縣"]` |
| MONO hover | 讀數面板無反應、cursor 維持 default（hover 已關） |

> ⚠️ 探針注意：驗運鏡順不順，**每幀位移一定要除以幀時距**。rAF 的間隔本來就會抖
> （實測中位數 8.3ms、最大 11.6ms），不除的話會把幀時距的抖動算成頓挫 ——
> 同一段運鏡量出來是 43.1%，除完之後是 6.8%。
>
> ⚠️ 探針注意：**不要用「連拍兩張比對差異」去驗「會動的東西是否消失」**。
> 鏡頭本身有自轉（`goal.theta += dt * 0.035`），整個畫面都在漂，量到的 0.9%
> 變動像素分不出是光柱還是鏡頭。改數 draw call 才是能回答這題的量。

> ⚠️ 探針注意：「非白像素占比」這個指標**不能跨主題比**。`lab` 的底色是 `#f2f2f0`，
> 用「接近純白」當背景判準會把整張畫布都算成「有畫東西」（實測 97.7%）。
> 跨主題要看的是「深墨占比」與「有彩度占比」。
