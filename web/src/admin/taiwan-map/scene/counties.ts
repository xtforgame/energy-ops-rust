/**
 * 縣市體塊 —— 對應 Demo2 的 `base.tsx`(City) + `shape.tsx`(UV 重映射) +
 * `shaderMaterial.tsx`(側面掃描光帶)，即 html 版的 §7 / §9。
 *
 * 白底反轉（每一條都刻意，不是漏改）：
 *   1. 側牆漸層翻面：原作上亮下暗（uTop=#5fe3ff / uBottom=#08172c），
 *      白底改成上淡下深 —— 貼地那圈深色就是接觸陰影，體塊才會「站」在地板上。
 *   2. 掃描光帶從「比底色亮」改成「比底色濃」（飽和紫），白底上唯一的顏色。
 *   3. alpha 梯度翻面：原作 `0.78 + 0.22*h`（上緣實），白底改 `1.0 - 0.16*h`（下緣實）。
 *   4. hover 不再乘亮度（`col * (1.0 + uHighlight*0.5)`）—— 白底乘亮只會洗白，
 *      改成往掃描色多混一點 + 提高 alpha。
 *   5. 頂面材質從 metalness .55 / roughness .65 改成非金屬霧面：白底高金屬度會變成
 *      一片死白，細節全靠墨格貼圖撐。
 */
import * as THREE from 'three';
import type { GeoModel, Poly, Region } from './geo';
import type { BuildCtx } from './ctx';
import type { MapPalette } from './palette';
import { color, OUTPUT_COLORSPACE } from './glColor';

/** hover / 特寫時的抬升倍率（原作 CONF.HOVER_Z）。targetZ 由 updateCounties 統一推。 */
const HOVER_Z = 1.55;

export interface County {
  region: Region;
  group: THREE.Group;
  mesh: THREE.Mesh;
  topMat: THREE.MeshStandardMaterial;
  sideMat: THREE.ShaderMaterial;
  edge: THREE.LineSegments;
  /** hover 抬升的當前值 / 目標值（scale.z） */
  z: number;
  targetZ: number;
  /** hover 的平滑係數 0~1 */
  hot: number;
  cone?: THREE.Mesh;
  halo?: THREE.Mesh;
}

/** shaderMaterial.tsx —— 側牆漸層 + 由下往上掃的光帶 */
export const createScanMaterial = (depth: number, pal: MapPalette) =>
  new THREE.ShaderMaterial({
    transparent: true,
    side: THREE.DoubleSide,
    uniforms: {
      uTime: { value: 0 },
      uDepth: { value: depth },
      // ⛔ 島的配色**不隨特寫改變**。試過整組 morph 成 VIOLET，整片側牆變紫的干擾性太強
      //    —— 特寫要換的是「清晰度」（alpha 與大氣透視），不是色相。
      //    唯一允許變紫的是**被選中的那一個縣市**，走下面的 uFocus。
      uTop: { value: color(pal.sideTop) },
      uBottom: { value: color(pal.sideBottom) },
      uScan: { value: color(pal.scan) },
      // hover / 特寫的焦點色 —— 只作用在那一個縣市（uHighlight 是 per-county）
      uFocus: { value: color(pal.focus) },
      uFocusMix: { value: pal.focusMix },
      uOpacity: { value: 0 },
      uHighlight: { value: 0 },
      // 大氣透視：0 = 完全退成背景色，1 = 原色（見 palette.focusDepth）
      uHaze: { value: color(pal.bg) },
      uDepth2: { value: pal.focusDepth.island },
    },
    vertexShader: `
      varying vec3 vPosition;
      void main () {
        vPosition = position;
        gl_Position = projectionMatrix * modelViewMatrix * vec4(position, 1.0);
      }`,
    fragmentShader: `
      varying vec3 vPosition;
      uniform float uTime, uDepth, uOpacity, uHighlight, uFocusMix;
      uniform vec3  uTop, uBottom, uScan, uFocus, uHaze;
      uniform float uDepth2;
      void main () {
        float bandHeight = 0.45;
        float h = clamp(vPosition.z / uDepth, 0.0, 1.0);
        float progress = fract(uTime) * (1.0 + bandHeight) - bandHeight;
        float d = (progress + bandHeight) - h;
        float below = step(0.0, d);
        float within = clamp(1.0 - d / bandHeight, 0.0, 1.0) * below;
        float core = pow(smoothstep(0.0, 1.0, within), 1.5);
        float edge = smoothstep(0.0, 0.6, within) * (1.0 - smoothstep(0.6, 1.0, within));
        float s = core * 0.85 + edge * 0.45;
        if (h < 0.001 || h > 0.999) s = 0.0;

        vec3 base = mix(uBottom, uTop, pow(h, 0.85));
        vec3 col  = mix(base, uScan, clamp(s, 0.0, 1.0));
        col = mix(col, uFocus, uHighlight * uFocusMix);
        col = mix(uHaze, col, uDepth2);
        float a = (uOpacity + uHighlight * 0.22) * (1.0 - 0.16 * h);
        gl_FragColor = vec4(col, clamp(a, 0.0, 1.0));
        ${OUTPUT_COLORSPACE}
      }`,
  });

