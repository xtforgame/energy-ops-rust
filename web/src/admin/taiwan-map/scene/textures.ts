/**
 * 程序化貼圖 —— 對應 `temp/taiwan-scifi-map.html` §5。
 *
 * 原專案（Demo2）用四張 png：`sc_normal_map1.png`（頂面 normalMap）、`quan1.png`（底部
 * 光環）、`guangquan01.png`（標記光圈）、`fly_line.png`（飛線流光）。素材沒進 repo，
 * html 版已改成 canvas 程序化生成 —— 這裡沿用同一套，只有頂面那張改成白底。
 *
 * ⭐ 只有 `gridTexture()` 需要重畫：其餘三張畫的是**白色 + alpha**，材質的 `color`
 *   會乘上去，所以白底版只要把材質顏色換掉就成立，貼圖本身不用動 ——
 *   兩套主題（jv / lab）也因此共用同三張，只有頂面那張跟著 palette 走。
 */
import * as THREE from 'three';
import type { MapPalette } from './palette';
import { makeRng, MAP_SEED } from './rng';

const cv = (w: number, h?: number): [HTMLCanvasElement, CanvasRenderingContext2D] => {
  const c = document.createElement('canvas');
  c.width = w;
  c.height = h || w;
  return [c, c.getContext('2d')!];
};

const tex = (c: HTMLCanvasElement, rep?: [number, number]) => {
  const t = new THREE.CanvasTexture(c);
  if (rep) {
    t.wrapS = THREE.RepeatWrapping;
    t.wrapT = THREE.RepeatWrapping;
    t.repeat.set(rep[0], rep[1]);
  }
  t.colorSpace = THREE.SRGBColorSpace;
  t.anisotropy = 4;
  return t;
};

/**
 * 頂面科技網格（原作是深青底 + 青色格線）。
 *
 * 白底版：純白底 + 墨色細格 + 淡紫粗格。線的 alpha 抄 `futuristic-white` 的
 * `--azfw-grid-line: rgba(10,10,10,.045)` 量級 —— 白底上的格線只能到這個強度，
 * 再重就從「質感」變成「圖案」，會把上面的掃描光帶壓掉。
 */
const gridTexture = (pal: MapPalette) => {
  const rnd = makeRng(MAP_SEED);
  const S = 1024;
  const [c, g] = cv(S);
  g.fillStyle = pal.grid.base;
  g.fillRect(0, 0, S, S);

  for (let i = 0; i < 2600; i++) {
    // 細雜訊：白底改用墨點（原作是青色亮點）
    const a = rnd() * 0.035;
    g.fillStyle = `rgba(${pal.grid.noise},${a})`;
    g.fillRect(rnd() * S, rnd() * S, 2, 2);
  }

  g.lineWidth = 1;
  for (let i = 0; i <= S; i += 32) {
    // 細格
    g.strokeStyle = pal.grid.fine;
    g.beginPath();
    g.moveTo(i, 0);
    g.lineTo(i, S);
    g.moveTo(0, i);
    g.lineTo(S, i);
    g.stroke();
  }
  for (let i = 0; i <= S; i += 256) {
    // 粗格（jv 是唯一帶顏色的線；lab 一律墨色）
    g.strokeStyle = pal.grid.bold;
    g.beginPath();
    g.moveTo(i, 0);
    g.lineTo(i, S);
    g.moveTo(0, i);
    g.lineTo(S, i);
    g.stroke();
  }
  for (let i = 0; i < 26; i++) {
    // 隨機資料方塊
    const x = ((rnd() * 32) | 0) * 32;
    const y = ((rnd() * 32) | 0) * 32;
    g.fillStyle = `rgba(${pal.grid.block},${0.02 + rnd() * 0.04})`;
    g.fillRect(x, y, 32 * (1 + ((rnd() * 3) | 0)), 32);
  }
  return tex(c);
};

