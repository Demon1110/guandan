//! M1 硬 gate 的 headless 全流程：建桌 → 发牌 → 行动窗口（计时 + 托管）→ 出牌 →
//! 结算 → 再来一局，外加断线恢复两条路径与回放录制自证。
//!
//! 时间全部用虚拟时钟（调用方推进），不读系统时间（§3.3 约束 2）。

use gf_engine::contract::{Action, Engine};
use gf_table_runtime::recovery::{
    recover_client, ClientLocal, ReconnectDecision, RecoveryOutcome,
};
use gf_table_runtime::{BotPolicy, PlayerKind, Room, RoomConfig, StepOutcome};

/// 测试用最小 bot：领出打手中第一张单张（view 里的 `hand` 首元素），跟牌一律过。
struct LeadFirstSingle;

impl BotPolicy for LeadFirstSingle {
    fn decide(&self, view_bytes: &[u8], _seat: u8) -> Action {
        if view_bytes.windows(7).any(|w| w == b"\"lastPlay\"") {
            return Action::Pass;
        }
        match first_hand_card(view_bytes) {
            Some(card) => Action::Play { cards: vec![card] },
            None => Action::Pass,
        }
    }
}

/// 从 canonical view 里取 `"hand":{"cards":[<first>,...` 的首个牌 id。
fn first_hand_card(view: &[u8]) -> Option<u16> {
    let needle = b"\"hand\":{\"cards\":[";
    let start = view
        .windows(needle.len())
        .position(|w| w == needle)? + needle.len();
    let mut end = start;
    while end < view.len() && view[end].is_ascii_digit() {
        end += 1;
    }
    if end == start {
        return None;
    }
    std::str::from_utf8(&view[start..end])
        .ok()?
        .parse::<u16>()
        .ok()
}

fn bot_room(seed: u64) -> Room {
    let mut room = Room::new(RoomConfig::all_bots("match-e2e", seed)).expect("建桌失败");
    for seat in 0..4u8 {
        room.attach_bot(seat, Box::new(LeadFirstSingle))
            .expect("装 bot 失败");
    }
    room
}

/// 把一局推到底（全 bot，无人值守），返回结算摘要与推进的动作数。
fn play_one_round(room: &mut Room, mut now_ms: u64) -> (gf_table_runtime::SettleSummary, u64, u64) {
    let mut actions = 0u64;
    loop {
        match room.step(now_ms).expect("step 失败") {
            StepOutcome::Applied { .. } => {
                actions += 1;
                now_ms += 1000;
            }
            StepOutcome::Waiting { .. } => {
                // 全 bot 局不该出现等待；出现即可判失败。
                panic!("全 bot 局出现等待人类动作");
            }
            StepOutcome::Settled(summary) => return (summary, actions, now_ms),
            StepOutcome::MatchEnded(end) => panic!("场次提前终止：{end:?}"),
        }
        assert!(actions < 4000, "一局动作数超预期上界：{actions}");
    }
}

#[test]
fn full_flow_deal_play_settle_next_round() {
    let mut room = bot_room(0x51ED_0001);
    room.start(0).expect("发牌失败");
    assert_eq!(room.state().expect("有状态").round_no, 1);

    let (first, actions, now_ms) = play_one_round(&mut room, 1000);
    assert!(actions > 100, "一局至少上百个动作，实际 {actions}");
    assert_eq!(first.ranks.len(), 4, "四人局名次四位");
    assert!(first.winner_team.is_some(), "结算必须给出头游所在队");
    assert!(first.match_end.is_none(), "首局不应终止场次");
    // 结算那一手也进日志：记录条数 = 已计数动作 + 1（收尾动作由 Settled 分支带出）。
    assert_eq!(room.log().len() as u64, actions + 1, "动作记录与执行数一致");

    // 回放录制自证：全量日志重放后的 stateHash 必须与权威状态逐字节相等。
    assert_eq!(
        room.replay_hash().expect("重放失败"),
        room.state_hash().expect("状态哈希失败"),
        "日志重放与权威状态哈希必须一致"
    );

    // 再来一局：局号 +1，跨局携带由引擎负责；这一局也必须能跑完。
    room.next_round(now_ms).expect("再来一局失败");
    assert_eq!(room.state().expect("有状态").round_no, 2);
    let (second, _, _) = play_one_round(&mut room, now_ms + 1000);
    assert_eq!(second.round_no, 2);
    assert_eq!(second.ranks.len(), 4);
    assert_eq!(
        room.replay_hash().expect("跨局重放失败"),
        room.state_hash().expect("状态哈希失败"),
        "跨两局的日志重放必须与权威状态一致"
    );
    // 跨局序号线不归零：roomSeq 单调累加。
    assert!(room.room_seq() > actions, "跨局 roomSeq 必须继续累加");
}

