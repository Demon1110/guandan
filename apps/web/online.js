// 掼蛋联机大厅 · 独立可选入口
//
// 设计边界：
// - 不依赖 apps/web/app.js 的本地 wasm 房间；本页只走 `/api/*` 与 `/ws`。
// - 服务端仍是唯一权威，客户端只做「解帧 → 渲染 → 提交 → 断线重连」。
// - 协议按 §5.9 帧序手工编解码；新增 RematchReady/RematchState 已在这里接上。

const CLIENT_VERSION = '2.0.0';
const DEFAULT_BASE = 'http://127.0.0.1:8090';

const K = {
  JOIN_ROOM: 1,
  RECONNECT: 2,
  SUBMIT_ACTION: 3,
  HEARTBEAT: 4,
  REMATCH_READY: 5,
};

const S = {
  JOIN_ACK: 11,
  ROOM_SNAPSHOT: 12,
  ROOM_DELTA: 13,
  ACTION_ACK: 14,
  PHASE_CHANGE: 15,
  SETTLE_RESULT: 16,
  TRUSTEE_STATE: 17,
  REMATCH_STATE: 18,
};

const PHASES = ['发牌', '进贡', '出牌', '结算'];
const RANKS = ['2', '3', '4', '5', '6', '7', '8', '9', '10', 'J', 'Q', 'K', 'A'];
const SUITS = ['♠', '♥', '♦', '♣'];
const JOKER_BASE = 104;
const SESSION_KEY = 'gf.online.table';
const TOKEN_KEY = 'gf.online.token';
const REFRESH_KEY = 'gf.online.refresh';
const BASE_KEY = 'gf.online.base';

const $ = (id) => document.getElementById(id);
const textMap = (msg) => ({
  RS_SCHEMA_INVALID: '请求参数不合法',
  INVITE_CODE_UNKNOWN: '房间码不存在，检查大小写或问问房主',
  INVITE_CODE_EXPIRED: '房间码已过期，让房主重新发一个',
  TICKET_UNKNOWN: '匹配票不存在或已过期',
  RS_SEAT_TAKEN: '座位已被占，换一个入口再试',
  AUTH_INVALID: '登录状态失效，请重新登录',
  CLIENT_TOO_OLD: '客户端版本过低，请刷新页面或用新版客户端',
  GAME_DISABLED: '玩法已被管理员关闭',
  GAME_MODE_DISABLED: '该模式暂时关闭，换休闲/排位再试',
  RATE_LIMITED: '操作太频繁，稍等几秒',
  ACTION_ILLEGAL: '这一步不合法：牌型压不过上一手，或当前不该你出',
  ACCOUNT_DISABLED: '账号已被停用',
  STORAGE_UNAVAILABLE: '服务端存储暂不可用',
}[msg] || msg || '请求失败');

const state = {
  base: localStorage.getItem(BASE_KEY) || DEFAULT_BASE,
  token: localStorage.getItem(TOKEN_KEY) || '',
  refreshToken: localStorage.getItem(REFRESH_KEY) || '',
  matchId: null,
  seat: null,
  seatToken: '',
  seatSeq: 0,
  roomSeq: 0,
  roundNo: 0,
  phase: -1,
  turnSeat: null,
  hand: [],
  selected: new Set(),
  lastPlay: null,
  view: null,
  ws: null,
  manualClose: false,
  retryTimer: null,
  retryStartedAt: 0,
  heartbeatTimer: null,
  queueTimer: null,
  requestSeq: 0,
  code: '',
  connected: false,
};

// ---------- 基础 ----------

function baseUrl() {
  return state.base.replace(/\/+$/, '');
}

function wsUrl() {
  return baseUrl().replace(/^http/, 'ws') + '/ws';
}

function saveSession() {
  if (!state.matchId) return;
  localStorage.setItem(SESSION_KEY, JSON.stringify({
    base: baseUrl(),
    matchId: state.matchId,
    seat: state.seat,
    seatToken: state.seatToken,
    seatSeq: state.seatSeq,
    code: state.code,
  }));
}

function clearSession() {
  localStorage.removeItem(SESSION_KEY);
  state.matchId = null;
  state.seat = null;
  state.seatToken = '';
  state.seatSeq = 0;
  state.code = '';
}

