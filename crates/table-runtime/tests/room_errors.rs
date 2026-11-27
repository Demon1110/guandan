//! 房间层错误出口与状态守卫：`RoomError` 七个变体、相位守卫、全员离线中止。
//!
//! 这些分支在页面里表现为「出牌被拒 / 续局被拒」，线上很少走到，但每条都是一个
//! 客户端能触发的入口，必须逐条钉死行为（不能 panic、不能静默吞掉）。

use gf_engine::contract::Action;
use gf_table_runtime::{AutoReason, PlayerKind, Room, RoomConfig, RoomError, StepOutcome};

struct LeadFirstSingle;

impl gf_table_runtime::BotPolicy for LeadFirstSingle {
    fn decide(&self, view_bytes: &[u8], _seat: u8) -> Action {
        if view_bytes.windows(7).any(|w| w == b"\"lastPlay\"") {
            return Action::Pass;
        }
        let needle = b"\"hand\":{\"cards\":[";
        match view_bytes
            .windows(needle.len())
            .position(|w| w == needle)
            .map(|p| p + needle.len())
        {
            Some(start) => {
                let mut end = start;
                while end < view_bytes.len() && view_bytes[end].is_ascii_digit() {
                    end += 1;
                }
                let card = std::str::from_utf8(&view_bytes[start..end])
                    .ok()
                    .and_then(|s| s.parse::<u16>().ok());
                match card {
                    Some(card) => Action::Play { cards: vec![card] },
                    None => Action::Pass,
                }
            }
            None => Action::Pass,
        }
    }
}

fn human_room(seed: u64) -> Room {
    Room::new(RoomConfig {
        match_id: "match-errors".to_owned(),
        ruleset_version: gf_table_runtime::DEFAULT_RULESET_VERSION.to_owned(),
        seed,
        players: vec![PlayerKind::Human; 4],
    })
    .expect("建桌失败")
}

fn bot_room(seed: u64) -> Room {
    let mut room = Room::new(RoomConfig::all_bots("match-errors-bot", seed)).expect("建桌失败");
    for seat in 0..4u8 {
        room.attach_bot(seat, Box::new(LeadFirstSingle))
            .expect("装 bot 失败");
    }
    room
}

/// 把一局推到底（全 bot）。
fn play_until_settled(room: &mut Room, mut now_ms: u64) -> u64 {
    loop {
        match room.step(now_ms).expect("step 失败") {
            StepOutcome::Applied { .. } => now_ms += 1000,
            StepOutcome::Settled(_) => return now_ms,
            StepOutcome::Waiting { .. } => panic!("全 bot 局不应等待"),
            StepOutcome::MatchEnded(_) => panic!("场次提前终止"),
        }
    }
}

#[test]
fn out_of_range_seat_is_no_such_seat_everywhere() {
    let mut room = bot_room(0x51ED_0010);
    room.start(0).expect("发牌失败");

    assert!(matches!(
        room.view_bytes(4).expect_err("越界座位必须拒绝"),
        RoomError::NoSuchSeat(4)
    ));
    assert!(matches!(
        room.seat_seq(4).expect_err("越界座位必须拒绝"),
        RoomError::NoSuchSeat(4)
    ));
    assert!(matches!(
        room.submit(4, Action::Pass, 1000)
            .expect_err("越界座位必须拒绝"),
        RoomError::NoSuchSeat(4)
    ));
    assert!(matches!(
        room.attach_bot(4, Box::new(LeadFirstSingle))
            .expect_err("越界座位必须拒绝"),
        RoomError::NoSuchSeat(4)
    ));
    assert!(matches!(
        room.disconnect(9, 0).expect_err("越界座位必须拒绝"),
        RoomError::NoSuchSeat(9)
    ));
    assert!(matches!(
        room.reconnect(9, 0).expect_err("越界座位必须拒绝"),
        RoomError::NoSuchSeat(9)
    ));
    // 合法座位仍然可用，确认上面不是「一律报错」。
    assert!(room.seat_seq(3).is_ok());
}

