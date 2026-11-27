// 牌桌场景：Phaser 3 渲染 + Tween 动画。订阅 Game 的 change/event，纯表现层。
// 逻辑一句都不在这里判定，全部读 Game.snapshot()，点牌回调 game.toggleCard。
// Phaser 由 index.html 以 UMD <script> 引入，走全局 window.Phaser。
import { cardFace } from '../cards.js';

const Phaser = window.Phaser;

const W = 1280;
const H = 720;
const FELT = 0x146b4a;
const FELT_DARK = 0x0e5238;

// 四个方位的几何配置（相对人类座位）。center 为该座位出牌区锚点。
const SLOTS = {
  bottom: { name: 'bottom', cx: 640, cy: 470, handY: 640, labelY: 560 },
  right: { name: 'right', cx: 950, cy: 340, handY: 250, labelY: 150 },
  top: { name: 'top', cx: 640, cy: 210, handY: 80, labelY: 40 },
  left: { name: 'left', cx: 330, cy: 340, handY: 250, labelY: 150 },
};
const SLOT_ORDER = ['bottom', 'right', 'top', 'left'];

export class TableScene extends Phaser.Scene {
  constructor(game) {
    super('table');
    this.gameModel = game;
    this.cardViews = new Map(); // id -> Container（人类手牌）
    this.trickViews = []; // 出牌区临时对象
    this.lastSnapshot = null;
  }

  create() {
    this.buildTable();
    this.buildHud();
    this.buildButtons();

    this.gameModel.on('change', (snap) => this.render(snap));
    this.gameModel.on('event', (ev) => this.onEvent(ev));

    // 首帧：若已在开局前，画一个空桌提示。
    this.statusText.setText('点「新局」开始');
    this.render = this.render.bind(this);
  }

  // ---- 静态桌面 ----
  buildTable() {
    const g = this.add.graphics();
    g.fillStyle(FELT_DARK, 1).fillRect(0, 0, W, H);
    g.fillStyle(FELT, 1).fillRoundedRect(40, 40, W - 80, H - 80, 28);
    g.lineStyle(2, 0x2a8f6a, 0.8).strokeRoundedRect(40, 40, W - 80, H - 80, 28);

    // 中央信息牌
    this.add.rectangle(640, 340, 300, 120, 0x0b3f2a, 0.75).setStrokeStyle(1, 0x3aa37a, 0.6);
    this.centerTitle = this.add.text(640, 312, '掼蛋 · Phaser', {
      fontFamily: 'sans-serif', fontSize: '26px', color: '#dff5e8', fontStyle: 'bold',
    }).setOrigin(0.5);
    this.centerSub = this.add.text(640, 352, '', {
      fontFamily: 'sans-serif', fontSize: '15px', color: '#a9d9c2',
    }).setOrigin(0.5);

    this.slotLabels = {};
    SLOT_ORDER.forEach((slot) => {
      const p = SLOTS[slot];
      this.slotLabels[slot] = this.add.text(p.cx, p.labelY + 30, '', {
        fontFamily: 'sans-serif', fontSize: '15px', color: '#e9fff3', align: 'center',
      }).setOrigin(0.5);
    });

    this.statusText = this.add.text(640, 690, '', {
      fontFamily: 'sans-serif', fontSize: '16px', color: '#ffe9a8',
    }).setOrigin(0.5);
    this.hudText = this.add.text(56, 56, '', {
      fontFamily: 'monospace', fontSize: '14px', color: '#cdeedd',
    });
  }

  buildHud() {
    this.hudText = this.add.text(56, 56, '', {
      fontFamily: 'monospace', fontSize: '14px', color: '#cdeedd', lineSpacing: 4,
    });
  }

  makeButton(x, y, label, onClick, color = 0x1d6f57) {
    const w = 92;
    const h = 40;
    const box = this.add.rectangle(x, y, w, h, color, 1)
      .setStrokeStyle(1, 0x7fd3ae, 0.9).setInteractive({ useHandCursor: true });
    const txt = this.add.text(x, y, label, {
      fontFamily: 'sans-serif', fontSize: '16px', color: '#ffffff',
    }).setOrigin(0.5);
    box.on('pointerover', () => box.setFillStyle(0x2b8a6c, 1));
    box.on('pointerout', () => box.setFillStyle(color, 1));
    box.on('pointerdown', onClick);
    return { box, txt };
  }

