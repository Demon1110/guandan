//! M0 引擎契约层：公共 trait 与配套类型。
//!
//! 依据：
//! - design_draft.md §2.4 第 2 项（第 185 行）：引擎最小对外接口，签名在 M0 锁定，
//!   M1 只补原语实现，不改契约形状；
//! - §5.4（第 578–579 行）：`Validation` / `Finding` / `Level` / `Severity` /
//!   `EngineError` 变体清单；
//! - §5.7（第 630–660 行）：27 个错误码全表。

use crate::hash::{LastPlay, RngState, TributeState, ZoneEntry};
use crate::ruleset::RuleSetBody;
use std::collections::HashSet;

/// 校验级别（§5.4 四级）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Level {
    L1,
    L2,
    L3,
    L4,
}

/// Finding 严重级别（§5.4 / §5.6）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    Blocker,
    Major,
    Minor,
}

/// §5.7 全表 27 个错误码，逐条抄全，不合并、不改名。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorCode {
    RsSchemaInvalid,
    RsFlushAnchorMissing,
    RsLadderConflict,
    RsRefMissing,
    RsNoTermination,
    RsIllegalActionRate,
    RsInterpreterTooOld,
    RsVersionLockedByRooms,
    RsCanaryBlocked,
    RsL4Mismatch,
    RsZoneConservation,
    RsZoneCapacity,
    RsPhaseUnimplemented,
    RsDeckCountMismatch,
    RsCapacityInvalid,
    RsTeamCoverageInvalid,
    RsQuotaExceeded,
    RsGoldenRegressionFailed,
    RsInternalPanic,
    RsSeatTaken,
    AuthInvalid,
    ClientTooOld,
    RateLimited,
    StaleSeq,
    TooLate,
    VersionConflict,
    RsNoActiveVersion,
}

impl ErrorCode {
    /// §5.7 全表 27 个错误码逐条枚举（reason 合法性校验遍历用，第二十四次变更）。
    pub fn all() -> [ErrorCode; 27] {
        [
            ErrorCode::RsSchemaInvalid,
            ErrorCode::RsFlushAnchorMissing,
            ErrorCode::RsLadderConflict,
            ErrorCode::RsRefMissing,
            ErrorCode::RsNoTermination,
            ErrorCode::RsIllegalActionRate,
            ErrorCode::RsInterpreterTooOld,
            ErrorCode::RsVersionLockedByRooms,
            ErrorCode::RsCanaryBlocked,
            ErrorCode::RsL4Mismatch,
            ErrorCode::RsZoneConservation,
            ErrorCode::RsZoneCapacity,
            ErrorCode::RsPhaseUnimplemented,
            ErrorCode::RsDeckCountMismatch,
            ErrorCode::RsCapacityInvalid,
            ErrorCode::RsTeamCoverageInvalid,
            ErrorCode::RsQuotaExceeded,
            ErrorCode::RsGoldenRegressionFailed,
            ErrorCode::RsInternalPanic,
            ErrorCode::RsSeatTaken,
            ErrorCode::AuthInvalid,
            ErrorCode::ClientTooOld,
            ErrorCode::RateLimited,
            ErrorCode::StaleSeq,
            ErrorCode::TooLate,
            ErrorCode::VersionConflict,
            ErrorCode::RsNoActiveVersion,
        ]
    }

