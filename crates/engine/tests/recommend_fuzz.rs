//! `recommend` 随机手牌扫射：0–27 张随机手牌 × 400 轮，只断言「不失败 + 分组守恒」。
//!
//! 目的是把「拆组 / 散张 / 三带二附件 / 顺子窗口 / 同花顺候选」的退化条件都触一遍
//! ——手牌长度、有无百搭、有无王、点数分布都由随机决定，具体分组仍由
//! `recommend_test.rs` 的确定性用例负责。

use gf_engine::cases::M0_SEED;
use gf_engine::contract::{Engine, Seat, State};
use gf_engine::engine::M0Engine;
use gf_engine::ruleset::{from_yaml, RuleSetBody};
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

fn fixture_body() -> RuleSetBody {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("tests/fixtures/guandan-classic/ruleset.yaml");
    from_yaml(&fs::read_to_string(p).expect("夹具读取失败")).expect("夹具解析失败")
}

fn engine() -> M0Engine {
    M0Engine::new(fixture_body())
}

fn hand_zone(engine: &M0Engine) -> u16 {
    engine
        .ruleset
        .zones
        .iter()
        .position(|z| z.id.0 == "hand")
        .expect("hand 区应存在") as u16
}

fn with_hand(engine: &M0Engine, base: &State, cards: Vec<u16>) -> State {
    let mut state = base.clone();
    let zone = hand_zone(engine);
    for z in state.zones.iter_mut() {
        if z.seat == 0 && z.zone_id == zone {
            z.cards = cards.clone();
        }
    }
    state.turn_seat = 0;
    state
}

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

fn sample(rng: &mut Rng, n: usize) -> Vec<u16> {
    let mut pool: Vec<u16> = (0..108).collect();
    let mut out = Vec::with_capacity(n);
    for _ in 0..n.min(pool.len()) {
        let i = rng.below(pool.len());
        out.push(pool.swap_remove(i));
    }
    out.sort_unstable();
    out
}

#[test]
fn recommend_keeps_hands_conservative_over_random_hands() {
    let engine = engine();
    let base = engine.deal(M0_SEED).expect("发牌失败");
    let mut rng = Rng(0x5EED_0000_1111_2222);
    let mut empty_hands = 0usize;

    for _ in 0..400 {
        let n = rng.below(28);
        let hand = sample(&mut rng, n);
        if hand.is_empty() {
            empty_hands += 1;
        }
        let state = with_hand(&engine, &base, hand.clone());
        let layout = engine
            .recommend(&state, Seat(0))
            .expect("recommend 不得失败");

        let mut used: BTreeMap<u16, usize> = BTreeMap::new();
        for g in &layout.groups {
            for &c in &g.cards {
                *used.entry(c).or_default() += 1;
            }
        }
        let mut have: BTreeMap<u16, usize> = BTreeMap::new();
        for &c in &hand {
            *have.entry(c).or_default() += 1;
        }
        for (c, k) in &used {
            assert!(
                have.get(c).copied().unwrap_or(0) >= *k,
                "牌 {c} 在分组里超发：分组 {k}，手牌 {have:?}"
            );
        }
        let used_total: usize = used.values().sum();
        assert_eq!(
            used_total + layout.single_count as usize,
            hand.len(),
            "分组张数 + 散张数应等于整手（手牌 {}，分组 {used_total}，散 {})",
            hand.len(),
            layout.single_count
        );
    }
    assert!(empty_hands > 0, "随机样本应包含空手牌边界");
}
