// 掼蛋 M1 本地房间 · 表现层
// 牌面 id 编码与引擎一致：`(suit × 13 + rank) × 2 + copy`，suit 0..3 = ♠♥♦♣，rank 0..12 = 2..A；
// 王 id 从 104 起：104/105 小王，106/107 大王（两副牌）。
const RANKS = ['2', '3', '4', '5', '6', '7', '8', '9', '10', 'J', 'Q', 'K', 'A'];
const SUITS = ['♠', '♥', '♦', '♣'];
const JOKER_BASE = 104;

function cardLabel(id) {
  if (id >= JOKER_BASE) return id < JOKER_BASE + 2 ? '小王' : '大王';
  const base = Math.floor(id / 2);
  return RANKS[base % 13] + SUITS[Math.floor(base / 13)];
}

function cardFace(id) {
  if (id >= JOKER_BASE) {
    return { rank: id < JOKER_BASE + 2 ? '小' : '大', suit: '王', red: id >= JOKER_BASE + 2, joker: true };
  }
  const base = Math.floor(id / 2);
  const suit = Math.floor(base / 13);
  return { rank: RANKS[base % 13], suit: SUITS[suit], red: suit === 1 || suit === 2, joker: false };
}

// 展示顺序：按点数升序（2…A、小王、大王），同点按花色，再按副本号。
function sortKey(id) {
  if (id >= JOKER_BASE) return 400 + (id - JOKER_BASE);
  const base = Math.floor(id / 2);
  return (base % 13) * 10 + Math.floor(base / 13) * 2 + (id % 2);
}

// ---------- IndexedDB 存档（只存动作日志 + 场次元信息） ----------

const DB_NAME = 'guandan-m1';
const STORE_MATCH = 'matches';
const STORE_ACTION = 'actions';

function openDb() {
  return new Promise((resolve, reject) => {
    const req = indexedDB.open(DB_NAME, 1);
    req.onupgradeneeded = () => {
      const db = req.result;
      if (!db.objectStoreNames.contains(STORE_MATCH)) db.createObjectStore(STORE_MATCH, { keyPath: 'matchId' });
      if (!db.objectStoreNames.contains(STORE_ACTION)) db.createObjectStore(STORE_ACTION, { keyPath: ['matchId', 'seq'] });
    };
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => reject(req.error);
  });
}

function tx(db, store, mode) {
  return db.transaction(store, mode).objectStore(store);
}

function reqToPromise(req) {
  return new Promise((resolve, reject) => {
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => reject(req.error);
  });
}

async function saveMatch(db, match) {
  await reqToPromise(tx(db, STORE_MATCH, 'readwrite').put(match));
}

async function loadMatch(db, matchId) {
  return reqToPromise(tx(db, STORE_MATCH, 'readonly').get(matchId));
}

async function loadActions(db, matchId) {
  const all = await reqToPromise(tx(db, STORE_ACTION, 'readonly').getAll());
  return all.filter((a) => a.matchId === matchId).sort((x, y) => x.seq - y.seq);
}

async function clearMatch(db, matchId) {
  const acts = await loadActions(db, matchId);
  const t = db.transaction([STORE_ACTION, STORE_MATCH], 'readwrite');
  for (const a of acts) t.objectStore(STORE_ACTION).delete([a.matchId, a.seq]);
  t.objectStore(STORE_MATCH).delete(matchId);
  await new Promise((resolve, reject) => {
    t.oncomplete = () => resolve();
    t.onerror = () => reject(t.error);
  });
}

// ---------- wasm 房间封装 ----------

let ex = null;
let clockBase = 0;

function now() {
  return Math.round(performance.now() - clockBase);
}

function cstr(ptr) {
  if (!ptr) throw new Error('wasm 返回空指针');
  const view = new Uint8Array(ex.memory.buffer, ptr);
  let end = 0;
  while (end < view.length && view[end] !== 0) end += 1;
  const text = new TextDecoder().decode(view.subarray(0, end));
  ex.gf_str_free(ptr);
  return text;
}