    /// 返回 §5.7 表中的字面值（协议 / 日志 / Finding.code 一律用这个串）。
    pub fn code_str(self) -> &'static str {
        match self {
            ErrorCode::RsSchemaInvalid => "RS_SCHEMA_INVALID",
            ErrorCode::RsFlushAnchorMissing => "RS_FLUSH_ANCHOR_MISSING",
            ErrorCode::RsLadderConflict => "RS_LADDER_CONFLICT",
            ErrorCode::RsRefMissing => "RS_REF_MISSING",
            ErrorCode::RsNoTermination => "RS_NO_TERMINATION",
            ErrorCode::RsIllegalActionRate => "RS_ILLEGAL_ACTION_RATE",
            ErrorCode::RsInterpreterTooOld => "RS_INTERPRETER_TOO_OLD",
            ErrorCode::RsVersionLockedByRooms => "RS_VERSION_LOCKED_BY_ROOMS",
            ErrorCode::RsCanaryBlocked => "RS_CANARY_BLOCKED",
            ErrorCode::RsL4Mismatch => "RS_L4_MISMATCH",
            ErrorCode::RsZoneConservation => "RS_ZONE_CONSERVATION",
            ErrorCode::RsZoneCapacity => "RS_ZONE_CAPACITY",
            ErrorCode::RsPhaseUnimplemented => "RS_PHASE_UNIMPLEMENTED",
            ErrorCode::RsDeckCountMismatch => "RS_DECK_COUNT_MISMATCH",
            ErrorCode::RsCapacityInvalid => "RS_CAPACITY_INVALID",
            ErrorCode::RsTeamCoverageInvalid => "RS_TEAM_COVERAGE_INVALID",
            ErrorCode::RsQuotaExceeded => "RS_QUOTA_EXCEEDED",
            ErrorCode::RsGoldenRegressionFailed => "RS_GOLDEN_REGRESSION_FAILED",
            ErrorCode::RsInternalPanic => "RS_INTERNAL_PANIC",
            ErrorCode::RsSeatTaken => "RS_SEAT_TAKEN",
            ErrorCode::AuthInvalid => "AUTH_INVALID",
            ErrorCode::ClientTooOld => "CLIENT_TOO_OLD",
            ErrorCode::RateLimited => "RATE_LIMITED",
            ErrorCode::StaleSeq => "STALE_SEQ",
            ErrorCode::TooLate => "TOO_LATE",
            ErrorCode::VersionConflict => "VERSION_CONFLICT",
            ErrorCode::RsNoActiveVersion => "RS_NO_ACTIVE_VERSION",
        }
    }

    /// 按 §5.6 第 610 行的闸门归类给出默认严重级别。
    pub fn default_severity(self) -> Severity {
        match self {
            ErrorCode::RsSchemaInvalid
            | ErrorCode::RsFlushAnchorMissing
            | ErrorCode::RsLadderConflict
            | ErrorCode::RsRefMissing
            | ErrorCode::RsPhaseUnimplemented
            | ErrorCode::RsNoTermination
            | ErrorCode::RsIllegalActionRate
            | ErrorCode::RsL4Mismatch
            | ErrorCode::RsGoldenRegressionFailed
            | ErrorCode::RsQuotaExceeded
            | ErrorCode::RsInterpreterTooOld
            | ErrorCode::RsDeckCountMismatch
            | ErrorCode::RsCapacityInvalid
            | ErrorCode::RsTeamCoverageInvalid => Severity::Blocker,
            ErrorCode::RsZoneConservation
            | ErrorCode::RsZoneCapacity
            | ErrorCode::RsInternalPanic
            | ErrorCode::RsSeatTaken
            | ErrorCode::AuthInvalid
            | ErrorCode::ClientTooOld
            | ErrorCode::RateLimited
            | ErrorCode::StaleSeq
            | ErrorCode::TooLate
            | ErrorCode::RsCanaryBlocked
            | ErrorCode::RsVersionLockedByRooms
            | ErrorCode::VersionConflict
            | ErrorCode::RsNoActiveVersion => Severity::Major,
        }
    }
}

/// 单条校验发现（§5.4 第 578 行、§5.6 第 610 行）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finding {
    pub level: Level,
    pub code: ErrorCode,
    pub severity: Severity,
    /// 字段路径，如 `patterns[0].params.count`；解析期错误可带行/列号。
    pub location: String,
    pub message: String,
}

/// 校验结果：`ran_levels` 是**实际跑完**的级别集合，未实现 / 未请求的级别不得记入。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Validation {
    pub ran_levels: HashSet<Level>,
    pub findings: Vec<Finding>,
}

/// 座位号 newtype（§5.2 红线 4：引用必须强类型）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Seat(pub u8);

/// 区号 newtype。
#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ZoneId(pub String);

/// 牌型 id newtype。
#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PatternId(pub String);

