/**
 * TopoJSON → 平面座標。對應 `temp/taiwan-scifi-map.html` §2 ~ §4。
 *
 * 這一層刻意**不 import three**：純資料轉換，可以單獨跑測試（見 tests/taiwanGeo.test.ts）。
 * 原作用 `THREE.Box2 / THREE.Vector2`，這裡換成純物件，數學完全一樣。
 */

export type Pt = [number, number];

export interface Poly {
  outer: Pt[];
  holes: Pt[][];
}

export interface Region {
  id: string;
  name: string;
  polys: Poly[];
  /** 面積最大的環的形心（標記/標籤的落點） */
  center: { x: number; y: number };
  /** 該環的絕對面積（投影後的地圖單位平方） */
  area: number;
  /**
   * 從 `center` 到**面積最大那一環**最遠一點的距離（地圖局部單位）。
   * 特寫運鏡靠它決定鏡頭要退多遠 —— 寫死距離的話大縣市會被切掉、小縣市會太空。
   *
   * ⚠️ 只能算主環，不能掃過所有 polygon：臺東縣含蘭嶼綠島、宜蘭縣含龜山島與釣魚臺、
   *    屏東含小琉球 —— 掃全部的話半徑會被離島灌大好幾倍，鏡頭直接退到看不出特寫。
   */
  radius: number;
}

export interface Bbox {
  minX: number;
  minY: number;
  maxX: number;
  maxY: number;
}

export interface GeoModel {
  regions: Region[];
  /** 只被用到一次的 arc 接起來 = 海岸線（外輪廓），由長到短 */
  outerRings: Pt[][];
  /** 全圖 bbox，給 shape.tsx 的 UV 重映射用 */
  bbox: Bbox;
}

/* ---------- TopoJSON arc 解碼（delta → 絕對經緯度） ---------- */
const decodeArcs = (t: any): Pt[][] => {
  const tr = t.transform;
  return t.arcs.map((arc: number[][]) => {
    let x = 0;
    let y = 0;
    return arc.map((d) => {
      if (tr) {
        x += d[0];
        y += d[1];
        return [x * tr.scale[0] + tr.translate[0], y * tr.scale[1] + tr.translate[1]] as Pt;
      }
      return [d[0], d[1]] as Pt;
    });
  });
};

/* ---------- 麥卡托投影 ---------- */
const D2R = Math.PI / 180;
const merc = (lon: number, lat: number): Pt => [
  lon * D2R,
  Math.log(Math.tan(Math.PI / 4 + (Math.max(-85, Math.min(85, lat)) * D2R) / 2)),
];

/**
 * 以「本島」為構圖基準置中；澎湖 / 金門 / 馬祖仍依真實相對位置渲染
 * （所以離島會落在畫面外圍，這是原作的取景，不是 bug）。
 */
const FRAME = { lon: [119.95, 122.05], lat: [21.85, 25.38] };

/**
 * 取景參數：把麥卡托座標對到場景的 local 座標。
 * 只跟 `FRAME` 與 `mapSize` 有關，**與 topojson 內容無關** —— 所以可以獨立算。
 */
const frameOf = (mapSize: number) => {
  const f0 = merc(FRAME.lon[0], FRAME.lat[0]);
  const f1 = merc(FRAME.lon[1], FRAME.lat[1]);
  return {
    scale: mapSize / Math.max(f1[0] - f0[0], f1[1] - f0[1]),
    cx: (f0[0] + f1[0]) / 2,
    cy: (f0[1] + f1[1]) / 2,
  };
};

/**
 * ⭐ **經緯度 → 場景 local 座標**（`local +x = 東`、`local +y = 北`）。
 *
 * 這就是 `buildGeo` 畫縣市邊界時用的**同一個投影**，只是抽成純函式，
 * 讓「任意一個點」（例如發電站）也能落在正確位置上。
 *
 * ⛔⛔ **不要用「查台灣頭尾經緯度做線性內插校準表」的做法**：
 *    麥卡托的 y 對緯度是 `log(tan(π/4 + φ/2))`，是**非線性**的——
 *    實算過線性內插最大誤差是全圖高度的 **0.337%（≈ 實地 1.33 km）**。
 *    更糟的是，對照表在有人改 `FRAME` 或 `mapSize` 時會**默默失準**
 *    （不會報錯、只是點慢慢跑掉），而公式會自動跟著改。
 */
export const projectLonLat = (lon: number, lat: number, mapSize: number): Pt => {
  const { scale, cx, cy } = frameOf(mapSize);
  const m = merc(lon, lat);
  return [(m[0] - cx) * scale, (m[1] - cy) * scale];
};