/**
 * 給 `MeshStandardMaterial` 注入大氣透視。
 *
 * 內建材質沒有這個開關，所以用 `onBeforeCompile` 在色彩空間轉換**之前**把顏色
 * 往背景色 mix（那個位置的 `gl_FragColor` 還在 linear，跟 `uHaze` 同一個空間）。
 * shader 物件存回材質上，之後每幀改 `uDepth2` 就好。
 */
const hazeStandardMaterial = (m: THREE.MeshStandardMaterial, pal: MapPalette) => {
  m.onBeforeCompile = (shader) => {
    shader.uniforms.uHaze = { value: color(pal.bg) };
    shader.uniforms.uDepth2 = { value: pal.focusDepth.island };
    shader.fragmentShader = `uniform vec3 uHaze;\nuniform float uDepth2;\n${shader.fragmentShader}`.replace(
      '#include <colorspace_fragment>',
      'gl_FragColor.rgb = mix(uHaze, gl_FragColor.rgb, uDepth2);\n#include <colorspace_fragment>',
    );
    (m as any).__shader = shader;
  };
};

const toShape = (poly: Poly) => {
  const s = new THREE.Shape(poly.outer.map((p) => new THREE.Vector2(p[0], p[1])));
  poly.holes.forEach((h) => s.holes.push(new THREE.Path(h.map((p) => new THREE.Vector2(p[0], p[1])))));
  return s;
};

/** shape.tsx：把 uv 依整張地圖的 bbox 重算，讓網格貼圖跨縣市連續 */
const remapUV = (geo: THREE.BufferGeometry, bbox: GeoModel['bbox']) => {
  const pos = geo.attributes.position;
  const w = bbox.maxX - bbox.minX;
  const h = bbox.maxY - bbox.minY;
  const uv = new Float32Array(pos.count * 2);
  for (let i = 0; i < pos.count; i++) {
    uv[i * 2] = (pos.getX(i) - bbox.minX) / w;
    uv[i * 2 + 1] = (pos.getY(i) - bbox.minY) / h;
  }
  geo.setAttribute('uv', new THREE.BufferAttribute(uv, 2));
};

export const buildCounties = (
  parent: THREE.Object3D,
  model: GeoModel,
  ctx: BuildCtx,
): { counties: County[]; pickables: THREE.Mesh[] } => {
  const { pal, focusPal, tex, reveal, res, depth } = ctx;
  const counties: County[] = [];
  const pickables: THREE.Mesh[] = [];

  for (const region of model.regions) {
    const shapes = region.polys.map(toShape);

    const geo = res.add(new THREE.ExtrudeGeometry(shapes, { depth, bevelEnabled: false }));
    remapUV(geo, model.bbox);

    const topMat = reveal.track(
      res.add(
        new THREE.MeshStandardMaterial({
          color: color(pal.topFace),
          map: tex.grid,
          emissive: color(pal.focus),
          emissiveIntensity: 0,
          metalness: 0,
          roughness: 0.85,
          side: THREE.DoubleSide,
        }),
      ),
      1,
    );
    hazeStandardMaterial(topMat, pal);
    const sideMat = reveal.uniform(
      res.add(createScanMaterial(depth, pal)),
      pal.alpha.sideWall,
      focusPal.alpha.sideWall,
    );

    // group0 = 上下蓋, group1 = 側牆（ExtrudeGeometry 的材質分組）
    const mesh = new THREE.Mesh(geo, [topMat, sideMat]);

    // 頂面輪廓線：白底上這條線才是「地圖」本體，比原作的作用重得多
    const edgeGeo = res.add(new THREE.EdgesGeometry(res.add(new THREE.ShapeGeometry(shapes)), 1));
    const edge = new THREE.LineSegments(
      edgeGeo,
      reveal.track(
        res.add(new THREE.LineBasicMaterial({ color: color(pal.edge), transparent: true })),
        pal.alpha.edge,
        focusPal.alpha.edge,
      ),
    );
    edge.position.z = depth + 0.008;
    edge.raycast = () => null;

    const group = new THREE.Group();
    group.add(mesh, edge);
    parent.add(group);

    const county: County = {
      region,
      group,
      mesh,
      topMat,
      sideMat,
      edge,
      z: 1,
      targetZ: 1,
      hot: 0,
    };
    mesh.userData.county = county;
    counties.push(county);
    pickables.push(mesh);
  }

  return { counties, pickables };
};

