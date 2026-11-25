//! AI-L0 档位用例 + 「四 bot 无人值守打完两局」的 M1 硬 gate 自动化演示。

use gf_ai::L0Bot;
use gf_engine::contract::Action;
use gf_table_runtime::{BotPolicy, Room, RoomConfig, StepOutcome};

#[test]
fn l0_leads_lowest_single_and_passes_when_following() {
    let bot = L0Bot;
    // 领出态：出牌 id 最小的一张单张。
    let lead_view = br#"{"hand":{"cards":[4,9,15]},"seat.levels":[0,0],"round.level":0}"#;
    assert_eq!(
        bot.decide(lead_view, 0),
        Action::Play { cards: vec![4] }
    );
    // 他座 lastPlay → 跟牌态：一律过（L0 不做牌力比较）。
    let follow_view = br#"{"hand":{"cards":[4,9,15]},"seat.levels":[0,0],"lastPlay":{"seat":1,"patternId":"single","cards":[30]},"round.level":0}"#;
    assert_eq!(bot.decide(follow_view, 0), Action::Pass);
    // 自己 lastPlay（回合清空交回 / 接风回手）→ 仍按领出打最小单张。
    let lead_back_view = br#"{"hand":{"cards":[4,9,15]},"seat.levels":[0,0],"lastPlay":{"seat":0,"patternId":"single","cards":[70]},"round.level":0}"#;
    assert_eq!(
        bot.decide(lead_back_view, 0),
        Action::Play { cards: vec![4] },
        "lastPlay 来自自己时是领出，不是跟牌"
    );
    // 空手：过（合法性由房间复核，这里只保证不 panic）。
    let empty_view = br#"{"hand":{"cards":[]},"seat.levels":[0,0],"round.level":0}"#;
    assert_eq!(bot.decide(empty_view, 0), Action::Pass);
}

/// 四 bot 无人值守推完一局：返回动作数。
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
fn four_l0_bots_play_two_rounds_unattended() {
    let mut room = Room::new(RoomConfig::all_bots("match-ai-l0", 0x51ED_A101)).expect("建桌失败");
    for seat in 0..4u8 {
        room.attach_bot(seat, Box::new(L0Bot)).expect("挂 bot 失败");
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
    // L0 全程合法：房间不应记下任何 bot 非法退化。
    let illegal = room
        .auto_actions()
        .iter()
        .filter(|a| a.reason == gf_table_runtime::AutoReason::BotIllegal)
        .count();
    assert_eq!(illegal, 0, "L0 不应出现非法动作退化");

    room.next_round(1_000_000).expect("再来一局失败");
    let (second, _) = drive_round(&mut room, 1_001_000);
    assert_eq!(second.round_no, 2);
    assert_eq!(
        room.replay_hash().expect("跨局重放失败"),
        room.state_hash().expect("哈希失败")
    );
}
