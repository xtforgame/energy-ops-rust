/**
 * 台灣 3D 地圖 —— 場景組裝與主迴圈。
 * 對應 Demo2 `map/index.tsx` + `lights.tsx`，以及 html 版 §6 / §8 / §18 / §19 / §21。
 *
 * 移植與白底改色的完整對照見 `../README.md`。
 */
import * as THREE from 'three';
import topo from '../data/taiwan-country.topo.json';
import { buildGeo } from './geo';
import { createTextures, disposeTextures } from './textures';
import { Reveal, Resources } from './reveal';
import { buildCounties, updateCounties, type County } from './counties';
import { buildBoundary } from './boundary';
import { buildTrail, updateTrail } from './trail';
import { buildMarkers } from './markers';
import { buildStations, type StationPoint } from './stations';
import { buildFlyLines } from './flyLines';
import { buildBottomRing, updateBottomRing } from './bottomRing';
import { buildFloor } from './floor';
import { buildBeams, updateBeams } from './beams';
import { createControls } from './controls';
import { createLabels } from './labels';
import type { BuildCtx } from './ctx';
import type { MapPalette, MapThemeName } from './palette';
import { mapThemes, DEFAULT_MAP_THEME } from './palette';
import { color } from './glColor';

/** 原作 §1 的 CONF，數值原封不動（只有顏色移到 palette.ts） */
const CONF = {
  /** 本島長邊對應的地圖單位 */
  MAP_SIZE: 10.6,
  /** 擠出厚度 */
  DEPTH: 0.9,
  /** 對應 base.tsx 的 scale 0.5 */
  ROOT_SCALE: 0.5,
};

export interface HoverInfo {
  name: string;
  id: string;
  area: number;
}

/** 給頁面畫按鈕用的縣市清單（不含幾何，頁面那邊不需要也不該碰） */
export interface RegionRef {
  id: string;
  name: string;
  area: number;
}

export interface TaiwanMapOptions {
  /**
   * ⭐ 真實案場點位（C5）。⛔ 空陣列／不給 ＝ 不建這一層（連 draw call 都省）。
   *
   * ⚠️ 這一層是**資料**，跟縣市標記（取景的一部分）不同 —— 見 `stations.ts` 檔頭。
   */
  stations?: StationPoint[];
  /**
   * 鏡頭停在多遠（進場動畫的終點）。預設 **11.5** ＝ 滿版背景的取景。
   *
   * ⚠️ 當地圖被塞進一塊**面板**（例如 Dashboard 的 460px 高卡片）時，
   *    11.5 會讓本島只佔畫面一小塊、點位擠成一團看不出來。
   *    ⭐ 面板請調小（實測 8 左右剛好），⛔ 但不要動這個預設值。
   */
  restingDistance?: number;
  /**
   * 案場層建好後回拋實際畫了幾個點 / 略過幾個（沒有座標的）。
   * ⭐ 這是頁面唯一能斷言「資料真的畫出來了」的管道 —— WebGL 裡面讀不回來。
   */
  onStations?: (info: { count: number; skipped: number }) => void;
  /**
   * ⭐ 點到某個案場點。
   * ⚠️ 只在**沒有拖曳**時觸發 —— ⛔ 拖完放開也會發 `click`，
   *    不擋的話使用者每轉一次地圖就被導走一次。
   */
  onStationClick?: (stationId: number) => void;
  /** 配色主題。'jv' = 白底 + 紫（JV 1C）；'lab' = futuristic-white 的白色色階 */
  theme?: MapThemeName;
  onHover?: (info: HoverInfo | null) => void;
  /** 場景建好後回拋縣市清單 —— 頁面靠它畫特寫按鈕，不必自己再解一次 topojson */
  onRegions?: (regions: RegionRef[]) => void;
  /** 特寫狀態變動（含使用者自己拖曳鏡頭而被動解除的情況） */
  onFocusChange?: (id: string | null) => void;
  /**
   * 第一幀畫完就叫 —— 載入遮罩要在這裡收掉，不要等 `onReady`。
   * 進場動畫本身就要好幾秒（鏡頭俯衝），遮罩蓋到 onReady 等於把動畫藏起來，
   * 使用者只會看到「讀很久」。實測（未縮短前）：載入 731ms、動畫 3077ms ——
   * 真正在等資源的只有前面那 0.7 秒。
   */
  onFirstFrame?: () => void;
  /** 進場動畫跑完、可互動（原作 §19 的 `revealed`）—— 用來淡入 HUD 讀數面板 */
  onReady?: () => void;
}