function withCards(cards, fn) {
  if (!cards || cards.length === 0) return fn(0, 0);
  const len = cards.length * 2;
  const ptr = ex.gf_alloc(len);
  new Uint16Array(ex.memory.buffer, ptr, cards.length).set(cards);
  try {
    return fn(ptr, cards.length);
  } finally {
    ex.gf_mem_free(ptr, len);
  }
}

const room = {
  create(seedHi, seedLo, humanMask, level) {
    // gf_room_new_level 是新增导出；旧产物没有它时退化为默认 L2。
    let ok;
    if (ex.gf_room_new_level) ok = ex.gf_room_new_level(seedHi, seedLo, humanMask, level);
    else ok = ex.gf_room_new(seedHi, seedLo, humanMask);
    if (ok !== 1) throw new Error('建桌失败');
    if (ex.gf_room_start(BigInt(0)) !== 1) throw new Error('发牌失败');
  },
  step(at) {
    return JSON.parse(cstr(ex.gf_room_step(BigInt(at))));
  },
  submit(seat, at, action) {
    return withCards(action.cards ?? [], (ptr, len) =>
      JSON.parse(cstr(ex.gf_room_submit(seat, BigInt(at), action.pass ? 1 : 0, ptr, len))),
    );
  },
  replay(rec) {
    return withCards(rec.cards ?? [], (ptr, len) =>
      ex.gf_room_replay_action(rec.seat, rec.pass ? 1 : 0, ptr, len) === 1,
    );
  },
  nextRound(at) {
    return JSON.parse(cstr(ex.gf_room_next_round(BigInt(at)))).kind === 'dealt';
  },
  resumeWindow(at) {
    return ex.gf_room_resume_window(BigInt(at)) === 1;
  },
  view(seat) {
    return JSON.parse(cstr(ex.gf_room_view(seat)));
  },
  hash() {
    return cstr(ex.gf_room_hash());
  },
  meta() {
    return JSON.parse(cstr(ex.gf_room_meta()));
  },
  seats() {
    return JSON.parse(cstr(ex.gf_room_seats())).seats;
  },
  lastRecord() {
    const raw = JSON.parse(cstr(ex.gf_room_last_record()));
    if (!raw) return null;
    return { seq: raw.roomSeq, roundNo: raw.roundNo, seat: raw.seat, pass: raw.action.pass, cards: raw.action.cards ?? [] };
  },
};

// ---------- 页面状态 ----------

const state = {
  db: null,
  matchId: '',
  seedHi: 0x51ed,
  seedLo: 0xa106,
  humanMask: 0b0001,
  humanSeat: 0,
  level: 2,
  recorded: 0,
  selected: new Set(),
  waitingSeat: null,
  deadline: 0,
  running: false,
  phase: 'idle', // idle | play | settle | ended
  speed: 240,
  showIds: false,
  showLog: true,
  lastBySeat: {},
  notice: '',
  noticeTimer: 0,
};

const el = (id) => document.getElementById(id);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

function seedFromQuery() {
  const q = new URLSearchParams(location.search);
  const raw = q.get('seed');
  if (!raw) return false;
  const v = raw.startsWith('0x') ? BigInt(raw) : BigInt('0x' + raw);
  const u = v & ((1n << 64n) - 1n);
  state.seedHi = Number(u >> 32n);
  state.seedLo = Number(u & 0xffffffffn);
  const seat = Number(q.get('seat') ?? 0);
  if (seat >= 0 && seat <= 3) state.humanSeat = seat;
  state.humanMask = 1 << state.humanSeat;
  const level = Number(q.get('level') ?? 2);
  state.level = level === 0 || level === 1 ? level : 2;
  return true;
}

function matchIdOf(hi, lo) {
  return `web-${hi.toString(16)}${lo.toString(16).padStart(8, '0')}`;
}

// ---------- 大厅 ----------

