//! `params` 分型 ADT（§5.2 参数表、§5.3 红线）。
//!
//! 按 `primitiveId` 分发到具名参数结构体：未知键、缺失必填键、类型不符一律
//! `RS_SCHEMA_INVALID`。两段式反序列化时，`serde_yaml::Mapping` 只是局部临时值，
//! 不出现在交付类型里；交付类型中没有 `Value` 字段，也没有 `#[serde(flatten)]`。

use crate::contract::ZoneId;
use crate::ruleset::model::{
    AfterAllPass, FirstRound, LevelScope, OrderDirection, PaySelection, ReturnCeiling,
    ReturnFallback, ScoreDimension, TributeTrigger, TurnDirection,
};
use crate::ruleset::parse::SchemaError;
use serde::Deserialize;

fn default_true() -> bool {
    true
}
fn default_zero_u8() -> u8 {
    0
}
fn default_one_u8() -> u8 {
    1
}
fn default_hand_zone() -> ZoneId {
    ZoneId("hand".to_owned())
}
fn default_asc() -> OrderDirection {
    OrderDirection::Asc
}
fn default_clockwise() -> TurnDirection {
    TurnDirection::Clockwise
}
fn default_last_player() -> AfterAllPass {
    AfterAllPass::LastPlayer
}
fn default_last_round_rank() -> TributeTrigger {
    TributeTrigger::LastRoundRank
}
fn default_skip() -> FirstRound {
    FirstRound::Skip
}
fn default_max_except_wildcard() -> PaySelection {
    PaySelection::MaxRankExceptWildcard
}
fn default_under_ten() -> ReturnCeiling {
    ReturnCeiling::UnderTen
}
fn default_smallest() -> ReturnFallback {
    ReturnFallback::Smallest
}
fn default_by_team() -> LevelScope {
    LevelScope::ByTeam
}
fn default_team() -> ScoreDimension {
    ScoreDimension::Team
}

/// 各原语的固定参数结构体（§5.2 第 508–545 行）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PrimitiveParams {
    // —— 牌型 ——
    SameRank {
        count: u8,
    },
    Straight {
        length: u8,
        wildcard_allowed: bool,
    },
    ConsecutivePairs {
        pairs: u8,
    },
    ConsecutiveTriples {
        triples: u8,
    },
    Bomb {
        count: u8,
    },
    StraightFlush {
        length: u8,
    },
    JokerBomb,
    /// 三带二：3 张同点主牌 + 1 对，固定 5 张，无参（§5.13 第 930 行）。
    TriplePair,
    // —— 比较 ——
    ByRank {
        direction: OrderDirection,
    },
    ByCount {
        direction: OrderDirection,
    },
    ByLadder,
    BombOrderParams,
    // —— 行动 ——
    RequireLegal {
        force: bool,
        consume: ZoneId,
    },
    ConsumeCards {
        zone: ZoneId,
        count: u8,
    },
    Pass,
    // —— 轮转 ——
    TurnNext {
        direction: TurnDirection,
        skip_finished: bool,
    },
    AfterAllPassParams {
        to: AfterAllPass,
    },
    SkipFinished {
        skip: bool,
        partner_lead: bool,
    },
    ReturnToLast,
    // —— 进贡 ——
    Determine {
        trigger: TributeTrigger,
        first_round: FirstRound,
    },
    PayParams {
        count: u8,
        selection: PaySelection,
    },
    ReturnParams {
        ceiling: ReturnCeiling,
        count: u8,
        fallback: ReturnFallback,
    },
    ResistParams,
    // —— 级牌 ——
    LevelCurrent {
        scope: LevelScope,
    },
    LevelUpgrade,
    LevelPass {
        combos: Option<Vec<Vec<u8>>>,
        streak_limit: u8,
        reset_on_fail: bool,
    },
    // —— 计分 ——
    Accumulate {
        dimension: ScoreDimension,
    },
    LevelUpParams,
    // —— 视图 / 随机 / 断言 / 发牌 ——
    ViewProject,
    RngPick {
        stream: u32,
    },
    MinPlayers {
        count: u8,
    },
    CardCount {
        expected: u32,
    },
    ZoneCapacityParams {
        zone: ZoneId,
    },
    Shuffle {
        zone: ZoneId,
        per_seat: Option<u16>,
        reveal: Option<bool>,
    },
    /// v2 预留 / v1 未启用 / 未知原语：L1 拒绝（§5.2 第 547–549 行）。
    Reserved {
        primitive_id: String,
    },
}

