//! 跨局结算连续轨迹（§5.13 队级 `levels` / `aFailStreak` 逐局累积）。
//!
//! 复用 [`ServerRoom::deal_next_round`]：它对 `seatSeq` / `roomSeq` 已不重置（见 harness
//! 上的 `M0-GAP-SEQ-CROSS-ROUND` 注释）。本测试在测试内连跑 3 局，核对队级档位跨局累加。
//!
//! **边界（⑧ 跨局携带尚未落地）**：
//! - `round_no` 全程占位 0，`settle → deal` 的相位转移与序号水位照旧不动；
//! - 引擎 `deal()` 目前仍把 `levels` / `aFailStreak` 重置为 initial，测试在每局 `deal_next_round`
//!   之后把上一局结算值**测试侧显式带回**，模拟将来 ⑧ 接线后的跨局携带；这不是引擎行为。
//! - 封顶 A、过 A、streak 达限退回 initial 三条状态依赖场景由 `gf-engine` 内 settle 函数级
//!   用例覆盖（构造 State → 调 settle → 断言），本轨迹只验自然策略下的跨局累加。

use gf_runtime_spike::harness::ServerRoom;
use gf_runtime_spike::recovery::ReconnectBuffer;

#[test]
fn three_rounds_carry_levels_across_deal() {
    let engine = gf_engine::cases::classic_engine().expect("classic 构造失败");
    let mut server = ServerRoom::new(engine).expect("首局发牌失败");
    let mut buffer = ReconnectBuffer::default();

    let mut prev_seat_seq = 0u64;

    for round in 1..=3 {
        server
            .run_to_settle(&mut buffer)
            .expect("本局跑到 settle 失败");
        let state = &server.state;

        // ⑧ 未做：roundNo 全程占位 0；序号水位跨局不归零（单调递增）。
        assert_eq!(state.round_no, 0, "roundNo 占位，⑧ 跨局序号未接线");
        let seat_seq = server.seat_seq(0);
        assert!(seat_seq > prev_seat_seq, "seatSeq 跨局不应重置");
        prev_seat_seq = seat_seq;

        match round {
            1 => {
                // 首局 initial（下标 0）打 1+3（头游 seat2 属队 0，队友 seat0 第 3 名）→ step=2。
                assert_eq!(state.levels, vec![2, 0]);
                assert_eq!(state.a_fail_streak, vec![0, 0]);
                assert_eq!(state.winner_team, Some(0));
                assert_eq!(state.ranks, vec![2, 1, 0, 3]);
                assert_eq!(state.settle_step, Some(2));
            }
            2 => {
                // 带回首局 [2,0]，再打 1+3 → +2 累加到 [4,0]；未触 A，streak 不动。
                assert_eq!(state.levels, vec![4, 0]);
                assert_eq!(state.a_fail_streak, vec![0, 0]);
                assert_eq!(state.winner_team, Some(0));
                assert_eq!(state.ranks, vec![2, 1, 0, 3]);
                assert_eq!(state.settle_step, Some(2));
            }
            3 => {
                // 第二局后轨迹翻转：头游落队 1，双下 1+2 → step=3；队 1 从 0 加到 3，队 0 保持 4。
                assert_eq!(state.levels, vec![4, 3]);
                assert_eq!(state.a_fail_streak, vec![0, 0]);
                assert_eq!(state.winner_team, Some(1));
                assert_eq!(state.ranks, vec![3, 1, 2, 0]);
                assert_eq!(state.settle_step, Some(3));
            }
            _ => unreachable!(),
        }

        if round < 3 {
            let carried_levels = state.levels.clone();
            let carried_streak = state.a_fail_streak.clone();
            server.deal_next_round().expect("下一局发牌失败");
            // 测试侧 seam：引擎 deal 重置队级状态，⑧ 接线前由测试把结算值带回新局。
            server.state.levels = carried_levels;
            server.state.a_fail_streak = carried_streak;
        }
    }
}
