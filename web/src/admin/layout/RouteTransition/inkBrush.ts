import type { LogoStroke } from './logoStrokes';

/**
 * 墨刷 logo 層 —— 移植自 `idea/ink-brush-trail.html`（純 shader 刷毛拖尾）。
 *
 * 與原 demo 的差異（都是「當轉場用」必須改的）：
 *   - 原 demo 由滑鼠事件 + `performance.now()` 時間戳驅動；這裡改成**確定性的
 *     `setTime(t)`**：每個取樣點的「出生時間」在 resize 時就算好，任何 t 都能直接
 *     重建畫面 —— 轉場可逆推、編輯器可 scrub、探針可量測。⛔ 不用 Math.random，
 *     seed 全固定，同一份筆畫資料畫一萬次都一樣。
 *   - 沒有紙張 shader（背景是雲），renderer 是透明的。
 *   - 筆色不是墨黑，per-stroke 用 logo 原色。
 *
 * 離場機制沿用 demo 的 trail life：點的年齡超過 life 就消失、接近 life 時淡出
 * （shader 裡 `smoothstep(0.0, 0.28, vLife)`）。把 life 設成 drawMs + holdMs，
 * 尾巴就會在 hold 結束後**以畫上去的相同節奏、沿著筆畫順序**離場 —— 這正是
 * 「順著筆畫離場」，不需要任何額外機制。
 *
 * 時間軸（setTime 的 t，單位 ms）：
 *   0 .. drawMs             所有筆畫**同時**下筆、等速前進（時長 ∝ 像素長度，
 *                           最長的那筆撐滿 drawMs、短的先收筆）
 *   drawMs .. drawMs+holdMs 全圖停留
 *   .. totalMs              尾巴追進度：最後一點出生於 drawMs、死於 drawMs+life
 *                           → totalMs = 2·drawMs + holdMs
 *
 * ⚠️ shader 與 rebuild 幾何邏輯是 demo 的逐字移植（含中文註解），也複製了一份在
 * tools/ink-stroke-editor.html（編輯器要在無 build 環境重現同樣畫面）——改這裡
 * 記得同步那邊。
 */

export interface InkBrushOptions {
  strokes: LogoStroke[];
  /** 筆畫座標系的邊長（logoStrokes 的 LOGO_VIEWBOX） */
  viewBox?: number;
  /** 墨量 0..1：越低飛白越多（demo 的「墨量」滑桿） */
  ink?: number;
  /** 毛數倍率（demo 的「毛數」滑桿） */
  bristle?: number;
  /** 全部筆畫畫完的時間 */
  drawMs?: number;
  /** 畫完後全圖停留的時間 */
  holdMs?: number;
  /** 全域筆寬倍率（乘在每筆的 width 上；demo 的「筆寬」滑桿） */
  widthScale?: number;
  /**
   * 餘韻（demo 的「餘韻」滑桿）：一個點從畫上到消失的壽命（ms）。
   * null = 自動 drawMs + holdMs —— 全圖剛好停留到 hold 結束才開始離場。
   * 設得比 drawMs 短 → 邊畫邊消失的拖尾（demo 的原始手感）。
   */
  lifeMs?: number | null;
}

export interface InkBrushLayout { x: number; y: number; size: number }

export interface InkBrush {
  canvas: HTMLCanvasElement;
  /** = drawMs + 餘韻（lifeMs 未給時餘韻 = drawMs + holdMs，即 2·drawMs + holdMs） */
  totalMs: number;
  resize: (w: number, h: number, layout: InkBrushLayout) => void;
  /** 依時間重建所有筆畫的可見片段（確定性；t 超出範圍就是全空） */
  setTime: (tMs: number) => void;
  render: () => void;
  debug: Record<string, any>;
  dispose: () => void;
}

/* ---------- 以下 shader 與 demo 逐字相同（拿掉紙張，其餘不動） ---------- */
const NOISE = /* glsl */ `
float hash11(float n){ return fract(sin(n*127.1)*43758.5453123); }
float hash21(vec2 p){
  p = fract(p * vec2(233.34, 851.73));
  p += dot(p, p + 23.45);
  return fract(p.x * p.y);
}
float vnoise(vec2 p){
  vec2 i = floor(p), f = fract(p);
  f = f*f*(3.0-2.0*f);
  return mix(mix(hash21(i),hash21(i+vec2(1,0)),f.x),
             mix(hash21(i+vec2(0,1)),hash21(i+vec2(1,1)),f.x), f.y);
}
float fbm(vec2 p){
  float s=0.0, a=0.5;
  for(int i=0;i<4;i++){ s+=a*vnoise(p); p*=2.11; a*=0.5; }
  return s;
}
/* fbm 實際只落在 0.3~0.7，必須正規化並拉開對比，否則任何閾值都切不動 */
float fbm01(vec2 p){
  float f = fbm(p) / 0.9375;
  return clamp((f - 0.5) * 1.6 + 0.5, 0.0, 1.0);
}`;

