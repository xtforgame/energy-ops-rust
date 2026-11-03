/**
 * 縣市標記：菱錐 + 旋轉光圈 —— 對應 Demo2 `cone.tsx`、html 版 §12。
 *
 * 白底反轉：兩個材質都從 AdditiveBlending 換成 NormalBlending。菱錐用飽和紫的實心，
 * 光圈用淺紫 + alphaMap；不透明度略降（原 0.95 / 0.9 → 0.85 / 0.55），因為白底上
 * 實色的存在感本來就比黑底的加法光暈強。
 *
 * 效能備註：Demo2 用 drei `<Instances>`，html 版與這裡都是共用 geometry/material 的
 * 個別 mesh。22 個標記的 draw call 差異在這個場景無感，但 hover 的個別位移要靠
 * mesh 各自的 transform，改 InstancedMesh 反而要自己維護矩陣。
 */
import * as THREE from 'three';
import type { County } from './counties';
import type { BuildCtx } from './ctx';
import { color } from './glColor';
import { makeRng, MAP_SEED } from './rng';

export const buildMarkers = (counties: County[], ctx: BuildCtx) => {
  const { pal, focusPal, tex, reveal, res, depth } = ctx;
  const rnd = makeRng(MAP_SEED + 1);

  const k = pal.markerScale;
  const coneGeo = res.add(new THREE.ConeGeometry(0.17 * k, 0.42 * k, 4));
  const coneMat = reveal.track(
    res.add(
      new THREE.MeshBasicMaterial({
        color: color(pal.marker),
        side: THREE.DoubleSide,
        transparent: true,
        depthWrite: false,
        fog: false,
      }),
    ),
    pal.alpha.cone,
    focusPal.alpha.cone,
  );

  const haloGeo = res.add(new THREE.PlaneGeometry(0.66 * k, 0.66 * k));
  const haloMat = reveal.track(
    res.add(
      new THREE.MeshBasicMaterial({
        color: color(pal.bgInkSoft),
        alphaMap: tex.halo,
        transparent: true,
        depthWrite: false,
        depthTest: false,
        fog: false,
      }),
    ),
    pal.alpha.halo,
    focusPal.alpha.halo,
  );

  for (const c of counties) {
    const p = c.region.center;

    const cone = new THREE.Mesh(coneGeo, coneMat);
    cone.rotation.x = -Math.PI / 2; // 尖端朝 +z（世界的上）
    cone.position.set(p.x, p.y, depth + 0.35 * k);
    cone.raycast = () => null;
    cone.renderOrder = 6;
    cone.userData = { base: depth + 0.3 * k, ph: rnd() * Math.PI * 2 };
    c.group.add(cone);

    const halo = new THREE.Mesh(haloGeo, haloMat);
    halo.position.set(p.x, p.y, depth + 0.02);
    halo.raycast = () => null;
    halo.renderOrder = 7;
    halo.rotation.z = rnd() * Math.PI;
    c.group.add(halo);

    c.cone = cone;
    c.halo = halo;
  }
};
