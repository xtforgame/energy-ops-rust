/**
 * 底部旋轉光環 —— 對應 Demo2 `bottom.tsx`、html 版 §14。
 *
 * 兩層同心刻度盤，反向慢轉。白底反轉：AdditiveBlending → NormalBlending，
 * 不透明度 0.55 / 0.28 → 0.30 / 0.14（白底上實色刻度很搶眼，原值會蓋過地圖本體）。
 */
import * as THREE from 'three';
import type { BuildCtx } from './ctx';
import { color } from './glColor';

export const buildBottomRing = (scene: THREE.Object3D, ctx: BuildCtx): THREE.Mesh[] => {
  const { pal, focusPal, tex, reveal, res } = ctx;
  const make = (size: number, y: number, tint: string, target: number, focusTarget: number) => {
    const mat = reveal.track(
      res.add(
        new THREE.MeshBasicMaterial({
          map: tex.plate,
          color: color(tint),
          transparent: true,
          depthWrite: false,
          fog: false,
        }),
      ),
      target,
      focusTarget,
    );
    const m = new THREE.Mesh(res.add(new THREE.PlaneGeometry(size, size)), mat);
    m.rotation.x = -Math.PI / 2;
    m.position.y = y;
    m.raycast = () => null;
    m.renderOrder = 0;
    scene.add(m);
    return m;
  };

  return [
    make(15, 0.012, pal.bgInk, pal.alpha.ring1, focusPal.alpha.ring1),
    make(23, 0.01, pal.bgInkSoft, pal.alpha.ring2, focusPal.alpha.ring2),
  ];
};

export const updateBottomRing = (rings: THREE.Mesh[], dt: number, spin: boolean) => {
  // ⚠️ 不轉的時候連 rotation 都不要動 —— 盤面貼圖有四段掃描弧，一轉就是「一道光沿著圓跑」
  if (!spin) return;
  rings[0].rotation.z += dt * 0.16;
  rings[1].rotation.z -= dt * 0.075;
};