  buildButtons() {
    const y = 686;
    this.makeButton(900, y, '新局', () => this.onNewGame());
    this.playBtn = this.makeButton(1000, y, '出牌', () => this.onPlay());
    this.passBtn = this.makeButton(1100, y, '过牌', () => this.onPass());
    this.hintBtn = this.makeButton(1200, y, '提示', () => this.gameModel.hint());
  }

  // ---- 交互回调 ----
  onNewGame() {
    this.clearTrick();
    const hi = (Math.random() * 0xffffffff) >>> 0;
    const lo = (Math.random() * 0xffffffff) >>> 0;
    this.gameModel.start({ seedHi: hi, seedLo: lo, humanSeat: 0, level: 2 });
  }

  onPlay() {
    const r = this.gameModel.play();
    if (!r.ok) this.statusText.setText(r.reason);
  }

  onPass() {
    const r = this.gameModel.pass();
    if (!r.ok) this.statusText.setText(r.reason);
  }

  // ---- 事件：动画 ----
  onEvent(ev) {
    if (ev.type === 'deal') {
      this.clearTrick();
      this.animateDeal(ev.snapshot);
    } else if (ev.type === 'play') {
      this.animatePlay(ev.seat, ev.action);
    } else if (ev.type === 'settle') {
      this.centerSub.setText(`第 ${ev.summary?.roundNo ?? '?'} 局结束 · 名次 ${(ev.summary?.ranks ?? []).join(' ')}`);
      this.statusText.setText(ev.ended ? '全场结束，点「新局」再来' : '点「新局」进入下一局');
    } else if (ev.type === 'error') {
      this.statusText.setText(`房间错误：${ev.message}`);
    }
  }

  animateDeal(snap) {
    const hand = snap.hand ?? [];
    hand.forEach((id, i) => {
      const view = this.makeCard(id, 640, 300, 0.35);
      this.cardViews.set(id, view);
      const target = this.handPos(id, i, hand.length);
      view.rotation = -0.2 + Math.random() * 0.4;
      this.tweens.add({
        targets: view,
        x: target.x, y: target.y, rotation: 0, scale: 1,
        delay: i * 12, duration: 320, ease: 'Cubic.easeOut',
      });
    });
  }

  animatePlay(seat, action) {
    if (!action || action.tributeReturn) return;
    const slot = SLOT_ORDER[this.gameModel.relSlot(seat)];
    const p = SLOTS[slot];
    if (action.pass) {
      const t = this.add.text(p.cx, p.cy, '过', {
        fontFamily: 'sans-serif', fontSize: '30px', color: '#cfe8da', fontStyle: 'bold',
      }).setOrigin(0.5).setAlpha(0);
      this.trickViews.push(t);
      this.tweens.add({ targets: t, alpha: 1, y: p.cy - 16, duration: 180, yoyo: true, hold: 400 });
      return;
    }
    const cards = action.cards ?? [];
    cards.forEach((id, i) => {
      const fromX = slot === 'bottom' ? 640 : p.cx;
      const fromY = slot === 'bottom' ? 640 : p.cy + 40;
      const c = this.makeCard(id, fromX, fromY, 0.9);
      const ox = (i - (cards.length - 1) / 2) * 30;
      this.trickViews.push(c);
      this.tweens.add({
        targets: c, x: p.cx + ox, y: p.cy, scale: 1, duration: 260,
        ease: 'Back.easeOut', delay: i * 30,
      });
    });
  }

  clearTrick() {
    this.trickViews.forEach((v) => v.destroy());
    this.trickViews = [];
  }

