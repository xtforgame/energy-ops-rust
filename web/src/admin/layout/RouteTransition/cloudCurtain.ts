import cloudTextureAsset from './assets/cloud.png';

// energy-ops-next：Next.js 的圖片 import 回的是 { src }，Vite 回字串。
const cloudTextureUrl: string = typeof cloudTextureAsset === 'string' ? cloudTextureAsset : (cloudTextureAsset as any).src;

/**
 * 雲幕 —— **完整重現** `idea/cloud.html`（原作 wuyifan0203「天空云」）。
 *
 * ⚠️ 這個檔案的立場是「照搬」，不是「參考後重寫」。
 * 先前試著自己造雲（程序化貼圖、拿掉天空、改成橫向掃動）三個環節同時改，
 * 結果又不像 demo、又查不出是哪一段搞砸的。所以先原樣重現，確認能重現之後，
 * 動作與變化再往上疊。
 *
 * 與 demo 相同（⛔ 不要「順手改良」，改了就不叫重現）：
 *   - 相同的 vertex / fragment shader，含原作那行對尚未初始化的 gl_FragColor
 *     做乘法的寫法 —— 重現就要照留，不要「修正」它
 *   - 相同參數：count 800、PlaneGeometry(64,64)、fov 30、far = count*1.5、
 *     fogColor #4584b4、fogNear -100、fogFar 3000
 *   - 相同雲分布：x ∈ [-500,500]、y = -rnd*rnd*200-15、**z = j（索引）**、
 *     rotation.z = rnd*PI、scale = rnd*rnd*1.5+0.5
 *   - 相同的天空漸層背景（#1e4877 → #4584b4）。實測確認 demo 的層次感幾乎全部
 *     來自這片藍天：雲本身對比極低，好看的是白雲對藍天、以及遠處雲融進藍霧。
 *   - 相同的飛行：camera.position.z = -((now - start) * 0.03) % count + count
 *
 * 與 demo 不同的只有「當轉場用」必須改的：
 *   - 沒有 dat.GUI、沒有 stats、沒有滑鼠視差
 *   - 尺寸跟著內容區而不是整個視窗
 *   - 多了 start()/stop()，不轉場時不要空轉 rAF
 *   淡入淡出由 CloudTransition 控制 host 的 CSS opacity，不動場景本身。
 *
 * three 是動態 import：只有真的選 mode='cloud' 才會載入這個 chunk。
 */