// —— 各原始参数结构体（deny_unknown_fields，默认值用 serde default） ——

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SameRankRaw {
    count: u8,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StraightRaw {
    length: u8,
    #[serde(rename = "wildcardAllowed", default = "default_true")]
    wildcard_allowed: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ConsecutivePairsRaw {
    pairs: u8,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ConsecutiveTriplesRaw {
    triples: u8,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BombRaw {
    count: u8,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StraightFlushRaw {
    length: u8,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EmptyRaw {}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DirectionRaw {
    #[serde(default = "default_asc")]
    direction: OrderDirection,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RequireLegalRaw {
    #[serde(default = "default_true")]
    force: bool,
    #[serde(default = "default_hand_zone")]
    consume: ZoneId,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ConsumeCardsRaw {
    zone: ZoneId,
    #[serde(default = "default_zero_u8")]
    count: u8,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TurnNextRaw {
    #[serde(default = "default_clockwise")]
    direction: TurnDirection,
    #[serde(rename = "skipFinished", default = "default_true")]
    skip_finished: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AfterAllPassRaw {
    #[serde(default = "default_last_player")]
    to: AfterAllPass,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SkipFinishedRaw {
    #[serde(default = "default_true")]
    skip: bool,
    #[serde(rename = "partnerLead", default = "default_true")]
    partner_lead: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DetermineRaw {
    #[serde(default = "default_last_round_rank")]
    trigger: TributeTrigger,
    #[serde(rename = "firstRound", default = "default_skip")]
    first_round: FirstRound,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PayRaw {
    #[serde(default = "default_one_u8")]
    count: u8,
    #[serde(default = "default_max_except_wildcard")]
    selection: PaySelection,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReturnRaw {
    #[serde(default = "default_under_ten")]
    ceiling: ReturnCeiling,
    #[serde(default = "default_one_u8")]
    count: u8,
    #[serde(default = "default_smallest")]
    fallback: ReturnFallback,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LevelCurrentRaw {
    #[serde(default = "default_by_team")]
    scope: LevelScope,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LevelPassRaw {
    // 缺省 combos 只在 teamSize = 2 时有定义；dispatch 拿不到 seats 上下文，
    // 默认值是否成立由 L1 顶层 `level.pass.combos` 闸门判定（§5.1 `level` 行）。
    combos: Option<Vec<Vec<u8>>>,
    #[serde(rename = "streakLimit", default = "default_one_u8")]
    streak_limit: u8,
    #[serde(rename = "resetOnFail", default = "default_true")]
    reset_on_fail: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AccumulateRaw {
    #[serde(default = "default_team")]
    dimension: ScoreDimension,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RngPickRaw {
    #[serde(default = "default_zero_u32")]
    stream: u32,
}
fn default_zero_u32() -> u32 {
    0
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MinPlayersRaw {
    count: u8,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CardCountRaw {
    expected: u32,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ZoneCapacityRaw {
    zone: ZoneId,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ShuffleRaw {
    zone: ZoneId,
    #[serde(rename = "perSeat")]
    per_seat: Option<u16>,
    reveal: Option<bool>,
}

/// 无参原语：必须显式写空对象 `{}`，缺 `params` 键即拒绝（§5.1 第 392 行、§5.2 参数表）。
const NO_PARAM_PRIMITIVES: [&str; 10] = [
    "pattern.joker-bomb",
    "pattern.triple-pair",
    "action.pass",
    "compare.by-ladder",
    "compare.bomb-order",
    "turn.return-to-last",
    "tribute.resist",
    "level.upgrade",
    "score.level-up",
    "view.project",
];

/// 判断原语是否为无参原语（缺 `params` 键应拒绝）。
pub fn is_no_param_primitive(primitive_id: &str) -> bool {
    NO_PARAM_PRIMITIVES.contains(&primitive_id)
}

/// v2 预留与 v1 未启用原语（§5.2 第 547–549 行）：引用即 L1 拒绝。
const RESERVED_PRIMITIVES: [&str; 8] = [
    "pattern.with-attachment",
    "pattern.all-same-suit",
    "action.claim",
    "action.declare",
    "score.table",
    "deal.draw",
    "deal.reveal",
    "rng.range",
];

fn parse_raw<T: serde::de::DeserializeOwned>(
    params: &serde_yaml::Mapping,
    location: &str,
    primitive_id: &str,
) -> Result<T, SchemaError> {
    serde_yaml::from_value::<T>(serde_yaml::Value::Mapping(params.clone())).map_err(|e| {
        SchemaError::schema_invalid(format!("{location}.{primitive_id}"), e.to_string())
    })
}

/// 按 `primitiveId` 把局部 `params` 映射成分型参数结构体。
///
/// `location` 用于 Finding 字段路径（如 `patterns[0].params`）。
pub fn dispatch(
    primitive_id: &str,
    params: &serde_yaml::Mapping,
    location: &str,
) -> Result<PrimitiveParams, SchemaError> {
    if RESERVED_PRIMITIVES.contains(&primitive_id) {
        return Ok(PrimitiveParams::Reserved {
            primitive_id: primitive_id.to_owned(),
        });
    }

    let params = match primitive_id {
        "pattern.same-rank" => {
            let r: SameRankRaw = parse_raw(params, location, primitive_id)?;
            PrimitiveParams::SameRank { count: r.count }
        }
        "pattern.straight" => {
            let r: StraightRaw = parse_raw(params, location, primitive_id)?;
            PrimitiveParams::Straight {
                length: r.length,
                wildcard_allowed: r.wildcard_allowed,
            }
        }
        "pattern.consecutive-pairs" => {
            let r: ConsecutivePairsRaw = parse_raw(params, location, primitive_id)?;
            PrimitiveParams::ConsecutivePairs { pairs: r.pairs }
        }
        "pattern.consecutive-triples" => {
            let r: ConsecutiveTriplesRaw = parse_raw(params, location, primitive_id)?;
            PrimitiveParams::ConsecutiveTriples { triples: r.triples }
        }
        "pattern.bomb" => {
            let r: BombRaw = parse_raw(params, location, primitive_id)?;
            PrimitiveParams::Bomb { count: r.count }
        }
        "pattern.straight-flush" => {
            let r: StraightFlushRaw = parse_raw(params, location, primitive_id)?;
            PrimitiveParams::StraightFlush { length: r.length }
        }
        "pattern.joker-bomb" => {
            let _: EmptyRaw = parse_raw(params, location, primitive_id)?;
            PrimitiveParams::JokerBomb
        }
        "pattern.triple-pair" => {
            let _: EmptyRaw = parse_raw(params, location, primitive_id)?;
            PrimitiveParams::TriplePair
        }
        "compare.by-rank" => {
            let r: DirectionRaw = parse_raw(params, location, primitive_id)?;
            PrimitiveParams::ByRank {
                direction: r.direction,
            }
        }
        "compare.by-count" => {
            let r: DirectionRaw = parse_raw(params, location, primitive_id)?;
            PrimitiveParams::ByCount {
                direction: r.direction,
            }
        }
        "compare.by-ladder" => {
            let _: EmptyRaw = parse_raw(params, location, primitive_id)?;
            PrimitiveParams::ByLadder
        }
        "compare.bomb-order" => {
            let _: EmptyRaw = parse_raw(params, location, primitive_id)?;
            PrimitiveParams::BombOrderParams
        }
        "action.require-legal" => {
            let r: RequireLegalRaw = parse_raw(params, location, primitive_id)?;
            PrimitiveParams::RequireLegal {
                force: r.force,
                consume: r.consume,
            }
        }
        "action.consume-cards" => {
            let r: ConsumeCardsRaw = parse_raw(params, location, primitive_id)?;
            PrimitiveParams::ConsumeCards {
                zone: r.zone,
                count: r.count,
            }
        }
        "action.pass" => {
            let _: EmptyRaw = parse_raw(params, location, primitive_id)?;
            PrimitiveParams::Pass
        }
        "turn.next" => {
            let r: TurnNextRaw = parse_raw(params, location, primitive_id)?;
            PrimitiveParams::TurnNext {
                direction: r.direction,
                skip_finished: r.skip_finished,
            }
        }
        "turn.after-all-pass" => {
            let r: AfterAllPassRaw = parse_raw(params, location, primitive_id)?;
            PrimitiveParams::AfterAllPassParams { to: r.to }
        }
        "turn.skip-finished" => {
            let r: SkipFinishedRaw = parse_raw(params, location, primitive_id)?;
            PrimitiveParams::SkipFinished {
                skip: r.skip,
                partner_lead: r.partner_lead,
            }
        }
        "turn.return-to-last" => {
            let _: EmptyRaw = parse_raw(params, location, primitive_id)?;
            PrimitiveParams::ReturnToLast
        }
        "tribute.determine" => {
            let r: DetermineRaw = parse_raw(params, location, primitive_id)?;
            PrimitiveParams::Determine {
                trigger: r.trigger,
                first_round: r.first_round,
            }
        }
        "tribute.pay" => {
            let r: PayRaw = parse_raw(params, location, primitive_id)?;
            PrimitiveParams::PayParams {
                count: r.count,
                selection: r.selection,
            }
        }
        "tribute.return" => {
            let r: ReturnRaw = parse_raw(params, location, primitive_id)?;
            PrimitiveParams::ReturnParams {
                ceiling: r.ceiling,
                count: r.count,
                fallback: r.fallback,
            }
        }
        "tribute.resist" => {
            let _: EmptyRaw = parse_raw(params, location, primitive_id)?;
            PrimitiveParams::ResistParams
        }
        "level.current" => {
            let r: LevelCurrentRaw = parse_raw(params, location, primitive_id)?;
            PrimitiveParams::LevelCurrent { scope: r.scope }
        }
        "level.upgrade" => {
            let _: EmptyRaw = parse_raw(params, location, primitive_id)?;
            PrimitiveParams::LevelUpgrade
        }
        "level.pass" => {
            let r: LevelPassRaw = parse_raw(params, location, primitive_id)?;
            PrimitiveParams::LevelPass {
                combos: r.combos,
                streak_limit: r.streak_limit,
                reset_on_fail: r.reset_on_fail,
            }
        }
        "score.accumulate" => {
            let r: AccumulateRaw = parse_raw(params, location, primitive_id)?;
            PrimitiveParams::Accumulate {
                dimension: r.dimension,
            }
        }
        "score.level-up" => {
            let _: EmptyRaw = parse_raw(params, location, primitive_id)?;
            PrimitiveParams::LevelUpParams
        }
        "view.project" => {
            let _: EmptyRaw = parse_raw(params, location, primitive_id)?;
            PrimitiveParams::ViewProject
        }
        "rng.pick" => {
            let r: RngPickRaw = parse_raw(params, location, primitive_id)?;
            PrimitiveParams::RngPick { stream: r.stream }
        }
        "assert.min-players" => {
            let r: MinPlayersRaw = parse_raw(params, location, primitive_id)?;
            PrimitiveParams::MinPlayers { count: r.count }
        }
        "assert.card-count" => {
            let r: CardCountRaw = parse_raw(params, location, primitive_id)?;
            PrimitiveParams::CardCount {
                expected: r.expected,
            }
        }
        "assert.zone-capacity" => {
            let r: ZoneCapacityRaw = parse_raw(params, location, primitive_id)?;
            PrimitiveParams::ZoneCapacityParams { zone: r.zone }
        }
        "deal.shuffle" => {
            let r: ShuffleRaw = parse_raw(params, location, primitive_id)?;
            PrimitiveParams::Shuffle {
                zone: r.zone,
                per_seat: r.per_seat,
                reveal: r.reveal,
            }
        }
        other => {
            return Ok(PrimitiveParams::Reserved {
                primitive_id: other.to_owned(),
            });
        }
    };
    Ok(params)
}