async function showLobby() {
  el('app').classList.add('hidden');
  el('lobby').classList.remove('hidden');
  el('lobby-seed').value = `0x${state.seedHi.toString(16)}${state.seedLo.toString(16).padStart(8, '0')}`;
  el('lobby-seat').value = String(state.humanSeat);
  el('lobby-level').value = String(state.level);
  if (!state.db) state.db = await openDb();
  await refreshLobbyNote();
}

async function refreshLobbyNote() {
  const note = el('lobby-note');
  const btn = el('lobby-resume');
  btn.classList.add('hidden');
  let parsed;
  try {
    parsed = parseSeedInput(el('lobby-seed').value);
  } catch (e) {
    note.textContent = `种子格式不对：${e.message}`;
    return;
  }
  const seat = Number(el('lobby-seat').value);
  const id = matchIdOf(parsed.hi, parsed.lo);
  const saved = await loadMatch(state.db, id);
  const acts = saved ? await loadActions(state.db, id) : [];
  if (saved && acts.length > 0) {
    note.textContent = `该种子 + 座位 ${seat} 有存档：${acts.length} 条动作，第 ${saved.lastRoundNo ?? '?'} 局，点「开始比赛」会自动续局。`;
    btn.classList.remove('hidden');
  } else {
    note.textContent = '该种子 + 座位没有存档，将开新局。';
  }
}

function parseSeedInput(text) {
  const t = text.trim();
  if (!/^(0x)?[0-9a-fA-F]{1,16}$/.test(t)) throw new Error('需要 1–16 位十六进制');
  const v = (t.startsWith('0x') ? BigInt(t) : BigInt('0x' + t)) & ((1n << 64n) - 1n);
  return { hi: Number(v >> 32n), lo: Number(v & 0xffffffffn) };
}

// ---------- 牌桌渲染 ----------

function relSlot(seat) {
  return (seat - state.humanSeat + 4) % 4; // 0 下 / 1 右 / 2 上 / 3 左
}

function renderSeats() {
  const seats = room.seats();
  for (const s of seats) {
    const box = el(`seat-slot-${relSlot(s.seat)}`);
    if (!box) continue;
    const mine = s.seat === state.humanSeat;
    const active = state.waitingSeat === s.seat;
    const last = state.lastBySeat[s.seat];
    box.className = `seat slot-${['bottom', 'right', 'top', 'left'][relSlot(s.seat)]}` + (active ? ' active' : '');
    const body = mine
      ? `手牌 ${(room.view(state.humanSeat).hand.cards ?? []).length} 张`
      : (last ? `最近：${last}` : '最近：—');
    box.innerHTML = `
      <div class="seat-head">
        <span>座位 ${s.seat} ${mine ? '· 你' : '· bot'}</span>
        <span class="tag${active ? ' gold' : ''}">${active ? '行动中' : mine ? '本家' : s.human ? '玩家' : 'AI'}</span>
      </div>
      <div class="seat-body">${body}</div>
    `;
  }
}

function renderCenter() {
  const v = room.view(state.humanSeat);
  const last = v.lastPlay;
  const box = el('last-play');
  if (!last || last.cards.length === 0) {
    box.innerHTML = `<span class="who">本回合无人出牌</span>`;
  } else {
    const cards = last.cards.map((id) => {
      const f = cardFace(id);
      return `<span class="mini-card${f.red ? ' red' : ''}">${f.rank}${f.suit}</span>`;
    }).join('');
    box.innerHTML = `<span class="who">座位 ${last.seat} 出（${last.patternId}）</span>${cards}`;
  }

  const myTurn = state.waitingSeat === state.humanSeat;
  if (state.phase === 'settle') el('turn').textContent = '本局结算中';
  else if (state.phase === 'ended') el('turn').textContent = '场次已结束';
  else if (myTurn) {
    const left = Math.max(0, Math.round(state.deadline - now()));
    el('turn').textContent = `轮到你 · 剩 ${left} ms`;
  } else if (state.waitingSeat === null) {
    el('turn').textContent = '…';
  } else {
    el('turn').textContent = `等座位 ${state.waitingSeat} 行动`;
  }

  const team = (state.humanSeat + 2) % 4;
  const extra = el('table-extra');
  if (state.notice) {
    extra.textContent = state.notice;
    extra.style.color = '#ffb0b0';
  } else {
    extra.textContent = `你座位 ${state.humanSeat} · 队友座位 ${team} · 两队等级 ${(v['seat.levels'] ?? []).join('/')}`;
    extra.style.color = '';
  }
}

