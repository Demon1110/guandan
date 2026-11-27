// 游戏主控：把 wasm 的 step/waiting/applied/settled 语义翻译成一次完整单机对局。
// 人类座位由 humanSeat 指定，其余三座由 wasm 内置的 Room bot（L2）自动决策。
// 本类只管状态与判定，不碰 Phaser；渲染由 GameScene 订阅 onChange/onEvent 完成。
import { sortHand } from './cards.js';

export class Game {
  constructor(wasm) {
    this.wasm = wasm;
    this.seedHi = 0x51ed;
    this.seedLo = 0xa106;
    this.humanSeat = 0;
    this.level = 2;
    this.running = false;
    this.phase = 'idle'; // idle | tribute | play | settle | ended
    this.waitingSeat = null;
    this.selected = new Set();
    this.lastBySeat = {}; // seat -> { pass, cards, patternId }
    this.hintIdx = 0;
    this.listeners = { change: [], event: [] };
    this.timer = null;
    this.speed = 320; // bot 每手间隔毫秒
    this.roundNo = 0; // 进贡事件去重用
    this.tributeSeen = new Set();
    this.tributeFlights = 0; // 正在播放的进贡飞牌数，>0 时主循环等待
    this.tributeWaiters = []; // 等飞牌完成的 resolver 队列
  }

  on(type, fn) {
    this.listeners[type].push(fn);
  }

  emitChange() {
    this.listeners.change.forEach((fn) => fn(this.snapshot()));
  }

  emitEvent(ev) {
    this.listeners.event.forEach((fn) => fn(ev));
  }

  relSlot(seat) {
    return (seat - this.humanSeat + 4) % 4;
  }

  seatLabel(seat) {
    if (seat === this.humanSeat) return '你';
    const role = this.relSlot(seat) === 2 ? '对家' : '对手';
    return `${role} ${seat}`;
  }

  // 当前人类手牌（按引擎合成序排好）。
  myHand() {
    const lv = this.levelRankLabel();
    const cards = this.wasm.view(this.humanSeat).hand?.cards ?? [];
    return sortHand(cards, lv);
  }

  levelRankLabel() {
    const lv = this.wasm.view(this.humanSeat)['round.level'];
    return typeof lv === 'number' && lv >= 0 && lv <= 12 ? ['2','3','4','5','6','7','8','9','10','J','Q','K','A'][lv] : null;
  }

  snapshot() {
    const info = this.wasm.info();
    return {
      phase: this.phase,
      info,
      humanSeat: this.humanSeat,
      waitingSeat: this.waitingSeat,
      myTurn: this.waitingSeat === this.humanSeat,
      hand: this.phase === 'idle' ? [] : this.myHand(),
      selected: this.selected,
      lastBySeat: this.lastBySeat,
      meta: this.wasm.meta(),
    };
  }

  // 第一步：只建桌 + 进入独立的「发牌」阶段。真正的对局要等 beginPlay() 才开始，
  // 让 TableScene 有机会把发牌动画完整播完（见 deal 事件）。
  start({ seedHi, seedLo, humanSeat = 0, level = 2 } = {}) {
    this.stop();
    this.seedHi = seedHi ?? 0x51ed;
    this.seedLo = seedLo ?? 0xa106;
    this.humanSeat = humanSeat;
    this.level = level;
    const mask = 1 << humanSeat;
    this.wasm.create(this.seedHi, this.seedLo, mask, level);
    this.phase = 'dealing';
    this.waitingSeat = null;
    this.selected.clear();
    this.lastBySeat = {};
    this.hintIdx = 0;
    this.roundNo = 0;
    this.tributeSeen.clear();
    // 发牌事件的载荷：四家各 27 张（不泄露牌面，bot 只显示牌背）。
    const info = this.wasm.info();
    this.emitEvent({
      type: 'deal',
      hands: (info.seats ?? []).map((s) => ({ seat: s.seat, count: s.hand ?? 0 })),
      snapshot: this.snapshot(),
    });
    this.emitChange();
  }

  // 第二步：发牌动画结束后由场景调用，进入主循环。
  beginPlay() {
    if (this.phase !== 'dealing') return;
    this.phase = 'play';
    this.emitChange();
    this.pump();
  }

  stop() {
    this.running = false;
    if (this.timer) clearTimeout(this.timer);
    this.timer = null;
  }