function notice(msg, isError = false) {
  const el = $('notice');
  el.textContent = msg;
  el.classList.toggle('error', isError);
  el.classList.remove('hidden');
  clearTimeout(notice.timer);
  notice.timer = setTimeout(() => el.classList.add('hidden'), 6000);
}

async function refreshAccess() {
  if (!state.refreshToken) return false;
  let resp;
  try {
    resp = await fetch(baseUrl() + '/api/auth/refresh', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ refreshToken: state.refreshToken }),
    });
  } catch {
    return false;
  }
  if (!resp.ok) return false;
  const body = await resp.json().catch(() => null);
  if (!body?.token) return false;
  state.token = body.token;
  state.refreshToken = body.refreshToken || state.refreshToken;
  localStorage.setItem(TOKEN_KEY, state.token);
  localStorage.setItem(REFRESH_KEY, state.refreshToken);
  return true;
}

async function api(path, options = {}, retry = true) {
  const headers = { 'Content-Type': 'application/json', ...(options.headers || {}) };
  if (state.token) headers.Authorization = `Bearer ${state.token}`;
  let resp;
  try {
    resp = await fetch(baseUrl() + path, { ...options, headers });
  } catch (err) {
    throw new Error('连不上服务端：确认地址、端口和 CORS');
  }
  if (resp.status === 401 && retry && state.refreshToken && !path.startsWith('/api/auth/')) {
    if (await refreshAccess()) return api(path, options, false);
  }
  let body = null;
  const raw = await resp.text();
  try { body = raw ? JSON.parse(raw) : null; } catch { body = { raw }; }
  if (!resp.ok) {
    const reason = body?.reason || body?.detail || `HTTP ${resp.status}`;
    const err = new Error(textMap(reason));
    err.reason = reason;
    err.status = resp.status;
    throw err;
  }
  return body;
}

function setPill(id, text, tone = '') {
  const el = $(id);
  el.textContent = text;
  el.className = 'pill' + (tone ? ' ' + tone : '');
}

// ---------- protobuf 手工编解码 ----------

const utf8 = new TextEncoder();

function concatBytes(...parts) {
  const total = parts.reduce((n, p) => n + p.length, 0);
  const out = new Uint8Array(total);
  let at = 0;
  for (const p of parts) { out.set(p, at); at += p.length; }
  return out;
}

function varint(n) {
  let v = BigInt(Math.trunc(Number(n)));
  const out = [];
  while (v >= 0x80n) {
    out.push(Number((v & 0x7fn) | 0x80n));
    v >>= 7n;
  }
  out.push(Number(v));
  return Uint8Array.from(out);
}

function tag(field, wire) {
  return varint((BigInt(field) << 3n) | BigInt(wire));
}

function fv(field, n) {
  return concatBytes(tag(field, 0), varint(n));
}

function fbytes(field, bytes) {
  return concatBytes(tag(field, 2), varint(bytes.length), bytes);
}

function fs(field, s) {
  return fbytes(field, utf8.encode(s || ''));
}

function readVarint(buf, pos) {
  let v = 0n, shift = 0n, i = pos;
  while (i < buf.length) {
    const b = BigInt(buf[i++]);
    v |= (b & 0x7fn) << shift;
    if ((b & 0x80n) === 0n) return [Number(v), i];
    shift += 7n;
    if (shift > 63n) break;
  }
  throw new Error('varint 截断');
}

function decodeFields(bytes) {
  const fields = [];
  let i = 0;
  while (i < bytes.length) {
    const [key, next] = readVarint(bytes, i);
    i = next;
    const field = key >> 3;
    const wire = key & 7;
    if (wire === 0) {
      const [v, n] = readVarint(bytes, i);
      fields.push({ field, wire, value: v });
      i = n;
    } else if (wire === 2) {
      const [len, n] = readVarint(bytes, i);
      fields.push({ field, wire, value: bytes.subarray(n, n + len) });
      i = n + len;
    } else if (wire === 1) {
      fields.push({ field, wire, value: 0 });
      i += 8;
    } else if (wire === 5) {
      fields.push({ field, wire, value: 0 });
      i += 4;
    } else {
      throw new Error(`未知 wire type ${wire}`);
    }
  }
  return fields;
}

function one(fields, n, dflt = undefined) {
  for (let i = fields.length - 1; i >= 0; i -= 1) if (fields[i].field === n) return fields[i].value;
  return dflt;
}

