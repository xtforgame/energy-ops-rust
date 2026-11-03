# RouteTransition — 後台切頁效果

tenant-console 切換頁面時的轉場。**預設 `none`（硬切）**，要換效果改一行：

```tsx
// src/tenant-console/src/layout/Layout.tsx
<RouteTransition mode="crossfade">{children}</RouteTransition>
```

`mode` 可填 `'none'` | `'enter'` | `'crossfade'`。另有 `duration`（秒）與 `offset`（px）兩個微調參數。

---

## 三種模式

| mode | 效果 | DOM | 依賴 | 適合 |
|---|---|---|---|---|
| **`none`**（預設） | 硬切，零轉場 | **不加任何 wrapper**，與沒有這層時完全相同 | 無 | 追求零風險、或這個專案不要動畫 |
| **`enter`** | 新頁淡入 + 上移 6px，永遠只有一層 | 一層 flex column wrapper | motion | 想要順一點又不想承擔疊層成本 |
| **`crossfade`** | 舊頁溶解成新頁，兩層同時在場 | grid 容器 + 兩層 `gridArea: 1/1` | motion + react-router `UNSAFE_LocationContext` | 要最好的視覺連續性 |
| **`cloud`** | 雲從右掃到左蓋住內容區，蓋滿的瞬間換頁，再掃出去 | 不加 wrapper；雲幕是 portal 到 body 的 fixed canvas | three（動態 import）+ `UNSAFE_LocationContext` | 想要有記憶點的重場面 |

三種都會在使用者開啟「減少動態」（`prefers-reduced-motion`）時退化成無動畫。

### 為什麼 `enter` 不做離場

RA5 傳給 `<Admin layout>` 的 children 是一個**沒有帶 `location` 的 `<Routes>`**
（`ra-core/src/core/CoreAdminRoutes.tsx`）。離場中的那份仍是活的 React 子樹，
router context 一變就會重新解析成新頁面 —— 直接加 `AnimatePresence` 的 `exit`，
看到的會是「**新**內容淡出、**新**內容淡入」，不是舊溶新。

`crossfade` 解掉這件事的方法是 `FrozenLocation.tsx`：**只在離場期間**把
`UNSAFE_LocationContext` 凍在最後一次在場的值。「只在離場」不能省 —— 無條件凍結會讓
同一個 key 內的導覽（`/users/3 → /users/5`，不 remount）永遠看不到新 location。

---

## 什麼時候會重播動畫

`getRouteKey.ts` 把 pathname 收斂成 key，key 變才重播：

| 導覽 | key | 重播？ |
|---|---|---|
| `/users` → `/projects` | `users` → `projects` | ✅ |
| `/users` → `/users/create` | `users` → `users/create` | ✅ |
| `/users` → `/users/3` | `users` → `users/edit` | ✅ |
| `/users/3` → `/users/5` | `users/edit` → `users/edit` | ❌ 換記錄不閃 |
| `/users?page=2`、改 filter | pathname 沒變 | ❌ |

---

## 挑 `crossfade` 前要知道的三件事

1. **疊合期間（預設 ~130ms）兩份 RA 頁面同時掛載** —— 兩份 react-query 訂閱、兩份
   mount effect。若某頁在 mount/unmount 時寫全域 store，理論上會互相蓋。壓短
   `duration` 可以縮小這個窗口。
2. **AppBar 標題要去重**。RA 的 `<Title>` 用 `createPortal` 打進 `#react-admin-title`，
   疊層時兩頁各塞一個 span 進去，標題會串接（實測看到「我的邀請專案」）。portal 內容
   在我們的層外面、opacity 蓋不到，所以 `CrossfadeTransition` 附帶一條 GlobalStyles
   只顯示 `:last-child`（＝後掛的進場頁）。平時 portal 只有一個子節點，規則不作用。