#[test]
fn pre_start_calls_report_not_started() {
    let mut room = bot_room(0x51ED_0011);
    assert!(matches!(
        room.state().expect_err("未发牌无状态"),
        RoomError::NotStarted
    ));
    assert!(matches!(
        room.view_bytes(0).expect_err("未发牌无视图"),
        RoomError::NotStarted
    ));
    assert!(matches!(
        room.step(0).expect_err("未发牌不能推进"),
        RoomError::NotStarted
    ));
    assert!(matches!(
        room.next_round(0).expect_err("未发牌不能续局"),
        RoomError::NotStarted
    ));
    assert!(matches!(
        room.replay_apply(0, Action::Pass)
            .expect_err("未发牌不能重演"),
        RoomError::NotStarted
    ));
    assert!(matches!(
        room.reconnect(0, 0).expect_err("未发牌不能重连"),
        RoomError::NotStarted
    ));
    // 未发牌时状态哈希也必须报错，而不是给一个假哈希。
    assert!(matches!(
        room.state_hash().expect_err("未发牌无哈希"),
        RoomError::NotStarted
    ));
}

#[test]
fn attach_bot_and_submit_respect_player_kind() {
    let mut room = human_room(0x51ED_0012);
    // 人类座位不能装 bot。
    assert!(matches!(
        room.attach_bot(0, Box::new(LeadFirstSingle))
            .expect_err("人类座位不该装 bot"),
        RoomError::NotHuman(0)
    ));

    let mut bots = bot_room(0x51ED_0013);
    bots.start(0).expect("发牌失败");
    // bot 座位不接受人类提交。
    assert!(matches!(
        bots.submit(0, Action::Pass, 1000)
            .expect_err("bot 座位不该接受人类提交"),
        RoomError::NotHuman(0)
    ));
    assert!(!bots.is_human(0));
    assert!(!bots.is_human(9), "越界座位按非人类处理");
}

#[test]
fn submit_and_replay_are_rejected_outside_play_phase() {
    let mut room = human_room(0x51ED_0014);
    // 未发牌 → NotStarted（先于 NotInPlay 判定）。
    assert!(matches!(
        room.submit(0, Action::Pass, 0).expect_err("未发牌不能提交"),
        RoomError::NotStarted
    ));

    room.start(0).expect("发牌失败");
    let now = play_a_round_human(&mut room);
    // 结算态：再提交 → NotInPlay；重演 → NotInPlay。
    assert_eq!(
        room.state().expect("有状态").phase,
        gf_engine::engine::PHASE_SETTLE
    );
    assert!(matches!(
        room.submit(0, Action::Pass, now)
            .expect_err("结算态不接受新动作"),
        RoomError::NotInPlay
    ));
    assert!(matches!(
        room.replay_apply(0, Action::Pass)
            .expect_err("结算态不接受重演"),
        RoomError::NotInPlay
    ));
    // 结算态 step 返回摘要（而不是错误），这条是页面「结算页」的数据来源。
    assert!(matches!(
        room.step(now + 10_000).expect("结算态 step 应成功"),
        StepOutcome::Settled(_)
    ));
}

/// 四人全人类：一路超时托管把一局推完，返回最后一个时间戳。
fn play_a_round_human(room: &mut Room) -> u64 {
    let mut now_ms = 0u64;
    loop {
        now_ms += gf_table_runtime::DEFAULT_TURN_MS + 1000;
        match room.step(now_ms).expect("step 失败") {
            StepOutcome::Applied { .. } => {}
            StepOutcome::Settled(_) => return now_ms,
            StepOutcome::Waiting { .. } => continue,
            StepOutcome::MatchEnded(_) => panic!("场次提前终止"),
        }
        assert!(now_ms < 8_000_000, "一局推进超预期");
    }
}

#[test]
fn offline_seat_waits_for_grace_then_auto_plays() {
    let mut room = human_room(0x51ED_0015);
    room.start(0).expect("发牌失败");
    let turn = room.state().expect("有状态").turn_seat;

    // 行动座位断线：宽限期内仍等重连，宽限结束转托管（Offline）。
    room.disconnect(turn, 1000).expect("断线失败");
    match room.step(1000).expect("step 失败") {
        StepOutcome::Waiting { seat, deadline_ms } => {
            assert_eq!(seat, turn);
            assert_eq!(
                deadline_ms,
                1000 + gf_table_runtime::DEFAULT_GRACE_MS,
                "宽限期从断线时刻起算"
            );
        }
        other => panic!("宽限期内不应托管：{other:?}"),
    }
    match room
        .step(1000 + gf_table_runtime::DEFAULT_GRACE_MS)
        .expect("step 失败")
    {
        StepOutcome::Applied { seat, auto, .. } => {
            assert_eq!(seat, turn);
            assert_eq!(auto, Some(AutoReason::Offline));
        }
        other => panic!("宽限期后应托管：{other:?}"),
    }
    // 重连后回到在线，不再托管。
    let plan = room.reconnect(turn, 0).expect("重连失败");
    assert!(plan.wire.verify());
    assert_eq!(
        room.state().expect("有状态").phase,
        gf_engine::engine::PHASE_PLAY
    );
}

