//! 最小往返测试：encode → decode 相等，证明 prost 生成代码真的可用（不是只有定义）。
//!
//! 覆盖 §5.9 消息表的关键帧：
//! - `JoinRoom`（含可选 `seat`）；
//! - `JoinAck`（`availableSeats` 非空、`reason` 可空）；
//! - `SubmitAction`：`play` / `pass` 两个 oneof 分支各一条；
//! - `RoomSnapshot`：`view` 为原始 bytes、`viewSchemaVersion` 保留；
//! - `RoomDelta`：两种不同 `events` 分支，decode 后 oneof 落到正确分支。

use gf_protocol::gf::protocol::v1::*;
use prost::Message;

/// encode → decode 往返，解码失败直接 panic（测试夹具，不留宽容出口）。
fn roundtrip<M: Message + PartialEq + Default>(msg: &M) -> M {
    let bytes = msg.encode_to_vec();
    M::decode(&bytes[..]).expect("prost decode")
}

#[test]
fn join_room_roundtrip() {
    let msg = JoinRoom {
        match_id: "match-001".into(),
        seat: Some(2),
        seat_token: "tok-join".into(),
        client_version: "1.4.2".into(),
    };
    let back = roundtrip(&msg);
    assert_eq!(msg, back);
    assert_eq!(back.seat, Some(2));
}

#[test]
fn join_ack_roundtrip_with_available_seats() {
    let msg = JoinAck {
        match_id: "match-001".into(),
        seat: 0,
        accept: false,
        available_seats: vec![1, 3],
        reason: Some("RS_SEAT_TAKEN".into()),
        seat_token: "tok-seat-0".into(),
    };
    let back = roundtrip(&msg);
    assert_eq!(msg, back);
    assert_eq!(back.available_seats, vec![1, 3]);
    assert_eq!(back.reason.as_deref(), Some("RS_SEAT_TAKEN"));
}

#[test]
fn submit_action_play_roundtrip() {
    let msg = SubmitAction {
        match_id: "match-001".into(),
        seat: 1,
        request_id: "req-7".into(),
        seat_seq: 12,
        kind: ActionKind::Play as i32,
        payload: Some(submit_action::Payload::Play(PlayPayload {
            cards: vec![3, 5, 7],
            pattern_hint: Some("single".into()),
        })),
    };
    let back = roundtrip(&msg);
    assert_eq!(msg, back);
    match back.payload {
        Some(submit_action::Payload::Play(ref p)) => {
            assert_eq!(p.cards, vec![3, 5, 7]);
            assert_eq!(p.pattern_hint.as_deref(), Some("single"));
        }
        _ => panic!("play 分支 decode 后未落到 Play"),
    }
}

#[test]
fn submit_action_pass_roundtrip() {
    let msg = SubmitAction {
        match_id: "match-001".into(),
        seat: 1,
        request_id: "req-8".into(),
        seat_seq: 13,
        kind: ActionKind::Pass as i32,
        payload: Some(submit_action::Payload::Pass(PassPayload {})),
    };
    let back = roundtrip(&msg);
    assert_eq!(msg, back);
    match back.payload {
        Some(submit_action::Payload::Pass(_)) => {}
        _ => panic!("pass 分支 decode 后未落到 Pass"),
    }
}

/// 互斥的类型层面证明：`payload` 是 `Option<submit_action::Payload>`，
/// `Payload` 是双变体枚举，一条消息只能持有其中一个分支——
/// 不存在同时携带 play 与 pass 的构造路径（prost oneof 落 Option<enum>）。
#[test]
fn payload_oneof_is_mutex_exclusive() {
    let play = submit_action::Payload::Play(PlayPayload {
        cards: vec![1],
        pattern_hint: None,
    });
    let pass = submit_action::Payload::Pass(PassPayload {});

    assert!(matches!(play, submit_action::Payload::Play(_)));
    assert!(!matches!(play, submit_action::Payload::Pass(_)));
    assert!(matches!(pass, submit_action::Payload::Pass(_)));
    assert!(!matches!(pass, submit_action::Payload::Play(_)));
}

#[test]
fn room_snapshot_roundtrip_keeps_view_bytes() {
    let view = br#"{"hand":{"cards":[0,1,2]}}"#.to_vec();
    let msg = RoomSnapshot {
        match_id: "match-001".into(),
        seat: 1,
        ruleset_id: "guandan-classic".into(),
        ruleset_version: "m0-minimal@1.0.0".into(),
        room_seq: 42,
        last_seat_seq: 11,
        view_hash: "9e0b756d600f8eb6".into(),
        view_schema_version: 3,
        view: view.clone(),
        snapshot_reset: false,
        round_no: 2,
    };
    let back = roundtrip(&msg);
    assert_eq!(msg, back);
    // view 是 canonical JSON bytes，往返后字节原样保留；不做结构化解码。
    assert_eq!(back.view, view);
    assert_eq!(back.view_schema_version, 3);
    assert_eq!(back.round_no, 2, "局号是快照头字段，必须随帧往返");
}

#[test]
fn room_delta_roundtrip_two_event_branches() {
    let msg = RoomDelta {
        match_id: "match-001".into(),
        room_seq: 9,
        events: vec![
            DeltaEvent {
                event: Some(delta_event::Event::Played(PlayedEvent {
                    seat: 1,
                    cards: vec![5],
                    pattern_id: "single".into(),
                })),
            },
            DeltaEvent {
                event: Some(delta_event::Event::Passed(PassedEvent { seat: 2 })),
            },
        ],
    };
    let back = roundtrip(&msg);
    assert_eq!(msg, back);

    let mut iter = back.events.iter();
    match iter.next().and_then(|e| e.event.as_ref()) {
        Some(delta_event::Event::Played(ref p)) => {
            assert_eq!(p.seat, 1);
            assert_eq!(p.cards, vec![5]);
            assert_eq!(p.pattern_id, "single");
        }
        _ => panic!("第一个事件 decode 后未落到 Played"),
    }
    match iter.next().and_then(|e| e.event.as_ref()) {
        Some(delta_event::Event::Passed(ref p)) => assert_eq!(p.seat, 2),
        _ => panic!("第二个事件 decode 后未落到 Passed"),
    }
}

#[test]
fn settle_result_roundtrip_with_reason() {
    let msg = SettleResult {
        round_no: 3,
        room_seq: 77,
        result: br#"{"winner_team":1,"ranks":[0,2,1,3],"points":3}"#.to_vec(),
        levels_after: vec![3, 0],
        next_deal_in_ms: 5000,
        reason: Some("RS_PHASE_UNIMPLEMENTED".into()),
    };
    let back = roundtrip(&msg);
    assert_eq!(msg, back);
    assert_eq!(back.reason.as_deref(), Some("RS_PHASE_UNIMPLEMENTED"));
    assert_eq!(back.levels_after, vec![3, 0]);
}

#[test]
fn settle_result_roundtrip_without_reason_defaults_none() {
    let msg = SettleResult {
        round_no: 1,
        room_seq: 12,
        result: br#"{"winner_team":0,"ranks":[0,1,2,3],"points":0}"#.to_vec(),
        levels_after: vec![2, 0],
        next_deal_in_ms: 0,
        reason: None,
    };
    let back = roundtrip(&msg);
    assert_eq!(msg, back);
    assert_eq!(back.reason, None);
}