function many(fields, n) {
  return fields.filter((f) => f.field === n).map((f) => f.value);
}

function strField(fields, n, dflt = '') {
  const v = one(fields, n);
  return v === undefined ? dflt : new TextDecoder().decode(v);
}

function makeFrame(kind, body) {
  const head = new Uint8Array([kind >> 8, kind & 0xff]);
  return concatBytes(head, body);
}

function encodeJoinRoom(matchId, seat, seatToken) {
  const parts = [fs(1, matchId)];
  if (seat !== null && seat !== undefined) parts.push(fv(2, seat));
  parts.push(fs(3, CLIENT_VERSION));
  if (seatToken) parts.push(fs(4, seatToken));
  return concatBytes(...parts);
}

function encodeReconnect(matchId, seat, lastSeq, seatToken) {
  return concatBytes(
    fs(1, matchId),
    fv(2, seat),
    fv(3, lastSeq),
    fs(4, seatToken),
  );
}

function encodeHeartbeat(matchId, seat, seq) {
  return concatBytes(fs(1, matchId), fv(2, seat), fv(3, seq));
}

function encodeRematchReady(matchId, seat) {
  return concatBytes(fs(1, matchId), fv(2, seat));
}

function encodeSubmit(matchId, seat, requestId, seatSeq, cards) {
  const head = concatBytes(
    fs(1, matchId),
    fv(2, seat),
    fs(3, requestId),
    fv(4, seatSeq),
  );
  if (cards && cards.length) {
    const payload = concatBytes(...cards.map((c) => fv(1, c)));
    return concatBytes(head, fv(5, 0), fbytes(6, payload));
  }
  return concatBytes(head, fv(5, 1), fbytes(7, new Uint8Array()));
}

// ---------- 牌面 ----------

function cardFace(id) {
  if (id >= JOKER_BASE) {
    return { rank: id < JOKER_BASE + 2 ? '小' : '大', suit: '王', red: id >= JOKER_BASE + 2, joker: true };
  }
  const base = Math.floor(id / 2);
  const suit = Math.floor(base / 13);
  return { rank: RANKS[base % 13], suit: SUITS[suit], red: suit === 1 || suit === 2, joker: false };
}

function cardNode(id, small = false, asButton = false, selected = false) {
  const f = cardFace(id);
  const face = document.createElement(asButton ? 'button' : 'div');
  face.className = 'card-face' + (f.red ? ' red' : '') + (f.joker ? ' joker' : '') + (small ? ' small' : '');
  if (asButton) {
    face.dataset.card = String(id);
    face.classList.toggle('sel', selected);
    face.title = `${f.rank}${f.suit}`;
  }
  const r = document.createElement('span');
  r.className = 'rank';
  r.textContent = f.rank;
  const s = document.createElement('span');
  s.className = 'suit';
  s.textContent = f.suit;
  face.append(r, s);
  return face;
}

function renderMini(container, cards) {
  container.replaceChildren();
  if (!cards || !cards.length) {
    const empty = document.createElement('span');
    empty.className = 'fine';
    empty.textContent = '—';
    container.append(empty);
    return;
  }
  for (const id of cards) container.append(cardNode(id, true));
}

// ---------- 大厅 ----------

async function loadGames() {
  const box = $('games');
  try {
    const body = await api(`/api/games?clientVersion=${encodeURIComponent(CLIENT_VERSION)}`);
    setPill('server-status', '服务在线', 'ok');
    $('games-sub').textContent = `服务端 ${body.serverTimeMs ? '已就绪' : '已连接'} · 规则集 ${body.items?.[0]?.rulesetVersion || '未知'}`;
    renderGames(body);
    renderUpgrade(body);
  } catch (err) {
    setPill('server-status', '连接失败', 'bad');
    box.replaceChildren(Object.assign(document.createElement('div'), {
      className: 'skeleton',
      textContent: err.message,
    }));
  }
}

function renderUpgrade(body) {
  const el = $('upgrade');
  if (body.upgrade?.required) {
    el.classList.remove('hidden');
    el.innerHTML = `<strong>需要升级</strong>：当前 ${body.clientVersion || CLIENT_VERSION} 低于最低 ${body.minimumVersion}，可以先看玩法但入座会被服务端拒绝。`;
  } else {
    el.classList.add('hidden');
  }
}

