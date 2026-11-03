/**
 * 發電站點位層（C5）—— 把**真實案場**標在地圖上。
 *
 * ⚠️ 這一層跟 `markers.ts` 是**兩件事**：
 *   - `markers.ts` 標的是**縣市**（座標來自 topojson 的縣市中心，22 個，固定的）
 *   - 這一層標的是**任意經緯度的點**（案場，數量與位置都來自 API）
 *   ⛔ 不要把兩者合併 —— 縣市標記是取景的一部分，案場是資料。
 *
 * ⭐ 投影一律走 `geo.ts` 的 `projectLonLat`，那是縣市邊界用的**同一個**麥卡托投影。
 *    ⛔⛔ 不要自己做「查台灣頭尾經緯度→線性內插」的校準表：麥卡托的 y 對緯度是
 *    `log(tan(π/4 + φ/2))`，線性內插最大誤差 ≈ 全圖高度 0.337%（實地 **1.33 km**），
 *    而且改 `FRAME`/`mapSize` 時會**默默失準**。
 *
 * ⛔ 色票一律從 `jvTokens` 拿；異常用**燒橘**（`warn`），⛔ 不是紅。
 */
import * as THREE from 'three';
import { jvTokens } from '../../themes/jvTheme';
import { projectLonLat } from './geo';
import type { BuildCtx } from './ctx';
import { color } from './glColor';

/** 一個案場點位。⚠️ 經緯度是 `number` —— 呼叫端負責把 DECIMAL 字串轉好。 */
export interface StationPoint {
  id: number;
  name: string;
  /** construction | operating | abnormal */
  status: string;
  latitude: number;
  longitude: number;
}

/**
 * 狀態 → 顏色。⛔ 異常是**燒橘**不是紅 —— 紅在這個設計系統裡沒有位置。
 * ⚠️ 未知狀態落到 `muted`（灰），⛔ 不要 fallback 到 operating 的綠：
 *    把不知道的東西畫成「正常」是最糟的預設。
 */
const STATUS_COLOR: Record<string, string> = {
  operating: jvTokens.ok,
  construction: jvTokens.gold,
  abnormal: jvTokens.warn,
};
const statusColor = (s: string) => STATUS_COLOR[s] ?? jvTokens.muted;

/**
 * ⭐⭐ 案場點位**自己的**不透明度，⛔ 不吃 `pal.alpha.cone` / `pal.alpha.halo`。
 *
 * ⚠️ 為什麼：MONO（`lab`）主題把縣市標記的 alpha 設成 **0** —— 那是**刻意**的
 *    （它是背景圖層，標記只在被選中的縣市才出現）。案場點位是**資料**不是裝飾，
 *    跟著那組 alpha 走的話會「點都建好了、計數也對，但畫面上一個都看不到」——
 *    而且**不會有任何錯誤**。這個坑真的踩到了（C5 第一次截圖整張是空的）。
 */
const STATION_ALPHA = { pin: 0.95, halo: 0.5 };

export interface StationLayer {
  /** 每幀呼叫：讓點位輕微上下浮動（跟縣市標記同一種語彙）。 */
  update: (t: number) => void;
  /**
   * ⭐ 可被點的案場 mesh（`userData.stationId` 帶著 id）。
   *
   * ⚠️ 這是**獨立的一組** —— 縣市 hover/特寫用的是 `createScene` 自己的
   *    `pickables`（只裝縣市）。⛔ 兩邊不要混，混了案場就會把縣市 hover 吃掉。
   */
  pickables: THREE.Object3D[];
  /** 案場 id → 它在世界座標的位置（⭐ 給「投影到畫面」用）。 */
  positionOf: (id: number) => THREE.Vector3 | null;
  /** 場景裡實際建了幾個點 —— ⭐ 呼叫端拿它斷言「資料真的畫出來了」。 */
  count: number;
  /** 被略過的（沒有座標的）案場數，⛔ 不要靜靜吞掉。 */
  skipped: number;
}

/**
 * @param parent   `mapGroup`（進場動畫那一層）—— 這樣案場會跟著地圖一起升起來
 * @param mapSize  `CONF.MAP_SIZE`，投影要用
 */
