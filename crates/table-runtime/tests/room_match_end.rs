//! 房间层「边界与终局」：未开局/非结算态/错轮次/整场打满。补 `room.rs` 里
//! 只有走到 `matchEnd` 或未 `start` 才会执行的分支。

use gf_engine::contract::{Action, Engine, MatchEndReason, Seat};
use gf_table_runtime::{Room, RoomConfig, RoomError, StepOutcome};
use std::time::Instant;

const MATCH: &str = "test-match-end";

fn bot_room() -> Room {
    let mut room = Room::new(RoomConfig::all_bots(MATCH, 0x51ED_A101)).expect("建房间");
    room.start(0).expect("开局");
    room
}

#[test]
fn next_round_before_settle_is_not_in_play() {
    let mut room = bot_room();
    match room.next_round(0) {
        Err(RoomError::NotInPlay) => {}
        other => panic!("未结算态续局应 NotInPlay，实际 {other:?}"),
    }
}

#[test]
fn helpers_on_unstarted_room_report_not_started() {
    let mut room = Room::new(RoomConfig::all_bots(MATCH, 0x51ED_A101)).expect("建房间");
    // bot_context → own_hand_len → state() 失败即返回 0（不 panic）。
    let ctx = room.bot_context(0);
    assert_eq!(ctx.own_hand, 0, "未开局时手牌长度应为 0");
    assert_eq!(ctx.partner_hand, Some(0), "搭档手牌长度也应为 0");

    // resume_window / replay_hash / next_round 都应报 NotStarted。
    assert!(matches!(room.resume_window(0), Err(RoomError::NotStarted)));
    assert!(matches!(room.replay_hash(), Err(RoomError::NotStarted)));
    assert!(matches!(room.next_round(0), Err(RoomError::NotStarted)));

    // status 为空时 tick 不进「全员离线」分支，也不报错。
    room.tick(0).expect("未开局 tick 不应报错");
    assert!(!room.aborted(), "未开局不应被判全员离线中止");
}

#[test]
fn replay_apply_rejects_wrong_turn() {
    let mut room = bot_room();
    let turn = room.state().expect("已开局").turn_seat;
    let other = (turn + 1) % 4;
    let legal_for_other = room
        .engine()
        .legal_actions(room.state().expect("已开局"))
        .expect("取合法动作");
    // 用一条合法动作去打非行动座位，必须被轮次校验拦下（与动作本身是否合法无关）。
    let action = legal_for_other
        .into_iter()
        .next()
        .expect("领出态必有合法动作");
    match room.replay_apply(other, action) {
        Err(RoomError::WrongTurn { expected, actual }) => {
            assert_eq!(expected, turn);
            assert_eq!(actual, other);
        }
        other_err => panic!("错轮次重演应 WrongTurn，实际 {other_err:?}"),
    }
}

#[test]
fn all_bot_match_runs_to_match_end_and_then_locks_down() {
    let mut room = bot_room();
    let mut now = 0u64;
    let mut settled_rounds = 0usize;
    let mut last = StepOutcome::Waiting {
        seat: 0,
        deadline_ms: 0,
    };
    let start = Instant::now();
    let mut steps = 0usize;
    loop {
        steps += 1;
        assert!(steps < 8000, "整场应在有限步内结束");
        now += 1;
        match room.step(now).expect("step 不应报错") {
            StepOutcome::MatchEnded(end) => {
                last = StepOutcome::MatchEnded(end);
                break;
            }
            StepOutcome::Settled(summary) => {
                settled_rounds += 1;
                assert_eq!(summary.round_no as usize, settled_rounds, "局号应连续");
                last = StepOutcome::Settled(summary);
                // 每局结算后手动续局（房间层不再自动续），直到打满 matchCap。
                match room.next_round(now) {
                    Ok(()) => {}
                    Err(RoomError::MatchEnded) => {}
                    Err(e) => panic!("结算后续局不应失败：{e:?}"),
                }
            }
            StepOutcome::Applied { .. } | StepOutcome::Waiting { .. } => {}
        }
        // 若续局成功则继续；若 matchEnd 已置位，step 会走 MatchEnded 分支。
        if matches!(&last, StepOutcome::Settled(s) if s.match_end.is_some()) {
            // 再 step 一次拿 MatchEnded。
            now += 1;
            match room.step(now).expect("step 不应报错") {
                StepOutcome::MatchEnded(end) => {
                    last = StepOutcome::MatchEnded(end);
                    break;
                }
                other => panic!("matchEnd 后 step 应返回 MatchEnded，实际 {other:?}"),
            }
        }
    }
    let elapsed = start.elapsed();
    match &last {
        StepOutcome::MatchEnded(end) => {
            assert_ne!(
                end.reason,
                MatchEndReason::Aborted,
                "四 bot 全程在线不应中止"
            );
        }
        other => panic!("应打到 matchEnd，实际 {other:?}"),
    }
    assert!(!room.aborted(), "正常打满不应标记中止");
    assert!(
        settled_rounds >= 2,
        "至少应结算两局（实际 {settled_rounds}），耗时 {elapsed:?}"
    );

    // 终局后：续局、提交、step 全部锁死（submit 先查「人类座位」，bot 房会先报 NotHuman）。
    assert!(matches!(room.next_round(now), Err(RoomError::MatchEnded)));
    assert!(
        room.submit(Seat(0).0, Action::Pass, now).is_err(),
        "终局后提交必须被拒"
    );
    assert!(matches!(
        room.step(now).expect("终局 step 仍返回 Ok"),
        StepOutcome::MatchEnded(_)
    ));
}