function renderGames(body) {
  const box = $('games');
  box.replaceChildren();
  const items = body.items || [];
  if (!items.length) {
    box.innerHTML = '<div class="skeleton">服务端尚未登记玩法。</div>';
    return;
  }
  for (const game of items) {
    const card = document.createElement('div');
    card.className = 'game' + (game.enabled ? '' : ' disabled');
    const title = document.createElement('div');
    title.className = 'game-title';
    title.innerHTML = `<strong>${game.displayName || game.gameId}</strong><span>${game.gameId} · ${game.seatsCount} 人 · ${game.rulesetVersion}</span>`;
    const modes = document.createElement('div');
    modes.className = 'modes';
    for (const m of game.modes || []) {
      const span = document.createElement('span');
      span.className = 'mode' + (m.enabled ? ' on' : '');
      span.textContent = `${m.label || m.mode}${m.enabled ? '' : '（关）'}`;
      modes.append(span);
    }
    card.append(title, modes);
    box.append(card);
  }
}

async function checkServer() {
  state.base = $('server-url').value.trim() || DEFAULT_BASE;
  localStorage.setItem(BASE_KEY, state.base);
  await loadGames();
}

function showAuth() {
  $('auth-guest').classList.add('hidden');
  $('auth-form').classList.remove('hidden');
}

async function sendCode() {
  const phone = $('auth-phone').value.trim();
  try {
    const body = await api('/api/auth/sms', { method: 'POST', body: JSON.stringify({ phone }) });
    $('auth-note').textContent = `验证码已发出，${body.cooldownSec || 60} 秒内可重发。本机白名单号固定测试码 666666。`;
  } catch (err) {
    $('auth-note').textContent = err.message;
  }
}

async function login(ev) {
  ev.preventDefault();
  const phone = $('auth-phone').value.trim();
  const code = $('auth-code').value.trim();
  try {
    const body = await api('/api/auth/login', {
      method: 'POST',
      body: JSON.stringify({ phone, code, clientVersion: CLIENT_VERSION }),
    });
    state.token = body.token;
    state.refreshToken = body.refreshToken || '';
    localStorage.setItem(TOKEN_KEY, state.token);
    localStorage.setItem(REFRESH_KEY, state.refreshToken);
    await loadProfile();
    $('auth-form').classList.add('hidden');
  } catch (err) {
    $('auth-note').textContent = err.message;
  }
}

async function loadProfile() {
  if (!state.token) {
    $('auth-me').classList.add('hidden');
    $('auth-guest').classList.remove('hidden');
    return;
  }
  try {
    const p = await api('/api/profile');
    $('auth-guest').classList.add('hidden');
    $('auth-me').classList.remove('hidden');
    $('auth-player').textContent = p.nickname || p.playerId;
    $('auth-expire').textContent = p.phoneTail4 ? `尾号 ${p.phoneTail4} · ${p.playerId}` : p.playerId;
  } catch {
    state.token = '';
    state.refreshToken = '';
    localStorage.removeItem(TOKEN_KEY);
    localStorage.removeItem(REFRESH_KEY);
    $('auth-me').classList.add('hidden');
    $('auth-guest').classList.remove('hidden');
  }
}

async function logout() {
  try {
    if (state.token) {
      await api('/api/auth/logout', {
        method: 'POST',
        body: JSON.stringify({ refreshToken: state.refreshToken }),
      });
    }
  } catch { /* 本地清掉即可 */ }
  state.token = '';
  state.refreshToken = '';
  localStorage.removeItem(TOKEN_KEY);
  localStorage.removeItem(REFRESH_KEY);
  await loadProfile();
}

async function createRoom(mode) {
  try {
    const body = await api('/api/room/create', {
      method: 'POST',
      body: JSON.stringify({ gameId: 'guandan', mode, seatsCount: 4 }),
    });
    await joinByCode(body.inviteCode, body.matchId);
  } catch (err) {
    notice(err.message, true);
  }
}

async function joinByCode(code, knownMatch) {
  const clean = (code || '').trim().toUpperCase();
  if (!clean) return notice('先填房间码', true);
  try {
    // 先 HTTP 预占座位，再拿 seat 去 WS 入座；匿名座位没有 owner，预占靠同一座位号兑现。
    const body = await api('/api/room/join', {
      method: 'POST',
      body: JSON.stringify({ inviteCode: clean }),
    });
    if (!body.accept) {
      return notice(body.reason === 'RS_SEAT_TAKEN' ? textMap('RS_SEAT_TAKEN') : '房间满员', true);
    }
    beginRoom(body.matchId || knownMatch, body.seat, clean);
  } catch (err) {
    notice(err.message, true);
  }
}

