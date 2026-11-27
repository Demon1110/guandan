// 牌桌场景：Phaser 3 渲染 + Tween 动画。订阅 Game 的 change/event，纯表现层。
// 逻辑一句都不在这里判定，全部读 Game.snapshot()，点牌回调 game.toggleCard。
// Phaser 由 index.html 以 UMD <script> 引入，走全局 window.Phaser。
import { ASSETS, resolveAsset } from '../assets.js';
import { cardFace, columnKey, detectBomb, rankCode, rankIndexOf, SPECIAL_PATTERN, suitIndexOf } from '../cards.js';
import { THEME_ORDER, THEMES } from '../skin.js';

const Phaser = window.Phaser;

const W = 1280;
const H = 720;
const FELT = 0x146b4a;
const FELT_DARK = 0x0e5238;
// 牌面按设计图放大：86×120（扑克牌比例 ~0.717），牌面完整清晰。
const CARD_W = 86;
const CARD_H = 120;

// 四个方位的几何配置（相对人类座位）。center 为该座位出牌区锚点。
// 重排垂直空间：顶部徽标条 → top 座位 → 出牌区 → 操作栏 → 手牌区（贴底）。
// seatX/seatY 是座位卡片锚点（左右贴边居中），cx/cy 是出牌区锚点。
const SLOTS = {
  bottom: { name: 'bottom', cx: 640, cy: 372, handY: 600, labelY: 316, seatX: 640, seatY: 316 },
  right: { name: 'right', cx: 980, cy: 318, handY: 250, labelY: 150, seatX: 1186, seatY: 360 },
  top: { name: 'top', cx: 640, cy: 168, handY: 80, labelY: 78, seatX: 640, seatY: 60 },
  left: { name: 'left', cx: 300, cy: 318, handY: 250, labelY: 150, seatX: 94, seatY: 360 },
};
const SLOT_ORDER = ['bottom', 'right', 'top', 'left'];
const DECK_X = 640;
const DECK_Y = 318;

// 操作栏（出牌/提示/过牌，或要不起）居中上移，紧贴手牌上方；底部工具条在手牌下方。
const BAR_Y = 424;
const HAND_TOOLS_Y = 690; // 组牌建议/四花色/理成一列/恢复理牌
const HAND_BASE_Y = 600; // 手牌区基准 y（上移给底部工具条让位）
const COUNTDOWN_SECONDS = 30;

// 名次标签（头游/二游/三游/末游）
const RANK_LABELS = ['', '头游', '二游', '三游', '末游'];

export class TableScene extends Phaser.Scene {
  constructor(game, skin) {
    super('table');
    this.gameModel = game;
    this.skin = skin;
    this.cardViews = new Map(); // id -> Container（人类手牌）
    this.cornerViews = new Map(); // id -> 角标高亮浮层（fan 模式保证点数恒可见）
    this.lockViews = new Map(); // id -> 理牌分组小锁图标（右上角）
    this.trickViews = new Map(); // seat -> 该座位出牌区视图（每座只留最近一手）
    this.teammateViews = []; // 我出完牌后：队友手牌只读展示（不进 cardViews、不挂交互）
    this.flyViews = []; // 飞牌/发牌临时对象
    this.bombFx = []; // 炸弹特效临时对象（闪光/冲击波/火花/横幅）
    this.bombTimer = null; // 横幅退场定时器
    this.lastSnapshot = null;
    this.dealing = false;
    this.handStyle = 'cols'; // 默认按设计图：同点数纵向叠列；?hand=fan 可切横排
    this.cardScale = 1; // 手牌牌面大小倍率（设置面板可调 0.8/1/1.2）
    // 理牌分组（对齐 web）：只改布局，不动手牌/选中/合法判定。
    // 每项 { cards:[id], patternId, layout:'left'|'right', suit? }。
    this.arranged = [];
    this.flushCursor = {}; // 各花色同花顺方案切换游标
    // 倒计时（纯 UI + 交互，不改 Game 状态机）：出牌 30s、还贡 15s 可调
    this.tributeSeconds = 15;
    this.countdown = { active: false, left: COUNTDOWN_SECONDS, total: COUNTDOWN_SECONDS, timer: null, seat: null };
    // 鼠标按住拖动连选：{ mode:'add'|'remove', touched:Set }
    this.dragSelect = null;
  }

  create() {
    this.buildTable();
    this.buildButtons();
    this.buildCountdown();

    this.gameModel.on('change', (snap) => this.render(snap));
    this.gameModel.on('event', (ev) => this.onEvent(ev));

    //  pointerup / 离开画布：结束拖动连选
    this.input.on('pointerup', () => { this.dragSelect = null; });
    this.input.on('gameout', () => { this.dragSelect = null; });
    // 点非牌面（牌桌空白/无交互对象处）即取消选牌，替代原"取消选牌"按钮。
    // 点手牌/按钮时 over 非空，不会误清；拖选 pointerdown 命中牌也不受影响。
    this.input.on('pointerdown', (pointer, over) => {
      if (!over || !over.length) this.gameModel.clearSelection();
    });

    // ?hand=cols 切到同点数纵向罗列（参照 web）；默认 fan 横排铺满。
    const handParam = new URLSearchParams(location.search).get('hand');
    if (handParam === 'cols' || handParam === 'fan') this.handStyle = handParam;
    this.updateHandBtn();

    this.statusText.setText('请选择开局方式');
    this.render = this.render.bind(this);
  }

  updateHandBtn() {
    if (!this.handBtn?.labelText) return;
    this.handBtn.labelText.setText(this.handStyle === 'cols' ? '手牌 列' : '手牌 横');
  }

  toggleHandStyle() {
    this.handStyle = this.handStyle === 'cols' ? 'fan' : 'cols';
    this.updateHandBtn();
    // 必须重新取实时快照：lastSnapshot 可能是发牌/结算过渡期的旧数据，
    // 用它 renderHand 会因 hand 不匹配而误销毁当前所有牌视图。
    if (this.dealing) return;
    this.lastSnapshot = this.gameModel.snapshot();
    this.renderHand(this.lastSnapshot);
  }

  setThemeLabel(label) {
    if (this.themeBtn?.labelText) this.themeBtn.labelText.setText(`主题 ${label ?? '?'}`.slice(0, 8));
  }

  // ---- 设置面板调用 ----
  setHandStyleDirect(style) {
    if (style !== 'cols' && style !== 'fan') return;
    if (this.handStyle === style) return;
    this.handStyle = style;
    this.updateHandBtn();
    if (this.dealing) return;
    this.lastSnapshot = this.gameModel.snapshot();
    this.renderHand(this.lastSnapshot);
  }

  // 牌面大小：倍率作用于手牌视图与布局间距，reflow 后保持落位一致。
  // 还贡倒计时秒数（设置面板可调，范围 5–60）。
  setTributeSeconds(sec) {
    const s = Number(sec);
    if (Number.isFinite(s) && s >= 5 && s <= 60) this.tributeSeconds = Math.round(s);
  }

  setCardScale(scale) {
    const s = Number(scale);
    if (!Number.isFinite(s) || s < 0.5 || s > 2 || s === this.cardScale) return;
    this.cardScale = s;
    if (this.dealing) return;
    // 主题 Image 的基准 scale 已固化在创建时，改大小必须销毁重建，避免倍率叠加。
    for (const view of this.cardViews.values()) view.destroy();
    this.cardViews.clear();
    for (const corner of this.cornerViews.values()) corner.destroy();
    this.cornerViews.clear();
    this.clearLocks();
    this.lastSnapshot = this.gameModel.snapshot();
    this.renderHand(this.lastSnapshot);
  }

  // 直接切到指定主题（区别于 cycleTheme 的循环切换）。
  async setThemeById(themeKey) {
    if (!THEMES[themeKey] || this.dealing || this.switchingTheme) return false;
    if (this.skin.themeId === themeKey) return true;
    this.switchingTheme = true;
    const prevStatus = this.statusText.text;
    this.statusText.setText(`切换主题：${THEMES[themeKey].label}…`);
    for (const view of this.cardViews.values()) view.destroy();
    this.cardViews.clear();
    for (const corner of this.cornerViews.values()) corner.destroy();
    this.cornerViews.clear();
    this.clearLocks();
    this.clearTrick();
    this.clearFly();
    this.clearBombFx();
    try {
      const r = await this.skin.load(this, themeKey);
      if (!r.ok) {
        this.statusText.setText(`主题切换失败：${r.error ?? '未知'}`);
        this.lastSnapshot = this.gameModel.snapshot();
        this.renderHand(this.lastSnapshot);
        return false;
      }
      const lv = this.gameModel.levelRankLabel();
      const rankCode = (label) => {
        if (!label) return 0;
        if (label === 'A') return 1;
        if (label === 'J') return 11;
        if (label === 'Q') return 12;
        if (label === 'K') return 13;
        return Number(label);
      };
      await this.skin.warmup(rankCode(lv));
      this.refreshDeckStack(); // 牌背纹理键随主题变化，重建中央牌堆
      this.lastSnapshot = this.gameModel.snapshot();
      this.renderHand(this.lastSnapshot);
      this.setThemeLabel(r.label);
      this.statusText.setText(prevStatus);
      return true;
    } finally {
      this.switchingTheme = false;
    }
  }

  // 循环切换主题：加载新包 → 重建纹理缓存 → 重绘当前手牌。
  async cycleTheme() {
    if (this.dealing || this.switchingTheme) return;
    this.switchingTheme = true;
    const cur = this.skin.themeId ?? THEME_ORDER[0];
    const next = THEME_ORDER[(THEME_ORDER.indexOf(cur) + 1) % THEME_ORDER.length];
    const prevStatus = this.statusText.text;
    this.statusText.setText(`切换主题：${THEMES[next]?.label ?? next}…`);
    // 先销毁所有引用旧纹理的对象，再让 skin.load 删除旧纹理，避免中间帧渲染到已删纹理。
    for (const view of this.cardViews.values()) view.destroy();
    this.cardViews.clear();
    for (const corner of this.cornerViews.values()) corner.destroy();
    this.cornerViews.clear();
    this.clearLocks();
    this.clearTrick();
    this.clearFly();
    this.clearBombFx();
    try {
      const r = await this.skin.load(this, next);
      if (!r.ok) {
        this.statusText.setText(`主题切换失败：${r.error ?? '未知'}`);
        // 加载失败：用旧主题重建（wasm 全局可能仍是旧主题），恢复牌面
        this.lastSnapshot = this.gameModel.snapshot();
        this.renderHand(this.lastSnapshot);
        return;
      }
      // 新主题的角标倍率可能与旧主题不同 → 重新预热当前级牌的全部纹理。
      const lv = this.gameModel.levelRankLabel();
      const rankCode = (label) => {
        if (!label) return 0;
        if (label === 'A') return 1;
        if (label === 'J') return 11;
        if (label === 'Q') return 12;
        if (label === 'K') return 13;
        return Number(label);
      };
      await this.skin.warmup(rankCode(lv));
      this.refreshDeckStack(); // 牌背纹理键随主题变化，重建中央牌堆

      // 场上对象已在 load 前清空；这里直接用新纹理重建。
      this.lastSnapshot = this.gameModel.snapshot();
      this.renderHand(this.lastSnapshot);
      this.setThemeLabel(r.label);
      this.statusText.setText(`已切换主题：${r.label}`);
    } finally {
      this.switchingTheme = false;
    }
  }