3. **尾端有一格殘影**：離場動畫結束到 React 移除節點之間有一幀，舊層的 inline opacity
   被 motion 清掉而 revert 回 1。`transitionEnd` 與寫死 base opacity 都無效（motion 對
   正在動畫的屬性會忽略後來的靜態值）。實測那一幀舊內容只貢獻約 6%（壓在 0.94 的新層
   底下），前一幀是 0.6%，即 8ms 的 5% 亮度跳動，且 rAF 取樣在 paint 之前、未必真的
   被畫出來。判斷不值得為它加 hack。

---

## `cloud`：一次只疊一層，每層都要能單獨驗證

⚠️ **這條開發紀律是踩過坑換來的**：第一次做的時候一口氣改了三件事（自製程序化貼圖、
拿掉天空、改成橫向掃動），結果既不像 demo，也**完全無法定位是哪一段搞砸的**。
後來拆成一次一步才查得出來。之後要再往上疊（速度曲線、傾斜角、雲的高度…）也照這個做法。

| 步驟 | 內容 | 怎麼驗的 |
|---|---|---|
| 1 | **完整重現** `idea/cloud.html` + 淡入淡出 | 把 demo 與重現版的相機設到**同一組 z**，讀同一位置的像素逐點對照 |
| 2 | 疊上**橫向掃動**（相機沿 x 移動） | 把特定雲團投影到螢幕座標，看位移方向與視差比例 |

### 步驟 1 的驗證結果

| z | demo (y=5%/40%/80%) | 重現版 |
|---|---|---|
| 800 | (102,150,186) (134,182,212) (247,249,250) | **(102,150,186)** (139,184,212) (227,230,233) |
| 760 | (102,150,186) (134,182,212) (246,248,250) | **(102,150,186)** (168,197,217) (236,239,241) |
| 200 | (102,150,186) (134,182,212) (232,238,243) | **(102,150,186)** (134,182,212) (217,223,226) |

天空像素完全一致。中段差異來自 demo 與重現版都用 `Math.random()` 佈雲，每次載入雲團
位置就不同 —— 是隨機性，不是實作差異。

### 步驟 2 的驗證結果

⛔ **剛體互相關對這個場景無效**，別再試了：相機橫移時近雲位移大、遠雲位移小，畫面是
**變形**不是平移，而且雲會進出畫面。實測用整條掃描線互相關得到 0.25→+412、0.5→−500
（撞搜尋邊界）、0.75→+468 這種自相矛盾的結果；改成量「雲海地平線輪廓」殘差飆到
131693 —— 兩次都是**探針在說謊**，不是系統壞了。

有效的量法是把雲團投影到螢幕座標：

| 雲團 | 距相機 | p=0 螢幕x | p=1 螢幕x | 位移 |
|---|---|---|---|---|
| #40 | 740 | 263 | −985 | −1248 px |
| #300 | 480 | 1217 | −707 | −1924 px |
| #600 | 180 | 5541 | 409 | −5132 px |

全部往左、單調移動；位移比 5132/1248 = 4.11，距離比 740/180 = 4.11 —— **位移與距離成
反比**，符合透視定律，視差是真的。

`sweepHalf` 現在是 **73**（歷程：260 → 146 → 73，一路往下收，每次都是「移動幅度太大」）。
最初取 260 是為了跟 demo 自己的滑鼠視差同一個數量級（demo 是 ±視窗半寬×0.25，1600 寬時
約 ±200），但當成**轉場**看，那個行程過頭了。

⛔ 不要調太大：雲只鋪在 x ∈ [-500, 500]，掃過頭會把構圖甩出雲帶、畫面只剩藍天。
⚠️ 這個值與 `durationMs` 一起決定相機的**視覺速度**（= 2·sweepHalf / duration）。
兩者不必等比例改 —— 146 → 73 就是刻意只砍行程、不動時間。

**遮蔽不靠它**：換頁能被藏住是 backdrop 底色保證的（`maskCount` 預設 0，那團全遮雲已移除）。
實測砍半前後的全遮期分別是 364–925ms 與 332–895ms，都是 0% 文字像素。