  // 主循环：连续 step，遇到 waiting 让出控制权（人类回合挂起，bot 回合稍后继续）。
  async pump() {
    if (this.running) return;
    this.running = true;
    try {
      const info = this.wasm.info();
      if (info.phase) this.phase = info.phase;

      for (let i = 0; i < 500; i += 1) {
        // 进贡事件先采集：引擎可能在一次 step 里同时产生贡牌与动作，动画要先于出牌飞。
        await this.syncTribute();
        const s = this.wasm.step();
        if (s.phase) this.phase = s.phase;
        await this.syncTribute();

        if (s.kind === 'applied') {
          this.remember(s);
          this.emitEvent({ type: 'play', seat: s.seat, action: s.action, auto: s.auto, snapshot: this.snapshot() });
          this.emitChange();
          if (s.seat !== this.humanSeat) await this.wait(this.speed);
          continue;
        }
        if (s.kind === 'waiting') {
          this.waitingSeat = s.seat;
          this.phase = s.phase ?? 'play';
          this.hintIdx = 0;
          this.emitChange();
          if (s.seat !== this.humanSeat) {
            await this.wait(Math.max(140, this.speed));
            // bot 回合：wasm 的 step 内部已按 bot 策略出牌，下一轮 step 会拿到 applied。
            this.running = false;
            this.pump();
            return;
          }
          // 人类回合挂起，等待 submit/pass 后由调用方再次 pump。
          this.running = false;
          return;
        }
        if (s.kind === 'settled' || s.kind === 'matchEnd') {
          const ended = s.kind === 'matchEnd' || !!s.summary?.matchEnd;
          this.phase = ended ? 'ended' : 'settle';
          this.waitingSeat = null;
          this.emitEvent({ type: 'settle', summary: s.summary, ended, snapshot: this.snapshot() });
          this.emitChange();
          return;
        }
        if (s.kind === 'error') {
          this.emitEvent({ type: 'error', message: s.error });
          this.running = false;
          return;
        }
      }
    } finally {
      this.running = false;
    }
  }

  wait(ms) {
    return new Promise((r) => setTimeout(r, ms));
  }

  // 采集进贡/还贡事件（info.tribute.events），增量派发给场景做飞牌动画。
  // 有飞牌在播时，返回的 Promise 会挂起主循环，避免出牌抢在飞牌前面。
  syncTribute() {
    let info;
    try {
      info = this.wasm.info();
    } catch {
      return Promise.resolve();
    }
    const t = info?.tribute;
    if (!t) return this.waitFlights();
    const round = info.roundNo ?? 0;
    if (round !== this.roundNo) {
      this.roundNo = round;
      this.tributeSeen.clear();
    }
    (t.events ?? []).forEach((ev, i) => {
      const key = `${round}:${i}:${ev.from}:${ev.to}:${ev.card}:${ev.return ? 1 : 0}`;
      if (this.tributeSeen.has(key)) return;
      this.tributeSeen.add(key);
      this.tributeFlights += 1; // 场景在 tribute 事件里递减
      this.emitEvent({ type: 'tribute', event: { ...ev, round } });
    });
    // 场景可能没有订阅（如 Node 冒烟），给一个兜底超时避免永久挂起。
    // 浏览器里飞牌动画约 800ms 就完成，3s 后 flights 已归零，兜底不会重复触发。
    if (this.tributeFlights > 0 && this.tributeWaiters.length === 0) {
      const self = this;
      setTimeout(() => { if (self.tributeFlights > 0) self.tributeDone(); }, 3000);
    }
    return this.waitFlights();
  }

  waitFlights() {
    if (this.tributeFlights <= 0) return Promise.resolve();
    return new Promise((resolve) => this.tributeWaiters.push(resolve));
  }

  // 场景每播完一张飞牌调用一次。
  tributeDone() {
    this.tributeFlights = Math.max(0, this.tributeFlights - 1);
    if (this.tributeFlights === 0) {
      const waiters = this.tributeWaiters.splice(0);
      waiters.forEach((resolve) => resolve());
    }
  }

  remember(s) {
    const a = s.action ?? {};
    this.lastBySeat[s.seat] = {
      pass: !!a.pass,
      tributeReturn: !!a.tributeReturn,
      cards: a.cards ?? [],
      patternId: a.patternId ?? '',
    };
    // lastPlay 被清空 ⇒ 新墩，撤掉四家的牌。
    try {
      if (!this.wasm.view(this.humanSeat).lastPlay) this.lastBySeat = {};
    } catch { /* 视图暂不可用忽略 */ }
  }