#[test]
fn action_window_timeout_triggers_auto_play() {
    // 四座位都是人类：不提交就必然超时。
    let mut room = Room::new(RoomConfig {
        match_id: "match-timeout".to_owned(),
        ruleset_version: gf_table_runtime::DEFAULT_RULESET_VERSION.to_owned(),
        seed: 0x51ED_0002,
        players: vec![PlayerKind::Human; 4],
    })
    .expect("建桌失败");
    room.start(0).expect("发牌失败");

    // 窗口内 → 等待人类。
    match room.step(1000).expect("step 失败") {
        StepOutcome::Waiting { seat, deadline_ms } => {
            assert_eq!(seat, room.state().expect("有状态").turn_seat);
            assert_eq!(deadline_ms, gf_table_runtime::DEFAULT_TURN_MS);
        }
        other => panic!("窗口内不应代打：{other:?}"),
    }
    // 越过窗口 → 托管代打，并记 Timeout。
    match room
        .step(gf_table_runtime::DEFAULT_TURN_MS + 1)
        .expect("step 失败")
    {
        StepOutcome::Applied {
            auto: Some(reason), ..
        } => assert_eq!(reason, gf_table_runtime::AutoReason::Timeout),
        other => panic!("超时应托管代打：{other:?}"),
    }
    assert_eq!(
        room.auto_actions().last().map(|a| a.reason),
        Some(gf_table_runtime::AutoReason::Timeout)
    );
    assert_eq!(room.log().len(), 1, "托管动作同样进动作记录");
}

#[test]
fn reconnect_delta_replays_to_same_hash() {
    let mut room = Room::new(RoomConfig {
        match_id: "match-reconnect".to_owned(),
        ruleset_version: gf_table_runtime::DEFAULT_RULESET_VERSION.to_owned(),
        seed: 0x51ED_0003,
        players: vec![PlayerKind::Bot; 4],
    })
    .expect("建桌失败");
    for seat in 0..4u8 {
        room.attach_bot(seat, Box::new(LeadFirstSingle))
            .expect("装 bot 失败");
    }
    room.start(0).expect("发牌失败");

    // 先推若干步，再按水位语义造「客户端副本」：本地状态只应用到自己最后一条动作
    // （`lastSeatSeq` 对应的 `roomSeq`）为止，之后的事件才是待重放的增量。
    let mut now_ms = 1000u64;
    for _ in 0..40 {
        room.step(now_ms).expect("step 失败");
        now_ms += 1000;
    }
    let seat = (0..4u8)
        .find(|s| room.seat_seq(*s).expect("水位查询失败") > 0)
        .expect("40 步内至少一个座位出过动作");
    let seat_seq = room.seat_seq(seat).expect("水位查询失败") - 1;
    let log: Vec<_> = room.log().to_vec();
    let watermark = log
        .iter()
        .find(|r| r.seat == seat && r.seat_seq == seat_seq)
        .expect("缓冲 / 日志里必须查得到该座位的动作")
        .room_seq;
    let prefix: Vec<_> = log.iter().filter(|r| r.room_seq <= watermark).cloned().collect();
    let mut client_state = room.engine().deal(room.round_seed(1)).expect("首局重发失败");
    for record in &prefix {
        client_state = room
            .engine()
            .apply(client_state, record.action.clone())
            .expect("客户端前缀重放失败");
    }
    let client = ClientLocal::new(Some(client_state), seat, seat_seq);

    // 服务端继续推进（bot 继续打），客户端掉线期间错过这些动作。
    for _ in 0..30 {
        room.step(now_ms).expect("step 失败");
        now_ms += 1000;
    }

    let plan = room.reconnect(seat, client.last_seat_seq).expect("重连失败");
    assert!(
        matches!(plan.decision, ReconnectDecision::Delta { .. }),
        "水位仍在缓冲内应为增量路径"
    );
    assert!(!plan.wire.snapshot_reset, "增量路径 snapshotReset 必须为 false");
    assert!(plan.wire.verify(), "服务端下发前 viewHash 自证必须通过");

    let mut local = client;
    let outcome = recover_client(room.engine(), &mut local, &plan.decision, &plan.snapshot)
        .expect("恢复失败");
    let state = match outcome {
        RecoveryOutcome::Delta { state, events_applied, .. } => {
            assert!(events_applied > 0, "增量路径必须带出事件");
            state
        }
        other => panic!("期望增量路径，实际 {other:?}"),
    };
    assert_eq!(
        gf_table_runtime::state_hash_of(&state),
        room.state_hash().expect("状态哈希失败"),
        "增量重放后的哈希必须与权威状态逐字节一致"
    );

    // 迁移自尊型用例的等价断言：恢复后的合法动作集与权威一致，同走一步仍同哈希。
    let server_state = room.state().expect("有状态").clone();
    if server_state.phase == gf_engine::engine::PHASE_PLAY {
        let legal_client = room
            .engine()
            .legal_actions(&state)
            .expect("客户端合法枚举失败");
        let legal_server = room
            .engine()
            .legal_actions(&server_state)
            .expect("服务端合法枚举失败");
        assert_eq!(legal_client, legal_server, "恢复后合法动作必须逐条相同");
        let step = legal_server.first().cloned().expect("非终局必有候选");
        let next_client = room
            .engine()
            .apply(state, step.clone())
            .expect("客户端同走一步失败");
        let next_server = room
            .engine()
            .apply(server_state, step)
            .expect("服务端同走一步失败");
        assert_eq!(
            gf_table_runtime::state_hash_of(&next_client),
            gf_table_runtime::state_hash_of(&next_server),
            "同走一步后两端哈希必须一致"
        );
    }
}

