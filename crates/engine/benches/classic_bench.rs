//! `guandan-classic` 满牌型解释执行基准（§10.5 第 1505 行：M1 签核前必须在
//! classic 夹具上重跑同一基准，不得沿用 m0-minimal 地板数字）。
//!
//! 豁免边界（§3.3）：引擎 `src/` 内保持零时钟调用；本文件是唯一允许使用
//! `std::time::Instant` 的计时落点之一（另一处是 wasm Node 宿主）。
//! 不引 criterion 重依赖：`[[bench]] harness = false`，自写计时与最近秩分位数。
//!
//! 两组 legal 口径与 `m0_bench` 一致：`@mid` 中局跟牌态；`@lead` 领出态
//! （`last_play = None`，候选峰值，枚举 push + 排序成本最高）。
//! 容量主结论按 `@lead` + apply 均值折算；门禁：单动作 P99 < 1ms、单核 ≥1500 房间。

use gf_engine::cases::{classic_engine, classic_lead_state, classic_mid_state};
use gf_engine::contract::{Engine, Seat, State};
use gf_engine::engine::{M0Engine, PHASE_SETTLE};
use std::alloc::{GlobalAlloc, Layout, System};
use std::hint::black_box;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

/// 每侧迭代次数（§10.5：10 万次，不许减产）。
const ITERATIONS: u32 = 100_000;

/// 房间数（§3.2 峰值 500 × 3 余量 = 1500，与 idle_1500 同口径）。
const ROOMS: usize = 1500;

/// 最近秩法（nearest-rank）：rank = ceil(p/100 · n)，取排序后第 rank 个样本。
fn percentile(samples: &mut [f64], p: f64) -> f64 {
    samples.sort_by(|a, b| a.partial_cmp(b).expect("基准样本含 NaN"));
    let n = samples.len();
    let rank = ((p / 100.0) * n as f64).ceil() as usize;
    let rank = rank.clamp(1, n);
    samples[rank - 1]
}

/// 固定状态连续枚举 `ITERATIONS` 次；返回逐次 µs 样本与累计候选条数 checksum。
fn bench_legal(engine: &M0Engine, state: &State) -> (Vec<f64>, u64) {
    let mut samples = Vec::with_capacity(ITERATIONS as usize);
    let mut checksum: u64 = 0;
    for _ in 0..ITERATIONS {
        let t0 = Instant::now();
        let legal = engine
            .legal_actions(state)
            .expect("classic 枚举合法动作失败");
        let dt = t0.elapsed().as_nanos() as f64 / 1000.0;
        checksum = checksum.wrapping_add(legal.len() as u64);
        samples.push(dt);
    }
    black_box((state, checksum));
    (samples, checksum)
}

/// 组牌推荐 `recommend`：classic 领出态满手 27 张，逐次计时。样本量单独取
/// `REC_ITERATIONS`（推荐束搜索成本高于 legal 枚举，不与 10 万次 legal 同批次）。
/// §5.11 预算：单次 ≤2ms（P99）；超预算记 `recommend_cpu_overrun_total`（P2），
/// 不判错、不回滚。本仓当前无 metrics 承载位，只做计时断言，不埋全局单例计数。
const REC_ITERATIONS: u32 = 10_000;

fn bench_recommend(engine: &M0Engine, state: &State) -> (Vec<f64>, u64) {
    let mut samples = Vec::with_capacity(REC_ITERATIONS as usize);
    let mut checksum: u64 = 0;
    for _ in 0..REC_ITERATIONS {
        let t0 = Instant::now();
        let layout = engine
            .recommend(state, Seat(0))
            .expect("classic recommend 不应报错");
        let dt = t0.elapsed().as_nanos() as f64 / 1000.0;
        checksum = checksum.wrapping_add(layout.score as u64);
        samples.push(dt);
    }
    black_box((state, checksum));
    (samples, checksum)
}

/// `apply`：classic 固定策略连续对局，每次 `apply` 单独计时；一局 settle 后重新 deal
/// （deal 不计时、不计入 apply 次数）。累计 10 万次 apply。
fn bench_apply() -> (Vec<f64>, u64) {
    let engine = classic_engine().expect("classic 构造失败");
    let mut state = engine
        .deal(gf_engine::cases::M0_SEED)
        .expect("classic 发牌失败");
    let mut samples = Vec::with_capacity(ITERATIONS as usize);
    let mut checksum: u64 = 0;
    let mut applied: u32 = 0;
    while applied < ITERATIONS {
        if state.phase == PHASE_SETTLE {
            state = engine
                .deal(gf_engine::cases::M0_SEED)
                .expect("classic 重发牌失败");
            continue;
        }
        let legal = engine
            .legal_actions(&state)
            .expect("classic 枚举合法动作失败");
        let action = legal
            .into_iter()
            .next()
            .expect("classic 非终局候选不应为空");
        let t0 = Instant::now();
        state = engine.apply(state, action).expect("classic 落子失败");
        let dt = t0.elapsed().as_nanos() as f64 / 1000.0;
        checksum = checksum.wrapping_add(state.turn_seat as u64);
        samples.push(dt);
        applied += 1;
    }
    black_box((checksum, applied));
    (samples, checksum)
}

fn report(name: &str, samples: &mut [f64], candidates_per_call: Option<f64>) {
    let p50 = percentile(samples, 50.0);
    let p90 = percentile(samples, 90.0);
    let p99 = percentile(samples, 99.0);
    let max = samples.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let avg = samples.iter().sum::<f64>() / samples.len() as f64;
    let candidate = candidates_per_call
        .map(|c| format!(" candidates/call={c:.2}"))
        .unwrap_or_default();
    println!(
        "{name}: n={}{candidate} avg={avg:.3} p50={p50:.3} p90={p90:.3} p99={p99:.3} max={max:.3} µs",
        samples.len()
    );
}

