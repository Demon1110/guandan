// 牌面 id → 文本。
// 编码与引擎一致：`(suit × 13 + rank) × 2 + copy`，suit 0..3 = ♠♥♦♣，rank 0..12 = 2..A；
// 王 id 从 104 起：104/105 小王，106/107 大王（两副牌）。
const RANKS = ['2', '3', '4', '5', '6', '7', '8', '9', '10', 'J', 'Q', 'K', 'A'];
const SUITS = ['♠', '♥', '♦', '♣'];
const JOKER_BASE = 104;

function cardLabel(id) {
  if (id >= JOKER_BASE) return id < JOKER_BASE + 2 ? '小王' : '大王';
  const base = Math.floor(id / 2);
  return RANKS[base % 13] + SUITS[Math.floor(base / 13)];
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
      if (!db.objectStoreNames.contains(STORE_MATCH)) {
        db.createObjectStore(STORE_MATCH, { keyPath: 'matchId' });
      }
      if (!db.objectStoreNames.contains(STORE_ACTION)) {
        db.createObjectStore(STORE_ACTION, { keyPath: ['matchId', 'seq'] });
      }
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

async function saveAction(db, matchId, rec) {
  await reqToPromise(tx(db, STORE_ACTION, 'readwrite').put({ matchId, ...rec }));
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
  for (const a of acts) {
    await reqToPromise(tx(db, STORE_ACTION, 'readwrite').delete([a.matchId, a.seq]));
  }
  await reqToPromise(tx(db, STORE_MATCH, 'readwrite').delete(matchId));
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
  create(seedHi, seedLo, humanMask) {
    if (ex.gf_room_new(seedHi, seedLo, humanMask) !== 1) throw new Error('建桌失败');
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
  seedLo: 0xa101,
  humanMask: 0b0001,
  humanSeat: 0,
  recorded: 0,
  selected: new Set(),
  waitingSeat: null,
  deadline: 0,
  running: false,
};

const el = (id) => document.getElementById(id);

function seedFromQuery() {
  const q = new URLSearchParams(location.search).get('seed');
  if (!q) return;
  const v = q.startsWith('0x') ? BigInt(q) : BigInt('0x' + q);
  const mask = (1n << 64n) - 1n;
  const u = v & mask;
  state.seedHi = Number(u >> 32n);
  state.seedLo = Number(u & 0xffffffffn);
}

function matchId() {
  return `web-${state.seedHi.toString(16)}${state.seedLo.toString(16).padStart(8, '0')}`;
}

function renderSeats() {
  const seats = room.seats();
  const box = el('seats');
  box.innerHTML = '';
  for (const s of seats) {
    const v = room.view(s.seat);
    const mine = s.seat === state.humanSeat;
    const cards = mine ? v.hand.cards : null;
    const cnt = mine ? cards.length : null;
    const node = document.createElement('div');
    node.className = 'seat' + (state.waitingSeat === s.seat ? ' active' : '');
    node.innerHTML = `
      <div class="seat-head">座位 ${s.seat} · ${s.human ? '你' : 'bot'}</div>
      <div class="seat-body">${mine ? `<span class="muted">剩 ${cards.length} 张</span>` : `<span class="muted">${cntLabel(v)}</span>`}</div>
    `;
    box.appendChild(node);
  }
}

// 非本人座位看不到手牌张数（视图只给自己手牌）；用 discard 与总牌数推不出，这里直接显示座位等级。
function cntLabel(v) {
  const lv = v['seat.levels'] ? v['seat.levels'].map((x) => x).join('/') : '';
  return lv ? `等级 ${lv}` : '';
}

function renderTable() {
  const v = room.view(state.humanSeat);
  const last = v.lastPlay;
  el('last-play').textContent = last
    ? `座位 ${last.seat} 出 ${last.cards.map(cardLabel).join(' ')}（${last.patternId}）`
    : '本回合无人出牌（轮到你领出）';

  const hand = el('hand');
  hand.innerHTML = '';
  for (const id of v.hand.cards) {
    const b = document.createElement('button');
    b.className = 'card' + (state.selected.has(id) ? ' picked' : '');
    b.textContent = cardLabel(id);
    b.onclick = () => {
      if (!state.waitingSeat) return;
      if (state.selected.has(id)) state.selected.delete(id);
      else state.selected.add(id);
      renderTable();
    };
    hand.appendChild(b);
  }

  const m = room.meta();
  el('meta').textContent = `第 ${m.roundNo} 局 · roomSeq ${m.roomSeq} · 存档 ${state.recorded} 条`;
  el('hash').textContent = room.hash();
  el('turn').textContent = state.waitingSeat === null
    ? '…'
    : (state.waitingSeat === state.humanSeat ? `轮到你（剩 ${Math.max(0, Math.round(state.deadline - now()))} ms）` : `等座位 ${state.waitingSeat} 行动`);
  el('play').disabled = state.waitingSeat !== state.humanSeat || state.selected.size === 0;
  el('pass').disabled = state.waitingSeat !== state.humanSeat;
}

function log(line) {
  const box = el('log');
  const li = document.createElement('li');
  li.textContent = line;
  box.prepend(li);
  while (box.children.length > 14) box.removeChild(box.lastChild);
}

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

async function pump() {
  if (state.running) return;
  state.running = true;
  try {
    for (let i = 0; i < 30; i += 1) {
      const s = room.step(now());
      if (s.kind === 'applied') {
        await record(toRecord(s));
        log(`座位 ${s.seat} ${s.action.pass ? '过牌' : '出 ' + (s.action.cards ?? []).map(cardLabel).join(' ')}${s.auto ? `（托管 ${s.auto}）` : ''}`);
        continue;
      }
      if (s.kind === 'settled') {
        const meta = room.meta();
        if (meta.records > state.recorded) {
          const last = room.lastRecord();
          if (last) {
            await record(last);
            log(`座位 ${last.seat} ${last.pass ? '过牌' : '出 ' + (last.cards ?? []).map(cardLabel).join(' ')}`);
          }
        }
        log(`第 ${s.summary.roundNo} 局结算：名次 ${s.summary.ranks.join(',')} 头游队 ${s.summary.winnerTeam}`);
        setTimeout(() => { room.nextRound(now()); pump(); }, 1200);
        return;
      }
      if (s.kind === 'matchEnd') {
        log(`场次结束：胜方 ${s.summary?.winnerTeam ?? '-'}`);
        el('turn').textContent = '场次已结束';
        return;
      }
      if (s.kind === 'waiting') {
        state.waitingSeat = s.seat;
        state.deadline = s.deadlineMs;
        renderTable();
        setTimeout(pump, 200);
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
  renderTable();
  renderSeats();
}

// ---------- 启动 / 续局 ----------

async function boot() {
  seedFromQuery();
  state.matchId = matchId();
  const bytes = await (await fetch('gf_client_wasm.wasm')).arrayBuffer();
  const module = await WebAssembly.compile(bytes);
  ex = (await WebAssembly.instantiate(module, {})).exports;
  state.db = await openDb();
  clockBase = performance.now();

  const saved = await loadMatch(state.db, state.matchId);
  const acts = await loadActions(state.db, state.matchId);

  room.create(state.seedHi, state.seedLo, state.humanMask);

  if (saved && acts.length > 0) {
    // 杀进程续局：不复用任何旧内存，只按 IndexedDB 里的动作日志重演。
    let currentRound = 1;
    for (const rec of acts) {
      while (rec.roundNo > currentRound) {
        if (!room.nextRound(0)) throw new Error('回放时续局失败');
        currentRound += 1;
      }
      if (!room.replay(rec)) throw new Error(`回放动作失败 seq=${rec.seq}`);
    }
    const h = room.hash();
    const same = h === saved.lastHash;
    log(`从 IndexedDB 恢复 ${acts.length} 条动作：${same ? '哈希一致 ✓' : `哈希不一致 ✗ 存档=${saved.lastHash} 恢复=${h}`}`);
    state.recorded = acts.length;
  } else {
    await saveMatch(state.db, { matchId: state.matchId, seedHi: state.seedHi, seedLo: state.seedLo, humanMask: state.humanMask, hash: '', updatedAt: Date.now() });
    log('新开一局（无存档）');
  }

  room.resumeWindow(now());
  el('match-id').textContent = state.matchId;
  renderSeats();
  renderTable();
  pump();
}

el('play').onclick = async () => {
  if (state.waitingSeat !== state.humanSeat || state.selected.size === 0) return;
  const cards = [...state.selected].sort((a, b) => a - b);
  const out = room.submit(state.humanSeat, now(), { pass: false, cards });
  if (out.kind !== 'applied') {
    log(`出牌被拒：${out.error ?? out.kind}`);
    return;
  }
  state.selected.clear();
  state.waitingSeat = null;
  await record(toRecord(out));
  log(`你出 ${cards.map(cardLabel).join(' ')}`);
  pump();
};

el('pass').onclick = async () => {
  if (state.waitingSeat !== state.humanSeat) return;
  const out = room.submit(state.humanSeat, now(), { pass: true, cards: [] });
  if (out.kind !== 'applied') {
    log(`过牌被拒：${out.error ?? out.kind}`);
    return;
  }
  state.selected.clear();
  state.waitingSeat = null;
  await record(toRecord(out));
  log('你过牌');
  pump();
};

el('reload').onclick = () => location.reload();

el('reset').onclick = async () => {
  await clearMatch(state.db, state.matchId);
  location.reload();
};

boot().catch((e) => {
  el('turn').textContent = `启动失败：${e.message}`;
  console.error(e);
});
