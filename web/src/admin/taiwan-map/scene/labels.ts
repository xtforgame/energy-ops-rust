/**
 * 縣市標籤 —— 對應 Demo2 `label.tsx`（drei `<Html>`）、html 版 §20。
 *
 * 一樣是把 3D 點投影回螢幕再擺 DOM。兩處差異：
 *   - 座標換算用容器尺寸，不是 `innerWidth/innerHeight`。
 *   - 白底版的標籤是**墨字 + 紫點**，不再需要黑色 text-shadow 把字從背景挖出來。
 *
 * 讓位規則（面積大的先佔位、太近的小縣市隱藏）原封不動照抄 —— 北部一團糊就是靠它。
 */
import * as THREE from 'three';
import type { County } from './counties';

export interface LabelLayer {
  root: HTMLDivElement;
  update(
    camera: THREE.PerspectiveCamera,
    hovered: County | null,
    focused: County | null,
    w: number,
    h: number,
  ): void;
  reveal(): void;
  dispose(): void;
}

export const createLabels = (
  container: HTMLElement,
  counties: County[],
  depth: number,
  /**
   * 標籤浮在頂面之上多高（地圖局部單位）。見 `palette.labelLift`。
   */
  lift: number,
  /** false = 只顯示被特寫選中的那一個（見 `palette.labelsAlwaysOn`） */
  alwaysOn: boolean,
): LabelLayer => {
  const root = document.createElement('div');
  root.className = 'tw-map-labels';
  container.appendChild(root);

  const nodes = new Map<County, HTMLDivElement>();
  for (const c of counties) {
    const el = document.createElement('div');
    el.className = 'tw-map-label';
    el.innerHTML = `<i class="dot"></i>${c.region.name}`;
    root.appendChild(el);
    nodes.set(c, el);
  }

  // 面積大的先佔位
  const order = counties.slice().sort((a, b) => b.region.area - a.region.area);
  const v3 = new THREE.Vector3();
  const placed: Array<[number, number]> = [];
  let timers: number[] = [];

  return {
    root,

    update(camera, hovered, focused, w, h) {
      placed.length = 0;
      const MIN_GAP = 34;
      for (const c of order) {
        const el = nodes.get(c)!;
        if (!alwaysOn && c !== focused) {
          el.style.visibility = 'hidden';
          continue;
        }
        v3.set(c.region.center.x, c.region.center.y, depth + lift);
        c.group.localToWorld(v3);
        v3.project(camera);
        if (v3.z > 1) {
          el.style.visibility = 'hidden';
          continue;
        }
        const sx = (v3.x * 0.5 + 0.5) * w;
        const sy = (-v3.y * 0.5 + 0.5) * h;
        let clash = false;
        // hover 與特寫的標籤一律優先佔位，不參與讓位判定 —— 那正是使用者要看的那一個
        if (c !== hovered && c !== focused) {
          for (const p of placed) {
            if (Math.abs(p[0] - sx) < MIN_GAP * 1.8 && Math.abs(p[1] - sy) < MIN_GAP * 0.55) {
              clash = true;
              break;
            }
          }
        }
        if (clash) {
          el.style.visibility = 'hidden';
          continue;
        }
        placed.push([sx, sy]);
        el.style.visibility = 'visible';
        el.style.transform = `translate(-50%,-50%) translate(${sx}px, ${sy}px)`;
        el.classList.toggle('hot', c === hovered);
        el.classList.toggle('sel', c === focused);
      }
    },

    reveal() {
      counties.forEach((c, i) => {
        timers.push(
          window.setTimeout(() => nodes.get(c)!.classList.add('on'), i * 45),
        );
      });
    },

    dispose() {
      timers.forEach(window.clearTimeout);
      timers = [];
      root.remove();
      nodes.clear();
    },
  };
};
