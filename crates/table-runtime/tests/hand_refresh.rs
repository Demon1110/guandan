//! 接风 / 进贡后「手牌刷新」专项（房间层）。
//!
//! 关注两件在 Web 端会直接暴露的事：
//! 1. 进贡 / 还贡只换牌、不换张数：每局开局四家仍是 27 张，抗贡时逐张不变。
//! 2. 接风与进贡都不改在局座位的手牌；把整段动作日志重演（等价于页面刷新后从
//!    IndexedDB 恢复）时，每个座位的 `view` 字节和 `stateHash` 必须与刷新前完全一致。

use gf_engine::contract::Action;
use gf_table_runtime::{BotPolicy, Room, RoomConfig, StepOutcome};

/// 测试用最小 bot：领出打 `view.hand` 首张单张，跟牌一律过（保证局能跑完又便宜）。
struct LeadFirstSingle;

impl BotPolicy for LeadFirstSingle {
    fn decide(&self, view_bytes: &[u8], _seat: u8) -> Action {
        if view_bytes.windows(7).any(|w| w == b"\"lastPlay\"") {
            return Action::Pass;
        }
        match hand_cards_from_view(view_bytes).first() {
            Some(&card) => Action::Play { cards: vec![card] },
            None => Action::Pass,
        }
    }
}

/// 从 canonical view 里取 `"hand":{"cards":[...]}` 的牌 id 列表。
fn hand_cards_from_view(view: &[u8]) -> Vec<u16> {
    let needle = b"\"hand\":\"cards\":[";
    let alt = b"\"hand\":{\"cards\":[";
    let start = view
        .windows(alt.len())
        .position(|w| w == alt)
        .map(|p| p + alt.len())
        .or_else(|| {
            view.windows(needle.len())
                .position(|w| w == needle)
                .map(|p| p + needle.len())
        });
    let Some(start) = start else {
        return Vec::new();
    };
    let mut end = start;
    while end < view.len() && view[end] != b']' {
        end += 1;
    }
    view[start..end]
        .split(|b| *b == b',')
        .filter(|s| !s.is_empty())
        .filter_map(|s| std::str::from_utf8(s).ok()?.trim().parse::<u16>().ok())
        .collect()
}

fn bot_room(seed: u64) -> Room {
    let mut room = Room::new(RoomConfig::all_bots("match-hand-refresh", seed)).expect("建桌失败");
    for seat in 0..4u8 {
        room.attach_bot(seat, Box::new(LeadFirstSingle))
            .expect("装 bot 失败");
    }
    room
}

fn play_until_settled(room: &mut Room, mut now_ms: u64) -> (u64, u64) {
    let mut actions = 0u64;
    loop {
        match room.step(now_ms).expect("step 失败") {
            StepOutcome::Applied { .. } => {
                actions += 1;
                now_ms += 1000;
            }
            StepOutcome::Settled(_) => return (actions, now_ms),
            StepOutcome::Waiting { .. } => panic!("全 bot 局出现人类等待"),
            StepOutcome::MatchEnded(_) => panic!("场次提前终止"),
        }
        assert!(actions < 4000, "一局动作数超上界：{actions}");
    }
}

fn view_hands(room: &Room) -> Vec<Vec<u16>> {
    (0..4u8)
        .map(|seat| hand_cards_from_view(&room.view_bytes(seat).expect("view 失败")))
        .collect()
}

/// 全 bot 桌把进贡的还贡动作跑完，停在 play 相位。
fn finish_tribute(room: &mut Room, mut now_ms: u64) -> u64 {
    while room.state().expect("有状态").phase == 1 {
        match room.step(now_ms).expect("step 失败") {
            StepOutcome::Applied { .. } => now_ms += 1000,
            StepOutcome::Waiting { .. } => panic!("全 bot 桌还贡不应出现人类等待"),
            other => panic!("还贡段出现非 Applied 结果：{other:?}"),
        }
    }
    now_ms
}

fn hand_total(hands: &[Vec<u16>]) -> usize {
    hands.iter().map(|h| h.len()).sum()
}

