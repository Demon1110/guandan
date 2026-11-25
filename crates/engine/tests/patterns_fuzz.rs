//! 牌型识别 / 比较的随机扫射：不是断言某个具体牌型，而是保证「任何牌面组合
//! 进 `apply` 都只可能成功或被判非法」，不允许 panic / 越界 / 伪合法。
//!
//! 确定性 xorshift，跑固定轮数，结果可复现；覆盖 `patterns.rs` 里大量
//! 「张数不足 / 超长 / 王混入 / 同花顺窗口 / 阶梯比较」的条件分支。

use gf_engine::cases::M0_SEED;
use gf_engine::contract::{Action, Engine, EngineError, State};
use gf_engine::engine::M0Engine;
use gf_engine::ruleset::{from_yaml, RuleSetBody};
use std::fs;
use std::path::PathBuf;

fn fixture_body() -> RuleSetBody {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("tests/fixtures/guandan-classic/ruleset.yaml");
    let src = fs::read_to_string(p).expect("夹具读取失败");
    from_yaml(&src).expect("guandan-classic 应构造成功")
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

/// 把 seat 0 的手牌换成 `cards`，并清空桌面（领出态）。
fn lead_state(engine: &M0Engine, base: &State, cards: Vec<u16>) -> State {
    let mut state = base.clone();
    let zone = hand_zone(engine);
    for z in state.zones.iter_mut() {
        if z.seat == 0 && z.zone_id == zone {
            z.cards = cards.clone();
        }
    }
    state.turn_seat = 0;
    state.last_play = None;
    state.passes_since_last_play = 0;
    state
}

/// 确定性 xorshift64*，避免引入 rand 依赖。
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

/// 从 108 张牌池里取 `n` 张不重复的牌。
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
fn random_lead_plays_only_succeed_or_report_illegal() {
    let engine = engine();
    let base = engine.deal(M0_SEED).expect("发牌失败");
    let mut rng = Rng(0x0BAD_F00D_1234_5678);
    let mut accepted = 0usize;
    let mut rejected = 0usize;

    for _ in 0..1200 {
        // 1..=12 张：覆盖「不足最小牌型 / 正常 / 超长连 / 超张数炸弹」的边界。
        let n = 1 + rng.below(12);
        let cards = sample(&mut rng, n);
        let state = lead_state(&engine, &base, cards.clone());
        match engine.apply(
            state,
            Action::Play {
                cards: cards.clone(),
            },
        ) {
            Ok(next) => {
                accepted += 1;
                assert_eq!(next.turn_seat, 1, "领出成功后应轮到下家");
                assert!(
                    next.zones
                        .iter()
                        .find(|z| z.seat == 0 && z.zone_id == hand_zone(&engine))
                        .map(|z| z.cards.is_empty())
                        .unwrap_or(false),
                    "出完的牌必须从手牌移除"
                );
                if let Some(last) = &next.last_play {
                    assert_eq!(last.cards, cards, "桌面记录应与出牌一致");
                }
            }
            Err(EngineError::IllegalAction { .. }) => rejected += 1,
            Err(other) => panic!("未预期的错误：{other:?}"),
        }
    }
    assert!(
        accepted > 50,
        "随机样本里应有足够多的合法牌型，实际 {accepted}"
    );
    assert!(
        rejected > 50,
        "随机样本里应有足够多的非法牌型，实际 {rejected}"
    );
}

#[test]
fn random_follow_plays_never_panic_and_respect_compare() {
    let engine = engine();
    let base = engine.deal(M0_SEED).expect("发牌失败");
    let mut rng = Rng(0xDEAD_BEEF_0000_0001);
    let mut bigger = 0usize;
    let mut refused = 0usize;

    for _ in 0..900 {
        // 先造一手合法的「上一手」：单张 / 对子 / 三张 / 四张炸弹都可能出现。
        let lead_n = 1 + rng.below(4);
        let lead_cards = sample(&mut rng, lead_n);
        let lead_state = lead_state(&engine, &base, lead_cards.clone());
        let Some(lead) = engine
            .apply(lead_state, Action::Play { cards: lead_cards })
            .ok()
        else {
            continue;
        };
        let Some(last) = lead.last_play.clone() else {
            continue;
        };

        // 换 seat 1 出牌，手牌随机，看它能不能压过上一手。
        let mut state = base.clone();
        let zone = hand_zone(&engine);
        let follow_n = 1 + rng.below(8);
        let follow = sample(&mut rng, follow_n);
        for z in state.zones.iter_mut() {
            if z.seat == 1 && z.zone_id == zone {
                z.cards = follow.clone();
            }
        }
        state.turn_seat = 1;
        state.last_play = Some(last);
        state.passes_since_last_play = 0;

        match engine.apply(
            state,
            Action::Play {
                cards: follow.clone(),
            },
        ) {
            Ok(next) => {
                bigger += 1;
                let new_last = next.last_play.expect("跟牌成功后必有桌面记录");
                assert_eq!(new_last.seat, 1);
                assert_eq!(new_last.cards, follow);
            }
            Err(EngineError::IllegalAction { .. }) => refused += 1,
            Err(other) => panic!("未预期的错误：{other:?}"),
        }
    }
    assert!(bigger > 5, "随机跟牌里应有压过上一手的样本，实际 {bigger}");
    assert!(refused > 0, "随机跟牌里应有压不过的样本");
}

#[test]
fn pass_after_lead_is_illegal_but_pass_after_play_is_legal() {
    // 领出态 Pass 非法（必须出牌）；跟牌态 Pass 合法。
    let engine = engine();
    let base = engine.deal(M0_SEED).expect("发牌失败");
    let mut rng = Rng(0x51ED_0BAD_0000_0002);
    let cards = sample(&mut rng, 1);
    let state = lead_state(&engine, &base, cards.clone());
    assert!(matches!(
        engine.apply(state, Action::Pass),
        Err(EngineError::IllegalAction { .. })
    ));

    let mut follow = base.clone();
    let zone = hand_zone(&engine);
    let follow_cards = sample(&mut rng, 1);
    for z in follow.zones.iter_mut() {
        if z.seat == 1 && z.zone_id == zone {
            z.cards = follow_cards.clone();
        }
    }
    follow.turn_seat = 1;
    follow.last_play = Some(gf_engine::hash::LastPlay {
        seat: 0,
        pattern_id: "single".to_owned(),
        pattern_key: (0, 0),
        cards: vec![0],
    });
    assert!(
        engine.apply(follow, Action::Pass).is_ok(),
        "跟牌态过牌必须合法"
    );
}

/// 随机手牌（5–27 张）直接喂 `legal_actions`：候选枚举内部会遍历同点数组、
/// 顺子/连对/钢板窗口、同花顺、各级炸弹与三带二附件，是识别分支最宽的入口。
#[test]
fn random_hands_enumerate_legal_actions() {
    let engine = engine();
    let base = engine.deal(M0_SEED).expect("发牌失败");
    let mut rng = Rng(0x1234_ABCD_0000_0003);
    let mut rounds = 0usize;

    for _ in 0..240 {
        let n = 5 + rng.below(23);
        let hand = sample(&mut rng, n);
        let state = lead_state(&engine, &base, hand);
        match engine.legal_actions(&state) {
            Ok(actions) => {
                assert!(!actions.is_empty(), "非空手牌领出必须有候选");
                if let Some(Action::Play { cards }) = actions
                    .into_iter()
                    .find(|a| matches!(a, Action::Play { .. }))
                {
                    engine
                        .apply(state, Action::Play { cards })
                        .expect("枚举出来的领出候选必须被接受");
                }
                rounds += 1;
            }
            Err(EngineError::QuotaExceeded) => {}
            Err(e) => panic!("候选枚举失败：{e:?}"),
        }
    }
    assert!(rounds > 180, "有效枚举轮数不足：{rounds}");
}

/// 随机跟牌态（上家已压桌）的候选枚举：覆盖 compare / 炸弹越级 / 同花顺位次
/// 在「生成候选」一侧的分支，与 `apply` 一侧的判定互为交叉。
#[test]
fn random_follow_states_enumerate_legal_actions() {
    let engine = engine();
    let base = engine.deal(M0_SEED).expect("发牌失败");
    let mut rng = Rng(0x9999_1111_0000_0004);
    let zone = hand_zone(&engine);
    let mut rounds = 0usize;

    for _ in 0..120 {
        // 用候选枚举造一手确定合法的领出，保证每轮都能进到跟牌态。
        let lead_n = 3 + rng.below(7);
        let lead_hand = sample(&mut rng, lead_n);
        let lead_state = lead_state(&engine, &base, lead_hand);
        let Some(Action::Play { cards }) = engine
            .legal_actions(&lead_state)
            .ok()
            .and_then(|v| v.into_iter().find(|a| matches!(a, Action::Play { .. })))
        else {
            continue;
        };
        let Ok(lead) = engine.apply(lead_state, Action::Play { cards }) else {
            continue;
        };
        let Some(last) = lead.last_play.clone() else {
            continue;
        };

        let mut state = base.clone();
        let follow_n = 6 + rng.below(22);
        let follow = sample(&mut rng, follow_n);
        for z in state.zones.iter_mut() {
            if z.seat == 1 && z.zone_id == zone {
                z.cards = follow.clone();
            }
        }
        state.turn_seat = 1;
        state.last_play = Some(last);
        state.passes_since_last_play = 0;

        match engine.legal_actions(&state) {
            Ok(actions) => {
                assert!(!actions.is_empty(), "跟牌态至少包含 pass");
                rounds += 1;
            }
            Err(EngineError::QuotaExceeded) => {}
            Err(e) => panic!("跟牌候选枚举失败：{e:?}"),
        }
    }
    assert!(rounds > 90, "有效跟牌枚举轮数不足：{rounds}");
}
