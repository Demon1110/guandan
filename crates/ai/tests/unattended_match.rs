//! AI 档位用例 + 「四 bot 无人值守打完两局」的 M1 硬 gate 自动化演示。
//!
//! - `l0_*`：AI-L0 的旧入口（无合法集）行为不变——领出最小单张、跟牌一律过；
//! - `l1_*`：AI-L1 在合法集里的取舍——跟牌不再无脑过、领出先走多张、留炸弹、配合队友；
//! - `l2_*`：AI-L2 的记牌（留控制牌）与炸弹时机（按手上的炸弹数定门槛）；
//! - `four_l2_bots_*`：四 bot 全程无人值守两局，且不得出现非法动作退化。

use gf_ai::{L0Bot, L1Bot, L2Bot};
use gf_engine::contract::Action;
use gf_table_runtime::{BotContext, BotPolicy, Room, RoomConfig, StepOutcome};

fn play(cards: Vec<u16>) -> Action {
    Action::Play { cards }
}

#[test]
fn l0_decide_leads_lowest_single_and_passes_when_following() {
    let bot = L0Bot;
    // 领出态：出牌 id 最小的一张单张。
    let lead_view = br#"{"hand":{"cards":[4,9,15]},"seat.levels":[0,0],"round.level":0}"#;
    assert_eq!(bot.decide(lead_view, 0), play(vec![4]));
    // 他座 lastPlay → 跟牌态：一律过（L0 不做牌力比较）。
    let follow_view = br#"{"hand":{"cards":[4,9,15]},"seat.levels":[0,0],"lastPlay":{"seat":1,"patternId":"single","cards":[30]},"round.level":0}"#;
    assert_eq!(bot.decide(follow_view, 0), Action::Pass);
    // 自己 lastPlay（回合清空交回 / 接风回手）→ 仍按领出打最小单张。
    let lead_back_view = br#"{"hand":{"cards":[4,9,15]},"seat.levels":[0,0],"lastPlay":{"seat":0,"patternId":"single","cards":[70]},"round.level":0}"#;
    assert_eq!(
        bot.decide(lead_back_view, 0),
        play(vec![4]),
        "lastPlay 来自自己时是领出，不是跟牌"
    );
    // 空手：过（合法性由房间复核，这里只保证不 panic）。
    let empty_view = br#"{"hand":{"cards":[]},"seat.levels":[0,0],"round.level":0}"#;
    assert_eq!(bot.decide(empty_view, 0), Action::Pass);
}

#[test]
fn l0_with_legal_plays_lowest_candidate() {
    let bot = L0Bot;
    let view = br#"{"hand":{"cards":[4,9,15]},"seat.levels":[0,0],"lastPlay":{"seat":1,"patternId":"single","cards":[30]},"round.level":0}"#;
    // 合法集里最小的单张是 9（rank 4 压 30 的 rank 2）。
    let legal = vec![Action::Pass, play(vec![9]), play(vec![15])];
    assert_eq!(bot.decide_with_legal(view, 0, &legal), play(vec![9]));
    // 只能过时过。
    assert_eq!(
        bot.decide_with_legal(view, 0, &[Action::Pass]),
        Action::Pass
    );
}

#[test]
fn l1_follows_with_cheapest_same_shape_instead_of_passing() {
    let bot = L1Bot;
    // 对手出单张（rank 2 = "4"），我有 9 / 15 两张单张能压。
    let view = br#"{"hand":{"cards":[4,9,15,22,23]},"seat.levels":[0,0],"lastPlay":{"seat":1,"patternId":"single","cards":[30]},"round.level":0}"#;
    let legal = vec![Action::Pass, play(vec![9]), play(vec![15])];
    assert_eq!(
        bot.decide_with_legal(view, 0, &legal),
        play(vec![9]),
        "跟牌应压最小能压的那张，而不是无脑过"
    );
}

