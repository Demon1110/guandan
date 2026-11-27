//! 浏览器 wasm 绑定层：把 `gf-table-runtime` 的房间 + `gf-ai` 的 L0/L1/L2 bot 暴露成
//! 一组 `extern "C"` 函数，浏览器用 `WebAssembly.instantiate` 裸加载（零 import，无需
//! wasm-bindgen / npm 工具链）。
//!
//! ## 为什么不做 JSON 入参
//! 仓库离线可用依赖里没有 `serde_json`，所以入参全部走**标量 + 定长数组**：
//! - 种子拆成 `(seed_hi, seed_lo)` 两个 u32（`u64` 参数在 JS 侧要 BigInt，两段更省事）；
//! - 动作 = `(seat, is_pass, cards_ptr, cards_len)`，牌 id 用 u16 数组写入 wasm 内存。
//!
//! 出参一律是**以 `\0` 结尾的 JSON 字符串**（指针 + `gf_str_free(ptr)` 释放）。浏览器侧
//! `TextDecoder` 读到 NUL 为止即可。视图本来就是 canonical JSON，直接原样透传。
//!
//! ## 续局（「杀进程再打开」）
//! 进程被杀后浏览器只剩 IndexedDB 里的动作日志。前端据此重演：
//! `gf_room_new` → `gf_room_start` → 逐条 `gf_room_next_round` / `gf_room_replay_action`，
//! 最后用 `gf_room_hash` 与存档里的哈希比对；相等即续局成功。

#[cfg(target_arch = "wasm32")]
use std::cell::UnsafeCell;
use std::collections::HashMap;
use std::ffi::c_void;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::{Mutex, MutexGuard};

use gf_engine::contract::{Action, Engine};
use gf_table_runtime::{
    BotPolicy, PlayerKind, Room, RoomConfig, StepOutcome, DEFAULT_RULESET_VERSION,
};

/// 全局槽的实现分两条路径：
/// - `wasm32-unknown-unknown` 是单线程环境（不启用 wasm threads），用零开销的
///   `UnsafeCell`，`Sync` 的唯一依据是「同一实例内所有导出都在同一条 JS 调用栈上串行」；
/// - native 目标（`cargo test/clippy --workspace` 会编译本 crate）没有这个前提，
///   必须用 `Mutex`，否则 `UnsafeCell` + `Sync` 是实打实的 UB。
#[cfg(target_arch = "wasm32")]
struct Global<T>(UnsafeCell<T>);

#[cfg(target_arch = "wasm32")]
impl<T> Global<T> {
    const fn new(value: T) -> Self {
        Self(UnsafeCell::new(value))
    }
}

// SAFETY: 只在 wasm32-unknown-unknown 且未启用线程时成立；native 分支用 Mutex。
#[cfg(target_arch = "wasm32")]
unsafe impl<T> Sync for Global<T> {}

#[cfg(not(target_arch = "wasm32"))]
struct Global<T>(Mutex<T>);

#[cfg(not(target_arch = "wasm32"))]
impl<T> Global<T> {
    const fn new(value: T) -> Self {
        Self(Mutex::new(value))
    }
}

static ROOM: Global<Option<Room>> = Global::new(None);
static ALLOCS: Global<Option<HashMap<usize, usize>>> = Global::new(None);
/// 单次入参上限：主题包最大 16MB；牌数组另有更小的上限。
const MAX_INPUT_BYTES: usize = 16 * 1024 * 1024;
const MAX_CARDS_INPUT: usize = 256;
/// 导出字符串长度头的字节数（见 [`out`]）。
const OUT_HEADER: usize = 8;

fn with_allocs<R>(f: impl FnOnce(&mut HashMap<usize, usize>) -> R) -> R {
    #[cfg(target_arch = "wasm32")]
    {
        // SAFETY: 单线程 wasm，导出函数不会重入；初始化只发生一次。
        unsafe { f((&mut *ALLOCS.0.get()).get_or_insert_with(HashMap::new)) }
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let mut guard = ALLOCS.0.lock().unwrap_or_else(|e| e.into_inner());
        f(guard.get_or_insert_with(HashMap::new))
    }
}

/// 校验 `[ptr, ptr+len)` 是一整块由 `gf_alloc` 登记、且满足对齐与上限的内存。
fn input_range_ok(ptr: *const u8, len: usize, align: usize, max: usize) -> bool {
    if len == 0 {
        return true;
    }
    if ptr.is_null() || len > max || !(ptr as usize).is_multiple_of(align) {
        return false;
    }
    let addr = ptr as usize;
    if addr.checked_add(len).is_none() {
        return false;
    }
    with_allocs(|allocs| allocs.get(&addr).is_some_and(|allocated| len <= *allocated))
}

/// 对全局槽的临时引用；`Deref` 直接得到槽里的值（`Room` / `Theme`）。
struct SlotRef<T: 'static> {
    #[cfg(target_arch = "wasm32")]
    slot: &'static mut Option<T>,
    #[cfg(not(target_arch = "wasm32"))]
    guard: MutexGuard<'static, Option<T>>,
}

impl<T: 'static> SlotRef<T> {
    #[cfg(target_arch = "wasm32")]
    fn as_option(&self) -> &Option<T> {
        &*self.slot
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn as_option(&self) -> &Option<T> {
        &self.guard
    }

    #[cfg(target_arch = "wasm32")]
    fn as_option_mut(&mut self) -> &mut Option<T> {
        &mut *self.slot
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn as_option_mut(&mut self) -> &mut Option<T> {
        &mut self.guard
    }
}

impl<T: 'static> std::ops::Deref for SlotRef<T> {
    type Target = T;

    fn deref(&self) -> &T {
        self.as_option().as_ref().expect("global slot is empty")
    }
}

