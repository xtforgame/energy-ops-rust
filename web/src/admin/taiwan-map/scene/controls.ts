/**
 * 軌道控制 —— 對應 html 版 §17（取代 drei 的 `<OrbitControls enableDamping />`）。
 *
 * 與 html 版的差別只有一處：座標一律換算成**容器相對**，不是 `innerWidth/innerHeight`。
 * 這頁的 canvas 只佔卡片的一塊，用視窗座標會讓拾取整個偏掉。
 */
import * as THREE from 'three';

export interface Controls {
  target: THREE.Vector3;
  sph: { r: number; phi: number; theta: number };
  goal: { r: number; phi: number; theta: number };
  min: number;
  max: number;
  minPhi: number;
  maxPhi: number;
  drag: boolean;
  px: number;
  py: number;
  auto: boolean;
  /** 進場動畫期間鎖住輸入 */
  lock: boolean;
  pointer: { x: number; y: number; has: boolean };
  /**
   * 定時運鏡（特寫用）。
   *
   * ⚠️ 不能沿用 `damp()` 的指數趨近 —— 那是 `x += (goal - x) * dt * k`，
   *    永遠只是「越來越靠近」，沒有明確的到達時間，起步也是最快的（沒有 ease-in），
   *    當成「運鏡」看就是一開始猛衝然後拖尾。特寫要的是有頭有尾的 ease-in-out，
   *    所以另外走一條計時 tween，期間直接驅動 `sph` / `target`，跳過阻尼。
   */
  flyTo(
    target: THREE.Vector3,
    to: { r: number; phi: number; theta: number },
    durationMs: number,
    /** 運鏡結束後要不要恢復自轉。解除特寫時要（回到總覽就該繼續慢慢轉） */
    resumeAuto?: boolean,
  ): void;
  /** 是否正在運鏡（運鏡中不自轉、不接受拾取的抖動） */
  flying(): boolean;
  apply(): void;
  damp(dt: number, revealed: boolean): void;
  dispose(): void;
}

/** 起步與收尾都慢、中段快 —— 「絲滑」的來源 */
const easeInOutCubic = (t: number) => (t < 0.5 ? 4 * t * t * t : 1 - Math.pow(-2 * t + 2, 3) / 2);

/** 角度取最短路徑，免得從 -3.0 轉到 3.0 時繞遠路 */
const shortestAngle = (d: number) => {
  const twoPi = Math.PI * 2;
  return ((((d + Math.PI) % twoPi) + twoPi) % twoPi) - Math.PI;
};

interface Tween {
  t: number;
  dur: number;
  resumeAuto: boolean;
  fromTarget: THREE.Vector3;
  toTarget: THREE.Vector3;
  from: { r: number; phi: number; theta: number };
  to: { r: number; phi: number; theta: number };
}