#[test]
fn l1_keeps_bomb_when_a_cheap_same_shape_exists() {
    let bot = L1Bot;
    // 对手出单张 4，我既能出单张 9，又能炸；应出单张。
    let view = br#"{"hand":{"cards":[6,7,32,33,9,20,21]},"seat.levels":[0,0],"lastPlay":{"seat":1,"patternId":"single","cards":[30]},"round.level":0}"#;
    let legal = vec![
        Action::Pass,
        play(vec![6, 7, 32, 33]),
        play(vec![9]),
        play(vec![20]),
    ];
    assert_eq!(bot.decide_with_legal(view, 0, &legal), play(vec![9]));
}

#[test]
fn l1_uses_bomb_when_no_same_shape_and_opponent_is_big() {
    let bot = L1Bot;
    // 对手出单张 A（rank 12，id 24）；我只有炸弹能压（合法性由 legal 表达）。
    let view = br#"{"hand":{"cards":[6,7,32,33,20,21]},"seat.levels":[0,0],"lastPlay":{"seat":1,"patternId":"single","cards":[24]},"round.level":0}"#;
    // 单张里没有比 A 更大的（合法集只给炸弹），所以只能靠炸弹压。
    let legal = vec![Action::Pass, play(vec![6, 7, 32, 33])];
    assert_eq!(
        bot.decide_with_legal(view, 0, &legal),
        play(vec![6, 7, 32, 33]),
        "对手压 A 且只有炸弹能压时应动炸弹"
    );
}

#[test]
fn l1_leads_with_multi_card_shape_to_clear_hand() {
    let bot = L1Bot;
    // 领出：手上有顺子 5 张与单张，应优先走 5 张。
    let view = br#"{"hand":{"cards":[0,2,4,6,8,10]},"seat.levels":[0,0],"round.level":0}"#;
    let legal = vec![play(vec![0, 2, 4, 6, 8]), play(vec![10])];
    assert_eq!(
        bot.decide_with_legal(view, 0, &legal),
        play(vec![0, 2, 4, 6, 8]),
        "领出应先走多张牌型"
    );
}

#[test]
fn l1_finishes_hand_when_one_candidate_empties_it() {
    let bot = L1Bot;
    let view = br#"{"hand":{"cards":[6,7,32]},"seat.levels":[0,0],"round.level":0}"#;
    let legal = vec![play(vec![6]), play(vec![6, 7, 32])];
    assert_eq!(
        bot.decide_with_legal(view, 0, &legal),
        play(vec![6, 7, 32]),
        "能一把走完就出完"
    );
}

#[test]
fn l1_passes_when_partner_holds_the_lead() {
    let bot = L1Bot;
    // 上一手是搭档（座位 2）出的对子；我手里 8 张、能压但不抢。
    let view = br#"{"hand":{"cards":[0,2,4,6,8,10,12,14]},"seat.levels":[0,0],"lastPlay":{"seat":2,"patternId":"pair","cards":[30,31]},"round.level":0}"#;
    let ctx = BotContext {
        partner: Some(2),
        partner_hand: Some(9),
        own_hand: 8,
    };
    let legal = vec![Action::Pass, play(vec![40, 41])];
    assert_eq!(
        bot.decide_with_ctx(view, 0, &legal, &ctx),
        Action::Pass,
        "搭档拿牌权时应让牌，不抢自己人"
    );
}

#[test]
fn l1_takes_over_when_partner_leads_but_own_hand_is_short() {
    let bot = L1Bot;
    // 自己只剩 4 张：可以接管收尾，不无条件让牌。
    let view = br#"{"hand":{"cards":[40,41,44,45]},"seat.levels":[0,0],"lastPlay":{"seat":2,"patternId":"pair","cards":[30,31]},"round.level":0}"#;
    let ctx = BotContext {
        partner: Some(2),
        partner_hand: Some(12),
        own_hand: 4,
    };
    let legal = vec![Action::Pass, play(vec![40, 41])];
    assert_eq!(
        bot.decide_with_ctx(view, 0, &legal, &ctx),
        play(vec![40, 41])
    );
}