impl<T: 'static> std::ops::DerefMut for SlotRef<T> {
    fn deref_mut(&mut self) -> &mut T {
        self.as_option_mut().as_mut().expect("global slot is empty")
    }
}

fn room() -> Option<SlotRef<Room>> {
    #[cfg(target_arch = "wasm32")]
    {
        // SAFETY: 单线程 wasm；只取一次裸指针，随后由 `SlotRef` 独占。
        let slot = unsafe { &mut *ROOM.0.get() };
        slot.is_some().then_some(SlotRef { slot })
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let guard = ROOM.0.lock().unwrap_or_else(|e| e.into_inner());
        guard.is_some().then_some(SlotRef { guard })
    }
}

fn replace_room(room: Option<Room>) {
    #[cfg(target_arch = "wasm32")]
    {
        // SAFETY: 单线程 wasm；替换整个槽，不留下别名。
        unsafe { *ROOM.0.get() = room };
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        *ROOM.0.lock().unwrap_or_else(|e| e.into_inner()) = room;
    }
}

/// 需要 `&mut Room` 的导出从这里拿槽；调用方绑定必须写 `mut r`
/// （`DerefMut` 要可变接收者），读路径继续用 [`room`]。
fn room_mut() -> Option<SlotRef<Room>> {
    room()
}

/// 把一段文本变成以 NUL 结尾的独立堆块：`[u64 长度][bytes...]`，返回 bytes 起点。
///
/// 生命周期契约：调用方拿到指针后必须调用 [`gf_str_free`]；不再有全局 `STRINGS`
/// 列表兜底，也不会因为一次漏 free 把整局所有导出字符串都留在 wasm 线性内存里。
fn out(s: String) -> *mut u8 {
    let mut buf = s.into_bytes();
    if buf.contains(&0) {
        buf.retain(|b| *b != 0);
    }
    buf.push(0);
    let len = buf.len();
    let mut all: Vec<u8> = Vec::with_capacity(OUT_HEADER + len);
    all.extend_from_slice(&(len as u64).to_le_bytes());
    all.extend_from_slice(&buf);
    let boxed: Box<[u8]> = all.into_boxed_slice();
    let base = Box::into_raw(boxed).cast::<u8>();
    // SAFETY: `base` 来自刚分配、未被释放的 `OUT_HEADER + len` 字节块。
    unsafe { base.add(OUT_HEADER) }
}

fn err(e: impl std::fmt::Debug) -> *mut u8 {
    out(format!(
        "{{\"kind\":\"error\",\"error\":{}}}",
        json_str(&format!("{e:?}"))
    ))
}

/// 最小 JSON 字符串转义（只覆盖日志/视图里可能出现的字符）。
fn json_str(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    o.push('"');
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

/// 数值输出：整数不带小数点，其余保留 2 位（`2.2` → `2.20`），避免浮点尾巴漏给 JS。
fn num_json(v: f64) -> String {
    if (v - v.round()).abs() < 1e-9 {
        format!("{}", v.round() as i64)
    } else {
        format!("{v:.2}")
    }
}

/// 拆种子：`(hi << 32) | lo`，与宿主 `gf_room_seed` 约定一致。
fn join_seed(hi: u32, lo: u32) -> u64 {
    ((hi as u64) << 32) | lo as u64
}

fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

fn action_json(action: &Action) -> String {
    match action {
        Action::Pass => "{\"pass\":true}".to_owned(),
        Action::Play { cards } => {
            let mut s = String::from("{\"pass\":false,\"cards\":[");
            for (i, c) in cards.iter().enumerate() {
                if i > 0 {
                    s.push(',');
                }
                s.push_str(&c.to_string());
            }
            s.push_str("]}");
            s
        }
        Action::TributeReturn { card } => {
            format!("{{\"pass\":false,\"tributeReturn\":true,\"cards\":[{card}]}}")
        }
    }
}

/// 带牌型名的候选动作：页面理牌要按「是不是炸弹 / 同花顺」分流，这里顺手把牌型带上。
fn action_json_patterned(action: &Action, pattern_id: &str) -> String {
    match action {
        Action::Pass => "{\"pass\":true}".to_owned(),
        Action::Play { cards } => {
            let mut s = format!(
                "{{\"pass\":false,\"patternId\":{},\"cards\":[",
                json_str(pattern_id)
            );
            for (i, c) in cards.iter().enumerate() {
                if i > 0 {
                    s.push(',');
                }
                s.push_str(&c.to_string());
            }
            s.push_str("]}");
            s
        }
        Action::TributeReturn { .. } => action_json(action),
    }
}

fn auto_json(auto: Option<gf_table_runtime::AutoReason>) -> &'static str {
    match auto {
        None => "null",
        Some(gf_table_runtime::AutoReason::Timeout) => "\"Timeout\"",
        Some(gf_table_runtime::AutoReason::Offline) => "\"Offline\"",
        Some(gf_table_runtime::AutoReason::BotIllegal) => "\"BotIllegal\"",
    }
}

fn opt_u8(v: Option<u8>) -> String {
    v.map(|x| x.to_string())
        .unwrap_or_else(|| "null".to_owned())
}

fn summary_json(r: &Room) -> String {
    let Some(sum) = r.last_settle() else {
        return "null".to_owned();
    };
    let ranks = sum
        .ranks
        .iter()
        .map(|x| x.to_string())
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"roundNo\":{},\"ranks\":[{}],\"winnerTeam\":{},\"completedTeam\":{},\"step\":{},\"points\":{},\"matchEnd\":{}}}",
        sum.round_no,
        ranks,
        opt_u8(sum.winner_team),
        opt_u8(sum.completed_team),
        opt_u8(sum.step),
        sum.points,
        sum.match_end.is_some()
    )
}

