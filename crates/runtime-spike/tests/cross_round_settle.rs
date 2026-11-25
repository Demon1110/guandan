//! ⑧ 跨局携带：`settle → deal` 真实相位转移的三局连续轨迹活测试。
//!
//! 口径（§6.2 裁定 B / §6.5 / §5.13）：
//! - `roundNo` 场次首局 = 1，只在下一局 `deal` 时 +1；`settle` 结束不改局号；
//!   `SettleResult.roundNo` 与本局 `ActionRecord.roundNo` 同为动作发生时的局号，不存在差 1。
//! - 跨局携带 `levels` / `aFailStreak` / `attackingTeam`；`seq_watermark` 与房间层
//!   `seatSeq` / `roomSeq` 同一 `matchId` 内连续递增，换局不清零、不归零。
//! - 快照头含 `roundNo`；`roundNo` 不在 `view` 键集内；`viewSchemaVersion` 不因此升版。
//!
//! 本用例走 `guandan-classic` 首局固定策略跑到 `settle`（rank-combo 升级档可观测），
//! 再由引擎 `deal_next_round` 真实续发第 2、3 局；不使用测试侧 seam 回填结算值。

use gf_engine::cases::classic_engine;
use gf_engine::contract::{Engine, Seat};
use gf_engine::engine::PHASE_SETTLE;
use gf_engine::view::view_hash;
use gf_runtime_spike::harness::ServerRoom;
use gf_table_runtime::recovery::ReconnectBuffer;

/// 一局跑完后的快照载体（只记录本用例需要跨局比对的字段）。
struct RoundOutcome {
    round_no: u16,
    levels: Vec<u8>,
    a_fail_streak: Vec<u8>,
    attacking_team: u8,
    steps: usize,
    /// 本局动作记录的 `roundNo`（应与本局 `round_no` 全等）。
    record_round_nos: Vec<u16>,
    /// 本局每条记录的 `roomSeq`（跨局严格递增）。
    record_room_seqs: Vec<u64>,
}

fn run_round(
    room: &mut ServerRoom,
    buffer: &mut ReconnectBuffer,
    round_idx: usize,
) -> RoundOutcome {
    let records_before = buffer.records().len();
    let steps = room
        .run_to_settle(buffer)
        .unwrap_or_else(|e| panic!("第 {round_idx} 局跑到 settle 失败：{e:?}"));

    let state = &room.state;
    assert_eq!(
        state.phase, PHASE_SETTLE,
        "第 {round_idx} 局结束相位应为 settle"
    );

    let mut record_round_nos = Vec::new();
    let mut record_room_seqs = Vec::new();
    for record in buffer.records().iter().skip(records_before) {
        record_round_nos.push(record.round_no);
        record_room_seqs.push(record.room_seq);
    }

    RoundOutcome {
        round_no: state.round_no,
        levels: state.levels.clone(),
        a_fail_streak: state.a_fail_streak.clone(),
        attacking_team: state.attacking_team,
        steps,
        record_round_nos,
        record_room_seqs,
    }
}

