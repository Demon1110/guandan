//! §2.4 M0 清单第 3 条、§10.5 第 5 行「空转压测」：1500 房间 × 10Hz 空转无出牌。
//!
//! §3.2 推导：A4 峰值 500 房间 × 3 倍余量 = 1500 房间，tick 10Hz。本基准实测两组数字：
//!
//! 1. **常驻堆内存**：统计型 `#[global_allocator]` 记录 alloc/dealloc/realloc 差值，
//!    1500 个互相独立的 `State` deal 完成后的 live 增量（不读 RSS、不用 `size_of` 推算）；
//! 2. **空转 tick 的 CPU 成本**：`Instant` 实测整段总耗时 / (1500 × TICKS)，
//!    不写常量、不照抄单动作微秒推算。
//!
//! 计时风格与 `m0_bench.rs` 一致：`harness = false`，自写 `Instant` 循环，不引 criterion、
//! 不引新依赖。引擎 `src/` 判定链路不做任何改动；房间包装结构与记账字段只落在本文件。

use gf_engine::contract::State;
use gf_engine::engine::M0Engine;
use gf_engine::ruleset::from_yaml;
use std::alloc::{GlobalAlloc, Layout, System};
use std::hint::black_box;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

/// 房间数：§3.2 峰值 500 × 3 余量。
const ROOMS: usize = 1500;

/// 每轮 tick 数：10Hz 下的 10 秒墙钟（10 × 10）。
const TICKS: u32 = 100;

/// 重复计时轮次。
const ROUNDS: usize = 5;

/// 确定性种子基线：每房间 `BASE_SEED + i`，不用时间戳 / 线程随机。
const BASE_SEED: u64 = 0xA5A5_0000_u64;

// ---------- 统计型全局分配器 ----------

/// 只做字节计数，自身不额外分配内存（两个 `AtomicUsize` 内联在静态量里）。
struct TrackingAlloc {
    /// 当前 live 堆字节数（相对清零基线）。
    current: AtomicUsize,
    /// 区间内 current 峰值（相对清零基线）。
    peak: AtomicUsize,
}

unsafe impl GlobalAlloc for TrackingAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = System.alloc(layout);
        if !ptr.is_null() {
            let live = self.current.fetch_add(layout.size(), Ordering::Relaxed) + layout.size();
            self.peak.fetch_max(live, Ordering::Relaxed);
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        self.current.fetch_sub(layout.size(), Ordering::Relaxed);
        System.dealloc(ptr, layout);
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let old_size = layout.size();
        let new_ptr = System.realloc(ptr, layout, new_size);
        if !new_ptr.is_null() {
            let live = match new_size as isize - old_size as isize {
                diff if diff >= 0 => {
                    self.current.fetch_add(diff as usize, Ordering::Relaxed) + diff as usize
                }
                diff => {
                    self.current.fetch_sub((-diff) as usize, Ordering::Relaxed) - (-diff) as usize
                }
            };
            self.peak.fetch_max(live, Ordering::Relaxed);
        }
        new_ptr
    }
}

#[global_allocator]
static ALLOC: TrackingAlloc = TrackingAlloc {
    current: AtomicUsize::new(0),
    peak: AtomicUsize::new(0),
};

// ---------- 房间包装（记账字段只在包装层，不改 State） ----------

/// 房间包装：`State` 是引擎真源；tick 计数 / 到期时刻属于房间级记账，
/// `State` 内没有时间戳 / 回合计数字段（见 contract.rs 165–195 行），故挂在这里。
struct Room {
    state: State,
    /// 房间级 tick 计数，每空转 tick +1。
    tick: u64,
    /// 回合到期 tick（与 `tick` 做一次整数比较）。
    deadline_tick: u64,
}

/// 空转 tick：**不调 `legal_actions`、不调 `apply`、不产生任何动作**。
/// 逐项只做房间级最小记账：
/// 1. 房间 tick 计数器 +1；
/// 2. 与回合到期时刻做一次整数比较（`tick >= deadline`）；
/// 3. 读一次 `state.phase`；
/// 4. 读一次 `state.turn_seat`。
///
/// 读值汇入 `checksum`，防止编译期把只读路径优化掉。
/// **不含**网络 IO、计时器内核开销（epoll / timer wheel / 系统调用）、动作枚举与状态迁移。
fn idle_tick(room: &mut Room, checksum: &mut u64) {
    room.tick = room.tick.wrapping_add(1);
    let expired = room.tick >= room.deadline_tick;
    let phase = room.state.phase;
    let seat = room.state.turn_seat;
    *checksum = checksum
        .wrapping_add(room.tick)
        .wrapping_add(phase as u64)
        .wrapping_add(seat as u64)
        .wrapping_add(expired as u64);
}