export interface CloudCurtainOptions {
  /**
   * sprite 數量，維持 demo 的 800。
   *
   * ⛔ 別指望調高 count 能把雲補得更密：demo 的 `z = j` 讓 count 同時決定**走廊深度**，
   * 相機也退到 `z = count`，所以單位體積的密度是不變的。實測 800 → 2400，遮蔽率幾乎
   * 沒有改善（最好的那一列 0.075 → 0.154），只是白白多三倍的 draw 成本。
   */
  count?: number;
  /**
   * 橫向掃動的半幅（世界座標）。相機沿 +x 等速移動，雲相對地由左往右掠過。
   *
   * 因為雲鋪在 z = 0..count 的深度上，近的雲位移大、遠的位移小，視差是免費的
   * （實測位移比 4.11 : 距離比 4.11，符合透視定律）。
   *
   * ⚠️ 這個值與 CloudTransition 的 `durationMs` 一起決定相機的**視覺速度**
   * （速度 = 2·sweepHalf / duration）。目前 sweepHalf 73 ↔ duration 900ms。
   *
   * 這裡刻意**不等比例**跟 duration 綁死：從 146 砍到 73 是為了讓鏡頭移動量減半、
   * 位移感更收斂，時間長度不動。遮蔽不受影響 —— 換頁是靠 backdrop 底色保證的，
   * 不是靠掃動距離（實測砍半後全遮期一樣是 0% 文字像素）。
   *
   * ⛔ 不要單獨調大：雲只鋪在 x ∈ [-500, 500]，掃過頭會把構圖甩出雲帶。
   * ⚠️ 若把 `maskCount` 開回非 0，改這個值一定要重算 `maskScale`（見下方公式，
   *    分母是相機總行程，行程砍半窗口就翻倍）。
   */
  sweepHalf?: number;
  /**
   * 遮罩雲：刻意安排一團**大到吃滿畫面**的雲，讓它在掃動正中間（t=0.5）通過鏡頭前，
   * 換頁就藏在那一刻。給 0 就不放。
   *
   * ⭐ 預設 0（不放）：backdrop 底色本來就保證換頁被遮住，這團雲純屬視覺；
   * 實際看下來它在中段把畫面吃成一片白，反而蓋掉雲原野的層次。要找回它，
   * 定案參數是 maskCount 70 / maskZ 718 / maskScale 68（全遮 ~200ms、峰值在 p=0.50）。
   *
   * 尺寸不要用猜的，有公式：
   *   深度 d 的可視全寬 = 2·tan(fov/2)·d·aspect
   *   sprite 邊長 = k × 可視全寬 時，相機還能再橫移 (k−1)×可視全寬 而不露出邊緣
   *   全遮佔比 = (k−1)×可視全寬 ÷ 相機總行程(2·sweepHalf)
   * ⭐ 要的是「**很近**的一般大小的雲」，不是「很遠的超大雲」。兩者都能填滿畫面，
   *   但窗口長度差很多：
   *     窗口佔比 = (k−1)·2·tan(fov/2)·d·aspect ÷ (2·sweepHalf)
   *   d 越小，同樣的 k 佔的時間越短。放遠又放大（maskZ 500 / maskScale 320）的結果是
   *   一片超大的雲從頭蓋到尾，**原本的層次感整個消失**，實測峰值還跑到 p=0.40。
   * 尺寸由兩個條件夾出來（W = 該深度的可視全寬、S = sprite 邊長、travel = 2·sweepHalf）：
   *   進場到出場 = (S+W)/travel  ——「這片雲只佔中間 0.2~0.25 秒」
   *   完全遮住需要 S > W
   * 900ms 的轉場、目標 0.2~0.25s → S+W ≈ 81~103。
   * 實測 maskZ 740 / maskScale 48 給 180ms（略短），放大到 maskZ 718 / maskScale 60。
   * 量法：把 maskCount=0 與正常值的覆蓋率曲線相減，隔離出遮罩雲自己的貢獻 —— 直接看
   * 總覆蓋率會被背景那層雲的起伏蓋掉，看不出這片雲何時進場出場。
   * ⚠️ 改 sweepHalf 一定要重算這裡：分母是相機總行程，行程砍半窗口就翻倍。
   * ⚠️ 數量也不能少：cloud.png 大部分面積的 alpha 很低（它本來就是靠疊層堆出體積），
   *   14 片疊起來實測只到 0.5 左右，中段根本吃不滿。要靠 maskCount 把 alpha 疊上去。
   * ⚠️ 改了 sweepHalf 就要重算 maskScale，否則全遮期會跟著爆掉或消失。
   */
  maskCount?: number;
  maskZ?: number;
  maskScale?: number;
  /** 佈雲用的亂數種子。固定 seed = 每次轉場的雲一模一樣，遮蔽才可預期。 */
  seed?: number;
  /**
   * 背景漸層的兩個色階（上 → 下）。給 `null` 就不畫背景。
   *
   * ⭐ 這個東西同時決定三件事，換色之前先想清楚：
   *   1. **遮蔽**：有背景 → canvas 不透明 → 轉場期間頁面必定被完全遮住；
   *      給 null → canvas 透明 → 雲本身遮不滿，換頁會從雲隙被看見（實測 min 0.008）。
   *   2. **雲看不看得見**：雲是近白色的，需要比它暗的底才有對比。demo 的藍天就是
   *      它的層次感來源；白底配白雲等於隱形。
   *   3. **像不像天空**：用 demo 的 #1e4877 → #4584b4 就是一片藍天；改用頁面自己的
   *      底色壓暗一點，讀起來就只是「頁面淡入一片霧」。
   */
  backdrop?: { top: string; bottom: string } | null;
}

export interface CloudCurtain {
  canvas: HTMLCanvasElement;
  /** 開始飛行（rAF 迴圈） */
  start: () => void;
  stop: () => void;
  /** 運鏡進度：0 = 相機在最左（雲偏右），1 = 相機在最右（雲已掠到左邊） */
  setProgress: (p: number) => void;
  /**
   * 就地換掉背景漸層（給 null 就變透明）。
   * ⭐ 換主題時務必用這個，不要重建整個雲幕：重建要重載貼圖、重算 800 個 instance，
   * 而且舊的 canvas 還得記得從 DOM 拔掉（曾經漏掉，結果 host 裡疊了兩個 canvas，
   * 上面那個是 context 已經 lost 的死畫布）。
   */
  setBackdrop: (colors: { top: string; bottom: string } | null) => void;
  resize: (width: number, height: number) => void;
  /** 量測用（唯讀）：這一幀畫面上有多少像素是不透明的 */
  sampleCoverage: (grid?: number) => { min: number; mean: number; leakRatio: number };
  /** 量測用（唯讀）：讀畫面上某一點的 RGBA，用來跟 demo 的 ground truth 逐點對照 */
  samplePixel: (u: number, v: number) => [number, number, number, number];
  /** 量測用：把場景內部物件開給探針檢查（貼圖有沒有載到、相機在哪） */
  debug: Record<string, any>;
  dispose: () => void;
}