#### ⭐ 淡出階段曾經凍結場景，現在拿掉了

以前 `opacity` 一離開平頂就 `curtain.stop()`，因為雲在半透明時持續變化會讓合成亮度非單調
（掃動尾段雲變稀 → 先偏亮、雲回來又變暗 → 「結尾閃一下」）。代價是**整段淡出畫面是靜止的**，
看起來不像「一邊移動一邊消失」。

`sweepHalf` 砍到 73 之後那個成因消失了：相機行程只剩一半，根本走不到雲變稀的尾段。
移除凍結後實測：淡出段（598–900ms）相機仍位移 **59.60 世界單位 / 35 幀**（佔全程 41%），
逐幀合成亮度的最大方向反轉只有 **0.20 階**（看得見閃爍約需 1.5 階以上）。

⚠️ 若之後把 `sweepHalf` 調回大值，**一定要重跑逐幀亮度量測**確認閃爍沒有回來。

---

### `cloud` 的兩個視覺旋鈕

雲是近白色的，**需要比它更暗的底才看得見**。這件事決定了下面兩個選項怎麼配：

| `backdrop` | 效果 | 遮蔽 | 適合 |
|---|---|---|---|
| `'theme'`（預設） | 頁面 `background.default` 壓暗 14%→6%（暗色主題則提亮）做成淡漸層 | ✅ canvas 不透明，換頁必定遮住（實測 min 1.000） | 想要「頁面淡入一片霧」，不要天空 |
| `'demo'` | demo 原本的藍天 `#1e4877 → #4584b4` | ✅ | 想要 demo 原汁原味 |
| `'none'` | 什麼都不鋪，只有雲疊在頁面上 | ⚠️ 遮不滿，實測換頁時 min **0.008**，抽換會被看見 | 只想要氣氛、不在意抽換 |

`blend` **只在 `backdrop='none'` 時生效**（有底色時 canvas 不透明，再套混合會把整片底色也拿去混）：
亮色主題 `multiply`（雲的亮部透明、暗部把頁面壓暗 → 雲影），暗色主題 `screen`（雲發光）。
實測亮色主題用 multiply 之後，內容區平均亮度變化 −10.64，雲才在純白底上讀得出來。

---

### `cloud` 的墨刷 logo 層（`inkLogo`，預設開）

疊在雲上的品牌過場：用 `idea/ink-brush-trail.html` 的刷毛 shader（逐字移植到
`inkBrush.ts`）沿 `logoStrokes.ts` 的筆畫「畫出」logo → 停一拍 → 尾巴以畫上去的相同
節奏**順著筆畫離場**。筆色：上方 `#949695`、下方那條 `#61619E`（定案色，非 SVG 原始的 #5E5493）。

- **離場不需要額外機制**：demo 的 trail life（點的年齡超過 life 就消失、接近時淡出）
  天生就是「順著筆畫消失」。把 life 設成 drawMs+holdMs，時間軸 = 畫上 0..0.42 →
  停留 ..0.58 → 尾巴追到 1.0，總長正好等於雲的 durationMs。
- **確定性**：demo 由滑鼠 + `performance.now()` 驅動；移植版改成 `setTime(t)`，
  出生時間在 resize 時算好、seed 全固定 —— 可 scrub、可量測、畫一萬次都一樣。
  （w/dry 的手感起伏原本來自滑鼠速度，改用固定 seed 雜訊合成。）
- **調形狀與手感**：`logoStrokes.ts` 的初始資料是自動描輪廓出來的（沿 SVG 填色輪廓每
  0.25 單位取樣 + RDP 簡化），要人眼調 —— **直接用瀏覽器開
  `tools/ink-stroke-editor.html`**（file:// 即可）：拖點 / 增刪節點（選點按 Delete）/
  反轉筆畫方向 / 逐筆顏色與筆寬，加上 demo 調墨面板的四個全域旋鈕
  （**墨量 / 毛數 / 筆寬倍率 / 餘韻**；餘韻設得比畫上時間短就是 demo 原始的
  邊畫邊消失拖尾）。匯出的 JSON 是 `{ params, strokes }` 兩塊：`strokes` 貼回
  `logoStrokes.ts` 的 `logoStrokes`、`params` 貼回同檔的 `logoInkParams`；
  單頁要臨時蓋掉再用 CloudTransition 的 `inkParams` prop。