  toggleCard(id) {
    if (this.waitingSeat !== this.humanSeat) return;
    if (this.selected.has(id)) this.selected.delete(id);
    else this.selected.add(id);
    this.emitChange();
  }

  clearSelection() {
    this.selected.clear();
    this.emitChange();
  }

  // 出牌：先 canPlay 预检，通过才 submit，避免误点被引擎拒绝。
  play() {
    if (this.waitingSeat !== this.humanSeat) return { ok: false, reason: '还没轮到你' };
    const cards = [...this.selected];
    if (!cards.length) return { ok: false, reason: '没有选中牌' };
    const check = this.wasm.canPlay?.(cards);
    if (check && !check.ok) return { ok: false, reason: check.reason || '这组牌压不过' };
    const isTribute = this.phase === 'tribute';
    const out = this.wasm.submit(this.humanSeat, this.wasm.now(), {
      cards,
      pass: false,
      tributeReturn: isTribute,
    });
    if (out.kind === 'applied' || out.kind === 'ok') {
      this.selected.clear();
      this.waitingSeat = null;
      // submit 已经消费了这一手，后续 step() 只会返回别人的动作。
      // 这里必须自己派发 play 事件，否则自己的出牌动画（含炸弹特效）永远不会播。
      this.remember(out);
      this.emitEvent({
        type: 'play',
        seat: out.seat ?? this.humanSeat,
        action: out.action ?? { pass: false, cards },
        auto: out.auto,
        snapshot: this.snapshot(),
      });
      this.emitChange();
      this.pump();
      return { ok: true, out };
    }
    return { ok: false, reason: out.error || out.reason || '出牌被拒' };
  }

  pass() {
    if (this.waitingSeat !== this.humanSeat) return { ok: false, reason: '还没轮到你' };
    if (this.phase === 'tribute') return { ok: false, reason: '进贡阶段不能过牌' };
    const out = this.wasm.submit(this.humanSeat, this.wasm.now(), { pass: true, cards: [] });
    if (out.kind === 'applied' || out.kind === 'ok') {
      this.selected.clear();
      this.waitingSeat = null;
      // 与 play() 同理：submit 消费过的过牌也要自己派发，场景才能显示"过"。
      this.remember(out);
      this.emitEvent({
        type: 'play',
        seat: out.seat ?? this.humanSeat,
        action: out.action ?? { pass: true, cards: [] },
        auto: out.auto,
        snapshot: this.snapshot(),
      });
      this.emitChange();
      this.pump();
      return { ok: true };
    }
    return { ok: false, reason: out.error || out.reason || '过牌被拒' };
  }

  // 提示：优先从合法动作集取，其次用组牌推荐 plan 的最小组。
  hint() {
    if (this.waitingSeat !== this.humanSeat) return;
    if (this.phase === 'tribute') {
      const info = this.wasm.info();
      const pending = info.tribute?.pending?.find((p) => p.receiver === this.humanSeat);
      const eligible = pending?.eligible ?? [];
      this.selected = new Set(eligible.slice(0, 1));
      this.emitChange();
      return;
    }
    const legal = this.wasm.legal().filter((a) => !a.pass && (a.cards?.length ?? 0) > 0);
    if (legal.length) {
      const a = legal[this.hintIdx % legal.length];
      this.hintIdx += 1;
      this.selected = new Set(a.cards);
      this.emitChange();
      return;
    }
    const plan = this.wasm.plan();
    const g = plan?.groups?.[0];
    if (g?.cards?.length) {
      this.selected = new Set(g.cards);
      this.emitChange();
    }
  }

  // 下一局同样走独立发牌阶段：引擎跨局发牌后，交给场景播动画，再 beginPlay。
  nextRound() {
    if (this.phase !== 'settle') return;
    if (this.wasm.nextRound(0)) {
      this.phase = 'dealing';
      this.waitingSeat = null;
      this.lastBySeat = {};
      this.selected.clear();
      const info = this.wasm.info();
      this.emitEvent({
        type: 'deal',
        hands: (info.seats ?? []).map((s) => ({ seat: s.seat, count: s.hand ?? 0 })),
        snapshot: this.snapshot(),
      });
      this.emitChange();
    }
  }

  // 结算界面点「再来一局」：场次未结束走 nextRound，结束则重开一桌。
  restart() {
    this.stop();
    this.start({ seedHi: this.seedHi, seedLo: this.seedLo, humanSeat: this.humanSeat, level: this.level });
  }
}
