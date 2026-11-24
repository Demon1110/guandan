//! M0 golden 确定性回归：双端比对锚点、投影稳定性、动作敏感性。
//!
//! 期望哈希先由 native 用例跑出、确认动作日志合法后再写死（不许先写期望再凑实现）。

use gf_engine::cases::{build_engine, run_classic_golden, run_m0_golden, M0_SEED};
use gf_engine::contract::{Action, Engine, State};
use gf_engine::engine::{M0Engine, PHASE_SETTLE};
use gf_engine::hash::{canonical_bytes, state_hash, CanonicalState};
use gf_engine::settlement::{valid_ruleset_version, Settlement};

/// native 用例跑出并核对过动作日志后的 32 字节 stateHash（双端逐字节比对锚点）。
/// 第二十六次变更（接风 / 全过判定顺序订正）后轨迹漂移，锚点已按最终口径重算。
/// 本批 `settle-single-round` 第二批：N18 score-only `levels` 由空序列改为 initial
/// 常量占位（第 2 段队位前缀 0→2），第 9 段 `aFailStreak` 仍为两队位 0；其余字段实测未变。
const EXPECTED_STATE_HASH: &str =
    "8040b7e6d3a780ce3dd38e3d0933a25e20941367b4385e785d9408f862d1ed5f";

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
    assert!(outcome.completed_team.is_some(), "终局应有队伍全员先出完");
    assert!(outcome.winner_team.is_some(), "终局头游应归属某一队");
    // 交叉核对两队口径：winner_team = 头游（finish_order[0]）所在队；
    // completed_team = 全员进入名次的队伍中最晚名次编号最小者（§5.13 第 902 行）。
    let teams = build_engine().expect("构造引擎失败").ruleset.seats.teams;
    let head = outcome.finish_order[0];
    let expected_winner = teams
        .iter()
        .position(|t| t.contains(&head))
        .map(|i| i as u8);
    let expected_completed = teams
        .iter()
        .enumerate()
        .filter(|(_, t)| t.iter().all(|s| outcome.finish_order.contains(s)))
        .map(|(i, t)| {
            let last = t
                .iter()
                .map(|s| {
                    outcome
                        .finish_order
                        .iter()
                        .position(|x| x == s)
                        .expect("队员应在名次中")
                })
                .max()
                .expect("队伍非空");
            (i as u8, last)
        })
        .min_by(|a, b| a.1.cmp(&b.1).then(a.0.cmp(&b.0)))
        .map(|(i, _)| i);
    assert_eq!(
        outcome.winner_team, expected_winner,
        "winner_team 应等于头游所在队"
    );
    assert_eq!(
        outcome.completed_team, expected_completed,
        "completed_team 应等于先全员出完的队"
    );
    // 当前种子实测为 1+3 形态（finish_order=[1,3,0,2]：头游 seat1 属队 1，队友 seat3 占
    // 第 2 名，对家 seat0、seat2 夹第 3、4 名）；此形态下 winner_team（头游队）与
    // completed_team（先全员出完的队）同为队 1，二者不是 1+4 下的对家关系。
    assert_eq!(
        outcome.finish_order,
        vec![1, 3, 0, 2],
        "收尾名次应与实测 1+3 形态一致"
    );
    // ranks 与 finish_order 同口径（名次 1→n 的 seat 数组）；score-only 局无升级档，step 为 None。
    assert_eq!(outcome.ranks, vec![1, 3, 0, 2], "ranks 应按名次 1→n 列座位");
    assert_eq!(
        outcome.settle_step, None,
        "score-only 局不查 upgrade[]，step 应为 None"
    );
    // N18：score-only 局 levels 保持 initial 常量占位（下标 0）、aFailStreak 全队恒 0，
    // 照常进第 2/9 段，不省略、不用 0xFF。
    assert_eq!(outcome.state.levels, vec![0, 0]);
    assert_eq!(outcome.state.a_fail_streak, vec![0, 0]);
    // result bytes：canonical JSON，键序固定 winner_team, ranks, points，单行无空白。
    let m0_result = Settlement {
        winner_team: outcome.winner_team.unwrap(),
        ranks: outcome.ranks.clone(),
        points: 0,
    };
    assert_eq!(
        m0_result.encode_result_bytes(),
        br#"{"winner_team":1,"ranks":[1,3,0,2],"points":0}"#
    );
    assert_eq!(
        outcome.completed_team, outcome.winner_team,
        "1+3 形态下 completed_team 与 winner_team 同为头游队"
    );
    // N17：settle 全段 lastPlay 保持 None，视图键不出现；stateHash 第 5 段走占位编码。
    assert!(
        outcome.state.last_play.is_none(),
        "settle 阶段 lastPlay 必须清空为 None"
    );
    let hex: String = outcome
        .state_hash
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    assert_eq!(hex, EXPECTED_STATE_HASH, "native 用例哈希与记录锚点不一致");
}

