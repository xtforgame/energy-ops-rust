/**
 * 沿海岸線飛行的拖尾 —— 對應 Demo2 `geoTrail.tsx`、html 版 §11。
 *
 * 白底反轉：原作靠 AdditiveBlending + 逐點顏色 `ACC * k * 2.2` 做尾巴衰減 ——
 * 加法混合下「顏色變暗」就等於「變透明」。白底沒有這回事，所以改成
 * **逐點 alpha**（color attribute 用 itemSize 4，three 會開 `USE_COLOR_ALPHA`），
 * 顏色固定、alpha 從 0 漸到 1。這是唯一能在白底做出同一條漸隱尾巴的寫法。
 */
import * as THREE from 'three';
import type { GeoModel, Pt } from './geo';
import type { BuildCtx } from './ctx';
import { color } from './glColor';

export interface Trail {
  path: Pt[];
  n: number;
  geo: THREE.BufferGeometry;
  head: THREE.Sprite;
  t: number;
}

export const buildTrail = (parent: THREE.Object3D, model: GeoModel, ctx: BuildCtx): Trail => {
  const { pal, tex, reveal, res, depth } = ctx;
  const path = model.outerRings[0]; // 最長的環 = 台灣本島
  const n = 46; // 拖尾取樣點
  const geo = res.add(new THREE.BufferGeometry());
  geo.setAttribute('position', new THREE.Float32BufferAttribute(new Float32Array(n * 3), 3));
  geo.setAttribute('color', new THREE.Float32BufferAttribute(new Float32Array(n * 4), 4));

  const mat = reveal.track(
    res.add(
      new THREE.LineBasicMaterial({
        vertexColors: true,
        transparent: true,
        depthWrite: false,
        fog: false,
      }),
    ),
    pal.alpha.trail,
  );
  const line = new THREE.Line(geo, mat);
  line.position.z = depth + 0.06;
  line.raycast = () => null;
  line.renderOrder = 12;
  parent.add(line);

  const head = new THREE.Sprite(
    reveal.track(
      res.add(
        new THREE.SpriteMaterial({
          map: tex.spark,
          color: color(pal.bgInk),
          transparent: true,
          depthWrite: false,
          fog: false,
        }),
      ),
      pal.alpha.trailHead,
    ),
  );
  head.scale.setScalar(0.5);
  line.add(head);

  return { path, n, geo, head, t: 0 };
};

export const updateTrail = (trail: Trail, dt: number, ink: THREE.Color) => {
  const { path, n, geo, head } = trail;
  trail.t += dt * 0.055;
  const total = path.length;
  const headIdx = trail.t * total;
  const pos = geo.attributes.position as THREE.BufferAttribute;
  const col = geo.attributes.color as THREE.BufferAttribute;
  for (let i = 0; i < n; i++) {
    const idx = Math.floor(headIdx - (n - 1 - i) * 1.15);
    const p = path[((idx % total) + total) % total];
    pos.setXYZ(i, p[0], p[1], 0);
    const f = i / (n - 1);
    const k = Math.pow(f, 2.6); // 衰減曲線沿用原作
    col.setXYZW(i, ink.r, ink.g, ink.b, k);
  }
  pos.needsUpdate = true;
  col.needsUpdate = true;
  const hp = path[((Math.floor(headIdx) % total) + total) % total];
  head.position.set(hp[0], hp[1], 0);
};