function renderHand() {
  const v = room.view(state.humanSeat);
  const cards = [...v.hand.cards].sort((a, b) => sortKey(a) - sortKey(b));
  const box = el('hand');
  box.innerHTML = '';
  const myTurn = state.waitingSeat === state.humanSeat;
  for (const id of cards) {
    const f = cardFace(id);
    const b = document.createElement('button');
    b.className = `card${f.red ? ' red' : ''}${f.joker ? ' joker' : ''}${state.selected.has(id) ? ' picked' : ''}${myTurn ? '' : ' dim'}`;
    b.innerHTML = `<span class="suit">${f.suit}</span><span>${f.rank}</span>${state.showIds ? `<span class="cid">${id}</span>` : ''}`;
    b.onclick = () => {
      if (state.waitingSeat !== state.humanSeat) return;
      if (state.selected.has(id)) state.selected.delete(id);
      else state.selected.add(id);
      renderHand();
      renderActions();
    };
    box.appendChild(b);
  }
  el('hand-count').textContent = `手牌 ${cards.length} 张`;
}

function renderActions() {
  const v = room.view(state.humanSeat);
  const myTurn = state.waitingSeat === state.humanSeat;
  // 领出时不能过牌：上一手为空或上一手就是自己，必须出牌。
  const canPass = myTurn && !!v.lastPlay && v.lastPlay.seat !== state.humanSeat;
  el('play').disabled = !myTurn || state.selected.size === 0;
  el('pass').disabled = !canPass;
  el('clear-select').disabled = state.selected.size === 0;
  el('selection').textContent = state.selected.size === 0
    ? '未选牌'
    : `已选 ${state.selected.size} 张：${[...state.selected].sort((a, b) => sortKey(a) - sortKey(b)).map(cardLabel).join(' ')}`;
}

function renderHeader() {
  const m = room.meta();
  el('match-id').textContent = state.matchId;
  el('meta').textContent = `第 ${m.roundNo} 局 · roomSeq ${m.roomSeq} · 存档 ${state.recorded} 条 · bot L${state.level}`;
  el('hash').textContent = room.hash();
}

function renderAll() {
  renderSeats();
  renderCenter();
  renderHand();
  renderActions();
  renderHeader();
}

// ---------- 日志 ----------

function log(line) {
  const box = el('log');
  const li = document.createElement('li');
  li.textContent = line;
  box.prepend(li);
  while (box.children.length > 120) box.removeChild(box.lastChild);
}

// ---------- 存档 ----------

async function record(rec) {
  state.recorded += 1;
  // 动作与「最新权威哈希」同一事务落盘：杀进程时两者必然一致，恢复后才有比对基线。
  const t = state.db.transaction([STORE_ACTION, STORE_MATCH], 'readwrite');
  t.objectStore(STORE_ACTION).put({ matchId: state.matchId, ...rec });
  t.objectStore(STORE_MATCH).put({
    matchId: state.matchId,
    seedHi: state.seedHi,
    seedLo: state.seedLo,
    humanMask: state.humanMask,
    humanSeat: state.humanSeat,
    level: state.level,
    lastHash: room.hash(),
    lastRoomSeq: rec.seq,
    lastRoundNo: rec.roundNo,
    updatedAt: Date.now(),
  });
  await new Promise((resolve, reject) => {
    t.oncomplete = () => resolve();
    t.onerror = () => reject(t.error);
  });
}