fn phase_name(phase: u16) -> &'static str {
    match phase {
        0 => "deal",
        1 => "tribute",
        2 => "play",
        3 => "settle",
        _ => "unknown",
    }
}

fn outcome_json(r: &Room, diff: StepOutcome) -> String {
    let round_no = r.state().map(|s| s.round_no).unwrap_or(0);
    let phase = r.state().map(|s| phase_name(s.phase)).unwrap_or("idle");
    // 本条动作的真实 `roomSeq`（`ActionRecord.room_seq`）；`r.room_seq()` 是**下一条**序号，
    // 直接拿它会让「逐手记录」与「结算补记（lastRecord 的真实序号）」差 1，同键覆盖丢一手。
    let action_seq = r.log().last().map(|rec| rec.room_seq).unwrap_or(0);
    match diff {
        StepOutcome::Waiting { seat, deadline_ms } => format!(
            "{{\"kind\":\"waiting\",\"phase\":\"{phase}\",\"seat\":{seat},\"deadlineMs\":{deadline_ms},\"roundNo\":{round_no}}}"
        ),
        StepOutcome::Applied { seat, action, auto } => format!(
            "{{\"kind\":\"applied\",\"phase\":\"{phase}\",\"seat\":{},\"action\":{},\"auto\":{},\"roundNo\":{},\"roomSeq\":{}}}",
            seat,
            action_json(&action),
            auto_json(auto),
            round_no,
            action_seq
        ),
        StepOutcome::Settled(_) => format!(
            "{{\"kind\":\"settled\",\"phase\":\"{phase}\",\"roundNo\":{},\"roomSeq\":{},\"summary\":{}}}",
            round_no,
            action_seq,
            summary_json(r)
        ),
        StepOutcome::MatchEnded(end) => format!(
            "{{\"kind\":\"matchEnd\",\"phase\":\"{phase}\",\"roundNo\":{},\"winnerTeam\":{},\"summary\":{}}}",
            round_no,
            opt_u8(end.winner_team),
            summary_json(r)
        ),
    }
}

fn read_cards(ptr: *const u16, len: usize) -> Vec<u16> {
    if len == 0 {
        return Vec::new();
    }
    let bytes = match len.checked_mul(2) {
        Some(bytes) => bytes,
        None => return Vec::new(),
    };
    if !input_range_ok(ptr.cast::<u8>(), bytes, 2, MAX_CARDS_INPUT * 2) {
        // 非法指针/长度按空动作处理，宁可判非法也不跨 FFI 读越界。
        return Vec::new();
    }
    // SAFETY: 上方已确认整段落在 `gf_alloc` 登记且对齐的块内。
    unsafe { std::slice::from_raw_parts(ptr, len) }.to_vec()
}

fn make_action(is_pass: u8, cards: Vec<u16>) -> Action {
    match is_pass {
        1 => Action::Pass,
        // 2 = 还贡：cards[0] 为选中的还贡牌（wasm 入参保持标量 + 数组，不引入 JSON）。
        2 => Action::TributeReturn {
            card: cards.first().copied().unwrap_or(0),
        },
        _ => Action::Play { cards },
    }
}

// ---------------- 内存与字符串 ----------------

/// 分配 `len` 字节供宿主写入（牌 id 数组等）。宿主用完调 [`gf_mem_free`]。
#[no_mangle]
pub extern "C" fn gf_alloc(len: usize) -> *mut c_void {
    if len == 0 || len > MAX_INPUT_BYTES {
        return std::ptr::null_mut();
    }
    let mut buf = vec![0u8; len];
    let ptr = buf.as_mut_ptr().cast::<c_void>();
    std::mem::forget(buf);
    with_allocs(|allocs| {
        allocs.insert(ptr as usize, len);
    });
    ptr
}

/// 释放 [`gf_alloc`] 分配的内存（容量按 `len` 还原）。
///
/// 只接受「登记过、长度一致」的块；伪造指针或改长度都不会走到 `from_raw_parts`。
///
/// # Safety
///
/// `ptr` 必须是 [`gf_alloc`] 返回、且尚未释放的指针；`len` 必须与分配时一致。
/// 重复释放、跨分配器指针或错误长度都会造成未定义行为（函数内部会先查登记表，
/// 但登记表只能防「本 crate 之外」的指针，不能防调用方重复释放同一块）。
#[no_mangle]
pub unsafe extern "C" fn gf_mem_free(ptr: *mut c_void, len: usize) {
    if ptr.is_null() || len == 0 {
        return;
    }
    let addr = ptr as usize;
    let Some(allocated) = with_allocs(|allocs| allocs.remove(&addr)) else {
        return;
    };
    if allocated != len {
        // 长度不匹配说明调用方状态错了：不释放（泄漏一块）比 UB 安全。
        with_allocs(|allocs| {
            allocs.insert(addr, allocated);
        });
        return;
    }
    // SAFETY: 指针来自 `gf_alloc` 的 `Vec<u8>`，登记长度与调用方一致。
    drop(unsafe { Vec::from_raw_parts(ptr.cast::<u8>(), len, len) });
}

/// 释放 [`gf_room_step`] 等返回的 JSON 字符串。
///
/// # Safety
///
/// `ptr` 必须是本 crate 的 [`out`] 返回、且尚未释放的指针。每个导出字符串只能
/// free 一次；free 后指针不得再被解引用。JS 侧应把「调用 → TextDecoder 解码 →
/// gf_str_free」包成一个同步辅助函数，避免异常路径漏掉 free。
#[no_mangle]
pub unsafe extern "C" fn gf_str_free(ptr: *mut u8) {
    if ptr.is_null() {
        return;
    }
    // SAFETY: `out` 在 bytes 起点前固定放了 8 字节长度头；调用方保证指针有效且唯一。
    unsafe {
        let base = ptr.sub(OUT_HEADER);
        let len = u64::from_le_bytes(*(base.cast::<[u8; OUT_HEADER]>())) as usize;
        let total = OUT_HEADER + len;
        drop(Box::from_raw(std::ptr::slice_from_raw_parts_mut(
            base, total,
        )));
    }
}