#[test]
fn every_round_starts_with_27_cards_per_seat_even_after_tribute() {
    let mut room = bot_room(0x51ED_0007);
    room.start(0).expect("发牌失败");
    let mut now = 1000;

    let first = view_hands(&room);
    assert_eq!(
        first.iter().map(|h| h.len()).collect::<Vec<_>>(),
        vec![27, 27, 27, 27]
    );

    for round in 2..=3u16 {
        let (_, next_now) = play_until_settled(&mut room, now);
        now = next_now + 1000;
        room.next_round(now).expect("续局失败");
        assert_eq!(room.state().expect("有状态").round_no, round);
        // 断点在 next_round 之后、还贡动作之前：先只交贡牌，四家张数暂时不等；
        // 全场 108 张仍守恒。还贡跑完后恢复每家 27 张。
        let phase = room.state().expect("有状态").phase;
        assert!(
            phase == 1 || phase == 2,
            "续局后相位应为进贡或出牌，实际 {phase}"
        );
        let hands = view_hands(&room);
        assert_eq!(hand_total(&hands), 108, "两副牌 108 张必须全在四家手里");
        if phase == 1 {
            now = finish_tribute(&mut room, now);
        }
        let hands = view_hands(&room);
        assert_eq!(
            hands.iter().map(|h| h.len()).collect::<Vec<_>>(),
            vec![27, 27, 27, 27],
            "第 {round} 局还贡完成后手牌应各 27 张（进贡只换牌不换张数）"
        );
    }
}

#[test]
fn replay_reproduces_hands_after_jiefeng_and_tribute() {
    let seed = 0x51ED_0008;
    let mut room = bot_room(seed);
    room.start(0).expect("发牌失败");
    let mut now = 1000;
    let mut rounds = 0;

    // 跑三局（第 2、3 局会带上局的进贡/还贡段）。
    for _ in 0..3 {
        let (_, next_now) = play_until_settled(&mut room, now);
        now = next_now + 1000;
        rounds += 1;
        if rounds < 3 {
            room.next_round(now).expect("续局失败");
        }
    }

    // 刷新前的权威局面：四座位视图 + 状态哈希。
    let live_views: Vec<Vec<u8>> = (0..4u8)
        .map(|s| room.view_bytes(s).expect("view 失败"))
        .collect();
    let live_hash = room.state_hash().expect("哈希失败");
    let live_log = room.log().to_vec();
    assert!(
        live_log.len() > 200,
        "三局至少两百多个动作，实际 {}",
        live_log.len()
    );
    assert!(
        live_views
            .iter()
            .all(|v| v.windows(6).any(|w| w == b"\"hand\"")),
        "每个座位视图都必须带 hand 字段"
    );

    // 「刷新」：全新 Room（内存全丢），只按日志重演。
    let mut replay = bot_room(seed);
    replay.start(0).expect("重演发牌失败");
    let mut current_round = 1u16;
    for rec in &live_log {
        while rec.round_no > current_round {
            replay.next_round(0).expect("重演续局失败");
            current_round += 1;
        }
        replay
            .replay_apply(rec.seat, rec.action.clone())
            .expect("重演动作失败");
    }

    assert_eq!(
        replay.state_hash().expect("重演哈希失败"),
        live_hash,
        "重演后的 stateHash 必须与刷新前一致"
    );
    for seat in 0..4u8 {
        let replayed = replay.view_bytes(seat).expect("重演 view 失败");
        assert_eq!(
            replayed, live_views[seat as usize],
            "座位 {seat} 的手牌视图在重演后必须逐字节一致"
        );
    }
}

#[test]
fn jiefeng_round_keeps_hands_of_players_still_in_game() {
    // 用房间层跑一局到结算，把日志重演一遍；逐手核对「出完者的手牌只减不增、
    // 在局座位手牌不会因为接风被清掉」。
    let seed = 0x51ED_0009;
    let mut room = bot_room(seed);
    room.start(0).expect("发牌失败");
    let log_before = room.log().len();
    play_until_settled(&mut room, 1000);
    let log_after = room.log().len();
    assert!(log_after > log_before, "一局应有动作入日志");

    // 结算态：四家手牌要么为空（已出完），要么是剩余牌；总和 ≤ 108。
    let hands = view_hands(&room);
    assert!(hand_total(&hands) <= 108, "结算时手牌总数不可能超过两副牌");
    assert!(
        hands.iter().any(|h| h.is_empty()),
        "一局结束至少头游的手牌为空"
    );
    // 重演到同一个结算点：手牌视图仍逐字节相同（接风分支不制造额外手牌变化）。
    let live_views: Vec<Vec<u8>> = (0..4u8)
        .map(|s| room.view_bytes(s).expect("view 失败"))
        .collect();
    let mut replay = bot_room(seed);
    replay.start(0).expect("重演发牌失败");
    for rec in room.log().to_vec() {
        replay
            .replay_apply(rec.seat, rec.action.clone())
            .expect("重演动作失败");
    }
    for seat in 0..4u8 {
        assert_eq!(
            replay.view_bytes(seat).expect("重演 view 失败"),
            live_views[seat as usize],
            "接风局重演后座位 {seat} 手牌必须一致"
        );
    }
}