export const buildStations = (
  parent: THREE.Object3D,
  stations: StationPoint[],
  mapSize: number,
  ctx: BuildCtx,
): StationLayer => {
  // ⛔ 刻意**不**解構 `pal` —— 這一層的可見度全部自己決定（見 `STATION_ALPHA`）。
  const { tex, reveal, res, depth } = ctx;

  // ⚠️ 沒有座標的案場**直接略過**，⛔ 不要標在 (0,0) —— 那會在非洲外海出現一根柱子，
  //    而且看起來像個真的案場。
  const usable = stations.filter(
    (s) => Number.isFinite(s.latitude) && Number.isFinite(s.longitude)
      && !(s.latitude === 0 && s.longitude === 0),
  );

  const group = new THREE.Group();
  group.name = 'stations';
  parent.add(group);

  // ⚠️ 尺寸**不吃 `pal.markerScale`**（MONO 是 0.38，小到看不見）。
  //
  // ⭐ MONO（正式版本）刻意把縣市標記藏起來 ⇒ **案場點位是圖上唯一的標記**，
  //    所以它不必為了跟縣市錐爭版面而縮小，該給到看得清楚的尺寸。
  //    （實測：0.62 在灰底上小到只剩 0.012% 的彩度像素。）
  const k = 1.15;
  const pinGeo = res.add(new THREE.ConeGeometry(0.1 * k, 0.34 * k, 6));
  const haloGeo = res.add(new THREE.PlaneGeometry(0.42 * k, 0.42 * k));

  // 同一個狀態共用材質（⛔ 不要每個點都建一個 material）。
  const matByStatus = new Map<string, THREE.MeshBasicMaterial>();
  const haloByStatus = new Map<string, THREE.MeshBasicMaterial>();
  const matFor = (status: string) => {
    if (!matByStatus.has(status)) {
      matByStatus.set(
        status,
        reveal.track(
          res.add(new THREE.MeshBasicMaterial({
            color: color(statusColor(status)),
            side: THREE.DoubleSide,
            transparent: true,
            depthWrite: false,
            fog: false,
          })),
          STATION_ALPHA.pin,
          STATION_ALPHA.pin,
        ),
      );
    }
    return matByStatus.get(status)!;
  };
  const haloFor = (status: string) => {
    if (!haloByStatus.has(status)) {
      haloByStatus.set(
        status,
        reveal.track(
          res.add(new THREE.MeshBasicMaterial({
            color: color(statusColor(status)),
            alphaMap: tex.halo,
            transparent: true,
            depthWrite: false,
            depthTest: false,
            fog: false,
          })),
          STATION_ALPHA.halo,
          STATION_ALPHA.halo,
        ),
      );
    }
    return haloByStatus.get(status)!;
  };

  const pins: Array<{ mesh: THREE.Mesh; base: number; ph: number }> = [];

  usable.forEach((s, i) => {
    const [x, y] = projectLonLat(s.longitude, s.latitude, mapSize);

    const pin = new THREE.Mesh(pinGeo, matFor(s.status));
    pin.rotation.x = -Math.PI / 2; // 尖端朝 +z（世界的上）
    // 站在縣市標記之上一點，⛔ 不要跟它們打架。
    const base = depth + 0.5 * k;
    pin.position.set(x, y, base);
    // ⭐ 案場點**要能被點**（C7d）。
    //    ⚠️ 原本這裡是 `pin.raycast = () => null` —— 那讓整層變成純裝飾。
    //    ⛔ 不會影響縣市：縣市 hover 走的是另一個只裝縣市 mesh 的陣列。
    pin.userData.stationId = s.id;
    pin.userData.stationName = s.name;
    pin.renderOrder = 8;
    pin.name = `station-${s.id}`;
    group.add(pin);
    // 相位錯開，⛔ 不要讓所有點同步跳動（那看起來像 bug 不像呼吸）。
    pins.push({ mesh: pin, base, ph: (i * 1.7) % (Math.PI * 2) });

    const halo = new THREE.Mesh(haloGeo, haloFor(s.status));
    halo.position.set(x, y, depth + 0.03);
    // ⛔ 光暈是裝飾，**保持不可點** —— 它比 pin 大得多，
    //    開放它會讓相鄰案場互相搶點擊。
    halo.raycast = () => null;
    halo.renderOrder = 9;
    halo.rotation.z = i * 0.9;
    group.add(halo);
  });

  return {
    count: pins.length,
    skipped: stations.length - usable.length,
    pickables: pins.map((p) => p.mesh),
    positionOf: (id: number) => {
      const hit = pins.find((p) => p.mesh.userData.stationId === id);
      if (!hit) return null;
      // ⚠️ 回**世界座標**（`mapGroup` 有進場動畫的位移／縮放）——
      //    ⛔ 回 local 座標的話投影會落在錯的地方。
      return hit.mesh.getWorldPosition(new THREE.Vector3());
    },
    update: (t: number) => {
      for (const p of pins) {
        // ⚠️ 幅度也不吃 `pal.markerBob`（MONO 是 0.05，幾乎不動）——同上，這是資料層。
        p.mesh.position.z = p.base + Math.sin(t * 1.6 + p.ph) * 0.12;
      }
    },
  };
};
