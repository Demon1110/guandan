//! `RuleSetBody` 全量类型（§5.1 顶层结构、§5.2 原语参数、§5.3 类型约束）。
//!
//! 红线（§5.3 第 558–563 行）：
//! - 所有结构体开 `deny_unknown_fields`；
//! - 交付类型里不出现 `serde_json::Value` / `serde_yaml::Value` 字段，不用 `#[serde(flatten)]`；
//! - 闭集枚举用 Rust enum；引用型 id 用 newtype。

use crate::contract::{PatternId, ZoneId};
use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer};

// ---------- 基础闭集枚举 ----------

/// 花色闭集 `S/H/D/C`（§5.1 `deck.suits`）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub enum Suit {
    #[serde(rename = "S")]
    S,
    #[serde(rename = "H")]
    H,
    #[serde(rename = "D")]
    D,
    #[serde(rename = "C")]
    C,
}

/// 级牌插入位（§5.1 `levelInsert`）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub enum LevelInsert {
    #[serde(rename = "above-A")]
    AboveA,
    #[serde(rename = "below-2")]
    Below2,
}

/// 炸弹之间的排序策略（§5.1 `compare.bombOrder`）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub enum BombOrder {
    #[serde(rename = "count-then-rank")]
    CountThenRank,
    #[serde(rename = "rank-then-count")]
    RankThenCount,
}

/// 同花顺与炸弹的序（§5.1 `compare.flushRank`，四值闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub enum FlushRank {
    #[serde(rename = "below-bomb-5")]
    BelowBomb5,
    #[serde(rename = "above-bomb-5")]
    AboveBomb5,
    #[serde(rename = "above-bomb-6")]
    AboveBomb6,
    #[serde(rename = "above-bomb-8")]
    AboveBomb8,
}

/// 生命周期四段（§5.1 `lifecycle`、§5.14）；`turn` 不是 phase。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub enum Phase {
    #[serde(rename = "deal")]
    Deal,
    #[serde(rename = "tribute")]
    Tribute,
    #[serde(rename = "play")]
    Play,
    #[serde(rename = "settle")]
    Settle,
}