async function queue(mode) {
  clearTimeout(state.queueTimer);
  try {
    const body = await api('/api/match/queue', {
      method: 'POST',
      body: JSON.stringify({ gameId: 'guandan', mode }),
    });
    setPill('queue-state', '排队中…');
    $('queue-state').textContent = `票号 ${body.ticketId}，等待 4 人成局`;
    const ticket = body.ticketId;
    const started = Date.now();
    const poll = async () => {
      if (Date.now() - started > 65_000) {
        $('queue-state').textContent = '匹配超时，重试或换模式';
        await api(`/api/match/queue/${ticket}`, { method: 'DELETE' }).catch(() => {});
        return;
      }
      try {
        const st = await api(`/api/match/queue/${ticket}`);
        if (st.state === 'matched') {
          $('queue-state').textContent = `已匹配：${st.matchId}`;
          beginRoom(st.matchId, null, '');
          return;
        }
        if (st.state === 'timeout') {
          $('queue-state').textContent = '匹配超时，换模式或稍后再试';
          return;
        }
      } catch (err) {
        notice(err.message, true);
        return;
      }
      state.queueTimer = setTimeout(poll, 2000);
    };
    state.queueTimer = setTimeout(poll, 2000);
  } catch (err) {
    notice(err.message, true);
  }
}

// ---------- 房间 / WebSocket ----------

function beginRoom(matchId, seat, code) {
  state.matchId = matchId;
  state.seat = seat;
  state.seatToken = '';
  state.seatSeq = 0;
  state.code = code || '';
  state.manualClose = false;
  $('wait-code').textContent = code || '已匹配';
  $('wait-seat').textContent = seat ?? '-';
  $('waiting').classList.remove('hidden');
  $('table').classList.add('hidden');
  $('enter-card').scrollIntoView({ behavior: 'smooth', block: 'center' });
  connect(false);
}

function connect(isRetry) {
  clearTimeout(state.retryTimer);
  if (!state.matchId) return;
  if (state.ws && (state.ws.readyState === WebSocket.OPEN || state.ws.readyState === WebSocket.CONNECTING)) return;

  setPill('conn-state', isRetry ? '重连中…' : '连接中…');
  const protocols = state.token ? [`gf-token.${state.token}`] : undefined;
  const ws = protocols ? new WebSocket(wsUrl(), protocols) : new WebSocket(wsUrl());
  ws.binaryType = 'arraybuffer';
  state.ws = ws;

  ws.onopen = () => {
    state.connected = true;
    setPill('conn-state', '在线', 'ok');
    if (isRetry && state.seat !== null) {
      send(K.RECONNECT, encodeReconnect(state.matchId, state.seat, state.seatSeq, state.seatToken));
    } else {
      send(K.JOIN_ROOM, encodeJoinRoom(state.matchId, state.seat, state.seatToken));
    }
  };

  ws.onmessage = async (ev) => {
    const buf = ev.data instanceof ArrayBuffer ? new Uint8Array(ev.data) : new Uint8Array(await ev.data.arrayBuffer());
    if (buf.length < 2) return;
    const kind = (buf[0] << 8) | buf[1];
    handleFrame(kind, buf.subarray(2));
  };

  ws.onclose = () => {
    state.connected = false;
    if (state.manualClose || !state.matchId) return;
    setPill('conn-state', '断线', 'bad');
    notice('连接断了，90 秒内会自动续座；超过时间就需要重新进房。');
    if (!state.retryStartedAt) state.retryStartedAt = Date.now();
    const retry = () => {
      if (state.manualClose) return;
      if (Date.now() - state.retryStartedAt > 90_000) {
        notice('重连超时，已退出牌桌；未结束的房间可以再凭码进入。', true);
        leaveTable(true);
        return;
      }
      connect(true);
      state.retryTimer = setTimeout(retry, 3000);
    };
    state.retryTimer = setTimeout(retry, 1500);
  };

  ws.onerror = () => { setPill('conn-state', '连接异常', 'bad'); };
}

