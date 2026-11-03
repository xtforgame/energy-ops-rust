/**
 * 上升光柱 —— 對應 Demo2 `beamLight.tsx`、html 版 §16。
 *
 * 白底反轉：原作 `gl_FragColor = vec4(uColor * b * 2.0, b * uOpacity)` —— 顏色乘 2
 * 是為了在黑底上過曝成光。白底不能這樣做（乘出去只會被 clamp 成白＝消失），
 * 改成顏色固定、只用 `b` 當 alpha，光柱變成一根根淡紫的直線。
 *
 * ⚠️ 白底上「光柱」這個東西本質上是矛盾的：黑底的加法光暈幾乎沒有面積感，
 *    換成實色遮蔽後同樣的參數會變成一片髒污的垂直抹痕（實測過）。所以除了換混合
 *    模式，還要一起收：不透明度 0.55 → 0.09、柱半徑 0.035 → 0.018、根數 26 → 18。
 *    收完的效果是「幾根極淡的直線」，那才是白底該有的量。
 */
import * as THREE from 'three';
import type { BuildCtx } from './ctx';
import { color, OUTPUT_COLORSPACE } from './glColor';
import { makeRng, MAP_SEED } from './rng';

export interface Beams {
  group: THREE.Group;
  range: number;
  rnd: () => number;
}

export const buildBeams = (scene: THREE.Object3D, ctx: BuildCtx): Beams => {
  const { pal, reveal, res } = ctx;
  const rnd = makeRng(MAP_SEED + 2);
  const range = 26;
  const n = 18;

  const mat = reveal.uniform(
    res.add(
      new THREE.ShaderMaterial({
        transparent: true,
        depthWrite: false,
        side: THREE.DoubleSide,
        uniforms: {
          uColor: { value: color(pal.bgInkSoft) },
          uOpacity: { value: 0 },
        },
        vertexShader: `
          varying vec2 vUv;
          void main () { vUv = uv;
            gl_Position = projectionMatrix * modelViewMatrix * vec4(position, 1.0); }`,
        fragmentShader: `
          uniform vec3 uColor; uniform float uOpacity; varying vec2 vUv;
          void main () {
            float s = pow(1.0 - abs(vUv.x - 0.5) * 2.0, 2.0);
            float v = pow(sin(vUv.y * 3.14159), 0.5);
            float b = s * v;
            gl_FragColor = vec4(uColor, b * uOpacity);
            ${OUTPUT_COLORSPACE}
          }`,
      }),
    ),
    pal.alpha.beam,
  );

  const geo = res.add(new THREE.CylinderGeometry(0.018, 0.018, 1, 6, 1, true));
  const group = new THREE.Group();
  for (let i = 0; i < n; i++) {
    const m = new THREE.Mesh(geo, mat);
    m.position.set((rnd() - 0.5) * range, 4 - rnd() * 6, (rnd() - 0.5) * range);
    m.scale.y = 2 + rnd() * 4;
    m.userData = { speed: 1.8 + rnd() * 1.6, reset: 11 + rnd() * 16 };
    m.raycast = () => null;
    group.add(m);
  }
  scene.add(group);
  return { group, range, rnd };
};

export const updateBeams = ({ group, range, rnd }: Beams, dt: number) => {
  for (const b of group.children) {
    const u = b.userData as { speed: number; reset: number };
    b.position.y += u.speed * dt;
    if (b.position.y > u.reset) {
      b.position.x = (rnd() - 0.5) * range;
      b.position.z = (rnd() - 0.5) * range;
      b.position.y = -1 - rnd() * 5;
      b.scale.y = 2 + rnd() * 4;
    }
  }
};
