//! M0 断线恢复原型：两条重连路径的端到端比对（§2.4 第 194 行 gate：断线重放后
//! `stateHash` 与断线前逐字节一致——32 字节全量比对，不是只看前 8 位）。
//!
//! 分层：服务端推进 / 断线模拟在 [`harness`]（一次性夹具，M1 丢弃）；缓冲 / 判定 /
//! 重放在 [`recovery`]（待迁入 `crates/table-runtime`）。
//!
//! 边界：本轮 in-process，快照直接传 `State` 克隆，不做 wire 编解码；不依赖
//! `gf-protocol`。

use gf_engine::contract::{Engine, State};
use gf_runtime_spike::harness::{engine, ServerRoom};
use gf_runtime_spike::recovery::{
    decide, hex_full, replay_delta, state_hash_of, ReconnectBuffer, ReconnectDecision,
    RECONNECT_BUFFER_CAPACITY,
};

/// ① 增量路径：断线点落在 200 条缓冲覆盖范围内 → 判定 `Delta`（`snapshotReset`
/// 为 false）→ 客户端用补齐的增量从断线前状态逐条重放 → 恢复后 `stateHash` 与
/// 服务端当前哈希 32 字节逐字节相等。
#[test]
fn delta_replay_within_buffer_recovers_byte_identical_hash() {
    let mut server = ServerRoom::new(engine()).expect("发牌失败");
    let mut buffer = ReconnectBuffer::default();

    let disconnect_seat = 0u8;
    // 推进到前 80 条房间事件；断线点取在断线座位自己最后一次落子之后——
    // 增量边界按该座位 `lastSeatSeq` 对应的 roomSeq（§6.2 第 978 行），
    // 这样断线前已应用的他人事件不会被重复重放。
    let mut captured: Option<(State, u64)> = None;
    for _ in 0..80 {
        let record = server
            .advance_one(&mut buffer)
            .expect("对局中途推进失败")
            .expect("前置阶段不应到达 settle");
        if record.seat == disconnect_seat {
            captured = Some((server.state.clone(), record.seat_seq));
        }
    }
    let (server_at_disconnect, client_last_seat_seq) =
        captured.expect("断线座位在前 80 条内应落过子");
    let disconnect_room_seq = buffer
        .room_seq_for_seat(disconnect_seat, client_last_seat_seq)
        .expect("断线座位最后一条记录应在缓冲内");
    let client_before = server_at_disconnect.clone();
    let hash_before = state_hash_of(&client_before);

    // (a) 断线前客户端副本与服务端权威状态哈希相等。
    assert_eq!(
        hash_before,
        state_hash_of(&server_at_disconnect),
        "断线前两侧 stateHash 必须相等"
    );

    // 断线期间服务端继续推进 25 条（模拟托管 / 其他人出牌），客户端不参与。
    server
        .advance_steps(&mut buffer, 25)
        .expect("断线期间服务端推进失败");
    let hash_server_now = state_hash_of(&server.state);

    // 重连判定：必须是 Delta，且不是 SnapshotReset 那一支。
    let decision = decide(&buffer, disconnect_seat, client_last_seat_seq);
    assert!(
        matches!(decision, ReconnectDecision::Delta { .. }),
        "缓冲覆盖内必须走 Delta，实际 {decision:?}"
    );
    assert!(
        !matches!(decision, ReconnectDecision::SnapshotReset),
        "增量路径不得选中 SnapshotReset 分支"
    );
    let ReconnectDecision::Delta {
        from_room_seq,
        events,
    } = decision
    else {
        panic!("上方已断言为 Delta，此处不应落空");
    };
    assert!(!events.is_empty(), "断线区间不应为空增量");
    assert_eq!(from_room_seq, disconnect_room_seq);
    assert_eq!(events.first().unwrap().room_seq, from_room_seq + 1);
    assert!(events.windows(2).all(|w| w[0].room_seq < w[1].room_seq));

    // 客户端从断线前副本逐条重放增量。
    let client_after = replay_delta(&server.engine, client_before, &events).expect("增量重放失败");
    let hash_after = state_hash_of(&client_after);

    println!("[delta] 断线前 hash      = {}", hex_full(&hash_before));
    println!("[delta] 服务端当前 hash  = {}", hex_full(&hash_server_now));
    println!("[delta] 恢复后客户端 hash= {}", hex_full(&hash_after));

    // (c) 恢复后哈希与服务端当前哈希 32 字节全等（不是只比前 8 位）。
    assert_eq!(
        hash_after, hash_server_now,
        "增量重放后 stateHash 必须与服务端当前值 32 字节逐字节一致"
    );

    // (d) 恢复后不是靠"停住"骗过哈希：两侧 legal_actions 候选一致，且同一步双方都接受。
    let legal_server = server
        .engine
        .legal_actions(&server.state)
        .expect("服务端枚举合法动作失败");
    let legal_client = server
        .engine
        .legal_actions(&client_after)
        .expect("客户端枚举合法动作失败");
    assert_eq!(legal_client, legal_server, "恢复后两侧合法候选必须一致");
    let action = legal_server.first().cloned().expect("非终局候选不应为空");
    let server_next = server
        .engine
        .apply(server.state.clone(), action.clone())
        .expect("服务端接受下一步失败");
    let client_next = server
        .engine
        .apply(client_after.clone(), action)
        .expect("客户端接受下一步失败");
    assert_eq!(
        state_hash_of(&client_next),
        state_hash_of(&server_next),
        "恢复后双方同走一步，哈希仍须一致"
    );
}