  // ---- 静态桌面（占位素材：上层海天、下层沙滩；后期按 UI 设计图替换 assets.js） ----
  buildTable() {
    const g = this.add.graphics();
    // 分区渐变：天空 → 海面 → 沙滩（用多条横带近似渐变）
    const bands = [
      [0, 150, ASSETS.seaTop.color], [150, 250, ASSETS.seaMid.color], [250, 340, ASSETS.seaLow.color],
      [340, 400, ASSETS.sandTop.color], [400, 520, ASSETS.sandMid.color], [520, H, ASSETS.sandLow.color],
    ];
    bands.forEach(([y0, y1, color]) => {
      g.fillStyle(color, 1).fillRect(0, y0, W, y1 - y0);
    });
    // 海面高光
    g.fillStyle(0xffffff, 0.10);
    for (let i = 0; i < 6; i += 1) g.fillRect(180 + i * 150, 300 + (i % 2) * 18, 90, 3);
    // 桌面圆角边框 + 内描边
    g.lineStyle(3, 0xffffff, 0.5).strokeRoundedRect(16, 16, W - 32, H - 32, 24);
    g.lineStyle(1, 0x6b5b3a, 0.35).strokeRoundedRect(22, 22, W - 44, H - 44, 20);
    this.placeDecor('watermark', (spec) => {
      this.add.text(DECK_X, 392, spec.text, {
        fontFamily: 'serif', fontSize: '34px', color: '#8a6f42',
      }).setOrigin(0.5).setAlpha(0.16).setAngle(-8);
      this.add.text(DECK_X, 424, spec.sub, {
        fontFamily: 'sans-serif', fontSize: '13px', color: '#6f5a35',
      }).setOrigin(0.5).setAlpha(0.5);
    }, DECK_X, 400);

    // 中央牌堆（发牌从这里飞出）：主题是硬依赖，牌堆在主题就绪后由
    // refreshDeckStack() 用 gfdeck 牌背纹理创建（buildTable 时纹理尚未登记）。
    this.deckStack = null;
    this.deckStackLabel = null;

    // 左上角记分板：我方/对方在上、级牌在下；打级方黄框高亮（无箭头/无行动方）。
    this.makeScoreBoard();
    this.makeMultiplier(1120, 40);

    // 四座位卡片（头像/昵称/分数/段位）；slotLabels 仍暴露内部文本以兼容 animateCountUp。
    this.slotLabels = {};
    SLOT_ORDER.forEach((slot) => this.makeSeatCard(slot));
  }

  // 素材替换统一入口：programmatic 走回调绘制，image 走贴图。
  placeDecor(key, drawProg, x, y) {
    const a = resolveAsset(key);
    if (a.kind === 'image') {
      this.load.image(key, a.url);
      this.add.image(x, y, key);
    } else {
      drawProg(a.spec);
    }
  }

  // 左上角记分板：上排「我方 | 对方」，下排「级牌 X」。
  // 左上角双方级牌板：每格显示「队伍名 + 该队当前级牌」（掼蛋双方各自升级）。
  // attackingTeam(0=我方/1=对方) 为当前打级方，只给它黄框高亮；无箭头/无行动方。
  makeScoreBoard() {
    const x = 20, y = 14, w = 208, h = 62;
    const g = this.add.graphics().setDepth(6);
    g.fillStyle(0x0f2d3a, 0.9).fillRoundedRect(x, y, w, h, 10);
    g.lineStyle(1.5, 0xf0d9a0, 0.7).strokeRoundedRect(x, y, w, h, 10);
    this.teamBoxes = {};
    [[0, '我方', x + 8], [1, '对方', x + w / 2 + 2]].forEach(([team, label, bx]) => {
      const bw = w / 2 - 12;
      const box = this.add.graphics().setDepth(7);
      box.fillStyle(0x1a4a58, 1).fillRoundedRect(bx, y + 6, bw, 48, 6);
      box.lineStyle(2, 0xffd45e, 0).strokeRoundedRect(bx, y + 6, bw, 48, 6); // 默认无高亮
      this.add.text(bx + bw / 2, y + 14, label, {
        fontFamily: 'sans-serif', fontSize: '13px', color: '#ffe9a8', fontStyle: 'bold',
      }).setOrigin(0.5, 0).setDepth(8);
      const levelText = this.add.text(bx + bw / 2, y + 38, '2', {
        fontFamily: 'sans-serif', fontSize: '24px', color: '#ffffff', fontStyle: 'bold',
      }).setOrigin(0.5).setDepth(8);
      this.teamBoxes[team] = { box, bx, by: y + 6, bw, bh: 48, levelText };
    });
    this.levelBadgeText = this.teamBoxes[0].levelText; // 兼容旧引用
  }

  // 刷新记分板：双方各自级牌 + 只给打级方黄框高亮。
  // levels: [team0级牌idx, team1级牌idx]；attackingTeam: 当前打级方。
  refreshScoreBoard(attackingTeam, levels) {
    const RANKS = ['2','3','4','5','6','7','8','9','10','J','Q','K','A'];
    const labelOf = (idx) => Number.isInteger(idx) && idx >= 0 && idx <= 12 ? RANKS[idx] : '2';
    for (const team of [0, 1]) {
      const t = this.teamBoxes?.[team];
      if (!t) continue;
      t.box.clear();
      t.box.fillStyle(0x1a4a58, 1).fillRoundedRect(t.bx, t.by, t.bw, t.bh, 6);
      t.box.lineStyle(2, 0xffd45e, attackingTeam === team ? 1 : 0)
        .strokeRoundedRect(t.bx, t.by, t.bw, t.bh, 6);
      if (Array.isArray(levels)) t.levelText.setText(labelOf(levels[team]));
    }
  }

  makeMultiplier(x, y) {
    const g = this.add.graphics();
    g.fillStyle(ASSETS.multiplier.color, 0.88).fillRoundedRect(x - 56, y - 20, 112, 40, 8);
    this.add.text(x - 46, y - 10, '倍数', { fontFamily: 'sans-serif', fontSize: '12px', color: '#cfe6e8' });
    this.add.text(x + 46, y, '1', {
      fontFamily: 'sans-serif', fontSize: '23px', color: '#ffd76a', fontStyle: 'bold',
    }).setOrigin(1, 0.5);
  }

  makeSeatCard(slot) {
    const p = SLOTS[slot];
    const isBottom = slot === 'bottom';
    const isTop = slot === 'top';
    const w = 158, h = 52;
    // 左右座位严格以 seatX 居中贴边；上下座位居中。
    const x = p.seatX ?? p.cx, y = p.seatY ?? p.labelY;
    const g = this.add.graphics().setDepth(4);
    g.fillStyle(0x0f2d3a, 0.88).fillRoundedRect(x - w / 2, y - h / 2, w, h, 10);
    g.lineStyle(1.5, 0xf0d9a0, 0.9).strokeRoundedRect(x - w / 2, y - h / 2, w, h, 10);
    // 头像框（左侧）—— 占位素材，后期替换为贴图。
    const av = isBottom ? ASSETS.avatarSelf : ASSETS.avatarBot;
    g.fillStyle(av.color, 1).fillCircle(x - w / 2 + 24, y, 17);
    this.add.text(x - w / 2 + 24, y, av.glyph, {
      fontFamily: 'serif', fontSize: '15px', color: av.text, fontStyle: 'bold',
    }).setOrigin(0.5).setDepth(5);
    // 昵称 + 手牌数（两行；setText 接口给 animateCountUp / render 用）
    const label = this.add.text(x - w / 2 + 48, y - 17, '', {
      fontFamily: 'sans-serif', fontSize: '12px', color: '#ffe9a8', lineSpacing: 1,
    }).setDepth(5);
    // 名次徽标（头游/二游/三游/末游），初始隐藏，render 时按 rank 显示。
    const rankText = this.add.text(x + w / 2 - 8, y - h / 2 + 8, '', {
      fontFamily: 'sans-serif', fontSize: '11px', color: '#2b1000',
      backgroundColor: '#ffd45e', padding: { x: 4, y: 2 },
    }).setOrigin(1, 0).setDepth(6).setVisible(false);
    // 分数/段位（底部小字）
    const tier = isTop ? '2900 · 青铜2' : isBottom ? '2900 · 白银3' : '2900 · 青铜2';
    this.add.text(x - w / 2 + 48, y + 14, tier, {
      fontFamily: 'sans-serif', fontSize: '9px', color: '#9fd0c8',
    }).setDepth(5);
    this.slotLabels[slot] = label;
    this.slotRankTexts = this.slotRankTexts ?? {};
    this.slotRankTexts[slot] = rankText;
  }

  makeButton(x, y, label, onClick, color = 0x1d6f57, w = 92, h = 40) {
    const box = this.add.rectangle(x, y, w, h, color, 1)
      .setStrokeStyle(1, 0xffffff, 0.55).setInteractive({ useHandCursor: true });
    const txt = this.add.text(x, y, label, {
      fontFamily: 'sans-serif', fontSize: '15px', color: '#ffffff',
    }).setOrigin(0.5).setDepth(34);
    box.labelText = txt;
    box.baseColor = color; // 可被 syncSuitTabs 改写为动态色，pointerout 回到它
    box.on('pointerover', () => { if (box.input?.enabled) box.setFillStyle(0x2b8a6c, 1); });
    box.on('pointerout', () => box.setFillStyle(box.baseColor ?? color, 1));
    box.on('pointerdown', onClick);
    // 按钮整体显示/隐藏（含文字与命中区），供"要不起/可出牌"状态切换。
    box.setBtnVisible = (v) => {
      box.setVisible(v);
      txt.setVisible(v);
      if (v) box.setInteractive({ useHandCursor: true });
      else box.disableInteractive();
    };
    return box;
  }

  // 操作栏：主按钮居中上移，位于手牌正上方；左侧不再放常驻提示文字。
  buildButtons() {
    const y = BAR_Y;
    // 去掉原灰色长条背景；按钮直接浮在牌桌上，避免长牌叠列时横条被牌切断显得杂乱。

    // 反馈文字移到屏幕底部居中，不再挤在按钮左侧（"轮到你出牌"已由中央闹钟提示）。
    this.statusText = this.add.text(640, 714, '', {
      fontFamily: 'sans-serif', fontSize: '12px', color: '#9fd0c8',
    }).setOrigin(0.5, 0.5).setDepth(34);

    // 主按钮水平居中：出牌 / 提示 / 过牌；"要不起"与其余三个互斥显示。
    const bw = 96, gap = 16;
    const x0 = 640 - (bw * 3 + gap * 2) / 2 + bw / 2;
    this.playBtn = this.makeButton(x0, y, '出牌', () => this.onPlay(), 0xb8860b, bw);
    this.hintBtn = this.makeButton(x0 + bw + gap, y, '提示', () => this.gameModel.hint(), 0x2b8a6c, bw);
    this.passBtn = this.makeButton(x0 + (bw + gap) * 2, y, '过牌', () => this.onPass(), 0x2b6f8a, bw);
    // 无可出牌时只显示"要不起"（点击等同过牌），与上面三个按钮互斥。
    this.cannotBtn = this.makeButton(640, y, '要不起', () => this.onPass(), 0x7a4a3a, 132);
    // 还贡专用按钮：仅 tribute 阶段我方受贡时显示，取代出牌/提示/过牌。
    this.returnBtn = this.makeButton(640, y, '还贡', () => this.onTributeReturn(), 0x8a5ab8, 132);
    // cols 叠列的最长列向上延伸会盖到按钮（手牌 depth 20+y/8 可达 76+），
    // 按钮连同文字抬到 156/157，高于手牌/角标浮层，低于炸弹横幅 190、结算层 200。
    [this.playBtn, this.hintBtn, this.passBtn, this.cannotBtn, this.returnBtn].forEach((b) => {
      b.setDepth(156);
      b.labelText.setDepth(157);
    });
    this.cannotBtn.setBtnVisible(false);
    this.returnBtn.setBtnVisible(false);

    // 底部工具条：组牌建议 / 四花色同花顺快速组牌 / 理成一列 / 恢复理牌。
    this.buildHandTools();
  }