/// 引擎运行期状态：直接承载 §5.12 第 2–11 项十段字段（N6）。
///
/// `CanonicalState` 是 `From<&State>` 的单向只读投影，只承担序列化与双端比对，
/// 不另存真源（§5.12 第 837 行）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct State {
    /// 第 2 段：phase 枚举序号 u16。
    pub phase: u16,
    /// 第 2 段：当前行动座位 u8。
    pub turn_seat: u8,
    /// 第 2 段：每队一个 u8，按队伍序号升序。
    pub levels: Vec<u8>,
    /// 第 3 段：座位号升序。
    pub seats: Vec<u8>,
    /// 第 4 段：按 `(seat, zoneId)` 实例化的各区手牌。
    pub zones: Vec<ZoneEntry>,
    /// 第 5 段：本回合无人出过牌时为 `None`。
    pub last_play: Option<LastPlay>,
    /// 第 6 段：每座位 u64 序号线，按座位号升序。
    pub seq_watermark: Vec<(u8, u64)>,
    /// 第 7 段：进贡 / 占位。
    pub tribute: TributeState,
    /// 第 8 段：计数器与种子摘要。
    pub rng: RngState,
    /// 第 9 段：每队一个 u8（0–3），按队伍序号升序。
    pub a_fail_streak: Vec<u8>,
    /// 第 10 段：按出完先后升序的座位序列。
    pub finish_order: Vec<u8>,
    /// 第 11 段：本局「打级队」队序号 u8（§5.12 第 822 行）。越界即不变量破坏。
    pub attacking_team: u8,
    /// 当前局序号 `roundNo: u16`（§6.2 裁定 B、§6.5 第 1100 行）：与 `SettleResult.roundNo`、
    /// `ActionRecord.roundNo` 同口径。
    ///
    /// 场次首局 = 1（不从 0 起）；只在下一局 `deal` 时 +1，`settle` 结束不改局号——
    /// 「第 N 局已结算、第 N+1 局未开始」的空窗期本字段仍是 N，判断相位只看 `phase`。
    /// **不进 `stateHash`**（§5.12 十段字段集不含局序号）。
    pub round_no: u16,
    /// §5.12 第 800 行：快照同时保存 `seed` 原值，`seedDigest` 由它派生；
    /// seed 本身不进哈希拼接。
    pub seed: u64,
    /// N11 运行期记账：连续过牌数。§5.12 十段内没有对应字段，
    /// **不进 stateHash**；崩溃恢复需由动作日志重建（设计缺口，见回报 N11）。
    pub passes_since_last_play: u8,
    /// 头游（`finish_order[0]`）所在队（升级用）。诊断字段，**不进 stateHash**
    /// （§5.12 第 2–11 项字段集不含）；1+4 形态下与 `completed_team` 不是同一概念。
    pub winner_team: Option<u8>,
    /// 先全员出完的队（局终止判定用）。诊断字段，**不进 stateHash**。
    pub completed_team: Option<u8>,
    /// 名次序列：按名次 1→n 列 seat（与 `SettleResult.result.ranks` 同口径）。
    /// 诊断字段，**不进 stateHash**。
    pub ranks: Vec<u8>,
    /// 本局升级档 `step`：按 `rankCombo` 查 `levelTable.upgrade[]`；score-only 局为 `None`。
    /// 诊断字段，**不进 stateHash**。
    pub settle_step: Option<u8>,
    /// 诊断字段，**不进 stateHash**：本局抗贡判定成立（§5.13 抗贡行）。抗贡 / 首局 /
    /// `enabled=false` 时为 `false`；抗贡成立时进贡与还贡均不发生、事件列为空。
    pub tribute_resisted: bool,
    /// 诊断字段，**不进 stateHash**：本局进贡 / 还贡逐条转移记录（§5.9 `TributeEvent`）。
    pub tribute_events: Vec<TributeEventRecord>,
}

/// 进贡 / 还贡逐条转移记录（§5.9 `TributeEvent{from,to,card}` 回传口径）。
///
/// 诊断字段，**不进 `stateHash`**（§5.12 第 7 段只载单组 `TributeState`）；双下两对的
/// 配对 / 选牌 / 还贡逐条落在这里，供房间层广播与测试断言。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TributeEventRecord {
    /// 贡方（进贡）或受贡方（还贡）座位。
    pub from: u8,
    /// 受贡方（进贡）或贡方（还贡）座位。
    pub to: u8,
    /// 转移的牌 id（u16）。
    pub card: u16,
    /// `true` = 还贡（受贡方→贡方）；`false` = 进贡（贡方→受贡方）。
    pub is_return: bool,
}