function send(kind, body) {
  if (!state.ws || state.ws.readyState !== WebSocket.OPEN) return false;
  state.ws.send(makeFrame(kind, body));
  return true;
}

function handleFrame(kind, body) {
  const f = decodeFields(body);
  if (kind === S.JOIN_ACK) return onJoinAck(f);
  if (kind === S.ROOM_SNAPSHOT) return onSnapshot(f);
  if (kind === S.ROOM_DELTA) return onDelta(f);
  if (kind === S.ACTION_ACK) return onAck(f);
  if (kind === S.PHASE_CHANGE) return onPhase(f);
  if (kind === S.SETTLE_RESULT) return onSettle(f);
  if (kind === S.REMATCH_STATE) return onRematch(f);
}

function onJoinAck(f) {
  const accept = one(f, 3, 0) === 1;
  const seat = one(f, 2, 0);
  const reason = strField(f, 5, '');
  if (!accept) {
    notice(textMap(reason || 'RS_SEAT_TAKEN'), true);
    leaveTable(true);
    return;
  }
  state.seat = seat;
  state.seatToken = strField(f, 6, '');
  state.seatSeq = 0;
  $('wait-seat').textContent = String(seat);
  $('self-seat').textContent = String(seat);
  saveSession();
  renderSeats();
}

function onSnapshot(f) {
  state.matchId = strField(f, 1, state.matchId);
  state.seat = one(f, 2, state.seat ?? 0);
  state.roomSeq = one(f, 5, state.roomSeq);
  state.seatSeq = one(f, 6, state.seatSeq);
  state.roundNo = one(f, 11, state.roundNo);
  saveSession();

  const rawView = one(f, 9);
  try {
    state.view = rawView ? JSON.parse(new TextDecoder().decode(rawView)) : {};
  } catch {
    state.view = {};
  }
  state.hand = state.view.hand?.cards ? [...state.view.hand.cards] : [];
  state.lastPlay = state.view.lastPlay || null;
  state.selected.clear();
  state.turnSeat = null;
  state.phase = state.phase >= 0 ? state.phase : 2;

  $('waiting').classList.add('hidden');
  $('table').classList.remove('hidden');
  $('self-seat').textContent = String(state.seat);
  $('table-seat').textContent = `座位 ${state.seat}`;
  $('table-round').textContent = `第 ${state.roundNo} 局`;
  $('settle').classList.add('hidden');
  $('btn-rematch').classList.add('hidden');
  $('btn-rematch').disabled = false;
  $('settle-rematch').disabled = false;
  renderAll();
}

function onDelta(f) {
  state.roomSeq = one(f, 2, state.roomSeq);
  for (const evBytes of many(f, 3)) {
    const ev = decodeFields(evBytes);
    if (ev.length === 0) continue;
    const type = ev[0].field;
    const inner = decodeFields(ev[0].value);
    if (type === 1) {
      const seat = one(inner, 1, 0);
      const cards = many(inner, 2);
      state.lastPlay = { seat, cards };
      if (seat === state.seat) {
        state.hand = state.hand.filter((c) => !cards.includes(c));
      }
    } else if (type === 5) {
      state.turnSeat = one(inner, 1, 0);
    } else if (type === 3) {
      const from = one(inner, 1, 0);
      const to = one(inner, 2, 0);
      const card = one(inner, 3, 0);
      if (from === state.seat) state.hand = state.hand.filter((c) => c !== card);
      if (to === state.seat) state.hand.push(card);
    }
  }
  renderAll();
}

function onAck(f) {
  const accepted = one(f, 4, 0) === 1;
  state.seatSeq = one(f, 2, state.seatSeq);
  state.roomSeq = one(f, 3, state.roomSeq);
  const reason = strField(f, 5, '');
  if (accepted) {
    state.selected.clear();
    $('action-note').textContent = '已提交，等服务端广播。';
  } else {
    $('action-note').textContent = textMap(reason);
    notice(textMap(reason), true);
  }
  saveSession();
  renderHand();
}

function onPhase(f) {
  state.phase = one(f, 1, 0);
  $('table-phase').textContent = PHASES[state.phase] || '未知';
  if (state.phase === 2 && state.turnSeat === null) {
    $('turn-line').textContent = '等待服务端回合信息…';
  }
  renderAll();
}

