/**
 * 種子亂數（mulberry32）。
 *
 * 原作到處用 `Math.random()`（雜訊、光柱位置、菱錐相位…）。這裡換成固定種子的
 * PRNG —— 跟 `layout/RouteTransition/cloudCurtain.ts` 同一個理由：**畫面要可重現**，
 * 否則截圖驗證每次跑出來的雜訊都不一樣，量到的差異分不清是改動還是亂數。
 */
export const makeRng = (seed: number) => {
  let a = seed >>> 0;
  return () => {
    a += 0x6d2b79f5;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
};

/** 這張地圖的固定種子 */
export const MAP_SEED = 20260823;