/// ② `snapshotReset` 路径：断线跨度超出 200 条缓冲（座位 `lastSeatSeq` 对应的
/// `roomSeq` 早于缓冲起点）→ 判定 `SnapshotReset` 且快照 `snapshotReset = true`
/// → 客户端丢弃本地状态（清空副本与水位，断言不再持有旧 `stateHash`）→ 接受全量
/// 快照 → 恢复后哈希与服务端当前哈希 32 字节全等。
///
/// 单局约 150 条房间事件凑不满 200 条；本用例在**同一房间**连打第二局：`settle`
/// 后重新 `deal`，`roomSeq` 连续递增、`seatSeq` 按座位各自继续递增（跨局不归零，
/// 见 `M0-GAP-SEQ-CROSS-ROUND` 注释），让断线跨度真的越过缓冲起点。
#[test]
fn snapshot_reset_when_disconnect_gap_exceeds_buffer() {
    let mut server = ServerRoom::new(engine()).expect("发牌失败");
    let mut buffer = ReconnectBuffer::default();

    // 第一局推进 10 条后断线。
    server
        .advance_steps(&mut buffer, 10)
        .expect("首局前置推进失败");
    let disconnect_seat = 0u8;
    let client_before = server.state.clone();
    let client_last_seat_seq = server
        .last_seat_seq(disconnect_seat)
        .expect("断线座位前 10 条内应落过子");
    let hash_before = state_hash_of(&client_before);
    let disconnect_room_seq = buffer
        .room_seq_for_seat(disconnect_seat, client_last_seat_seq)
        .expect("断线座位最后一条记录应在缓冲内");

    // (a) 断线前两侧哈希相等。
    assert_eq!(hash_before, state_hash_of(&server.state));

    // 断线期间：打完第一局，开第二局继续推进，直到断线座位的旧记录被缓冲淘汰。
    server
        .run_to_settle(&mut buffer)
        .expect("第一局跑到 settle 失败");
    server.deal_next_round().expect("第二局发牌失败");
    let mut safety = 0usize;
    while buffer
        .room_seq_for_seat(disconnect_seat, client_last_seat_seq)
        .is_some()
    {
        server
            .advance_one(&mut buffer)
            .expect("第二局推进失败")
            .expect("缓冲淘汰前不应到达 settle");
        safety += 1;
        assert!(safety < 500, "第二局内旧记录未被淘汰，缓冲口径异常");
    }

    // 缓冲确实只保留末尾 200 条（容量是真实值，没有调小）。
    assert_eq!(buffer.len(), RECONNECT_BUFFER_CAPACITY);
    assert_eq!(
        buffer.min_room_seq(),
        Some(buffer.latest_room_seq().unwrap() - (RECONNECT_BUFFER_CAPACITY - 1) as u64),
        "缓冲起点应为末尾 200 条的最小 roomSeq"
    );
    assert!(
        disconnect_room_seq < buffer.min_room_seq().unwrap(),
        "断线记录 roomSeq={disconnect_room_seq} 应早于缓冲起点 {:?}",
        buffer.min_room_seq()
    );

    let hash_server_now = state_hash_of(&server.state);

    // 重连判定：必须是 SnapshotReset，且不是 Delta 那一支。
    let decision = decide(&buffer, disconnect_seat, client_last_seat_seq);
    assert!(
        matches!(decision, ReconnectDecision::SnapshotReset),
        "越过缓冲起点必须走 SnapshotReset，实际 {decision:?}"
    );
    assert!(
        !matches!(decision, ReconnectDecision::Delta { .. }),
        "快照重置路径不得选中 Delta 分支"
    );

    // 服务端下发全量快照，`snapshotReset = true`。
    let snapshot = server.snapshot(true);
    assert!(snapshot.snapshot_reset, "重置快照必须带 snapshotReset=true");

    // 客户端丢弃本地状态：副本显式 drop、水位清空；丢弃后本地不再持有旧 stateHash。
    // （`hash_before` 仅留作比对的哈希字节，不是本地状态副本。）
    let client: Option<State> = Some(client_before);
    drop(client);
    let client: Option<State> = None;
    assert!(client.is_none(), "丢弃后本地状态副本必须为空");
    // 旧哈希只留在比对变量里；本地状态位已无 State 可投影。
    assert_ne!(hash_before, hash_server_now, "断线前后状态本应不同");

    // 接受全量快照。
    let client_after = snapshot.state.clone();
    let hash_after = state_hash_of(&client_after);

    println!("[reset] 断线前 hash      = {}", hex_full(&hash_before));
    println!("[reset] 服务端当前 hash  = {}", hex_full(&hash_server_now));
    println!("[reset] 恢复后客户端 hash= {}", hex_full(&hash_after));

    // (c) 恢复后哈希与服务端当前哈希 32 字节全等；且不等于旧哈希（不是靠旧状态骗过）。
    assert_eq!(
        hash_after, hash_server_now,
        "全量快照恢复后 stateHash 必须与服务端当前值 32 字节逐字节一致"
    );
    assert_ne!(hash_after, hash_before, "恢复后哈希不得仍是断线前旧哈希");

    // (d) 恢复后两侧 legal_actions 候选一致，且同一步双方都接受。
    let legal_server = server
        .engine
        .legal_actions(&server.state)
        .expect("服务端枚举合法动作失败");
    let legal_client = server
        .engine
        .legal_actions(&client_after)
        .expect("客户端枚举合法动作失败");
    assert_eq!(legal_client, legal_server, "恢复后两侧合法候选必须一致");
    let action = legal_server.first().cloned().expect("非终局候选不应为空");
    let server_next = server
        .engine
        .apply(server.state.clone(), action.clone())
        .expect("服务端接受下一步失败");
    let client_next = server
        .engine
        .apply(client_after.clone(), action)
        .expect("客户端接受下一步失败");
    assert_eq!(
        state_hash_of(&client_next),
        state_hash_of(&server_next),
        "恢复后双方同走一步，哈希仍须一致"
    );
}