#[test]
fn reconnect_falls_back_to_snapshot_reset_when_evicted() {
    let mut room = bot_room(0x51ED_0004);
    room.start(0).expect("发牌失败");

    // 推满一局 + 连开下一局再推一段：总动作数越过 200 条缓冲容量，
    // seat 0 的早期记录（roomSeq 0/4/8…）必然被淘汰。
    let (summary, actions, now_ms) = play_one_round(&mut room, 1000);
    assert!(summary.ranks.len() == 4);
    room.next_round(now_ms + 1000).expect("再来一局失败");
    let mut cursor = now_ms + 1000;
    let mut extra = 0;
    while room.room_seq() <= gf_table_runtime::RECONNECT_BUFFER_CAPACITY as u64 + 1 {
        room.step(cursor).expect("step 失败");
        cursor += 1000;
        extra += 1;
        assert!(extra < 2000, "续局推进异常");
    }
    assert!(
        actions + extra > gf_table_runtime::RECONNECT_BUFFER_CAPACITY as u64,
        "动作数必须超过缓冲容量才能触发淘汰，实际 {}",
        actions + extra
    );

    let plan = room.reconnect(0, 0).expect("重连失败");
    assert!(
        matches!(plan.decision, ReconnectDecision::SnapshotReset),
        "记录已被淘汰必须落快照重置"
    );
    assert!(plan.wire.snapshot_reset, "快照重置路径 snapshotReset 必须为 true");
    assert!(plan.wire.verify());
    // wire 帧可编解码且 roundNo 走头字段（此时已在第 2 局）。
    let decoded = gf_table_runtime::RoomSnapshotWire::decode(&plan.wire.encode())
        .expect("wire 解码失败");
    assert_eq!(decoded.round_no, 2);
    assert!(decoded.snapshot_reset);

    let mut local = ClientLocal::new(None, 0, 0);
    let outcome = recover_client(room.engine(), &mut local, &plan.decision, &plan.snapshot)
        .expect("恢复失败");
    match outcome {
        RecoveryOutcome::SnapshotReset { state, snapshot_reset, .. } => {
            assert!(snapshot_reset);
            assert_eq!(
                gf_table_runtime::state_hash_of(&state),
                room.state_hash().expect("状态哈希失败")
            );
        }
        other => panic!("期望快照重置，实际 {other:?}"),
    }
}

#[test]
fn human_submit_validates_turn_and_legality() {
    let mut room = Room::new(RoomConfig {
        match_id: "match-submit".to_owned(),
        ruleset_version: gf_table_runtime::DEFAULT_RULESET_VERSION.to_owned(),
        seed: 0x51ED_0005,
        players: vec![PlayerKind::Human; 4],
    })
    .expect("建桌失败");
    room.start(0).expect("发牌失败");
    let turn = room.state().expect("有状态").turn_seat;

    // 非行动座位提交 → WrongTurn。
    let other = (turn + 1) % 4;
    let err = room
        .submit(other, Action::Pass, 1000)
        .expect_err("非行动座位必须拒绝");
    assert!(matches!(
        err,
        gf_table_runtime::RoomError::WrongTurn { .. }
    ));

    // 领出态必须出牌，`Pass` 非法 → Engine 拒绝。
    let err = room
        .submit(turn, Action::Pass, 1000)
        .expect_err("领出态 Pass 必须拒绝");
    assert!(matches!(err, gf_table_runtime::RoomError::Engine(_)));

    // 合法单张 → Applied。
    let legal = room
        .engine()
        .legal_actions(room.state().expect("有状态")).expect("合法枚举失败");
    let play = legal
        .into_iter()
        .find(|a| matches!(a, Action::Play { cards } if cards.len() == 1))
        .expect("领出态必有单张候选");
    match room.submit(turn, play, 1000).expect("合法单张应通过") {
        StepOutcome::Applied { seat, .. } => assert_eq!(seat, turn),
        other => panic!("期望 Applied，实际 {other:?}"),
    }
}