// ---------------- 房间 ----------------

/// 建桌：`human_mask` 为 1 的座位是人类，其余挂 AI-L2（默认最智能档）。
#[no_mangle]
pub extern "C" fn gf_room_new(seed_hi: u32, seed_lo: u32, human_mask: u32) -> u8 {
    gf_room_new_level(seed_hi, seed_lo, human_mask, 2)
}

/// 按档位建桌：`bot_level` 0/1/2 = AI-L0/L1/L2（其他人用值按 L2 处理）。
#[no_mangle]
pub extern "C" fn gf_room_new_level(
    seed_hi: u32,
    seed_lo: u32,
    human_mask: u32,
    bot_level: u8,
) -> u8 {
    let seed = join_seed(seed_hi, seed_lo);
    let players: Vec<PlayerKind> = (0..4u8)
        .map(|s| {
            if human_mask & (1 << s) != 0 {
                PlayerKind::Human
            } else {
                PlayerKind::Bot
            }
        })
        .collect();
    let cfg = RoomConfig {
        match_id: format!("web-{seed:016x}"),
        ruleset_version: DEFAULT_RULESET_VERSION.to_owned(),
        seed,
        players,
    };
    let mut r = match Room::new(cfg) {
        Ok(r) => r,
        Err(_) => return 0,
    };
    for seat in 0..4u8 {
        if human_mask & (1 << seat) == 0 && r.attach_bot(seat, make_bot(bot_level)).is_err() {
            return 0;
        }
    }
    replace_room(Some(r));
    1
}

/// 档位 → bot 实现（设计 §5.11 的 easy/normal/hard 三档）。
fn make_bot(level: u8) -> Box<dyn gf_table_runtime::BotPolicy> {
    match level {
        0 => Box::new(gf_ai::L0Bot),
        1 => Box::new(gf_ai::L1Bot),
        _ => Box::new(gf_ai::L2Bot),
    }
}

/// 发牌开局。
#[no_mangle]
pub extern "C" fn gf_room_start(now_ms: u64) -> u8 {
    match room_mut() {
        Some(mut r) => u8::from(r.start(now_ms).is_ok()),
        None => 0,
    }
}

/// 推进一步：bot 行动、托管、结算、场次终止都在这里出。
#[no_mangle]
pub extern "C" fn gf_room_step(now_ms: u64) -> *mut u8 {
    let Some(mut r) = room_mut() else {
        return err("no room");
    };
    match r.step(now_ms) {
        Ok(diff) => out(outcome_json(&r, diff)),
        Err(e) => err(e),
    }
}

/// 人类提交动作。
///
/// # Safety
///
/// `cards_ptr` 必须指向 [`gf_alloc`] 登记、至少 `cards_len * 2` 字节且按 u16 对齐的块；
/// `cards_len` 为 0 时可为空指针。函数只读这段内存，不接管所有权。
#[no_mangle]
pub unsafe extern "C" fn gf_room_submit(
    seat: u8,
    now_ms: u64,
    is_pass: u8,
    cards_ptr: *const u16,
    cards_len: usize,
) -> *mut u8 {
    let Some(mut r) = room_mut() else {
        return err("no room");
    };
    let action = make_action(is_pass, read_cards(cards_ptr, cards_len));
    match r.submit(seat, action, now_ms) {
        Ok(diff) => out(outcome_json(&r, diff)),
        Err(e) => err(e),
    }
}

/// 续局重演单条录制动作（不校验玩家类型、不推进时钟）。
///
/// # Safety
///
/// 同 [`gf_room_submit`]：`cards_ptr` 必须落在 [`gf_alloc`] 登记块内，长度为
/// `cards_len * 2` 字节且按 u16 对齐；为 0 时可为空指针。
#[no_mangle]
pub unsafe extern "C" fn gf_room_replay_action(
    seat: u8,
    is_pass: u8,
    cards_ptr: *const u16,
    cards_len: usize,
) -> u8 {
    let Some(mut r) = room_mut() else {
        return 0;
    };
    let action = make_action(is_pass, read_cards(cards_ptr, cards_len));
    u8::from(r.replay_apply(seat, action).is_ok())
}

/// 再来一局（结算态调用）。
#[no_mangle]
pub extern "C" fn gf_room_next_round(now_ms: u64) -> *mut u8 {
    let Some(mut r) = room_mut() else {
        return err("no room");
    };
    match r.next_round(now_ms) {
        Ok(()) => out(format!(
            "{{\"kind\":\"dealt\",\"roundNo\":{},\"roomSeq\":{}}}",
            r.state().map(|s| s.round_no).unwrap_or(0),
            r.room_seq()
        )),
        Err(e) => err(e),
    }
}

/// 重开当前行动窗口（页面重载 / 杀进程后回放完调用，否则窗口 deadline 停在 0 会立即托管）。
#[no_mangle]
pub extern "C" fn gf_room_resume_window(now_ms: u64) -> u8 {
    let Some(mut r) = room_mut() else {
        return 0;
    };
    u8::from(r.resume_window(now_ms).is_ok())
}

/// 座位视图（canonical JSON 原样透传）。
#[no_mangle]
pub extern "C" fn gf_room_view(seat: u8) -> *mut u8 {
    let Some(r) = room() else {
        return err("no room");
    };
    match r.view_bytes(seat) {
        Ok(v) => out(String::from_utf8_lossy(&v).to_string()),
        Err(e) => err(e),
    }
}

