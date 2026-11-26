// 掼蛋 M1 本地房间 · 表现层
// 牌面 id 编码与引擎一致：`(suit × 13 + rank) × 2 + copy`，suit 0..3 = ♠♥♦♣，rank 0..12 = 2..A；
// 王 id 从 104 起：104/105 小王，106/107 大王（两副牌）。
// 点数标签表，自然序 2→A。**全文件只有这一份点数表**：牌编码的点数段、引擎
// `round.level`（`levelTable.ranks` 的档位下标）都直接拿它当下标取标签；
// 只有跨进主题（deckart `Rank` 编号 1=A、2=2…13=K）时才由 `rankCode()` 转一次。
const RANKS = ['2', '3', '4', '5', '6', '7', '8', '9', '10', 'J', 'Q', 'K', 'A'];
const SUITS = ['♠', '♥', '♦', '♣'];
const JOKER_BASE = 104;
// 横排手牌：一行最多几张，按屏宽取值，多了折行（27 张在 1600 宽上正好 14 + 13）
function fanPerRow() {
  const w = window.innerWidth;
  if (w < 700) return 7;
  if (w < 1100) return 10;
  return 14;
}

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

// 手牌列序：大王 > 小王 > 级牌 > A > K > … > 3 > 2（级牌从自然序里抽出，与引擎的合成序一致）。
function levelRankIndex() {
  try {
    const lv = room.view(state.humanSeat)['round.level'];
    if (typeof lv === 'number' && lv >= 0 && lv <= 12) return lv;
  } catch { /* 视图不可用时按无级牌处理 */ }
  return -1;
}

/** 本局级牌的点数标签（`'2'`…`'A'`）；无级牌（score-only / 视图缺字段）返回 `null`。 */
function levelRankLabel() {
  const idx = levelRankIndex();
  return idx < 0 ? null : RANKS[idx];
}

/** 点数标签 → deckart `Rank` 编号（1=A、2=2……13=K）；`null` → `0`（不带级牌态）。
 *  只给 `gf_theme_card` 用，业务逻辑一律用标签，不碰这套编号。 */
function rankCode(label) {
  if (!label) return 0;
  if (label === 'A') return 1;
  if (label === 'J') return 11;
  if (label === 'Q') return 12;
  if (label === 'K') return 13;
  return Number(label); // '2'…'10'
}

function rankIndexOf(id) {
  if (id >= JOKER_BASE) return -1;
  return Math.floor(id / 2) % 13;
}

function suitIndexOf(id) {
  if (id >= JOKER_BASE) return id < JOKER_BASE + 2 ? 4 : 5;
  return Math.floor(Math.floor(id / 2) / 13);
}

// 列键越大越靠左；王 1000/999，级牌 998，其余按「去掉级牌后的自然序位次」100+pos。
function columnKey(id, levelRank) {
  if (id >= JOKER_BASE) return id >= JOKER_BASE + 2 ? 1000 : 999;
  const r = rankIndexOf(id);
  if (RANKS[r] === levelRank) return 998;
  let pos = 0;
  for (let i = 0; i < 13; i += 1) {
    if (RANKS[i] === levelRank) continue;
    if (i === r) return 100 + pos;
    pos += 1;
  }
  return 100;
}