  // ---- 渲染（响应 state 变化） ----
  render(snap) {
    this.lastSnapshot = snap;
    const info = snap.info ?? {};
    const m = snap.meta ?? {};
    this.hudText.setText(
      `局 ${m.roundNo ?? '-'} · 序号 ${m.roomSeq ?? '-'}\n` +
      `相位 ${info.phase ?? '-'} · 行动 ${info.turnSeat ?? '-'}\n` +
      `打级队 ${info.attackingTeam ?? '-'}`,
    );
    const lv = this.gameModel.levelRankLabel();
    this.centerSub.setText(`级牌 ${lv ?? '—'} · 打 ${info.attackingTeam === 0 ? '我方' : info.attackingTeam === 1 ? '对方' : '—'}`);

    // 座位牌数 / 名次 / 身份
    const seats = info.seats ?? [];
    seats.forEach((s) => {
      const slot = SLOT_ORDER[this.gameModel.relSlot(s.seat)];
      const label = this.slotLabels[slot];
      const mine = s.seat === snap.humanSeat;
      const rank = s.rank ? ` · ${s.rank}名` : '';
      label.setText(`${mine ? '你' : '座位 ' + s.seat}${rank}\n手牌 ${s.hand ?? 0}`);
      label.setColor(mine ? '#ffe9a8' : '#e9fff3');
    });

    const myTurn = snap.myTurn;
    this.statusText.setText(myTurn
      ? (snap.phase === 'tribute' ? '轮到你还贡（选一张 ≤10）' : '轮到你出牌 · 点牌选中')
      : (snap.waitingSeat === null ? '' : `等待座位 ${snap.waitingSeat} 行动…`));
    this.playBtn.box.setAlpha(myTurn ? 1 : 0.5);
    this.passBtn.box.setAlpha(myTurn && snap.phase !== 'tribute' ? 1 : 0.5);

    this.renderHand(snap);
  }

  handPos(id, index, total) {
    const spacing = total > 18 ? 40 : 52;
    const startX = 640 - ((total - 1) * spacing) / 2;
    return { x: startX + index * spacing, y: 640 };
  }

  renderHand(snap) {
    const hand = snap.hand ?? [];
    const selected = snap.selected ?? new Set();
    const present = new Set(hand);

    // 移除已不在手里的牌视图
    for (const [id, view] of this.cardViews) {
      if (!present.has(id)) {
        view.destroy();
        this.cardViews.delete(id);
      }
    }
    // 新增或更新位置 / 选中抬高
    hand.forEach((id, i) => {
      const target = this.handPos(id, i, hand.length);
      let view = this.cardViews.get(id);
      if (!view) {
        view = this.makeCard(id, target.x, target.y, 1);
        view.on('pointerdown', () => this.gameModel.toggleCard(id));
        this.cardViews.set(id, view);
      }
      const ty = selected.has(id) ? target.y - 26 : target.y;
      if (Math.abs(view.x - target.x) > 1 || Math.abs(view.y - ty) > 1) {
        this.tweens.add({ targets: view, x: target.x, y: ty, duration: 110, ease: 'Quad.easeOut' });
      }
      const border = view.list?.find?.((c) => c.getData?.('selborder'));
      if (border) border.setStrokeStyle(selected.has(id) ? 3 : 1, selected.has(id) ? 0xffd45e : 0x9fd8bd, 1);
    });
  }

  // 一张牌 = 圆角矩形 + 点数花色文本。红桃/方块/大王为红。
  makeCard(id, x, y, scale) {
    const f = cardFace(id);
    const c = this.add.container(x, y).setScale(scale);
    const border = this.add.rectangle(0, 0, 46, 66, 0xfdfdf7, 1)
      .setStrokeStyle(1, 0x9fd8bd, 1).setInteractive({ useHandCursor: true });
    border.setData('selborder', true);
    const color = f.red ? '#c0392b' : '#1c2733';
    const rankText = this.add.text(-17, -22, f.rank, {
      fontFamily: 'sans-serif', fontSize: '20px', color, fontStyle: 'bold',
    });
    const suitText = this.add.text(-17, -4, f.suit, {
      fontFamily: 'sans-serif', fontSize: '16px', color,
    });
    const bigText = this.add.text(6, 8, f.joker ? '★' : f.suit, {
      fontFamily: 'sans-serif', fontSize: '26px', color,
    }).setOrigin(0.5).setAlpha(0.35);
    c.add([border, rankText, suitText, bigText]);
    return c;
  }
}

export { H, W };
