import * as THREE from 'three';

/**
 * ⚠️ 自訂 ShaderMaterial 的顏色慣例
 *
 * three 0.184 的 ColorManagement 會把 `new THREE.Color('#5B4A96')` 轉進 **linear working
 * space**；自訂 shader 若把 uniform 直接寫進 `gl_FragColor`，畫面就會暗掉一大截
 * （毛筆 logo 那次實測 [97,97,158] 變成 [29,29,83]）。
 *
 * 這裡的解法不是「不要轉」（NoColorSpace），而是**兩端都留在 linear**：
 *   - 顏色照常用 `new THREE.Color(hex)`（sRGB → linear）
 *   - 每支自訂 fragment shader 的最後一行加 `#include <colorspace_fragment>`
 *     （three 會注入 `linearToOutputTexel`，把 linear 轉回輸出色彩空間）
 *
 * 好處是鏡面倒影那張 render target 也是 linear（非 XR 的 RT 一律輸出 linear-sRGB），
 * 跟 shader 內的底色同一個空間，可以直接 mix，不必手刻 gamma。
 */
export const OUTPUT_COLORSPACE = '\n#include <colorspace_fragment>\n';

export const color = (hex: string) => new THREE.Color(hex);
