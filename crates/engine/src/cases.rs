//! M0 确定性用例：native 与 wasm 编译同一份代码（§2.4 gate 第 1 项）。
//!
//! 引擎内不做文件 I/O（§3.3 约束 2）：夹具 YAML 由 `include_str!` 在编译期嵌入，
//! 双端读到的是同一份字节串。

use crate::contract::{Action, Engine, EngineError, State};
use crate::engine::{M0Engine, PHASE_SETTLE};
use crate::hash::{state_hash, CanonicalState};
use crate::ruleset::from_yaml;

/// 固定种子（§5.13 RNG 唯一来源；N7 取 BLAKE3(seed‖counter) 前 8 字节）。
pub const M0_SEED: u64 = 0x0D15EA5E_00000001;

const M0_RULESET_YAML: &str = include_str!("../tests/fixtures/m0-minimal/ruleset.yaml");

/// 单步动作日志：seat / kind / cards（可逐条复现）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActionLogEntry {
    pub seat: u8,
    pub kind: String,
    pub cards: Vec<u16>,
}

/// 双端比对产物：最终状态、32 字节 stateHash、动作日志，外加一组终局诊断字段。
///
/// 诊断字段（`steps`/`phase`/`finish_order`/`completed_team`/`winner_team`/
/// `settle_count`/`discard_count`/`hands_total`）只服务终局复核与双端逐字段比对，
/// **不进 stateHash**（§5.12 第 8 项字段集不含）。
#[derive(Clone, Debug)]
pub struct GoldenOutcome {
    pub state: State,
    pub state_hash: [u8; 32],
    pub actions: Vec<ActionLogEntry>,
    /// 实际走的动作步数（与 `actions.len()` 一致）。
    pub steps: usize,
    /// 终局相位（应为 `PHASE_SETTLE`）。
    pub phase: u16,
    /// 终局名次：4 个座位按出完先后排序。
    pub finish_order: Vec<u8>,
    /// 先全员出完的队伍下标（局终止判定用；原 `terminal_team` 改名，语义不变）。
    /// 诊断字段，**不进 stateHash**（§5.12 第 8 项字段集不含）。§5.13 第 902 行三种
    /// 收尾：双下 1+2 / 1+3 / 1+4；1+4 形态下是对家队，不等于头游所在队。
    pub completed_team: Option<u8>,
    /// 头游（`finish_order[0]`）所在队（升级用）。诊断字段，**不进 stateHash**
    /// （§5.12 第 8 项字段集不含）。1+4 形态下是头游队，与 `completed_team`
    /// 不是同一概念，不许互相替代。
    pub winner_team: Option<u8>,
    /// settle 转移发生次数，正常应恰为 1。
    pub settle_count: u32,
    /// 弃牌区累计牌数。
    pub discard_count: usize,
    /// 终局留在各人手牌区的牌数。
    pub hands_total: usize,
}

pub fn m0_ruleset_yaml() -> &'static str {
    M0_RULESET_YAML
}

pub fn build_engine() -> Result<M0Engine, String> {
    let body = from_yaml(M0_RULESET_YAML).map_err(|e| e.to_string())?;
    Ok(M0Engine { ruleset: body })
}

/// 基准夹具：固定策略走 12 步到中局（手牌约 27、已有 lastPlay）。
/// 纯状态推进，不含任何时钟调用（§3.3）。
pub fn mid_game_state() -> (M0Engine, State) {
    let engine = build_engine().expect("构造引擎失败");
    let mut state = engine.deal(M0_SEED).expect("发牌失败");
    for _ in 0..12 {
        if state.phase == PHASE_SETTLE {
            break;
        }
        let legal = engine.legal_actions(&state).expect("枚举合法动作失败");
        let action = legal.into_iter().next().expect("中局候选不应为空");
        state = engine.apply(state, action).expect("落子失败");
    }
    (engine, state)
}

/// 基准夹具：只 `deal`、不落子（`last_play = None`）的领出态。
/// 领出态是候选峰值口径：手牌全部为合法候选，枚举 push 与四元组排序成本最高，
/// 对应 gate「单次动作解释 <1ms」的最坏态。纯状态推进，不含时钟调用（§3.3）。
pub fn lead_state() -> (M0Engine, State) {
    let engine = build_engine().expect("构造引擎失败");
    let state = engine.deal(M0_SEED).expect("发牌失败");
    debug_assert!(state.last_play.is_none(), "领出态 last_play 应为 None");
    (engine, state)
}

