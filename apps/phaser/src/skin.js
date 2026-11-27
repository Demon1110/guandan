// 主题皮肤管理：加载 .gfdeck（强制验签）、缓存 wasm 合成的牌面 SVG、登记为 Phaser 纹理。
// 主题是硬依赖：加载/验签失败返回 {ok:false}，由 main.js 中止启动，不回退自画字卡。
// 支持运行时热切换：wasm 的 themeLoad 是全局替换式，切换时旧纹理/SVG 缓存全部失效。
import { faceCode, rankCode } from './cards.js';

const PUBKEY_HEX = 'd04ab232742bb4ab3a1368bd4615e4e6d0224ab71a016baf8520a332c9778737';
const SUITS = ['S', 'H', 'D', 'C'];
const RANKS = ['2', '3', '4', '5', '6', '7', '8', '9', '10', 'J', 'Q', 'K', 'A'];
const ALL_CODES = [
  ...SUITS.flatMap((s) => RANKS.map((r) => s + r)),
  'JOKER-S', 'JOKER-B',
];

// 主题清单：新增主题只需在这里加一项 + 把 .gfdeck 放到 pkgs/。
// cornerPct 不写死，加载后从主题元数据 cornerScale×100 自动取（squabby=250、defaults=220）。
export const THEMES = {
  squabby: { url: 'pkgs/squabby.gfdeck', label: '肥仔' },
  defaults: { url: 'pkgs/defaults.gfdeck', label: '默认' },
};
export const THEME_ORDER = ['squabby', 'defaults'];
export const DEFAULT_THEME = 'squabby';
const STORAGE_KEY = 'gf-theme';

function hexToBytes(hex) {
  const out = new Uint8Array(hex.length / 2);
  for (let i = 0; i < out.length; i += 1) out[i] = parseInt(hex.slice(i * 2, i * 2 + 2), 16);
  return out;
}

export class Skin {
  constructor(wasm) {
    this.wasm = wasm;
    this.scene = null;
    this.loaded = false;
    this.meta = null;
    this.themeId = null;
    this.suitInks = null; // wasm 导出的四花色墨色 [#RRGGBB ×4]，与牌面 suit_ink 一致
    this.cornerPct = 220; // 加载后由主题元数据覆盖
    this.svgCache = new Map(); // key -> svg|null
    this.texKeys = new Set(); // 已登记的纹理键（带主题前缀）
    this.backKey = null; // 牌背纹理键（主题加载后登记）
  }

  // 选初始主题：URL ?theme= 优先，其次 localStorage，最后默认 squabby。
  static initialThemeKey() {
    const fromUrl = new URLSearchParams(location.search).get('theme');
    if (fromUrl && THEMES[fromUrl]) return fromUrl;
    try {
      const saved = localStorage.getItem(STORAGE_KEY);
      if (saved && THEMES[saved]) return saved;
    } catch { /* 隐私模式下 localStorage 不可用，忽略 */ }
    return DEFAULT_THEME;
  }

  // 加载/切换主题。themeKey 必须在 THEMES 清单里。返回 {ok, id, label}。
  async load(scene, themeKey = DEFAULT_THEME) {
    const spec = THEMES[themeKey];
    if (!spec) return { ok: false, id: this.themeId, label: null, error: '未知主题' };
    this.scene = scene;
    try {
      const res = await fetch(spec.url);
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      const pack = new Uint8Array(await res.arrayBuffer());
      const r = this.wasm.themeLoad(pack, hexToBytes(PUBKEY_HEX));
      if (!r || !r.ok) {
        console.error('[skin] 主题加载失败（中止启动）：', r?.error ?? '无导出');
        return { ok: false, id: this.themeId, label: null, error: r?.error ?? '无导出' };
      }
      // 主题切换：清掉上一主题的纹理与 SVG 缓存，避免纹理键串味。
      if (this.themeId && this.themeId !== themeKey) this.clearCache();
      this.themeId = themeKey;
      this.meta = r;
      this.loaded = true;
      // 四花色墨色由 wasm 从 palette.suits（或 red/black 回落）算出，UI 直接用它着色。
      this.suitInks = Array.isArray(r.suitInks) && r.suitInks.length === 4 ? r.suitInks.slice() : null;
      // 角标倍率跟随主题声明（squabby 2.5→250，defaults 2.2→220），未声明回退 220。
      const scale = Number(r.cornerScale);
      this.cornerPct = Number.isFinite(scale) && scale > 0 ? Math.round(scale * 100) : 220;
      // 牌背：从 gfdeck 取 SVG 并登记纹理（主题硬依赖，失败即中止）。
      this.backKey = await this.registerBack();
      if (!this.backKey) {
        return { ok: false, id: themeKey, label: null, error: '牌背素材缺失（gfdeck 未提供可用 back.svg）' };
      }
      try { localStorage.setItem(STORAGE_KEY, themeKey); } catch { /* 忽略 */ }
      console.info(`[skin] 主题 ${r.themeId}@${r.version}「${r.name}」已载入，角标 ${this.cornerPct}%`);
      return { ok: true, id: themeKey, label: spec.label };
    } catch (e) {
      console.error('[skin] 主题加载异常（中止启动）：', e.message);
      return { ok: false, id: this.themeId, label: null, error: e.message };
    }
  }

  // 切换主题时只清 JS 侧映射，不删除 Phaser 纹理对象：
  // 纹理键已带 themeId + cornerPct，天然隔离；删除纹理会让 WebGL 渲染器持有失效引用
  // （glTexture null 异常）。旧纹理留在纹理管理器里，切回时按同名键直接复用。
  clearCache() {
    this.texKeys.clear();
    this.svgCache.clear();
    this.backKey = null; // 随后 load 会 registerBack 重设
  }