export interface TaiwanMapHandle {
  /** 特寫到某個縣市；傳 null 回到背景模式 */
  focus(id: string | null): void;
  /**
   * ⭐ 某個案場點現在在**畫面上的哪裡**（CSS 像素，相對 container）。
   *
   * ⚠️ 這是**量測 API**：給 crawler 算出「該點哪裡」用 ——
   *    ⛔ 不是驅動 API，點擊一律走真的滑鼠事件（`/browser-verify` §1）。
   *    看不到（在鏡頭後方或還沒建好）回 null。
   */
  screenPositionOf(stationId: number): { x: number; y: number } | null;
  dispose(): void;
}

export const createTaiwanMap = (container: HTMLElement, opts: TaiwanMapOptions = {}): TaiwanMapHandle => {
  const pal = mapThemes[opts.theme ?? DEFAULT_MAP_THEME];
  /**
   * 特寫時整組 morph 過去的配色。使用者定的規則：**「MONO 選取時就套用 VIOLET 那些設定」**。
   * VIOLET 指向自己 ⇒ 不 morph。
   *
   * ⛔ 不要在這裡做「只覆寫某幾層」的折衷 —— 試過一次，結果是一半 MONO 一半 VIOLET，
   *    看起來不上不下。要嘛整組換過去，要嘛不換。
   */
  const focusPal: MapPalette = mapThemes[pal.name === 'lab' ? 'jv' : pal.name];
  const model = buildGeo(topo, CONF.MAP_SIZE);
  const tex = createTextures(pal);
  const reveal = new Reveal();
  const res = new Resources();
  const ctx: BuildCtx = { pal, focusPal, tex, reveal, res, depth: CONF.DEPTH };

  /* ---------- 場景 / renderer（§6） ---------- */
  // alpha:true —— 讓卡片的白底直接透上來，霧色也設成同一個白，遠處自然溶進頁面。
  const renderer = new THREE.WebGLRenderer({ antialias: true, alpha: true, powerPreference: 'high-performance' });
  renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2));
  renderer.setClearAlpha(0);
  renderer.outputColorSpace = THREE.SRGBColorSpace;
  renderer.domElement.style.display = 'block';
  renderer.domElement.style.width = '100%';
  renderer.domElement.style.height = '100%';
  renderer.domElement.style.touchAction = 'none';
  container.appendChild(renderer.domElement);

  const scene = new THREE.Scene();
  scene.fog = new THREE.Fog(color(pal.fog), pal.fogNear, pal.fogFar);

  const size = () => {
    const r = container.getBoundingClientRect();
    return { w: Math.max(1, Math.round(r.width)), h: Math.max(1, Math.round(r.height)) };
  };
  let { w, h } = size();
  renderer.setSize(w, h, false);

  const camera = new THREE.PerspectiveCamera(52, w / h, 0.1, 400);

  /* ---------- 燈光（lights.tsx） ----------
     白底的曝光基準跟黑底完全不同：原作 ambient 1.9 + dir 2.6 是為了把深色地表打亮，
     照搬到白底會整片死白。這裡降到剛好讓頂面的墨格看得見的程度。 */
  scene.add(new THREE.AmbientLight(0xffffff, pal.lights.ambient));
  const dir = new THREE.DirectionalLight(0xffffff, pal.lights.dir);
  dir.position.set(6, 22, -12);
  scene.add(dir);
  const fill = new THREE.DirectionalLight(color(pal.bgInkSoft), pal.lights.fill);
  fill.position.set(-14, 8, 14);
  scene.add(fill);

  /* ---------- 根節點（§8，對應 base.tsx 的 <Center><group rotation=-90°>） ---------- */
  const world = new THREE.Group();
  const mapRoot = new THREE.Group();
  mapRoot.rotation.x = -Math.PI / 2;
  mapRoot.scale.setScalar(CONF.ROOT_SCALE);
  mapRoot.position.y = 0.02;
  const mapGroup = new THREE.Group(); // 進場動畫層（scale.z 0 → 1）
  mapGroup.scale.z = 0;
  mapRoot.add(mapGroup);
  world.add(mapRoot);
  scene.add(world);

  /* ---------- 各層 ---------- */
  const { counties, pickables } = buildCounties(mapGroup, model, ctx);
  const { coastMaterial } = buildBoundary(mapGroup, model, ctx);
  // 關掉的層直接不建 —— 不是建好再設透明，連每幀的更新與 draw call 都省掉。
  // 判準是 `pal.layers` 而不是 alpha：指標平時 alpha 也是 0，但特寫要浮出來（見 palette）。
  const trail = pal.layers.trail ? buildTrail(mapGroup, model, ctx) : null;
  buildMarkers(counties, ctx);
  // ⭐ 案場點位掛在 `mapGroup`（進場動畫那一層），才會跟著地圖一起升起來。
  const stationLayer = (opts.stations && opts.stations.length)
    ? buildStations(mapGroup, opts.stations, CONF.MAP_SIZE, ctx)
    : null;
  if (stationLayer) opts.onStations?.({ count: stationLayer.count, skipped: stationLayer.skipped });
  const fly = pal.layers.flyLines ? buildFlyLines(mapGroup, counties, ctx) : null;
  const rings = pal.layers.rings ? buildBottomRing(scene, ctx) : null;
  buildFloor(scene, ctx);
  const beams = pal.layers.beams ? buildBeams(scene, ctx) : null;
  const trailInk = color(pal.bgInk);

  const labels = createLabels(container, counties, CONF.DEPTH, pal.labelLift, pal.labelsAlwaysOn);
  const controls = createControls(renderer.domElement, camera);

  opts.onRegions?.(model.regions.map((r) => ({ id: r.id, name: r.name, area: r.area })));

  /* ---------- Hover 拾取（§18） ---------- */
  const ray = new THREE.Raycaster();
  let hovered: County | null = null;

  /**
   * ⭐ 案場點的拾取 —— **獨立於縣市**（各自 raycast 自己的陣列）。
   * ⛔ 混進 `pickables` 的話案場會把縣市 hover 整個吃掉。
   */
  const pickStation = (): number | null => {
    const layer = stationLayer;
    if (!layer || !layer.pickables.length || !controls.pointer.has) return null;
    ray.setFromCamera(new THREE.Vector2(controls.pointer.x, controls.pointer.y), camera);
    const hit = ray.intersectObjects(layer.pickables, false)[0];
    const id = hit?.object?.userData?.stationId;
    return typeof id === 'number' ? id : null;
  };

  const pick = () => {
    if (!pal.hoverEnabled || !controls.pointer.has || controls.lock) return;
    ray.setFromCamera(new THREE.Vector2(controls.pointer.x, controls.pointer.y), camera);
    // ⭐ 案場優先：它畫在縣市之上，游標在點上時就該是「點得動」的手勢。
    if (pickStation() != null) {
      renderer.domElement.style.cursor = 'pointer';
      return;
    }
    const hit = ray.intersectObjects(pickables, false)[0];
    const c = (hit ? (hit.object.userData.county as County) : null) ?? null;
    if (c === hovered) return;
    hovered = c;
    if (c) {
      renderer.domElement.style.cursor = 'pointer';
      opts.onHover?.({ name: c.region.name, id: c.region.id, area: c.region.area });
    } else {
      renderer.domElement.style.cursor = 'default';
      opts.onHover?.(null);
    }
  };

  const clearHover = () => {
    if (!hovered) return;
    hovered = null;
    opts.onHover?.(null);
  };
  if (pal.hoverEnabled) renderer.domElement.addEventListener('pointerleave', clearHover);

  /* ---------- ⭐ 案場點：點擊 ---------- */
  // ⚠️ 用「按下與放開的位移」判斷是不是點擊 —— ⛔ 拖曳結束也會發 `click`。
  let downX = 0;
  let downY = 0;
  const TAP_SLOP = 5; // px
  const onCanvasDown = (e: PointerEvent) => { downX = e.clientX; downY = e.clientY; };
  const onCanvasClick = (e: MouseEvent) => {
    if (!opts.onStationClick) return;
    if (Math.hypot(e.clientX - downX, e.clientY - downY) > TAP_SLOP) return;
    const id = pickStation();
    if (id != null) opts.onStationClick(id);
  };
  renderer.domElement.addEventListener('pointerdown', onCanvasDown);
  renderer.domElement.addEventListener('click', onCanvasClick);

  /* ---------- 特寫 ----------
     點某個縣市 → 鏡頭滑到它上方，同時整座島的景深從中景拉回前景（＝恢復正常色彩）。
     解除 → 鏡頭回預設位、景深退回背景模式。 */
  const FOCUS_MS = 1100;
  let focused: County | null = null;
  /** 0 = 背景模式、1 = 特寫（每幀朝目標平滑推進，跟鏡頭同步變化） */
  let boost = 0;
  const bgColor = color(pal.bg);
  const coastColor = color(pal.coast);
  // 進場期間 Reveal 只推 intro；特寫維度在 revealed 之後才由主迴圈接手
  const scratch = new THREE.Vector3();

  const focus = (id: string | null) => {
    const next = id ? counties.find((c) => c.region.id === id) ?? null : null;
    if (next === focused) return;
    focused = next;

    if (!next) {
      // 解除特寫：回總覽位置，並在運鏡結束後恢復自轉
      controls.flyTo(
        new THREE.Vector3(0, 0.45, 0),
        { r: CAM_TO.r, phi: CAM_TO.phi, theta: controls.sph.theta },
        FOCUS_MS,
        true,
      );
      opts.onFocusChange?.(null);
      return;
    }

    // 目標點：縣市中心的頂面，換算成世界座標（含 mapRoot 的旋轉/縮放）
    scratch.set(next.region.center.x, next.region.center.y, CONF.DEPTH);
    next.group.localToWorld(scratch);

    // 距離照縣市大小算：讓它的外接半徑約等於垂直半視野（＝縣市長邊大致填滿整個畫面高）
    const worldRadius = next.region.radius * CONF.ROOT_SCALE;
    const halfFov = (camera.fov * Math.PI) / 360;
    const r = Math.min(controls.max, Math.max(controls.min, worldRadius / Math.tan(halfFov) / 1.05));

    // 方位角：把鏡頭擺到「島心 → 該縣市」的延長線上，縣市因此落在前景、其餘島身在後 ——
    // 每個縣市的方位都不同，所以運鏡自然帶著一段轉身。太靠島心的縣市方向不穩，維持原角度。
    const dx = scratch.x;
    const dz = scratch.z;
    const spread = Math.hypot(dx, dz);
    const theta = spread > 0.35 ? Math.atan2(dx, dz) : controls.sph.theta;

    // 俯角：phi 是從 +Y 軸量的，越小越接近正上方。1.12 太平（幾乎是側視、只看得到山壁），
    // 收到 0.82 —— 還看得出體塊的厚度，但主要在看形狀。
    controls.flyTo(scratch.clone(), { r, phi: 0.82, theta }, FOCUS_MS);
    opts.onFocusChange?.(next.region.id);
  };

  /* ---------- 進場動畫（§19，gsap timeline 的等價實作） ---------- */
  const easeOutCirc = (t: number) => Math.sqrt(1 - Math.pow(t - 1, 2));
  const easeOutCubic = (t: number) => 1 - Math.pow(1 - t, 3);
  /**
   * 進場時間軸的縮放。原作是滿版 showcase，3.6 秒的鏡頭俯衝很合理；
   * 這裡是後台的一個分頁，同樣的編排壓到 0.6 倍（總長 2.16s、可互動 1.86s）。
   * 只縮時間，各段的比例與 easing 完全照原作。
   */
  const INTRO = 0.6;
  const CAM_FROM = { r: 30, phi: 0.24 };
  // 原作 r=11.2 配 16:9 滿版視窗。這頁改成滿版背景後 canvas 約 1.3:1（更高），
  // 垂直 FOV 固定 52° ⇒ 同樣距離島會佔到 62% 畫面高、幾乎沒有留白 —— 退到 11.5
  // （島佔約 47%），讓大氣有地方可以呼吸。
  //
  // ⭐ C5：**面板**用的取景比滿版背景近（`opts.restingDistance`）。
  //    ⚠️ 預設維持 11.5 —— ⛔ 不要改動既有 `#/taiwan-map` 那頁的畫面。
  const CAM_TO = { r: opts.restingDistance ?? 11.5, phi: 0.92 };
  let T = 0;
  let revealed = false;

  const intro = (t: number) => {
    // 0 ~ 2.6s 鏡頭俯衝
    const a = Math.min(1, t / (2.6 * INTRO));
    const ea = easeOutCirc(a);
    controls.goal.r = CAM_FROM.r + (CAM_TO.r - CAM_FROM.r) * ea;
    controls.goal.phi = CAM_FROM.phi + (CAM_TO.phi - CAM_FROM.phi) * ea;
    controls.goal.theta = -0.55 + 0.55 * ea;

    // 2.2s 起：地圖長高 + 全體淡入
    const b = Math.max(0, Math.min(1, (t - 2.2 * INTRO) / (1.3 * INTRO)));
    const eb = easeOutCubic(b);
    mapGroup.scale.z = eb;
    reveal.apply(eb);

    if (t > 3.1 * INTRO && !revealed) {
      revealed = true;
      controls.lock = false;
      labels.reveal();
      opts.onReady?.();
    }
  };

  /* ---------- resize ----------
     ⚠️ `setSize` 會無條件寫 canvas.width/height，等於清空 drawing buffer。
        ResizeObserver 在版面穩定時也會叫，所以尺寸沒變一定要直接 return。 */
  const applySize = () => {
    const s = size();
    if (s.w === w && s.h === h) return;
    w = s.w;
    h = s.h;
    camera.aspect = w / h;
    camera.updateProjectionMatrix();
    renderer.setSize(w, h, false);
  };
  const ro = new ResizeObserver(applySize);
  ro.observe(container);

  /* ---------- 主迴圈（§21） ---------- */
  const clock = new THREE.Clock();
  let raf = 0;
  let firstFrame = true;
  const frame = () => {
    raf = requestAnimationFrame(frame);
    const dt = Math.min(clock.getDelta(), 0.05);
    T += dt;

    if (T < 3.6 * INTRO) intro(T);
    controls.damp(dt, revealed);
    pick();

    // 景深跟著特寫狀態平滑進出。用跟鏡頭 tween 相近的速率，兩者看起來是同一個動作。
    const wantBoost = focused ? 1 : 0;
    boost += (wantBoost - boost) * Math.min(1, dt * 3.4);
    const islandDepth = pal.focusDepth.island + (1 - pal.focusDepth.island) * boost;
    if (revealed) {
      // 進場已結束 ⇒ intro 固定 1，只推特寫這一維
      reveal.apply(1, boost);
      (coastMaterial as THREE.LineBasicMaterial).color.copy(bgColor).lerp(coastColor, islandDepth);
    }

    updateCounties(counties, hovered, focused, dt, revealed, pal, islandDepth);
    if (trail) updateTrail(trail, dt, trailInk);
    if (fly) fly.texture.offset.x -= dt * 0.26; // 飛線流光
    if (rings) updateBottomRing(rings, dt, pal.ringSpin);
    if (stationLayer) stationLayer.update(T);
    if (beams) updateBeams(beams, dt);

    // 相機狀態掛到 host 上（唯讀的量測掛點，一幀一次 dataset 寫入）。
    // ⚠️ 這個必須**全程**寫，不能只在運鏡時寫：驗「自轉有沒有恢復」得直接讀 theta ——
    //    用「連拍兩張比對變動像素」量不到，那個數字會被菱錐的浮動旋轉與掃描光帶蓋過
    //    （實測特寫中 7.4% > 總覽 2.7%，方向完全相反）。
    if (revealed) {
      container.dataset.cam = `${controls.sph.r.toFixed(4)},${controls.sph.phi.toFixed(4)},${controls.sph.theta.toFixed(4)},${controls.target.x.toFixed(4)},${controls.target.y.toFixed(4)},${controls.target.z.toFixed(4)}`;
    }

    if (revealed) labels.update(camera, hovered, focused, w, h);
    renderer.render(scene, camera);

    if (firstFrame) {
      firstFrame = false;
      opts.onFirstFrame?.();
    }
  };

  controls.apply();
  frame();

  return {
    focus,

    /**
     * ⭐ 把案場點投影到畫面座標（CSS 像素，相對 container 左上角）。
     *
     * ⚠️ **量測用**（crawler 要知道「該點哪裡」）——
     *    ⛔ 不要拿它當驅動：點擊一律走真的滑鼠事件。
     * ⚠️ `z > 1` ＝ 在鏡頭後面 → 回 null，⛔ 不要回一個翻轉過的假座標。
     */
    screenPositionOf(stationId: number) {
      const world = stationLayer?.positionOf(stationId);
      if (!world) return null;
      const v = world.clone().project(camera);
      if (v.z > 1) return null;
      const rect = renderer.domElement.getBoundingClientRect();
      return {
        x: ((v.x + 1) / 2) * rect.width,
        y: ((1 - v.y) / 2) * rect.height,
      };
    },

    dispose() {
      cancelAnimationFrame(raf);
      ro.disconnect();
      controls.dispose();
      renderer.domElement.removeEventListener('pointerdown', onCanvasDown);
      renderer.domElement.removeEventListener('click', onCanvasClick);
      if (pal.hoverEnabled) renderer.domElement.removeEventListener('pointerleave', clearHover);
      labels.dispose();
      res.dispose();
      disposeTextures(tex);
      renderer.dispose();
      // ⚠️ dispose() 不會把 canvas 從 DOM 拿掉 —— 漏掉就會疊出第二張、
      //    上面那張直接 context lost（切頁雲霧那次實測過）。
      renderer.domElement.remove();
    },
  };
};