// ---------- 统计型全局分配器（与 idle_1500 同口径：堆字节差值，不是 RSS 推算） ----------

struct TrackingAlloc {
    current: AtomicUsize,
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

// ---------- Windows RSS（WorkingSet）只读 FFI，不引新依赖 ----------

#[cfg(windows)]
#[repr(C)]
struct ProcessMemoryCounters {
    cb: u32,
    page_fault_count: u32,
    peak_ws: usize,
    ws: usize,
    quota_peak_paged: usize,
    quota_paged: usize,
    quota_peak_nonpaged: usize,
    quota_nonpaged: usize,
    pagefile_usage: usize,
    peak_pagefile_usage: usize,
}

#[cfg(windows)]
#[link(name = "psapi")]
extern "system" {
    fn GetProcessMemoryInfo(process: isize, counters: *mut ProcessMemoryCounters, cb: u32) -> i32;
}

#[cfg(windows)]
fn working_set_bytes() -> usize {
    let mut counters = ProcessMemoryCounters {
        cb: std::mem::size_of::<ProcessMemoryCounters>() as u32,
        page_fault_count: 0,
        peak_ws: 0,
        ws: 0,
        quota_peak_paged: 0,
        quota_paged: 0,
        quota_peak_nonpaged: 0,
        quota_nonpaged: 0,
        pagefile_usage: 0,
        peak_pagefile_usage: 0,
    };
    // GetCurrentProcess 伪句柄 = -1。
    let ok = unsafe { GetProcessMemoryInfo(-1, &mut counters, counters.cb) };
    if ok == 0 {
        0
    } else {
        counters.ws
    }
}

fn main() {
    // ---------- 单动作延迟：legal@mid / legal@lead / apply，各 10 万次 ----------
    let (engine_mid, state_mid) = classic_mid_state();
    let (mut legal_mid, cs_mid) = bench_legal(&engine_mid, &state_mid);
    let (engine_lead, state_lead) = classic_lead_state();
    let (mut legal_lead, cs_lead) = bench_legal(&engine_lead, &state_lead);
    let (mut recommend, cs_rec) = bench_recommend(&engine_lead, &state_lead);
    let (mut apply, _) = bench_apply();

    let iters = ITERATIONS as f64;
    report(
        "classic legal@mid",
        &mut legal_mid,
        Some(cs_mid as f64 / iters),
    );
    report(
        "classic legal@lead",
        &mut legal_lead,
        Some(cs_lead as f64 / iters),
    );
    report("classic apply", &mut apply, None);
    report(
        "classic recommend@lead",
        &mut recommend,
        Some(cs_rec as f64 / REC_ITERATIONS as f64),
    );

    // ---------- 容量推算（§10.5 第 1499 行同口径） ----------
    // 10Hz、每房间每秒 1 次 legal + 1 次 apply = 20 动作/秒；门槛 33.33 µs/动作。
    let lead_avg = legal_lead.iter().sum::<f64>() / legal_lead.len() as f64;
    let apply_avg = apply.iter().sum::<f64>() / apply.len() as f64;
    let avg_action = (lead_avg + apply_avg) / 2.0;
    let rooms_capacity = 1_000_000.0 / (20.0 * avg_action);
    let p99_max = legal_lead
        .iter()
        .chain(apply.iter())
        .cloned()
        .fold(f64::NEG_INFINITY, f64::max);
    let p99_legal = percentile(&mut legal_lead, 99.0);
    let p99_apply = percentile(&mut apply, 99.0);
    println!(
        "classic capacity: avg_action={avg_action:.3} µs; lead legal avg={lead_avg:.3} apply avg={apply_avg:.3}"
    );
    println!(
        "classic capacity: 推算房间数 ≈ {rooms_capacity:.0}（10Hz、20 动作/秒/房间）；legal@lead P99={p99_legal:.3} apply P99={p99_apply:.3} max={p99_max:.3} µs"
    );

    // ---------- 房间维度内存：1500 classic 房间常驻堆字节 + RSS ----------
    ALLOC.current.store(0, Ordering::Relaxed);
    ALLOC.peak.store(0, Ordering::Relaxed);
    let ws_before;
    #[cfg(windows)]
    {
        ws_before = working_set_bytes();
    }
    #[cfg(not(windows))]
    {
        ws_before = 0;
    }

    let engine = classic_engine().expect("classic 构造失败");
    let mut rooms: Vec<State> = Vec::with_capacity(ROOMS);
    for i in 0..ROOMS {
        let seed = 0xA5A5_0000_u64 + i as u64;
        let state = engine.deal(seed).expect("classic 确定性种子发牌失败");
        rooms.push(state);
    }
    black_box(&rooms);

    let resident_bytes = ALLOC.current.load(Ordering::Relaxed);
    let peak_bytes = ALLOC.peak.load(Ordering::Relaxed);
    let bytes_per_room = resident_bytes as f64 / ROOMS as f64;
    let ws_after;
    #[cfg(windows)]
    {
        ws_after = working_set_bytes();
    }
    #[cfg(not(windows))]
    {
        ws_after = 0;
    }
    let ws_delta = ws_after.saturating_sub(ws_before);
    let ws_per_room = ws_delta as f64 / ROOMS as f64;
    println!(
        "[内存] classic 房间数={ROOMS} 常驻堆字节={resident_bytes} B（峰值 {peak_bytes} B），{bytes_per_room:.1} B/room"
    );
    println!(
        "[内存] classic RSS(WorkingSet) 增量={ws_delta} B，{ws_per_room:.1} B/room；进程 RSS={ws_after} B"
    );
}