function toRecord(s) {
  return { seq: s.roomSeq, roundNo: s.roundNo, seat: s.seat, pass: s.action.pass, cards: s.action.cards ?? [] };
}

function describe(rec) {
  return rec.pass ? '过牌' : '出 ' + (rec.cards ?? []).map(cardLabel).join(' ');
}

// ---------- 结算 ----------

function showSettle(summary, ended) {
  state.phase = ended ? 'ended' : 'settle';
  state.selected.clear();
  const ranks = summary?.ranks ?? [];
  const rows = el('settle-rows');
  rows.innerHTML = '';
  ranks.forEach((seat, i) => {
    const mine = seat === state.humanSeat;
    const ally = seat % 2 === state.humanSeat % 2;
    const tr = document.createElement('tr');
    tr.innerHTML = `<td>${i + 1}</td><td>座位 ${seat}${mine ? '（你）' : ''}</td><td>${ally ? '我方' : '对方'}</td>`;
    rows.appendChild(tr);
  });
  const winnerTeam = summary?.winnerTeam;
  el('settle-title').textContent = ended ? `场次结束 · 第 ${summary?.roundNo ?? '?'} 局` : `第 ${summary?.roundNo ?? '?'} 局结算`;
  el('settle-extra').innerHTML = [
    `名次：${ranks.join(' → ') || '-'}`,
    `头游队：team ${winnerTeam ?? '-'}`,
    ended ? '本场已结束（有队伍过 A 或触发安全上限），可以回大厅换种子重开。' : '点「再来一局」按本局结果发牌续打。',
  ].join('<br/>');
  el('settle-next').disabled = !!ended;
  el('settle').classList.remove('hidden');
  renderAll();
}

// ---------- 主循环 ----------

async function flushTail() {
  const meta = room.meta();
  if (meta.records > state.recorded) {
    const last = room.lastRecord();
    if (last) {
      await record(last);
      state.lastBySeat[last.seat] = describe(last);
      log(`座位 ${last.seat} ${describe(last)}`);
    }
  }
}

async function pump() {
  if (state.running || state.phase === 'settle' || state.phase === 'ended') return;
  state.running = true;
  try {
    for (let i = 0; i < 400; i += 1) {
      const s = room.step(now());
      if (s.kind === 'applied') {
        const rec = toRecord(s);
        await record(rec);
        state.lastBySeat[s.seat] = describe(rec);
        log(`座位 ${s.seat} ${describe(rec)}${s.auto ? `（托管 ${s.auto}）` : ''} · #${rec.seq}`);
        if (s.seat !== state.humanSeat) await sleep(state.speed);
        continue;
      }
      if (s.kind === 'settled') {
        await flushTail();
        const ended = !!s.summary?.matchEnd;
        log(`第 ${s.summary.roundNo} 局结算：名次 ${s.summary.ranks.join(',')} 头游队 ${s.summary.winnerTeam}${ended ? ' · 场次结束' : ''}`);
        showSettle(s.summary, ended);
        return;
      }
      if (s.kind === 'matchEnd') {
        await flushTail();
        log(`场次结束：胜方 team ${s.winnerTeam ?? s.summary?.winnerTeam ?? '-'}`);
        showSettle(s.summary, true);
        return;
      }
      if (s.kind === 'waiting') {
        state.waitingSeat = s.seat;
        state.deadline = s.deadlineMs;
        state.phase = 'play';
        renderAll();
        // 人类回合：低频轮询只为检查超时托管；bot 回合快一点。
        setTimeout(pump, state.waitingSeat === state.humanSeat ? 400 : Math.max(120, state.speed));
        return;
      }
      if (s.kind === 'error') {
        log(`房间错误：${s.error}`);
        return;
      }
    }
  } finally {
    state.running = false;
  }
  renderAll();
}

// ---------- 启动 / 续局 ----------