/** 每幀：hover 抬升 + 掃描光帶推進（原 §21 主迴圈裡的縣市段落） */
export const updateCounties = (
  counties: County[],
  hovered: County | null,
  /** 被特寫鎖定的縣市 —— 視覺上等同一個「黏著的 hover」（抬升 + 焦點色輪廓） */
  focused: County | null,
  dt: number,
  revealed: boolean,
  pal: MapPalette,
  /** 這一幀島層的景深基準（0=全退成背景色、1=原色）。特寫時由 createScene 拉到 1 */
  islandDepth: number,
) => {
  const bg = color(pal.bg);
  const edgeIdle = color(pal.edge);
  const edgeHot = color(pal.focus);
  /**
   * 焦點景深：hot=0 → 中景（× island）、hot=1 → 前景（×1）。
   * 淡入的目標值已經先乘過 island，所以 hot=0 時這裡算出來的正好等於淡入結束的值，
   * 進場動畫接到穩態不會跳。
   */
  const depthMul = (hot: number) => islandDepth + (1 - islandDepth) * hot;
  for (const c of counties) {
    // 抬升目標同樣吃 hover / 特寫（原作只有 hover）
    c.targetZ = c === hovered || c === focused ? HOVER_Z : 1;
    c.z += (c.targetZ - c.z) * Math.min(1, dt * 9);
    c.group.scale.z = c.z;
    c.sideMat.uniforms.uTime.value += dt / 3;

    const want = c === hovered || c === focused ? 1 : 0;
    c.hot += (want - c.hot) * Math.min(1, dt * 9);
    c.sideMat.uniforms.uHighlight.value = c.hot;
    c.topMat.emissiveIntensity = c.hot * 0.22;
    if (revealed) {
      const k = depthMul(c.hot);
      // ⭐ 景深走「顏色退向背景」，不動 opacity —— 動 opacity 會讓地板格線與
      //    其他縣市的掃描光帶透出來，反而更雜亂
      const sh = (c.topMat as any).__shader;
      if (sh) sh.uniforms.uDepth2.value = k;
      c.sideMat.uniforms.uDepth2.value = k;
      const em = c.edge.material as THREE.LineBasicMaterial;
      // hover 的縣市除了回到前景，輪廓線再多給一點銳利度
      em.opacity = Math.min(1, pal.alpha.edge + c.hot * 0.35);
      // 焦點色只在 hover 時滲進輪廓線 —— lab 版全場唯一的顏色就是這一條
      // 線條的「退場」同樣走顏色：先退向背景色，hover 再拉回並帶進焦點色
      em.color.copy(bg).lerp(edgeIdle, k).lerp(edgeHot, c.hot);
    }

    // 標記：常態關閉的主題（MONO）只畫被特寫的那一個 —— 直接切 visible，
    // 不是靠透明度藏，這樣其餘 21 個連 draw call 都不會發生。
    const showMarker = pal.markersAlwaysOn || c === focused;
    if (c.halo) {
      c.halo.visible = showMarker;
      if (showMarker) c.halo.rotation.z += dt * (0.5 + c.hot * 2.4);
    }
    if (c.cone) {
      c.cone.visible = showMarker;
      if (showMarker) {
        const u = c.cone.userData as { base: number; ph: number };
        u.ph += dt * 1.6;
        c.cone.position.z = u.base + Math.sin(u.ph) * pal.markerBob + c.hot * 0.25 * pal.markerScale;
        c.cone.rotation.y += dt * 1.1;
      }
    }
  }
};
