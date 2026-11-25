//! 组牌推荐 `recommend` 性能基准（§5.11 预算段、第三十七次变更第 7 条）。
//!
//! 主指标 = **1000 手随机 27 张手牌的单次冷调用耗时**：随机源取引擎自己的发定长牌
//! （`deal(seed)`，250 个种子 × 4 座位），每次调用都是独立 `Layout`、记忆化表不跨调用，
//! 即冷调用；单次耗时含两条后处理与独立同花顺候选枚举。
//!
//! 判据：`P99 ≤ 2ms` 是**目标线**，超了只记 P2（`recommend_cpu_overrun_total`）、
//! 不判错、不回滚（§6.7 CPU 口径同一原则）；本文件只报数 + 打印是否越线。
//!
//! 豁免边界（§3.3）：引擎 `src/` 内零时钟调用，计时只允许出现在基准落点。

use gf_engine::cases::{classic_engine, M0_SEED};
use gf_engine::contract::{Engine, Seat};
use std::hint::black_box;
use std::time::Instant;

/// 手数目标：250 个种子 × 4 座位 = 1000 手。
const HANDS: usize = 1000;

/// §5.11 目标线（µs）。
const BUDGET_P99_US: f64 = 2000.0;

/// 最近秩法（nearest-rank）：rank = ceil(p/100 · n)，取排序后第 rank 个样本。
fn percentile(samples: &mut [f64], p: f64) -> f64 {
    samples.sort_by(|a, b| a.partial_cmp(b).expect("基准样本含 NaN"));
    let n = samples.len();
    let rank = ((p / 100.0) * n as f64).ceil() as usize;
    samples[rank.clamp(1, n) - 1]
}

fn main() {
    let engine = classic_engine().expect("classic 构造失败");
    let mut samples: Vec<f64> = Vec::with_capacity(HANDS);
    let mut checksum: u64 = 0;

    // 一个种子发一次牌，四座位各跑一次（每次都是独立冷调用）。
    for seed in 0..(HANDS / 4) as u64 {
        let state = engine
            .deal(M0_SEED ^ seed.wrapping_mul(0x9e37_79b9))
            .expect("发牌失败");
        for seat in 0..4u8 {
            let t0 = Instant::now();
            let layout = engine
                .recommend(&state, Seat(seat))
                .expect("classic recommend 不应报错");
            let dt = t0.elapsed().as_nanos() as f64 / 1000.0;
            checksum = checksum.wrapping_add(layout.score as u64);
            checksum = checksum.wrapping_add(layout.groups.len() as u64);
            black_box(&state);
            samples.push(dt);
        }
    }

    let avg = samples.iter().sum::<f64>() / samples.len() as f64;
    let p50 = percentile(&mut samples, 50.0);
    let p90 = percentile(&mut samples, 90.0);
    let p99 = percentile(&mut samples, 99.0);
    let max = samples.iter().cloned().fold(f64::NEG_INFINITY, f64::max);

    println!(
        "classic recommend 1000 手随机 27 张（冷调用）: n={} avg={avg:.3} p50={p50:.3} p90={p90:.3} p99={p99:.3} max={max:.3} µs",
        samples.len()
    );
    println!("checksum={checksum}");
    if p99 <= BUDGET_P99_US {
        println!("budget: p99 {p99:.3}µs ≤ {BUDGET_P99_US}µs —— 未越线");
    } else {
        println!(
            "budget: p99 {p99:.3}µs > {BUDGET_P99_US}µs —— 越线（记 recommend_cpu_overrun_total，P2，不判错）"
        );
    }
}
