//! 浏览器 wasm 绑定层：把 `gf-table-runtime` 的房间 + `gf-ai` 的 L0 bot 暴露成
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

#![allow(clippy::missing_safety_doc)]

use std::cell::UnsafeCell;
use std::ffi::c_void;

use gf_engine::contract::Action;
use gf_table_runtime::{
    BotPolicy, PlayerKind, Room, RoomConfig, StepOutcome, DEFAULT_RULESET_VERSION,
};

/// `wasm32-unknown-unknown` 是单线程环境；用零开销的全局可变槽代替 `Mutex`。
struct Global<T>(UnsafeCell<T>);

// SAFETY: 目标平台单线程，且所有入口都在同一条 JS 调用栈上串行执行。
unsafe impl<T> Sync for Global<T> {}

static ROOM: Global<Option<Room>> = Global(UnsafeCell::new(None));
static STRINGS: Global<Vec<Vec<u8>>> = Global(UnsafeCell::new(Vec::new()));

fn room() -> &'static mut Option<Room> {
    unsafe { &mut *ROOM.0.get() }
}

/// 把一段文本变成以 NUL 结尾的堆字符串并登记，供 [`gf_str_free`] 回收。
fn out(s: String) -> *mut u8 {
    let mut buf = s.into_bytes();
    if buf.contains(&0) {
        buf.retain(|b| *b != 0);
    }
    buf.push(0);
    let ptr = buf.as_mut_ptr();
    unsafe {
        (*STRINGS.0.get()).push(buf);
    }
    ptr
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

fn outcome_json(r: &Room, diff: StepOutcome) -> String {
    let round_no = r.state().map(|s| s.round_no).unwrap_or(0);
    match diff {
        StepOutcome::Waiting { seat, deadline_ms } => format!(
            "{{\"kind\":\"waiting\",\"seat\":{seat},\"deadlineMs\":{deadline_ms},\"roundNo\":{round_no}}}"
        ),
        StepOutcome::Applied { seat, action, auto } => format!(
            "{{\"kind\":\"applied\",\"seat\":{},\"action\":{},\"auto\":{},\"roundNo\":{},\"roomSeq\":{}}}",
            seat,
            action_json(&action),
            auto_json(auto),
            round_no,
            r.room_seq()
        ),
        StepOutcome::Settled(_) => format!(
            "{{\"kind\":\"settled\",\"roundNo\":{},\"roomSeq\":{},\"summary\":{}}}",
            round_no,
            r.room_seq(),
            summary_json(r)
        ),
        StepOutcome::MatchEnded(end) => format!(
            "{{\"kind\":\"matchEnd\",\"roundNo\":{},\"winnerTeam\":{},\"summary\":{}}}",
            round_no,
            opt_u8(end.winner_team),
            summary_json(r)
        ),
    }
}

fn read_cards(ptr: *const u16, len: usize) -> Vec<u16> {
    if ptr.is_null() || len == 0 {
        return Vec::new();
    }
    // SAFETY: 宿主写入的内存块由 `gf_alloc` 分配，长度由调用方传入。
    unsafe { std::slice::from_raw_parts(ptr, len) }.to_vec()
}

fn make_action(is_pass: u8, cards: Vec<u16>) -> Action {
    if is_pass != 0 {
        Action::Pass
    } else {
        Action::Play { cards }
    }
}

// ---------------- 内存与字符串 ----------------

/// 分配 `len` 字节供宿主写入（牌 id 数组等）。宿主用完调 [`gf_mem_free`]。
#[no_mangle]
pub extern "C" fn gf_alloc(len: usize) -> *mut c_void {
    let mut buf = vec![0u8; len];
    let ptr = buf.as_mut_ptr().cast::<c_void>();
    std::mem::forget(buf);
    ptr
}

/// 释放 [`gf_alloc`] 分配的内存（容量按 `len` 还原）。
#[no_mangle]
pub unsafe extern "C" fn gf_mem_free(ptr: *mut c_void, len: usize) {
    if ptr.is_null() || len == 0 {
        return;
    }
    drop(Vec::from_raw_parts(ptr.cast::<u8>(), len, len));
}

/// 释放 [`gf_room_step`] 等返回的 JSON 字符串。
#[no_mangle]
pub extern "C" fn gf_str_free(ptr: *mut u8) {
    if ptr.is_null() {
        return;
    }
    unsafe {
        let list = &mut *STRINGS.0.get();
        list.retain(|v| v.as_ptr() != ptr);
    }
}

// ---------------- 房间 ----------------

/// 建桌：`human_mask` 为 1 的座位是人类，其余挂 L0 bot（AI-L0，只看 view(seat)）。
#[no_mangle]
pub extern "C" fn gf_room_new(seed_hi: u32, seed_lo: u32, human_mask: u32) -> u8 {
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
        if human_mask & (1 << seat) == 0
            && r.attach_bot(seat, Box::new(gf_ai::L0Bot)).is_err()
        {
            return 0;
        }
    }
    *room() = Some(r);
    1
}

/// 发牌开局。
#[no_mangle]
pub extern "C" fn gf_room_start(now_ms: u64) -> u8 {
    match room() {
        Some(r) => u8::from(r.start(now_ms).is_ok()),
        None => 0,
    }
}

/// 推进一步：bot 行动、托管、结算、场次终止都在这里出。
#[no_mangle]
pub extern "C" fn gf_room_step(now_ms: u64) -> *mut u8 {
    let Some(r) = room() else {
        return err("no room");
    };
    match r.step(now_ms) {
        Ok(diff) => out(outcome_json(r, diff)),
        Err(e) => err(e),
    }
}

/// 人类提交动作。
#[no_mangle]
pub unsafe extern "C" fn gf_room_submit(
    seat: u8,
    now_ms: u64,
    is_pass: u8,
    cards_ptr: *const u16,
    cards_len: usize,
) -> *mut u8 {
    let Some(r) = room() else {
        return err("no room");
    };
    let action = make_action(is_pass, read_cards(cards_ptr, cards_len));
    match r.submit(seat, action, now_ms) {
        Ok(diff) => out(outcome_json(r, diff)),
        Err(e) => err(e),
    }
}

/// 续局重演单条录制动作（不校验玩家类型、不推进时钟）。
#[no_mangle]
pub unsafe extern "C" fn gf_room_replay_action(
    seat: u8,
    is_pass: u8,
    cards_ptr: *const u16,
    cards_len: usize,
) -> u8 {
    let Some(r) = room() else {
        return 0;
    };
    let action = make_action(is_pass, read_cards(cards_ptr, cards_len));
    u8::from(r.replay_apply(seat, action).is_ok())
}

/// 再来一局（结算态调用）。
#[no_mangle]
pub extern "C" fn gf_room_next_round(now_ms: u64) -> *mut u8 {
    let Some(r) = room() else {
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
    let Some(r) = room() else {
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

/// bot 决策入口自证：让 L0 直接对指定座位当前视图决策，返回动作 JSON（诊断用）。
#[no_mangle]
pub extern "C" fn gf_l0_probe(seat: u8) -> *mut u8 {
    let Some(r) = room() else {
        return err("no room");
    };
    match r.view_bytes(seat) {
        Ok(v) => {
            let bot = gf_ai::L0Bot;
            let action = bot.decide(&v, seat);
            out(action_json(&action))
        }
        Err(e) => err(e),
    }
}