async function loadWasm() {
  if (ex) return;
  const bytes = await (await fetch('gf_client_wasm.wasm')).arrayBuffer();
  const module = await WebAssembly.compile(bytes);
  ex = (await WebAssembly.instantiate(module, {})).exports;
  clockBase = performance.now();
}

async function startGame() {
  await loadWasm();
  if (!state.db) state.db = await openDb();
  state.matchId = matchIdOf(state.seedHi, state.seedLo);
  state.recorded = 0;
  state.selected.clear();
  state.lastBySeat = {};
  state.waitingSeat = null;
  state.phase = 'play';
  el('log').innerHTML = '';

  const saved = await loadMatch(state.db, state.matchId);
  const acts = await loadActions(state.db, state.matchId);

  room.create(state.seedHi, state.seedLo, state.humanMask, state.level);

  if (saved && acts.length > 0) {
    // 杀进程续局：不复用任何旧内存，只按 IndexedDB 里的动作日志重演。
    let currentRound = 1;
    for (let i = 0; i < acts.length; i += 1) {
      const rec = acts[i];
      while (rec.roundNo > currentRound) {
        if (!room.nextRound(0)) throw new Error('回放时续局失败');
        currentRound += 1;
      }
      if (!room.replay(rec)) {
        throw new Error(
          `回放动作失败 idx=${i}/${acts.length} seq=${rec.seq} round=${rec.roundNo} seat=${rec.seat} pass=${rec.pass} cards=${rec.cards}`
          + ` 前四条=${JSON.stringify(acts.slice(Math.max(0, i - 3), i + 1))}`,
        );
      }
      state.lastBySeat[rec.seat] = describe(rec);
    }
    const h = room.hash();
    const same = h === saved.lastHash;
    log(`从 IndexedDB 恢复 ${acts.length} 条动作：${same ? '哈希一致 ✓' : `哈希不一致 ✗ 存档=${saved.lastHash} 恢复=${h}`}`);
    state.recorded = acts.length;
  } else {
    await saveMatch(state.db, {
      matchId: state.matchId,
      seedHi: state.seedHi,
      seedLo: state.seedLo,
      humanMask: state.humanMask,
      humanSeat: state.humanSeat,
      level: state.level,
      lastHash: '',
      updatedAt: Date.now(),
    });
    log('新开一局（无存档）');
  }

  room.resumeWindow(now());
  el('lobby').classList.add('hidden');
  el('app').classList.remove('hidden');
  el('settle').classList.add('hidden');
  el('side').classList.toggle('hidden', !state.showLog);
  renderAll();
  pump();
}

// ---------- 人类动作 ----------

async function humanPlay(pass) {
  if (state.waitingSeat !== state.humanSeat) return;
  const cards = pass ? [] : [...state.selected].sort((a, b) => a - b);
  if (!pass && cards.length === 0) return;
  const out = room.submit(state.humanSeat, now(), { pass, cards });
  if (out.kind !== 'applied') {
    // 被拒时给一次明显的视觉反馈，同时把原来的选牌留着方便改选。
    state.notice = `${pass ? '过牌' : '出牌'}被拒：这套牌现在不合法（牌型不匹配或压不过上一手）`;
    log(`${pass ? '过牌' : '出牌'}被拒：${out.error ?? out.kind}`);
    renderCenter();
    clearTimeout(state.noticeTimer);
    state.noticeTimer = setTimeout(() => { state.notice = ''; renderCenter(); }, 3000);
    return;
  }
  state.selected.clear();
  state.waitingSeat = null;
  state.lastBySeat[state.humanSeat] = pass ? '过牌' : '出 ' + cards.map(cardLabel).join(' ');
  await record(toRecord(out));
  log(`你 ${pass ? '过牌' : '出 ' + cards.map(cardLabel).join(' ')}`);
  pump();
}

// ---------- 设置 ----------

