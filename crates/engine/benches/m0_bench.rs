//! M0 解释执行基准（§2.4 gate 第 4 项、§10.5 基准表）。
//!
//! 豁免边界（§3.3）：引擎 `src/` 内保持零时钟调用；本文件是唯一允许使用
//! `std::time::Instant` 的计时落点之一（另一处是 wasm Node 宿主）。
//! 不引 criterion 重依赖：`[[bench]] harness = false`，自写计时与最近秩分位数。

use gf_engine::cases::{build_engine, mid_game_state, M0_SEED};
use gf_engine::contract::Engine;
use gf_engine::engine::PHASE_SETTLE;
use std::hint::black_box;
use std::time::Instant;

/// 每侧迭代次数（§2.4：10 万次，不许减产）。
const ITERATIONS: u32 = 100_000;

/// 最近秩法（nearest-rank）：rank = ceil(p/100 · n)，取排序后第 rank 个样本。
fn percentile(samples: &mut [f64], p: f64) -> f64 {
    samples.sort_by(|a, b| a.partial_cmp(b).expect("基准样本含 NaN"));
    let n = samples.len();
    let rank = ((p / 100.0) * n as f64).ceil() as usize;
    let rank = rank.clamp(1, n);
    samples[rank - 1]
}

/// `legal_actions`：固定中局状态（手牌约 27、已有 lastPlay），连续调用 10 万次；
/// 候选条数累加进 checksum 并由 `black_box` 吃掉，防止优化删除枚举结果。
fn bench_legal_actions() -> Vec<f64> {
    let (engine, state) = mid_game_state();
    let mut samples = Vec::with_capacity(ITERATIONS as usize);
    let mut checksum: u64 = 0;
    for _ in 0..ITERATIONS {
        let t0 = Instant::now();
        let legal = engine.legal_actions(&state).expect("枚举合法动作失败");
        let dt = t0.elapsed().as_nanos() as f64 / 1000.0;
        checksum = checksum.wrapping_add(legal.len() as u64);
        samples.push(dt);
    }
    black_box((&state, checksum));
    samples
}

/// `apply`：固定策略连续对局，每次 `apply` 单独计时；一局 settle 后重新 deal
/// （deal 不计时、不计入 apply 次数）。累计 10 万次 apply。
fn bench_apply() -> Vec<f64> {
    let engine = build_engine().expect("构造引擎失败");
    let mut state = engine.deal(M0_SEED).expect("发牌失败");
    let mut samples = Vec::with_capacity(ITERATIONS as usize);
    let mut checksum: u64 = 0;
    let mut applied: u32 = 0;
    while applied < ITERATIONS {
        if state.phase == PHASE_SETTLE {
            state = engine.deal(M0_SEED).expect("重发牌失败");
            continue;
        }
        let legal = engine.legal_actions(&state).expect("枚举合法动作失败");
        let action = legal.into_iter().next().expect("非终局候选不应为空");
        let t0 = Instant::now();
        state = engine.apply(state, action).expect("落子失败");
        let dt = t0.elapsed().as_nanos() as f64 / 1000.0;
        checksum = checksum.wrapping_add(state.turn_seat as u64);
        samples.push(dt);
        applied += 1;
    }
    black_box((checksum, applied));
    samples
}

fn report(name: &str, samples: &mut [f64]) {
    let p50 = percentile(samples, 50.0);
    let p90 = percentile(samples, 90.0);
    let p99 = percentile(samples, 99.0);
    let max = samples.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let avg = samples.iter().sum::<f64>() / samples.len() as f64;
    println!(
        "{name}: n={} avg={avg:.3} p50={p50:.3} p90={p90:.3} p99={p99:.3} max={max:.3} µs",
        samples.len()
    );
}

fn main() {
    let mut legal = bench_legal_actions();
    let mut apply = bench_apply();
    report("legal_actions", &mut legal);
    report("apply", &mut apply);
}
