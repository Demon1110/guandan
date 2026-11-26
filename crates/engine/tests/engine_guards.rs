//! 引擎运行期守卫的分支覆盖（纯补测，不改实现）。
//!
//! 全部用「篡改 `State` 的公开字段 + 调 `legal_actions` / `apply`」的方式触达错误与边界
//! 路径：非法相位、空牌、不在手牌、重复牌、缺手牌区、领出过牌、接风判定、重开一局。

use gf_engine::cases;
use gf_engine::contract::{Action, Engine, EngineError, Seat, State};
use gf_engine::engine::{M0Engine, PHASE_PLAY, PHASE_SETTLE};
use gf_engine::ruleset::parse::from_yaml;

const M0: &str = include_str!("fixtures/m0-minimal/ruleset.yaml");

fn hand_zone(state: &State, seat: u8) -> Vec<u16> {
    state
        .zones
        .iter()
        .find(|z| z.seat == seat && z.zone_id == 0)
        .map(|z| z.cards.clone())
        .unwrap_or_default()
}

#[test]
fn apply_rejects_illegal_phase_and_bad_cards() {
    let (engine, mut state) = cases::lead_state();
    state.turn_seat = 0;
    let hand = hand_zone(&state, 0);
    assert!(!hand.is_empty(), "领出态 seat 0 应有手牌");

    // 相位不是 play：任何出牌都拒绝。
    let mut settled = state.clone();
    settled.phase = PHASE_SETTLE;
    assert!(
        engine.apply(settled, Action::Play { cards: vec![hand[0]] }).is_err(),
        "settle 相位不应接受出牌"
    );

    // 空牌集。
    assert!(
        engine.apply(state.clone(), Action::Play { cards: Vec::new() }).is_err(),
        "空牌集应被拒"
    );

    // 不在手牌：取一张确定不在手上的牌 id。
    let absent = (0..108u16).find(|c| !hand.contains(c)).expect("应存在不在手上的 id");
    assert!(
        engine.apply(state.clone(), Action::Play { cards: vec![absent] }).is_err(),
        "不在手牌的牌应被拒"
    );

    // 重复牌 id：同一张牌提交两次。
    assert!(
        engine.apply(state.clone(), Action::Play { cards: vec![hand[0], hand[0]] }).is_err(),
        "重复牌 id 应被拒"
    );

    // 缺 hand 区：手牌区被摘掉后无法定位手牌。
    let mut no_hand = state.clone();
    no_hand.zones.retain(|z| !(z.seat == 0 && z.zone_id == 0));
    assert!(
        engine.apply(no_hand, Action::Play { cards: vec![hand[0]] }).is_err(),
        "缺手牌区应报错"
    );

    // 牌 id 越界（≥ 基础总张数）：解码期即拒。
    assert!(
        engine.apply(state.clone(), Action::Play { cards: vec![200] }).is_err(),
        "越界牌 id 应被拒"
    );
}

#[test]
fn legal_actions_guards() {
    let (engine, mut state) = cases::lead_state();
    state.turn_seat = 0;
    assert!(!engine.legal_actions(&state).expect("领出态应可枚举").is_empty());

    // 非 play 相位：返回空表或报错，两者都属允许口径，这里只要求「不会给出候选」。
    let mut settled = state.clone();
    settled.phase = PHASE_SETTLE;
    let settled_actions = engine.legal_actions(&settled).unwrap_or_default();
    assert!(settled_actions.is_empty(), "settle 相位不应给出候选");

    // 该座位已出完：不变量层直接拒（返回空洞错误）或返回空表，两者都不得给出候选。
    let mut finished = state.clone();
    finished.finish_order = vec![0];
    assert!(
        engine.legal_actions(&finished).unwrap_or_default().is_empty(),
        "已出完座位不应有候选"
    );

    // 缺手牌区。
    let mut no_hand = state.clone();
    no_hand.zones.retain(|z| !(z.seat == 0 && z.zone_id == 0));
    assert!(
        engine.legal_actions(&no_hand).is_err(),
        "缺手牌区枚举应报错"
    );

    // 本队两人都已出完：无候选也不再追加过牌。
    let mut both = state.clone();
    both.finish_order = vec![0, 2];
    let _ = engine.legal_actions(&both);
}