// --- 以下 shader 與 demo 逐字相同 ---
const vs = /* glsl */ `
  varying vec2 vUv;
  void main(){
    vUv = uv;
    gl_Position = projectionMatrix * modelViewMatrix * instanceMatrix * vec4(position, 1.0);
  }
  `;

const fs = /* glsl */ `
  varying vec2 vUv;
  uniform sampler2D map;
  uniform float fogNear;
  uniform float fogFar;
  uniform vec3 fogColor;
  uniform int enableFog; // 0: false, 1: true

  void main(){
    if(enableFog == 1){
      // 计算片源深度
      float depth = gl_FragCoord.z / gl_FragCoord.w;
      // 计算归一化的深度
      float fogFactor = smoothstep(fogNear, fogFar, depth);
      // 计算雾透明度
      gl_FragColor.w *= pow(gl_FragCoord.z, 20.0);
      // 最终结果
      gl_FragColor = mix(texture2D(map, vUv), vec4(fogColor, gl_FragColor.w), fogFactor);
    }else{
      gl_FragColor = texture2D(map, vUv);
    }
  }
  `;

/**
 * ⚠️ 對 demo 的**刻意偏離**：原作用 `Math.random()` 佈雲，所以每次載入都是不同的場景。
 * 當背景動畫無所謂，但當轉場就不行 —— 實測同一份程式碼連跑三次，正中間的雲覆蓋率
 * 是 0.301 / 0.654 / 0.653，「中間一定被雲遮住」這件事根本無法保證，調參也無從調起。
 * 改成固定 seed 的 mulberry32：分布公式與 demo 逐字相同，只是亂數源可重現。
 */
