/**
 * 鏡面地板 —— 對應 Demo2 `mirror.tsx`、html 版 §15。
 *
 * ⭐ 這是把 html 版**唯一一處真的偷工減料**補回來的地方。
 *
 * 原專案用 drei 的 `<MeshReflectorMaterial blur={[400,100]} resolution={1024}
 * mixBlur={10} mixStrength={10} depthScale={1} />` —— 那是真的把場景重畫進一張
 * render target 的平面鏡。`temp/taiwan-scifi-map.html` 沒有做，改成「把縣市 mesh
 * 沿 y 翻面複製一份、塗成半透明青色」：所以那一版的倒影**只有縣市體塊**，
 * 光柱、菱錐、拖尾、外輪廓全都不會倒映，也沒有模糊與距離衰減。
 *
 * 這裡改用 three 內建的 `Reflector`（addons），它跟 MeshReflectorMaterial 是同一個
 * 原理：鏡射相機 + 斜截頭體裁切 + render target。差別是 drei 多做了模糊與深度混合，
 * 這裡用「距離衰減 + 反射率壓暗」取代 —— 白底地板本來就不該給到鏡面級的清晰倒影。
 *
 * 白底反轉：黑底上「亮的倒影」自然浮出；白底相反，倒影要**壓暗**才看得見
 * （uTint < 1，且帶一點紫偏移），否則白色的島倒映在白色地板上等於沒畫。
 */
import * as THREE from 'three';
import { Reflector } from 'three/examples/jsm/objects/Reflector.js';
import type { BuildCtx } from './ctx';
import { color, OUTPUT_COLORSPACE } from './glColor';

/**
 * 倒影的那一段。主題若 `reflect.mix <= 0` 就整段不編進 shader ——
 * 空的 `tDiffuse` 取樣雖然不會爆，但也沒必要留著。
 */
const REFLECTION_BLOCK = `
      // 真倒影：Reflector 把整個場景畫進 tDiffuse。renderer 走 alpha:true，
      // 沒畫到東西的地方 alpha = 0，所以用 alpha 當覆蓋率權重，不會把地板抹黑。
      vec4 refl = texture2DProj(tDiffuse, vUv);
      float k = clamp(refl.a * uMix * (1.0 - smoothstep(0.05, 0.55, rr)), 0.0, 1.0);
      vec3 col = mix(base, refl.rgb * uTint, k);
`;

const NO_REFLECTION_BLOCK = `
      float k = 0.0;
      vec3 col = base;
`;