const STROKE_VS = /* glsl */ `
attribute float aU, aV, aLife, aDry, aHalf;
varying float vU, vV, vLife, vDry, vHalf;
void main(){
  vU=aU; vV=aV; vLife=aLife; vDry=aDry; vHalf=aHalf;
  gl_Position = projectionMatrix * modelViewMatrix * vec4(position,1.0);
}`;

const STROKE_FS = NOISE + /* glsl */ `
precision highp float;
varying float vU, vV, vLife, vDry, vHalf;
uniform float uLen, uInk, uBristle, uSeed;
uniform vec3  uColor;

void main(){
  float s  = vU * uLen;          // 沿筆畫的像素距離
  float x  = abs(vV);            // 0=中心 1=包絡外緣
  float hp = max(vHalf, 1.0);    // 此處半寬（px）
  float y  = vV * hp;            // 橫向的像素座標（讓孔洞是圓的，不會被拉長）

  /* 乾濕度：墨量、下筆速度、老化、尾端 */
  float dry = (1.0 - uInk)*0.95 + vDry*0.45 + (1.0 - vLife)*0.30;
  dry += 0.75 * smoothstep(0.22, 0.0, vU);
  dry = clamp(dry, 0.0, 1.15);

  /* 門檻：濕 → 遠低於 0，每根毛都撐爆自己的格子而互相黏成實心 */
  float th = mix(-0.30, 0.86, dry) + 0.16*x*x;

  /* 刷毛：取樣鄰近 5 根，粗的會蓋過旁邊 → 重壓時併成不規則墨塊 */
  float NB  = clamp(hp * 0.45 * uBristle, 4.0, 34.0);
  float pos = (vV*0.5 + 0.5) * NB;
  float b0  = floor(pos);
  float cover = 0.0;
  for(int k=-2; k<=2; k++){
    float bi  = b0 + float(k);
    float sd  = hash11(bi*1.37 + uSeed*7.0);
    float splay = 0.55 + 0.90*fbm01(vec2(s*0.0030, uSeed*5.0));     // 毛時聚時散
    float wob = (vnoise(vec2(s*0.0035, sd*77.0)) - 0.5) * 1.15 * splay;
    float wet = fbm01(vec2(s*0.0040, sd*120.0 + uSeed));            // 沿長度緩慢起伏
    wet = mix(wet, 1.0, 0.22*(1.0 - x*x));
    float hw  = clamp((wet - th) * 2.2 * (0.30 + 1.5*sd), 0.0, 2.0); // 每根毛粗細隨機
    float d   = abs(pos - (bi + 0.5 + wob));
    cover = max(cover, 1.0 - smoothstep(max(hw - 0.22, 0.0), hw + 0.02, d));
  }

  /* 跳筆留白：橫向切成幾條會蛇行的寬帶，帶內沿 s 斷續；
     再用「離帶中心的距離」抬高門檻，讓孔的上下兩側收尖 → 長條紡錘狀，而非圓斑 */
  float rowN   = clamp(hp * 0.17, 3.0, 9.0);      // 帶更窄 → 孔的橫向尺寸上限更小
  float rowPos = (vV*0.5 + 0.5) * rowN + (fbm01(vec2(s*0.0060, uSeed*11.0)) - 0.5) * 1.2;
  float rs     = hash11(floor(rowPos)*5.7 + uSeed);
  float rf     = abs(fract(rowPos) - 0.5) * 2.0;
  float skipN  = fbm01(vec2(s*0.0165 + rs*35.0, rs*70.0)) + 0.42*rf*rf;  // 頻率提高 → 孔更短
  float ts     = 0.06 + 0.34*dry;
  float gap    = smoothstep(ts, ts + 0.13, skipN);   // 0 = 留白

  /* 孔洞尺寸上限：較高頻的補墨場把過大的白塊切開，
     大孔被切成幾個小孔，小孔則幾乎不受影響 */
  float refill = smoothstep(0.60, 0.80, fbm01(vec2(s*0.030, y*0.20) + uSeed*7.0));
  cover *= max(gap, refill*0.92);

  /* 細碎漏白，同樣沿筆向拉長 8:1，不做成點狀 */
  float pin = fbm01(vec2(s*0.045, y*0.42) + uSeed*3.0);
  float tp  = 0.05 + 0.22*dry;
  cover *= mix(1.0, smoothstep(tp, tp + 0.11, pin), 0.40 + 0.30*dry);

  /* 左右輪廓各自被啃出不規則的邊 */
  float side = vV < 0.0 ? 0.0 : 41.0;
  float lim  = 0.78 + 0.22*fbm01(vec2(s*0.008, side + uSeed)) - 0.09*vnoise(vec2(s*0.060, side));
  float outer = 1.0 - smoothstep(lim, lim + 1.6/hp, x);

  /* 兩端不收尖：每根毛結束在不同位置 */
  float ej   = hash11(b0*3.11 + 7.0 + uSeed);
  float head = smoothstep(0.0, 2.0, (1.0 - vU)*uLen - ej*9.0);
  float tail = smoothstep(0.0, 2.0, s - ej*14.0);

  float a = cover * outer * head * tail * smoothstep(0.0, 0.28, vLife);
  if(a < 0.012) discard;
  float g = fbm(vec2(s*0.06, vV*5.0));
  gl_FragColor = vec4(uColor + (g - 0.5)*0.05*vec3(1.0,0.92,0.8), a*0.97);
}`;