/// 载入共享 RuleSet（只构造一份，不计入每房间内存）。
fn build_engine() -> M0Engine {
    let manifest = env!("CARGO_MANIFEST_DIR");
    let yaml_path = format!("{manifest}/tests/fixtures/m0-minimal/ruleset.yaml");
    let yaml = std::fs::read_to_string(&yaml_path)
        .unwrap_or_else(|e| panic!("读取夹具 ruleset.yaml 失败: {e}"));
    let body = from_yaml(&yaml).expect("夹具 ruleset.yaml 解析失败");
    M0Engine::new(body)
}

/// 中位数（最近秩法，与 m0_bench 同口径）。
fn median(samples: &[f64]) -> f64 {
    let mut sorted = samples.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).expect("基准样本含 NaN"));
    let n = sorted.len();
    if n % 2 == 1 {
        sorted[n / 2]
    } else {
        (sorted[n / 2 - 1] + sorted[n / 2]) / 2.0
    }
}

fn main() {
    let engine = build_engine();

    // ---------- 数字①：1500 房间常驻堆内存 ----------
    // 引擎与共享 RuleSet 构造在清零基线之前，不计入每房间。
    ALLOC.current.store(0, Ordering::Relaxed);
    ALLOC.peak.store(0, Ordering::Relaxed);

    let mut rooms: Vec<Room> = Vec::with_capacity(ROOMS);
    for i in 0..ROOMS {
        let seed = BASE_SEED + i as u64;
        let state = engine.deal(seed).expect("确定性种子发牌失败");
        rooms.push(Room {
            state,
            tick: 0,
            deadline_tick: TICKS as u64,
        });
    }

    // 自检断言：房间数与牌数合计，任一不满足直接退出，不打印内存数字。
    assert_eq!(
        rooms.len(),
        ROOMS,
        "房间数必须为 {ROOMS}，实际 {}",
        rooms.len()
    );
    let total_cards: usize = rooms
        .iter()
        .map(|r| r.state.zones.iter().map(|z| z.cards.len()).sum::<usize>())
        .sum();
    let expected_cards = ROOMS * 108;
    assert_eq!(
        total_cards, expected_cards,
        "牌数合计必须为 {expected_cards}（{ROOMS}×108），实际 {total_cards}"
    );

    let resident_bytes = ALLOC.current.load(Ordering::Relaxed);
    let peak_bytes = ALLOC.peak.load(Ordering::Relaxed);
    let bytes_per_room = resident_bytes as f64 / ROOMS as f64;
    let per_room_int = resident_bytes / ROOMS;

    println!("=== idle_1500 空转压测（§2.4 第 3 条 / §10.5 第 5 行 / §3.2） ===");
    println!("[内存] 房间数={ROOMS} 牌数合计={total_cards}（期望 {expected_cards}）");
    println!("[内存] 1500 房间常驻堆字节={resident_bytes} B，峰值(区间)={peak_bytes} B");
    println!("[内存] 每房间={bytes_per_room:.1} B/room（整数 {per_room_int} B/room）");

    // ---------- 数字②：空转 tick 的 CPU 成本 ----------
    let mut per_round_ns: Vec<f64> = Vec::with_capacity(ROUNDS);
    let mut checksum: u64 = 0;

    for round in 0..ROUNDS {
        let t0 = Instant::now();
        for _tick in 0..TICKS {
            for room in &mut rooms {
                idle_tick(room, &mut checksum);
            }
        }
        let elapsed_ns = t0.elapsed().as_nanos() as f64;
        let ns_per = elapsed_ns / (ROOMS as f64 * TICKS as f64);
        per_round_ns.push(ns_per);
        println!("[CPU] round {round}: 总耗时={elapsed_ns:.0} ns, {ns_per:.3} ns/(room·tick)");
    }
    black_box(checksum);

    let min = per_round_ns.iter().cloned().fold(f64::INFINITY, f64::min);
    let max = per_round_ns
        .iter()
        .cloned()
        .fold(f64::NEG_INFINITY, f64::max);
    let med = median(&per_round_ns);
    println!(
        "[CPU] 汇总: median={med:.3} min={min:.3} max={max:.3} ns/(room·tick)（{} 轮，{ROOMS} 房间 × {TICKS} tick/轮）",
        per_round_ns.len()
    );
}