#[test]
fn classic_first_round_settles_with_concrete_step_and_teams() {
    // `guandan-classic` level-up 首局固定策略跑到 settle；数值按实测写死，不许只断言非空。
    // 实测轨迹：ranks=[2,1,0,3]，头游 seat2 属队 0，队友 seat0 占第 3 名 → rankCombo [1,3]
    // 查 upgrade[] 得 step=2；completed_team=0（队 0 先全员出完），winner_team=0（头游队）。
    let outcome = run_classic_golden().expect("classic golden 运行失败");
    assert_eq!(outcome.state.phase, PHASE_SETTLE, "classic 必须跑到 settle");
    assert_eq!(outcome.settle_count, 1, "settle 转移应恰为一次");
    assert_eq!(outcome.ranks, vec![2, 1, 0, 3], "ranks 应按名次 1→n 列座位");
    assert_eq!(outcome.finish_order, vec![2, 1, 0, 3]);
    assert_eq!(outcome.winner_team, Some(0), "头游 seat2 属队 0");
    assert_eq!(outcome.completed_team, Some(0), "先全员出完的是队 0");
    assert_eq!(
        outcome.settle_step,
        Some(2),
        "rankCombo [1,3] 升级档应为 +2"
    );
    // §5.13「决定首出」：settle 结束 attackingTeam 交接头游所在队。
    assert_eq!(
        outcome.state.attacking_team, 0,
        "attackingTeam 应为头游队 0"
    );
    // 首局 initial（下标 0）打 1+3 → step=2，levels[0]=2；未触 A，streak 全队 0。
    assert_eq!(outcome.state.levels, vec![2, 0]);
    assert_eq!(outcome.state.a_fail_streak, vec![0, 0]);
    // N17：settle 阶段 lastPlay 清空，结算输出与它无关。
    assert!(outcome.state.last_play.is_none());
    assert_eq!(outcome.discard_count + outcome.hands_total, 108, "牌数守恒");
    // result bytes 与 SettleResult 载体：正常结算 reason 缺省；rulesetVersion 仅作身份键。
    let settlement = Settlement {
        winner_team: 0,
        ranks: outcome.ranks.clone(),
        points: 0,
    };
    assert_eq!(
        settlement.encode_result_bytes(),
        br#"{"winner_team":0,"ranks":[2,1,0,3],"points":0}"#
    );
    assert!(valid_ruleset_version("guandan-classic-1.0.0"));
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
    // 同一 seed、同一固定策略，仅把第一步首攻换成另一张合法单张，两条分支都跑到
    // settle 后再比 32 字节哈希——比一步版更能抓住后期状态被抹平的回归。
    // 抽牌前置后发牌分布变化：个别首攻选择会在终局收敛（两张牌最后都进弃牌区），
    // 故枚举全部首攻候选，取两条终局哈希确实不同的分支对照。
    let hex = |h: [u8; 32]| h.iter().map(|b| format!("{b:02x}")).collect::<String>();
    let mut traced: Vec<(u16, String)> = Vec::new();
    for a in &plays {
        let c = match a {
            Action::Play { cards } => cards[0],
            Action::Pass => continue,
        };
        let st = engine.apply(s0.clone(), a.clone()).expect("落子失败");
        let (h, _) = run_to_settle(&engine, st);
        traced.push((c, hex(h)));
    }
    let uniq: std::collections::HashSet<&String> = traced.iter().map(|(_, h)| h).collect();
    assert!(
        uniq.len() >= 2,
        "领出阶段应至少有两条首攻跑到不同终局；实际收敛到 {} 种",
        uniq.len()
    );
    let first = &traced[0];
    let second = traced
        .iter()
        .find(|(_, h)| h != &first.1)
        .expect("已有两条不同终局");
    assert_ne!(
        first.1, second.1,
        "首步换一张出牌并跑到底，终局 stateHash 必须不同"
    );
}

fn run_to_settle(engine: &M0Engine, mut state: State) -> ([u8; 32], State) {
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
    (state_hash(&CanonicalState::from(&state)), state)
}