/// 动作 kind 闭集：v1 仅 `play`/`pass`（§5.1 `actions.kind`）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub enum ActionKind {
    #[serde(rename = "play")]
    Play,
    #[serde(rename = "pass")]
    Pass,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub enum TurnDirection {
    #[serde(rename = "clockwise")]
    Clockwise,
    #[serde(rename = "counterclockwise")]
    Counterclockwise,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub enum AfterAllPass {
    #[serde(rename = "last-player")]
    LastPlayer,
    #[serde(rename = "next-player")]
    NextPlayer,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub enum ScoreDimension {
    #[serde(rename = "team")]
    Team,
    #[serde(rename = "player")]
    Player,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub enum LevelScope {
    #[serde(rename = "by-team")]
    ByTeam,
    #[serde(rename = "global")]
    Global,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub enum TributeTrigger {
    #[serde(rename = "last-round-rank")]
    LastRoundRank,
}

/// 升级判定 basis（§5.1 `levelTable.upgradeBasis`）：默认 `rank-combo`。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub enum UpgradeBasis {
    #[serde(rename = "rank-combo")]
    RankCombo,
    #[serde(rename = "score-diff")]
    ScoreDiff,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub enum TributeWhen {
    #[serde(rename = "single-loss")]
    SingleLoss,
    #[serde(rename = "double-loss")]
    DoubleLoss,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub enum ReturnCeiling {
    #[serde(rename = "under-ten")]
    UnderTen,
    #[serde(rename = "none")]
    None,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub enum ReturnFallback {
    #[serde(rename = "smallest")]
    Smallest,
    #[serde(rename = "largest")]
    Largest,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub enum TributeResist {
    #[serde(rename = "two-big-jokers")]
    TwoBigJokers,
    #[serde(rename = "none")]
    None,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub enum PaySelection {
    #[serde(rename = "max-rank-except-wildcard")]
    MaxRankExceptWildcard,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub enum FirstRound {
    #[serde(rename = "skip")]
    Skip,
    #[serde(rename = "play")]
    Play,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub enum OrderDirection {
    #[serde(rename = "asc")]
    Asc,
    #[serde(rename = "desc")]
    Desc,
}

/// `ai.features` 合法取值（§5.11 第 772–781 行，六个字符串）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub enum AiFeature {
    #[serde(rename = "hand.count")]
    HandCount,
    #[serde(rename = "last.play")]
    LastPlay,
    #[serde(rename = "partner.state")]
    PartnerState,
    #[serde(rename = "opponent.count")]
    OpponentCount,
    #[serde(rename = "table.history")]
    TableHistory,
    #[serde(rename = "bomb.count")]
    BombCount,
}

// ---------- RankKey：必须按字符串字面值反序列化 ----------

const RANK_KEYS: [&str; 15] = [
    "2",
    "3",
    "4",
    "5",
    "6",
    "7",
    "8",
    "9",
    "10",
    "J",
    "Q",
    "K",
    "A",
    "little-joker",
    "big-joker",
];

/// 点数键（§5.1 第 486 行）：只接受字符串字面值；YAML 里未加引号的数字 `2` 必须拒绝。
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RankKey(pub String);

impl RankKey {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn is_joker(&self) -> bool {
        self.0 == "little-joker" || self.0 == "big-joker"
    }
}

impl<'de> Deserialize<'de> for RankKey {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct RankKeyVisitor;
        impl<'de> Visitor<'de> for RankKeyVisitor {
            type Value = RankKey;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(
                    "a RankKey string literal (\"2\"..\"10\", J/Q/K/A, little-joker/big-joker)",
                )
            }
            fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
                if RANK_KEYS.contains(&value) {
                    Ok(RankKey(value.to_owned()))
                } else {
                    Err(E::custom(format!("unknown RankKey literal `{value}`")))
                }
            }
            fn visit_u64<E: de::Error>(self, _v: u64) -> Result<Self::Value, E> {
                Err(E::custom(
                    "RankKey 必须是字符串字面值，数字类型被拒绝（§5.1 第 486 行）",
                ))
            }
        }
        deserializer.deserialize_str(RankKeyVisitor)
    }
}

/// 逢人配源点数：具体 RankKey 或受控占位符 `@level`（§5.1 `wildcard.source.rank`）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WildcardRank {
    Rank(RankKey),
    LevelPlaceholder,
}

impl<'de> Deserialize<'de> for WildcardRank {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        if raw == "@level" {
            Ok(WildcardRank::LevelPlaceholder)
        } else if RANK_KEYS.contains(&raw.as_str()) {
            Ok(WildcardRank::Rank(RankKey(raw)))
        } else {
            Err(de::Error::custom(format!("unknown wildcard rank `{raw}`")))
        }
    }
}

// ---------- newtype 反序列化（ZoneId / PatternId） ----------

impl<'de> Deserialize<'de> for ZoneId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        String::deserialize(deserializer).map(ZoneId)
    }
}

impl<'de> Deserialize<'de> for PatternId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        String::deserialize(deserializer).map(PatternId)
    }
}

// ---------- 顶层结构体 ----------

/// `seats`：座位数与队伍划分（§5.1）。
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Seats {
    pub count: u8,
    pub teams: Vec<Vec<u8>>,
}

/// `deck`：牌堆构成（§5.1）。
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Deck {
    pub copies: u8,
    pub ranks: Vec<RankKey>,
    pub suits: Vec<Suit>,
    pub jokers: u8,
    #[serde(default)]
    pub exclude: Vec<RankKey>,
}