function onSettle(f) {
  state.phase = 3;
  state.turnSeat = null;
  state.selected.clear();
  const raw = one(f, 3);
  let result = {};
  try { result = raw ? JSON.parse(new TextDecoder().decode(raw)) : {}; } catch { result = {}; }
  const levels = many(f, 4);
  const body = $('settle-body');
  body.replaceChildren();
  const winner = Number(result.winner_team ?? 0);
  body.insertAdjacentHTML('beforeend', `<div>赢家：<strong>${winner === 0 ? 'A 队' : 'B 队'}</strong> · ${Number(result.points ?? 0)} 分</div>`);
  for (const [i, seat] of (result.ranks || []).entries()) {
    const row = document.createElement('div');
    row.className = 'rank-row';
    row.innerHTML = `<span>第 ${i + 1} 名</span><span>座位 ${seat}${Number(seat) === state.seat ? '（我）' : ''}</span>`;
    body.append(row);
  }
  if (levels.length) {
    body.insertAdjacentHTML('beforeend', `<div class="fine">级牌：${levels.map((l) => RANKS[l] || l).join(' · ')}</div>`);
  }
  $('settle').classList.remove('hidden');
  $('btn-rematch').classList.remove('hidden');
  $('rematch-note').textContent = `等待全员确认或 ${Math.round(Number(one(f, 5, 0)) / 1000)} 秒后自动开下一局。`;
  renderAll();
}

function onRematch(f) {
  const ready = many(f, 3);
  const humans = many(f, 4);
  const remain = one(f, 5, 0);
  $('rematch-note').textContent = `已确认 ${ready.length}/${humans.length} 人 · ${Math.ceil(Number(remain) / 1000)} 秒后自动开局`;
  // 结算弹层只由 SettleResult 打开、由玩家「留在牌桌」或新局快照关闭；
  // RematchState 只更新确认进度，不能把玩家主动关掉的弹层又弹回来。
}

// ---------- 渲染 ----------

function renderAll() {
  renderSeats();
  renderHand();
  renderMini($('last-play'), state.lastPlay?.cards || []);
  $('table-phase').textContent = PHASES[state.phase] ?? '未知';
  $('table-round').textContent = `第 ${state.roundNo || 1} 局`;
  $('level-line').textContent = levelText();
  $('turn-line').textContent = turnText();
  const myTurn = state.phase === 2 && state.turnSeat === state.seat;
  $('btn-pass').disabled = !myTurn;
  $('btn-play').disabled = !myTurn;
  $('action-note').textContent = myTurn ? '轮到你了：点牌选中后出牌。' : (state.phase === 3 ? '本局已结算。' : '等待其他玩家。');
}

function renderSeats() {
  document.querySelectorAll('.seat-box').forEach((el) => {
    const seat = Number(el.dataset.seat);
    el.classList.toggle('active', state.turnSeat !== null && state.turnSeat === seat);
  });
  const self = document.querySelector('.seat-self');
  if (self) self.dataset.seat = String(state.seat ?? 0);
}

function renderHand() {
  const box = $('hand');
  box.replaceChildren();
  if (!state.hand.length) {
    const empty = document.createElement('span');
    empty.className = 'fine';
    empty.textContent = state.phase === 3 ? '本局手牌已清空。' : '还没有手牌，等待发牌…';
    box.append(empty);
    return;
  }
  for (const id of [...state.hand].sort((a, b) => a - b)) {
    const btn = cardNode(id, false, true, state.selected.has(id));
    btn.addEventListener('click', () => {
      if (state.phase !== 2 || state.turnSeat !== state.seat) {
        notice('还没轮到你，先别急着点牌。');
        return;
      }
      if (state.selected.has(id)) state.selected.delete(id);
      else state.selected.add(id);
      renderHand();
    });
    box.append(btn);
  }
}

function turnText() {
  if (state.phase === 3) return '本局已结算';
  if (state.turnSeat === null || state.turnSeat === undefined) return '等待回合信息…';
  return state.turnSeat === state.seat ? '轮到你出牌' : `轮到座位 ${state.turnSeat}`;
}

function levelText() {
  const lv = state.view?.['round.level'];
  if (lv === undefined || lv === null) return '';
  return `本局打 ${RANKS[Number(lv)] || lv}`;
}

// ---------- 动作 ----------

function playSelected() {
  if (!state.selected.size) return notice('先选至少一张牌。', true);
  const cards = [...state.selected];
  submit(cards);
}

function passTurn() {
  submit([]);
}