/* ---------- demo 的 CPU 端雜訊（用來合成 w / dry 起伏，取代滑鼠速度） ---------- */
const h1 = (n: number) => { const s = Math.sin(n * 127.1) * 43758.5453; return s - Math.floor(s); };
const n1 = (x: number) => { const i = Math.floor(x); const f = x - i; const u = f * f * (3 - 2 * f); return h1(i) * (1 - u) + h1(i + 1) * u; };
const sstep = (a: number, b: number, x: number) => { const t = Math.min(1, Math.max(0, (x - a) / (b - a))); return t * t * (3 - 2 * t); };

const MAXP = 520;

interface PreparedPoint { x: number; y: number; w: number; dry: number; born: number }
interface PreparedStroke {
  pts: PreparedPoint[];
  widthPx: number;
  seed: number;
  mesh: any; geo: any; mat: any;
}

export const createInkBrush = async ({
  strokes,
  viewBox = 56,
  ink = 0.82,
  bristle = 1.0,
  drawMs = 380,
  holdMs = 160,
  widthScale = 1,
  lifeMs = null,
}: InkBrushOptions): Promise<InkBrush> => {
  const THREE = await import('three');

  const renderer = new THREE.WebGLRenderer({ antialias: true, alpha: true });
  renderer.setPixelRatio(Math.min(window.devicePixelRatio || 1, 2));
  renderer.setClearColor(0x000000, 0);

  const scene = new THREE.Scene();
  // demo 是 top-left 原點、y 往下 —— 筆畫座標照這個約定
  const camera = new THREE.OrthographicCamera(0, 1, 0, 1, -10, 10);

  const life = lifeMs && lifeMs > 0 ? lifeMs : drawMs + holdMs;
  const totalMs = drawMs + life;

  /* --- 每條筆畫一組 geometry（demo 的 makeStroke，attribute 相同） --- */
  const prepared: PreparedStroke[] = strokes.map((def, i) => {
    const g = new THREE.BufferGeometry();
    const n = MAXP * 2;
    const attr = (a: Float32Array, size: number) => {
      const b = new THREE.BufferAttribute(a, size);
      b.setUsage(THREE.DynamicDrawUsage);
      return b;
    };
    g.setAttribute('position', attr(new Float32Array(n * 3), 3));
    for (const k of ['aU', 'aV', 'aLife', 'aDry', 'aHalf']) g.setAttribute(k, attr(new Float32Array(n), 1));
    const idx = new Uint16Array((MAXP - 1) * 6);
    for (let q = 0; q < MAXP - 1; q += 1) {
      const o = q * 6; const a = q * 2;
      idx[o] = a; idx[o + 1] = a + 1; idx[o + 2] = a + 2;
      idx[o + 3] = a + 1; idx[o + 4] = a + 3; idx[o + 5] = a + 2;
    }
    g.setIndex(new THREE.BufferAttribute(idx, 1));
    const m = new THREE.ShaderMaterial({
      vertexShader: STROKE_VS,
      fragmentShader: STROKE_FS,
      transparent: true,
      depthTest: false,
      depthWrite: false,
      side: THREE.DoubleSide,
      uniforms: {
        uLen: { value: 1 },
        uInk: { value: ink },
        uBristle: { value: bristle },
        uSeed: { value: (i + 1) * 7.31 },   // 固定 seed：畫幾次都一樣
        // ⚠️ 一定要用 NoColorSpace 吃原始 sRGB 值。three 0.184 預設開 ColorManagement，
        //    `new Color('#61619E')` 會被轉成 linear（0.38→0.12），而 RAW ShaderMaterial
        //    輸出時不會轉回 sRGB → 畫出來整個暗一截（實測 [97,97,158] 變 [29,29,83]）。
        //    demo 的 r128 沒有 ColorManagement，編輯器（CDN r128）也是吃原始值。
        uColor: { value: new THREE.Color().setStyle(def.color, THREE.NoColorSpace) },
      },
    });
    const mesh = new THREE.Mesh(g, m);
    mesh.frustumCulled = false;
    mesh.visible = false;
    scene.add(mesh);
    return { pts: [], widthPx: 10, seed: (i + 1) * 7.31, mesh, geo: g, mat: m };
  });

  /* --- resize：把 viewBox 座標鋪到版面上，重取樣成等距點並配好出生時間 --- */
  // 同 cloudCurtain：no-op resize 要擋掉（setSize 對 canvas 尺寸賦值會清 buffer）
  let lastKey = '';
  const resize = (w: number, h: number, layout: InkBrushLayout) => {
    const width = Math.max(1, Math.round(w));
    const height = Math.max(1, Math.round(h));
    const key = `${width}x${height}:${layout.x.toFixed(1)},${layout.y.toFixed(1)},${layout.size.toFixed(1)}`;
    if (key === lastKey) return;
    lastKey = key;
    renderer.setSize(width, height, false);
    camera.left = 0; camera.right = width; camera.top = 0; camera.bottom = height;
    camera.updateProjectionMatrix();

    const k = layout.size / viewBox;
    // 每筆的時長 ∝ 像素長度；筆畫依序下（書法是一筆一筆寫的）
    const pxPolys = strokes.map((def) => def.points.map(([x, y]) => [layout.x + x * k, layout.y + y * k] as [number, number]));
    const lens = pxPolys.map((poly) => {
      let L = 0;
      for (let i = 1; i < poly.length; i += 1) L += Math.hypot(poly[i][0] - poly[i - 1][0], poly[i][1] - poly[i - 1][1]);
      return L;
    });
    // 同時下筆、等速：每筆時長 ∝ 長度，最長那筆撐滿 drawMs、短的先收筆。
    //（曾經是依序下筆——書法式一筆一筆寫——但 drawMs 只有 ~380ms，5 筆分完
    //  每筆剩幾十 ms，視覺上反而像亂序閃現；同時下筆每筆能畫慢一點。）
    const maxLen = Math.max(1, ...lens);
    prepared.forEach((ps, si) => {
      const poly = pxPolys[si];
      const L = lens[si];
      const dur = (L / maxLen) * drawMs;
      const spacing = Math.max(1.6, L / (MAXP - 2));
      const seed = ps.seed;

      const pts: PreparedPoint[] = [];
      let acc = 0;
      let travelled = 0;
      const push = (x: number, y: number, u: number) => {
        // w / dry 的起伏用固定 seed 的雜訊合成（demo 是由滑鼠速度來的）：
        // 中低頻讓筆身粗細不勻，尾端 dry 升高 = 收筆的飛白
        const w0 = 0.86 + 0.26 * n1(u * 9 + seed * 3);
        const dry = 0.16 + 0.42 * n1(u * 13 + seed * 7) + 0.30 * sstep(0.8, 1, u);
        pts.push({ x, y, w: w0, dry, born: u * dur });
      };
      push(poly[0][0], poly[0][1], 0);
      for (let i = 1; i < poly.length; i += 1) {
        const [ax, ay] = poly[i - 1];
        const [bx, by] = poly[i];
        const seg = Math.hypot(bx - ax, by - ay);
        let d = spacing - acc;
        while (d <= seg) {
          const u2 = (travelled + d) / L;
          push(ax + (bx - ax) * (d / seg), ay + (by - ay) * (d / seg), u2);
          d += spacing;
        }
        acc = (acc + seg) % spacing;
        travelled += seg;
      }
      push(poly[poly.length - 1][0], poly[poly.length - 1][1], 1);
      ps.pts = pts;
      ps.widthPx = strokes[si].width * k * widthScale;
    });
  };

  /* --- rebuild：demo 的 rebuild 逐字移植，輸入改成「此刻可見的點片段」 --- */
  const tmp: PreparedPoint[] = [];
  const rebuildStroke = (ps: PreparedStroke, t: number) => {
    const vis = ps.pts.filter((p) => p.born <= t && t - p.born <= life);
    if (vis.length < 2) { ps.mesh.visible = false; return; }

    tmp.length = 0;
    for (const p of vis) tmp.push({ ...p });
    for (let pass = 0; pass < 2; pass += 1) {
      for (let i = 1; i < tmp.length - 1; i += 1) {
        tmp[i].x = 0.25 * tmp[i - 1].x + 0.5 * tmp[i].x + 0.25 * tmp[i + 1].x;
        tmp[i].y = 0.25 * tmp[i - 1].y + 0.5 * tmp[i].y + 0.25 * tmp[i + 1].y;
        tmp[i].w = 0.25 * tmp[i - 1].w + 0.5 * tmp[i].w + 0.25 * tmp[i + 1].w;
        tmp[i].dry = 0.25 * tmp[i - 1].dry + 0.5 * tmp[i].dry + 0.25 * tmp[i + 1].dry;
      }
    }

    const n = tmp.length;
    const len = new Float32Array(n);
    let total = 0;
    for (let i = 1; i < n; i += 1) {
      total += Math.hypot(tmp[i].x - tmp[i - 1].x, tmp[i].y - tmp[i - 1].y);
      len[i] = total;
    }
    if (total < 2) { ps.mesh.visible = false; return; }

    const A = ps.geo.attributes;
    const pos = A.position.array as Float32Array;
    const seed = ps.seed;
    for (let i = 0; i < n; i += 1) {
      const p = tmp[i];
      const pv = i > 0 ? tmp[i - 1] : tmp[i];
      const nx = i < n - 1 ? tmp[i + 1] : tmp[i];
      let tx = nx.x - pv.x; let ty = nx.y - pv.y;
      const tl = Math.hypot(tx, ty) || 1; tx /= tl; ty /= tl;
      const u = len[i] / total;

      // 筆身粗細本來就不勻：低頻大起伏 + 中頻小起伏
      const wob = 0.70 + 0.42 * n1(len[i] * 0.008 + seed) + 0.20 * n1(len[i] * 0.038 + seed * 3);
      // 兩端幾乎不收（散開交給 shader 的毛絲處理）
      const cap = 0.82 + 0.18 * sstep(0, 0.03, u);
      const half = 0.5 * ps.widthPx * p.w * wob * cap * 1.18;

      const lifeFrac = Math.min(1, Math.max(0, 1 - (t - p.born) / life));
      const j = i * 2; const kk = j * 3;
      pos[kk] = p.x - ty * half; pos[kk + 1] = p.y + tx * half; pos[kk + 2] = 0;
      pos[kk + 3] = p.x + ty * half; pos[kk + 4] = p.y - tx * half; pos[kk + 5] = 0;
      A.aU.array[j] = u; A.aU.array[j + 1] = u;
      A.aV.array[j] = -1; A.aV.array[j + 1] = 1;
      A.aLife.array[j] = lifeFrac; A.aLife.array[j + 1] = lifeFrac;
      A.aDry.array[j] = p.dry; A.aDry.array[j + 1] = p.dry;
      A.aHalf.array[j] = half; A.aHalf.array[j + 1] = half;
    }
    for (const kName of ['position', 'aU', 'aV', 'aLife', 'aDry', 'aHalf']) A[kName].needsUpdate = true;
    ps.geo.setDrawRange(0, (n - 1) * 6);
    ps.mat.uniforms.uLen.value = total;
    ps.mesh.visible = true;
  };

  const setTime = (tMs: number) => {
    for (const ps of prepared) rebuildStroke(ps, tMs);
  };

  const render = () => renderer.render(scene, camera);

  const dispose = () => {
    // ⛔ dispose 不等於移除：canvas 要自己從 DOM 拔掉（cloudCurtain 踩過的坑）
    renderer.domElement.remove();
    for (const ps of prepared) { ps.geo.dispose(); ps.mat.dispose(); }
    renderer.dispose();
    renderer.forceContextLoss();
  };

  return {
    canvas: renderer.domElement,
    totalMs,
    resize,
    setTime,
    render,
    debug: { scene, camera, renderer, prepared, life, drawMs, holdMs },
    dispose,
  };
};

export default createInkBrush;