/// 玩家提交的动作（§5.9 第 721–726 行 `SubmitAction.payload` 按 kind 分型）。
///
/// N12（待裁决）：协议侧 payload 形状是唯一依据，引擎侧未另写；按它镜像为
/// `Play{cards}` / `Pass` 两型，`pass` 为空结构体、禁止带字段。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    /// 出牌：只传牌 id（`patternId` 由服务端判定，`patternHint` 不参与判定）。
    Play { cards: Vec<u16> },
    /// 过牌：空结构体。
    Pass,
}

/// 下发视图：`view` 是 bytes，内容为 canonical JSON（§5.9 / §5.12）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct View {
    pub bytes: Vec<u8>,
}

/// 引擎运行期错误出口（§5.4 第 579 行）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EngineError {
    /// 动作非法，运行期拒绝。
    IllegalAction { reason: String },
    /// 候选枚举峰值超 4096（对应 `RS_QUOTA_EXCEEDED`，§5.7 第 648 行）。
    QuotaExceeded,
    /// 引擎内部不变量破坏（与 `RS_INTERNAL_PANIC` 同族告警处置）。
    Internal { message: String },
    /// native 侧 panic 被 `catch_unwind` 捕获。
    Panic,
    /// Σ 各区当前牌数 ≠ deck 总张数（`RS_ZONE_CONSERVATION`）。
    ZoneConservation,
    /// 某区当前牌数 > `capacity`（`RS_ZONE_CAPACITY`）。
    ZoneCapacity { zone: ZoneId, count: u32 },
}

impl std::fmt::Display for EngineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EngineError::IllegalAction { reason } => write!(f, "illegal action: {reason}"),
            EngineError::QuotaExceeded => write!(f, "action candidate quota exceeded"),
            EngineError::Internal { message } => write!(f, "internal invariant broken: {message}"),
            EngineError::Panic => write!(f, "engine panic captured"),
            EngineError::ZoneConservation => write!(f, "zone card conservation failed"),
            EngineError::ZoneCapacity { zone, count } => {
                write!(f, "zone {} capacity exceeded: {count} cards", zone.0)
            }
        }
    }
}

impl std::error::Error for EngineError {}

/// M0 锁定的引擎最小对外接口（§2.4 第 2 项、§5.4）。
///
/// 四个方法都在同一个 trait 上；双产物（native / wasm）共用同一份实现。
/// M1 只补原语实现，不改契约形状。
pub trait Engine {
    /// 校验 RuleSetBody。入参 `levels` 声明本次要跑的级别集合；
    /// `ran_levels` 记录实际跑完的级别（§5.4 第 578 行）。
    fn validate(&self, ruleset: &RuleSetBody, levels: &[Level]) -> Validation {
        crate::ruleset::validate::validate(ruleset, levels)
    }

    /// 枚举当前状态下的合法动作。
    ///
    /// **契约冲突 G1（待裁决）**：§2.4 第 185 行写 `legal_actions(&State) -> Vec<Action>`，
    /// 但 §5.4 第 579 行把 `legal_actions` 列为 `EngineError` 的错误出口，且
    /// `QuotaExceeded` 必须从它出来（§5.7 第 648 行）。两处字面冲突，本轮按保守读法
    /// 锁定为 `Result<Vec<Action>, EngineError>`；若裁决改回裸 `Vec`，需同步改 trait 签名。
    fn legal_actions(&self, state: &State) -> Result<Vec<Action>, EngineError> {
        let _ = state;
        todo!("M1 补原语实现，接口形状在 M0 锁定，不得改")
    }

    /// 应用一个动作，返回新状态。
    fn apply(&self, state: State, action: Action) -> Result<State, EngineError> {
        let _ = (state, action);
        todo!("M1 补原语实现，接口形状在 M0 锁定，不得改")
    }

    /// 按座位裁剪出下发视图（信息隔离，§3.3 约束 3）。
    fn view(&self, state: &State, seat: Seat) -> View {
        let _ = (state, seat);
        todo!("M1 补原语实现，接口形状在 M0 锁定，不得改")
    }

    /// 组牌推荐（只读旁路，§5.11「组牌推荐」段、第二十次变更）。
    ///
    /// 纯函数、只读、确定性：**不进 `stateHash`、不改 `legal_actions`、不进判定链**；
    /// native 与 wasm 必须同解。
    fn recommend(
        &self,
        state: &State,
        seat: Seat,
    ) -> Result<crate::recommend::Layout, EngineError> {
        let _ = (state, seat);
        todo!("M1 补原语实现，接口形状在 M0 锁定，不得改")
    }
}