const makeShader = (reflective: boolean) => ({
  name: 'TaiwanFloor',
  uniforms: {
    // ↓ 這三個是 Reflector 硬性要求的 uniform 名稱，不能改
    color: { value: null as THREE.Color | null },
    tDiffuse: { value: null as THREE.Texture | null },
    textureMatrix: { value: null as THREE.Matrix4 | null },
    // ↓ 自訂
    uLine: { value: new THREE.Color() },
    // 反射率：白底要把倒影**壓暗**才看得見（原作在黑底靠變亮）。實際值由 palette 決定
    uTint: { value: new THREE.Vector3(0.55, 0.5, 0.68) },
    uMix: { value: 0.7 },
    // 格線強度（細 / 粗），由 palette 決定：lab 版對齊 --azfw-grid-line 的 .045
    uGrid: { value: new THREE.Vector2(0.05, 0.12) },
    uOpacity: { value: 0 },
  },
  vertexShader: `
    uniform mat4 textureMatrix;
    varying vec4 vUv;
    varying vec2 vP;
    void main () {
      vUv = textureMatrix * vec4(position, 1.0);
      vP = position.xy;                              // 平面局部座標（旋轉前）＝地板網格用
      gl_Position = projectionMatrix * modelViewMatrix * vec4(position, 1.0);
    }`,
  fragmentShader: `
    uniform vec3 color;            // 地板底色
    uniform vec3 uLine;            // 網格線顏色
    uniform vec3 uTint;            // 倒影的反射率（<1＝壓暗，帶紫偏移）
    uniform float uMix, uOpacity;
    uniform vec2 uGrid;
    uniform sampler2D tDiffuse;
    varying vec4 vUv;
    varying vec2 vP;

    // ⚠️ 原作是 \`smoothstep(0.0, 0.035, abs(fract(p/s)-0.5))\` —— 距離沒有換回世界單位，
    //    所以線寬會跟著格距放大：10 單位的粗格畫出來是 0.7 單位寬的「帶」。黑底上那是
    //    一層淡淡的輝光看不出來，白底就是一條條刺眼的寬帶（實測滿版斜紋）。
    //    這裡把 f 乘回 s，線寬改成固定的世界寬度 w，格線才是 JV 語言要的 1px 細線。
    float grid (vec2 p, float s, float w) {
      vec2 f = abs(fract(p / s) - 0.5) * s;
      float d = min(f.x, f.y);
      return 1.0 - smoothstep(0.0, w, d);
    }

    void main () {
      // 兩個獨立的距離尺度（本島長邊約 5.3 單位）：
      //   rg —— 格線用，鋪到 ~42 單位，在斜視下自然收斂成一條地平線
      //         （對齊 futuristic-white-atmos 的滿版地格；這一層是「大氣」）
      //   rr —— 倒影用，只在島的周圍（超出島的範圍就收掉，免得遠處光柱的倒影
      //         被透視拉成長條）
      // 原作用單一的 34.0 配 90x90 的地板。
      float rg = length(vP) / 42.0;
      float rr = length(vP) / 15.0;
      float g = grid(vP, 1.0, 0.012) * uGrid.x + grid(vP, 5.0, 0.02) * uGrid.y;
      vec3 base = mix(color, uLine, g);

      ${reflective ? REFLECTION_BLOCK : NO_REFLECTION_BLOCK}

      // surface = 近處的實面（遮住地板下的光柱）；lines = 一路到地平線的格線。
      // 分開兩條衰減，格線才不會跟著實面一起在 17 單位處被切掉。
      float surface = mix(0.85, 0.0, smoothstep(0.06, 0.42, rg));
      float lines   = g * (1.0 - smoothstep(0.30, 1.0, rg));
      gl_FragColor = vec4(col, clamp(surface + lines * 0.9 + k * 0.5, 0.0, 1.0) * uOpacity);
      ${OUTPUT_COLORSPACE}
    }`,
});

export const buildFloor = (scene: THREE.Scene, ctx: BuildCtx) => {
  const { pal, reveal, res } = ctx;
  // reflect.mix <= 0 ⇒ 連 Reflector 都不建：省掉「整個場景再畫一遍」進 render target
  const reflective = pal.reflect.mix > 0;
  const shader = makeShader(reflective);
  const geo = res.add(new THREE.PlaneGeometry(90, 90));

  let floor: THREE.Mesh;
  let mat: THREE.ShaderMaterial;

  if (reflective) {
    const r = new Reflector(geo, {
      color: color(pal.floorBase),
      textureWidth: 1024,
      textureHeight: 1024,
      multisample: 4,
      shader: shader as any,
    });
    res.add({ dispose: () => r.dispose() });
    floor = r;
    mat = r.material as THREE.ShaderMaterial;
    mat.uniforms.uTint.value.set(...pal.reflect.tint);
    mat.uniforms.uMix.value = pal.reflect.mix;
  } else {
    mat = res.add(
      new THREE.ShaderMaterial({
        uniforms: THREE.UniformsUtils.clone(shader.uniforms as any),
        vertexShader: shader.vertexShader,
        fragmentShader: shader.fragmentShader,
      }),
    );
    mat.uniforms.color.value = color(pal.floorBase);
    // 沒有 Reflector 就沒人餵 textureMatrix，自己給一個單位矩陣（vUv 用不到）
    mat.uniforms.textureMatrix.value = new THREE.Matrix4();
    floor = new THREE.Mesh(geo, mat);
  }

  floor.rotation.x = -Math.PI / 2;
  floor.raycast = () => null;
  floor.renderOrder = -1;

  mat.transparent = true;
  mat.depthWrite = false;
  mat.uniforms.uLine.value = color(pal.floorLine);
  mat.uniforms.uGrid.value.set(pal.floorGrid[0], pal.floorGrid[1]);
  reveal.uniform(mat, 1);

  scene.add(floor);
  return floor;
};