  // 我方出牌阶段是否有可出的牌（legal 过滤掉 pass）；非我方回合/非 play 阶段恒为 true。
  hasPlayableCards(snap) {
    if (!snap?.myTurn || snap.phase !== 'play') return true;
    try {
      const legal = this.gameModel.wasm.legal?.() ?? [];
      return legal.some((a) => !a.pass && (a.cards?.length ?? 0) > 0);
    } catch {
      return true;
    }
  }

  // 手牌下方工具条：组牌建议、♠♥♦♣ 同花顺快速组牌、理成一列、恢复理牌。
  buildHandTools() {
    const y = HAND_TOOLS_Y;
    const bar = this.add.graphics().setDepth(30);
    bar.fillStyle(0x0f2d3a, 0.78).fillRoundedRect(100, y - 22, W - 200, 44, 12);
    bar.lineStyle(1, 0xf0d9a0, 0.5).strokeRoundedRect(100, y - 22, W - 200, 44, 12);

    const mk = (x, label, color, onClick, w = 108) =>
      this.makeButton(x, y, label, onClick, color, w, 34).setDepth(33);

    mk(190, '组牌建议', 0x2b8a6c, () => this.onSuggestGroup(), 104);
    // 四花色同花顺快速组牌（花色符号作按钮文字）
    this.suitButtons = [];
    [[0, '♠'], [1, '♥'], [2, '♦'], [3, '♣']].forEach(([suit, sym]) => {
      const b = mk(360 + suit * 64, sym, 0x8a5a16, () => this.onSuitFlush(suit), 46);
      this.suitButtons.push(b);
    });
    mk(700, '理成一列', 0x1d6f57, () => this.onAlignColumn(), 104);
    mk(830, '恢复理牌', 0x6a4a8a, () => this.onRestoreLayout(), 104);
  }

  // ---- 底部工具条行为（对齐 apps/web 的理牌语义：只改布局，不动手牌/选中/合法判定） ----
  patternName(pid) {
    const NAMES = {
      'bomb-4': '四炸', 'bomb-5': '五炸', 'bomb-6': '六炸', 'bomb-7': '七炸', 'bomb-8': '八炸',
      'straight-flush-5': '同花顺', 'joker-bomb': '王炸',
    };
    return NAMES[pid] ?? pid;
  }

  // 同花顺的本色：跳过红桃级牌（百搭），取第一张自然牌的花色。
  straightFlushSuit(cards) {
    const lvIdx = this.levelRankIndex();
    const natural = cards.find((id) => id < 104 &&
      !(lvIdx >= 0 && suitIndexOf(id) === 1 && rankIndexOf(id) === lvIdx));
    return natural === undefined ? null : suitIndexOf(natural);
  }

  levelRankIndex() {
    return this.rankLabelToIndex(this.gameModel.levelRankLabel());
  }

  // 级牌标签 → 0..12 下标（2..A）；无级牌返回 -1。展示排序用它识别红配。
  rankLabelToIndex(label) {
    if (!label) return -1;
    const RANKS = ['2','3','4','5','6','7','8','9','10','J','Q','K','A'];
    return RANKS.indexOf(label);
  }

  // 该花色的同花顺窗口（含红桃级牌百搭补位），对齐 web flushWindows。
  flushWindows(suit, ids) {
    const lvIdx = this.levelRankIndex();
    const wildIds = lvIdx < 0 ? [] : ids.filter((id) =>
      id < 104 && suitIndexOf(id) === 1 && rankIndexOf(id) === lvIdx);
    const wildSet = new Set(wildIds);
    const byRank = new Map();
    for (const id of ids) {
      if (id >= 104 || wildSet.has(id) || suitIndexOf(id) !== suit) continue;
      const r = rankIndexOf(id);
      if (!byRank.has(r)) byRank.set(r, []);
      byRank.get(r).push(id);
    }
    const out = [];
    const addWindow = (start, ranks, wheel = false) => {
      const cards = [];
      let missing = 0;
      for (const rank of ranks) {
        const list = byRank.get(rank);
        if (list && list.length) cards.push(list[0]);
        else missing += 1;
      }
      if (missing > wildIds.length) return;
      out.push({ start, wheel, cards: cards.concat(wildIds.slice(0, missing)), wild: Math.min(missing, wildIds.length) });
    };
    for (let start = 0; start + 4 < 13; start += 1) {
      addWindow(start, Array.from({ length: 5 }, (_, d) => start + d));
    }
    addWindow(12, [12, 0, 1, 2, 3], true); // A-2-3-4-5 轮子
    const unique = new Map();
    for (const opt of out) {
      const key = opt.cards.slice().sort((a, b) => a - b).join(',');
      const prev = unique.get(key);
      if (!prev || opt.start > prev.start) unique.set(key, opt);
    }
    return [...unique.values()];
  }

  // 花色可用牌池：排除其它花色已理组，保留本花色组。
  poolForSuit(cards, suit) {
    const others = new Set(this.arranged.filter((g) => g.suit !== suit).flatMap((g) => g.cards));
    return cards.filter((id) => !others.has(id));
  }

  // 组牌建议：直接用 plan 的最优整手拆分铺开成理牌组（炸弹/同花顺左列、其余右列）。
  onSuggestGroup() {
    if (this.gameModel.waitingSeat !== this.gameModel.humanSeat) {
      this.statusText.setText('轮到你时才能看组牌建议'); return;
    }
    const plan = this.gameModel.wasm.plan?.();
    if (!plan || !Array.isArray(plan.groups) || plan.groups.length === 0) {
      this.statusText.setText('暂时拿不到组牌建议（本次拆分没有可出的组）'); return;
    }
    this.arranged = plan.groups
      .filter((g) => g.cards?.length)
      .map((g) => ({
        cards: g.cards.slice(),
        patternId: g.patternId ?? '',
        layout: SPECIAL_PATTERN(g.patternId) ? 'left' : 'right',
        suit: g.patternId?.startsWith('straight-flush') ? this.straightFlushSuit(g.cards) : null,
      }));
    this.flushCursor = {};
    this.gameModel.selected.clear();
    this.gameModel.emitChange();
    const left = this.arranged.filter((g) => g.layout === 'left').length;
    this.statusText.setText(`组牌建议（最优拆分）：${this.arranged.length} 组 · 左 ${left} / 右 ${this.arranged.length - left}`);
  }

  // 四花色：在该花色的同花顺窗口里循环切换，理成一组（支持反复点击换方案）。
  onSuitFlush(suit) {
    if (this.gameModel.waitingSeat !== this.gameModel.humanSeat) {
      this.statusText.setText('轮到你时才能理同花顺'); return;
    }
    const cards = this.gameModel.myHand();
    const opts = this.flushWindows(suit, this.poolForSuit(cards, suit));
    if (!opts.length) {
      this.statusText.setText(`${['♠','♥','♦','♣'][suit]} 凑不出同花顺`); return;
    }
    const cur = this.flushCursor[suit] ?? -1;
    const idx = (cur + 1) % opts.length;
    this.flushCursor[suit] = idx;
    const pick = opts[idx];
    this.arranged = this.arranged.filter((g) => g.suit !== suit);
    this.arranged.push({ cards: pick.cards.slice(), patternId: 'straight-flush-5', layout: 'left', suit });
    this.gameModel.selected.clear();
    this.gameModel.emitChange();
    this.statusText.setText(`${['♠','♥','♦','♣'][suit]} 同花顺方案 ${idx + 1}/${opts.length}${pick.wild ? `（红配代 ${pick.wild} 张）` : ''}`);
  }

  // 理成一列：把当前选中的牌按 canPlay 认牌型，理成一组（炸弹/同花顺左、其余右）。
  onAlignColumn() {
    if (this.gameModel.waitingSeat !== this.gameModel.humanSeat) {
      this.statusText.setText('轮到你时才能理牌'); return;
    }
    if (this.gameModel.selected.size === 0) {
      this.statusText.setText('先选中要理的牌型'); return;
    }
    const cards = [...this.gameModel.selected].sort((a, b) => a - b);
    const pre = this.gameModel.wasm.canPlay?.(cards);
    const pid = pre?.patternId ?? '';
    if (!pid) {
      this.statusText.setText('选中的牌凑不成牌型，理不了'); return;
    }
    const suit = pid.startsWith('straight-flush') ? this.straightFlushSuit(cards) : null;
    const key = cards.join(',');
    this.arranged = this.arranged.filter((g) => g.cards.slice().sort((a, b) => a - b).join(',') !== key);
    this.arranged.push({ cards, patternId: pid, layout: SPECIAL_PATTERN(pid) ? 'left' : 'right', suit });
    this.gameModel.selected.clear();
    this.gameModel.emitChange();
    this.statusText.setText(`${this.patternName(pid)} 已理到${SPECIAL_PATTERN(pid) ? '最左' : '最右'}一列${pre.ok ? '' : '（当前压不过上家）'}`);
  }

  // 恢复理牌：选中的牌正好是一组时只恢复那组，否则恢复全部。
  onRestoreLayout() {
    if (!this.arranged.length) {
      this.statusText.setText('没有可恢复的理牌'); return;
    }
    const sel = [...this.gameModel.selected].sort((a, b) => a - b).join(',');
    const hit = sel ? this.arranged.findIndex((g) => g.cards.slice().sort((a, b) => a - b).join(',') === sel) : -1;
    if (hit >= 0) {
      const [g] = this.arranged.splice(hit, 1);
      this.gameModel.selected.clear();
      this.statusText.setText(`已恢复 ${this.patternName(g.patternId)}`);
    } else {
      this.arranged = [];
      this.flushCursor = {};
      this.statusText.setText('已恢复全部理牌');
    }
    this.gameModel.emitChange();
  }

  // ---- 闹钟倒计时（UI + 交互；超时自动过牌） ----
  // 我方回合：闹钟居中屏幕 + 下方「轮到你出牌」；他人回合：闹钟放到该座位出牌区前。
  buildCountdown() {
    this.countdownG = this.add.graphics().setDepth(150);
    this.countdownText = this.add.text(0, 0, '', {
      fontFamily: 'sans-serif', fontSize: '18px', color: '#2b1000', fontStyle: 'bold',
    }).setOrigin(0.5).setDepth(152);
    this.countdownHint = this.add.text(640, 300, '', {
      fontFamily: 'sans-serif', fontSize: '20px', color: '#ffe9a8', fontStyle: 'bold',
      backgroundColor: '#0f2d3a', padding: { x: 10, y: 5 },
    }).setOrigin(0.5).setDepth(152).setVisible(false);
    this.drawCountdown();
  }