#[test]
fn reconnect_does_not_extend_turn_deadline() {
    let mut room = human_room(0x51ED_0019);
    room.start(0).expect("发牌失败");
    let turn = room.state().expect("有状态").turn_seat;
    room.disconnect(turn, 0).expect("断线失败");
    let deadline = room.turn_deadline_ms().expect("窗口应已打开");
    room.reconnect(turn, 0).expect("重连失败");
    assert_eq!(
        room.turn_deadline_ms(),
        Some(deadline),
        "重连不得重开行动窗口"
    );
}

#[test]
fn all_offline_aborts_match_and_freezes_room() {
    let mut room = human_room(0x51ED_0016);
    room.start(0).expect("发牌失败");
    for seat in 0..4u8 {
        room.disconnect(seat, 0).expect("断线失败");
    }
    // 第 1 次 tick：记下全员离线起点，还未超时。
    room.tick(0).expect("tick 失败");
    assert!(!room.aborted(), "刚到全离线不应立即中止");
    // 只要有人回来，全员离线计时清零。
    room.reconnect(0, 0).expect("重连失败");
    room.tick(1000).expect("tick 失败");
    assert!(!room.aborted(), "有人在线就不中止");

    for seat in 0..4u8 {
        room.disconnect(seat, 1000).expect("断线失败");
    }
    room.tick(1000).expect("tick 失败");
    room.tick(1000 + gf_table_runtime::DEFAULT_OFFLINE_ALL_MS)
        .expect("tick 失败");
    assert!(room.aborted(), "全员离线超时后必须中止");

    assert!(matches!(
        room.step(0).expect_err("中止后不能推进"),
        RoomError::MatchEnded
    ));
    assert!(matches!(
        room.next_round(0).expect_err("中止后不能续局"),
        RoomError::MatchEnded
    ));
}

#[test]
fn mismatched_player_count_is_engine_error() {
    // 座位数与规则集不符：建桌阶段就拒，而不是发牌时才炸。
    let built = Room::new(RoomConfig {
        match_id: "match-bad-seats".to_owned(),
        ruleset_version: gf_table_runtime::DEFAULT_RULESET_VERSION.to_owned(),
        seed: 1,
        players: vec![PlayerKind::Human; 3],
    });
    match built {
        Ok(_) => panic!("3 个座位应被拒绝"),
        Err(err) => assert!(matches!(err, RoomError::Engine(_))),
    }
}

#[test]
fn legal_now_and_bot_context_stay_consistent_at_settle() {
    // 结算态调用：legal_now 应报 NotInPlay；bot_context 仍可读（0 张）。
    let mut room = bot_room(0x51ED_0017);
    room.start(0).expect("发牌失败");
    let now = play_until_settled(&mut room, 0);
    // 结算态调用：legal_now 给空集（引擎认为没有可选动作，而不是报错）。
    let legal = room.legal_now().expect("结算态合法集应可枚举");
    assert!(legal.is_empty(), "结算态没有可提交动作");
    let ctx = room.bot_context(0);
    // 结算后名次已定，但手牌区仍保留未出完的牌；上下文必须还能读出来。
    assert!(ctx.own_hand <= 27, "结算后手牌张数不应超过一手牌");
    assert_eq!(ctx.partner, Some(2), "classic 队伍为 [0,2] / [1,3]");
    assert_eq!(ctx.partner_hand, Some(room.bot_context(2).own_hand));
    // 未装 bot 的座位也能取上下文（own_hand 取自 hand 区）。
    let mut bare = Room::new(RoomConfig::all_bots("match-bare", 0x51ED_0018)).expect("建桌失败");
    bare.start(0).expect("发牌失败");
    assert_eq!(bare.bot_context(1).own_hand, 27);
    assert_eq!(bare.bot_context(1).partner_hand, Some(27));
    let _ = now;
}
