//! 服务端房间闭环：四座位入座 → 发牌 → 动作 → 结算 → 重连快照。
//!
//! 走 `RoomHost` 的公开入口（与 WS 层同一套调用），不绕过判定链路。

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use gf_engine::contract::Action;
use gf_protocol::gf::protocol::v1 as pb;
use gf_protocol::wire;
use prost::Message as _;
use tokio::sync::mpsc;

use gf_server::host::RoomHost;
use gf_server::RoomOptions;

const MATCH: &str = "match-host-e2e";

async fn frames(rx: &mut mpsc::Receiver<Vec<u8>>) -> Vec<(u16, Vec<u8>)> {
    let mut out = Vec::new();
    while let Ok(frame) = rx.try_recv() {
        let (kind, body) = wire::split(&frame).expect("帧头应可拆");
        out.push((kind, body.to_vec()));
    }
    out
}

fn play_msg(seat: u8, seq: u64, request_id: &str, action: &Action) -> pb::SubmitAction {
    match action {
        Action::Play { cards } => pb::SubmitAction {
            match_id: MATCH.to_owned(),
            seat: u32::from(seat),
            request_id: request_id.to_owned(),
            seat_seq: seq,
            kind: pb::ActionKind::Play as i32,
            payload: Some(pb::submit_action::Payload::Play(pb::PlayPayload {
                cards: cards.iter().map(|c| u32::from(*c)).collect(),
                pattern_hint: None,
            })),
        },
        Action::Pass => pb::SubmitAction {
            match_id: MATCH.to_owned(),
            seat: u32::from(seat),
            request_id: request_id.to_owned(),
            seat_seq: seq,
            kind: pb::ActionKind::Pass as i32,
            payload: Some(pb::submit_action::Payload::Pass(pb::PassPayload {})),
        },
        Action::TributeReturn { .. } => {
            unreachable!("v1 协议没有还贡动作；本测试只跑首局")
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn four_humans_play_a_round_then_settle() {
    let room = RoomHost::spawn(
        MATCH,
        0x51ED_0007,
        RoomOptions {
            tick_ms: 50,
            next_deal_ms: 5_000,
        },
    )
    .expect("建桌失败");

    // 四座位入座：满员自动发牌。
    let mut rxs = Vec::new();
    for _ in 0..4u8 {
        let (tx, rx) = mpsc::channel(1024);
        let seat = room.join(None, tx).await.expect("入座失败");
        rxs.push((seat, rx));
    }
    assert_eq!(
        rxs.iter().map(|(s, _)| *s).collect::<Vec<_>>(),
        vec![0, 1, 2, 3]
    );
    room.start_if_full().await;

    // 每个座位都应收到开局快照。
    for (seat, rx) in &mut rxs {
        let got = frames(rx).await;
        assert!(
            got.iter().any(|(k, _)| *k == wire::kind::ROOM_SNAPSHOT),
            "座位 {seat} 未收到开局快照"
        );
        let snap = got
            .iter()
            .find(|(k, _)| *k == wire::kind::ROOM_SNAPSHOT)
            .map(|(_, b)| pb::RoomSnapshot::decode(&b[..]).expect("快照解码失败"))
            .expect("有快照");
        assert_eq!(snap.seat, u32::from(*seat));
        assert!(snap.snapshot_reset, "开局快照必须 snapshotReset");
        assert!(!snap.view.is_empty(), "视图不得为空");
    }

    let counter = AtomicU64::new(0);
    let mut actions = 0u64;
    let mut ack_kinds = 0u64;

    loop {
        let state = room.state_snapshot().await.expect("有状态");
        if state.phase == gf_engine::engine::PHASE_SETTLE {
            break;
        }
        let legal = room.legal_actions().await;
        let action = legal.first().cloned().expect("出牌阶段必有合法动作");
        let seat = state.turn_seat;
        let seq = room.seat_seq(seat).await + 1;
        let request_id = format!("r{}", counter.fetch_add(1, Ordering::Relaxed));
        let ack_bytes = room
            .submit(
                seat,
                play_msg(seat, seq, &request_id, &action),
                room.now_ms(),
            )
            .await;
        let (kind, body) = wire::split(&ack_bytes).expect("回执应可拆");
        assert_eq!(kind, wire::kind::ACTION_ACK);
        let ack = pb::ActionAck::decode(body).expect("回执解码失败");
        assert_eq!(ack.request_id, request_id);
        assert!(ack.accepted, "合法动作必须被接受：{ack:?}");
        assert!(ack.reason.is_none(), "accepted=true 不得带 reason");
        ack_kinds += 1;
        actions += 1;
        assert!(actions < 600, "一局动作数超上界：{actions}");
    }

    // 结算后：权威哈希与日志重放哈希必须相等（服务端自证）。
    assert_eq!(
        room.state_hash().await.expect("状态哈希失败"),
        room.replay_hash().await.expect("重放哈希失败"),
        "动作日志重放必须与权威状态同解"
    );
    assert_eq!(ack_kinds, actions);

    // 结算帧必须广播到所有在线座位。
    for (seat, rx) in &mut rxs {
        let got = frames(rx).await;
        assert!(
            got.iter().any(|(k, _)| *k == wire::kind::SETTLE_RESULT),
            "座位 {seat} 未收到结算帧"
        );
        let settle = got
            .iter()
            .find(|(k, _)| *k == wire::kind::SETTLE_RESULT)
            .map(|(_, b)| pb::SettleResult::decode(&b[..]).expect("结算解码失败"))
            .expect("有结算帧");
        let json = String::from_utf8(settle.result.clone()).expect("结算结果必须是 UTF-8");
        assert!(
            json.starts_with("{\"winner_team\""),
            "结算 JSON 键序错：{json}"
        );
        assert!(json.contains("\"ranks\":["), "结算必须带名次：{json}");
        assert_eq!(settle.levels_after.len(), 2, "两队级牌");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn duplicate_request_id_is_idempotent() {
    let room =
        RoomHost::spawn("match-idem", 0x51ED_0008, RoomOptions::default()).expect("建桌失败");
    let mut rxs = Vec::new();
    for _ in 0..4u8 {
        let (tx, rx) = mpsc::channel(1024);
        room.join(None, tx).await.expect("入座失败");
        rxs.push(rx);
    }
    room.start_if_full().await;
    // 开局快照先清掉。
    for rx in &mut rxs {
        let _ = frames(rx).await;
    }

    let state = room.state_snapshot().await.expect("有状态");
    let seat = state.turn_seat;
    let action = room
        .legal_actions()
        .await
        .first()
        .cloned()
        .expect("有合法动作");
    let seq = room.seat_seq(seat).await + 1;
    let msg = play_msg(seat, seq, "same-request", &action);

    let first = room.submit(seat, msg.clone(), room.now_ms()).await;
    let seq_after_first = room.room_seq().await;
    let second = room.submit(seat, msg, room.now_ms()).await;

    assert_eq!(first, second, "同一 requestId 必须回同一份回执");
    assert_eq!(
        seq_after_first,
        room.room_seq().await,
        "幂等重放不得推进房间序号"
    );
    let ack = pb::ActionAck::decode(wire::split(&second).expect("可拆").1).expect("解码失败");
    assert!(ack.accepted);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stale_seat_seq_is_rejected_without_advancing() {
    let room =
        RoomHost::spawn("match-stale", 0x51ED_0009, RoomOptions::default()).expect("建桌失败");
    let mut rxs = Vec::new();
    for _ in 0..4u8 {
        let (tx, rx) = mpsc::channel(1024);
        room.join(None, tx).await.expect("入座失败");
        rxs.push(rx);
    }
    room.start_if_full().await;
    for rx in &mut rxs {
        let _ = frames(rx).await;
    }

    // 推到「某个已出过手的座位重新轮到」的时刻，再拿旧水位提交。
    let mut target = None;
    let mut played = 0u64;
    loop {
        let state = room.state_snapshot().await.expect("有状态");
        if state.phase == gf_engine::engine::PHASE_SETTLE {
            break;
        }
        let seat = state.turn_seat;
        let current = room.seat_seq(seat).await;
        if current >= 1 {
            target = Some((seat, current));
            break;
        }
        let action = room
            .legal_actions()
            .await
            .first()
            .cloned()
            .expect("有合法动作");
        room.submit(
            seat,
            play_msg(seat, current + 1, &format!("adv-{played}"), &action),
            room.now_ms(),
        )
        .await;
        played += 1;
        assert!(played < 600, "推进手数超上界");
    }
    let (seat, _current) = target.expect("一局内必然有座位二次轮到");
    let advanced = room.room_seq().await;
    let action = room
        .legal_actions()
        .await
        .first()
        .cloned()
        .expect("有合法动作");

    // 旧水位（0 < current）必须 STALE_SEQ，且不得推进房间序号。
    let stale = room
        .submit(seat, play_msg(seat, 0, "stale-1", &action), room.now_ms())
        .await;
    let ack = pb::ActionAck::decode(wire::split(&stale).expect("可拆").1).expect("解码失败");
    assert!(!ack.accepted, "旧水位必须拒");
    assert_eq!(ack.reason.as_deref(), Some("STALE_SEQ"));
    assert_eq!(
        room.room_seq().await,
        advanced,
        "被拒的动作不得推进房间序号"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reconnect_without_local_state_gets_reset_snapshot() {
    let room =
        RoomHost::spawn("match-reconnect", 0x51ED_000A, RoomOptions::default()).expect("建桌失败");
    let mut rxs = Vec::new();
    for _ in 0..4u8 {
        let (tx, rx) = mpsc::channel(1024);
        room.join(None, tx).await.expect("入座失败");
        rxs.push(rx);
    }
    room.start_if_full().await;
    for rx in &mut rxs {
        let _ = frames(rx).await;
    }

    // 先把前两手打出去，制造非零水位。
    for i in 0..2 {
        let state = room.state_snapshot().await.expect("有状态");
        let seat = state.turn_seat;
        let action = room
            .legal_actions()
            .await
            .first()
            .cloned()
            .expect("有合法动作");
        let seq = room.seat_seq(seat).await + 1;
        room.submit(
            seat,
            play_msg(seat, seq, &format!("pre-{i}"), &action),
            room.now_ms(),
        )
        .await;
    }

    // 挑一个还没出过手的座位：本地零水位，服务端无法换算 roomSeq，只能给快照。
    let mut idle = None;
    for seat in 0..4u8 {
        if room.seat_seq(seat).await == 0 {
            idle = Some(seat);
            break;
        }
    }
    let seat = idle.expect("两手动作后必然还有座位没出过手");
    let (tx, mut rx) = mpsc::channel(1024);
    assert!(room.rebind(seat, tx).await, "重连必须能接管座位通道");
    let frames = room.reconnect(seat, 0).await.expect("重连失败");
    assert_eq!(frames.len(), 1, "零水位只能收到一份快照，不得凭空补发增量");
    let (kind, body) = wire::split(&frames[0]).expect("可拆");
    assert_eq!(kind, wire::kind::ROOM_SNAPSHOT);
    let snap = pb::RoomSnapshot::decode(body).expect("解码失败");
    assert_eq!(snap.seat, u32::from(seat));
    assert!(snap.snapshot_reset, "无法换算水位时必须 snapshotReset");
    assert!(!snap.view_hash.is_empty(), "viewHash 必须由服务端给出");
    assert_eq!(snap.view_hash.len(), 16, "viewHash 是 16 字符十六进制");

    // 重连后该座位重新在线，且能继续收到广播。
    let turn = room.state_snapshot().await.expect("有状态").turn_seat;
    let action = room.legal_actions().await.first().cloned().expect("有动作");
    let seq = room.seat_seq(turn).await + 1;
    room.submit(
        turn,
        play_msg(turn, seq, "after-reconnect", &action),
        room.now_ms(),
    )
    .await;
    assert!(rx.try_recv().is_ok(), "重连座位必须重新收到下行帧");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reconnect_within_buffer_gets_delta_then_snapshot() {
    let room =
        RoomHost::spawn("match-delta", 0x51ED_000C, RoomOptions::default()).expect("建桌失败");
    let mut rxs = Vec::new();
    for _ in 0..4u8 {
        let (tx, rx) = mpsc::channel(1024);
        room.join(None, tx).await.expect("入座失败");
        rxs.push(rx);
    }
    room.start_if_full().await;
    for rx in &mut rxs {
        let _ = frames(rx).await;
    }

    // 打一手，等这个座位真的有了记录，再取它的水位当 `lastSeatSeq`。
    let seat = room.state_snapshot().await.expect("有状态").turn_seat;
    let action = room.legal_actions().await.first().cloned().expect("有动作");
    let seq = room.seat_seq(seat).await + 1;
    room.submit(seat, play_msg(seat, seq, "delta-0", &action), room.now_ms())
        .await;
    let last = room.seat_watermark(seat).await;
    assert!(
        room.seat_seq(seat).await > 0,
        "该座位必须已有记录（记录里的 seatSeq 从 0 起算）"
    );

    // 掉线期间再打两手，这两手就是要补发的增量。
    for i in 0..2 {
        let turn = room.state_snapshot().await.expect("有状态").turn_seat;
        let action = room.legal_actions().await.first().cloned().expect("有动作");
        let seq = room.seat_seq(turn).await + 1;
        room.submit(
            turn,
            play_msg(turn, seq, &format!("delta-gap-{i}"), &action),
            room.now_ms(),
        )
        .await;
    }
    let latest = room.room_seq().await;

    let (tx, rx) = mpsc::channel(1024);
    assert!(room.rebind(seat, tx).await, "重连必须能接管座位通道");
    let frames = room.reconnect(seat, last).await.expect("重连失败");
    assert!(
        frames.len() >= 2,
        "增量路径必须先补 RoomDelta 再附快照，实际 {} 帧",
        frames.len()
    );
    let (last_kind, last_body) = wire::split(frames.last().expect("至少一帧")).expect("可拆");
    assert_eq!(last_kind, wire::kind::ROOM_SNAPSHOT, "最后一帧必须是快照");
    let snap = pb::RoomSnapshot::decode(last_body).expect("解码失败");
    assert!(
        !snap.snapshot_reset,
        "增量路径 snapshotReset 必须为 false（客户端保留本地状态）"
    );
    assert_eq!(
        snap.last_seat_seq,
        room.seat_watermark(seat).await,
        "快照水位必须是该座位当前已应用到的 seatSeq"
    );

    // 增量帧：roomSeq 连续、逐帧对齐，末帧 roomSeq 追平房间广播序。
    let mut deltas = Vec::new();
    for frame in &frames[..frames.len() - 1] {
        let (kind, body) = wire::split(frame).expect("可拆");
        assert_eq!(
            kind,
            wire::kind::ROOM_DELTA,
            "增量路径中间帧必须是 RoomDelta"
        );
        deltas.push(pb::RoomDelta::decode(body).expect("解码失败"));
    }
    assert_eq!(
        deltas.last().expect("至少一条增量").room_seq,
        latest.saturating_sub(1),
        "补发必须覆盖到最新一条记录"
    );
    for pair in deltas.windows(2) {
        assert_eq!(
            pair[1].room_seq,
            pair[0].room_seq + 1,
            "补发的 roomSeq 必须连续"
        );
        assert!(!pair[0].events.is_empty(), "每条记录至少带来一个事件");
    }
    // 末条增量带当前行动窗口：TurnChanged 的 deadline 必须是新的，不能回放历史值。
    let tail = deltas.last().expect("至少一条增量");
    let turn_changed = tail
        .events
        .iter()
        .find_map(|e| match &e.event {
            Some(pb::delta_event::Event::TurnChanged(t)) => Some(t),
            _ => None,
        })
        .expect("末条增量必须带 TurnChanged");
    assert!(turn_changed.deadline_ms > 0, "行动窗口 deadline 必须有效");
    let _ = rx;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn seat_assignment_rejects_overflow_and_reports_free_seats() {
    let room =
        RoomHost::spawn("match-seats", 0x51ED_000B, RoomOptions::default()).expect("建桌失败");
    let mut keep = Vec::new();
    for _ in 0..3u8 {
        let (tx, rx) = mpsc::channel(1024);
        room.join(None, tx.clone()).await.expect("入座失败");
        // 接收端必须活着：有界队列在接收端消失时会判定为掉线并摘座。
        keep.push((tx, rx));
    }

    // 指座到已占座位：拒，并给出当前空座（此时只剩 3）。
    let (tx, _rx) = mpsc::channel(1024);
    let err = room.join(Some(1), tx).await.expect_err("占座必须拒");
    let gf_server::JoinError::SeatTaken { available } = err else {
        panic!("期望 SeatTaken，实际 {err:?}");
    };
    assert_eq!(available, vec![3], "座位 1 已占，可分配只剩 3");

    // 补满第四座 → 满员触发发牌。
    let (tx, mut rx) = mpsc::channel(1024);
    assert_eq!(room.join(None, tx).await.expect("补满第四座"), 3);
    room.start_if_full().await;
    assert!(
        frames(&mut rx)
            .await
            .iter()
            .any(|(k, _)| *k == wire::kind::ROOM_SNAPSHOT),
        "满员即发牌并下发快照"
    );

    // 房间已满：再入座必须被拒，并给出空表。
    let (tx, _rx) = mpsc::channel(1024);
    let err = room.join(None, tx).await.expect_err("满员必须拒");
    let gf_server::JoinError::SeatTaken { available } = err else {
        panic!("期望 SeatTaken，实际 {err:?}");
    };
    assert!(available.is_empty(), "满员时 availableSeats 必须为空表");
}

/// 帮助断言：`Room` 的等级别字段在测试里可直接读。
#[allow(dead_code)]
fn _assert_send_sync() {
    fn assert_send<T: Send>() {}
    fn assert_sync<T: Sync>() {}
    assert_send::<Arc<RoomHost>>();
    assert_sync::<Arc<RoomHost>>();
}