  // 画一个闹钟：表盘 + 顶部双铃 + 剩余时间弧 + 刻度 + 两脚；表盘内只放数字，无指针。
  drawAlarmClock(g, cx, cy, scale, ratio, color) {
    const R = 30 * scale;
    // 顶部双铃铛
    g.fillStyle(color, 1);
    g.fillCircle(cx - R * 0.78, cy - R * 0.78, R * 0.3);
    g.fillCircle(cx + R * 0.78, cy - R * 0.78, R * 0.3);
    // 铃铛连接柄
    g.lineStyle(2 * scale, color, 1);
    g.beginPath();
    g.moveTo(cx - R * 0.6, cy - R * 0.62); g.lineTo(cx - R * 0.3, cy - R * 0.92);
    g.moveTo(cx + R * 0.6, cy - R * 0.62); g.lineTo(cx + R * 0.3, cy - R * 0.92);
    g.strokePath();
    // 表盘底
    g.fillStyle(0xfdf8e8, 1).fillCircle(cx, cy, R);
    g.lineStyle(3 * scale, color, 1).strokeCircle(cx, cy, R);
    // 剩余时间弧（从顶部顺时针）
    g.lineStyle(4 * scale, color, 1);
    g.beginPath();
    g.arc(cx, cy, R * 0.82, Phaser.Math.DegToRad(-90), Phaser.Math.DegToRad(-90 + 360 * ratio), false);
    g.strokePath();
    // 刻度
    g.lineStyle(1.5 * scale, 0x8a7a5a, 0.8);
    for (let i = 0; i < 12; i += 1) {
      const a = Phaser.Math.DegToRad(i * 30 - 90);
      g.beginPath();
      g.moveTo(cx + Math.cos(a) * R * 0.66, cy + Math.sin(a) * R * 0.66);
      g.lineTo(cx + Math.cos(a) * R * 0.74, cy + Math.sin(a) * R * 0.74);
      g.strokePath();
    }
    // 不画指针：表盘中央只显示剩余秒数（countdownText），剩余比例用上面的弧表达。
    // 两脚
    g.lineStyle(2.5 * scale, color, 1);
    g.beginPath();
    g.moveTo(cx - R * 0.42, cy + R * 0.88); g.lineTo(cx - R * 0.6, cy + R * 1.18);
    g.moveTo(cx + R * 0.42, cy + R * 0.88); g.lineTo(cx + R * 0.6, cy + R * 1.18);
    g.strokePath();
  }

  drawCountdown() {
    const g = this.countdownG;
    g.clear();
    this.countdownHint.setVisible(false);
    this.countdownText.setText('');
    if (!this.countdown.active) return;

    const ratio = Math.max(0, Math.min(1, this.countdown.left / this.countdown.total));
    const color = ratio <= 0.3 ? ASSETS.countdownLow.color : ASSETS.countdownOk.color;
    const seat = this.countdown.seat;
    const isMine = seat === this.gameModel.humanSeat;
    let cx, cy, scale;
    if (isMine) {
      // 自己回合：屏幕中上方闹钟（小一号）+ 下方提示（还贡阶段提示不同）
      cx = 640; cy = 250; scale = 1.1;
      const hint = this.countdown.phase === 'tribute' ? '轮到你还贡' : '轮到你出牌';
      this.countdownHint.setText(hint).setPosition(cx, cy + 68).setVisible(true);
    } else {
      // 他人回合：闹钟放到其出牌区前（缩小），不居中、无中间文字。
      const slot = SLOT_ORDER[this.gameModel.relSlot(seat)];
      const p = SLOTS[slot];
      // 放到出牌区上方，避免压住刚打出的牌。
      cx = p.cx; cy = p.cy - 76; scale = 0.55;
    }
    this.drawAlarmClock(g, cx, cy, scale, ratio, color);
    this.countdownText.setText(String(Math.ceil(this.countdown.left)))
      .setPosition(cx, cy).setFontSize(Math.round(17 * scale)).setVisible(true);
  }

  startCountdown(seat) {
    this.stopCountdown();
    this.countdown.active = true;
    this.countdown.seat = seat;
    // 还贡用可调的 15s，出牌用 30s。
    this.countdown.phase = this.gameModel.phase;
    this.countdown.total = this.countdown.phase === 'tribute' ? this.tributeSeconds : COUNTDOWN_SECONDS;
    this.countdown.left = this.countdown.total;
    this.drawCountdown();
    // 只有人类回合才真正计时；bot 由引擎自行驱动。
    if (seat !== this.gameModel.humanSeat) return;
    this.countdown.timer = this.time.addEvent({
      delay: 1000, loop: true,
      callback: () => {
        this.countdown.left -= 1;
        this.drawCountdown();
        if (this.countdown.left <= 0) {
          const wasTribute = this.countdown.phase === 'tribute';
          this.stopCountdown();
          if (wasTribute) {
            // 还贡超时：自动选引擎默认合规牌（≤10 最小）并提交。
            this.statusText.setText('还贡超时 · 自动还贡');
            this.onTributeReturnAuto();
          } else {
            this.statusText.setText('超时 · 自动过牌');
            this.onPass();
          }
        }
      },
    });
  }

  // 还贡超时：取第一张合规牌自动提交。
  onTributeReturnAuto() {
    const eligible = this.gameModel.tributeEligible();
    if (!eligible.length) return;
    const r = this.gameModel.tributeReturn(eligible[0]);
    if (!r.ok) this.statusText.setText(r.reason);
  }

  stopCountdown() {
    this.countdown.active = false;
    if (this.countdown.timer) {
      this.countdown.timer.remove();
      this.countdown.timer = null;
    }
    this.countdown.left = 0;
    this.countdown.seat = null;
    this.countdown.phase = null;
    this.drawCountdown();
  }

  updateCountdown(snap, iAmFinished = false) {
    // 出牌阶段与还贡阶段都显示倒计时；我已出完、发牌中、结算层存在时不显示。
    const inWindow = (snap.phase === 'play' || snap.phase === 'tribute')
      && !this.dealing && !this.settleLayer && !iAmFinished;
    const active = inWindow;
    const seat = active ? snap.waitingSeat : null;
    if (seat == null) {
      if (this.countdown.active) this.stopCountdown();
      return;
    }
    // 座位变化或从未激活 → 重启（他人回合只显示闹钟不计时）。
    if (!this.countdown.active || this.countdown.seat !== seat) {
      this.startCountdown(seat);
    }
  }

  // ---- 手牌选择（点击 / 按住拖动连选） ----
  onCardDown(id) {
    if (this.gameModel.waitingSeat !== this.gameModel.humanSeat) return;
    // 还贡：只能选一张引擎合规牌（≤10），选新的替换旧的，点已选的取消。
    if (this.gameModel.phase === 'tribute') {
      const eligible = this.gameModel.tributeEligible();
      if (!eligible.includes(id)) {
        this.statusText.setText('还贡只能选 ≤10 的合规牌');
        return;
      }
      const sel = this.gameModel.selected;
      if (sel.has(id)) sel.clear();
      else { sel.clear(); sel.add(id); }
      this.dragSelect = null; // 还贡不支持拖动连选
      this.gameModel.emitChange();
      return;
    }
    // 理牌分组列：整组一起选中/取消。
    const group = this.arranged.find((g) => g.cards.includes(id));
    if (group) {
      const all = group.cards.every((c) => this.gameModel.selected.has(c))
        && this.gameModel.selected.size === group.cards.length;
      this.gameModel.selected.clear();
      if (!all) group.cards.forEach((c) => this.gameModel.selected.add(c));
      this.gameModel.emitChange();
      this.dragSelect = null; // 整组点选不进入拖动连选
      return;
    }
    const isSel = this.gameModel.selected.has(id);
    this.dragSelect = { mode: isSel ? 'remove' : 'add', touched: new Set([id]) };
    this.gameModel.toggleCard(id);
  }

  onCardOver(id) {
    const drag = this.dragSelect;
    if (!drag || drag.touched.has(id)) return;
    if (this.gameModel.waitingSeat !== this.gameModel.humanSeat) return;
    if (this.gameModel.phase === 'tribute') return; // 还贡只选单张，不拖动连选
    drag.touched.add(id);
    const isSel = this.gameModel.selected.has(id);
    // 加选模式跳过已选、取消模式跳过未选，避免重复 toggle 抖动
    if (drag.mode === 'add' && !isSel) this.gameModel.toggleCard(id);
    else if (drag.mode === 'remove' && isSel) this.gameModel.toggleCard(id);
  }

  // ---- 交互回调 ----
  onNewGame() {
    if (this.dealing) return;
    this.stopCountdown();
    this.clearTrick();
    this.clearFly();
    this.clearBombFx();
    for (const view of this.cardViews.values()) view.destroy();
    this.cardViews.clear();
    const hi = (Math.random() * 0xffffffff) >>> 0;
    const lo = (Math.random() * 0xffffffff) >>> 0;
    this.gameModel.start({ seedHi: hi, seedLo: lo, humanSeat: 0, level: 2 });
  }

  onPlay() {
    const r = this.gameModel.play();
    if (!r.ok) this.statusText.setText(r.reason);
    else this.stopCountdown();
  }

  onPass() {
    const r = this.gameModel.pass();
    if (!r.ok) this.statusText.setText(r.reason);
    else this.stopCountdown();
  }

  onTributeReturn() {
    const r = this.gameModel.tributeReturn();
    if (!r.ok) this.statusText.setText(r.reason);
    else this.stopCountdown();
  }

  // ---- 事件分发 ----
  onEvent(ev) {
    if (ev.type === 'deal') {
      this.hideSettle();
      this.clearTeammateHand(); // 新一局：清掉上一局的队友手牌只读展示
      this.stopCountdown();
      this.arranged = [];
      this.flushCursor = {};
      this.handleDeal(ev);
    } else if (ev.type === 'resume') {
      // 读档恢复：跳过发牌动画，直接渲染恢复后的牌桌状态。
      this.hideSettle();
      this.clearTeammateHand(); // 读档恢复：先清，render 会按恢复状态重建
      this.stopCountdown();
      this.arranged = [];
      this.flushCursor = {};
      this.clearTrick();
      this.clearFly();
      this.clearBombFx();
      for (const view of this.cardViews.values()) view.destroy();
      this.cardViews.clear();
      for (const corner of this.cornerViews.values()) corner.destroy();
      this.cornerViews.clear();
      this.clearLocks();
      this.dealing = false;
      this.statusText.setText(`已恢复上一局（${ev.actions ?? 0} 步）`);
      this.render(ev.snapshot ?? this.gameModel.snapshot());
      // 恢复的各家最后出牌由 lastBySeat 还原表现层
      this.restoreTricks();
    } else if (ev.type === 'play') {
      // ev.level 是 submit 前捕获的级牌：最后一手结算后引擎已升级级牌，纹理只预热了旧级牌，
      // 必须用当时级牌渲染，否则 makeCardView 抛"主题纹理缺失"并中断结算流程。
      this.animatePlay(ev.seat, ev.action, false, ev.level);
    } else if (ev.type === 'tribute') {
      this.animateTribute(ev.event);
    } else if (ev.type === 'settle') {
      this.stopCountdown();
      this.statusText.setText(`第 ${ev.summary?.roundNo ?? '?'} 局结束`);
      // 结算时级牌可能已升级，剩余手牌要用新 level 的主题纹理；先异步预热再弹结算层。
      const lv = this.gameModel.levelRankLabel();
      const ready = this.skin?.loaded
        ? this.skin.warmup(rankCode(lv)).catch(() => {})
        : Promise.resolve();
      ready.then(() => this.showSettle(ev.summary, ev.ended));
    } else if (ev.type === 'error') {
      this.statusText.setText(`房间错误：${ev.message}`);
    }
  }

