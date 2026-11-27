// 牌桌场景：Phaser 3 渲染 + Tween 动画。订阅 Game 的 change/event，纯表现层。
// 逻辑一句都不在这里判定，全部读 Game.snapshot()，点牌回调 game.toggleCard。
// Phaser 由 index.html 以 UMD <script> 引入，走全局 window.Phaser。
import { ASSETS, resolveAsset } from '../assets.js';
import { cardFace, columnKey, detectBomb } from '../cards.js';
import { THEMES, THEME_ORDER } from '../skin.js';

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
const SLOTS = {
  bottom: { name: 'bottom', cx: 640, cy: 385, handY: 648, labelY: 452 },
  right: { name: 'right', cx: 980, cy: 318, handY: 250, labelY: 150 },
  top: { name: 'top', cx: 640, cy: 170, handY: 80, labelY: 78 },
  left: { name: 'left', cx: 300, cy: 318, handY: 250, labelY: 150 },
};
const SLOT_ORDER = ['bottom', 'right', 'top', 'left'];
const DECK_X = 640;
const DECK_Y = 318;

// 操作栏（倒计时 + 功能按钮）位于手牌正上方；留出空间给 cols 模式向上叠放。
const BAR_Y = 490;
const COUNTDOWN_SECONDS = 30;