/** 底部旋轉光環（quan1.png）—— 白色描邊 + alpha，顏色由材質決定 */
const ringPlateTexture = (flatten: number) => {
  // a ** (1 - flatten)：flatten=0 原樣、越接近 1 各元素的 alpha 越靠攏（見 palette.plateFlatten）
  const A = (a: number) => Math.pow(a, 1 - flatten);
  const S = 1024;
  const [c, g] = cv(S);
  const R = S / 2;
  g.translate(R, R);
  const ring = (r: number, w: number, a: number, dash?: number[]) => {
    g.beginPath();
    g.arc(0, 0, r, 0, Math.PI * 2);
    g.strokeStyle = `rgba(255,255,255,${A(a)})`;
    g.lineWidth = w;
    g.setLineDash(dash || []);
    g.stroke();
    g.setLineDash([]);
  };
  ring(R * 0.30, 1.5, 0.30);
  ring(R * 0.44, 1, 0.16, [3, 9]);
  ring(R * 0.62, 2, 0.22, [40, 22]);
  ring(R * 0.66, 1, 0.10);
  ring(R * 0.82, 1.2, 0.16, [6, 14]);
  ring(R * 0.94, 3, 0.10, [110, 60]);
  for (let i = 0; i < 72; i++) {
    // 刻度
    const a = (i / 72) * Math.PI * 2;
    const long = i % 6 === 0;
    const r0 = R * 0.68;
    const r1 = r0 + (long ? 22 : 9);
    g.beginPath();
    g.moveTo(Math.cos(a) * r0, Math.sin(a) * r0);
    g.lineTo(Math.cos(a) * r1, Math.sin(a) * r1);
    g.strokeStyle = `rgba(255,255,255,${A(long ? 0.34 : 0.14)})`;
    g.lineWidth = long ? 2 : 1;
    g.stroke();
  }
  for (let i = 0; i < 4; i++) {
    // 四角掃描弧
    const a = (i / 4) * Math.PI * 2;
    g.beginPath();
    g.arc(0, 0, R * 0.52, a, a + 0.5);
    g.strokeStyle = `rgba(255,255,255,${A(0.42)})`;
    g.lineWidth = 4;
    g.stroke();
  }
  return tex(c);
};

/** 標記光圈（guangquan01.png，當 alphaMap 用） */
const haloTexture = () => {
  const S = 256;
  const [c, g] = cv(S);
  const R = S / 2;
  g.translate(R, R);
  g.beginPath();
  g.arc(0, 0, R * 0.74, 0, Math.PI * 2);
  g.strokeStyle = 'rgba(255,255,255,.95)';
  g.lineWidth = 5;
  g.stroke();
  g.beginPath();
  g.arc(0, 0, R * 0.50, 0.2, 2.1);
  g.strokeStyle = 'rgba(255,255,255,.8)';
  g.lineWidth = 8;
  g.stroke();
  g.beginPath();
  g.arc(0, 0, R * 0.50, 3.4, 5.2);
  g.strokeStyle = 'rgba(255,255,255,.8)';
  g.lineWidth = 8;
  g.stroke();
  for (let i = 0; i < 4; i++) {
    const a = (i / 4) * Math.PI * 2 + 0.4;
    g.beginPath();
    g.arc(0, 0, R * 0.90, a, a + 0.22);
    g.strokeStyle = 'rgba(255,255,255,.7)';
    g.lineWidth = 6;
    g.stroke();
  }
  return tex(c);
};

/** 飛線流光（fly_line.png） */
const flyTexture = () => {
  const W = 256;
  const H = 16;
  const [c, g] = cv(W, H);
  const grd = g.createLinearGradient(0, 0, W, 0);
  grd.addColorStop(0.00, 'rgba(255,255,255,0)');
  grd.addColorStop(0.55, 'rgba(255,255,255,0)');
  grd.addColorStop(0.80, 'rgba(255,255,255,.55)');
  grd.addColorStop(0.97, 'rgba(255,255,255,1)');
  grd.addColorStop(1.00, 'rgba(255,255,255,0)');
  g.fillStyle = grd;
  g.fillRect(0, 0, W, H);
  return tex(c, [1, 1]);
};

/** 拖尾頭部的光點 */
const sparkTexture = () => {
  const S = 128;
  const [c, g] = cv(S);
  const R = S / 2;
  const grd = g.createRadialGradient(R, R, 0, R, R, R);
  grd.addColorStop(0, 'rgba(255,255,255,1)');
  grd.addColorStop(0.25, 'rgba(255,255,255,.6)');
  grd.addColorStop(1, 'rgba(255,255,255,0)');
  g.fillStyle = grd;
  g.fillRect(0, 0, S, S);
  return tex(c);
};

export interface MapTextures {
  grid: THREE.Texture;
  plate: THREE.Texture;
  halo: THREE.Texture;
  fly: THREE.Texture;
  spark: THREE.Texture;
}

export const createTextures = (pal: MapPalette): MapTextures => ({
  grid: gridTexture(pal),
  plate: ringPlateTexture(pal.plateFlatten),
  halo: haloTexture(),
  fly: flyTexture(),
  spark: sparkTexture(),
});

export const disposeTextures = (t: MapTextures) => {
  Object.values(t).forEach((x) => x.dispose());
};