export const createControls = (el: HTMLElement, camera: THREE.PerspectiveCamera): Controls => {
  let tween: Tween | null = null;

  const c: Controls = {
    target: new THREE.Vector3(0, 0.45, 0),
    sph: { r: 30, phi: 0.3, theta: 0 },
    goal: { r: 30, phi: 0.3, theta: 0 },
    // 特寫要能貼很近，所以下限放到 2.2（原作 5.5 是給滿版總覽用的）
    min: 2.2,
    max: 30,
    maxPhi: 1.46,
    minPhi: 0.16,
    drag: false,
    px: 0,
    py: 0,
    auto: true,
    lock: true,
    pointer: { x: 0, y: 0, has: false },
    flyTo(target, to, durationMs, resumeAuto = false) {
      tween = {
        t: 0,
        dur: Math.max(1, durationMs),
        resumeAuto,
        fromTarget: c.target.clone(),
        toTarget: target.clone(),
        from: { ...c.sph },
        // theta 走最短路徑
        to: { r: to.r, phi: to.phi, theta: c.sph.theta + shortestAngle(to.theta - c.sph.theta) },
      };
      c.auto = false;
    },

    flying() {
      return tween !== null;
    },

    apply() {
      const { r, phi, theta } = c.sph;
      camera.position.set(
        c.target.x + r * Math.sin(phi) * Math.sin(theta),
        c.target.y + r * Math.cos(phi),
        c.target.z + r * Math.sin(phi) * Math.cos(theta),
      );
      camera.lookAt(c.target);
    },
    damp(dt, revealed) {
      if (tween) {
        tween.t += dt * 1000;
        const k = Math.min(1, tween.t / tween.dur);
        const e = easeInOutCubic(k);
        c.target.lerpVectors(tween.fromTarget, tween.toTarget, e);
        c.sph.r = tween.from.r + (tween.to.r - tween.from.r) * e;
        c.sph.phi = tween.from.phi + (tween.to.phi - tween.from.phi) * e;
        c.sph.theta = tween.from.theta + (tween.to.theta - tween.from.theta) * e;
        if (k >= 1) {
          // 交棒給阻尼：goal 對齊現值，否則會被瞬間拉回運鏡前的位置
          c.goal.r = c.sph.r;
          c.goal.phi = c.sph.phi;
          c.goal.theta = c.sph.theta;
          // ⚠️ 只有「解除特寫」會把自轉開回來。使用者自己拖過鏡頭時 auto 也是 false，
          //    那是刻意的（不跟他搶鏡頭），不能在這裡一併復原。
          if (tween.resumeAuto) c.auto = true;
          tween = null;
        }
        c.apply();
        return;
      }
      if (c.auto && !c.drag && revealed) c.goal.theta += dt * 0.035;
      c.sph.r += (c.goal.r - c.sph.r) * Math.min(1, dt * 4.5);
      c.sph.phi += (c.goal.phi - c.sph.phi) * Math.min(1, dt * 6);
      c.sph.theta += (c.goal.theta - c.sph.theta) * Math.min(1, dt * 6);
      c.apply();
    },
    dispose() {
      el.removeEventListener('pointerdown', onDown);
      el.removeEventListener('pointermove', onMove);
      el.removeEventListener('pointerleave', onLeave);
      window.removeEventListener('pointerup', onUp);
      el.removeEventListener('wheel', onWheel);
      el.removeEventListener('touchmove', onTouch);
      el.removeEventListener('touchend', onTouchEnd);
    },
  };

  /** 使用者一動手就取消運鏡，不要跟他搶鏡頭 */
  const cancelTween = () => {
    if (!tween) return;
    tween = null;
    c.goal.r = c.sph.r;
    c.goal.phi = c.sph.phi;
    c.goal.theta = c.sph.theta;
  };

  const onDown = (e: PointerEvent) => {
    if (c.lock) return;
    cancelTween();
    c.drag = true;
    c.auto = false;
    c.px = e.clientX;
    c.py = e.clientY;
    el.setPointerCapture(e.pointerId);
  };

  const onMove = (e: PointerEvent) => {
    const rect = el.getBoundingClientRect();
    c.pointer.x = ((e.clientX - rect.left) / rect.width) * 2 - 1;
    c.pointer.y = -((e.clientY - rect.top) / rect.height) * 2 + 1;
    c.pointer.has = true;
    if (!c.drag) return;
    c.goal.theta -= (e.clientX - c.px) * 0.005;
    c.goal.phi = Math.max(c.minPhi, Math.min(c.maxPhi, c.goal.phi - (e.clientY - c.py) * 0.004));
    c.px = e.clientX;
    c.py = e.clientY;
  };

  const onUp = () => {
    c.drag = false;
  };

  const onLeave = () => {
    c.pointer.has = false;
  };

  const onWheel = (e: WheelEvent) => {
    if (c.lock) return;
    e.preventDefault();
    cancelTween();
    c.auto = false;
    c.goal.r = Math.max(c.min, Math.min(c.max, c.goal.r * (1 + Math.sign(e.deltaY) * 0.09)));
  };

  let pinch = 0;
  const onTouch = (e: TouchEvent) => {
    if (e.touches.length !== 2 || c.lock) return;
    const d = Math.hypot(
      e.touches[0].clientX - e.touches[1].clientX,
      e.touches[0].clientY - e.touches[1].clientY,
    );
    if (pinch) {
      c.auto = false;
      c.goal.r = Math.max(c.min, Math.min(c.max, c.goal.r * (pinch / d)));
    }
    pinch = d;
  };
  const onTouchEnd = () => {
    pinch = 0;
  };

  el.addEventListener('pointerdown', onDown);
  el.addEventListener('pointermove', onMove);
  el.addEventListener('pointerleave', onLeave);
  window.addEventListener('pointerup', onUp);
  el.addEventListener('wheel', onWheel, { passive: false });
  el.addEventListener('touchmove', onTouch, { passive: true });
  el.addEventListener('touchend', onTouchEnd);

  return c;
};