  // ================= 发牌动画（独立一步） =================
  // 收到 deal 事件 → 预热主题纹理 → 把 27 张真牌逐张从中央牌堆飞到"我"的手牌区 →
  // 其余三家只递增手牌数（不飞牌）→ 全部落位后换成正式手牌视图并 beginPlay。
  async handleDeal(ev) {
    if (this.dealing) return;
    this.dealing = true;
    this.clearTrick();
    this.clearFly();
    this.clearBombFx();
    for (const view of this.cardViews.values()) view.destroy();
    this.cardViews.clear();
    for (const corner of this.cornerViews.values()) corner.destroy();
    this.cornerViews.clear();
    this.clearLocks();
    this.statusText.setText('发牌中…');

    // 等主题加载完成（若仍在进行）。保证从第一张飞牌起就是主题牌面，发完不再突变。
    if (this.themeReady) {
      try { await this.themeReady; }
      catch (e) { console.warn('[theme] 等待主题失败：', e.message); }
    }

    // 主题纹理预热（发牌前完成，动画里才有主题牌面）
    const rankCode = (label) => {
      if (!label) return 0;
      if (label === 'A') return 1;
      if (label === 'J') return 11;
      if (label === 'Q') return 12;
      if (label === 'K') return 13;
      return Number(label);
    };
    // 主题是硬依赖：预热失败会让 makeCardView 抛错中止，这里不再有字卡兜底。
    await this.skin.warmup(rankCode(this.gameModel.levelRankLabel()));

    // 真实的 27 张手牌 + 真实落点（与正式手牌布局完全一致，落位后无缝切换）。
    const snap = this.gameModel.snapshot();
    const hand = snap.hand ?? [];
    const level = this.gameModel.levelRankLabel();
    const layout = this.layoutHand(hand, level);
    const CARD_DELAY = 55;
    const CARD_DURATION = 340;
    const totalMs = (Math.max(1, hand.length) - 1) * CARD_DELAY + CARD_DURATION;

    // 只给自己发牌：27 张真牌全部亮牌面，逐张飞到最终落点。
    const flights = hand.map((id, i) => {
      const t = layout.get(id) ?? { x: DECK_X, y: SLOTS.bottom.handY };
      return this.flyDealCard(id, t.x, t.y, i, i * CARD_DELAY, CARD_DURATION, level);
    });
    // 其余三家不飞牌，只把手牌数从 0 递增到真实值（与发牌节奏同步）。
    const others = (ev.hands ?? []).filter((h) => h.seat !== this.gameModel.humanSeat);
    const counters = this.animateCountUp(others, totalMs, hand.length);

    await Promise.all(flights.concat([counters]));

    // 飞牌落位后统一放大到手牌尺寸，再无缝切成正式手牌视图（避免落位瞬间变大的突兀）。
    await this.expandFlyToHand();
    this.clearFly();

    // 真牌已落位：销毁飞牌，换成正式手牌视图（可点选/拖动/切布局）。
    this.render(this.gameModel.snapshot());
    this.dealing = false;
    this.statusText.setText(`发牌完成 · ${hand.length} 张全部到手`);
    this.gameModel.beginPlay();
  }

  // 飞牌全程用小尺寸（不遮挡牌桌），落位后一起放大到手牌尺寸 1。
  // 落点与正式手牌完全一致，放大后 clearFly + render 即可无缝衔接。
  expandFlyToHand() {
    return new Promise((resolve) => {
      const cards = this.flyViews.slice();
      if (!cards.length) { resolve(); return; }
      let left = cards.length;
      cards.forEach((c) => {
        this.tweens.add({
          targets: c, scaleX: 1, scaleY: 1,
          duration: 180, ease: 'Quad.easeOut',
          onComplete: () => { left -= 1; if (left === 0) resolve(); },
        });
      });
    });
  }

  // 单张发牌：从中央牌堆飞向"我"的手牌区最终位置，亮真牌面。
  // 飞行中用 FLY_SCALE（0.62）小尺寸，避免 27 张满尺寸牌在屏幕中间堆叠遮挡。
  flyDealCard(id, targetX, targetY, index, delayMs, duration, level) {
    return new Promise((resolve) => {
      const FLY_SCALE = 0.62;
      const card = this.makeCardView(id, DECK_X, DECK_Y, {
        scale: FLY_SCALE * 0.5, faceUp: true, level,
      });
      this.flyViews.push(card);
      // 必须与正式手牌同规则：cols 模式下 depth 按落点 y 递增（下压上），
      // 否则飞牌按发牌顺序叠，动画中上牌盖住下牌角标；落位 render 才恢复。
      // fan 模式正式手牌统一 depth=12，靠创建顺序横叠，飞牌保持一致。
      card.setDepth(this.handStyle === 'cols' ? 20 + Math.round(targetY / 8) : 12);
      card.rotation = index % 2 ? 0.06 : -0.06; // 起手略带倾角，飞行中平滑归零
      this.tweens.add({
        targets: card,
        x: targetX,
        y: targetY,
        scaleX: FLY_SCALE,
        scaleY: FLY_SCALE,
        rotation: 0,
        duration: duration ?? 300,
        delay: delayMs,
        ease: 'Cubic.easeOut',
        onComplete: resolve,
      });
    });
  }

  // 发牌期间递增手牌数。mineCount 可选：给自己用（配合 27 张真牌飞牌），
  // 不传则只按 hands 递增其余座位。
  animateCountUp(hands, duration, mineCount = null) {
    return new Promise((resolve) => {
      const state = { t: 0 };
      const targets = hands.map((h) => ({
        label: this.slotLabels[SLOT_ORDER[this.gameModel.relSlot(h.seat)]],
        seat: h.seat,
        count: h.count,
      }));
      const mineLabel = this.slotLabels[SLOT_ORDER[this.gameModel.relSlot(this.gameModel.humanSeat)]];
      this.tweens.add({
        targets: state,
        t: 1,
        duration,
        ease: mineCount == null ? 'Quad.easeOut' : 'Linear',
        onUpdate: () => {
          targets.forEach((x) => {
            if (!x.label) return;
            const n = Math.round(x.count * state.t);
            x.label.setText(`座位 ${x.seat}\n手牌 ${n}`);
          });
          if (mineCount != null && mineLabel) {
            mineLabel.setText(`你\n手牌 ${Math.round(mineCount * state.t)}`);
          }
        },
        onComplete: resolve,
      });
    });
  }

  // ================= 进贡飞牌动画 =================
  // 贡方 → 受贡方的弧线飞行，带"贡/还"标签；飞完调用 game.tributeDone() 放行主循环。
  animateTribute(event) {
    if (!event) {
      this.gameModel.tributeDone();
      return;
    }
    const fromSlot = SLOT_ORDER[this.gameModel.relSlot(event.from)];
    const toSlot = SLOT_ORDER[this.gameModel.relSlot(event.to)];
    const from = SLOTS[fromSlot];
    const to = SLOTS[toSlot];
    const dx = to.cx - from.cx;
    const dy = to.cy - from.cy;
    const dist = Math.max(1, Math.hypot(dx, dy));
    // 弧线控制点：中点向上偏移
    const midX = (from.cx + to.cx) / 2 - (dy / dist) * 60;
    const midY = (from.cy + to.cy) / 2 + (dx / dist) * 60;

    const card = this.makeCardView(event.card, from.cx, from.cy, {
      scale: 0.9, faceUp: true, level: this.gameModel.levelRankLabel(),
    });
    card.setDepth(60);
    this.flyViews.push(card);

    const tag = this.add.text(from.cx, from.cy - 44, event.return ? '还' : '贡', {
      fontFamily: 'sans-serif', fontSize: '19px', color: '#ffe08a', fontStyle: 'bold',
      backgroundColor: '#0b3f2a', padding: { x: 6, y: 3 },
    }).setOrigin(0.5).setDepth(61);
    this.flyViews.push(tag);
    this.tweens.add({ targets: tag, x: to.cx, y: to.cy - 44, duration: 620, ease: 'Sine.easeInOut' });

    const curve = new Phaser.Curves.QuadraticBezier(
      new Phaser.Math.Vector2(from.cx, from.cy),
      new Phaser.Math.Vector2(midX, midY),
      new Phaser.Math.Vector2(to.cx, to.cy),
    );
    const follower = { t: 0 };
    this.tweens.add({
      targets: follower,
      t: 1,
      duration: 620,
      ease: 'Sine.easeInOut',
      onUpdate: () => {
        const pt = curve.getPoint(follower.t);
        card.x = pt.x;
        card.y = pt.y;
        card.rotation = Math.sin(follower.t * Math.PI) * 0.35;
      },
      onComplete: () => {
        this.tweens.add({
          targets: [card, tag],
          alpha: 0,
          scale: 0.4,
          duration: 180,
          onComplete: () => {
            card.destroy();
            tag.destroy();
            this.flyViews = this.flyViews.filter((v) => v !== card && v !== tag);
            this.gameModel.tributeDone();
          },
        });
      },
    });
  }

  // ================= 出牌动画 =================
  // 每个座位只保留最近一手：出牌前先销毁该座位上一手的视图，避免多墩叠在一起看不清。
  clearSeatTrick(seat) {
    const old = this.trickViews.get(seat);
    if (old) old.forEach((v) => v.destroy());
    this.trickViews.set(seat, []);
  }

  animatePlay(seat, action, silent = false, level = null) {
    if (!action || action.tributeReturn) return;
    // 未指定时用当前级牌（bot 出牌/读档还原走这条）；结算最后一手会显式传入升级前级牌。
    level = level ?? this.gameModel.levelRankLabel();
    const slot = SLOT_ORDER[this.gameModel.relSlot(seat)];
    const p = SLOTS[slot];
    this.clearSeatTrick(seat);
    const views = this.trickViews.get(seat);
    if (action.pass) {
      const t = this.add.text(p.cx, p.cy, '过', {
        fontFamily: 'sans-serif', fontSize: '32px', color: '#cfe8da', fontStyle: 'bold',
      }).setOrigin(0.5).setAlpha(0);
      views.push(t);
      this.tweens.add({ targets: t, alpha: 1, y: p.cy - 16, duration: 180, yoyo: true, hold: 400 });
      return;
    }
    // 展示排序：调用 wasm 纯函数把这一手按牌型规范重排（顺子从小到大、三代二三个在前等）。
    // 只改桌面展示顺序，不碰判定/存档；炸弹识别仍用原始 cards 顺序。
    const rawCards = action.cards ?? [];
    const cards = this.gameModel.wasm.displayOrder
      ? this.gameModel.wasm.displayOrder(rawCards, action.patternId ?? '', this.rankLabelToIndex(level))
      : rawCards.slice();
    // 出牌区牌比手牌小一号（手牌 86×120，出牌 53×74），避免盖住徽标/操作栏。
    const PLAY_SCALE = 0.62;
    // 一手牌按紧凑间距铺开，避免长顺子/钢板挤成一坨；牌多时缩小间距。
    const gap = cards.length > 8 ? 20 : 30;
    cards.forEach((id, i) => {
      const fromX = slot === 'bottom' ? SLOTS.bottom.cx : p.cx;
      const fromY = slot === 'bottom' ? SLOTS.bottom.handY : p.cy + 40;
      const c = this.makeCardView(id, fromX, fromY, {
        scale: PLAY_SCALE * 0.92, faceUp: true, level,
      });
      views.push(c);
      const ox = (i - (cards.length - 1) / 2) * gap;
      c.setDepth(10 + i);
      this.tweens.add({
        targets: c, x: p.cx + ox, y: p.cy, scale: PLAY_SCALE, duration: 260,
        ease: 'Back.easeOut', delay: i * 30,
      });
    });
    // 炸弹优先用引擎 patternId 判定（含红配补位的炸弹）；step 的 applied 不带时回退牌结构识别。
    // silent=true 用于读档还原，不重播特效。
    const bomb = detectBomb(rawCards, action.patternId);
    if (bomb && !silent) this.playBombFx(seat, bomb, views);
  }

