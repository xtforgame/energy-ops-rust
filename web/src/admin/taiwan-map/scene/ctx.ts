import type { MapPalette } from './palette';
import type { MapTextures } from './textures';
import type { Reveal, Resources } from './reveal';

/**
 * 各層 builder 共用的建構情境。
 *
 * 原本是把 palette / textures / reveal / res / depth 一個個當參數傳，加上第二套主題後
 * 參數列就爆了 —— 收成一個物件，順便讓「換主題只要換 `ctx.pal`」這件事變成型別上的事實
 * （沒有任何一層可以偷偷 import 全域色票）。
 */
export interface BuildCtx {
  pal: MapPalette;
  /**
   * 特寫模式要 morph 過去的配色。
   * MONO 指向 VIOLET —— 「特寫＝整組回到 VIOLET 的表現」是使用者定的規則；
   * VIOLET 自己指向自己，等於不 morph。
   */
  focusPal: MapPalette;
  tex: MapTextures;
  reveal: Reveal;
  res: Resources;
  /** 擠出厚度（CONF.DEPTH） */
  depth: number;
}