- ⚠️ shader / rebuild / 時間模型在 `inkBrush.ts` 與編輯器裡各有一份（編輯器無 build
  環境只能複製），改一邊記得同步另一邊。

---

## ⛔ 走過的死路（別再走一次）

### `dispose()` 不等於移除

`dispose()` 只釋放 WebGL 資源，**canvas 還留在 DOM 裡**。曾經因為把 `backdropColors` 放進
建立雲幕那個 effect 的 deps，換主題就整個重建雲幕 —— 結果 host 裡疊了兩個 canvas，
上面那個是 context 已經 lost 的死畫布，暗色主題看起來就是「掛掉」。

兩層修法都留著：`dispose()` 會 `renderer.domElement.remove()`；而換底色一律走
`setBackdrop()` 就地更新，根本不重建場景。**改 deps 之前先想「這會不會觸發重建」。**


### 自己造雲

最早不是照搬，而是「參考 demo 重寫」：自製程序化貼圖、拿掉藍天、改成雲團橫向平移。
做出來是**一坨髒污飛過**，而且因為一次改了三件事，完全定位不出是哪一段搞砸的。
後來把原始 demo 跑起來當 ground truth 才看清楚：

> **demo 的層次感幾乎 100% 來自藍天。** 它的雲本身對比極低、就是一片近白的柔霧，
> 好看的是「白雲 vs 藍天」的對比，以及遠處雲融進藍霧的空氣透視。

拿掉天空就等於抽掉唯一的對比來源；再想讓雲「看得出形狀」就只能把雲調暗、加內部明暗，
那正是髒污的由來。所以現在的做法是**照搬**，不是重寫。

### 想靠調高 count 把雲補密

沒用。demo 的 `z = j` 讓 count 同時決定**走廊深度**，相機也退到 `z = count`，
單位體積的密度是不變的。實測 800 → 2400，遮蔽率幾乎沒改善（最好的那一列 0.075 → 0.154），
只是白白多三倍 draw 成本。已改回 demo 的 800。

### 想靠「中段放慢」做出全遮期

⛔ 試過 `0.5 + 0.5*(2t-1)^3` 這種兩端快、中段慢的曲線 —— 中點速度會掉到 0（實測 −0.0002），
看起來就是**動 → 停 → 動**的三段式，一眼就被挑出來。運鏡要單一條等速直線。
（同理，opacity 也不要用「淡入 / 停留 / 淡出」三段直線，那兩個轉角的二階差分尖峰倍率
實測 126.3；改成單一條 `smootherstep(clamp(sin(πt)·plateau))` 之後降到 3.7。）

---

## 版面上的鐵律

兩種動畫模式都**不用 `position: absolute`**。RA 的 Datagrid sticky header 與捲動容器
依賴正常文件流；絕對定位會讓層撐不出高度，長頁面捲不到底。`crossfade` 的兩層疊合改用
CSS grid（同一個 cell），既能重疊又保有正常的高度撐開行為。

wrapper 的 `flexGrow/flexBasis/minWidth`（`types.ts` 的 `containerBase`）是照抄
`.RaLayout-content`，目的是讓這一層對版面完全透明。

---

## 驗證

- 單元測試：`yarn sr rt-tenant-console tcadmin/tests RouteTransition.test.tsx`（`getRouteKey` 的收斂規則）
- 真瀏覽器：三個模式都用 puppeteer 實測過（真實點側欄 → 取樣 opacity/transform 曲線 →
  截圖確認）。`crossfade` 的關鍵斷言是「疊合期間離場層全程顯示**舊**內容」，這是它與
  「新內容淡出又淡入」的分界。