/// 固定策略：每步取 `legal_actions` 排序后的第一个（最小合法单张）；
/// 跑到 `settle` 为止。策略固定即序列固定，日志可逐条复现。
pub fn run_m0_golden() -> Result<GoldenOutcome, String> {
    let engine = build_engine()?;
    let mut state = engine.deal(M0_SEED).map_err(|e| e.to_string())?;
    let mut actions = Vec::new();
    let mut steps = 0usize;
    let mut settle_count: u32 = 0;

    loop {
        if state.phase == PHASE_SETTLE {
            break;
        }
        let legal = engine.legal_actions(&state).map_err(|e| e.to_string())?;
        // N15：phase=PLAY 且非终局时 legal 为空，引擎必须报 Internal；用例侧不允许静默 break——
        // 一旦守卫回归被绕过，静默停止会出一个假锚点。
        if legal.is_empty() {
            return Err(format!(
                "phase=PLAY 下 legal_actions 返回空（应由引擎报 Internal）：turnSeat={} finishOrder={:?}",
                state.turn_seat, state.finish_order
            ));
        }
        let action = legal.into_iter().next().unwrap();
        let log = match &action {
            Action::Play { cards } => ActionLogEntry {
                seat: state.turn_seat,
                kind: "play".to_owned(),
                cards: cards.clone(),
            },
            Action::Pass => ActionLogEntry {
                seat: state.turn_seat,
                kind: "pass".to_owned(),
                cards: Vec::new(),
            },
        };
        state = engine.apply(state, action).map_err(|e| e.to_string())?;
        if state.phase == PHASE_SETTLE {
            settle_count += 1;
        }
        actions.push(log);
        steps += 1;
        if steps > 10_000 {
            return Err("m0 golden 超出安全步数上限，疑似死循环".to_owned());
        }
    }

    // —— 终局硬断言（§5.13「局终止与名次」）：禁止把「legal 为空」当退出条件，
    // 上次「双端一致」是在 102 步提前退出的 bug 下跑出来的，一致只证明两边算得一样，
    // 不证明算的是完整一局；以下断言任一失败即用例红。 ——

    let hand_idx = engine.hand_zone_index();
    let hands_total: usize = state
        .zones
        .iter()
        .filter(|z| z.zone_id == hand_idx)
        .map(|z| z.cards.len())
        .sum();
    let discard_count = engine
        .discard_zone_index()
        .and_then(|i| state.zones.iter().find(|z| z.zone_id == i))
        .map(|z| z.cards.len())
        .unwrap_or(0);
    // 终局触发（§5.13 第 902 行「局终止与名次」）：一局在某队全部队员均已出完时结束。
    // 三种收尾形态（四人两队基线）：双下 1+2（该队占 1、2 名）、1+3（夹进对家一人）、
    // 1+4（对家两人先出完而本队头游在前）。
    // completed_team = 最先全员出完的队伍：队员全部进入 finish_order 的队伍中，
    // 「最晚名次」（在 finish_order 中的最大下标）最小者；并列按队伍下标小者。
    // winner_team = 头游（finish_order[0]）所在队，供升级用。1+4 形态下
    // completed_team 是对家队、winner_team 是头游队，两者不是同一概念，不许互相替代。
    let completed_team = engine
        .ruleset
        .seats
        .teams
        .iter()
        .enumerate()
        .filter(|(_, team)| team.iter().all(|s| state.finish_order.contains(s)))
        .map(|(i, team)| {
            let last_rank = team
                .iter()
                .map(|s| {
                    state
                        .finish_order
                        .iter()
                        .position(|&x| x == *s)
                        .expect("队员应已进入名次")
                })
                .max()
                .expect("队伍非空");
            (i, last_rank)
        })
        .min_by(|a, b| a.1.cmp(&b.1).then(a.0.cmp(&b.0)))
        .map(|(i, _)| i as u8);
    let winner_team = state.finish_order.first().and_then(|&head| {
        engine
            .ruleset
            .seats
            .teams
            .iter()
            .position(|team| team.contains(&head))
            .map(|i| i as u8)
    });

    if state.phase != PHASE_SETTLE {
        return Err(format!(
            "终局相位应为 settle，实际 phase={} turnSeat={} finishOrder={:?}",
            state.turn_seat, state.phase, state.finish_order
        ));
    }
    if settle_count != 1 {
        return Err(format!("settle 转移次数应为 1，实际 {settle_count}"));
    }
    if state.finish_order.len() != 4 {
        return Err(format!(
            "finish_order 应补位到 4 个座位，实际 {:?}",
            state.finish_order
        ));
    }
    let mut sorted_finish = state.finish_order.clone();
    sorted_finish.sort_unstable();
    if sorted_finish != vec![0, 1, 2, 3] {
        return Err(format!(
            "finish_order 必须无重复且覆盖 0..3，实际 {:?}",
            state.finish_order
        ));
    }
    let Some(completed) = completed_team else {
        return Err(format!(
            "终局没有队伍全员进入名次：finishOrder={:?}",
            state.finish_order
        ));
    };
    let Some(winner) = winner_team else {
        return Err(format!(
            "头游座位未归属任何队伍：finishOrder={:?}",
            state.finish_order
        ));
    };
    let completed_seats = &engine.ruleset.seats.teams[completed as usize];
    if !completed_seats
        .iter()
        .all(|s| state.finish_order.contains(s))
    {
        return Err(format!(
            "completed_team={completed} 座位 {completed_seats:?} 未全部进入名次：finishOrder={:?}",
            state.finish_order
        ));
    }
    // 旧 bug：102 步提前退出、finish_order=[1]；下界 120 不允许退回那种位置
    // （当前种子实测 150 步）。
    if steps < 120 {
        return Err(format!(
            "步数 {steps} 低于下界 120，疑似提前退出（旧 bug 为 102 步）"
        ));
    }
    if discard_count + hands_total != 108 {
        return Err(format!(
            "牌数不守恒：discard={discard_count} hands={hands_total} 合计 {} ≠ 108",
            discard_count + hands_total
        ));
    }
    // 终局后 apply 必须拒绝（证明是「打完了」而不是「停住了」）。
    match engine.apply(state.clone(), Action::Pass) {
        Err(EngineError::IllegalAction { .. }) => {}
        other => return Err(format!("终局后 apply 应返回 IllegalAction，实际 {other:?}")),
    }

    let outcome_phase = state.phase;
    let canonical = CanonicalState::from(&state);
    let state_hash = state_hash(&canonical);
    Ok(GoldenOutcome {
        finish_order: state.finish_order.clone(),
        state,
        state_hash,
        actions,
        steps,
        phase: outcome_phase,
        completed_team: Some(completed),
        winner_team: Some(winner),
        settle_count,
        discard_count,
        hands_total,
    })
}