const makeRandom = (seed: number) => {
  let a = seed >>> 0;
  return () => {
    a += 0x6d2b79f5;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
};

export const createCloudCurtain = async ({
  count = 800,
  sweepHalf = 73,
  maskCount = 0,
  maskZ = 718,
  maskScale = 68,
  seed = 20260822,
  backdrop = { top: '#1e4877', bottom: '#4584b4' },
}: CloudCurtainOptions = {}): Promise<CloudCurtain> => {
  const THREE = await import('three');

  // demo: initRenderer()（沒有背景時改成透明，讓頁面透出來）
  // alpha 一律開著：有 scene.background 時背景會畫成整屏不透明的 quad，遮蔽照樣是滿的；
  // 沒有背景時就透出頁面。這樣換底色不必重建 renderer。
  const renderer = new THREE.WebGLRenderer({ antialias: true, alpha: true });
  renderer.shadowMap.enabled = true;
  renderer.shadowMap.type = THREE.PCFSoftShadowMap;
  renderer.setPixelRatio(window.devicePixelRatio);
  renderer.setClearColor(0x000000, 0);

  const scene = new THREE.Scene();
  const camera = new THREE.PerspectiveCamera(30, 1, 1, count * 1.5);

  // demo: background —— 保留原作「不設 width/height」的寫法（canvas 預設 300x150）
  // 背景 canvas / texture 只建一次、就地重畫 —— setBackdrop 會被轉場逐幀呼叫
  //（底色壓暗量跟著 opacity 走，見 CloudTransition），每幀重建 texture 太浪費。
  let bgCanvas: HTMLCanvasElement | null = null;
  let bgTexture: any = null;
  const setBackdrop = (colors: { top: string; bottom: string } | null) => {
    if (!colors) {
      if (bgTexture) { bgTexture.dispose(); bgTexture = null; bgCanvas = null; }
      scene.background = null;
      return;
    }
    if (!bgCanvas) {
      bgCanvas = document.createElement('canvas');
      bgTexture = new THREE.CanvasTexture(bgCanvas);
    }
    const ctx = bgCanvas.getContext('2d');
    if (ctx) {
      const gradient = ctx.createLinearGradient(0, 0, 0, bgCanvas.height);
      gradient.addColorStop(0, colors.top);
      gradient.addColorStop(0.5, colors.bottom);
      ctx.fillStyle = gradient;
      ctx.fillRect(0, 0, bgCanvas.width, bgCanvas.height);
    }
    bgTexture.needsUpdate = true;
    scene.background = bgTexture;
  };
  setBackdrop(backdrop);

  // demo: cloud texture（原作從 CDN 抓，這裡改成本地資產，內容是同一張圖）
  const loader = new THREE.TextureLoader();
  const cloudTexture = await loader.loadAsync(cloudTextureUrl);

  const geometry = new THREE.PlaneGeometry(64, 64);
  const material = new THREE.ShaderMaterial({
    uniforms: {
      map: { value: cloudTexture },
      // ⚠️ NoColorSpace：three 0.184 的 ColorManagement 會把 uniform 顏色線性化，
      //    RAW ShaderMaterial 又不會在輸出端轉回 sRGB → 霧色偏暗。demo 的 r128 沒有
      //    這回事（先前逐點對照時雲區一直比 demo 暗 15~20，就是這個造成的）。
      fogColor: { value: new THREE.Color().setStyle('#4584b4', THREE.NoColorSpace) },
      fogNear: { value: -100 },
      fogFar: { value: 3000 },
      enableFog: { value: 1 },
    },
    vertexShader: vs,
    fragmentShader: fs,
    depthWrite: false,
    depthTest: false,
    transparent: true,
  });

  const mesh = new THREE.InstancedMesh(geometry, material, count);
  scene.add(mesh);

  // demo: updateMeshCount()（分布公式與 demo 相同，亂數源換成固定 seed）
  const rnd = makeRandom(seed);
  const dummy = new THREE.Object3D();
  for (let j = 0; j < count; j += 1) {
    dummy.position.x = rnd() * 1000 - 500;
    dummy.position.y = -rnd() * rnd() * 200 - 15;
    dummy.position.z = j;
    dummy.rotation.z = rnd() * Math.PI;
    dummy.scale.x = rnd() * rnd() * 1.5 + 0.5;
    dummy.scale.y = dummy.scale.x;
    dummy.updateMatrix();
    mesh.setMatrixAt(j, dummy.matrix);
  }
  // --- 遮罩雲：覆寫 z ≈ maskZ 附近那幾個 instance ---
  // ⭐ 一定要「覆寫既有索引」而不是往後追加：demo 的 z = j 讓索引順序 = 由遠到近的
  //    繪製順序，配合 depthTest:false 才是正確的畫家演算法。追加在最後面會讓這團雲
  //    永遠畫在最上層，穿幫。
  if (maskCount > 0) {
    const first = Math.max(0, Math.min(count - maskCount, Math.round(maskZ) - Math.floor(maskCount / 2)));
    for (let i = 0; i < maskCount; i += 1) {
      const j = first + i;
      // 圍繞視線中心（相機從 x=0、y=0 通過），小幅 jitter 讓它是一團雲不是一塊板
      const ring = i / maskCount;
      // jitter 壓小：亂數偏移會把整團雲的重心推離視線中心，全遮的那一小段就會
      // 偏離 t=0.5（實測 ±40 的 jitter 讓峰值跑到 0.55）。
      // +17：雲團的實際重心會被 jitter 與環的取樣推偏，往右補這麼多才讓遮蔽峰值
      // 落在 p=0.50。⚠️ 改 maskCount / maskScale 都會動到重心，改了就要重量。
      dummy.position.x = Math.cos(ring * Math.PI * 2) * maskScale * 0.20 + (rnd() - 0.5) * 12 + 17;
      dummy.position.y = Math.sin(ring * Math.PI * 2) * maskScale * 0.15 + (rnd() - 0.5) * 12;
      dummy.position.z = j;
      dummy.rotation.z = rnd() * Math.PI;
      const sc = (maskScale / 64) * (0.85 + rnd() * 0.3);
      dummy.scale.x = sc;
      dummy.scale.y = sc;
      dummy.updateMatrix();
      mesh.setMatrixAt(j, dummy.matrix);
    }
  }

  mesh.instanceMatrix.needsUpdate = true;
  camera.position.z = count;
  camera.far = count * 1.5;
  camera.updateProjectionMatrix();

  // ⚠️ three 的 setSize 是無條件 `canvas.width = ...` —— 就算同尺寸，對 canvas
  //    尺寸賦值也會清空 drawing buffer。呼叫端（fitToContent）每幀都會叫 resize，
  //    不擋掉 no-op 的話 buffer 會被反覆清掉，凍結後畫面直接消失（實測就是
  //    「結尾閃一下」的來源之一）。真的變了就立刻補渲染一幀，不留空白 buffer。
  let lastW = -1;
  let lastH = -1;
  const resize = (w: number, h: number) => {
    const width = Math.max(1, Math.round(w));
    const height = Math.max(1, Math.round(h));
    if (width === lastW && height === lastH) return;
    lastW = width; lastH = height;
    renderer.setSize(width, height, false);
    camera.aspect = width / height;
    camera.updateProjectionMatrix();
    renderer.render(scene, camera);
  };
  resize(window.innerWidth, window.innerHeight);

  // demo: render() —— 相機一路往前推進穿過雲層，到底繞回去
  let raf = 0;
  let startTime = 0;
  const renderFrame = () => {
    camera.position.z = -((Date.now() - startTime) * 0.03) % count + count;
    renderer.render(scene, camera);
    raf = requestAnimationFrame(renderFrame);
  };

  // ⭐ 疊在重現之上的動作：相機走「左上 → 右下」的斜線
  //    x：往右移 → 雲由右往左掠過（近雲快、遠雲慢，視差免費）
  //    y：由上往下穿過雲毯 → 中段整片被雲吃掉，換頁藏在那裡
  const setProgress = (p: number) => {
    const t = Math.min(1, Math.max(0, p));
    // 單純由左往右、等速。y 保持在 demo 的預設 0（相機在雲毯之上）。
    //
    // ⛔ 不要為了製造全遮期而讓相機在中段變慢或停住。試過 `0.5+0.5*(2t-1)^3`
    //    這種「兩端快、中段慢」的曲線，中點速度會掉到 0 —— 看起來就是
    //    「動 → 停 → 動」的三段式，一眼就被挑出來。
    camera.position.x = (t - 0.5) * 2 * sweepHalf;
  };
  setProgress(0);

  const start = () => {
    if (raf) return;
    startTime = Date.now();
    raf = requestAnimationFrame(renderFrame);
  };
  const stop = () => {
    if (!raf) return;
    cancelAnimationFrame(raf);
    raf = 0;
  };

  const sampleCoverage = (grid = 16) => {
    renderer.render(scene, camera);
    const gl = renderer.getContext();
    const bw = renderer.domElement.width;
    const bh = renderer.domElement.height;
    const pixels = new Uint8Array(bw * bh * 4);
    gl.readPixels(0, 0, bw, bh, gl.RGBA, gl.UNSIGNED_BYTE, pixels);
    let min = 1;
    let total = 0;
    let leaks = 0;
    let n = 0;
    for (let gy = 0; gy < grid; gy += 1) {
      for (let gx = 0; gx < grid; gx += 1) {
        const px = Math.floor(((gx + 0.5) / grid) * bw);
        const py = Math.floor(((gy + 0.5) / grid) * bh);
        const alpha = pixels[(py * bw + px) * 4 + 3] / 255;
        min = Math.min(min, alpha);
        total += alpha;
        if (alpha < 0.9) leaks += 1;
        n += 1;
      }
    }
    return { min, mean: total / n, leakRatio: leaks / n };
  };

  const dispose = () => {
    stop();
    // ⛔ 一定要把 canvas 從 DOM 拔掉。只 dispose 不移除的話，重建時新舊兩個 canvas
    //    會疊在 host 裡，上面那個是 context 已經 lost 的死畫布（換主題時實際踩過）。
    renderer.domElement.remove();
    if (bgTexture) bgTexture.dispose();
    geometry.dispose();
    material.dispose();
    cloudTexture.dispose();
    mesh.dispose();
    renderer.dispose();
    renderer.forceContextLoss();
  };

  const samplePixel = (u: number, v: number): [number, number, number, number] => {
    renderer.render(scene, camera);
    const gl = renderer.getContext();
    const bw = renderer.domElement.width;
    const bh = renderer.domElement.height;
    const px = Math.min(bw - 1, Math.max(0, Math.floor(u * bw)));
    // readPixels 的原點在左下角，畫面座標是左上角，要翻轉
    const py = Math.min(bh - 1, Math.max(0, Math.floor((1 - v) * bh)));
    const out = new Uint8Array(4);
    gl.readPixels(px, py, 1, 1, gl.RGBA, gl.UNSIGNED_BYTE, out);
    return [out[0], out[1], out[2], out[3]];
  };

  return {
    canvas: renderer.domElement,
    start,
    stop,
    setProgress,
    setBackdrop,
    resize,
    sampleCoverage,
    samplePixel,
    debug: {
      scene, camera, mesh, material, cloudTexture, renderer,
    },
    dispose,
  };
};

export default createCloudCurtain;