  // ================= 炸弹特效 =================
  // 全屏闪光 + 镜头震动 + 冲击波/放射线 + 火花 + 中央横幅；纯表现层，不阻塞主循环。
  playBombFx(seat, bomb, cardViews = []) {
    this.clearBombFx(); // 连续炸弹只保留最新一个，避免特效叠加糊成一片
    const slot = SLOT_ORDER[this.gameModel.relSlot(seat)];
    const p = SLOTS[slot];
    const cx = p.cx;
    const cy = p.cy;
    const skin = {
      rank: { main: '#ff8a3d', glow: 0xff8a3d, sub: `${bomb.count} 张同点 · 全场翻倍` },
      joker: { main: '#ffd76a', glow: 0xffd000, sub: '四王 · 全场翻倍' },
      flush: { main: '#8be9ff', glow: 0x49d6ff, sub: `${bomb.count} 张同花顺 · 全场翻倍` },
    }[bomb.kind] ?? { main: '#ff8a3d', glow: 0xff8a3d, sub: '全场翻倍' };

    // 1) 全屏闪光
    const flash = this.add.rectangle(640, H / 2, W, H, 0xffffff, 0).setDepth(180);
    this.bombFx.push(flash);
    this.tweens.add({
      targets: flash, alpha: { from: 0, to: 0.46 }, duration: 90,
      yoyo: true, hold: 40,
      onComplete: () => this.forgetBombFx(flash),
    });

    // 2) 镜头震动（特效期间轻微抖，不影响点击）
    this.cameras.main.shake(260, 0.005);

    // 3) 冲击波 + 放射线：一个 tween 驱动重绘，避免生成几十个 Graphics 对象。
    const fire = this.add.graphics().setDepth(181);
    const wave = { t: 0 };
    this.bombFx.push(fire, wave);
    this.tweens.add({
      targets: wave, t: 1, duration: 760, ease: 'Cubic.easeOut',
      onUpdate: () => {
        const t = wave.t;
        const r = 26 + t * 260;
        const fade = 1 - t * 0.72;
        fire.clear();
        // 中心亮球
        fire.fillStyle(0xffffff, (1 - t) * 0.5);
        fire.fillCircle(cx, cy, r * 0.34);
        fire.lineStyle(8 * (1 - t) + 1, skin.glow, fade);
        fire.strokeCircle(cx, cy, r);
        fire.lineStyle(3 * (1 - t) + 1, 0xffffff, fade * 0.65);
        fire.strokeCircle(cx, cy, r * 0.72);
        for (let i = 0; i < 16; i += 1) {
          const a = (i / 16) * Math.PI * 2 + t * 0.18;
          const r0 = r * 0.5;
          const r1 = r * (1 + 0.16 * Math.sin(i * 2.3));
          fire.lineStyle(5 * (1 - t) + 1, i % 2 ? skin.glow : 0xffffff, fade * 0.95);
          fire.beginPath();
          fire.moveTo(cx + Math.cos(a) * r0, cy + Math.sin(a) * r0);
          fire.lineTo(cx + Math.cos(a) * r1, cy + Math.sin(a) * r1);
          fire.strokePath();
        }
      },
      onComplete: () => { this.forgetBombFx(fire); this.forgetBombFx(wave); },
    });

    // 4) 火花四散
    for (let i = 0; i < 18; i += 1) {
      const a = (i / 18) * Math.PI * 2 + Math.random() * 0.3;
      const dist = 90 + Math.random() * 150;
      const dot = this.add.circle(cx, cy, 4 + Math.random() * 5, i % 3 === 0 ? 0xffffff : skin.glow, 1)
        .setDepth(182);
      this.bombFx.push(dot);
      this.tweens.add({
        targets: dot,
        x: cx + Math.cos(a) * dist,
        y: cy + Math.sin(a) * dist + 40,
        alpha: 0, scale: 0.2,
        duration: 680 + Math.random() * 300,
        ease: 'Cubic.easeOut',
        onComplete: () => this.forgetBombFx(dot),
      });
    }

    // 5) 中央横幅（depth 190，低于结算层 200）
    const banner = this.add.container(640, 302).setDepth(190).setScale(0.2);
    const bg = this.add.graphics();
    bg.fillStyle(0x1a0e08, 0.9).fillRoundedRect(-190, -58, 380, 116, 16);
    bg.fillStyle(skin.glow, 0.18).fillRoundedRect(-184, -52, 368, 104, 12);
    bg.lineStyle(3, skin.glow, 1).strokeRoundedRect(-190, -58, 380, 116, 16);
    const title = this.add.text(0, -16, `${bomb.label}！`, {
      fontFamily: 'sans-serif', fontSize: '52px', color: skin.main,
      fontStyle: 'bold', stroke: '#2b1000', strokeThickness: 6,
    }).setOrigin(0.5);
    const sub = this.add.text(0, 32, `${this.gameModel.seatLabel(seat)} 打出 · ${skin.sub}`, {
      fontFamily: 'sans-serif', fontSize: '16px', color: '#ffe9c0',
    }).setOrigin(0.5);
    banner.add([bg, title, sub]);
    this.bombFx.push(banner);
    this.tweens.add({ targets: banner, scale: 1, duration: 280, ease: 'Back.easeOut' });
    this.tweens.add({ targets: title, scaleX: 1.06, scaleY: 0.94, duration: 90, yoyo: true, repeat: 3 });
    this.bombTimer = this.time.delayedCall(1150, () => {
      this.bombTimer = null;
      this.tweens.add({
        targets: banner, alpha: 0, y: 274, duration: 240,
        onComplete: () => this.forgetBombFx(banner),
      });
    });

    // 6) 出牌炸开：该座位刚打出的牌轻微弹跳，强化"炸"的读感。
    cardViews.forEach((v, i) => {
      if (!v || !v.active) return;
      // 相对当前 scale 小幅放大，不写死 1.1（出牌区牌已缩到 0.62）。
      const s0 = v.scaleX || 1;
      this.tweens.add({
        targets: v, y: v.y - 8 - i * 2, scaleX: s0 * 1.1, scaleY: s0 * 1.1,
        duration: 100, yoyo: true, repeat: 2, ease: 'Sine.easeInOut',
      });
    });
  }

  forgetBombFx(obj) {
    this.bombFx = this.bombFx.filter((v) => v !== obj);
  }

  clearBombFx() {
    if (this.bombTimer) {
      this.bombTimer.remove();
      this.bombTimer = null;
    }
    for (const v of this.bombFx) {
      this.tweens.killTweensOf(v);
      v.destroy?.();
    }
    this.bombFx = [];
  }

  // 读档后按 lastBySeat 还原四家最后出的牌（静态展示，不再重播动画）。
  restoreTricks() {
    this.clearTrick();
    const last = this.gameModel.lastBySeat ?? {};
    for (const seat of Object.keys(last)) {
      const rec = last[seat];
      if (!rec || rec.pass || (rec.cards?.length ?? 0) === 0) continue;
      this.animatePlay(Number(seat), { pass: false, cards: rec.cards }, true);
    }
  }

  clearTrick() {
    this.trickViews.forEach((views) => views.forEach((v) => v.destroy()));
    this.trickViews.clear();
  }

  // 我出完牌后：把队友（对家）的真实剩余手牌摊在「我的出牌区」下方，仅供查看。
  // 只读保证：不进 cardViews、不挂 pointerdown、不 setInteractive；纯 image + 文字标签。
  // 结算弹窗出现时（offsetY）整体下移到弹窗下方，避免被中央弹窗压住看不到。
  showTeammateHand(offsetY = 0) {
    const g = this.gameModel;
    const teammate = (g.humanSeat + 2) % 4; // 掼蛋对家 = 同队队友
    let cards = [];
    try { cards = g.wasm.view(teammate)?.hand?.cards ?? []; } catch { /* 视图不可用则不展示 */ }
    // 队友已出完（无牌）时清掉旧展示。
    if (!cards.length) { this.clearTeammateHand(); return; }

    const rank = g.wasm.info()?.seats?.find((s) => s.seat === teammate)?.rank ?? 0;
    const sig = `${rank}|${cards.slice().sort((a, b) => a - b).join(',')}|${offsetY}`;
    if (this.teammateSig === sig) return; // 手牌/名次/位置没变，不重建（render 高频触发）
    this.teammateSig = sig;
    this.clearTeammateHand();

    const level = g.levelRankLabel();
    const p = SLOTS.bottom;
    // 我的出牌区在 (640,372)，结算弹窗压住 150~570；offsetY 时搬到弹窗下方 y≈600。
    const anchorX = p.cx;
    const anchorY = offsetY ? 610 : p.cy + 58;
    const SCALE = 0.4;
    const cw = CARD_W * SCALE;
    const rowStep = 13;
    const cols = new Map();
    for (const id of cards) {
      const key = columnKey(id, level);
      if (!cols.has(key)) cols.set(key, []);
      cols.get(key).push(id);
    }
    const keys = [...cols.keys()].sort((a, b) => b - a);
    const colGap = cw * 0.6;
    const totalW = (keys.length - 1) * colGap;
    // 同点数叠列必须「下面的牌压上面的牌」，否则上牌盖住下牌角标看不到所有数字。
    // ri=0 最下、ri 越大越往上；depth 随 y 递增（y 越大越靠下 depth 越高）。
    const maxRows = Math.max(1, ...[...cols.values()].map((ids) => ids.length));
    keys.forEach((key, ci) => {
      const ids = cols.get(key).slice().sort((a, b) => a - b);
      const x = anchorX - totalW / 2 + ci * colGap;
      ids.forEach((id, ri) => {
        const y = anchorY - ri * rowStep;
        const view = this.makeCardView(id, x, y, { scale: SCALE, faceUp: true, level });
        // 基础 208 在结算遮罩(200)/弹窗(201~204)之上；叠列内按行号递增。
        view.setDepth(208 + (maxRows - 1 - ri));
        this.teammateViews.push(view);
      });
    });
    const rankText = rank >= 1 && rank <= 4 ? RANK_LABELS[rank] : '';
    // 标签按牌的实际上边缘定位，留出固定间隔，避免压住牌面。
    // 牌中心 y = anchorY - ri*rowStep，牌半高 = CARD_H*SCALE/2；标签 origin 0.5、高约 20。
    const cardHalfH = (CARD_H * SCALE) / 2;
    const labelHalfH = 12; // 13px 字 + padding(3*2) ≈ 23px 高
    const gap = 24;
    const topCardY = anchorY - (maxRows - 1) * rowStep;
    // 结算时弹窗底边 y=570：标签放牌下方，否则上移会钻进弹窗被遮住。
    const labelY = offsetY
      ? anchorY + cardHalfH + gap + labelHalfH
      : topCardY - cardHalfH - gap - labelHalfH;
    const label = this.add.text(anchorX, labelY,
      `队友（座位 ${teammate}）手牌 ${cards.length} 张${rankText ? ' · ' + rankText : ''} · 仅查看`, {
        fontFamily: 'sans-serif', fontSize: '13px', color: '#ffe9a8', fontStyle: 'bold',
        backgroundColor: '#0f2d3a', padding: { x: 6, y: 3 },
      }).setOrigin(0.5).setDepth(208 + maxRows + 1); // 高于所有叠列牌
    this.teammateViews.push(label);
  }

  clearTeammateHand() {
    this.teammateViews.forEach((v) => v.destroy());
    this.teammateViews = [];
    this.teammateSig = '';
  }

  clearFly() {
    this.flyViews.forEach((v) => v.destroy());
    this.flyViews = [];
  }

