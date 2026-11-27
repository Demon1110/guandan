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
    this.level = 2; // 级牌（本局打几），与 bot 难度无关
    this.botLevel = 2; // 0=L0简单 / 1=L1普通 / 2=L2困难
    this.actions = []; // 已发生动作序列，用于存档恢复（replay）
    this.running = false;
    this.phase = 'idle'; // idle | tribute | play | settle | ended
    this.waitingSeat = null;
    this.selected = new Set();
    this.lastBySeat = {}; // seat -> { pass, cards, patternId }
    this.hintIdx = 0;
    this.listeners = { change: [], event: [] };
    this.timer = null;
    this.settleTimer = null; // 延迟结算定时器（等最后一手特效播完）
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
  start({ seedHi, seedLo, humanSeat = 0, level = 2, botLevel = 2 } = {}) {
    this.stop();
    this.seedHi = seedHi ?? 0x51ed;
    this.seedLo = seedLo ?? 0xa106;
    this.humanSeat = humanSeat;
    this.level = level;
    this.botLevel = botLevel;
    const mask = 1 << humanSeat;
    this.wasm.create(this.seedHi, this.seedLo, mask, botLevel);
    this.phase = 'dealing';
    this.waitingSeat = null;
    this.selected.clear();
    this.lastBySeat = {};
    this.actions = [];
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
    if (this.settleTimer) clearTimeout(this.settleTimer);
    this.settleTimer = null;
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
          // 人类回合：若全部手牌能作为一手直接出完，自动打出，无需手动选牌。
          const auto = this.tryAutoFinish();
          if (auto === 'settled') return; // 这一手已终局，结算事件已派发
          if (auto === 'played') continue; // 已出完这一手，继续主循环推进其他家
          // 否则挂起，等待 submit/pass 后由调用方再次 pump。
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
    // 累积动作序列（含 seat/pass/cards/tributeReturn）用于存档恢复。
    // roomSeq 递增即一条新动作，去重避免 submit 自己派发与 step 重复记录。
    if (s.roomSeq != null) {
      if (this.actions.length < s.roomSeq) {
        this.actions.push({
          seat: s.seat,
          pass: !!a.pass,
          tributeReturn: !!a.tributeReturn,
          cards: a.cards ?? [],
        });
      }
    } else {
      this.actions.push({
        seat: s.seat,
        pass: !!a.pass,
        tributeReturn: !!a.tributeReturn,
        cards: a.cards ?? [],
      });
    }
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

  // 自动出完：轮到我方且手牌能作为一手合法牌全部打出时，直接出完（如剩一手炸弹/顺子/单牌）。
  // canPlay 与 submit 判定口径一致（含压牌），故用它判定；legal() 只是候选清单不保证含整手组合。
  // 返回 'settled'（这一手已终局）/ 'played'（已出牌，主循环继续）/ null（不能一把出完，交给玩家）。
  tryAutoFinish() {
    if (this.phase !== 'play' || this.waitingSeat !== this.humanSeat) return null;
    const hand = this.myHand();
    if (!hand.length) return null;
    const check = this.wasm.canPlay?.(hand);
    if (!check || !check.ok) return null;

    const out = this.wasm.submit(this.humanSeat, this.wasm.now(), {
      cards: hand, pass: false, tributeReturn: false,
    });
    // 出完最后一张直接结算：submit 返回 settled/matchEnd。
    if (out.kind === 'settled' || out.kind === 'matchEnd') {
      this.settleFromSubmit(out, { seat: this.humanSeat, pass: false, cards: hand, roomSeq: out.roomSeq }, check?.patternId);
      return 'settled';
    }
    if (out.kind === 'applied' || out.kind === 'ok') {
      this.selected.clear();
      this.waitingSeat = null;
      this.remember(out);
      this.emitEvent({
        type: 'play',
        seat: out.seat ?? this.humanSeat,
        action: out.action ?? { pass: false, cards: hand, patternId: check?.patternId },
        auto: true,
        snapshot: this.snapshot(),
      });
      this.emitChange();
      return 'played';
    }
    return null; // submit 意外被拒：交还玩家手动处理
  }

  // 还贡：tribute 阶段 canPlay 会报"当前不在 play 相位"，必须走专用提交。
  // 校验：轮到我 + phase=tribute + 恰好一张 + 在引擎 eligible（≤10 的合规牌）内。
  tributeReturn(cardId) {
    if (this.waitingSeat !== this.humanSeat) return { ok: false, reason: '还没轮到你还贡' };
    if (this.phase !== 'tribute') return { ok: false, reason: '当前不是还贡阶段' };
    const id = cardId ?? [...this.selected][0];
    if (id == null) return { ok: false, reason: '请先选一张 ≤10 的牌' };
    const pending = this.wasm.info()?.tribute?.pending?.find((p) => p.receiver === this.humanSeat);
    const eligible = pending?.eligible ?? [];
    if (!eligible.includes(id)) return { ok: false, reason: '只能还 ≤10 的合规牌' };
    const out = this.wasm.submit(this.humanSeat, this.wasm.now(), {
      cards: [id], pass: false, tributeReturn: true,
    });
    if (out.kind === 'settled' || out.kind === 'matchEnd') {
      this.settleFromSubmit(out, { seat: this.humanSeat, pass: false, cards: [id], tributeReturn: true, roomSeq: out.roomSeq });
      return { ok: true, out };
    }
    if (out.kind === 'applied' || out.kind === 'ok') {
      this.selected.clear();
      this.waitingSeat = null;
      this.remember(out);
      this.emitEvent({
        type: 'play', seat: out.seat ?? this.humanSeat,
        action: out.action ?? { pass: false, tributeReturn: true, cards: [id] },
        auto: out.auto, snapshot: this.snapshot(),
      });
      this.emitChange();
      this.pump();
      return { ok: true, out };
    }
    return { ok: false, reason: out.error || out.reason || '还贡被拒' };
  }

  // 当前我方还贡可选的牌（≤10 合规牌），供场景限制选牌。
  tributeEligible() {
    if (this.phase !== 'tribute') return [];
    const pending = this.wasm.info()?.tribute?.pending?.find((p) => p.receiver === this.humanSeat);
    return pending?.eligible ?? [];
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
    // 出最后一张牌会直接触发结算：submit 返回 settled/matchEnd（而非 applied）。
    // 此时牌已被引擎消耗、名次已定，必须按结算处理，否则误判"出牌被拒"卡死。
    if (out.kind === 'settled' || out.kind === 'matchEnd') {
      this.settleFromSubmit(out, { seat: this.humanSeat, pass: false, cards, roomSeq: out.roomSeq }, check?.patternId);
      return { ok: true, out };
    }
    if (out.kind === 'applied' || out.kind === 'ok') {
      this.selected.clear();
      this.waitingSeat = null;
      // submit 已经消费了这一手，后续 step() 只会返回别人的动作。
      // 这里必须自己派发 play 事件，否则自己的出牌动画（含炸弹特效）永远不会播。
      // action 带引擎 patternId（含红配炸弹），场景据此识别炸弹，不靠牌结构猜测。
      this.remember(out);
      this.emitEvent({
        type: 'play',
        seat: out.seat ?? this.humanSeat,
        action: out.action ?? { pass: false, cards, patternId: check?.patternId },
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
    // 过牌后其余三家也出完会直接结算（最后一手是别人的牌），同样按结算处理。
    if (out.kind === 'settled' || out.kind === 'matchEnd') {
      this.settleFromSubmit(out, { seat: this.humanSeat, pass: true, cards: [], roomSeq: out.roomSeq });
      return { ok: true, out };
    }
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

  // submit 直接返回结算态（出完最后一张 / 最后一手过牌后收尾）时的统一处理。
  // 与 pump() 的 settled 分支保持一致：补记这一手动作 → 派发 settle → 停在主循环外。
  // patternId：最后一手若是炸弹（含红配补位），先派发 play 让特效播完，再弹结算层。
  settleFromSubmit(out, action, patternId = null) {
    this.selected.clear();
    this.waitingSeat = null;
    // 结算返回不带 action，用调用方提供的这一手补记，保证存档 replay 序列完整。
    this.remember({ ...action, roomSeq: action.roomSeq ?? out.roomSeq });
    const ended = out.kind === 'matchEnd' || !!out.summary?.matchEnd;
    this.phase = ended ? 'ended' : 'settle';

    const isBomb = typeof patternId === 'string' && /^(bomb-|joker-bomb|straight-flush)/.test(patternId);
    const targetPhase = this.phase;
    // 延迟结算期间若已开新局/重开，旧定时器不得再弹结算层污染新对局。
    const fireSettle = () => {
      if (this.phase !== targetPhase) return;
      this.emitEvent({ type: 'settle', summary: out.summary, ended, snapshot: this.snapshot() });
    };
    if (!action.pass && !action.tributeReturn && (action.cards?.length ?? 0) > 0) {
      // 最后一手是出牌：先派发 play（出牌动画 + 炸弹特效），结算层延迟到特效之后。
      this.emitEvent({
        type: 'play',
        seat: action.seat ?? this.humanSeat,
        action: { pass: false, cards: action.cards, patternId },
        auto: true,
        snapshot: this.snapshot(),
      });
      if (this.settleTimer) clearTimeout(this.settleTimer);
      this.settleTimer = setTimeout(() => { this.settleTimer = null; fireSettle(); },
        isBomb ? 1100 : 320); // 炸弹特效约 1s，先播完再弹结算；普通出牌等落位
    } else {
      fireSettle(); // 过牌收尾：无出牌动画，立即结算
    }
    this.emitChange();
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
    if (this.wasm.nextRound()) {
      this.phase = 'dealing';
      this.waitingSeat = null;
      this.lastBySeat = {};
      this.actions = []; // 新一局重新记录动作（存档按局恢复）
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
    this.start({
      seedHi: this.seedHi, seedLo: this.seedLo, humanSeat: this.humanSeat,
      level: this.level, botLevel: this.botLevel,
    });
  }

  // ---- 存档 / 恢复 ----
  // 存档只存"种子 + 难度 + 动作序列"，不存 whole state。
  // 恢复时用同种子重建房间并按序 replay 动作，哈希与进度可精确还原（wasm 已验证）。
  save() {
    return {
      v: 1,
      seedHi: this.seedHi,
      seedLo: this.seedLo,
      botLevel: this.botLevel,
      level: this.level,
      humanSeat: this.humanSeat,
      actions: this.actions.slice(),
      phase: this.phase,
    };
  }

  // 从存档恢复：返回 {ok, resume:boolean}。场景拿到 ok 后跳过发牌动画直接渲染。
  resume(save) {
    if (!save || save.v !== 1 || !Array.isArray(save.actions)) return { ok: false };
    this.stop();
    this.seedHi = save.seedHi >>> 0;
    this.seedLo = save.seedLo >>> 0;
    this.botLevel = Number.isInteger(save.botLevel) ? save.botLevel : 2;
    this.level = save.level ?? 2;
    this.humanSeat = save.humanSeat ?? 0;
    const mask = 1 << this.humanSeat;
    this.wasm.create(this.seedHi, this.seedLo, mask, this.botLevel);
    this.selected.clear();
    this.lastBySeat = {};
    this.hintIdx = 0;
    this.roundNo = 0;
    this.tributeSeen.clear();
    // 按序重放所有已发生动作
    let replayed = 0;
    for (const a of save.actions) {
      if (!this.wasm.replay(a)) break;
      replayed += 1;
      // 同步 lastBySeat 的表现层状态
      this.remember({ seat: a.seat, roomSeq: replayed, action: a });
    }
    this.actions = save.actions.slice(0, replayed);
    // 读档后进入 play（跳过发牌动画）；若存档在结算态则保持 settle 展示由场景决定。
    const restoredPhase = save.phase === 'settle' || save.phase === 'ended'
      ? save.phase
      : 'play';
    this.phase = restoredPhase;
    const info = this.wasm.info();
    this.waitingSeat = info?.turnSeat ?? null;
    this.emitEvent({
      type: 'resume',
      snapshot: this.snapshot(),
      info,
      actions: replayed,
    });
    this.emitChange();
    if (this.phase === 'play') this.pump();
    return { ok: true, actions: replayed };
  }
}