// ---------- wasm C ABI 导出（仅 wasm32 目标） ----------

// 报告缓冲定长 64 字节，native 端与 Node 宿主共用同一份小端布局：
// - `[0..4]`   abiVersion u32 LE（本次 = 2；1 表示旧的 48 字节布局）；
// - `[4..5]`   completedTeam u8，先全员出完的队，`0xFF` = 无；
// - `[5..6]`   winnerTeam u8，头游所在队，`0xFF` = 无；
// - `[6..8]`   reserved u16 LE，恒 0，留给以后；
// - `[8..40]`  stateHash 原始 32 字节；
// - `[40..44]` steps u32 LE；
// - `[44..48]` phase u32 LE；
// - `[48..52]` finishOrderLen u32 LE；
// - `[52..56]` finishOrder 4×u8，不足位填 `0xFF`；
// - `[56..60]` discardCount u32 LE；
// - `[60..64]` handsTotal u32 LE。
// finishOrder 固定 4 槽：四座基线够用；六人变体要扩 ABI 时靠 abiVersion 区分，
// 不在本笔做大。宿主读完必须调用 `gf_free(ptr, 64)` 释放。

#[cfg(target_arch = "wasm32")]
const REPORT_BUF_LEN: usize = 64;

/// 报告 ABI 版本号：2 = 64 字节布局；1 = 旧 48 字节布局。
#[cfg(target_arch = "wasm32")]
const REPORT_ABI_VERSION: u32 = 2;