#[test]
fn three_rounds_carry_levels_streak_team_and_seq_across_deal() {
    let engine = classic_engine().expect("classic 夹具构造失败");
    let mut room = ServerRoom::new(engine).expect("首局发牌失败");
    let mut buffer = ReconnectBuffer::default();

    // —— 第 1 局 ——
    let r1 = run_round(&mut room, &mut buffer, 1);
    assert_eq!(r1.round_no, 1, "场次首局 roundNo 应为 1");
    assert!(!r1.record_round_nos.is_empty(), "第 1 局不应没有动作记录");
    assert!(
        r1.record_round_nos.iter().all(|&n| n == 1),
        "第 1 局所有 ActionRecord.roundNo 应为 1"
    );
    // 升级可观测：classic 首局 rank-combo 1+3，胜方队 +2（golden 同口径）。
    assert_eq!(
        r1.levels,
        vec![2, 0],
        "第 1 局 settle 后胜方队 level 下标应为 2"
    );
    assert_eq!(r1.a_fail_streak, vec![0, 0], "未在 A 上，streak 全队 0");
    assert_eq!(
        r1.attacking_team, 0,
        "settle 结束 attackingTeam 接头游所在队 0"
    );

    // 快照头：roundNo 入快照头、不进 view、schemaVersion 不升版。
    let snapshot = room.snapshot(false);
    assert_eq!(snapshot.round_no, 1, "快照头 roundNo 应等于本局局号");
    assert_eq!(
        snapshot.view_schema_version, 2,
        "classic view.schemaVersion 仍为 2，跨局携带不触发升版"
    );
    let view = room.engine.view(&room.state, Seat(0));
    assert!(
        !view.bytes.windows(7).any(|w| w == b"roundNo"),
        "roundNo 不属于 view 白名单，视图字节内不得出现 roundNo 键"
    );
    assert_eq!(
        snapshot.view_hash,
        view_hash(0, snapshot.room_seq, &view.bytes),
        "viewHash 按 seat ‖ roomSeq ‖ viewBytes 重算应一致"
    );

    let mut last_room_seq = *r1.record_room_seqs.last().unwrap();
    let last_seat_seqs = room.seat_seqs();

    // —— 续发第 2 局（真实 settle → deal 转移，不回填结算值）——
    room.deal_next_round().expect("第 2 局续发失败");
    assert_eq!(room.state.round_no, 2, "第 2 局 deal 后 roundNo 应为 2");
    // 跨局携带：levels / streak / attackingTeam 原样延续。
    assert_eq!(
        room.state.levels, r1.levels,
        "第 2 局开局 levels 应携带第 1 局"
    );
    assert_eq!(
        room.state.a_fail_streak, r1.a_fail_streak,
        "第 2 局开局 streak 应携带第 1 局"
    );
    assert_eq!(
        room.state.attacking_team, r1.attacking_team,
        "第 2 局开局 attackingTeam 应携带第 1 局"
    );
    // 水位跨局不清零：引擎水位 = 房间层各座位已落子条数。
    for seat in 0..4u8 {
        assert_eq!(
            room.state.seq_watermark.iter().find(|(s, _)| *s == seat),
            Some(&(seat, last_seat_seqs[seat as usize])),
            "第 2 局开局座位 {seat} 水位不得复位"
        );
    }

    let r2 = run_round(&mut room, &mut buffer, 2);
    assert_eq!(r2.round_no, 2, "第 2 局 settle 时 roundNo 仍为 2");
    assert!(
        r2.record_round_nos.iter().all(|&n| n == 2),
        "第 2 局所有 ActionRecord.roundNo 应为 2"
    );
    for &seq in &r2.record_room_seqs {
        assert!(seq > last_room_seq, "roomSeq 跨局必须严格递增，不得归零");
        last_room_seq = seq;
    }
    for seat in 0..4u8 {
        assert!(
            room.seat_seqs()[seat as usize] >= last_seat_seqs[seat as usize],
            "seatSeq 跨局不归零（座位 {seat}）"
        );
    }

    // —— 续发第 3 局 ——
    room.deal_next_round().expect("第 3 局续发失败");
    assert_eq!(room.state.round_no, 3, "第 3 局 deal 后 roundNo 应为 3");
    assert_eq!(
        room.state.levels, r2.levels,
        "第 3 局开局 levels 携带第 2 局"
    );
    assert_eq!(
        room.state.a_fail_streak, r2.a_fail_streak,
        "第 3 局开局 streak 携带第 2 局"
    );

    let r3 = run_round(&mut room, &mut buffer, 3);
    assert_eq!(r3.round_no, 3, "第 3 局 settle 时 roundNo 仍为 3");
    assert!(
        r3.record_round_nos.iter().all(|&n| n == 3),
        "第 3 局所有 ActionRecord.roundNo 应为 3"
    );
    for &seq in &r3.record_room_seqs {
        assert!(seq > last_room_seq, "roomSeq 第 3 局继续严格递增");
    }

    // 三局局号与记录局号逐一一致：不存在 SettleResult.roundNo 与 ActionRecord.roundNo 差 1。
    for outcome in [&r1, &r2, &r3] {
        assert!(
            outcome
                .record_round_nos
                .iter()
                .all(|&n| n == outcome.round_no),
            "第 {} 局 SettleResult.roundNo={} 与记录 roundNo 一致",
            outcome.round_no,
            outcome.round_no
        );
    }

    println!(
        "[cross-round] 步数 r1={} r2={} r3={}；levels r1={:?} r2={:?} r3={:?}；streak r1={:?} r2={:?} r3={:?}；attacking r1={} r2={} r3={}",
        r1.steps, r2.steps, r3.steps,
        r1.levels, r2.levels, r3.levels,
        r1.a_fail_streak, r2.a_fail_streak, r3.a_fail_streak,
        r1.attacking_team, r2.attacking_team, r3.attacking_team
    );
}

/// 非 settle 状态不允许续发下一局（相位转移闸门）。
#[test]
fn deal_next_round_rejected_before_settle() {
    let engine = classic_engine().expect("classic 夹具构造失败");
    let mut room = ServerRoom::new(engine).expect("首局发牌失败");
    // 开局是 play 相位，直接续发必须被引擎拒绝。
    assert!(room.deal_next_round().is_err(), "play 相位续发下一局应报错");
}