/// 权威 `stateHash`（32 字节转 hex）。
#[no_mangle]
pub extern "C" fn gf_room_hash() -> *mut u8 {
    let Some(r) = room() else {
        return err("no room");
    };
    match r.state_hash() {
        Ok(h) => out(hex(&h)),
        Err(e) => err(e),
    }
}

/// 回放自证哈希：从首局起点按全量日志重放，应与 [`gf_room_hash`] 相等。
#[no_mangle]
pub extern "C" fn gf_room_replay_hash() -> *mut u8 {
    let Some(r) = room() else {
        return err("no room");
    };
    match r.replay_hash() {
        Ok(h) => out(hex(&h)),
        Err(e) => err(e),
    }
}

/// 房间元信息：当前局号、广播水位、已落动作条数。
#[no_mangle]
pub extern "C" fn gf_room_meta() -> *mut u8 {
    let Some(r) = room() else {
        return err("no room");
    };
    out(format!(
        "{{\"roundNo\":{},\"roomSeq\":{},\"records\":{}}}",
        r.state().map(|s| s.round_no).unwrap_or(0),
        r.room_seq(),
        r.log().len()
    ))
}

/// 全量日志里的最后一条动作（结算那一手由 `Settled` 分支带出，前端要补记它）。
/// 无记录时返回 `null`。
#[no_mangle]
pub extern "C" fn gf_room_last_record() -> *mut u8 {
    let Some(r) = room() else {
        return err("no room");
    };
    let Some(rec) = r.log().last() else {
        return out("null".to_owned());
    };
    out(format!(
        "{{\"roundNo\":{},\"roomSeq\":{},\"seat\":{},\"action\":{}}}",
        rec.round_no,
        rec.room_seq,
        rec.seat,
        action_json(&rec.action)
    ))
}

/// 座位身份（前端渲染用）：人类 / bot。
#[no_mangle]
pub extern "C" fn gf_room_seats() -> *mut u8 {
    let Some(r) = room() else {
        return err("no room");
    };
    let mut s = String::from("{\"seats\":[");
    for seat in 0..4u8 {
        if seat > 0 {
            s.push(',');
        }
        s.push_str(&format!(
            "{{\"seat\":{seat},\"human\":{}}}",
            r.is_human(seat)
        ));
    }
    s.push_str("]}");
    out(s)
}

/// 预检一手牌能否出（「出牌」按钮的判定源）：`{"ok":true,"patternId":"straight-5"}`，
/// 不可出时 `{"ok":false,"reason":"…"}`。与 `gf_room_submit` 的判定口径一致，
/// 但不推进状态；`gf_room_legal` 只是候选清单，顺子/三带二的具体花色组合未必都在里面。
///
/// # Safety
///
/// `cards_ptr` 必须落在 [`gf_alloc`] 登记块内，至少 `cards_len * 2` 字节且按 u16
/// 对齐；为 0 时可为空指针。只读，不接管所有权。
#[no_mangle]
pub unsafe extern "C" fn gf_room_can_play(cards_ptr: *const u16, cards_len: usize) -> *mut u8 {
    let Some(r) = room() else {
        return err("no room");
    };
    let cards = read_cards(cards_ptr, cards_len);
    let state = match r.state() {
        Ok(s) => s,
        Err(e) => return err(e),
    };
    match r.engine().preview_play(state, &cards) {
        Ok((pattern, beats)) => out(format!(
            "{{\"ok\":{},\"patternId\":{},\"reason\":{}}}",
            beats,
            json_str(&pattern),
            json_str(if beats { "" } else { "压不过上家" })
        )),
        Err(e) => out(format!(
            "{{\"ok\":false,\"reason\":{}}}",
            json_str(&format!("{e:?}"))
        )),
    }
}

/// 当前行动座位的合法动作集（`[{"pass":…,"patternId":"…","cards":[…]}]`）；
/// 「提示」选牌、理牌分流都按它。`patternId` 由 `preview_play` 顺带判定，不改枚举结果。
#[no_mangle]
pub extern "C" fn gf_room_legal() -> *mut u8 {
    let Some(r) = room() else {
        return err("no room");
    };
    let state = r.state().ok();
    match r.legal_now() {
        Ok(legal) => {
            let items: Vec<String> = legal
                .iter()
                .map(|a| {
                    let pid = match (a, state) {
                        (Action::Play { cards }, Some(s)) => r
                            .engine()
                            .preview_play(s, cards)
                            .map(|(pid, _)| pid)
                            .unwrap_or_default(),
                        _ => String::new(),
                    };
                    action_json_patterned(a, &pid)
                })
                .collect();
            out(format!("[{}]", items.join(",")))
        }
        Err(e) => err(e),
    }
}

/// 当前行动座位的一手牌「推荐拆分」（`recommend`，纯只读、不进 `stateHash`）。
///
/// 返回 `{"ok":true,"seat":2,"score":…,"singleCount":…,"groups":[{"patternId":…,"cards":[…]}]}`。
/// bot 出牌规划与页面「组牌建议」共用同一份拆分：先走最小的组、不拆对子。
#[no_mangle]
pub extern "C" fn gf_room_plan() -> *mut u8 {
    let Some(r) = room() else {
        return err("no room");
    };
    let state = match r.state() {
        Ok(s) => s,
        Err(e) => return err(e),
    };
    let seat = state.turn_seat;
    let layout = match r.engine().recommend(state, gf_engine::contract::Seat(seat)) {
        Ok(l) => l,
        Err(e) => return err(e),
    };
    let groups: Vec<String> = layout
        .groups
        .iter()
        .map(|g| {
            format!(
                "{{\"patternId\":{},\"cards\":[{}]}}",
                json_str(&g.pattern_id),
                g.cards
                    .iter()
                    .map(|c| c.to_string())
                    .collect::<Vec<_>>()
                    .join(",")
            )
        })
        .collect();
    out(format!(
        "{{\"ok\":true,\"seat\":{},\"score\":{},\"singleCount\":{},\"groups\":[{}]}}",
        seat,
        layout.score,
        layout.single_count,
        groups.join(",")
    ))
}