function submit(cards) {
  if (state.phase !== 2 || state.turnSeat !== state.seat) return notice('现在不该你出牌。');
  const requestId = `${state.seat}-${Date.now().toString(36)}-${state.requestSeq++}`;
  const body = encodeSubmit(state.matchId, state.seat, requestId, state.seatSeq + 1, cards);
  if (!send(K.SUBMIT_ACTION, body)) notice('连接已断，稍后自动重试。', true);
}

function rematch() {
  if (!state.matchId || state.seat === null) return;
  send(K.REMATCH_READY, encodeRematchReady(state.matchId, state.seat));
  $('btn-rematch').disabled = true;
  $('settle-rematch').disabled = true;
  $('rematch-note').textContent = '已确认，等其他人；超时会自动开局。';
}

function leaveTable(silent = false) {
  state.manualClose = true;
  clearTimeout(state.retryTimer);
  clearTimeout(state.queueTimer);
  clearInterval(state.heartbeatTimer);
  if (state.ws) {
    try { state.ws.close(); } catch { /* ignore */ }
  }
  state.ws = null;
  state.connected = false;
  state.retryStartedAt = 0;
  clearSession();
  $('waiting').classList.add('hidden');
  $('table').classList.add('hidden');
  $('settle').classList.add('hidden');
  $('btn-rematch').classList.add('hidden');
  $('btn-rematch').disabled = false;
  $('settle-rematch').disabled = false;
  state.phase = -1;
  state.turnSeat = null;
  state.hand = [];
  state.view = null;
  state.lastPlay = null;
  if (!silent) notice('已离开牌桌。');
}

// ---------- 事件绑定 ----------

function bind() {
  $('server-url').value = state.base;
  $('client-version').textContent = `客户端 ${CLIENT_VERSION}`;
  $('server-check').addEventListener('click', checkServer);
  $('auth-open').addEventListener('click', showAuth);
  $('auth-send').addEventListener('click', sendCode);
  $('auth-form').addEventListener('submit', login);
  $('auth-logout').addEventListener('click', logout);

  $('create-casual').addEventListener('click', () => createRoom('casual'));
  $('create-ranked').addEventListener('click', () => createRoom('ranked'));
  $('join-form').addEventListener('submit', (ev) => {
    ev.preventDefault();
    joinByCode($('join-code').value);
  });
  $('queue-casual').addEventListener('click', () => queue('casual'));
  $('queue-ranked').addEventListener('click', () => queue('ranked'));

  $('copy-code').addEventListener('click', async () => {
    try {
      await navigator.clipboard.writeText(state.code || $('wait-code').textContent);
      notice('房间码已复制。');
    } catch {
      notice('浏览器不允许读剪贴板，手动抄一下房间码。', true);
    }
  });
  $('wait-leave').addEventListener('click', () => leaveTable());
  $('table-leave').addEventListener('click', () => leaveTable());

  $('btn-play').addEventListener('click', playSelected);
  $('btn-pass').addEventListener('click', passTurn);
  $('btn-rematch').addEventListener('click', rematch);
  $('settle-rematch').addEventListener('click', rematch);
  $('settle-stay').addEventListener('click', () => $('settle').classList.add('hidden'));
}

function recoverSession() {
  const raw = localStorage.getItem(SESSION_KEY);
  if (!raw) return;
  try {
    const saved = JSON.parse(raw);
    if (!saved.matchId || saved.seat === null || saved.seat === undefined) return;
    state.base = saved.base || state.base;
    state.matchId = saved.matchId;
    state.seat = saved.seat;
    state.seatToken = saved.seatToken || '';
    state.seatSeq = saved.seatSeq || 0;
    state.code = saved.code || '';
    state.manualClose = false;
    $('server-url').value = state.base;
    $('wait-code').textContent = state.code || '已匹配';
    $('wait-seat').textContent = String(state.seat);
    $('waiting').classList.remove('hidden');
    connect(true);
  } catch { /* 坏存档直接忽略 */ }
}

bind();
loadProfile();
loadGames();
recoverSession();

state.heartbeatTimer = setInterval(() => {
  if (state.matchId && state.seat !== null && state.ws?.readyState === WebSocket.OPEN) {
    send(K.HEARTBEAT, encodeHeartbeat(state.matchId, state.seat, Date.now()));
  }
}, 15_000);