  // ---- 结算覆盖层（参考设计图：大标题 + 名次 + 继续按钮） ----
  hideSettle() {
    if (this.settleLayer) {
      this.settleLayer.forEach((v) => v.destroy());
      this.settleLayer = null;
    }
  }

  // 牌局结束：其他三家未出完的手牌用小号 cols 亮在其牌区。
  // 用引擎真实 view(seat).hand.cards；depth 210 在全屏遮罩(200)之上，位置避开中央结算弹窗。
  showSettleRemainingHands(L) {
    const g = this.gameModel;
    const level = g.levelRankLabel();
    for (const seat of [1, 2, 3]) {
      if (seat === g.humanSeat) continue;
      let cards = [];
      try { cards = g.wasm.view(seat)?.hand?.cards ?? []; } catch { /* 视图不可用则跳过 */ }
      if (!cards.length) continue; // 已出完（头游/二游）不展示
      const slot = SLOT_ORDER[g.relSlot(seat)];
      const p = SLOTS[slot];
      const isTop = slot === 'top';
      const anchorX = p.cx;
      // 顶部牌区 x=640 正对中央弹窗，锚点下移、牌往上叠，避免越出屏幕顶；标签放牌下方。
      const anchorY = isTop ? 118 : p.handY;
      // 小号 cols：同点数一列、列内小幅纵向错开，整体居中在牌区锚点。
      const SCALE = 0.42;
      const cw = CARD_W * SCALE;
      const rowStep = isTop ? 12 : 15;
      const cols = new Map();
      for (const id of cards) {
        const key = columnKey(id, level);
        if (!cols.has(key)) cols.set(key, []);
        cols.get(key).push(id);
      }
      const keys = [...cols.keys()].sort((a, b) => b - a);
      const colGap = cw * 0.62; // 小牌列稍重叠，紧凑
      const totalW = (keys.length - 1) * colGap;
      // 下压上：ri=0 最下 depth 最高，上牌不盖住下牌角标。
      const maxRows = Math.max(1, ...[...cols.values()].map((ids) => ids.length));
      keys.forEach((key, ci) => {
        const ids = cols.get(key).slice().sort((a, b) => a - b);
        const x = anchorX - totalW / 2 + ci * colGap;
        ids.forEach((id, ri) => {
          const y = anchorY - ri * rowStep;
          const view = this.makeCardView(id, x, y, { scale: SCALE, faceUp: true, level });
          view.setDepth(210 + (maxRows - 1 - ri));
          L.push(view);
        });
      });
      // 剩余张数标签：顶部放牌下方，其余放牌上方（避开牌区）。
      const labelY = isTop
        ? anchorY + 26
        : anchorY - (maxRows - 1) * rowStep - 38;
      const label = this.add.text(anchorX, labelY,
        `座位 ${seat} 剩 ${cards.length} 张`, {
          fontFamily: 'sans-serif', fontSize: '13px', color: '#ffe9a8', fontStyle: 'bold',
          backgroundColor: '#0f2d3a', padding: { x: 6, y: 3 },
        }).setOrigin(0.5).setDepth(210 + maxRows + 1);
      L.push(label);
    }
  }

  showSettle(summary, ended) {
    this.hideSettle();
    const L = [];
    // 牌局结束：把其他三家未出完的真实手牌用小号 cols 亮在其牌区（遮罩之上，不挡结算弹窗）。
    this.showSettleRemainingHands(L);
    const shade = this.add.rectangle(640, H / 2, W, H, 0x0a1a20, 0.62).setDepth(200);
    L.push(shade);
    const card = this.add.graphics().setDepth(201);
    card.fillStyle(0x123c44, 1).fillRoundedRect(390, 150, 500, 420, 20);
    card.lineStyle(2, 0xf0d9a0, 0.9).strokeRoundedRect(390, 150, 500, 420, 20);
    L.push(card);

    const title = this.add.text(640, 210, ended ? '全场结束' : '本局结束', {
      fontFamily: 'serif', fontSize: '46px', color: '#ffd76a', fontStyle: 'bold',
    }).setOrigin(0.5).setDepth(202);
    L.push(title);
    const ranks = summary?.ranks ?? [];
    const winner = summary?.winnerTeam;
    const sub = this.add.text(640, 268, `名次  ${ranks.join('  ·  ')}   头游队 ${winner === 0 ? '我方' : winner === 1 ? '对方' : '-'}`, {
      fontFamily: 'sans-serif', fontSize: '16px', color: '#d8ecdf',
    }).setOrigin(0.5).setDepth(202);
    L.push(sub);
    // 名次徽章：四个小圆
    ranks.forEach((seat, i) => {
      const bx = 520 + i * 80;
      const g = this.add.graphics().setDepth(202);
      g.fillStyle(i === 0 ? 0xf3c34a : 0x2d6b7a, 1).fillCircle(bx, 330, 22);
      L.push(g);
      L.push(this.add.text(bx, 330, `${i + 1}`, {
        fontFamily: 'sans-serif', fontSize: '18px', color: '#fff', fontStyle: 'bold',
      }).setOrigin(0.5).setDepth(203));
      L.push(this.add.text(bx, 366, `${ranks[i]}号`, {
        fontFamily: 'sans-serif', fontSize: '12px', color: '#cfe6e8',
      }).setOrigin(0.5).setDepth(203));
    });

    const nextBtn = this.makeButton(640, 440, ended ? '再来一局' : '下一局', () => {
      if (ended) this.onNewGame();
      else this.gameModel.nextRound();
    }, 0xb8860b, 200, 52);
    nextBtn.setDepth(203);
    nextBtn.labelText.setDepth(204);
    L.push(nextBtn);
    L.push(nextBtn.labelText);
    this.statusText.setText(ended ? '全场结束' : '本局结算');
    this.settleLayer = L;
    // 结算弹窗已就位：把队友手牌展示从出牌区挪到弹窗下方（render 传 settleLayer 判断需要重新定位）。
    if (this.lastSnapshot) this.render(this.lastSnapshot);
  }

  // ---- 渲染（响应 state 变化） ----
  render(snap) {
    if (this.dealing) return; // 发牌阶段由动画自己驱动显示
    this.lastSnapshot = snap;
    const info = snap.info ?? {};
    // 记分板：双方各自级牌 + 打级方黄框高亮；不再显示箭头/当前行动方。
    this.refreshScoreBoard(info.attackingTeam, info.levels);

    const seats = info.seats ?? [];
    seats.forEach((s) => {
      const slot = SLOT_ORDER[this.gameModel.relSlot(s.seat)];
      const label = this.slotLabels[slot];
      const mine = s.seat === snap.humanSeat;
      label.setText(`${mine ? '你' : '座位 ' + s.seat}\n手牌 ${s.hand ?? 0}`);
      label.setColor(mine ? '#ffe9a8' : '#e9fff3');
      // 名次徽标：头游/二游/三游/末游
      const rankText = this.slotRankTexts?.[slot];
      if (rankText) {
        const r = Number(s.rank) || 0;
        if (r >= 1 && r <= 4) {
          rankText.setText(RANK_LABELS[r]).setVisible(true);
        } else {
          rankText.setVisible(false);
        }
      }
    });

    const myTurn = snap.myTurn;
    // 我已出完（拿到名次）：牌局可能仍在等其他三家打完，此时轮次不该再算到我头上，
    // 否则会出现"手牌已空却提示轮到你出牌/要不起"的矛盾。已出完就不参与出牌交互。
    const myRank = snap.info?.seats?.find((s) => s.seat === snap.humanSeat)?.rank ?? 0;
    const iAmFinished = myRank > 0;
    const canAct = myTurn && !iAmFinished; // 真正需要我操作的回合

    // 底部反馈：已出完→等待结算；还贡提示；等待他人；"轮到你出牌"由中央闹钟承担。
    this.statusText.setText(iAmFinished
      ? `你已出完（第 ${myRank} 名）· 等待其他人`
      : (snap.phase === 'tribute' && canAct
        ? '轮到你还贡（选一张 ≤10）'
        : (!canAct && snap.waitingSeat !== null ? `等待座位 ${snap.waitingSeat} 行动…` : '')));
    // 还贡阶段：只显示"还贡"按钮（选中合规牌后启用）；其余四按钮隐藏。
    // 出牌阶段：显示 出牌/提示/过牌 或"要不起"；已出完/非我回合隐藏全部。
    const isTributeTurn = canAct && snap.phase === 'tribute';
    const hasReturnPick = isTributeTurn && (snap.selected?.size ?? 0) === 1;
    this.returnBtn.setBtnVisible(isTributeTurn);
    this.returnBtn.setAlpha(hasReturnPick ? 1 : 0.5);
    const mustPass = canAct && snap.phase === 'play' && !this.hasPlayableCards(snap);
    [this.playBtn, this.hintBtn, this.passBtn].forEach((b) => b.setBtnVisible(canAct && !isTributeTurn && !mustPass));
    this.cannotBtn.setBtnVisible(canAct && !isTributeTurn && mustPass);
    const dim = canAct ? 1 : 0.5;
    [this.playBtn, this.hintBtn, this.passBtn].forEach((b) => b.setAlpha(dim));
    this.cannotBtn.setAlpha(1);

    this.updateCountdown(snap, iAmFinished);
    this.renderHand(snap);
    this.syncSuitTabs(snap);
    // 我出完牌后：把队友手牌只读摊在我的出牌区（结算弹窗出现时下移避让）。
    if (iAmFinished) this.showTeammateHand(this.settleLayer ? 1 : 0);
    else this.clearTeammateHand();
  }

  // 四花色按钮：能凑出同花顺→用主题花色墨色高亮、可点；否则灰显、不可点。
  // 颜色取 skin.suitInk（palette.suits 优先，回落 red/black），与牌面花色一致。
  syncSuitTabs(snap) {
    if (!this.suitButtons) return;
    const cards = snap.hand ?? [];
    const myTurn = snap.myTurn;
    this.suitButtons.forEach((btn, suit) => {
      const opts = this.flushWindows(suit, this.poolForSuit(cards, suit));
      const enabled = myTurn && opts.length > 0;
      // 让 pointerout 回到主题墨色而非构造时的固定色。
      const ink = this.skin?.suitInk?.(suit);
      const lit = ink ?? 0xdd0808;
      btn.baseColor = enabled ? lit : 0x3a4a44;
      btn.setFillStyle(btn.baseColor, 1);
      btn.labelText.setColor(enabled ? '#ffffff' : '#7f8c88');
      btn.setAlpha(1);
      if (enabled) btn.setInteractive({ useHandCursor: true });
      else btn.disableInteractive();
    });
  }

  // 稳定排序一列内的牌：columnKey 降序 → 花色 → copy（对齐 web sortedCards）。
  sortedColumn(ids, level) {
    return [...ids].sort((a, b) =>
      columnKey(b, level) - columnKey(a, level) || suitIndexOf(a) - suitIndexOf(b) || (a % 2) - (b % 2));
  }