export class TableScene extends Phaser.Scene {
  constructor(game, skin) {
    super('table');
    this.gameModel = game;
    this.skin = skin;
    this.cardViews = new Map(); // id -> Container（人类手牌）
    this.cornerViews = new Map(); // id -> 角标高亮浮层（fan 模式保证点数恒可见）
    this.trickViews = new Map(); // seat -> 该座位出牌区视图（每座只留最近一手）
    this.flyViews = []; // 飞牌/发牌临时对象
    this.bombFx = []; // 炸弹特效临时对象（闪光/冲击波/火花/横幅）
    this.bombTimer = null; // 横幅退场定时器
    this.lastSnapshot = null;
    this.dealing = false;
    this.handStyle = 'cols'; // 默认按设计图：同点数纵向叠列；?hand=fan 可切横排
    // 出牌倒计时（纯 UI + 交互，不改 Game 状态机）
    this.countdown = { active: false, left: COUNTDOWN_SECONDS, total: COUNTDOWN_SECONDS, timer: null };
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

    // ?hand=cols 切到同点数纵向罗列（参照 web）；默认 fan 横排铺满。
    const handParam = new URLSearchParams(location.search).get('hand');
    if (handParam === 'cols' || handParam === 'fan') this.handStyle = handParam;
    this.updateHandBtn();

    this.statusText.setText('点「新局」开始');
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

    // 中央牌堆占位（发牌从这里飞出）
    this.deckStack = this.add.rectangle(DECK_X, DECK_Y, 68, 94, 0x2b6a52, 0.92)
      .setStrokeStyle(2, 0xffffff, 0.7);
    this.add.text(DECK_X, DECK_Y, '掼蛋', {
      fontFamily: 'serif', fontSize: '21px', color: '#f3ead0',
    }).setOrigin(0.5);

    this.centerSub = this.add.text(DECK_X, 432, '', {
      fontFamily: 'sans-serif', fontSize: '14px', color: '#5c4a2c',
    }).setOrigin(0.5);

    // 顶部：本局打 X（金色徽标）+ 倍数牌 + 局数
    this.makeLevelBadge();
    this.makeMultiplier(1120, 40);
    this.hudText = this.add.text(140, 26, '', {
      fontFamily: 'sans-serif', fontSize: '13px', color: '#334a52', lineSpacing: 2,
    });

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

  makeLevelBadge() {
    const g = this.add.graphics();
    g.fillStyle(ASSETS.levelBadge.color, 1).fillRoundedRect(20, 14, 108, 46, 8);
    g.fillStyle(0xd88408, 1).fillRoundedRect(20, 46, 108, 14, 4);
    this.add.text(30, 26, '本局打', { fontFamily: 'sans-serif', fontSize: '14px', color: '#fff4dd' });
    this.levelBadgeText = this.add.text(120, 36, '2', {
      fontFamily: 'sans-serif', fontSize: '32px', color: '#ffffff', fontStyle: 'bold',
    }).setOrigin(1, 0.5);
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
    const x = p.cx, y = p.labelY;
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
    // 分数/段位（底部小字）
    const tier = isTop ? '2900 · 青铜2' : isBottom ? '2900 · 白银3' : '2900 · 青铜2';
    this.add.text(x - w / 2 + 48, y + 14, tier, {
      fontFamily: 'sans-serif', fontSize: '9px', color: '#9fd0c8',
    }).setDepth(5);
    this.slotLabels[slot] = label;
  }

  makeButton(x, y, label, onClick, color = 0x1d6f57, w = 92, h = 40) {
    const box = this.add.rectangle(x, y, w, h, color, 1)
      .setStrokeStyle(1, 0xffffff, 0.55).setInteractive({ useHandCursor: true });
    const txt = this.add.text(x, y, label, {
      fontFamily: 'sans-serif', fontSize: '15px', color: '#ffffff',
    }).setOrigin(0.5).setDepth(34);
    box.labelText = txt;
    box.on('pointerover', () => box.setFillStyle(0x2b8a6c, 1));
    box.on('pointerout', () => box.setFillStyle(color, 1));
    box.on('pointerdown', onClick);
    return box;
  }

  // 操作栏：倒计时在左，状态居中，功能按钮在右，全部位于手牌上方。
  buildButtons() {
    const y = BAR_Y;
    const bar = this.add.graphics();
    bar.fillStyle(0x0f2d3a, 0.78).fillRoundedRect(20, y - 24, W - 40, 48, 12);
    bar.lineStyle(1, 0xf0d9a0, 0.5).strokeRoundedRect(20, y - 24, W - 40, 48, 12);
    bar.setDepth(30);

    this.statusText = this.add.text(120, y, '', {
      fontFamily: 'sans-serif', fontSize: '15px', color: '#ffe9a8',
    }).setOrigin(0, 0.5).setDepth(34);

    // 右侧主操作（出牌/过牌/提示/新局/手牌切换/主题）
    this.makeButton(620, y, '新局', () => this.onNewGame(), 0x8a5a16, 88);
    this.playBtn = this.makeButton(720, y, '出牌', () => this.onPlay(), 0xb8860b, 88);
    this.passBtn = this.makeButton(820, y, '过牌', () => this.onPass(), 0x2b6f8a, 88);
    this.hintBtn = this.makeButton(920, y, '提示', () => this.gameModel.hint(), 0x2b8a6c, 88);
    this.handBtn = this.makeButton(1040, y, '手牌 列', () => this.toggleHandStyle(), 0x1d6f57, 108);
    this.themeBtn = this.makeButton(1170, y, '主题 肥仔', () => this.cycleTheme(), 0x6a4a8a, 96);
    [this.handBtn, this.playBtn, this.passBtn, this.hintBtn].forEach((b) => b.setDepth(33));
  }

  // ---- 出牌倒计时（UI + 交互；超时自动过牌） ----
  buildCountdown() {
    this.countdownG = this.add.graphics().setDepth(34);
    this.countdownText = this.add.text(66, BAR_Y, '', {
      fontFamily: 'sans-serif', fontSize: '18px', color: '#ffffff', fontStyle: 'bold',
    }).setOrigin(0.5).setDepth(35);
    this.drawCountdown();
  }

  drawCountdown() {
    const g = this.countdownG;
    g.clear();
    const cx = 66;
    const cy = BAR_Y;
    const r = 22;
    if (!this.countdown.active) {
      this.countdownText.setText('');
      return;
    }
    const ratio = Math.max(0, Math.min(1, this.countdown.left / this.countdown.total));
    const color = ratio <= 0.3 ? ASSETS.countdownLow.color : ASSETS.countdownOk.color;
    g.lineStyle(5, 0xffffff, 0.22).strokeCircle(cx, cy, r);
    g.lineStyle(5, color, 1);
    g.beginPath();
    g.arc(cx, cy, r, Phaser.Math.DegToRad(-90), Phaser.Math.DegToRad(-90 + 360 * ratio), false);
    g.strokePath();
    this.countdownText.setText(String(Math.ceil(this.countdown.left)));
  }

  startCountdown() {
    this.stopCountdown();
    this.countdown.active = true;
    this.countdown.left = this.countdown.total;
    this.drawCountdown();
    this.countdown.timer = this.time.addEvent({
      delay: 1000, loop: true,
      callback: () => {
        this.countdown.left -= 1;
        this.drawCountdown();
        if (this.countdown.left <= 0) {
          this.stopCountdown();
          this.statusText.setText('超时 · 自动过牌');
          this.onPass();
        }
      },
    });
  }

  stopCountdown() {
    this.countdown.active = false;
    if (this.countdown.timer) {
      this.countdown.timer.remove();
      this.countdown.timer = null;
    }
    this.countdown.left = 0;
    this.drawCountdown();
  }

  updateCountdown(snap) {
    const shouldRun = snap.myTurn && snap.phase === 'play' && !this.dealing && !this.settleLayer;
    if (shouldRun && !this.countdown.active) this.startCountdown();
    else if (!shouldRun && this.countdown.active) this.stopCountdown();
  }

  // ---- 手牌选择（点击 / 按住拖动连选） ----
  onCardDown(id) {
    if (this.gameModel.waitingSeat !== this.gameModel.humanSeat) return;
    const isSel = this.gameModel.selected.has(id);
    this.dragSelect = { mode: isSel ? 'remove' : 'add', touched: new Set([id]) };
    this.gameModel.toggleCard(id);
  }

  onCardOver(id) {
    const drag = this.dragSelect;
    if (!drag || drag.touched.has(id)) return;
    if (this.gameModel.waitingSeat !== this.gameModel.humanSeat) return;
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

  // ---- 事件分发 ----
  onEvent(ev) {
    if (ev.type === 'deal') {
      this.hideSettle();
      this.stopCountdown();
      this.handleDeal(ev);
    } else if (ev.type === 'play') {
      this.animatePlay(ev.seat, ev.action);
    } else if (ev.type === 'tribute') {
      this.animateTribute(ev.event);
    } else if (ev.type === 'settle') {
      this.stopCountdown();
      this.centerSub.setText(`第 ${ev.summary?.roundNo ?? '?'} 局结束`);
      this.showSettle(ev.summary, ev.ended);
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
    this.statusText.setText('发牌中…');

    // 主题纹理预热（发牌前完成，动画里才有主题牌面）
    const rankCode = (label) => {
      if (!label) return 0;
      if (label === 'A') return 1;
      if (label === 'J') return 11;
      if (label === 'Q') return 12;
      if (label === 'K') return 13;
      return Number(label);
    };
    if (this.skin?.loaded) {
      try { await this.skin.warmup(rankCode(this.gameModel.levelRankLabel())); }
      catch (e) { console.warn('[skin] 预热失败，回退字卡：', e.message); }
    }

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
      return this.flyDealCard(id, t.x, t.y, i, i * CARD_DELAY, CARD_DURATION);
    });
    // 其余三家不飞牌，只把手牌数从 0 递增到真实值（与发牌节奏同步）。
    const others = (ev.hands ?? []).filter((h) => h.seat !== this.gameModel.humanSeat);
    const counters = this.animateCountUp(others, totalMs, hand.length);

    await Promise.all(flights.concat([counters]));
    this.clearFly();

    // 真牌已落位：销毁飞牌，换成正式手牌视图（可点选/拖动/切布局）。
    this.render(this.gameModel.snapshot());
    this.dealing = false;
    this.statusText.setText(`发牌完成 · ${hand.length} 张全部到手`);
    this.gameModel.beginPlay();
  }

  // 单张发牌：从中央牌堆飞向"我"的手牌区最终位置，亮真牌面。
  flyDealCard(id, targetX, targetY, index, delayMs, duration) {
    return new Promise((resolve) => {
      const card = this.makeCardView(id, DECK_X, DECK_Y, { scale: 0.5, faceUp: true });
      this.flyViews.push(card);
      card.setDepth(20 + index);
      card.rotation = index % 2 ? 0.06 : -0.06; // 起手略带倾角，飞行中平滑归零
      this.tweens.add({
        targets: card,
        x: targetX,
        y: targetY,
        scaleX: 1,
        scaleY: 1,
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

    const card = this.makeCardView(event.card, from.cx, from.cy, { scale: 0.9, faceUp: true });
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

  animatePlay(seat, action) {
    if (!action || action.tributeReturn) return;
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
    const cards = action.cards ?? [];
    // 一手牌按紧凑间距铺开，避免长顺子/钢板挤成一坨；牌多时缩小间距。
    const gap = cards.length > 8 ? 30 : 40;
    cards.forEach((id, i) => {
      const fromX = slot === 'bottom' ? SLOTS.bottom.cx : p.cx;
      const fromY = slot === 'bottom' ? SLOTS.bottom.handY : p.cy + 40;
      const c = this.makeCardView(id, fromX, fromY, { scale: 0.92, faceUp: true });
      views.push(c);
      const ox = (i - (cards.length - 1) / 2) * gap;
      c.setDepth(10 + i);
      this.tweens.add({
        targets: c, x: p.cx + ox, y: p.cy, scale: 1, duration: 260,
        ease: 'Back.easeOut', delay: i * 30,
      });
    });
    // applied 事件不带 patternId，炸弹按牌结构识别；四家出炸弹都播特效。
    const bomb = detectBomb(cards);
    if (bomb) this.playBombFx(seat, bomb, views);
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
      this.tweens.add({
        targets: v, y: v.y - 8 - i * 2, scaleX: 1.1, scaleY: 1.1,
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

  clearTrick() {
    this.trickViews.forEach((views) => views.forEach((v) => v.destroy()));
    this.trickViews.clear();
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

  showSettle(summary, ended) {
    this.hideSettle();
    const L = [];
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
  }

  // ---- 渲染（响应 state 变化） ----
  render(snap) {
    if (this.dealing) return; // 发牌阶段由动画自己驱动显示
    this.lastSnapshot = snap;
    const info = snap.info ?? {};
    const m = snap.meta ?? {};
    this.hudText.setText(`第 ${m.roundNo ?? '-'} 局 · 序号 ${m.roomSeq ?? '-'}   |   相位 ${info.phase ?? '-'} · 行动 ${info.turnSeat ?? '-'}`);
    const lv = this.gameModel.levelRankLabel();
    if (this.levelBadgeText) this.levelBadgeText.setText(lv ?? '2');
    this.centerSub.setText(`级牌 ${lv ?? '—'} · 打 ${info.attackingTeam === 0 ? '我方' : info.attackingTeam === 1 ? '对方' : '—'}`);

    const seats = info.seats ?? [];
    seats.forEach((s) => {
      const slot = SLOT_ORDER[this.gameModel.relSlot(s.seat)];
      const label = this.slotLabels[slot];
      const mine = s.seat === snap.humanSeat;
      const rank = s.rank ? ` ${s.rank}名` : '';
      label.setText(`${mine ? '你' : '座位 ' + s.seat}${rank}\n手牌 ${s.hand ?? 0}`);
      label.setColor(mine ? '#ffe9a8' : '#e9fff3');
    });

    const myTurn = snap.myTurn;
    if (!this.countdown.active) {
      this.statusText.setText(myTurn
        ? (snap.phase === 'tribute' ? '轮到你还贡（选一张 ≤10）' : '轮到你出牌 · 点牌选中')
        : (snap.waitingSeat === null ? '' : `等待座位 ${snap.waitingSeat} 行动…`));
    }
    this.playBtn.setAlpha(myTurn ? 1 : 0.5);
    this.passBtn.setAlpha(myTurn && snap.phase !== 'tribute' ? 1 : 0.5);

    this.updateCountdown(snap);
    this.renderHand(snap);
  }

  // 计算整手牌的落点：fan = 横排铺满全屏；cols = 同点数一列纵向罗列（参照 apps/web）。
  // 返回 Map<id, {x, y}>，y 为未选中的基准位置。
  layoutHand(hand, level) {
    const pos = new Map();
    const n = Math.max(1, hand.length);
    if (this.handStyle === 'cols') {
      // 按 columnKey 分组：大王>小王>级牌>A>K…2；同点数一列，列内按 id 稳定排序。
      const cols = new Map();
      for (const id of hand) {
        const key = columnKey(id, level);
        if (!cols.has(key)) cols.set(key, []);
        cols.get(key).push(id);
      }
      const keys = [...cols.keys()].sort((a, b) => b - a);
      // 设计图：列间距接近牌宽（铺满全屏、列间几乎不重叠），同列纵向错开露角标。
      const colGap = Math.min(92, CARD_W + 4);
      const rowStep = 34;          // 同列每张上移：牌高 120，露顶部角标可辨
      const totalW = (keys.length - 1) * colGap;
      keys.forEach((key, ci) => {
        const ids = cols.get(key).slice().sort((a, b) => a - b);
        const x = 640 - totalW / 2 + ci * colGap;
        ids.forEach((id, ri) => {
          pos.set(id, { x, y: 648 - ri * rowStep });
        });
      });
    } else {
      // fan：横排铺满全屏。间距按可用宽度反推，牌宽 66、适度重叠露角标。
      const usable = W - 72;                       // 左右各留 36px
      const maxGap = Math.max(20, Math.floor((usable - CARD_W) / (n - 1)));
      const spacing = Math.min(58, maxGap);        // 上限 58：牌多时仍保留叠放感
      const startX = 640 - ((n - 1) * spacing) / 2;
      hand.forEach((id, i) => pos.set(id, { x: startX + i * spacing, y: 658 }));
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
    hand.forEach((id, index) => {
      const target = layout.get(id);
      let view = this.cardViews.get(id);
      if (!view) {
        view = this.makeCardView(id, target.x, target.y, { scale: 1, faceUp: true, level, interactive: true });
        view.on('pointerdown', () => this.onCardDown(id));
        view.on('pointerover', () => this.onCardOver(id));
        this.cardViews.set(id, view);
      }
      view.setData?.('order', index);
      const isSel = selected.has(id);
      // 选中上移：cols 列内已向上叠放，上移幅度收小避免顶到操作栏；fan 上移更多以露整张牌。
      const lift = this.handStyle === 'cols' ? 12 : 34;
      const ty = isSel ? target.y - lift : target.y;
      if (Math.abs(view.x - target.x) > 1 || Math.abs(view.y - ty) > 1) {
        this.tweens.add({ targets: view, x: target.x, y: ty, duration: 110, ease: 'Quad.easeOut' });
      }
      // 关键：选中的牌 depth 必须高于相邻牌，否则 fan 横叠时会被右边的牌盖住，看不清。
      // 普通牌 12（fan）/ 20+y/8（cols，下盖上的牌）；选中牌统一抬到 100+order，
      // 仍低于结算层 200，且选中牌之间按原始次序排列（不抖动）。
      const baseDepth = this.handStyle === 'cols' ? 20 + Math.round(target.y / 8) : 12;
      view.setDepth(isSel ? 100 + index : baseDepth);
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
    });
    // cols 模式不需要浮层（牌纵向叠放、角标本就露出），销毁避免残留
    if (this.handStyle !== 'fan') {
      for (const [id, corner] of this.cornerViews) {
        corner.destroy();
        this.cornerViews.delete(id);
      }
    }
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

  // 统一的牌视图：主题模式用 SVG 纹理 Image，否则字卡 Container。
  // 返回的对象都支持 x/y/scale/rotation/alpha/destroy/setDepth/on/tweens。
  makeCardView(id, x, y, { scale = 1, faceUp = true, level = null, interactive = false } = {}) {
    const texKey = faceUp && this.skin ? this.skin.textureKey(id, level) : null;
    if (texKey && this.textures.exists(texKey)) {
      // SVG 纹理基准缩放让它落到 CARD_W 宽，再乘外部 scale。
      const tex = this.textures.get(texKey).getSourceImage();
      const base = tex && tex.width ? CARD_W / tex.width : CARD_W / 500;
      const img = this.add.image(x, y, texKey).setScale(base * scale);
      if (interactive) img.setInteractive({ useHandCursor: true });
      return img;
    }
    return this.makeTextCard(id, x, y, { scale, faceUp, interactive });
  }

  // 牌背：蓝色斜纹 + 掼字（占位素材，后期可替换为贴图）。
  makeBack(x, y, scale = 1) {
    const c = this.add.container(x, y).setScale(scale);
    const back = ASSETS.cardBack;
    const box = this.add.rectangle(0, 0, CARD_W, CARD_H, back.color, 1)
      .setStrokeStyle(1, 0x8fc4ee, 1);
    const inner = this.add.rectangle(0, 0, CARD_W - 10, CARD_H - 10, back.inner, 1)
      .setStrokeStyle(1, 0x4a86bd, 0.9);
    const txt = this.add.text(0, 0, back.glyph, {
      fontFamily: 'serif', fontSize: '30px', color: '#9fc8ea',
    }).setOrigin(0.5);
    c.add([box, inner, txt]);
    return c;
  }

  // 字卡（无主题或牌背时的回退）：圆角矩形 + 点数花色。
  makeTextCard(id, x, y, { scale = 1, faceUp = true, interactive = false } = {}) {
    const f = cardFace(id);
    const c = this.add.container(x, y).setScale(scale);
    const box = faceUp
      ? this.add.rectangle(0, 0, CARD_W, CARD_H, 0xfdfdf7, 1).setStrokeStyle(1, 0x9fd8bd, 1)
      : this.add.rectangle(0, 0, CARD_W, CARD_H, 0x174a7a, 1).setStrokeStyle(1, 0x8fc4ee, 1);
    box.setData('selborder', true);
    if (interactive) box.setInteractive({ useHandCursor: true });
    if (faceUp) {
      const color = f.red ? '#c0392b' : '#1c2733';
      c.add([
        this.add.text(-25, -34, f.rank, { fontFamily: 'sans-serif', fontSize: '28px', color, fontStyle: 'bold' }),
        this.add.text(-25, -6, f.suit, { fontFamily: 'sans-serif', fontSize: '22px', color }),
        this.add.text(8, 12, f.joker ? '★' : f.suit, {
          fontFamily: 'sans-serif', fontSize: '36px', color,
        }).setOrigin(0.5).setAlpha(0.35),
      ]);
    } else {
      c.add([this.add.text(0, 0, '掼', {
        fontFamily: 'serif', fontSize: '30px', color: '#9fc8ea',
      }).setOrigin(0.5)]);
    }
    c.addAt(box, 0);
    return c;
  }
}

export { H, W };