function loadSettings() {
  const stored = localStorage.getItem('guandan-m1-settings');
  try {
    const raw = JSON.parse(stored ?? '{}');
    const sp = { fast: 120, normal: 240, slow: 600 };
    if (raw.speedName && sp[raw.speedName]) state.speed = sp[raw.speedName];
    state.showIds = !!raw.showIds;
    state.showLog = raw.showLog !== false;
    el('set-speed').value = raw.speedName ?? 'normal';
    el('set-ids').checked = state.showIds;
    el('set-log').checked = state.showLog;
  } catch { /* 忽略坏设置 */ }
  // 窄屏没有横向空间放侧栏，首次进入默认收起（用户仍可在设置里打开）。
  if (!stored && window.innerWidth < 1200) {
    state.showLog = false;
    el('set-log').checked = false;
  }
}

function saveSettings() {
  const speedName = el('set-speed').value;
  state.speed = { fast: 120, normal: 240, slow: 600 }[speedName] ?? 240;
  state.showIds = el('set-ids').checked;
  state.showLog = el('set-log').checked;
  localStorage.setItem('guandan-m1-settings', JSON.stringify({ speedName, showIds: state.showIds, showLog: state.showLog }));
  el('side').classList.toggle('hidden', !state.showLog || state.phase === 'idle');
}

// ---------- 事件绑定 ----------

el('play').onclick = () => humanPlay(false);
el('pass').onclick = () => humanPlay(true);
el('clear-select').onclick = () => { state.selected.clear(); renderHand(); renderActions(); };
el('reload').onclick = () => location.reload();
el('toggle-log').onclick = () => {
  state.showLog = !state.showLog;
  el('side').classList.toggle('hidden', !state.showLog);
};
el('open-settings').onclick = () => el('settings').classList.remove('hidden');
el('set-close').onclick = () => { saveSettings(); el('settings').classList.add('hidden'); renderAll(); };
el('set-clear').onclick = async () => {
  if (state.matchId && state.db) await clearMatch(state.db, state.matchId);
  location.reload();
};
el('set-reload').onclick = () => location.reload();
el('back-lobby').onclick = () => {
  state.phase = 'idle';
  showLobby();
};

el('settle-next').onclick = () => {
  el('settle').classList.add('hidden');
  if (!room.nextRound(now())) {
    log('续局失败');
    return;
  }
  state.phase = 'play';
  state.waitingSeat = null;
  pump();
};
el('settle-lobby').onclick = () => {
  el('settle').classList.add('hidden');
  state.phase = 'idle';
  showLobby();
};

el('lobby-seed').oninput = () => { refreshLobbyNote().catch(() => {}); };
el('lobby-seat').onchange = () => { refreshLobbyNote().catch(() => {}); };

el('lobby-start').onclick = async () => {
  try {
    const p = parseSeedInput(el('lobby-seed').value);
    state.seedHi = p.hi;
    state.seedLo = p.lo;
    state.humanSeat = Number(el('lobby-seat').value);
    state.humanMask = 1 << state.humanSeat;
    state.level = Number(el('lobby-level').value);
    const qs = `?seed=0x${p.hi.toString(16)}${p.lo.toString(16).padStart(8, '0')}&seat=${state.humanSeat}&level=${state.level}`;
    history.replaceState(null, '', qs);
    await startGame();
  } catch (e) {
    el('lobby-note').textContent = `开局失败：${e.message}`;
  }
};

el('lobby-resume').onclick = () => el('lobby-start').onclick();

// ---------- 入口 ----------

async function init() {
  loadSettings();
  const hasSeed = seedFromQuery();
  if (hasSeed) {
    el('app').classList.remove('hidden');
    try {
      await startGame();
    } catch (e) {
      el('app').classList.add('hidden');
      await showLobby();
      el('lobby-note').textContent = `启动失败：${e.message}`;
      console.error(e);
    }
  } else {
    await showLobby();
  }
}

init().catch((e) => {
  console.error(e);
  const box = el('lobby-note');
  if (box) box.textContent = `启动失败：${e.message}`;
});
