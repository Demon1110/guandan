// 游戏主控：把 wasm 的 step/waiting/applied/settled 语义翻译成一次完整单机对局。
// 人类座位由 humanSeat 指定，其余三座由 wasm 内置的 Room bot（L2）自动决策。
// 本类只管状态与判定，不碰 Phaser；渲染由 GameScene 订阅 onChange/onEvent 完成。
import { sortHand } from './cards.js';
import { ReplayStore, matchIdOf } from './store.js';

export class Game {
  constructor(wasm, store = null) {
    this.wasm = wasm;
    this.store = store ?? new ReplayStore();
    this.seedHi = 0x51ed;
    this.seedLo = 0xa106;
    this.humanSeat = 0;
    this.level = 2; // 级牌（本局打几），与 bot 难度无关
    this.botLevel = 2; // 0=L0简单 / 1=L1普通 / 2=L2困难
    this.matchId = ''; // IndexedDB 场次主键（种子 + 座位）
    this.matchMeta = null; // 场次元信息（种子/难度/lastHash/lastRoomSeq/lastRoundNo）
    this.actions = []; // 整场动作日志（跨局累积，按 seq 去重），用于回放恢复
    this.seenSeqs = new Set(); // roomSeq 去重：submit 自派发与 step 可能指向同一手
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
    const snap = this.snapshot();
    // 单个监听器渲染异常不能拖垮游戏主流程（否则最后一手动画报错会连带吞掉结算）。
    for (const fn of this.listeners.change) {
      try { fn(snap); } catch (e) { console.error('[game] change 监听器异常', e); }
    }
  }

  emitEvent(ev) {
    for (const fn of this.listeners.event) {
      try { fn(ev); } catch (e) { console.error('[game] event 监听器异常', ev.type, e); }
    }
  }

