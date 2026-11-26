//! 状态差分 → §5.9 下行帧。服务端只做「权威状态变了什么」的翻译，不做判定。
//!
//! 事件来源全部取自 `State` 的前后对比（`last_play` / `turn_seat` / `levels` / `phase`），
//! 不额外新增记账字段，避免与 `stateHash` 口径分叉。

use gf_engine::contract::{Action, State};
use gf_engine::engine::{PHASE_PLAY, PHASE_SETTLE};
use gf_protocol::gf::protocol::v1 as pb;
use gf_protocol::wire::{encode, kind};
use gf_table_runtime::{ReplayedStep, RoomSnapshotWire, SettleSummary};

/// 一个动作（含托管代打）造成的事件列表。
pub fn events_for(
    before: &State,
    after: &State,
    action: &Action,
    deadline_ms: u64,
) -> Vec<pb::DeltaEvent> {
    let mut events = Vec::new();

    match action {
        Action::Play { cards } => {
            let pattern_id = after
                .last_play
                .as_ref()
                .filter(|play| play.cards == *cards)
                .map(|play| play.pattern_id.clone())
                .unwrap_or_default();
            events.push(delta(pb::delta_event::Event::Played(pb::PlayedEvent {
                seat: u32::from(
                    after
                        .last_play
                        .as_ref()
                        .map(|p| p.seat)
                        .unwrap_or(before.turn_seat),
                ),
                cards: cards.iter().map(|c| u32::from(*c)).collect(),
                pattern_id,
            })));
        }
        Action::Pass => {
            events.push(delta(pb::delta_event::Event::Passed(pb::PassedEvent {
                seat: u32::from(before.turn_seat),
            })));
        }
        // 还贡本身由下方 `tribute_events` 差量统一补发，不额外造 Played 事件。
        Action::TributeReturn { .. } => {}
    }

    // 进贡 / 还贡：只在新增记录时补发（首局与抗贡局为空）。
    for record in after
        .tribute_events
        .iter()
        .skip(before.tribute_events.len())
    {
        events.push(delta(pb::delta_event::Event::Tribute(pb::TributeEvent {
            from: u32::from(record.from),
            to: u32::from(record.to),
            card: u32::from(record.card),
        })));
    }

    // 级牌变化：按队伍序号逐队比对。
    for (team, (old, new)) in before.levels.iter().zip(after.levels.iter()).enumerate() {
        if old != new {
            events.push(delta(pb::delta_event::Event::LevelChanged(
                pb::LevelChangedEvent {
                    team: team as u32,
                    from: u32::from(*old),
                    to: u32::from(*new),
                },
            )));
        }
    }

    // 行动座位变化：只在出牌阶段报（结算相位由 SettleResult 表达）。
    if after.phase == PHASE_PLAY && before.turn_seat != after.turn_seat {
        events.push(delta(pb::delta_event::Event::TurnChanged(
            pb::TurnChangedEvent {
                seat: u32::from(after.turn_seat),
                deadline_ms,
            },
        )));
    }

    if before.phase != PHASE_SETTLE && after.phase == PHASE_SETTLE {
        events.push(delta(pb::delta_event::Event::RoundSettled(
            pb::RoundSettledEvent {
                round_no: u32::from(after.round_no),
            },
        )));
    }

    events
}

/// 组装 `RoomDelta` 帧。
pub fn delta_frame(match_id: &str, room_seq: u64, events: Vec<pb::DeltaEvent>) -> Vec<u8> {
    let msg = pb::RoomDelta {
        match_id: match_id.to_owned(),
        room_seq,
        events,
    };
    encode(kind::ROOM_DELTA, &prost::Message::encode_to_vec(&msg))
}