/// 桌面公开信息（不进座位视图、不影响 `stateHash`）：相位、行动座位、连续过牌数、
/// 双方级牌、打级队、各家手牌数与已锁定名次，以及本局进贡 / 还贡事件与待还贡牌。
#[no_mangle]
pub extern "C" fn gf_room_info() -> *mut u8 {
    let Some(r) = room() else {
        return err("no room");
    };
    let Ok(state) = r.state() else {
        return out("{\"phase\":\"idle\"}".to_owned());
    };
    let hand_idx = r
        .engine()
        .ruleset
        .zones
        .iter()
        .position(|z| z.id.0 == "hand")
        .unwrap_or(0) as u16;
    let mut seats = String::new();
    for seat in 0..4u8 {
        if seat > 0 {
            seats.push(',');
        }
        let hand: usize = state
            .zones
            .iter()
            .filter(|z| z.seat == seat && z.zone_id == hand_idx)
            .map(|z| z.cards.len())
            .sum();
        let rank = state
            .finish_order
            .iter()
            .position(|&x| x == seat)
            .map(|p| p + 1)
            .unwrap_or(0);
        seats.push_str(&format!(
            "{{\"seat\":{seat},\"human\":{},\"hand\":{hand},\"rank\":{rank}}}",
            r.is_human(seat)
        ));
    }
    let levels: Vec<String> = state.levels.iter().map(u8::to_string).collect();
    let phase = phase_name(state.phase);
    let mut pending = String::new();
    for (i, p) in state.tribute_pending_returns.iter().enumerate() {
        if i > 0 {
            pending.push(',');
        }
        let eligible: Vec<String> = if p.receiver == state.turn_seat {
            r.engine()
                .tribute_return_cards(state)
                .into_iter()
                .map(|c| c.to_string())
                .collect()
        } else {
            Vec::new()
        };
        pending.push_str(&format!(
            "{{\"receiver\":{},\"payer\":{},\"eligible\":[{}]}}",
            p.receiver,
            p.payer,
            eligible.join(",")
        ));
    }
    let mut events = String::new();
    for (i, e) in state.tribute_events.iter().enumerate() {
        if i > 0 {
            events.push(',');
        }
        events.push_str(&format!(
            "{{\"from\":{},\"to\":{},\"card\":{},\"return\":{}}}",
            e.from, e.to, e.card, e.is_return
        ));
    }
    out(format!(
        "{{\"phase\":\"{phase}\",\"roundNo\":{},\"turnSeat\":{},\"passesSince\":{},\"levels\":[{}],\"attackingTeam\":{},\"seats\":[{}],\"tribute\":{{\"active\":{},\"resisted\":{},\"leader\":{},\"pending\":[{}],\"events\":[{}]}}}}",
        state.round_no,
        state.turn_seat,
        state.passes_since_last_play,
        levels.join(","),
        state.attacking_team,
        seats,
        state.phase == 1,
        state.tribute_resisted,
        opt_u8(state.tribute_leader),
        pending,
        events
    ))
}

/// bot 决策入口自证：对**当前行动座位**走 AI-L2 决策，返回动作 JSON。
///
/// 视图/合法集/搭档上下文必须同属一个座位；非当前行动座位直接报错，避免拿 A 的合法集
/// 套 B 的视图算出错乱动作。实现是 L2Bot，名字不再叫 L0。
#[no_mangle]
pub extern "C" fn gf_l2_probe(seat: u8) -> *mut u8 {
    let Some(r) = room() else {
        return err("no room");
    };
    let turn = match r.state() {
        Ok(state) => state.turn_seat,
        Err(e) => return err(e),
    };
    if seat != turn {
        return err("seat is not current turn");
    }
    let view = match r.view_bytes(seat) {
        Ok(v) => v,
        Err(e) => return err(e),
    };
    let legal = match r.legal_now() {
        Ok(l) => l,
        Err(e) => return err(e),
    };
    let ctx = r.bot_context(seat);
    let bot = gf_ai::L2Bot;
    let action = bot.decide_with_ctx(&view, seat, &legal, &ctx);
    out(action_json(&action))
}

/// 兼容旧调用名；行为等同 [`gf_l2_probe`]。
#[no_mangle]
pub extern "C" fn gf_l0_probe(seat: u8) -> *mut u8 {
    gf_l2_probe(seat)
}

// ---------------- 牌面主题（gf-deckart 运行时合成） ----------------
//
// 客户端只拿 `.gfdeck` 字节（`Uint8Array`），在 wasm 里解密 + 组装主题，牌面 SVG 由
// `gf-deckart` 现场生成，与 CLI `gfdeck render` 逐字节一致。三条导出：
// - `gf_theme_load(pack, pub)`  加载主题包（公钥长度 0 = 跳过验签）
// - `gf_theme_preview()`        取「主题选择界面」要的那几张（`theme.json` 的 `preview` 配）
// - `gf_theme_card(code, level, cornerPct)` 取任意一张（牌桌用，`level` 由服务端下发）

static DECK: Global<Option<gf_deckart::Theme>> = Global::new(None);

