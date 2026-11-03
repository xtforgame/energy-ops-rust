/**
 * 地址 → 座標的邊界（`DESIGN.md §1.2.1`）。
 *
 * **與 OCR 邊界（`billExtractor.ts`）同一招**：介面是我們的，實作是可換的。
 * 不要在系統各處直接呼叫某個 provider —— 換 provider 時會找不完。
 *
 * 這裡的三條約束是 **2026-08-26 實測 ArcGIS 之後**定下來的，
 * 不要憑直覺重新發明（尤其是第 1 條，很反直覺）。
 */

/** 比對到什麼層級。這是**一級欄位**，見下方為什麼不用 score。 */
export type GeocodePrecision =
  /** 門牌／建物級 —— 可直接當初值 */
  | 'exact'
  /** 路段級 —— 可當初值，建議人工確認 */
  | 'street'
  /** 只到鄉鎮／行政區級 —— **不可當座標用**，只能提示 */
  | 'locality'
  | 'unknown';

export interface GeocodeResult {
  latitude: number;
  longitude: number;
  /**
   * **`precision` 是一級欄位，不要用 score 當門檻。**
   *
   * 實測：「台南市七股區大潭段123地號」→ ArcGIS 回 **score 76.4**，
   * 分數看起來很像可用，但**座標是區公所**。
   * 分數高不代表對；`Addr_type` / precision 才是真訊號。
   */
  precision: GeocodePrecision;
  /** `'mock' | 'nominatim' | 'tgos' | ...` —— 一定要記，換 provider 時才知道哪些座標該重跑。 */
  source: string;
  /** provider 認為它比對到什麼（給人核對用）。 */
  matchedAddress?: string;
  /** provider 原始回應。不 pin shape —— 換 provider 時這裡本來就會長不一樣。 */
  raw?: unknown;
}

/**
 * **查不到就回 `null`，絕不回退成行政區中心點。**
 *
 * 回一個「看起來很正常」的座標**比查不到危險得多**：它不會報錯，
 * 只會把電廠靜靜標在錯的地方，然後沒有人發現。
 */
export type Geocoder = (address: string) => Promise<GeocodeResult | null>;

/** 這個精度可不可以直接填進座標欄位。 */
export const isUsablePrecision = (p: GeocodePrecision): boolean => p === 'exact' || p === 'street';

// ---------------------------------------------------------------------------
// v1 的 mock
// ---------------------------------------------------------------------------

/**
 * fixture 對照表。這是**假資料**，只為了讓 UI 與流程能被驗證。
 * demo seed 的案場**不靠它** —— 那些座標一律 `geocodeSource='manual'` 直接給
 * （`DESIGN.md §1.2.1`：「地圖不該等 geocoder」）。
 */
const FIXTURES: Array<{ match: RegExp; result: Omit<GeocodeResult, 'source'> }> = [
  {
    // 有門牌的：geocoder 幫得上忙的那一批
    match: /屏東.*枋寮/,
    result: {
      latitude: 22.3654, longitude: 120.5931, precision: 'exact', matchedAddress: '屏東縣枋寮鄉',
    },
  },
  {
    match: /嘉義.*六腳/,
    result: {
      latitude: 23.4921, longitude: 120.3078, precision: 'street', matchedAddress: '嘉義縣六腳鄉',
    },
  },
  {
    /**
     * 這一筆是**故意**的：地段地號查得到「區」但查不到「地」。
     *
     * `precision='locality'` ＝ 只到行政區級 → **不可以填進座標**，只能提示。
     * 這正是實測 ArcGIS 回 score 76.4 卻指到區公所的那個案例。
     * 太陽能場最常見的登記形式就是地段地號，沒有任何 geocoder 解得了它。
     */
    match: /地號|段\s*\d/,
    result: {
      latitude: 23.1417, longitude: 120.1394, precision: 'locality', matchedAddress: '（僅比對到行政區）',
    },
  },
];

/**
 * v1 的 mock geocoder。查不到回 `null`（不是丟例外，也不是給個大概的點）。
 */
export const mockGeocoder: Geocoder = async (address: string) => {
  const a = String(address ?? '').trim();
  if (!a) return null;
  const hit = FIXTURES.find((f) => f.match.test(a));
  if (!hit) return null;
  return { ...hit.result, source: 'mock', raw: { fixture: true, query: a } };
};