/// 断线补发的增量帧：每条缓冲记录一帧，`roomSeq` 逐条对齐（客户端按水位顺序应用）。
///
/// 与实时广播只有两处差别：
/// - 历史记录里的 `TurnChanged.deadlineMs` 早已过期，这里只为**最后一条**补一个当前窗口的
///   `TurnChanged`（`deadline_ms == 0` 表示没有行动窗口，跳过）；中间记录不报换手；
/// - 跨局与结算不在增量路径内（服务端在这两种情形一律走快照重置），
///   所以只需要 `PlayedEvent` / `PassedEvent` 两种事件。
pub fn replay_frames(match_id: &str, steps: &[ReplayedStep], deadline_ms: u64) -> Vec<Vec<u8>> {
    let last = steps.len().saturating_sub(1);
    let mut frames = Vec::with_capacity(steps.len());
    for (i, step) in steps.iter().enumerate() {
        let mut events = Vec::new();
        match &step.record.action {
            Action::Play { cards } => {
                let pattern_id = step
                    .after
                    .last_play
                    .as_ref()
                    .filter(|play| play.cards == *cards)
                    .map(|play| play.pattern_id.clone())
                    .unwrap_or_default();
                events.push(delta(pb::delta_event::Event::Played(pb::PlayedEvent {
                    seat: u32::from(step.record.seat),
                    cards: cards.iter().map(|c| u32::from(*c)).collect(),
                    pattern_id,
                })));
            }
            Action::Pass => {
                events.push(delta(pb::delta_event::Event::Passed(pb::PassedEvent {
                    seat: u32::from(step.record.seat),
                })));
            }
            Action::TributeReturn { .. } => {
                if let Some(rec) = step
                    .after
                    .tribute_events
                    .iter()
                    .rev()
                    .find(|r| r.is_return)
                {
                    events.push(delta(pb::delta_event::Event::Tribute(pb::TributeEvent {
                        from: u32::from(rec.from),
                        to: u32::from(rec.to),
                        card: u32::from(rec.card),
                    })));
                }
            }
        }
        if i == last && deadline_ms != 0 {
            events.push(delta(pb::delta_event::Event::TurnChanged(
                pb::TurnChangedEvent {
                    seat: u32::from(step.after.turn_seat),
                    deadline_ms,
                },
            )));
        }
        frames.push(delta_frame(match_id, step.record.room_seq, events));
    }
    frames
}

/// 组装 `PhaseChange` 帧。
pub fn phase_frame(phase: u16, deadline_ms: u64) -> Vec<u8> {
    let msg = pb::PhaseChange {
        phase: phase_enum(phase) as i32,
        deadline_ms,
    };
    encode(kind::PHASE_CHANGE, &prost::Message::encode_to_vec(&msg))
}

/// 引擎相位 → 协议相位枚举。
pub fn phase_enum(phase: u16) -> pb::Phase {
    match phase {
        gf_engine::engine::PHASE_DEAL => pb::Phase::Deal,
        gf_engine::engine::PHASE_TRIBUTE => pb::Phase::Tribute,
        gf_engine::engine::PHASE_PLAY => pb::Phase::Play,
        _ => pb::Phase::Settle,
    }
}

/// 组装 `SettleResult` 帧。`result` 走 canonical JSON（§5.9 三类载体第 1 条）。
pub fn settle_frame(
    summary: &SettleSummary,
    room_seq: u64,
    levels_after: &[u8],
    next_deal_in_ms: u64,
) -> Vec<u8> {
    let msg = pb::SettleResult {
        round_no: u32::from(summary.round_no),
        room_seq,
        result: settle_json(summary).into_bytes(),
        levels_after: levels_after.iter().map(|l| u32::from(*l)).collect(),
        next_deal_in_ms,
        reason: None,
    };
    encode(kind::SETTLE_RESULT, &prost::Message::encode_to_vec(&msg))
}

/// `SettleResult.result` 的 canonical JSON：键序 `winner_team, ranks, points`，
/// 单行、无空白、不写 `null`（可选字段缺省即省略）。
pub fn settle_json(summary: &SettleSummary) -> String {
    let mut out = String::from("{");
    if let Some(team) = summary.winner_team {
        out.push_str(&format!("\"winner_team\":{team},"));
    }
    out.push_str("\"ranks\":[");
    for (i, seat) in summary.ranks.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&seat.to_string());
    }
    out.push_str(&format!("],\"points\":{}}}", summary.points));
    out
}