fn deck() -> Option<SlotRef<gf_deckart::Theme>> {
    #[cfg(target_arch = "wasm32")]
    {
        // SAFETY: 单线程 wasm；只取一次裸指针，随后由 `SlotRef` 独占。
        let slot = unsafe { &mut *DECK.0.get() };
        slot.is_some().then_some(SlotRef { slot })
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let guard = DECK.0.lock().unwrap_or_else(|e| e.into_inner());
        guard.is_some().then_some(SlotRef { guard })
    }
}

fn replace_deck(theme: Option<gf_deckart::Theme>) {
    #[cfg(target_arch = "wasm32")]
    {
        // SAFETY: 单线程 wasm；替换整个槽，不留下别名。
        unsafe { *DECK.0.get() = theme };
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        *DECK.0.lock().unwrap_or_else(|e| e.into_inner()) = theme;
    }
}

/// 把宿主传进来的字节块读成 slice；长度 0 视为空。
fn bytes_at(ptr: *const u8, len: usize) -> &'static [u8] {
    if len == 0 {
        return &[];
    }
    if !input_range_ok(ptr, len, 1, MAX_INPUT_BYTES) {
        return &[];
    }
    // SAFETY: 上方已确认整段落在 `gf_alloc` 登记且地址+长度不溢出的块内。
    unsafe { std::slice::from_raw_parts(ptr, len) }
}

fn card_json(code: &str, svg: &str) -> String {
    format!("{{\"code\":{},\"svg\":{}}}", json_str(code), json_str(svg))
}

fn theme_err(e: impl std::fmt::Debug) -> *mut u8 {
    out(format!(
        "{{\"ok\":false,\"error\":{}}}",
        json_str(&format!("{e:?}"))
    ))
}

/// 加载 `.gfdeck` 主题包。`pub_len` 为 32 时验签，为 0 时跳过（开发期未签名包）。
/// 返回 `{"ok":true,"themeId","version","name","license","layout","cornerScale","bytes","preview":[...]}`，
/// 其中 `cornerScale` 是主题声明的默认角标倍率（`null` = 主题没声明，由宿主自己兜底），
/// `preview` 是 `[{"code","svg"}]`，省掉客户端再调一次 `gf_theme_preview`。
///
/// # Safety
///
/// `pack_ptr` 必须落在 [`gf_alloc`] 登记、至少 `pack_len` 字节的块内（`pack_len` 为 0
/// 时可传空指针）；`pub_ptr` 同理，`pub_len` 应为 0 或 32。函数只读这两段内存。
#[no_mangle]
pub unsafe extern "C" fn gf_theme_load(
    pack_ptr: *const u8,
    pack_len: usize,
    pub_ptr: *const u8,
    pub_len: usize,
) -> *mut u8 {
    // 跨 FFI 边界 panic 会 abort 整个 wasm 实例，任何输入都只能返回错误串。
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        theme_load_impl(pack_ptr, pack_len, pub_ptr, pub_len)
    }))
    .unwrap_or_else(|_| theme_err("theme load panicked"))
}

/// 生产入口：必须提供公钥，签名包缺失/伪造一律拒绝（不提供 `gf_theme_load` 的无密钥降级）。
///
/// # Safety
///
/// 同 [`gf_theme_load`]：`pack_ptr`/`pub_ptr` 必须落在 [`gf_alloc`] 登记块内，
/// 长度分别不少于 `pack_len`/`pub_len`；`pub_len` 必须为 32（为 0 直接返回错误）。
#[no_mangle]
pub unsafe extern "C" fn gf_theme_load_verified(
    pack_ptr: *const u8,
    pack_len: usize,
    pub_ptr: *const u8,
    pub_len: usize,
) -> *mut u8 {
    if pub_len == 0 {
        return theme_err("public key required");
    }
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        theme_load_impl(pack_ptr, pack_len, pub_ptr, pub_len)
    }))
    .unwrap_or_else(|_| theme_err("theme load panicked"))
}

unsafe fn theme_load_impl(
    pack_ptr: *const u8,
    pack_len: usize,
    pub_ptr: *const u8,
    pub_len: usize,
) -> *mut u8 {
    let pack = bytes_at(pack_ptr, pack_len);
    if pack.is_empty() {
        return theme_err("empty pack");
    }
    let pubkey = if pub_len == 0 {
        None
    } else {
        match gf_deckart::pack::verifying_key(bytes_at(pub_ptr, pub_len)) {
            Ok(vk) => Some(vk),
            Err(e) => return theme_err(e),
        }
    };
    let (key, _kid) = gf_deckart::pack::builtin_key();
    let theme = match gf_deckart::pack::open_bytes(pack, &key, pubkey.as_ref()) {
        Ok(t) => t,
        Err(e) => return theme_err(e),
    };
    replace_deck(Some(theme));
    let Some(t) = deck() else {
        return theme_err("theme disappeared");
    };
    let meta = t.meta();
    let layout = match meta.layout {
        gf_deckart::Layout::Compact => "compact",
        gf_deckart::Layout::Classic => "classic",
    };
    let (faces, level) = match t.preview() {
        Ok(v) => v,
        Err(e) => return theme_err(e),
    };
    let codes: Vec<String> = faces.iter().map(|f| json_str(&f.code())).collect();
    let preview = match gf_deckart::render_preview(&t) {
        Ok(v) => v
            .iter()
            .map(|r| card_json(&r.code, &r.svg))
            .collect::<Vec<_>>()
            .join(","),
        Err(e) => return theme_err(e),
    };
    out(format!(
        "{{\"ok\":true,\"themeId\":{},\"version\":{},\"name\":{},\"license\":{},\"layout\":\"{layout}\",\"cornerScale\":{},\"bytes\":{},\"levelRank\":{},\"faces\":[{}],\"preview\":[{}]}}",
        json_str(&meta.id),
        meta.version,
        json_str(&meta.name),
        json_str(&meta.license),
        meta.corner_scale
            .map(num_json)
            .unwrap_or_else(|| "null".to_string()),
        pack.len(),
        level.map(|l| l.level_rank.value()).unwrap_or(0),
        codes.join(","),
        preview
    ))
}

