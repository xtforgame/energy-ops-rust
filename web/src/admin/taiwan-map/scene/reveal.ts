/**
 * 進場淡入的登記簿 + 資源回收。
 *
 * 原作（§7 的 `track()` / `revealMats`）把每個材質的目標不透明度收在一個陣列裡，
 * 進場動畫再一次乘上進度。這裡保留同一套機制，順手把 geometry/material/texture
 * 收進 `Resources`，離場時一起 dispose —— WebGL 這層漏掉就是整張 context 帶著走
 * （切頁雲霧那次實測堆了兩張 canvas，上面那張直接 context lost）。
 */
import * as THREE from 'three';

export interface RevealEntry {
  m: THREE.Material;
  /** 背景模式的目標不透明度 */
  target: number;
  /** 特寫模式的目標不透明度（預設同 target ⇒ 不隨特寫變化） */
  focusTarget: number;
  /** true = 目標值寫進 `uniforms.uOpacity`（自訂 shader），false = 寫 `material.opacity` */
  uniform?: boolean;
}

/**
 * 淡入登記簿 —— 管**兩個獨立維度**的不透明度：
 *   `intro` 0→1  進場動畫（原作 §19 的 revealMats）
 *   `focus` 0→1  背景模式 → 特寫模式
 *
 * ⚠️ 特寫改變的只有**清晰度**（各層 alpha + 島的大氣透視），**不改配色**。
 *    曾經做過「整組 morph 成 VIOLET」，整片側牆變紫的干擾性太強；現在全場唯一會變紫的
 *    是被選中的那一個縣市（走 scan shader 的 uFocus，那是 per-county 的）。
 */
export class Reveal {
  private entries: RevealEntry[] = [];

  /** 一般材質：淡入 `material.opacity` */
  track<T extends THREE.Material>(m: T, target: number, focusTarget = target): T {
    m.transparent = true;
    m.opacity = 0;
    this.entries.push({ m, target, focusTarget });
    return m;
  }

  /** 自訂 shader：淡入 `uniforms.uOpacity` */
  uniform<T extends THREE.ShaderMaterial>(m: T, target: number, focusTarget = target): T {
    m.uniforms.uOpacity.value = 0;
    this.entries.push({ m, target, focusTarget, uniform: true });
    return m;
  }

  /** intro = 進場進度、focus = 特寫進度，兩者都是 0~1 */
  apply(intro: number, focus = 0) {
    for (const r of this.entries) {
      const v = (r.target + (r.focusTarget - r.target) * focus) * intro;
      if (r.uniform) (r.m as THREE.ShaderMaterial).uniforms.uOpacity.value = v;
      else r.m.opacity = v;
    }
  }
}

type Disposable = { dispose(): void };

export class Resources {
  private items: Disposable[] = [];

  add<T extends Disposable>(x: T): T {
    this.items.push(x);
    return x;
  }

  dispose() {
    for (const x of this.items) {
      try {
        x.dispose();
      } catch {
        /* 單一資源回收失敗不該擋住其他資源 */
      }
    }
    this.items.length = 0;
  }
}
