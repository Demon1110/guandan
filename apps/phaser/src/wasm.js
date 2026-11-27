// gf-client-wasm 桥接层：零依赖裸加载，纯 JS 重写（不引用 apps/web 的任何代码）。
// wasm 出参一律是 NUL 结尾 JSON 字符串；入参牌组用 u16 数组写入 wasm 内存。
// 导出清单与 crates/client-wasm/src/lib.rs 的 28 个 #[no_mangle] 一一对应。

export class WasmBridge {
  constructor() {
    this.ex = null;
    this.clockBase = 0;
  }

  async load(url = 'gf_client_wasm.wasm') {
    if (this.ex) return;
    const bytes = await (await fetch(url)).arrayBuffer();
    const module = await WebAssembly.compile(bytes);
    this.ex = (await WebAssembly.instantiate(module, {})).exports;
    this.clockBase = performance.now();
  }

  now() {
    return Math.round(performance.now() - this.clockBase);
  }

  // 读 wasm 返回的 NUL 结尾字符串并释放。
  cstr(ptr) {
    if (!ptr) throw new Error('wasm 返回空指针');
    const view = new Uint8Array(this.ex.memory.buffer, ptr);
    let end = 0;
    while (end < view.length && view[end] !== 0) end += 1;
    const text = new TextDecoder().decode(view.subarray(0, end));
    this.ex.gf_str_free(ptr);
    return text;
  }

  // 把 u16 牌 id 数组写入 wasm 内存，回调后释放。
  withCards(cards, fn) {
    if (!cards || cards.length === 0) return fn(0, 0);
    const bytes = cards.length * 2;
    const ptr = this.ex.gf_alloc(bytes);
    new Uint16Array(this.ex.memory.buffer, ptr, cards.length).set(cards);
    try {
      return fn(ptr, cards.length);
    } finally {
      this.ex.gf_mem_free(ptr, bytes);
    }
  }

  // 把 u8 字节（主题包 / 公钥 / faceCode）写入 wasm 内存，回调后释放。
  withBytes(bytes, fn) {
    const len = bytes.length;
    const ptr = this.ex.gf_alloc(len);
    new Uint8Array(this.ex.memory.buffer, ptr, len).set(bytes);
    try {
      return fn(ptr, len);
    } finally {
      this.ex.gf_mem_free(ptr, len);
    }
  }

  json(ptr) {
    return JSON.parse(this.cstr(ptr));
  }

  // ---- 房间生命周期 ----
  create(seedHi, seedLo, humanMask, level = 2) {
    const ok = this.ex.gf_room_new_level
      ? this.ex.gf_room_new_level(seedHi, seedLo, humanMask, level)
      : this.ex.gf_room_new(seedHi, seedLo, humanMask);
    if (ok !== 1) throw new Error('建桌失败');
    if (this.ex.gf_room_start(BigInt(0)) !== 1) throw new Error('发牌失败');
  }

  step(at = this.now()) {
    return this.json(this.ex.gf_room_step(BigInt(at)));
  }

  submit(seat, at, action) {
    const code = action.tributeReturn ? 2 : action.pass ? 1 : 0;
    return this.withCards(action.cards ?? [], (ptr, len) =>
      this.json(this.ex.gf_room_submit(seat, BigInt(at), code, ptr, len)),
    );
  }

  replay(rec) {
    const code = rec.tributeReturn ? 2 : rec.pass ? 1 : 0;
    return this.withCards(rec.cards ?? [], (ptr, len) =>
      this.ex.gf_room_replay_action(rec.seat, code, ptr, len) === 1,
    );
  }

  nextRound(at = 0) {
    return this.json(this.ex.gf_room_next_round(BigInt(at))).kind === 'dealt';
  }

  resumeWindow(at = this.now()) {
    return this.ex.gf_room_resume_window(BigInt(at)) === 1;
  }

  // ---- 视图与元信息 ----
  view(seat) {
    return this.json(this.ex.gf_room_view(seat));
  }

  info() {
    if (!this.ex.gf_room_info) return { phase: 'idle', seats: [], levels: [] };
    return this.json(this.ex.gf_room_info());
  }

  meta() {
    return this.json(this.ex.gf_room_meta());
  }

  seats() {
    if (!this.ex.gf_room_seats) return [0, 1, 2, 3].map((seat) => ({ seat, human: seat === 0 }));
    return this.json(this.ex.gf_room_seats()).seats;
  }

  hash() {
    return this.cstr(this.ex.gf_room_hash());
  }

  replayHash() {
    return this.cstr(this.ex.gf_room_replay_hash());
  }

  lastRecord() {
    const raw = this.json(this.ex.gf_room_last_record());
    if (!raw) return null;
    return {
      seq: raw.roomSeq,
      roundNo: raw.roundNo,
      seat: raw.seat,
      pass: raw.action.pass,
      tributeReturn: !!raw.action.tributeReturn,
      cards: raw.action.cards ?? [],
    };
  }

  // ---- 决策辅助 ----
  canPlay(cards) {
    if (!this.ex.gf_room_can_play) return null;
    return this.withCards(cards, (ptr, len) => this.json(this.ex.gf_room_can_play(ptr, len)));
  }

  legal() {
    if (!this.ex.gf_room_legal) return [];
    return this.json(this.ex.gf_room_legal());
  }

  plan() {
    if (!this.ex.gf_room_plan) return null;
    try {
      return this.json(this.ex.gf_room_plan());
    } catch {
      return null;
    }
  }

  botProbe(seat) {
    if (!this.ex.gf_l0_probe) return null;
    try {
      return this.json(this.ex.gf_l0_probe(seat));
    } catch {
      return null;
    }
  }

  // ---- 主题（可选，包损坏时调用方回退字卡） ----
  // 用 gf_theme_load_verified（强制验签），审计中 gf_theme_load 的 FLAG_SIGNED 可被清除绕过。
  themeLoad(packBytes, pubBytes) {
    const fn = this.ex.gf_theme_load_verified ?? this.ex.gf_theme_load;
    if (!fn) return null;
    return this.withBytes(packBytes, (ptr, len) =>
      this.withBytes(pubBytes, (pp, plen) => this.json(fn(ptr, len, pp, plen))),
    );
  }

  themeCard(code, level, pct = 0) {
    if (!this.ex.gf_theme_card) return null;
    return this.withBytes(new TextEncoder().encode(code), (ptr, len) =>
      this.json(this.ex.gf_theme_card(ptr, len, level, pct)),
    );
  }

  themeUnload() {
    if (this.ex.gf_theme_unload) this.ex.gf_theme_unload();
  }
}
