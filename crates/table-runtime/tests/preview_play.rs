//! 出牌预检：`M0Engine::preview_play` 必须与 `submit` 同口径。
//!
//! 页面「出牌」按钮的可用状态直接读它——`legal_actions` 只是候选清单，顺子、三带二这类
//! 多组合牌型不会把用户手里的每个具体花色组合都枚举出来，只按清单比对会把合法牌判成非法。

use gf_engine::contract::{Action, EngineError};
use gf_table_runtime::{PlayerKind, Room, RoomConfig, RoomError, StepOutcome};

fn human_room(seed: u64) -> Room {
    Room::new(RoomConfig {
        match_id: "match-preview".to_owned(),
        ruleset_version: gf_table_runtime::DEFAULT_RULESET_VERSION.to_owned(),
        seed,
        players: vec![PlayerKind::Human; 4],
    })
    .expect("建桌失败")
}

/// 走到第一个需要人类行动的轮次，返回 (座位, 时间戳)。
fn wait_for_human(room: &mut Room) -> (u8, u64) {
    let mut now = 0u64;
    loop {
        match room.step(now).expect("step 失败") {
            StepOutcome::Waiting { seat, .. } => return (seat, now),
            StepOutcome::Applied { .. } => now += 1000,
            _ => panic!("未开局就走到意外分支"),
        }
    }
}

#[test]
fn preview_accepts_legal_candidate_and_submit_agrees() {
    let mut room = human_room(0x51ED_0030);
    room.start(0).expect("发牌失败");
    let (seat, now) = wait_for_human(&mut room);

    let legal = room.legal_now().expect("合法集失败");
    let play = legal
        .iter()
        .find_map(|a| match a {
            Action::Play { cards } => Some(cards.clone()),
            Action::Pass => None,
            Action::TributeReturn { .. } => None,
        })
        .expect("领出必有可出牌型");

    let (pattern_id, beats) = {
        let state = room.state().expect("取状态失败");
        room.engine()
            .preview_play(state, &play)
            .expect("预检应通过合法候选")
    };
    assert!(beats, "领出时不受上家压制");
    assert!(!pattern_id.is_empty(), "预检应给出牌型 id");

    // 预检说能出，提交就必须成功——两条路径不能分叉。
    room.submit(seat, Action::Play { cards: play }, now)
        .expect("预检通过的动作提交应成功");
}

#[test]
fn preview_rejects_cards_not_in_hand_and_broken_shapes() {
    let mut room = human_room(0x51ED_0031);
    room.start(0).expect("发牌失败");
    let (seat, _now) = wait_for_human(&mut room);
    let state = room.state().expect("取状态失败");

    // 手牌里没有的牌：必须拒。
    let err = room
        .engine()
        .preview_play(state, &[9999])
        .expect_err("不在手牌的牌应拒");
    assert!(
        matches!(err, EngineError::IllegalAction { .. }),
        "应为 IllegalAction"
    );

    // 同一张牌重复提交：必须拒（否则 recognize 会把同一张牌算两次）。
    let mut hand_cards = state
        .zones
        .iter()
        .find(|z| z.seat == seat && !z.cards.is_empty())
        .map(|z| z.cards.clone())
        .expect("手牌区缺失");
    hand_cards.truncate(1);
    let dup = [hand_cards[0], hand_cards[0]];
    let err = room
        .engine()
        .preview_play(state, &dup)
        .expect_err("重复牌应拒");
    assert!(
        matches!(err, EngineError::IllegalAction { .. }),
        "应为 IllegalAction"
    );
}

#[test]
fn submit_accepts_hand_picked_play_outside_candidate_list() {
    // 回归：合法集只枚举每种牌型的代表组合（单张只列能压的那一张）。
    // 用户在牌桌上手选的另一张同型牌，预检能过、提交也必须能过，
    // 否则会出现「按钮亮着、点出牌被拒」。
    let mut room = human_room(0x51ED_0033);
    room.start(0).expect("发牌失败");
    let (lead_seat, now) = wait_for_human(&mut room);

    let lead_single = room
        .legal_now()
        .expect("合法集失败")
        .iter()
        .find_map(|a| match a {
            Action::Play { cards } if cards.len() == 1 => Some(cards.clone()),
            _ => None,
        })
        .expect("领出必有单张候选");
    room.submit(lead_seat, Action::Play { cards: lead_single }, now)
        .expect("领出单张应成功");

    let (seat, now) = wait_for_human(&mut room);
    let legal_now = room.legal_now().expect("合法集失败");
    let legal_singles: Vec<u16> = legal_now
        .iter()
        .filter_map(|a| match a {
            Action::Play { cards } if cards.len() == 1 => Some(cards[0]),
            _ => None,
        })
        .collect();

    let outside = {
        let state = room.state().expect("取状态失败");
        let engine = room.engine();
        let hand = state
            .zones
            .iter()
            .find(|z| z.seat == seat && !z.cards.is_empty())
            .map(|z| z.cards.clone())
            .expect("手牌区缺失");
        hand.into_iter()
            .find(|&id| {
                !legal_singles.contains(&id)
                    && matches!(engine.preview_play(state, &[id]), Ok((_, true)))
            })
            .expect("该有一张不在候选清单里、但压得过上家的单张")
    };

    assert!(
        !legal_now.contains(&Action::Play {
            cards: vec![outside]
        }),
        "前提：这张单张不在合法集里（否则用例测不到白名单外的手选路径）"
    );
    room.submit(
        seat,
        Action::Play {
            cards: vec![outside],
        },
        now,
    )
    .expect("预检能过的手选单张，提交通道也必须放行");
}

#[test]
fn preview_rejects_before_deal() {
    let room = human_room(0x51ED_0032);
    // 未开局：房间没有状态，合法集也报 NotStarted，预检同样无从谈起。
    assert!(matches!(
        room.state().expect_err("未开局应报错"),
        RoomError::NotStarted
    ));
    assert!(matches!(
        room.legal_now().expect_err("未开局应报错"),
        RoomError::NotStarted
    ));
}