#[test]
fn l2_keeps_controller_shape_and_leads_the_cheap_one() {
    // 记牌：四张王都已在 discard 里出完，手上的 10-J-Q-K-A 顺子顶 A 且无人能压（控制牌）。
    let view = br#"{"hand":{"cards":[0,1,16,18,20,22,24]},"seat.levels":[0,0],"discard":{"cards":[104,105,106,107]},"round.level":0}"#;
    let straight = play(vec![16, 18, 20, 22, 24]);
    let pair_low = play(vec![0, 1]);
    let legal = vec![straight.clone(), pair_low.clone()];
    // L1 只看张数：先走 5 张顺子。
    assert_eq!(L1Bot.decide_with_legal(view, 0, &legal), straight);
    // L2 记牌后把控制牌留到收尾：先走小对子。
    let ctx = BotContext::default();
    assert_eq!(
        L2Bot.decide_with_ctx(view, 0, &legal, &ctx),
        pair_low,
        "控制牌应留作收尾牌权"
    );
}

#[test]
fn l2_holds_single_bomb_when_hand_is_thick() {
    // 炸弹时机：手牌 16 张、只有一个炸弹，对手压 10 —— L1 会炸，L2 留炸。
    let view = br#"{"hand":{"cards":[6,7,32,33,0,1,2,3,4,5,8,9,10,11,12,13]},"seat.levels":[0,0],"lastPlay":{"seat":1,"patternId":"single","cards":[20]},"round.level":0}"#;
    let bomb = play(vec![6, 7, 32, 33]);
    let legal = vec![Action::Pass, bomb.clone()];
    assert_eq!(L1Bot.decide_with_legal(view, 0, &legal), bomb);
    assert_eq!(
        L2Bot.decide_with_legal(view, 0, &legal),
        Action::Pass,
        "手牌还厚且只有一炸时应留住"
    );
}

/// 四 bot 无人值守推完一局：返回结算摘要与动作数。
fn drive_round(room: &mut Room, mut now_ms: u64) -> (gf_table_runtime::SettleSummary, u64) {
    let mut actions = 0u64;
    loop {
        match room.step(now_ms).expect("step 失败") {
            StepOutcome::Applied { .. } => {
                actions += 1;
                now_ms += 1000;
            }
            StepOutcome::Waiting { seat, .. } => {
                panic!("纯 bot 局不得等待人类：seat={seat}");
            }
            StepOutcome::Settled(summary) => return (summary, actions),
            StepOutcome::MatchEnded(end) => panic!("场次提前终止：{end:?}"),
        }
        assert!(actions < 4000, "动作数超上界");
    }
}

#[test]
fn four_l2_bots_play_two_rounds_unattended() {
    let mut room = Room::new(RoomConfig::all_bots("match-ai-l2", 0x51ED_A101)).expect("建桌失败");
    for seat in 0..4u8 {
        room.attach_bot(seat, Box::new(L2Bot)).expect("挂 bot 失败");
    }
    room.start(0).expect("发牌失败");

    let (first, actions) = drive_round(&mut room, 1000);
    assert!(actions > 50, "一局动作数太少：{actions}");
    assert_eq!(first.ranks.len(), 4);
    assert!(first.winner_team.is_some());
    assert!(first.match_end.is_none());
    // 回放录制自证。
    assert_eq!(
        room.replay_hash().expect("重放失败"),
        room.state_hash().expect("哈希失败")
    );
    // L2 全程合法：房间不应记下任何 bot 非法退化。
    let illegal = room
        .auto_actions()
        .iter()
        .filter(|a| a.reason == gf_table_runtime::AutoReason::BotIllegal)
        .count();
    assert_eq!(illegal, 0, "L2 不应出现非法动作退化");

    room.next_round(1_000_000).expect("再来一局失败");
    let (second, _) = drive_round(&mut room, 1_001_000);
    assert_eq!(second.round_no, 2);
    assert_eq!(
        room.replay_hash().expect("跨局重放失败"),
        room.state_hash().expect("哈希失败")
    );
}