function levelLabel(idx) {
  if (typeof idx !== 'number' || idx < 0) return '—';
  return RANKS[idx] ?? `${idx}`;
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

// ---------- 牌面主题（gf-deckart 在 wasm 里现场合成） ----------
//
// `.gfdeck` 只装 theme.json + 23 个分层素材；这里解密后逐张要 SVG，与 CLI 输出逐字节一致。
// 加载失败（包损坏 / 签名不符）一律回退到原来的字卡渲染，牌桌不阻塞。

const THEME_PACK_URL = 'themes/defaults.gfdeck';
// 开发期签名公钥（私钥种子全 0x11）；正式分发换成后台下发的公钥。
const THEME_PUBKEY = 'd04ab232742bb4ab3a1368bd4615e4e6d0224ab71a016baf8520a332c9778737';
const CODE_SUIT = ['S', 'H', 'D', 'C'];
const CODE_RANK = ['2', '3', '4', '5', '6', '7', '8', '9', '10', 'J', 'Q', 'K', 'A'];

function faceCode(id) {
  if (id >= JOKER_BASE) return id >= JOKER_BASE + 2 ? 'JOKER-B' : 'JOKER-S';
  const base = Math.floor(id / 2);
  return CODE_SUIT[Math.floor(base / 13)] + CODE_RANK[base % 13];
}

function hexToBytes(hex) {
  const out = new Uint8Array(hex.length / 2);
  for (let i = 0; i < out.length; i += 1) out[i] = parseInt(hex.slice(i * 2, i * 2 + 2), 16);
  return out;
}

function withBytes(bytes, fn) {
  const ptr = ex.gf_alloc(bytes.length);
  new Uint8Array(ex.memory.buffer, ptr, bytes.length).set(bytes);
  try {
    return fn(ptr, bytes.length);
  } finally {
    ex.gf_mem_free(ptr, bytes.length);
  }
}

const theme = {
  loaded: false,
  meta: null,
  cache: new Map(),
  stats: { cards: 0, ms: 0 },

  async load() {
    if (this.loaded || !ex || !ex.gf_theme_load) return this.loaded;
    try {
      const buf = new Uint8Array(await (await fetch(THEME_PACK_URL)).arrayBuffer());
      const pub = hexToBytes(THEME_PUBKEY);
      const r = JSON.parse(withBytes(buf, (ptr, len) => withBytes(pub, (pp, plen) => cstr(ex.gf_theme_load(ptr, len, pp, plen)))));
      if (!r.ok) {
        log(`主题加载失败：${r.error}（回退字卡）`);
        return false;
      }
      this.meta = r;
      this.loaded = true;
      log(`主题 ${r.themeId}@${r.version}「${r.name}」已载入：${r.bytes} 字节、${r.layout}、预览 ${r.preview.length} 张`);
      return true;
    } catch (e) {
      log(`主题加载异常：${e.message}（回退字卡）`);
      return false;
    }
  },

  /** 单张牌面 SVG。`levelRank` 用点数标签（`'2'`…`'A'`，`null` = 不带级牌态），
   *  真正进 wasm 时才由 `rankCode()` 换成主题的 `Rank` 编号；缓存键也用标签。
   *  `cornerPct` 是角标字号百分比，由 deckart 在牌面内部放大点数与花色（不是整张图放大）。 */
  svg(id, levelRank, cornerPct = state.cornerPct) {
    if (!this.loaded) return null;
    // 0 = 主题声明的默认倍率（由 deckart 在牌面内部换算），其余走客户端的合法区间
    const pct = cornerPct === 0 ? 0 : clampCornerPct(cornerPct);
    const key = `${id}@${levelRank ?? '-'}@${pct}`;
    if (this.cache.has(key)) return this.cache.get(key);
    const code = new TextEncoder().encode(faceCode(id));
    const t0 = performance.now();
    const level = rankCode(levelRank);
    const r = JSON.parse(withBytes(code, (ptr, len) => cstr(ex.gf_theme_card(ptr, len, level, pct))));
    this.stats.cards += 1;
    this.stats.ms += performance.now() - t0;
    const svg = r.ok ? r.svg : null;
    if (!r.ok) this.lastError = r.error;
    this.cache.set(key, svg);
    return svg;
  },
};

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
  // 当前行动座位的合法动作集（wasm 新增导出；旧产物没有时退化为空表）。
  legal() {
    if (!ex.gf_room_legal) return [];
    return JSON.parse(cstr(ex.gf_room_legal()));
  },
  // 桌面公开信息：相位 / 行动座位 / 连续过牌数 / 双方级牌 / 打级队 / 各家手牌与名次。
  info() {
    if (!ex.gf_room_info) return { phase: 'idle', seats: [], levels: [] };
    return JSON.parse(cstr(ex.gf_room_info()));
  },
  // 预检这组牌能否出（与 submit 同口径）；旧产物没有该导出时返回 null，交给清单兜底。
  canPlay(cards) {
    if (!ex.gf_room_can_play) return null;
    return withCards(cards, (ptr, len) => JSON.parse(cstr(ex.gf_room_can_play(ptr, len))));
  },
  // 一手牌的推荐拆分（`recommend`）：bot 出牌与「组牌建议」共用；旧产物没有该导出时返回 null。
  plan() {
    if (!ex.gf_room_plan) return null;
    try {
      return JSON.parse(cstr(ex.gf_room_plan()));
    } catch {
      return null;
    }
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
  hintIdx: 0,
  revealLast: false,
  legalCache: null,
  legalSeq: 0,
  // 手理牌分组：{ cards, patternId, layout: 'left'|'right', suit? }。
  // 只是布局，不动手牌本身；出牌 / 合法性仍走 selected + canPlay。
  arranged: [],
  // 每个花色当前展示到第几个同花顺窗口（多点几次轮换方案）。
  flushCursor: {},
  previewCache: null,
  // 牌面皮肤：true = 用主题包 SVG（wasm 现场合成），false = 原来的 DOM 字卡
  themeSkin: false,
  // 手牌布局：cols（同点数一列纵向叠放，现状）| fan（横排紧叠，露左上角标）
  handStyle: 'cols',
  // 角标字号（百分比）：牌面内部放大点数与花色，100 = 主题原样。
  // 手牌横排时每张只露一条边，靠它把点数顶到能读的大小；设置面板可调。
  // 220 只是主题包没声明 `cornerScale` 时的兜底值，包里有就按包的来（见 startGame）。
  cornerPct: 220,
  // 滑块/URL 有没有被用户显式指定过；没指定时才允许主题包的默认值覆盖上面那个兜底
  cornerPinned: false,
};

/** 角标字号的合法区间（和 deckart 的 CORNER_SCALE_MIN/MAX 对齐，单位百分比）。 */
const CORNER_PCT_MIN = 100;
const CORNER_PCT_MAX = 300;

function clampCornerPct(v) {
  const n = Math.round(Number(v));
  if (!Number.isFinite(n)) return 220;
  return Math.min(CORNER_PCT_MAX, Math.max(CORNER_PCT_MIN, n));
}

/** 把当前角标字号同步到设置面板的滑块与数值（初始化顺序：loadSettings 先跑，URL 覆盖在后）。 */
function syncCornerControl() {
  const slider = el('set-corner');
  if (!slider) return;
  slider.value = String(state.cornerPct);
  el('set-corner-val').textContent = `${state.cornerPct}%`;
}

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
  // 实验开关（只走查询参数，不进设置面板）：?theme=1 用主题 SVG 牌面；?hand=fan 手牌横排紧叠
  state.themeSkin = q.get('theme') === '1';
  // 主题牌面只能配横排：44×62 的纵列叠放会把整张牌缩到角标只有 5px（已截图验证糊成白带）。
  // 所以开了主题又没显式给 hand 参数时，默认走 fan。
  const handParam = q.get('hand');
  state.handStyle = handParam === 'fan' || (handParam === null && state.themeSkin) ? 'fan' : 'cols';
  // ?corner=260 覆盖角标字号（好处是截图/联调能给死值，不受本机 localStorage 影响）
  const cornerParam = q.get('corner');
  const storedCorner = localStorage.getItem('guandan-corner-pct');
  state.cornerPinned = cornerParam !== null || storedCorner !== null;
  state.cornerPct = clampCornerPct(cornerParam ?? storedCorner ?? state.cornerPct);
  syncCornerControl();
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

const RANK_NAMES = ['', '头游', '二游', '三游', '末游'];

// 牌型 id → 中文名（出牌区与日志共用这一套）
const PATTERN_NAMES = {
  single: '单张',
  pair: '对子',
  triple: '三张',
  'triple-pair': '三带二',
  'straight-5': '顺子',
  'plate-3': '三连对',
  'steel-2': '钢板',
  'bomb-4': '四炸',
  'bomb-5': '五炸',
  'bomb-6': '六炸',
  'bomb-7': '七炸',
  'bomb-8': '八炸',
  'straight-flush-5': '同花顺',
  'joker-bomb': '王炸',
};

function patternName(id) {
  return PATTERN_NAMES[id] ?? id;
}

function renderSeats() {
  const seats = room.seats();
  const info = room.info();
  const bySeat = new Map((info.seats ?? []).map((x) => [x.seat, x]));
  for (const s of seats) {
    const box = el(`seat-slot-${relSlot(s.seat)}`);
    if (!box) continue;
    const mine = s.seat === state.humanSeat;
    const ally = s.seat % 2 === state.humanSeat % 2;
    const active = state.waitingSeat === s.seat;
    const si = bySeat.get(s.seat) ?? { hand: 0, rank: 0 };
    const rankName = RANK_NAMES[si.rank] ?? '';
    const last = state.lastBySeat[s.seat];
    box.className = `seat slot-${['bottom', 'right', 'top', 'left'][relSlot(s.seat)]}` + (active ? ' active' : '');
    let tag;
    if (active) tag = '<span class="tag gold">行动中</span>';
    else if (rankName) tag = `<span class="tag rank">${rankName}</span>`;
    else tag = `<span class="tag">${mine ? '本家' : ally ? '我方' : '对方'}</span>`;
    const body = `${mine ? '本家' : ally ? '我方' : '对方'} · 手牌 ${si.hand} 张` + (!mine && last ? `<br/>最近：${last}` : '');
    box.innerHTML = `
      <div class="seat-head">
        <span>座位 ${s.seat} ${mine ? '· 你' : s.human ? '· 玩家' : '· AI'}</span>
        ${tag}
      </div>
      <div class="seat-body">${body}</div>
    `;
  }
}

function renderScoreboard() {
  const info = room.info();
  const levels = info.levels ?? [];
  const myTeam = state.humanSeat % 2;
  const attack = info.attackingTeam;
  const turnTeam = Number.isFinite(info.turnSeat) ? info.turnSeat % 2 : -1;
  const mine = levels[myTeam];
  const theirs = levels[1 - myTeam];
  const side = (team, label, level) => {
    const cls = [
      'score-side',
      attack === team ? 'attack' : '',
      turnTeam === team ? 'turn' : '',
    ].filter(Boolean).join(' ');
    const hint = [attack === team ? '打级方' : '', turnTeam === team ? '当前行动' : '']
      .filter(Boolean).join(' · ');
    return `<div class="${cls}" title="${hint}">${turnTeam === team ? '▶ ' : ''}${label} <b>${levelLabel(level)}</b></div>`;
  };
  el('scoreboard').innerHTML = side(myTeam, '我方', mine) + side(1 - myTeam, '对方', theirs);
}

function renderPlayZone() {
  const v = room.view(state.humanSeat);
  const info = room.info();
  const zone = el('play-zone');
  const box = el('last-play');
  const who = el('play-who');
  // 出牌区停在「出牌那人」桌前。座次在页面上是 0 上 / 1 左 / 2 下 / 3 右，
  // 相对自己即 diff=0 下、1 右、2 上、3 左，正好对上 CSS 的 data-rel。
  // 还没人出牌时退回当前行动家（领出前）。
  const last = v.lastPlay;
  const anchor = Number(last?.seat ?? info.turnSeat ?? state.humanSeat);
  zone.dataset.rel = String((anchor - state.humanSeat + 4) % 4);
  if (!last || !last.cards || last.cards.length === 0) {
    who.textContent = '本回合无人出牌';
    box.innerHTML = '';
    box.onclick = null;
    return;
  }
  const mine = last.seat === state.humanSeat;
  const passes = Number(info.passesSince ?? 0);
  const folded = passes >= 2 && !state.revealLast;
  const relSeat = (last.seat - state.humanSeat + 4) % 4;
  // 用页面方位称呼（1 右 / 2 对 / 3 左），避免「上家/下家」因出牌方向理解歧义。
  const whoName = mine ? '你' : (relSeat === 1 ? '右家' : relSeat === 3 ? '左家' : '对家');
  who.textContent = `${whoName}(${last.seat}) 出 ${patternName(last.patternId)}${passes > 0 ? ` · 已过${passes}家` : ''}`;
  if (folded) {
    box.innerHTML = `<span class="fold-hint">已过两家，牌面收起（点此展开）</span>`;
    box.onclick = () => { state.revealLast = true; renderPlayZone(); };
    return;
  }
  box.onclick = null;
  const cards = [...last.cards].sort((a, b) => columnKey(b, levelRankLabel()) - columnKey(a, levelRankLabel())
    || suitIndexOf(a) - suitIndexOf(b));
  box.innerHTML = cards.map((id) => {
    const svg = state.themeSkin ? theme.svg(id, levelRankLabel()) : null;
    if (svg) return `<span class="mini-card themed">${svg}</span>`;
    const f = cardFace(id);
    return `<span class="mini-card${f.red ? ' red' : ''}">${f.rank}${f.suit}</span>`;
  }).join('');
}

function renderCenter() {
  const info = room.info();
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

  const extra = el('table-extra');
  if (state.notice) {
    extra.textContent = state.notice;
    extra.style.color = '#ffb0b0';
  } else {
    const team = (state.humanSeat + 2) % 4;
    extra.textContent = `你座位 ${state.humanSeat} · 队友座位 ${team} · 第 ${info.roundNo ?? '?'} 局`;
    extra.style.color = '';
  }
}

// 理牌分组只改布局：手牌本身、选中集、合法判定都不受影响。
// 炸弹 / 同花顺这类「一眼要能认出来」的牌型理到最左，其余理到最右。
const SPECIAL_PATTERN = (pid) => pid === 'joker-bomb' || pid.startsWith('bomb-') || pid.startsWith('straight-flush');

function cardButton(id, index, myTurn) {
  const f = cardFace(id);
  const b = document.createElement('button');
  b.className = `card${f.red ? ' red' : ''}${f.joker ? ' joker' : ''}${state.selected.has(id) ? ' picked' : ''}${myTurn ? '' : ' dim'}`;
  b.style.zIndex = String(index + 1);
  b.dataset.cid = String(id);
  const svg = state.themeSkin ? theme.svg(id, levelRankLabel()) : null;
  if (svg) {
    // 整张牌面交给主题：角标、花阵、级牌胶囊、逢人配都在 SVG 里
    b.classList.add('themed');
    // 王没有「角标」这一层：素材是整幅图案，横排叠放时只见左边一条，
    // 大小王分不出来。补一个跟随角标字号的文字角标（小王 / 大王）。
    const tag = f.joker
      ? `<span class="corner joker-tag" style="font-size:${Math.round(9 * state.cornerPct / 100)}px"><b>${f.rank}</b><span>${f.suit}</span></span>`
      : '';
    b.innerHTML = `${svg}${tag}${state.showIds ? `<span class="cid">${id}</span>` : ''}`;
  } else {
    b.innerHTML = `<span class="corner"><b>${f.rank}</b><span class="suit">${f.suit}</span></span>${state.showIds ? `<span class="cid">${id}</span>` : ''}`;
  }
  return b;
}

function sortedCards(list) {
  return [...list].sort((a, b) => columnKey(b, levelRankLabel()) - columnKey(a, levelRankLabel())
    || suitIndexOf(a) - suitIndexOf(b) || (a % 2) - (b % 2));
}

function pushColumn(box, ids, myTurn, grouped) {
  const col = document.createElement('div');
  col.className = `hand-col${grouped ? ` grouped group-${grouped.side}` : ''}`;
  if (grouped) {
    col.dataset.group = String(grouped.index);
    col.dataset.cards = ids.join(',');
    col.dataset.pattern = grouped.patternId ?? '';
  }
  sortedCards(ids).forEach((id, i) => col.appendChild(cardButton(id, i, myTurn)));
  box.appendChild(col);
}

function renderHand() {
  const v = room.view(state.humanSeat);
  const cards = [...(v.hand.cards ?? [])];
  const levelRank = levelRankLabel();
  const box = el('hand');
  box.innerHTML = '';
  const myTurn = state.waitingSeat === state.humanSeat;

  // 出完 / 被换局后，理牌组里可能有已经不存在的牌：先剪掉。
  const handSet = new Set(cards);
  state.arranged = state.arranged
    .map((g) => ({ ...g, cards: g.cards.filter((id) => handSet.has(id)) }))
    .filter((g) => g.cards.length > 0);
  const groupedIds = new Set(state.arranged.flatMap((g) => g.cards));

  const left = state.arranged.filter((g) => g.layout === 'left');
  const right = state.arranged.filter((g) => g.layout === 'right');
  const fan = state.handStyle === 'fan';
  box.classList.toggle('fan', fan);
  if (fan) {
    // 横排紧叠：左→右按牌力降序，理牌组仍在两端（只是不再单独框列）。
    // 牌大一号后一行放不下 27 张：按最多 13 张一行折成 1~3 行（手机上也不会挤成一条白带）。
    const ordered = [
      ...left.flatMap((g) => sortedCards(g.cards)),
      ...sortedCards(cards.filter((id) => !groupedIds.has(id))),
      ...right.flatMap((g) => sortedCards(g.cards)),
    ];
    const rows = Math.min(2, Math.max(1, Math.ceil(ordered.length / fanPerRow())));
    const per = Math.ceil(ordered.length / rows);
    let idx = 0;
    for (let r = 0; r < rows; r += 1) {
      const row = document.createElement('div');
      row.className = 'hand-col fan-row';
      for (const id of ordered.slice(r * per, (r + 1) * per)) {
        row.appendChild(cardButton(id, idx, myTurn));
        idx += 1;
      }
      if (row.childElementCount > 0) box.appendChild(row);
    }
  } else {
    left.forEach((g) => pushColumn(box, g.cards, myTurn, { side: 'left', index: state.arranged.indexOf(g), patternId: g.patternId }));

    // 未理的牌仍按同点数一列、列键降序（大王、小王、级牌、A、K…2）。
    const cols = new Map();
    for (const id of cards) {
      if (groupedIds.has(id)) continue;
      const key = columnKey(id, levelRank);
      if (!cols.has(key)) cols.set(key, []);
      cols.get(key).push(id);
    }
    for (const key of [...cols.keys()].sort((a, b) => b - a)) {
      pushColumn(box, cols.get(key), myTurn, null);
    }

    right.forEach((g) => pushColumn(box, g.cards, myTurn, { side: 'right', index: state.arranged.indexOf(g), patternId: g.patternId }));
  }

  el('hand-count').textContent = `手牌 ${cards.length} 张`;
  syncSuitTabs(cards, groupedIds);
}

// ---------- 理牌：只调布局，不动手牌 / 选中 / 合法判定 ----------

const SUIT_NAMES = ['♠', '♥', '♦', '♣'];

function flash(msg) {
  state.notice = msg;
  renderCenter();
  clearTimeout(state.noticeTimer);
  state.noticeTimer = setTimeout(() => { state.notice = ''; renderCenter(); }, 2600);
}

// 该花色手牌里所有「5 张连续」的窗口（王不参与；级牌转换 / 百搭不在此展开）。
function flushWindows(suit, ids) {
  const byRank = new Map();
  for (const id of ids) {
    if (id >= 100) continue;
    if (Math.floor(id / 26) !== suit) continue;
    const rank = Math.floor(id / 2) % 13;
    if (!byRank.has(rank)) byRank.set(rank, []);
    byRank.get(rank).push(id);
  }
  const out = [];
  for (let start = 0; start + 4 < 13; start += 1) {
    const cards = [];
    for (let d = 0; d < 5; d += 1) {
      const list = byRank.get(start + d);
      if (!list || list.length === 0) { cards.length = 0; break; }
      cards.push(list[0]);
    }
    if (cards.length === 5) out.push({ start, cards });
  }
  return out;
}

// 该花色「可用的牌池」：排除其它理牌组，但保留自己这一组（否则高亮会在切换时消失）。
function poolForSuit(cards, suit) {
  const others = new Set(
    state.arranged.filter((g) => g.suit !== suit).flatMap((g) => g.cards),
  );
  return cards.filter((id) => !others.has(id));
}

// 四个花色按钮：能凑出同花顺就高亮，已经理出来的那套再压重一档。
function syncSuitTabs(cards, groupedIds) {
  const myTurn = state.waitingSeat === state.humanSeat;
  for (const btn of document.querySelectorAll('.suit-tabs .suit')) {
    const suit = Number(btn.dataset.suit);
    const opts = flushWindows(suit, poolForSuit(cards, suit));
    btn.classList.toggle('hot', opts.length > 0);
    btn.classList.toggle('active', state.arranged.some((g) => g.suit === suit));
    btn.disabled = !myTurn || opts.length === 0;
    btn.title = opts.length
      ? `${SUIT_NAMES[suit]} 可凑 ${opts.length} 种同花顺（可反复点击切换）`
      : `${SUIT_NAMES[suit]} 凑不出同花顺`;
  }
}

// 点花色：在「该花色所有同花顺窗口」里循环切换，只保留最后选中的那套。
function cycleFlush(suit) {
  if (state.waitingSeat !== state.humanSeat) {
    flash('轮到你时才能理同花顺');
    return;
  }
  const cards = [...(room.view(state.humanSeat).hand.cards ?? [])];
  const opts = flushWindows(suit, poolForSuit(cards, suit));
  if (opts.length === 0) {
    flash(`${SUIT_NAMES[suit]} 凑不出同花顺`);
    return;
  }
  const cur = state.flushCursor?.[suit] ?? -1;
  const idx = (cur + 1) % opts.length;
  state.flushCursor = { ...(state.flushCursor ?? {}), [suit]: idx };
  const pick = opts[idx];
  state.arranged = state.arranged.filter((g) => g.suit !== suit);
  state.arranged.push({
    cards: pick.cards.slice(),
    patternId: 'straight-flush-5',
    layout: 'left',
    suit,
  });
  state.selected.clear();
  renderHand();
  renderActions();
  flash(`${SUIT_NAMES[suit]} 同花顺方案 ${idx + 1}/${opts.length}`);
}

// 「理成一列」：只认牌型，不看压不压得住；炸弹 / 同花理到最左，其余理到最右。
function arrangeSelection() {
  if (state.waitingSeat !== state.humanSeat) {
    flash('轮到你时才能理牌');
    return;
  }
  if (state.selected.size === 0) {
    flash('先选中要理的牌型');
    return;
  }
  const cards = [...state.selected].sort((a, b) => a - b);
  const pre = room.canPlay(cards);
  const pid = pre?.patternId ?? '';
  if (!pid) {
    flash('选中的牌凑不成牌型，理不了');
    return;
  }
  const suit = pid.startsWith('straight-flush') ? suitIndexOf(cards[0]) : null;
  state.arranged = state.arranged.filter((g) => g.cards.slice().sort((a, b) => a - b).join(',') !== cards.join(','));
  state.arranged.push({ cards, patternId: pid, layout: SPECIAL_PATTERN(pid) ? 'left' : 'right', suit });
  state.selected.clear();
  renderHand();
  renderActions();
  const beat = pre.ok ? '' : '（当前压不过上家）';
  flash(`${patternName(pid)} 已理到${SPECIAL_PATTERN(pid) ? '最左' : '最右'}一列${beat}`);
}

// 「组牌建议」：直接用 recommend 的最优整手拆分铺开——炸弹/同花顺进左列、其余进右列，
// 这样点一组就是整组出，不用自己拼牌。
function arrangeSuggest() {
  if (state.waitingSeat !== state.humanSeat) {
    flash('轮到你时才能看组牌建议');
    return;
  }
  const plan = room.plan();
  if (!plan || !Array.isArray(plan.groups) || plan.groups.length === 0) {
    flash('暂时拿不到组牌建议（本次拆分没有可出的组）');
    return;
  }
  const groups = plan.groups
    .filter((g) => g.cards?.length)
    .map((g) => ({
      cards: g.cards.slice(),
      patternId: g.patternId ?? '',
      layout: SPECIAL_PATTERN(g.patternId) ? 'left' : 'right',
      suit: g.patternId?.startsWith('straight-flush') ? suitIndexOf(g.cards[0]) : null,
    }));
  state.arranged = groups;
  state.flushCursor = {};
  state.selected.clear();
  renderHand();
  renderActions();
  const left = groups.filter((g) => g.layout === 'left').length;
  const right = groups.length - left;
  flash(`组牌建议（最优拆分）：${groups.length} 组 · 左 ${left} / 右 ${right}`);
}

// 「回复理牌」：选中的牌正好是某一组时只恢复那一组；否则恢复全部理牌。
function restoreArrange() {
  if (state.arranged.length === 0) {
    flash('没有可恢复的理牌');
    return;
  }
  const sel = [...state.selected].sort((a, b) => a - b).join(',');
  const hit = sel
    ? state.arranged.findIndex((g) => g.cards.slice().sort((a, b) => a - b).join(',') === sel)
    : -1;
  if (hit >= 0) {
    const [g] = state.arranged.splice(hit, 1);
    state.selected.clear();
    flash(`已恢复 ${patternName(g.patternId)}`);
  } else {
    state.arranged = [];
    state.flushCursor = {};
    flash('已恢复全部理牌');
  }
  renderHand();
  renderActions();
}

function renderHeader() {
  const m = room.meta();
  el('match-id').textContent = state.matchId;
  el('meta').textContent = `第 ${m.roundNo} 局 · roomSeq ${m.roomSeq} · 存档 ${state.recorded} 条 · bot L${state.level}`;
  el('hash').textContent = room.hash();
}

// 合法动作集（按 roomSeq + 行动座位缓存，一次渲染只问 wasm 一次）。
function currentLegal() {
  if (state.waitingSeat !== state.humanSeat) return [];
  const key = `${state.legalSeq}|${state.waitingSeat}|${state.phase}`;
  if (state.legalCache && state.legalCache.key === key) return state.legalCache.list;
  let list = [];
  try {
    list = room.legal();
  } catch { /* wasm 不可用时按空表 */ }
  state.legalCache = { key, list };
  return list;
}

// 选中这组牌能不能出：先用 wasm 的 preview_play 预检（与 submit 同口径，覆盖顺子/三带二
// 这类多组合牌型），旧产物没有该导出时退化为「在候选清单里找同一组牌」。
// 返回 {ok, pattern}，按 roomSeq + 选牌集缓存，避免每次重绘都跨边界调用。
function previewSelection() {
  if (state.selected.size === 0) return { ok: false, pattern: '' };
  const cards = [...state.selected].sort((a, b) => a - b);
  const key = `${state.legalSeq}|${state.waitingSeat}|${cards.join(',')}`;
  if (state.previewCache && state.previewCache.key === key) return state.previewCache;
  let out = null;
  try {
    const r = room.canPlay(cards);
    if (r) out = { ok: !!r.ok, pattern: r.patternId ? patternName(r.patternId) : '' };
  } catch { /* wasm 不可用时走清单兜底 */ }
  if (!out) {
    const target = cards.join(',');
    const hit = currentLegal().find((a) => !a.pass && (a.cards ?? []).slice().sort((x, y) => x - y).join(',') === target);
    out = { ok: !!hit, pattern: '' };
  }
  state.previewCache = { key, ...out };
  return state.previewCache;
}

function selectedPlayIsLegal() {
  return previewSelection().ok;
}

function renderActions() {
  const myTurn = state.waitingSeat === state.humanSeat;
  const legal = myTurn ? currentLegal() : [];
  const plays = legal.filter((a) => !a.pass);
  const canPass = myTurn && legal.some((a) => a.pass);
  const pickLegal = selectedPlayIsLegal();
  const pickPattern = pickLegal ? previewSelection().pattern : '';
  // 动作行只在轮到自己时露出来，压在牌型行上方。
  el('turn-actions').classList.toggle('hidden', !myTurn);
  el('play').disabled = !myTurn || !pickLegal;
  el('hint').disabled = !myTurn || plays.length === 0;
  el('pass').disabled = !canPass;
  el('clear-select').disabled = state.selected.size === 0;
  el('suggest').disabled = !myTurn || plays.length === 0;
  el('arrange').disabled = !myTurn || state.selected.size === 0 || !pickLegal;
  el('restore').disabled = state.arranged.length === 0;
  const parts = [];
  if (state.selected.size === 0) parts.push('未选牌');
  else {
    parts.push(`已选 ${state.selected.size} 张：${[...state.selected]
      .sort((a, b) => columnKey(b, levelRankLabel()) - columnKey(a, levelRankLabel()))
      .map(cardLabel).join(' ')}`);
    parts.push(pickLegal ? `✔ 可出${pickPattern ? `（${pickPattern}）` : ''}` : '✘ 不合法');
  }
  if (myTurn) parts.push(plays.length === 0 ? (canPass ? '只剩过牌' : '要不起') : `可出 ${plays.length} 手`);
  el('selection').textContent = parts.join(' · ');
}

function renderAll() {
  try {
    state.legalSeq = room.meta().roomSeq;
  } catch { /* 未开局 */ }
  renderSeats();
  renderScoreboard();
  renderPlayZone();
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
    tr.innerHTML = `<td>${RANK_NAMES[i + 1] ?? i + 1}</td><td>座位 ${seat}${mine ? '（你）' : ''}</td><td>${ally ? '我方' : '对方'}</td>`;
    rows.appendChild(tr);
  });
  const winnerTeam = summary?.winnerTeam;
  el('settle-title').textContent = ended ? `场次结束 · 第 ${summary?.roundNo ?? '?'} 局` : `第 ${summary?.roundNo ?? '?'} 局结算`;
  el('settle-extra').innerHTML = [
    `名次：${ranks.map((s, i) => `${RANK_NAMES[i + 1] ?? i + 1}=座位${s}`).join('，') || '-'}`,
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
        if (s.seat !== state.waitingSeat) {
          state.hintIdx = 0;
          state.revealLast = false;
        }
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
  // 牌面主题：包损坏 / 签名不符时只回退字卡，不影响开局（日志清空后再载入，避免提示被清掉）
  if (state.themeSkin) {
    await theme.load();
    // 主题包可以在 theme.json 里声明默认角标倍率（cornerScale，1.0–3.0）。
    // 优先级：?corner= / localStorage（用户手动调过）> 主题包默认 > state.cornerPct 的兜底值。
    if (theme.loaded && !state.cornerPinned && theme.meta.cornerScale != null) {
      state.cornerPct = clampCornerPct(theme.meta.cornerScale * 100);
      syncCornerControl();
      log(`角标字号采用主题默认：${state.cornerPct}%（theme.json cornerScale=${theme.meta.cornerScale}）`);
    }
  }

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

function useHint() {
  if (state.waitingSeat !== state.humanSeat) return;
  const plays = currentLegal().filter((a) => !a.pass);
  if (plays.length === 0) return;
  const pick = plays[state.hintIdx % plays.length];
  state.hintIdx += 1;
  state.selected.clear();
  for (const c of pick.cards ?? []) state.selected.add(c);
  renderHand();
  renderActions();
}

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
  state.revealLast = false;
  state.hintIdx = 0;
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
    // 角标字号由 seedFromQuery() 定好（URL > localStorage > 默认），这里只把控件同步过去
    syncCornerControl();
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
el('hint').onclick = () => useHint();
el('pass').onclick = () => humanPlay(true);
el('clear-select').onclick = () => { state.selected.clear(); renderHand(); renderActions(); };
el('reload').onclick = () => location.reload();
el('toggle-log').onclick = () => {
  state.showLog = !state.showLog;
  el('side').classList.toggle('hidden', !state.showLog);
};
el('open-settings').onclick = () => el('settings').classList.remove('hidden');
// 角标字号：拖一下就地重算牌面（wasm 单张 0.015ms，一手 27 张也不到 1ms）
el('set-corner').oninput = (e) => {
  state.cornerPct = clampCornerPct(e.target.value);
  state.cornerPinned = true;
  el('set-corner-val').textContent = `${state.cornerPct}%`;
  localStorage.setItem('guandan-corner-pct', String(state.cornerPct));
  if (state.themeSkin) theme.cache.clear();
  renderPlayZone();
  renderCenter();
  renderHand();
};
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

// ---------- 选牌：点选 + 按住滑动连选 ----------
//
// 只改选中状态对应的 DOM 类与按钮状态，不重建手牌——重建会让 pointermove 打在已失效的
// 元素上，拖选会断。起点是「已选」时整段按取消处理，反之整段按选中处理。

function setPicked(id, on) {
  if (state.selected.has(id) === on) return;
  if (on) state.selected.add(id);
  else state.selected.delete(id);
  const card = el('hand').querySelector(`.card[data-cid="${id}"]`);
  if (card) card.classList.toggle('picked', on);
  renderActions();
}

let dragPick = null;
el('hand').addEventListener('pointerdown', (e) => {
  if (state.waitingSeat !== state.humanSeat) return;
  // 理牌列：整组一起选中 / 取消，方便再理或只恢复这一组。
  const grouped = e.target.closest('.hand-col.grouped');
  if (grouped) {
    const g = state.arranged[Number(grouped.dataset.group)];
    if (!g) return;
    const all = g.cards.every((id) => state.selected.has(id)) && state.selected.size === g.cards.length;
    state.selected.clear();
    if (!all) g.cards.forEach((id) => state.selected.add(id));
    dragPick = null;
    renderHand();
    renderActions();
    return;
  }
  const card = e.target.closest('.card');
  if (!card) return;
  const id = Number(card.dataset.cid);
  dragPick = { want: !state.selected.has(id) };
  setPicked(id, dragPick.want);
});
el('hand').addEventListener('pointermove', (e) => {
  if (!dragPick) return;
  const hit = document.elementFromPoint(e.clientX, e.clientY);
  const card = hit && hit.closest('.card');
  if (!card) return;
  setPicked(Number(card.dataset.cid), dragPick.want);
});
window.addEventListener('pointerup', () => { dragPick = null; });
window.addEventListener('pointercancel', () => { dragPick = null; });

// ---------- 理牌条 ----------

el('suggest').onclick = arrangeSuggest;
el('arrange').onclick = arrangeSelection;
el('restore').onclick = restoreArrange;
for (const btn of document.querySelectorAll('.suit-tabs .suit')) {
  btn.onclick = () => cycleFlush(Number(btn.dataset.suit));
}

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

// 调试钩子：URL 带 debug=1 时可从控制台看内部状态（不影响正常路径）。
if (location.search.includes('debug')) {
  window.__gd = { state, room, currentLegal, previewSelection, theme, get ex() { return ex; } };
}


