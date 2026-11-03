/**
 * 外輪廓光牆 + 海岸線亮邊 —— 對應 Demo2 `boundary.tsx`、html 版 §10。
 *
 * 白底反轉：
 *   - 海岸線原本是 AdditiveBlending 的青光（黑底上會過曝成白）。白底改 NormalBlending
 *     的深紫實線 —— 這條線在白底版是整張圖的骨架，強度要留住。
 *   - 光牆的目標不透明度從 0.34 降到 0.20：暗底的加法光暈很淡，換成白底的實色遮蔽
 *     同樣數值會變成一圈明顯的紫裙邊。
 */
import * as THREE from 'three';
import type { GeoModel } from './geo';
import type { BuildCtx } from './ctx';
import { color, OUTPUT_COLORSPACE } from './glColor';

export const buildBoundary = (parent: THREE.Object3D, model: GeoModel, ctx: BuildCtx) => {
  const { pal, focusPal, reveal, res, depth } = ctx;
  // 光牆高度：原作 2.6（黑底的加法光暈越高越好看）。白底是實色遮蔽，照抄會在島的
  // 四周糊出一圈裙邊、把海岸線壓掉，離島更會變成漂浮的板子 —— 由主題決定，見 palette。
  const D = pal.boundaryHeight;
  const shapes = model.outerRings.map((r) => new THREE.Shape(r.map((p) => new THREE.Vector2(p[0], p[1]))));

  const mat = reveal.uniform(
    res.add(
      new THREE.ShaderMaterial({
        transparent: true,
        depthWrite: false,
        depthTest: false,
        side: THREE.DoubleSide,
        uniforms: {
          uColor: { value: color(pal.bgInkSoft) },
          uOpacity: { value: 0 },
          uDepth: { value: D },
        },
        vertexShader: `
          varying vec3 vN; varying float vH;
          void main () { vN = normal; vH = position.z;
            gl_Position = projectionMatrix * modelViewMatrix * vec4(position, 1.0); }`,
        fragmentShader: `
          uniform vec3 uColor; uniform float uOpacity, uDepth;
          varying vec3 vN; varying float vH;
          void main () {
            if (abs(vN.z) > 0.9) discard;               // 只要側牆
            float h = 1.0 - clamp(vH / uDepth, 0.0, 1.0);
            gl_FragColor = vec4(uColor, pow(h, 1.6) * uOpacity);
            ${OUTPUT_COLORSPACE}
          }`,
      }),
    ),
    pal.alpha.boundaryWall,
    focusPal.alpha.boundaryWall,
  );

  const wall = new THREE.Mesh(res.add(new THREE.ExtrudeGeometry(shapes, { depth: D, bevelEnabled: false })), mat);
  wall.raycast = () => null;
  wall.renderOrder = 11;
  wall.position.z = 0.02;
  parent.add(wall);

  // 海岸線亮邊
  const pts: number[] = [];
  for (const r of model.outerRings) {
    for (let i = 0; i < r.length - 1; i++) {
      pts.push(r[i][0], r[i][1], 0, r[i + 1][0], r[i + 1][1], 0);
    }
  }
  const lg = res.add(new THREE.BufferGeometry());
  lg.setAttribute('position', new THREE.Float32BufferAttribute(pts, 3));
  const lm = reveal.track(
    res.add(
      new THREE.LineBasicMaterial({
        // 海岸線是島的輪廓，跟著整座島退到中景 —— 一樣走顏色而不是透明度。
        // 實際值每幀由 createScene 推（特寫時要跟著恢復）。
        color: color(pal.bg),
        transparent: true,
        depthWrite: false,
      }),
    ),
    pal.alpha.coast,
    focusPal.alpha.coast,
  );
  const coast = new THREE.LineSegments(lg, lm);
  coast.position.z = depth + 0.02;
  coast.raycast = () => null;
  parent.add(coast);

  return { coastMaterial: lm };
};