  // 计算整手牌落点：cols = 同点数一列（含理牌分组左/右），fan = 横排铺满。
  // 理牌分组只改布局：left 组排最左、right 组排最右、未理的牌居中按列。
  // 返回 Map<id, {x, y}>，y 为未选中的基准位置。
  layoutHand(hand, level) {
    const pos = new Map();
    const n = Math.max(1, hand.length);
    const colGap = Math.min(92, CARD_W + 4);
    const rowStep = 30;

    // 剪掉已不在手牌里的分组牌（出牌后被消耗）
    const handSet = new Set(hand);
    this.arranged = this.arranged
      .map((g) => ({ ...g, cards: g.cards.filter((id) => handSet.has(id)) }))
      .filter((g) => g.cards.length > 0);
    const groupedIds = new Set(this.arranged.flatMap((g) => g.cards));

    if (this.handStyle === 'cols') {
      const left = this.arranged.filter((g) => g.layout === 'left');
      const right = this.arranged.filter((g) => g.layout === 'right');
      // 未理的牌按同点数分组
      const cols = new Map();
      for (const id of hand) {
        if (groupedIds.has(id)) continue;
        const key = columnKey(id, level);
        if (!cols.has(key)) cols.set(key, []);
        cols.get(key).push(id);
      }
      const midCols = [...cols.keys()].sort((a, b) => b - a).map((k) => this.sortedColumn(cols.get(k), level));
      // 列序列：左组 + 中间散列 + 右组
      const columns = [
        ...left.map((g) => this.sortedColumn(g.cards, level)),
        ...midCols,
        ...right.map((g) => this.sortedColumn(g.cards, level)),
      ];
      const totalW = (columns.length - 1) * colGap;
      columns.forEach((ids, ci) => {
        const x = 640 - totalW / 2 + ci * colGap;
        ids.forEach((id, ri) => pos.set(id, { x, y: HAND_BASE_Y - ri * rowStep }));
      });
    } else {
      // fan：横排铺满全屏。理牌组仍在两端：左组 → 散牌 → 右组。
      const left = this.arranged.filter((g) => g.layout === 'left');
      const right = this.arranged.filter((g) => g.layout === 'right');
      const ordered = [
        ...left.flatMap((g) => this.sortedColumn(g.cards, level)),
        ...this.sortedColumn(hand.filter((id) => !groupedIds.has(id)), level),
        ...right.flatMap((g) => this.sortedColumn(g.cards, level)),
      ];
      const usable = W - 72;
      const maxGap = Math.max(16, Math.floor((usable - CARD_W) / (Math.max(1, ordered.length) - 1)));
      const spacing = Math.min(58, maxGap);
      const startX = 640 - ((ordered.length - 1) * spacing) / 2;
      ordered.forEach((id, i) => pos.set(id, { x: startX + i * spacing, y: HAND_BASE_Y + 6 }));
    }
    return pos;
  }

  renderHand(snap) {
    const hand = snap.hand ?? [];
    const selected = snap.selected ?? new Set();
    const present = new Set(hand);
    const level = this.gameModel.levelRankLabel();
    const layout = this.layoutHand(hand, level);

    for (const [id, view] of this.cardViews) {
      if (!present.has(id)) {
        view.destroy();
        this.cardViews.delete(id);
      }
    }
    // 清理不再在手牌里的角标浮层
    for (const [id, corner] of this.cornerViews) {
      if (!present.has(id)) {
        corner.destroy();
        this.cornerViews.delete(id);
      }
    }
    // 理牌分组的牌集合
    const grouped = new Set(this.arranged.flatMap((g) => g.cards));
    // 清理不再在手牌、或已不在任何分组的锁图标
    for (const [id, lock] of this.lockViews) {
      if (!present.has(id) || !grouped.has(id)) {
        lock.destroy();
        this.lockViews.delete(id);
      }
    }
    hand.forEach((id, index) => {
      const target = layout.get(id);
      let view = this.cardViews.get(id);
      if (!view) {
        view = this.makeCardView(id, target.x, target.y, {
          scale: this.cardScale, faceUp: true, level, interactive: true,
        });
        view.on('pointerdown', () => this.onCardDown(id));
        view.on('pointerover', () => this.onCardOver(id));
        this.cardViews.set(id, view);
      }
      view.setData?.('order', index);
      const isSel = selected.has(id);
      // fan 横叠：选中上移 34 且抬 depth 到 100+index，露出整张牌、不被右邻牌盖住。
      // cols 叠列：选中「不上移、不抬 depth」——每张牌只露出顶部 30px 角标条带，
      //   若选中牌抬高/上移，会盖住紧邻下方牌的角标，导致从上往下拖选时下面牌点不中。
      //   cols 选中态靠金色 tint + 粗金边标识，叠列几何与命中区保持原样。
      const lift = this.handStyle === 'cols' ? 0 : 34;
      const ty = isSel ? target.y - lift : target.y;
      if (Math.abs(view.x - target.x) > 1 || Math.abs(view.y - ty) > 1) {
        this.tweens.add({ targets: view, x: target.x, y: ty, duration: 110, ease: 'Quad.easeOut' });
      }
      const baseDepth = this.handStyle === 'cols' ? 20 + Math.round(target.y / 8) : 12;
      view.setDepth(isSel && this.handStyle !== 'cols' ? 100 + index : baseDepth);
      const border = view.list?.find((c) => c.getData?.('selborder'));
      if (border) {
        border.setStrokeStyle(isSel ? 3 : 1, isSel ? 0xffd45e : 0x9fd8bd, 1);
      } else if (view.setTint) {
        // 主题牌面是 Image：选中用明显金色 tint，未选中还原。
        if (isSel) view.setTint(0xffe08a);
        else view.clearTint();
      }

      // fan 模式：牌面横叠时，宽角标（如 10）或靠内侧的角标可能被右边相邻牌盖住，
      // 造成"有的能看到点数、有的只露花色"。给每张牌叠一个独立角标浮层，
      // depth 高于所有普通牌面、低于选中牌，确保点数恒可见、且随牌选中一起上移。
      if (this.handStyle === 'fan') {
        let corner = this.cornerViews.get(id);
        if (!corner) {
          corner = this.makeCorner(id);
          this.cornerViews.set(id, corner);
        }
        const cy = ty - 26;
        const cx = target.x - 25;
        if (Math.abs(corner.x - cx) > 1 || Math.abs(corner.y - cy) > 1) {
          this.tweens.add({ targets: corner, x: cx, y: cy, duration: 110, ease: 'Quad.easeOut' });
        }
        corner.setDepth(isSel ? 90 + index : 60 + index);
        corner.setAlpha(isSel ? 0.95 : 1);
      }

      // 理牌分组的牌右上角加一枚很小的锁，标识"这是理好的一组"。
      if (grouped.has(id)) {
        let lock = this.lockViews.get(id);
        if (!lock) {
          lock = this.makeLockIcon();
          this.lockViews.set(id, lock);
        }
        // 右上角：随牌面尺寸缩放，选中上移时跟随。
        const halfW = (CARD_W * this.cardScale) / 2;
        const halfH = (CARD_H * this.cardScale) / 2;
        const lx = target.x + halfW - 9 * this.cardScale;
        const ly = ty - halfH + 9 * this.cardScale;
        if (Math.abs(lock.x - lx) > 1 || Math.abs(lock.y - ly) > 1) {
          this.tweens.add({ targets: lock, x: lx, y: ly, duration: 110, ease: 'Quad.easeOut' });
        }
        lock.setDepth(isSel ? 130 + index : 70 + index);
      }
    });
    // cols 模式不需要浮层（牌纵向叠放、角标本就露出），销毁避免残留
    if (this.handStyle !== 'fan') {
      for (const [id, corner] of this.cornerViews) {
        corner.destroy();
        this.cornerViews.delete(id);
      }
    }
  }

  clearLocks() {
    for (const lock of this.lockViews.values()) lock.destroy();
    this.lockViews.clear();
  }

  // 理牌分组小锁图标：一枚很小的金色挂锁（锁体 + 锁梁），画在牌右上角。
  // 主题牌面是 Image 不能挂子对象，故用独立 Graphics 浮层，随牌移动。
  makeLockIcon() {
    const scale = this.cardScale;
    const g = this.add.graphics().setScale(scale);
    // 锁体
    g.fillStyle(0xd4a017, 1).fillRoundedRect(-5, -1, 10, 8, 2);
    g.lineStyle(1, 0x5a3a06, 1).strokeRoundedRect(-5, -1, 10, 8, 2);
    // 锁梁（半圆）
    g.lineStyle(2, 0xd4a017, 1);
    g.beginPath();
    g.arc(0, -1, 3.2, Phaser.Math.DegToRad(180), Phaser.Math.DegToRad(360), false);
    g.strokePath();
    // 锁孔
    g.fillStyle(0x5a3a06, 1).fillCircle(0, 3, 1);
    return g;
  }

  // fan 模式角标浮层：rank + suit 两行，配色跟随主题牌（红桃/方块红，其余黑）。
  // 独立对象、不依赖主题 SVG，后期换 UI 素材也不受影响。
  makeCorner(id) {
    const f = cardFace(id);
    const color = f.red ? '#c0392b' : '#1c2733';
    const c = this.add.container(0, 0);
    const rank = this.add.text(0, 0, f.rank, {
      fontFamily: 'Arial, sans-serif', fontSize: f.rank === '10' ? '15px' : '18px',
      color, fontStyle: 'bold',
    });
    const suit = this.add.text(0, 17, f.suit, {
      fontFamily: 'Arial, sans-serif', fontSize: '15px', color,
    });
    c.add([rank, suit]);
    return c;
  }

  // 统一的牌视图：一律用 gfdeck 主题纹理 Image（boot 已强制主题加载成功）。
  // 纹理缺失属异常情况，直接抛错中止，绝不再回退自画字卡（用户要求必须 gfdeck 主题）。
  // 返回的对象都支持 x/y/scale/rotation/alpha/destroy/setDepth/on/tweens。
  makeCardView(id, x, y, { scale = 1, faceUp = true, level = null, interactive = false } = {}) {
    const texKey = faceUp && this.skin ? this.skin.textureKey(id, level) : null;
    if (!texKey || !this.textures.exists(texKey)) {
      throw new Error(`主题纹理缺失：card=${id} level=${level} faceUp=${faceUp}（必须使用 gfdeck 主题，不回退自画）`);
    }
    // SVG 纹理基准缩放让它落到 CARD_W 宽，再乘外部 scale。
    const tex = this.textures.get(texKey).getSourceImage();
    const base = tex && tex.width ? CARD_W / tex.width : CARD_W / 500;
    const img = this.add.image(x, y, texKey).setScale(base * scale);
    if (interactive) img.setInteractive({ useHandCursor: true });
    return img;
  }

  // 中央牌堆：主题就绪后由 main.js 调用，用 gfdeck 牌背纹理替换占位。
  // 主题是硬依赖，牌背纹理缺失直接抛错，不保留自画占位。
  refreshDeckStack() {
    if (this.deckStack) this.deckStack.destroy();
    if (this.deckStackLabel) this.deckStackLabel.destroy();
    const key = this.skin?.backKey;
    if (!key || !this.textures.exists(key)) {
      throw new Error('主题牌背纹理缺失，牌堆无法渲染（必须使用 gfdeck 主题）');
    }
    // 牌堆比手牌略小：CARD_W 宽对应 scale 1，这里用 0.8 约 69×96。
    this.deckStack = this.makeBack(DECK_X, DECK_Y, 0.8).setDepth(2);
  }

  // 牌背：一律用 gfdeck 主题牌背纹理（主题硬依赖，缺失抛错，不回退自画）。
  makeBack(x, y, scale = 1) {
    const key = this.skin?.backKey;
    if (!key || !this.textures.exists(key)) {
      throw new Error('主题牌背纹理缺失（必须使用 gfdeck 主题，不回退自画牌背）');
    }
    const tex = this.textures.get(key).getSourceImage();
    const base = tex && tex.width ? CARD_W / tex.width : CARD_W / 500;
    return this.add.image(x, y, key).setScale(base * scale);
  }

}

export { H, W };