export const buildGeo = (topo: any, mapSize: number): GeoModel => {
  const ARCS = decodeArcs(topo);
  const OBJ = topo.objects[Object.keys(topo.objects)[0]];

  const arcPts = (i: number) => (i < 0 ? ARCS[~i].slice().reverse() : ARCS[i]);

  /** 一圈 = 多段 arc 接起來（銜接點去重） */
  const ringCoords = (ring: number[]): Pt[] => {
    const out: Pt[] = [];
    for (const i of ring) {
      const p = arcPts(i);
      for (let k = out.length ? 1 : 0; k < p.length; k++) out.push(p[k]);
    }
    return out;
  };

  const polygonsOf = (g: any): Poly[] => {
    const src = g.type === 'Polygon' ? [g.arcs] : g.arcs;
    return src.map((rings: number[][]) => ({
      outer: ringCoords(rings[0]),
      holes: rings.slice(1).map(ringCoords),
    }));
  };

  const features = OBJ.geometries.map((g: any) => ({
    id: g.properties?.id ?? g.properties?.COUNTYCODE ?? '',
    name: g.properties?.name ?? g.properties?.COUNTYNAME ?? g.id ?? '未命名',
    polygons: polygonsOf(g),
  }));

  // local +x = 東, local +y = 北；父層 rotation.x = -90° 之後 +z 就是「上」
  // ⭐ 與 `projectLonLat` 是**同一個**投影（它就是這段抽出去的），
  //    ⛔ 不要讓兩邊各自演化 —— 縣市邊界與案場點位一旦用不同投影就會錯開。
  const project = (c: Pt): Pt => projectLonLat(c[0], c[1], mapSize);

  const bbox: Bbox = { minX: Infinity, minY: Infinity, maxX: -Infinity, maxY: -Infinity };
  const expand = (v: Pt) => {
    if (v[0] < bbox.minX) bbox.minX = v[0];
    if (v[1] < bbox.minY) bbox.minY = v[1];
    if (v[0] > bbox.maxX) bbox.maxX = v[0];
    if (v[1] > bbox.maxY) bbox.maxY = v[1];
  };

  const regions: Region[] = features.map((f: any) => {
    const polys: Poly[] = f.polygons.map((p: Poly) => ({
      outer: p.outer.map((c) => {
        const v = project(c);
        expand(v);
        return v;
      }),
      holes: p.holes.map((h) => h.map(project)),
    }));

    // 中心點：取面積最大的環的形心
    let best: Pt | null = null;
    let bestA = -1;
    let bestRing: Pt[] | null = null;
    for (const p of polys) {
      let a = 0;
      let cx = 0;
      let cy = 0;
      const r = p.outer;
      for (let i = 0, j = r.length - 1; i < r.length; j = i++) {
        const cr = r[j][0] * r[i][1] - r[i][0] * r[j][1];
        a += cr;
        cx += (r[j][0] + r[i][0]) * cr;
        cy += (r[j][1] + r[i][1]) * cr;
      }
      a *= 0.5;
      const A = Math.abs(a);
      if (A > bestA) {
        bestA = A;
        bestRing = r;
        best = A > 1e-9 ? [cx / (6 * a), cy / (6 * a)] : [r[0][0], r[0][1]];
      }
    }

    let radius = 0;
    for (const c of bestRing!) {
      const d = Math.hypot(c[0] - best![0], c[1] - best![1]);
      if (d > radius) radius = d;
    }

    return {
      id: f.id,
      name: f.name,
      polys,
      center: { x: best![0], y: best![1] },
      area: bestA,
      radius,
    };
  });

  /* ---------- 外輪廓：只被用到一次的 arc = 海岸線 ---------- */
  const outerRings = (): Pt[][] => {
    const use = new Map<number, number>();
    const ringsOf = (g: any): number[][] => (g.type === 'Polygon' ? g.arcs : g.arcs.flat());
    for (const g of OBJ.geometries) {
      for (const r of ringsOf(g)) {
        for (const i of r) {
          const k = i < 0 ? ~i : i;
          use.set(k, (use.get(k) || 0) + 1);
        }
      }
    }
    const segs: Pt[][] = [];
    use.forEach((n, k) => {
      if (n === 1) segs.push(ARCS[k].map(project));
    });

    // 把散段依端點接成封閉環
    const key = (p: Pt) => `${p[0].toFixed(6)}|${p[1].toFixed(6)}`;
    const used = new Array(segs.length).fill(false);
    const ends = new Map<string, number[]>();
    const push = (k: string, i: number) => {
      if (!ends.has(k)) ends.set(k, []);
      ends.get(k)!.push(i);
    };
    segs.forEach((s, i) => {
      push(key(s[0]), i);
      push(key(s[s.length - 1]), i);
    });

    const rings: Pt[][] = [];
    for (let i = 0; i < segs.length; i++) {
      if (used[i]) continue;
      used[i] = true;
      const ring = segs[i].slice();
      const startK = key(ring[0]);
      let guard = 0;
      while (guard++ < 4000) {
        const tailK = key(ring[ring.length - 1]);
        if (tailK === startK) break; // 收口
        const cand = (ends.get(tailK) || []).find((j) => !used[j]);
        if (cand === undefined) break;
        used[cand] = true;
        let s = segs[cand];
        if (key(s[0]) !== tailK) s = s.slice().reverse();
        for (let k = 1; k < s.length; k++) ring.push(s[k]);
      }
      if (ring.length > 3) rings.push(ring);
    }
    return rings.sort((a, b) => b.length - a.length);
  };

  return { regions, outerRings: outerRings(), bbox };
};