/// 主题选择界面的预览（`theme.json` 的 `preview.faces` / `preview.levelRank` 决定）。
/// 返回 `{"ok":true,"cards":[{"code","svg"}...]}`。
#[no_mangle]
pub extern "C" fn gf_theme_preview() -> *mut u8 {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(theme_preview_impl))
        .unwrap_or_else(|_| theme_err("theme preview panicked"))
}

fn theme_preview_impl() -> *mut u8 {
    let Some(t) = deck() else {
        return theme_err("no theme loaded");
    };
    match gf_deckart::render_preview(&t) {
        Ok(v) => {
            let cards: Vec<String> = v.iter().map(|r| card_json(&r.code, &r.svg)).collect();
            out(format!("{{\"ok\":true,\"cards\":[{}]}}", cards.join(",")))
        }
        Err(e) => theme_err(e),
    }
}

/// 单张牌面。`level_rank` 传 0 表示「不带级牌态」（主题选择页看纯牌面时用）；
/// `corner_pct` 是角标字号百分比（100 = 素材原样，`0` = 用主题声明的默认倍率）。
/// 返回 `{"ok":true,"code","svg"}`。缓存键建议 `(themeId, themeVersion, code, levelRank, cornerPct)`。
///
/// # Safety
///
/// `code_ptr` 必须落在 [`gf_alloc`] 登记、至少 `code_len` 字节的块内；`code_len` 为 0
/// 时可传空指针。函数只读这段内存。
#[no_mangle]
pub unsafe extern "C" fn gf_theme_card(
    code_ptr: *const u8,
    code_len: usize,
    level_rank: u8,
    corner_pct: u8,
) -> *mut u8 {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        theme_card_impl(code_ptr, code_len, level_rank, corner_pct)
    }))
    .unwrap_or_else(|_| theme_err("theme card panicked"))
}

unsafe fn theme_card_impl(
    code_ptr: *const u8,
    code_len: usize,
    level_rank: u8,
    corner_pct: u8,
) -> *mut u8 {
    let Some(t) = deck() else {
        return theme_err("no theme loaded");
    };
    let code = match std::str::from_utf8(bytes_at(code_ptr, code_len)) {
        Ok(s) => s,
        Err(_) => return theme_err("code is not utf8"),
    };
    let face = match gf_deckart::Face::parse(code) {
        Ok(f) => f,
        Err(e) => return theme_err(e),
    };
    let level = if level_rank == 0 {
        None
    } else {
        match gf_deckart::Rank::new(level_rank) {
            Ok(r) => Some(gf_deckart::LevelState::new(r)),
            Err(e) => return theme_err(e),
        }
    };
    match gf_deckart::render_face_opts(
        &t,
        face,
        gf_deckart::FaceOptions {
            level,
            corner_scale: if corner_pct == 0 {
                t.corner_scale()
            } else {
                f64::from(corner_pct) / 100.0
            },
        },
    ) {
        Ok(svg) => out(format!(
            "{{\"ok\":true,\"code\":{},\"svg\":{}}}",
            json_str(&face.code()),
            json_str(&svg)
        )),
        Err(e) => theme_err(e),
    }
}

/// 牌背（跟级牌无关，一个主题只有一张）。返回 `{"ok":true,"svg"}`。
#[no_mangle]
pub extern "C" fn gf_theme_back() -> *mut u8 {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(theme_back_impl))
        .unwrap_or_else(|_| theme_err("theme back panicked"))
}

fn theme_back_impl() -> *mut u8 {
    let Some(t) = deck() else {
        return theme_err("no theme loaded");
    };
    match gf_deckart::render_back(&t) {
        Ok(svg) => out(format!("{{\"ok\":true,\"svg\":{}}}", json_str(&svg))),
        Err(e) => theme_err(e),
    }
}

/// 卸载当前主题（换主题前先断引用，避免同时挂两份素材）。
#[no_mangle]
pub extern "C" fn gf_theme_unload() -> u8 {
    replace_deck(None);
    1
}

#[cfg(test)]
mod memory_tests {
    use super::*;

    #[test]
    fn alloc_ledger_rejects_unknown_and_oversize_ranges() {
        assert!(gf_alloc(0).is_null());
        assert!(gf_alloc(MAX_INPUT_BYTES + 1).is_null());
        let p = gf_alloc(8);
        assert!(!p.is_null());
        let p8 = p.cast::<u8>();
        assert!(input_range_ok(p8, 8, 1, MAX_INPUT_BYTES));
        assert!(
            !input_range_ok(p8, 9, 1, MAX_INPUT_BYTES),
            "长度不能超过分配块"
        );
        assert!(
            !input_range_ok(0x1000 as *const u8, 8, 1, MAX_INPUT_BYTES),
            "未登记指针一律拒绝"
        );
        unsafe { gf_mem_free(p, 8) };
    }

    #[test]
    fn output_string_is_independent_and_freeable() {
        let p = out("hello".to_string());
        assert!(!p.is_null());
        // SAFETY: `out` 返回以 NUL 结尾的块，长度头在 p 前 8 字节。
        let len = unsafe { u64::from_le_bytes(*(p.sub(OUT_HEADER).cast::<[u8; 8]>())) };
        assert_eq!(len, 6, "5 字节正文 + NUL");
        // SAFETY: 同上；只读，不越过块尾。
        let text = unsafe { std::slice::from_raw_parts(p, len as usize) };
        assert_eq!(text, b"hello\0");
        unsafe { gf_str_free(p) };
    }
}