/// `zones[]`：区域定义（§5.1）。`capacity` 是上限，`-1` 表无上限。
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ZoneDef {
    pub id: ZoneId,
    pub scope: ZoneScope,
    pub capacity: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub enum ZoneScope {
    #[serde(rename = "private")]
    Private,
    #[serde(rename = "shared")]
    Shared,
}

/// `levelTable.upgrade[]` 单行（§5.1）。`teamRanks` 长度随 `teamSize` 参数化，L1 校验。
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpgradeRow {
    #[serde(rename = "teamRanks")]
    pub team_ranks: Vec<u8>,
    pub step: u8,
}

/// `levelTable.upgradeByDiff[]` 单行（§5.1 `score-diff` basis：按两队分值差升级）。
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpgradeDiffRow {
    #[serde(rename = "minDiff")]
    pub min_diff: u16,
    pub step: u8,
}

fn default_upgrade_basis() -> UpgradeBasis {
    UpgradeBasis::RankCombo
}

/// 级牌表本体（§5.1 `levelTable`）。两个 basis 的键集互斥，L1 校验（§5.4 L1）。
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LevelTable {
    pub id: String,
    pub initial: RankKey,
    pub ranks: Vec<RankKey>,
    #[serde(rename = "upgradeBasis", default = "default_upgrade_basis")]
    pub upgrade_basis: UpgradeBasis,
    pub upgrade: Option<Vec<UpgradeRow>>,
    #[serde(rename = "rankScores")]
    pub rank_scores: Option<Vec<u16>>,
    #[serde(rename = "upgradeByDiff")]
    pub upgrade_by_diff: Option<Vec<UpgradeDiffRow>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PassConfig {
    /// 名次组合，长度随 `teamSize` 参数化；缺省仅在 `teamSize = 2` 时有定义（§5.1 `level` 行）。
    pub combos: Option<Vec<Vec<u8>>>,
    #[serde(rename = "streakLimit")]
    pub streak_limit: u8,
    #[serde(rename = "resetOnFail")]
    pub reset_on_fail: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LevelCurrentConfig {
    pub scope: LevelScope,
}

/// `level`：过 A 与级牌取值入口（§5.1）。
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LevelConfig {
    pub pass: PassConfig,
    pub current: LevelCurrentConfig,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WildcardSource {
    pub suit: Suit,
    pub rank: WildcardRank,
}

/// `wildcard`：逢人配（§5.1）。
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Wildcard {
    pub source: WildcardSource,
    pub substitute: Vec<RankKey>,
    #[serde(rename = "inStraightFlush")]
    pub in_straight_flush: bool,
}

/// `patterns[]` 定义；`params` 按 `primitiveId` 分型（见 [`crate::ruleset::params`]）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PatternDef {
    pub id: PatternId,
    pub primitive_id: String,
    pub params: crate::ruleset::params::PrimitiveParams,
}

fn build_pattern<'de, D>(value: &serde_yaml::Value, location: &str) -> Result<PatternDef, D::Error>
where
    D: Deserializer<'de>,
{
    let map = value
        .as_mapping()
        .ok_or_else(|| de::Error::custom(format!("{location}: pattern 必须是 mapping")))?;
    for key in map.keys() {
        let Some(k) = key.as_str() else { continue };
        if !["id", "primitiveId", "params"].contains(&k) {
            return Err(de::Error::unknown_field(
                k,
                &["id", "primitiveId", "params"],
            ));
        }
    }
    let id_value = map
        .get("id")
        .ok_or_else(|| de::Error::missing_field("id"))?
        .clone();
    let id: PatternId = serde_yaml::from_value(id_value).map_err(de::Error::custom)?;
    let primitive_id = map
        .get("primitiveId")
        .ok_or_else(|| de::Error::missing_field("primitiveId"))?
        .as_str()
        .ok_or_else(|| de::Error::custom("primitiveId 必须是字符串"))?
        .to_owned();

    let params_location = format!("{location}.params");
    let params_map = match map.get("params") {
        Some(params_value) => params_value
            .as_mapping()
            .ok_or_else(|| de::Error::custom(format!("{params_location} 必须是 mapping")))?
            .clone(),
        None => {
            // §5.1 第 392 行：无参原语也必须写空对象 `{}`；有参原语缺省取默认值。
            if crate::ruleset::params::is_no_param_primitive(&primitive_id) {
                return Err(de::Error::custom(format!(
                    "{params_location}: 无参原语 `{primitive_id}` 必须显式写空对象 `{{}}`（§5.1 第 392 行）"
                )));
            }
            serde_yaml::Mapping::default()
        }
    };
    let params = crate::ruleset::params::dispatch(&primitive_id, &params_map, &params_location)
        .map_err(|e| de::Error::custom(e.message))?;
    Ok(PatternDef {
        id,
        primitive_id,
        params,
    })
}

/// 逐元素反序列化 `patterns[]`，location 带下标 `patterns[{i}]`（§5.3 索引引用）。
fn deserialize_patterns<'de, D>(deserializer: D) -> Result<Vec<PatternDef>, D::Error>
where
    D: Deserializer<'de>,
{
    let values: Vec<serde_yaml::Value> = Deserialize::deserialize(deserializer)?;
    values
        .iter()
        .enumerate()
        .map(|(i, value)| build_pattern::<D>(value, &format!("patterns[{i}]")))
        .collect()
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompareConfig {
    pub ladder: Vec<PatternId>,
    #[serde(rename = "bombOrder")]
    pub bomb_order: BombOrder,
    #[serde(rename = "flushRank")]
    pub flush_rank: FlushRank,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TributePair {
    /// 名次数值 `1..=seats.count`（`1` 即头游，§5.1 `tribute` 行），L1 校验值域。
    pub payer: u8,
    pub receiver: u8,
    pub when: TributeWhen,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TributeReturn {
    pub ceiling: ReturnCeiling,
    pub count: u8,
    pub fallback: ReturnFallback,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TributePay {
    pub count: u8,
    pub selection: PaySelection,
}

/// `tribute` 段（§5.1）。缺段由 L1 按必填拒绝（location `tribute`）；
/// `enabled=false` 时其余子键可整段省略，`enabled=true` 时 `trigger`/`pairs`/`return`/`resist` 必填。
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TributeConfig {
    pub enabled: bool,
    pub trigger: Option<TributeTrigger>,
    pub pairs: Option<Vec<TributePair>>,
    #[serde(rename = "return")]
    pub return_: Option<TributeReturn>,
    pub resist: Option<TributeResist>,
    pub pay: Option<TributePay>,
    #[serde(rename = "firstRound")]
    pub first_round: Option<FirstRound>,
}

/// `actions[]`：玩家可提交的动作（§5.1）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActionDef {
    pub kind: ActionKind,
    pub primitive: String,
    pub legal_from: LegalFrom,
    pub params: crate::ruleset::params::PrimitiveParams,
}

fn build_action<'de, D>(value: &serde_yaml::Value, location: &str) -> Result<ActionDef, D::Error>
where
    D: Deserializer<'de>,
{
    let map = value
        .as_mapping()
        .ok_or_else(|| de::Error::custom(format!("{location}: action 必须是 mapping")))?;
    for key in map.keys() {
        let Some(k) = key.as_str() else { continue };
        if !["kind", "primitive", "legalFrom", "params"].contains(&k) {
            return Err(de::Error::unknown_field(
                k,
                &["kind", "primitive", "legalFrom", "params"],
            ));
        }
    }
    let kind_value = map
        .get("kind")
        .ok_or_else(|| de::Error::missing_field("kind"))?
        .clone();
    let kind: ActionKind = serde_yaml::from_value(kind_value).map_err(de::Error::custom)?;
    let primitive = map
        .get("primitive")
        .ok_or_else(|| de::Error::missing_field("primitive"))?
        .as_str()
        .ok_or_else(|| de::Error::custom("primitive 必须是字符串"))?
        .to_owned();
    let legal_from_value = map
        .get("legalFrom")
        .ok_or_else(|| de::Error::missing_field("legalFrom"))?
        .clone();
    let legal_from: LegalFrom =
        serde_yaml::from_value(legal_from_value).map_err(de::Error::custom)?;

    let params_location = format!("{location}.params");
    let params_map = match map.get("params") {
        Some(params_value) => params_value
            .as_mapping()
            .ok_or_else(|| de::Error::custom(format!("{params_location} 必须是 mapping")))?
            .clone(),
        None => {
            if crate::ruleset::params::is_no_param_primitive(&primitive) {
                return Err(de::Error::custom(format!(
                    "{params_location}: 无参原语 `{primitive}` 必须显式写空对象 `{{}}`（§5.1 第 392 行）"
                )));
            }
            serde_yaml::Mapping::default()
        }
    };
    let params = crate::ruleset::params::dispatch(&primitive, &params_map, &params_location)
        .map_err(|e| de::Error::custom(e.message))?;
    Ok(ActionDef {
        kind,
        primitive,
        legal_from,
        params,
    })
}

/// 逐元素反序列化 `actions[]`，location 带下标 `actions[{i}]`。
fn deserialize_actions<'de, D>(deserializer: D) -> Result<Vec<ActionDef>, D::Error>
where
    D: Deserializer<'de>,
{
    let values: Vec<serde_yaml::Value> = Deserialize::deserialize(deserializer)?;
    values
        .iter()
        .enumerate()
        .map(|(i, value)| build_action::<D>(value, &format!("actions[{i}]")))
        .collect()
}

/// `legalFrom` 三取值：`patterns`、某个 zone id、某个 phase 名（§5.1）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LegalFrom {
    Patterns,
    Zone(ZoneId),
    Phase(Phase),
}

impl<'de> Deserialize<'de> for LegalFrom {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        match raw.as_str() {
            "patterns" => Ok(LegalFrom::Patterns),
            "deal" => Ok(LegalFrom::Phase(Phase::Deal)),
            "tribute" => Ok(LegalFrom::Phase(Phase::Tribute)),
            "play" => Ok(LegalFrom::Phase(Phase::Play)),
            "settle" => Ok(LegalFrom::Phase(Phase::Settle)),
            other => Ok(LegalFrom::Zone(ZoneId(other.to_owned()))),
        }
    }
}

/// 结算变体（§5.1 `settle`）：`level-up` 引用级牌表；`score-only` 无等级表引用。
///
/// 显式反序列化：按变体列允许键集，多余键一律拒绝（内部标签枚举会静默吃掉未知键，
/// 违反「未知键 RS_SCHEMA_INVALID」红线，§5.3 第 561 行）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Settle {
    LevelUp { level_table_ref: String },
    ScoreOnly,
}

impl<'de> Deserialize<'de> for Settle {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = serde_yaml::Value::deserialize(deserializer)?;
        let map = value
            .as_mapping()
            .ok_or_else(|| de::Error::custom("settle 必须是 mapping"))?;
        let algorithm = map
            .get("algorithm")
            .and_then(|v| v.as_str())
            .ok_or_else(|| de::Error::missing_field("algorithm"))?;
        match algorithm {
            "level-up" => {
                for key in map.keys() {
                    let Some(k) = key.as_str() else { continue };
                    if !["algorithm", "levelTableRef"].contains(&k) {
                        return Err(de::Error::unknown_field(k, &["algorithm", "levelTableRef"]));
                    }
                }
                let level_table_ref = map
                    .get("levelTableRef")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| de::Error::missing_field("levelTableRef"))?
                    .to_owned();
                Ok(Settle::LevelUp { level_table_ref })
            }
            "score-only" => {
                for key in map.keys() {
                    let Some(k) = key.as_str() else { continue };
                    if k != "algorithm" {
                        return Err(de::Error::unknown_field(k, &["algorithm"]));
                    }
                }
                Ok(Settle::ScoreOnly)
            }
            other => Err(de::Error::custom(format!(
                "unknown settle algorithm `{other}`"
            ))),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MatchCap {
    pub rounds: u16,
}

/// `view.fields` 五类写法（§5.1）：`seat.levels`、区号、`lastPlay`、
/// `round.level`、`round.attackingTeam`（第二十二次变更）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ViewField {
    SeatLevels,
    Zone(ZoneId),
    LastPlay,
    /// `round.level`：本局局级牌档位下标（= `levels[attackingTeam]`）。
    RoundLevel,
    /// `round.attackingTeam`：本局打级队队序号。
    RoundAttackingTeam,
}

impl<'de> Deserialize<'de> for ViewField {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        match raw.as_str() {
            "seat.levels" => Ok(ViewField::SeatLevels),
            "lastPlay" => Ok(ViewField::LastPlay),
            "round.level" => Ok(ViewField::RoundLevel),
            "round.attackingTeam" => Ok(ViewField::RoundAttackingTeam),
            other => Ok(ViewField::Zone(ZoneId(other.to_owned()))),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ViewConfig {
    #[serde(rename = "schemaVersion")]
    pub schema_version: u16,
    pub fields: Vec<ViewField>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Timing {
    #[serde(rename = "turnTimeSec")]
    pub turn_time_sec: u16,
    #[serde(rename = "graceSec")]
    pub grace_sec: u16,
    #[serde(rename = "offlineAllSec")]
    pub offline_all_sec: u16,
}

/// `ai.recommend.weights`（§5.11 第 848–851 行，刻度 10，整数 `i16`，禁浮点）。
///
/// 表内数字是「基准分 ×10」，引擎按原值直接比较、不再换算；自定义权重时必须显式
/// 给全所有键（第二十八次变更 ⑤），不允许只写一半、其余隐式按默认。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecommendWeights {
    pub bomb4: i16,
    pub bomb5: i16,
    pub bomb6: i16,
    pub bomb7: i16,
    pub bomb8: i16,
    #[serde(rename = "jokerBomb")]
    pub joker_bomb: i16,
    #[serde(rename = "straightFlush")]
    pub straight_flush: i16,
    #[serde(rename = "straightBase")]
    pub straight_base: i16,
    #[serde(rename = "straightRankBonus")]
    pub straight_rank_bonus: i16,
    #[serde(rename = "triplePairBase")]
    pub triple_pair_base: i16,
    #[serde(rename = "triplePairRankBonus")]
    pub triple_pair_rank_bonus: i16,
    #[serde(rename = "plate3Base")]
    pub plate3_base: i16,
    #[serde(rename = "plate3RankBonus")]
    pub plate3_rank_bonus: i16,
    #[serde(rename = "steel2Base")]
    pub steel2_base: i16,
    #[serde(rename = "steel2RankBonus")]
    pub steel2_rank_bonus: i16,
    pub pair: i16,
    pub single: i16,
}

/// 默认权重表（§5.11 第 848–851 行，刻度 10；`weights` 省略即全上此表）。
///
/// `straightFlush = 55` 严格落在 `bomb5 = 50` 与 `bomb6 = 60` 之间（第二十八次
/// 变更 ③），这是刻意取舍，不得改动。
impl Default for RecommendWeights {
    fn default() -> Self {
        Self {
            bomb4: 40,
            bomb5: 50,
            bomb6: 60,
            bomb7: 70,
            bomb8: 80,
            joker_bomb: 90,
            straight_flush: 55,
            straight_base: 40,
            straight_rank_bonus: 20,
            triple_pair_base: 30,
            triple_pair_rank_bonus: 20,
            plate3_base: 30,
            plate3_rank_bonus: 10,
            steel2_base: 40,
            steel2_rank_bonus: 10,
            pair: 0,
            single: -10,
        }
    }
}

/// `ai.recommend.mode`（§5.11 第三十七次变更）：`least-hands` 先比手数、
/// `weighted` 先比分数；省略 = `weighted`。未知取值在反序列化期拒绝。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Default)]
pub enum RecommendMode {
    #[serde(rename = "least-hands")]
    LeastHands,
    #[serde(rename = "weighted")]
    #[default]
    Weighted,
}

/// `ai.recommend`：组牌推荐开关、模式与权重（§5.11「组牌推荐」段）。
///
/// `enabled` 默认 `true`；`enabled=false` 时 `weights`/`mode` 可整段省略。`weights`
/// 省略 = 默认权重全上；`mode` 省略 = `weighted`。
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecommendConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub mode: RecommendMode,
    pub weights: Option<RecommendWeights>,
}

fn default_true() -> bool {
    true
}

/// `ai`：AI 特征、提示开关与组牌推荐（§5.1 / §5.11）。
///
/// `hint` 默认 `false`：只表示「该规则集允许开提示」，客户端自行调 `recommend`，
/// 服务端不主动下发、不加帧不加字段（第二十九次变更）。
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AiConfig {
    #[serde(default)]
    pub features: Vec<AiFeature>,
    #[serde(default)]
    pub hint: bool,
    pub recommend: Option<RecommendConfig>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DealFlow {
    #[serde(rename = "perSeat")]
    pub per_seat: Option<u16>,
    pub zone: Option<ZoneId>,
    pub reveal: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TurnFlow {
    pub direction: Option<TurnDirection>,
    #[serde(rename = "skipFinished")]
    pub skip_finished: Option<bool>,
    #[serde(rename = "partnerLead")]
    pub partner_lead: Option<bool>,
    #[serde(rename = "afterAllPass")]
    pub after_all_pass: Option<AfterAllPass>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScoreFlow {
    pub dimension: Option<ScoreDimension>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RngFlow {
    pub stream: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssertFlow {
    #[serde(rename = "minPlayers")]
    pub min_players: Option<u8>,
    #[serde(rename = "cardCount")]
    pub card_count: Option<u32>,
    #[serde(rename = "zoneCapacity")]
    pub zone_capacity: Option<Vec<ZoneId>>,
}

/// `flow`：运行期流程参数归集（§5.1 第 400–415 行）。
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FlowConfig {
    pub deal: Option<DealFlow>,
    pub turn: Option<TurnFlow>,
    pub score: Option<ScoreFlow>,
    pub rng: Option<RngFlow>,
    pub assert: Option<AssertFlow>,
}

/// RuleSet 草稿态本体（不含发布态注入的 `version` 键；§5.1 第 378 行）。
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuleSetBody {
    #[serde(rename = "rulesetId")]
    pub ruleset_id: String,
    pub interpreter: String,
    pub seats: Seats,
    pub deck: Deck,
    pub zones: Vec<ZoneDef>,
    #[serde(rename = "levelTable")]
    pub level_table: Option<LevelTable>,
    pub level: Option<LevelConfig>,
    #[serde(rename = "naturalOrder")]
    pub natural_order: Vec<RankKey>,
    #[serde(rename = "levelInsert")]
    pub level_insert: LevelInsert,
    pub wildcard: Option<Wildcard>,
    #[serde(deserialize_with = "deserialize_patterns")]
    pub patterns: Vec<PatternDef>,
    pub compare: CompareConfig,
    pub tribute: Option<TributeConfig>,
    pub lifecycle: Vec<Phase>,
    #[serde(deserialize_with = "deserialize_actions")]
    pub actions: Vec<ActionDef>,
    pub settle: Settle,
    #[serde(rename = "matchCap")]
    pub match_cap: Option<MatchCap>,
    pub view: ViewConfig,
    pub timing: Option<Timing>,
    pub ai: Option<AiConfig>,
    pub flow: Option<FlowConfig>,
}