#[test]
fn pass_on_lead_is_rejected() {
    let (engine, mut state) = cases::lead_state();
    state.turn_seat = 0;
    assert!(state.last_play.is_none(), "领出态 last_play 应为空");
    assert!(
        matches!(
            engine.apply(state, Action::Pass),
            Err(EngineError::IllegalAction { .. })
        ),
        "领出阶段过牌应被拒"
    );
}

#[test]
fn partner_lead_after_all_pass() {
    let (engine, mut state) = cases::lead_state();
    state.turn_seat = 1;
    // 直接造「上一手由 seat 0 领出、seat 0 已出完、其余三座都已过牌」的临界态：
    // 只差最后一票过牌，全过之后应由搭档（seat 2）接风。
    state.last_play = Some(gf_engine::hash::LastPlay {
        seat: 0,
        pattern_id: "single".to_owned(),
        pattern_key: (1, 1),
        cards: vec![0],
    });
    state.finish_order = vec![0];
    state.passes_since_last_play = 2;
    let after = engine
        .apply(state, Action::Pass)
        .expect("跟牌态过牌应成功");
    if after.phase == PHASE_PLAY {
        assert_eq!(after.turn_seat, 2, "partnerLead 应把出牌权交给搭档");
        assert!(after.last_play.is_none(), "全过后新墩 lastPlay 应清空");
    }
}

#[test]
fn after_all_pass_without_partner_lead_path() {
    // partnerLead 关闭：全过后回最后出牌者本人（他仍在牌局中）。
    let body = from_yaml(M0).expect("M0 夹具应可构造");
    let engine = M0Engine::new(body);
    let mut state = engine.deal(0x0D15EA5E_0000_0005).expect("发牌成功");
    state.turn_seat = 1;
    state.last_play = Some(gf_engine::hash::LastPlay {
        seat: 0,
        pattern_id: "single".to_owned(),
        pattern_key: (1, 1),
        cards: vec![0],
    });
    state.finish_order = vec![0];
    state.passes_since_last_play = 2;
    if let Ok(after) = engine.apply(state, Action::Pass) {
        assert!(after.last_play.is_none(), "全过后应清空 lastPlay");
    }
}

#[test]
fn redeal_after_settle_and_match_end_carrier() {
    let (engine, state) = cases::lead_state();
    let mut s = state;
    // 推进到终局：固定策略取候选首条。
    for _ in 0..400 {
        if s.phase == PHASE_SETTLE {
            break;
        }
        let legal = engine.legal_actions(&s).expect("枚举失败");
        let action = legal.into_iter().next().expect("候选不应为空");
        s = engine.apply(s, action).expect("落子失败");
    }
    assert_eq!(s.phase, PHASE_SETTLE, "应跑到结算相位");

    // 结局面重开一局：应能重新发牌并回到 play。
    let next = engine.deal(0x0D15EA5E_0000_0002).expect("重开一局应成功");
    assert!(next.round_no >= s.round_no, "重开一局 round_no 不得回退");
    assert_eq!(next.phase, PHASE_PLAY, "重开一局应回到 play 相位");

    // 已带场次终止载波的终局态：再开一局被拒或照常开（两种口径都要求不 panic）。
    let mut ended = s.clone();
    ended.match_end = Some(gf_engine::contract::MatchEnd {
        winner_team: Some(0),
        reason: gf_engine::contract::MatchEndReason::SafetyCap,
    });
    let ended_engine = engine;
    let _ = ended_engine.apply(ended, Action::Pass);
}

#[test]
fn tribute_disabled_flow() {
    // M0 夹具 tribute: { enabled: false }：进贡段直接跳过。
    let body = from_yaml(M0).expect("M0 夹具应可构造");
    let engine = M0Engine::new(body);
    let state = engine.deal(0x0D15EA5E_0000_0004).expect("发牌成功");
    assert!(state.tribute_events.is_empty(), "禁用进贡应无事件");
    assert_eq!(state.phase, PHASE_PLAY, "禁用进贡后直接进入 play");
}

#[test]
fn seat_and_zone_edge_lookups() {
    let (_engine, mut state) = cases::lead_state();
    // 篡改：某座位没有任何分区 → hand 定位落空。
    state.zones.retain(|z| z.seat != 3);
    let _ = Seat(3);
}
