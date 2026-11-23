//! M0 引擎契约层：公共 trait 与配套类型。
//!
//! 依据：
//! - design_draft.md §2.4 第 2 项（第 185 行）：引擎最小对外接口，签名在 M0 锁定，
//!   M1 只补原语实现，不改契约形状；
//! - §5.4（第 578–579 行）：`Validation` / `Finding` / `Level` / `Severity` /
//!   `EngineError` 变体清单；
//! - §5.7（第 630–660 行）：25 个错误码全表。

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

/// §5.7 全表 25 个错误码，逐条抄全，不合并、不改名。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorCode {
    RsSchemaInvalid,
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
    /// 返回 §5.7 表中的字面值（协议 / 日志 / Finding.code 一律用这个串）。
    pub fn code_str(self) -> &'static str {
        match self {
            ErrorCode::RsSchemaInvalid => "RS_SCHEMA_INVALID",
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

/// 引擎运行期状态（M1 补全内部结构；M0 只锁定接口形状）。
#[derive(Clone, Debug, Default)]
pub struct State {
    /// 占位：M1 接入完整牌局状态（§3.3 信息隔离：对外只开 `view(seat)`）。
    pub placeholder: (),
}

/// 玩家提交的动作（M1 补全 payload 分型；§5.9 `SubmitAction.payload`）。
#[derive(Clone, Debug, Default)]
pub struct Action {
    pub placeholder: (),
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
}