  texKey(code, level, pct) {
    return `card_${this.themeId}_${code}_${level}_${pct}`;
  }

  ensureSvg(code, level, pct) {
    const key = `${this.themeId}@${code}@${level}@${pct}`;
    if (this.svgCache.has(key)) return this.svgCache.get(key);
    let svg = null;
    try {
      const r = this.wasm.themeCard(code, level, pct);
      svg = r?.ok ? r.svg : null;
    } catch { /* 单张 SVG 合成失败 → 返回 null，上层 makeCardView 抛错中止 */ }
    this.svgCache.set(key, svg);
    return svg;
  }

  // 把一张 SVG 登记为 Phaser 纹理，返回 Promise<key>（失败回退 null）。
  // SVG 原生是 500×700；先用 canvas 以 2~3× 显示尺寸光栅化，避免 WebGL 缩小时糊。
  // 牌面按设计图放大到 86×120 后基准宽度 90*dpr。
  registerTexture(code, level, pct) {
    const key = this.texKey(code, level, pct);
    if (!this.loaded || !this.scene) return Promise.resolve(null);
    if (this.texKeys.has(key)) return Promise.resolve(key);
    const svg = this.ensureSvg(code, level, pct);
    if (!svg) return Promise.resolve(null);
    return this.rasterize(key, svg);
  }

  // 把主题牌背 SVG 登记为纹理（与级牌无关，key 固定带 BACK）。
  // 主题硬依赖：失败由 load 返回 ok:false 中止启动。
  registerBack() {
    const key = `card_${this.themeId}_BACK`;
    if (!this.scene) return Promise.resolve(null);
    if (this.texKeys.has(key)) return Promise.resolve(key);
    let svg = null;
    try { svg = this.wasm.themeBack?.()?.svg ?? null; } catch { /* 由下方判空处理 */ }
    if (!svg) return Promise.resolve(null);
    return this.rasterize(key, svg);
  }

  // SVG → Phaser canvas 纹理（牌面/牌背共用）：500×700 光栅化 + 内缘圆角描边。
  rasterize(key, svg) {
    return new Promise((resolve) => {
      try {
        const image = new Image();
        image.onload = () => {
          const dpr = Math.min(3, Math.max(2, Math.round(window.devicePixelRatio || 1) + 1));
          const tw = 90 * dpr;
          const th = Math.round(tw * 700 / 500);
          const canvas = document.createElement('canvas');
          canvas.width = tw;
          canvas.height = th;
          const g = canvas.getContext('2d');
          g.imageSmoothingEnabled = true;
          g.imageSmoothingQuality = 'high';
          g.drawImage(image, 0, 0, tw, th);
          // 主题牌面多为白底，叠放时边界糊成一片；统一在纹理内缘描圆角边框。
          // 画在纹理里而不是场景对象上：手牌/出牌/飞牌/角标所有复用点自动生效。
          const inset = Math.max(1, Math.round(dpr * 0.9));
          const radius = Math.round(8 * dpr);
          g.strokeStyle = '#4d6b5f';
          g.lineWidth = Math.max(2, Math.round(1.8 * dpr));
          g.lineJoin = 'round';
          g.beginPath();
          g.moveTo(inset + radius, inset);
          g.lineTo(tw - inset - radius, inset);
          g.quadraticCurveTo(tw - inset, inset, tw - inset, inset + radius);
          g.lineTo(tw - inset, th - inset - radius);
          g.quadraticCurveTo(tw - inset, th - inset, tw - inset - radius, th - inset);
          g.lineTo(inset + radius, th - inset);
          g.quadraticCurveTo(inset, th - inset, inset, th - inset - radius);
          g.lineTo(inset, inset + radius);
          g.quadraticCurveTo(inset, inset, inset + radius, inset);
          g.closePath();
          g.stroke();
          if (!this.scene.textures.exists(key)) this.scene.textures.addCanvas(key, canvas);
          this.texKeys.add(key);
          resolve(key);
        };
        image.onerror = () => resolve(null);
        image.src = `data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`;
      } catch {
        resolve(null);
      }
    });
  }

  // 预热当前级牌下的全部 54 个牌面纹理；发牌动画前调用，确保动画里能看到主题牌。
  async warmup(level, pct = this.cornerPct) {
    if (!this.loaded) return;
    await Promise.all(ALL_CODES.map((code) => this.registerTexture(code, level, pct)));
  }

  // 取某花色的主题墨色（0xRRGGBB）。wasm 在 themeLoad 时已按
  // palette.suits.{spade|heart|diamond|club} 优先、回落 red/black 算出四色，
  // 这里只做 #RRGGBB 解析；无主题/格式非法返回 null（调用方自行回退）。
  suitInk(suit) {
    const hex = this.suitInks?.[suit];
    if (typeof hex !== 'string') return null;
    const m = /^#([0-9a-fA-F]{6})$/.exec(hex.trim());
    return m ? parseInt(m[1], 16) : null;
  }

  // 取已登记的纹理键（同步）；未预热/失败返回 null → 场景抛错中止（不回退字卡）。
  // level 兼容数字级牌码(1..13)与字符串标签('A'/'10'…)，统一成数字 key，
  // 避免发牌/出牌等场景传不同形态导致 key 不匹配、牌面突然从字卡变主题。
  textureKey(cardId, level) {
    if (!this.loaded) return null;
    const lv = typeof level === 'string' ? rankCode(level) : level;
    const key = this.texKey(faceCode(cardId), lv, this.cornerPct);
    return this.texKeys.has(key) ? key : null;
  }
}
