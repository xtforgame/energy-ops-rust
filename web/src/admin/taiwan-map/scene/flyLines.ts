/**
 * 貝茲飛線 —— 對應 Demo2 `flyLine.tsx`、html 版 §13。
 *
 * 以臺北市為 hub，向其餘縣市拉二次貝茲曲線的 TubeGeometry，貼圖沿 x 捲動做流光。
 *
 * 白底反轉：AdditiveBlending → NormalBlending。貼圖本身是「白 + alpha 漸層」，
 * 材質 color 換成紫就直接成立（見 textures.ts 的說明），不透明度 0.7 → 0.45。
 */
import * as THREE from 'three';
import type { County } from './counties';
import type { BuildCtx } from './ctx';
import { color } from './glColor';

export const buildFlyLines = (parent: THREE.Object3D, counties: County[], ctx: BuildCtx) => {
  const { pal, focusPal, tex, reveal, res, depth } = ctx;
  const hub =
    counties.find((c) => c.region.name.includes('臺北市') || c.region.name.includes('台北市')) || counties[0];
  const h = hub.region.center;

  const mat = reveal.track(
    res.add(
      new THREE.MeshBasicMaterial({
        color: color(pal.bgInk),
        map: tex.fly,
        transparent: true,
        depthWrite: false,
        depthTest: false,
        fog: false,
      }),
    ),
    pal.alpha.flyLine,
    focusPal.alpha.flyLine,
  );
  tex.fly.wrapS = THREE.RepeatWrapping;
  tex.fly.wrapT = THREE.RepeatWrapping;
  tex.fly.repeat.set(1, 1);

  const grp = new THREE.Group();
  grp.position.z = depth + 0.05;
  grp.renderOrder = 10;
  for (const c of counties) {
    if (c === hub) continue;
    const p = c.region.center;
    const dist = Math.hypot(h.x - p.x, h.y - p.y);
    const mid = new THREE.Vector3((h.x + p.x) / 2, (h.y + p.y) / 2, Math.max(1.1, dist * 0.62));
    const curve = new THREE.QuadraticBezierCurve3(
      new THREE.Vector3(h.x, h.y, 0),
      mid,
      new THREE.Vector3(p.x, p.y, 0),
    );
    const m = new THREE.Mesh(res.add(new THREE.TubeGeometry(curve, 40, 0.028, 5, false)), mat);
    m.raycast = () => null;
    grp.add(m);
  }
  parent.add(grp);

  return { texture: tex.fly };
};