  // 场次元信息：恢复时的种子/难度指纹 + 哈希基线 + 跨局进度检查点。
  buildMatchMeta(lastHash, lastRoomSeq, lastRoundNo) {
    return {
      matchId: this.matchId,
      seedHi: this.seedHi >>> 0,
      seedLo: this.seedLo >>> 0,
      humanSeat: this.humanSeat,
      humanMask: 1 << this.humanSeat,
      botLevel: this.botLevel,
      level: this.level,
      lastHash,
      lastRoomSeq,
      lastRoundNo,
      updatedAt: Date.now(),
    };
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
    // 动作日志按「场次」累积，不再每局清空：恢复时要跨局回放才能回到当前局。
    this.matchId = matchIdOf(this.seedHi, this.seedLo, this.humanSeat);
    // 同种子重开是全新一场：先清掉旧动作，避免高 seq 残留混进新局回放。
    this.store.clearMatch(this.matchId);
    this.actions = [];
    this.seenSeqs.clear();
    // lastHash 留空：没有任何动作时回放无法自证，按旧格式放行；第一手落盘时会填上权威 hash。
    this.matchMeta = this.buildMatchMeta('', 0, 1);
    this.store.putMatch(this.matchMeta);
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
          await this.flushTail();
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
  syncTribute({ silent = false } = {}) {
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
      // 恢复时静默标记已经发生过的贡牌，只把 pending 摊出来，不重播历史飞牌动画。
      if (silent) return;
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

  // 采集一条已发生动作：更新内存日志 + 表现层，并把动作与权威 hash 同事务落盘。
  // s 兼容三种来源：step/submit 的 applied（s.action + roomSeq/roundNo）、
  // wasm.lastRecord()（平铺 seq/roundNo/seat/...）、settleFromSubmit 补记（roomSeq + 平铺 action）。
  // persist=false 用于回放：只重建内存与表现层，不覆盖 IndexedDB 的哈希基线。
  remember(s, persist = true) {
    const a = s.action ?? s;
    const seq = s.seq ?? s.roomSeq ?? (this.actions.length + 1);
    const roundNo = s.roundNo ?? this.wasm.info?.()?.roundNo ?? 1;
    const rec = {
      seq,
      roundNo,
      seat: s.seat ?? a.seat,
      pass: !!a.pass,
      tributeReturn: !!a.tributeReturn,
      patternId: a.patternId ?? '',
      cards: a.cards ?? [],
    };
    // roomSeq 是引擎全局流水号：submit 自派发与 step 可能指向同一手，按 seq 去重。
    if (!this.seenSeqs.has(seq)) {
      this.seenSeqs.add(seq);
      this.actions.push(rec);
    }
    this.lastBySeat[rec.seat] = {
      pass: rec.pass,
      tributeReturn: rec.tributeReturn,
      cards: rec.cards,
      patternId: rec.patternId,
    };
    // lastPlay 被清空 ⇒ 新墩，撤掉四家的牌。
    try {
      if (!this.wasm.view(this.humanSeat).lastPlay) this.lastBySeat = {};
    } catch { /* 视图暂不可用忽略 */ }
    if (!persist || !this.matchMeta) return;
    // 动作与「最新权威哈希」同一事务落盘：杀进程时两者必然一致，恢复后才有比对基线。
    this.matchMeta = {
      ...this.matchMeta,
      lastHash: this.wasm.hash(),
      lastRoomSeq: seq,
      lastRoundNo: roundNo,
      updatedAt: Date.now(),
    };
    this.store.appendAction(this.matchId, rec, this.matchMeta);
  }

  // 结算态补记：引擎可能直接返回 settled/matchEnd 而不先给 applied，
  // 用 lastRecord 把最后一手补齐（按 seq 去重，重复调用无副作用）。
  async flushTail() {
    try {
      const last = this.wasm.lastRecord();
      if (last && !this.seenSeqs.has(last.seq)) this.remember(last);
    } catch { /* 旧 wasm 没有 last_record 导出时忽略 */ }
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

    // submit 内可能结算并升级级牌；先记下这一手打出时的级牌供动画渲染。
    const playedLevel = this.levelRankLabel();
    const out = this.wasm.submit(this.humanSeat, this.wasm.now(), {
      cards: hand, pass: false, tributeReturn: false,
    });
    // 出完最后一张直接结算：submit 返回 settled/matchEnd。
    if (out.kind === 'settled' || out.kind === 'matchEnd') {
      this.settleFromSubmit(out, { seat: this.humanSeat, pass: false, cards: hand, roomSeq: out.roomSeq }, check?.patternId, playedLevel);
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
    // submit 内可能结算并升级级牌；先记下这一手打出时的级牌供动画渲染。
    const playedLevel = this.levelRankLabel();
    const out = this.wasm.submit(this.humanSeat, this.wasm.now(), {
      cards,
      pass: false,
      tributeReturn: isTribute,
    });
    // 出最后一张牌会直接触发结算：submit 返回 settled/matchEnd（而非 applied）。
    // 此时牌已被引擎消耗、名次已定，必须按结算处理，否则误判"出牌被拒"卡死。
    if (out.kind === 'settled' || out.kind === 'matchEnd') {
      this.settleFromSubmit(out, { seat: this.humanSeat, pass: false, cards, roomSeq: out.roomSeq }, check?.patternId, playedLevel);
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
  // playedLevel：submit 之前捕获的级牌。引擎在 submit 内已结算并升级级牌（如 6→7），
  //   最后一手动画必须用升级前的级牌渲染——纹理只预热了旧级牌，用新级牌会抛"主题纹理缺失"。
  settleFromSubmit(out, action, patternId = null, playedLevel = null) {
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
      // 先注册结算定时器，再播最后一手动画：动画渲染异常也绝不会吞掉结算弹框。
      if (this.settleTimer) clearTimeout(this.settleTimer);
      this.settleTimer = setTimeout(() => { this.settleTimer = null; fireSettle(); },
        isBomb ? 1100 : 320); // 炸弹特效约 1s，先播完再弹结算；普通出牌等落位
      // 最后一手是出牌：派发 play（出牌动画 + 炸弹特效），结算层延迟到特效之后。
      this.emitEvent({
        type: 'play',
        seat: action.seat ?? this.humanSeat,
        action: { pass: false, cards: action.cards, patternId },
        level: playedLevel,
        auto: true,
        snapshot: this.snapshot(),
      });
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
      // 动作日志是整场的，跨局不清空：恢复时要跨局回放才能回到当前局。
      this.selected.clear();
      const info = this.wasm.info();
      // 跨局检查点：新局可能还没有任何动作（停在 tribute），也要把进度和发牌后哈希落盘，
      // 否则刷新会退回上一局结算。
      if (this.matchMeta) {
        this.matchMeta = {
          ...this.matchMeta,
          lastHash: this.wasm.hash(),
          lastRoomSeq: this.wasm.meta?.()?.roomSeq ?? this.matchMeta.lastRoomSeq,
          lastRoundNo: info.roundNo ?? this.matchMeta.lastRoundNo,
          updatedAt: Date.now(),
        };
        this.store.putMatch(this.matchMeta);
      }
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

  // ---- 回放存档 / 恢复 ----
  // 只存「种子 + 难度 + 座位 + 动作日志 + 权威哈希」，不存 whole state。
  // 恢复时用同种子重建房间、按 roundNo 跨局 replay 动作，哈希一致才继续打。
  save() {
    return {
      v: 2,
      matchId: this.matchId,
      seedHi: this.seedHi,
      seedLo: this.seedLo,
      botLevel: this.botLevel,
      level: this.level,
      humanSeat: this.humanSeat,
      actions: this.actions.slice(),
      phase: this.phase,
      meta: this.matchMeta ? { ...this.matchMeta } : null,
    };
  }

  /** 供 UI 判断是否有可续的 IndexedDB 存档（按更新时序取最近一场）。 */
  async hasReplaySave(matchId = null) {
    await this.store.open();
    const meta = matchId ? await this.store.loadMatch(matchId) : await this.store.latestMatch();
    if (!meta) return null;
    const acts = await this.store.loadActions(meta.matchId);
    return acts.length ? { meta, actions: acts } : null;
  }

  /**
   * 从 IndexedDB 恢复：不复用任何旧内存，只按动作日志重演。
   * onReady(game) 在重放完成后、派发恢复事件前调用，供场景预热当前级牌的主题纹理
   * （恢复跳过发牌动画，否则同步渲染手牌会因纹理缺失抛错中断主循环）。
   */
  async resumeFromDb(matchId = null, onReady = null) {
    await this.store.open();
    const meta = matchId ? await this.store.loadMatch(matchId) : await this.store.latestMatch();
    if (!meta) return { ok: false, reason: '没有找到可续的存档' };
    const acts = await this.store.loadActions(meta.matchId);
    if (!acts.length) return { ok: false, reason: '存档没有动作日志' };
    return this.resumeFrom(meta, acts, onReady);
  }

  // 核心重放：IndexedDB / 旧 localStorage 两个入口共用。
  async resumeFrom(meta, acts, onReady = null) {
    this.stop();
    this.seedHi = meta.seedHi >>> 0;
    this.seedLo = meta.seedLo >>> 0;
    this.botLevel = Number.isInteger(meta.botLevel) ? meta.botLevel : 2;
    this.level = meta.level ?? 2;
    this.humanSeat = meta.humanSeat ?? 0;
    this.matchId = meta.matchId || matchIdOf(this.seedHi, this.seedLo, this.humanSeat);
    this.matchMeta = {
      ...this.buildMatchMeta(meta.lastHash ?? '', meta.lastRoomSeq ?? 0, meta.lastRoundNo ?? 1),
      matchId: this.matchId,
    };
    const mask = 1 << this.humanSeat;
    this.wasm.create(this.seedHi, this.seedLo, mask, this.botLevel);
    this.selected.clear();
    this.lastBySeat = {};
    this.hintIdx = 0;
    this.roundNo = 0;
    this.tributeSeen.clear();
    this.tributeFlights = 0;
    this.actions = [];
    this.seenSeqs.clear();

    let currentRound = 1;
    for (let i = 0; i < acts.length; i += 1) {
      const rec = acts[i];
      // 引擎 round_no 从 1 开始；动作跨局时先把房间推进到那一局。
      while ((rec.roundNo ?? 1) > currentRound) {
        if (!this.wasm.nextRound(0)) {
          return { ok: false, reason: `回放时续局失败（动作 ${i + 1}/${acts.length}）` };
        }
        currentRound += 1;
      }
      if (!this.wasm.replay(rec)) {
        return {
          ok: false,
          reason: `回放动作失败 idx=${i + 1}/${acts.length} seq=${rec.seq} round=${rec.roundNo} seat=${rec.seat}`,
        };
      }
      // 只重建表现层，不写盘：回放不能覆盖 IndexedDB 的哈希基线。
      this.remember(rec, false);
    }
    // 跨局检查点：新一局可能还没有任何动作（停在 tribute），也要补到存档时的局号。
    const targetRound = Number(meta.lastRoundNo ?? currentRound);
    while (currentRound < targetRound) {
      if (!this.wasm.nextRound(0)) {
        return { ok: false, reason: `回放时补齐第 ${currentRound + 1} 局失败` };
      }
      currentRound += 1;
    }

    const h = this.wasm.hash();
    const hashMatched = h === meta.lastHash;
    // 老存档没有 lastHash 字段，按旧格式续；有基线就必须自证，不一致直接拒绝续局，
    // 否则「哈希自证」只是日志装饰，坏存档会带着错误状态继续打。
    if (meta.lastHash && !hashMatched) {
      return {
        ok: false,
        hashMatched,
        reason: `存档哈希不一致，拒绝续局：存档=${meta.lastHash} 恢复=${h}（${acts.length} 条动作）`,
      };
    }
    // 恢复前准备（纹理预热等）：此时房间已重放到当前局，能拿到真实级牌。
    if (onReady) {
      try {
        await onReady(this);
      } catch (e) {
        return { ok: false, reason: `恢复前准备失败：${e?.message ?? e}` };
      }
    }
    this.actions = acts.slice();
    this.seenSeqs = new Set(acts.map((a) => a.seq));
    this.phase = 'play'; // 跳过发牌动画；引擎若在结算/进贡，由 pump 自然推进
    this.waitingSeat = null;
    // 恢复倒计时窗口：wasm 仍持有原 deadline，不恢复会被判超时托管。
    this.wasm.resumeWindow?.();
    // 已发生过的贡牌不重播动画，只把 pending 留给玩家操作。
    this.syncTribute({ silent: true });
    const info = this.wasm.info();
    // 单个监听器抛错（如纹理异常）不能吞掉主循环，否则牌桌停在"已恢复"却不走牌。
    try {
      this.emitEvent({
        type: 'resume',
        snapshot: this.snapshot(),
        info,
        actions: acts.length,
        roundNo: currentRound,
        hashMatched,
        matchId: this.matchId,
      });
      this.emitChange();
    } catch (e) {
      console.error('[replay] 恢复后渲染异常，主循环仍会启动', e);
    }
    this.pump();
    return { ok: true, actions: acts.length, hashMatched, roundNo: currentRound };
  }

  // 兼容旧 localStorage 存档入口：老格式只有当前局动作、没有 seq/roundNo/哈希基线，
  // 映射成 roundNo=1 的动作日志后走同一套重放逻辑。
  resume(save) {
    if (!save || save.v !== 1 || !Array.isArray(save.actions)) return { ok: false };
    const meta = {
      matchId: matchIdOf(save.seedHi, save.seedLo, save.humanSeat ?? 0),
      seedHi: save.seedHi,
      seedLo: save.seedLo,
      botLevel: save.botLevel ?? 2,
      level: save.level ?? 2,
      humanSeat: save.humanSeat ?? 0,
      lastHash: '',
      lastRoomSeq: 0,
      lastRoundNo: 1,
    };
    const acts = save.actions.map((a, i) => ({
      seq: i + 1,
      roundNo: 1,
      seat: a.seat,
      pass: !!a.pass,
      tributeReturn: !!a.tributeReturn,
      patternId: a.patternId ?? '',
      cards: a.cards ?? [],
    }));
    return this.resumeFrom(meta, acts);
  }
}