/// 组装 `RoomSnapshot` 帧。`viewHash` 为服务端自算，客户端不重算（§5.12/§7.2）。
pub fn snapshot_frame(wire: &RoomSnapshotWire, snapshot_reset: bool) -> Vec<u8> {
    let msg = pb::RoomSnapshot {
        match_id: wire.match_id.clone(),
        seat: u32::from(wire.seat),
        ruleset_id: wire.ruleset_id.clone(),
        ruleset_version: wire.ruleset_version.clone(),
        room_seq: wire.room_seq,
        last_seat_seq: wire.last_seat_seq,
        view_hash: hex8(&wire.view_hash),
        view_schema_version: u32::from(wire.view_schema_version),
        view: wire.view.clone(),
        snapshot_reset,
        round_no: u32::from(wire.round_no),
    };
    encode(kind::ROOM_SNAPSHOT, &prost::Message::encode_to_vec(&msg))
}

/// 组装 `JoinAck` 帧。
pub fn join_ack_frame(
    match_id: &str,
    seat: u8,
    accept: bool,
    available_seats: &[u8],
    reason: Option<&str>,
) -> Vec<u8> {
    let msg = pb::JoinAck {
        match_id: match_id.to_owned(),
        seat: u32::from(seat),
        accept,
        available_seats: available_seats.iter().map(|s| u32::from(*s)).collect(),
        reason: reason.map(str::to_owned),
    };
    encode(kind::JOIN_ACK, &prost::Message::encode_to_vec(&msg))
}

/// 组装 `ActionAck` 帧。
pub fn action_ack_frame(
    request_id: &str,
    seat_seq: u64,
    room_seq: u64,
    accepted: bool,
    reason: Option<&str>,
) -> Vec<u8> {
    let msg = pb::ActionAck {
        request_id: request_id.to_owned(),
        seat_seq,
        room_seq,
        accepted,
        reason: reason.map(str::to_owned),
    };
    encode(kind::ACTION_ACK, &prost::Message::encode_to_vec(&msg))
}

/// 组装 `TrusteeState` 帧（托管状态变化）。
pub fn trustee_frame(seat: u8, reason: &str) -> Vec<u8> {
    let msg = pb::TrusteeState {
        seat: u32::from(seat),
        mode: pb::TrusteeMode::Ai as i32,
        reason: reason.to_owned(),
    };
    encode(kind::TRUSTEE_STATE, &prost::Message::encode_to_vec(&msg))
}

fn delta(event: pb::delta_event::Event) -> pb::DeltaEvent {
    pb::DeltaEvent { event: Some(event) }
}

/// 8 字节视图哈希 → 16 字符小写十六进制（§5.12）。
pub fn hex8(bytes: &[u8; 8]) -> String {
    let mut out = String::with_capacity(16);
    for b in bytes {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn summary(winner: Option<u8>) -> SettleSummary {
        SettleSummary {
            round_no: 3,
            ranks: vec![2, 0, 3, 1],
            winner_team: winner,
            completed_team: winner,
            step: Some(2),
            points: 0,
            match_end: None,
        }
    }

    #[test]
    fn settle_json_is_single_line_and_ordered() {
        let json = settle_json(&summary(Some(1)));
        assert_eq!(json, "{\"winner_team\":1,\"ranks\":[2,0,3,1],\"points\":0}");
        assert!(!json.contains(' '), "canonical JSON 不得含空白");
        assert!(!json.contains("null"), "canonical JSON 不得写 null");
    }

    #[test]
    fn settle_json_omits_absent_winner_team() {
        let json = settle_json(&summary(None));
        assert_eq!(json, "{\"ranks\":[2,0,3,1],\"points\":0}");
    }

    #[test]
    fn view_hash_is_16_lowercase_hex_chars() {
        let hash = [0x00, 0x01, 0xAB, 0xFF, 0x10, 0x20, 0x30, 0x40];
        let text = hex8(&hash);
        assert_eq!(text.len(), 16);
        assert_eq!(text, "0001abff10203040");
    }

    #[test]
    fn phase_enum_covers_all_four_engine_phases() {
        assert_eq!(phase_enum(gf_engine::engine::PHASE_DEAL), pb::Phase::Deal);
        assert_eq!(
            phase_enum(gf_engine::engine::PHASE_TRIBUTE),
            pb::Phase::Tribute
        );
        assert_eq!(phase_enum(gf_engine::engine::PHASE_PLAY), pb::Phase::Play);
        assert_eq!(
            phase_enum(gf_engine::engine::PHASE_SETTLE),
            pb::Phase::Settle
        );
    }
}
