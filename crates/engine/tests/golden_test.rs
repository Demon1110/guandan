//! M0 golden 确定性回归：双端比对锚点、投影稳定性、动作敏感性。
//!
//! 期望哈希先由 native 用例跑出、确认动作日志合法后再写死（不许先写期望再凑实现）。

use gf_engine::cases::{build_engine, run_m0_golden, M0_SEED};
use gf_engine::contract::{Action, Engine, State};
use gf_engine::engine::{M0Engine, PHASE_SETTLE};
use gf_engine::hash::{canonical_bytes, state_hash, CanonicalState};

/// native 用例跑出并核对过动作日志后的 32 字节 stateHash（双端逐字节比对锚点）。
/// §5.2 随机源口径变更（20 字节 BLAKE3 + 拒绝采样 + Fisher-Yates 降序）后重算。
const EXPECTED_STATE_HASH: &str =
    "925e4018c4c51ef276d9bfbb754038fc00c81c6eadb7c5a3cf60e16e3af281ac";

#[test]
fn golden_reaches_settle_and_matches_recorded_hash() {
    let outcome = run_m0_golden().expect("m0 golden 运行失败");
    assert_eq!(
        outcome.state.phase,
        PHASE_SETTLE,
        "golden 必须跑到 settle；finish_order={:?}, actions={}",
        outcome.state.finish_order,
        outcome.actions.len()
    );
    assert_eq!(outcome.settle_count, 1, "settle 转移应恰为一次");
    assert_eq!(outcome.finish_order.len(), 4, "名次应补位到 4 个座位");
    assert!(outcome.steps >= 120, "步数不得退回 102 那种提前退出位置");
    assert_eq!(
        outcome.discard_count + outcome.hands_total,
        108,
        "终局牌数必须守恒"
    );
    assert!(outcome.terminal_team.is_some(), "终局应有队伍全员先出完");
    let hex: String = outcome
        .state_hash
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    assert_eq!(hex, EXPECTED_STATE_HASH, "native 用例哈希与记录锚点不一致");
}

#[test]
fn projection_is_stable_for_same_state() {
    let outcome = run_m0_golden().expect("m0 golden 运行失败");
    let c1 = CanonicalState::from(&outcome.state);
    let c2 = CanonicalState::from(&outcome.state);
    // 同一个 State 两次投影：canonical 字节与哈希都必须稳定。
    assert_eq!(canonical_bytes(&c1), canonical_bytes(&c2));
    assert_eq!(state_hash(&c1), state_hash(&c2));
}

#[test]
fn different_action_changes_state_hash() {
    let engine = build_engine().expect("构造引擎失败");
    let s0 = engine.deal(M0_SEED).expect("发牌失败");
    let legal = engine.legal_actions(&s0).expect("枚举合法动作失败");
    let plays: Vec<&Action> = legal
        .iter()
        .filter(|a| matches!(a, Action::Play { .. }))
        .collect();
    assert!(plays.len() >= 2, "领出阶段至少应有两张合法单张");

    let s1 = engine
        .apply(s0.clone(), plays[0].clone())
        .expect("落子失败");
    let s2 = engine.apply(s0, plays[1].clone()).expect("落子失败");
    let h1 = state_hash(&CanonicalState::from(&s1));
    let h2 = state_hash(&CanonicalState::from(&s2));
    assert_ne!(h1, h2, "换一张出牌，stateHash 必须变化");
}

#[test]
fn different_first_play_changes_state_hash_to_settle() {
    let engine = build_engine().expect("构造引擎失败");
    let s0 = engine.deal(M0_SEED).expect("发牌失败");
    let legal = engine.legal_actions(&s0).expect("枚举合法动作失败");
    let plays: Vec<Action> = legal
        .into_iter()
        .filter(|a| matches!(a, Action::Play { .. }))
        .collect();
    assert!(plays.len() >= 2, "领出阶段至少应有两张合法单张");
    let copies = engine.ruleset.deck.copies as usize;
    let rank_count = engine.ruleset.deck.ranks.len();
    let first_card = match &plays[0] {
        Action::Play { cards } => cards[0] as usize,
        Action::Pass => unreachable!("领出候选不含 Pass"),
    };
    // 排序后前两张可能是同点数的两张副本（copies>1，如 id 0/1 同为点数 2）；换副本不产生
    // 策略差异，两条分支会收敛到同一终局。对照分支取首个点数桶真正不同的合法单张。
    let second = plays
        .iter()
        .find(|a| match a {
            Action::Play { cards } => {
                (cards[0] as usize / copies) % rank_count != (first_card / copies) % rank_count
            }
            Action::Pass => false,
        })
        .expect("领出阶段应有不同点数的合法单张")
        .clone();

    // 同一 seed、同一固定策略，仅把第一步首攻换成另一张不同点数的合法单张，两条分支都跑到
    // settle 后再比 32 字节哈希——比一步版更能抓住后期状态被抹平的回归。
    let s1 = engine
        .apply(s0.clone(), plays[0].clone())
        .expect("落子失败");
    let s2 = engine.apply(s0, second).expect("落子失败");
    let h1 = run_to_settle(&engine, s1);
    let h2 = run_to_settle(&engine, s2);
    assert_ne!(
        h1, h2,
        "首步换一张不同点数出牌并跑到底，终局 stateHash 必须不同"
    );
}

fn run_to_settle(engine: &M0Engine, mut state: State) -> [u8; 32] {
    for _ in 0..10_000 {
        if state.phase == PHASE_SETTLE {
            break;
        }
        let legal = engine
            .legal_actions(&state)
            .expect("对局中途不应出现非终局空候选");
        let action = legal
            .into_iter()
            .next()
            .expect("非终局空候选应由引擎报 Internal");
        state = engine.apply(state, action).expect("落子失败");
    }
    assert_eq!(state.phase, PHASE_SETTLE, "两条分支都必须跑到 settle");
    state_hash(&CanonicalState::from(&state))
}