#[cfg(target_arch = "wasm32")]
#[no_mangle]
pub extern "C" fn gf_free(ptr: *mut u8, len: usize) {
    if ptr.is_null() {
        return;
    }
    // 重建 Vec 释放；容量按 len 还原（本链路只释放定长 64 字节报告缓冲）。
    unsafe {
        let _ = Vec::<u8>::from_raw_parts(ptr, len, len);
    }
}

/// 跑完 m0 golden，返回 64 字节报告堆缓冲指针（布局见 `REPORT_BUF_LEN` 注释）。
#[cfg(target_arch = "wasm32")]
#[no_mangle]
pub extern "C" fn gf_m0_golden_report() -> *mut u8 {
    let outcome = run_m0_golden().expect("m0 golden 运行失败");
    let mut buf = Vec::with_capacity(REPORT_BUF_LEN);
    buf.extend_from_slice(&REPORT_ABI_VERSION.to_le_bytes());
    buf.push(outcome.completed_team.unwrap_or(0xFF));
    buf.push(outcome.winner_team.unwrap_or(0xFF));
    buf.extend_from_slice(&0u16.to_le_bytes());
    buf.extend_from_slice(&outcome.state_hash);
    buf.extend_from_slice(&(outcome.steps as u32).to_le_bytes());
    buf.extend_from_slice(&(outcome.phase as u32).to_le_bytes());
    buf.extend_from_slice(&(outcome.finish_order.len() as u32).to_le_bytes());
    for i in 0..4 {
        buf.push(outcome.finish_order.get(i).copied().unwrap_or(0xFF));
    }
    buf.extend_from_slice(&(outcome.discard_count as u32).to_le_bytes());
    buf.extend_from_slice(&(outcome.hands_total as u32).to_le_bytes());
    let ptr = buf.as_mut_ptr();
    std::mem::forget(buf);
    ptr
}

/// 基准批处理：对固定中局状态连续枚举 `batch` 次合法动作，返回累计候选条数
/// （checksum，防止宿主侧优化删除；宿主用 checksum/iterations 得每次候选数）。
/// 计时由 Node 宿主 `process.hrtime` 完成，本函数不含时钟调用（§3.3 豁免边界）。
#[cfg(target_arch = "wasm32")]
#[no_mangle]
pub extern "C" fn gf_bench_legal_actions(batch: u32) -> u64 {
    let (engine, state) = mid_game_state();
    let mut checksum: u64 = 0;
    for _ in 0..batch {
        let legal = engine
            .legal_actions(&state)
            .expect("基准夹具枚举合法动作失败");
        checksum = checksum.wrapping_add(legal.len() as u64);
    }
    checksum
}

/// 基准批处理：领出态（`last_play = None`，候选峰值口径）连续枚举 `batch` 次，
/// 返回累计候选条数 checksum。本函数不含时钟调用（§3.3 豁免边界）。
#[cfg(target_arch = "wasm32")]
#[no_mangle]
pub extern "C" fn gf_bench_legal_actions_lead(batch: u32) -> u64 {
    let (engine, state) = lead_state();
    let mut checksum: u64 = 0;
    for _ in 0..batch {
        let legal = engine
            .legal_actions(&state)
            .expect("基准夹具枚举合法动作失败");
        checksum = checksum.wrapping_add(legal.len() as u64);
    }
    checksum
}

/// 基准批处理：固定策略连续落子 `batch` 次；一局 settle 后重新 deal（deal 不计时、
/// 不计入 apply 次数）。返回 checksum（座位号累计），防止被优化删除。
#[cfg(target_arch = "wasm32")]
#[no_mangle]
pub extern "C" fn gf_bench_apply(batch: u32) -> u64 {
    let engine = build_engine().expect("构造引擎失败");
    let mut state = engine.deal(M0_SEED).expect("发牌失败");
    let mut checksum: u64 = 0;
    let mut applied: u32 = 0;
    while applied < batch {
        if state.phase == PHASE_SETTLE {
            state = engine.deal(M0_SEED).expect("重发牌失败");
            continue;
        }
        let legal = engine
            .legal_actions(&state)
            .expect("基准夹具枚举合法动作失败");
        let action = legal.into_iter().next().expect("非终局候选不应为空");
        state = engine.apply(state, action).expect("落子失败");
        checksum = checksum.wrapping_add(state.turn_seat as u64);
        applied += 1;
    }
    checksum
}
