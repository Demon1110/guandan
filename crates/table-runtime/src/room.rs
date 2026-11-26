//! 房间主干：建桌 → 发牌 → 行动窗口（计时 + 托管）→ 出牌 → 结算 → 再来一局，
//! 外加回放录制与断线恢复（§6.2/§6.5/§6.6、`docs/开发计划.md` §2 M1 模块表）。
//!
//! 边界与纪律：
//! - 时间是**虚拟时钟**（调用方传入毫秒），不读系统时间（§3.3 约束 2）；
//! - 房间是服务端权威源，`State` 只此一份；客户端只拿 [`RoomSnapshotWire`]（view 字节）；
//! - 判定一律走引擎：`legal_actions` 校验 + `apply`，房间不复制任何牌型/牌力逻辑；
//! - 每个成功动作都进动作记录（回放录制）与重连环形缓冲（§6.2 第 978 行，容量 200）。
//!
//! 端口 / 协议 / 联机均不在此层：本层只保证「一桌纯本地 bot 局可无人值守跑完并续局」。

use gf_engine::contract::{Action, Engine, MatchEnd, Seat, State};
use gf_engine::engine::{M0Engine, PHASE_PLAY, PHASE_SETTLE};
use gf_engine::ruleset::from_yaml;
use gf_engine::view::view_hash;

use crate::recovery::{
    decide, replay_delta, state_hash_of, ActionRecord, ReconnectBuffer, ReconnectDecision,
    RoomSnapshotState,
};
use crate::snapshot::RoomSnapshotWire;

/// `guandan-classic` 夹具（编译期内嵌，运行期不读文件）。
const CLASSIC_RULESET_YAML: &str =
    include_str!("../../engine/tests/fixtures/guandan-classic/ruleset.yaml");

/// 规则集版本身份串缺省值（`RuleSetBody` 无版本字段，房间创建时锁定一份）。
pub const DEFAULT_RULESET_VERSION: &str = "guandan-classic-1.0.0";

/// 动作窗口缺省值（规则集未声明 `timing` 时）：出牌 30s、断线宽限 40s、全离线 120s。
pub const DEFAULT_TURN_MS: u64 = 30_000;
pub const DEFAULT_GRACE_MS: u64 = 40_000;
pub const DEFAULT_OFFLINE_ALL_MS: u64 = 120_000;

/// 座位身份：本地人类玩家 / 本地 bot。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlayerKind {
    /// 人类玩家（由客户端提交动作；超时转托管）。
    Human,
    /// 本地 bot（服务端驱动，决策只读 `view(seat)`）。
    Bot,
}

/// 座位连接状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlayerStatus {
    Connected,
    Offline,
}

/// bot 决策上下文：设计 §5.11 的 `partner.state` 特征（AI-L1 档位起可用）。
///
/// 只含**本座位阵营可见**的信息——搭档座位号与搭档剩余手牌张数。对手手牌数、对手手牌内容
/// 一律不给（§3.3 约束 3）。没有队伍信息的规则集里 `partner` 为 `None`。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BotContext {
    /// 搭档座位号（`teams` 里与 `seat` 同队的另一座）。
    pub partner: Option<u8>,
    /// 搭档剩余手牌张数。
    pub partner_hand: Option<usize>,
    /// 本座位剩余手牌张数（与 `view.hand` 一致）。
    pub own_hand: usize,
}

/// 一手牌的「推荐拆分」：`recommend` 的对外形态，只含牌型与牌。
///
/// 与合法集一样由引擎按本座位手牌算出（纯只读、不进 `stateHash`），
/// 组之间互不重叠、合起来正好是这手牌。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HandPlan {
    pub groups: Vec<HandPlanGroup>,
    /// 散张：没进任何组的牌（recommend 只把它们计入 `single_count`，不给牌 id，
    /// 这里按「手牌减去各组」补齐），按牌力升序排好，bot 直接取第一张就是最小的。
    pub singles: Vec<HandPlanCard>,
    /// 拆分后剩余的单张数（散张）。
    pub single_count: u8,
    /// 跟单张时的兜底：上家出的是单张，且散张里存在压得过的那一张时给出最小的一张。
    ///
    /// 牌力比较放在这里做，是因为只有引擎知道当前局的合成牌力序（级牌提升、王最大）；
    /// bot 自己按原始点数比会把级牌当小牌，出的手会被房间判非法。
    pub follow_single: Option<u16>,
}

/// 推荐拆分里的一组。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HandPlanGroup {
    pub pattern_id: String,
    pub cards: Vec<u16>,
    /// 组内最大牌力（引擎口径）：bot 用它决定「先走哪一组」。
    pub power: u32,
}

/// 推荐拆分里的一张散张（带牌力，便于 bot 按引擎口径挑最小）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HandPlanCard {
    pub card: u16,
    pub power: u32,
}

/// 本地 bot 策略。**只给 view 字节与座位号**，不给 `State`（信息隔离，§3.3 约束 3）。
pub trait BotPolicy {
    /// 按本座位视图决策；返回的动作由房间做合法性复核。
    fn decide(&self, view_bytes: &[u8], seat: u8) -> Action;

    /// 带合法集的决策入口（AI-L1 用，§5.11「评估 + 有限扫描」）。
    ///
    /// `legal` 由房间用引擎按**本座位手牌 + 桌面公共信息**推出，不含其他座位的手牌，
    /// 因此不构成信息隔离泄漏（§3.3）。默认实现忽略合法集，回落到 [`BotPolicy::decide`]，
    /// 保持 L0 与既有 bot 的行为不变。
    fn decide_with_legal(&self, view_bytes: &[u8], seat: u8, legal: &[Action]) -> Action {
        let _ = legal;
        self.decide(view_bytes, seat)
    }

    /// 带合法集与搭档状态的决策入口（AI-L2 用）。
    ///
    /// 默认实现忽略上下文，回落到 [`BotPolicy::decide_with_legal`]。
    fn decide_with_ctx(
        &self,
        view_bytes: &[u8],
        seat: u8,
        legal: &[Action],
        ctx: &BotContext,
    ) -> Action {
        let _ = ctx;
        self.decide_with_legal(view_bytes, seat, legal)
    }

    /// 带「推荐拆分」的决策入口：bot 按 `recommend` 的最优手组牌出手，
    /// 而不是把对子拆成单张零着打。
    ///
    /// `plan` 与合法集同源（本座位手牌 + 公开信息），纯只读。默认忽略它，
    /// 回落到 [`BotPolicy::decide_with_ctx`]。
    fn decide_with_plan(
        &self,
        view_bytes: &[u8],
        seat: u8,
        legal: &[Action],
        ctx: &BotContext,
        plan: Option<&HandPlan>,
    ) -> Action {
        let _ = plan;
        self.decide_with_ctx(view_bytes, seat, legal, ctx)
    }
}

/// 建桌参数。
#[derive(Clone, Debug)]
pub struct RoomConfig {
    /// 场次标识（与 `MatchRecord.matchId` 同口径）。
    pub match_id: String,
    /// 规则集版本身份串（房间创建时锁定，之后不可变）。
    pub ruleset_version: String,
    /// 场次种子（各局派生，见 [`Room::round_seed`]）。
    pub seed: u64,
    /// 每个座位的身份；长度必须等于规则集 `seats.count`。
    pub players: Vec<PlayerKind>,
}

impl RoomConfig {
    /// 四座纯 bot 局（M1 硬 gate 的自动化演示形态）。
    pub fn all_bots(match_id: impl Into<String>, seed: u64) -> Self {
        Self {
            match_id: match_id.into(),
            ruleset_version: DEFAULT_RULESET_VERSION.to_owned(),
            seed,
            players: vec![PlayerKind::Bot; 4],
        }
    }
}

/// 动作是怎么产生的（托管记账用）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AutoReason {
    /// 行动窗口超时 → 托管。
    Timeout,
    /// 断线（座位 `Offline`）→ 立即托管。
    Offline,
    /// bot 给出的动作不合法 → 退化到合法动作首条。
    BotIllegal,
}

/// 托管记录（诊断字段；`ActionRecord` 不带此位，保持 §6.5 记录形状）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AutoAction {
    pub room_seq: u64,
    pub seat: u8,
    pub reason: AutoReason,
}

/// 一局结算摘要（`SettleResult` 的服务端内部形状；`rounds` 等由房间层承载）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SettleSummary {
    pub round_no: u16,
    pub ranks: Vec<u8>,
    pub winner_team: Option<u8>,
    pub completed_team: Option<u8>,
    pub step: Option<u8>,
    /// 整数分值；`level-up` 基线不计分，取 0。
    pub points: u8,
    pub match_end: Option<MatchEnd>,
}

/// `step` 的一次结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StepOutcome {
    /// 还在等某座位（人类）动作。
    Waiting { seat: u8, deadline_ms: u64 },
    /// 已代打 / 已执行一个动作。
    Applied {
        seat: u8,
        action: Action,
        auto: Option<AutoReason>,
    },
    /// 本局刚结束。
    Settled(SettleSummary),
    /// 场次已终止（过 A / 安全上限 / 中止）。
    MatchEnded(MatchEnd),
}

/// 房间错误出口。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RoomError {
    /// 座位越界。
    NoSuchSeat(u8),
    /// 尚未发牌 / 尚未 `start`。
    NotStarted,
    /// 已结算，等 `next_round`。
    NotInPlay,
    /// 不是该座位行动。
    WrongTurn { expected: u8, actual: u8 },
    /// 引擎拒绝（非法牌型 / 超配额 / 内部不变量）。
    Engine(String),
    /// 座位身份不是人类，不能 `submit`。
    NotHuman(u8),
    /// 场次已终止，不再开新局。
    MatchEnded,
}

impl std::fmt::Display for RoomError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RoomError::NoSuchSeat(s) => write!(f, "座位 {s} 不存在"),
            RoomError::NotStarted => write!(f, "房间尚未发牌"),
            RoomError::NotInPlay => write!(f, "当前不在出牌阶段"),
            RoomError::WrongTurn { expected, actual } => {
                write!(f, "轮次在座位 {expected}，提交来自座位 {actual}")
            }
            RoomError::Engine(msg) => write!(f, "引擎拒绝：{msg}"),
            RoomError::NotHuman(s) => write!(f, "座位 {s} 不是人类玩家"),
            RoomError::MatchEnded => write!(f, "场次已终止"),
        }
    }
}

impl std::error::Error for RoomError {}

impl From<gf_engine::contract::EngineError> for RoomError {
    fn from(value: gf_engine::contract::EngineError) -> Self {
        RoomError::Engine(value.to_string())
    }
}

/// 断线重连的服务端答复（增量事件 + 全量快照 + wire 帧三件齐）。
#[derive(Clone, Debug)]
pub struct ReconnectPlan {
    /// 增量 / 快照重置判定（§6.2 第 978 行）。
    pub decision: ReconnectDecision,
    /// 服务端全量快照（in-process 形态，供服务端自证与测试重放）。
    pub snapshot: RoomSnapshotState,
    /// 下发给客户端 `seat` 的 wire 帧（`snapshotReset` 按判定置位）。
    pub wire: RoomSnapshotWire,
}

/// 一张桌：服务端权威状态 + 动作窗口 + 记录 + 重连缓冲。
pub struct Room {
    engine: M0Engine,
    match_id: String,
    ruleset_id: String,
    ruleset_version: String,
    seed: u64,
    state: Option<State>,
    players: Vec<PlayerKind>,
    status: Vec<PlayerStatus>,
    bots: Vec<Option<Box<dyn BotPolicy>>>,
    room_seq: u64,
    seat_seq: Vec<u64>,
    log: Vec<ActionRecord>,
    buffer: ReconnectBuffer,
    auto: Vec<AutoAction>,
    turn_deadline_ms: Option<u64>,
    turn_ms: u64,
    grace_ms: u64,
    offline_all_ms: u64,
    all_offline_since: Option<u64>,
    /// 每座位断线时刻（虚拟时钟毫秒）；在线为 `None`。
    offline_since: Vec<Option<u64>>,
    aborted: bool,
    last_settle: Option<SettleSummary>,
}

impl Room {
    /// 建桌：锁定规则集、装座位、准备 bot。**不发牌**（`start` 才发）。
    pub fn new(config: RoomConfig) -> Result<Self, RoomError> {
        let body = from_yaml(CLASSIC_RULESET_YAML)
            .map_err(|e| RoomError::Engine(format!("classic 规则集构造失败：{e:?}")))?;
        let seat_count = body.seats.count as usize;
        if config.players.len() != seat_count {
            return Err(RoomError::Engine(format!(
                "座位数不匹配：config {} vs 规则集 {seat_count}",
                config.players.len()
            )));
        }
        let ruleset_id = body.ruleset_id.clone();
        let turn_ms = body
            .timing
            .as_ref()
            .map(|t| u64::from(t.turn_time_sec) * 1000)
            .unwrap_or(DEFAULT_TURN_MS);
        let grace_ms = body
            .timing
            .as_ref()
            .map(|t| u64::from(t.grace_sec) * 1000)
            .unwrap_or(DEFAULT_GRACE_MS);
        let offline_all_ms = body
            .timing
            .as_ref()
            .map(|t| u64::from(t.offline_all_sec) * 1000)
            .unwrap_or(DEFAULT_OFFLINE_ALL_MS);
        let engine = M0Engine::new(body);
        Ok(Self {
            engine,
            match_id: config.match_id,
            ruleset_id,
            ruleset_version: config.ruleset_version,
            seed: config.seed,
            state: None,
            players: config.players,
            status: vec![PlayerStatus::Connected; seat_count],
            bots: (0..seat_count).map(|_| None).collect(),
            room_seq: 0,
            seat_seq: vec![0; seat_count],
            log: Vec::new(),
            buffer: ReconnectBuffer::default(),
            auto: Vec::new(),
            turn_deadline_ms: None,
            turn_ms,
            grace_ms,
            offline_all_ms,
            all_offline_since: None,
            offline_since: vec![None; seat_count],
            aborted: false,
            last_settle: None,
        })
    }

    /// 给某个座位装上 bot（座位必须是 [`PlayerKind::Bot`]）。
    pub fn attach_bot(&mut self, seat: u8, bot: Box<dyn BotPolicy>) -> Result<(), RoomError> {
        let idx = self.seat_index(seat)?;
        if self.players[idx] != PlayerKind::Bot {
            return Err(RoomError::NotHuman(seat));
        }
        self.bots[idx] = Some(bot);
        Ok(())
    }

    /// 发牌开局（场次首局）。发完即进入出牌阶段并开第一个行动窗口。
    pub fn start(&mut self, now_ms: u64) -> Result<(), RoomError> {
        let state = self.engine.deal(self.round_seed(1))?;
        self.state = Some(state);
        self.last_settle = None;
        self.open_window(now_ms);
        Ok(())
    }

    /// 第 `round_no` 局的派生种子（确定性：同一场次同一局号必得同一手牌）。
    pub fn round_seed(&self, round_no: u16) -> u64 {
        self.seed
            .wrapping_mul(0x9E37_79B9_7F4A_7C15)
            .wrapping_add(u64::from(round_no))
    }

    /// 再来一局：只允许在结算结束后续发；`roundNo +1`，跨局携带由引擎负责。
    pub fn next_round(&mut self, now_ms: u64) -> Result<(), RoomError> {
        if self.aborted {
            return Err(RoomError::MatchEnded);
        }
        let Some(prev) = &self.state else {
            return Err(RoomError::NotStarted);
        };
        if prev.phase != PHASE_SETTLE {
            return Err(RoomError::NotInPlay);
        }
        if prev.match_end.is_some() {
            return Err(RoomError::MatchEnded);
        }
        let round_no = prev.round_no.saturating_add(1);
        let state = self
            .engine
            .deal_next_round(prev, self.round_seed(round_no))?;
        self.state = Some(state);
        self.last_settle = None;
        self.open_window(now_ms);
        Ok(())
    }

    /// 当前状态（服务端权威；测试 / 房间层内部用，不下发客户端）。
    pub fn state(&self) -> Result<&State, RoomError> {
        self.state.as_ref().ok_or(RoomError::NotStarted)
    }

    /// 当前行动窗口的截止时刻（虚拟时钟毫秒）；未开窗为 `None`。
    ///
    /// 服务端广播 `TurnChangedEvent.deadlineMs` 时读这个值，客户端按它画倒计时。
    pub fn turn_deadline_ms(&self) -> Option<u64> {
        self.turn_deadline_ms
    }

    /// 引擎句柄（只读；供服务端自证、客户端本地副本重放、测试使用）。
    pub fn engine(&self) -> &M0Engine {
        &self.engine
    }

    /// 本座位视图字节（canonical JSON；下发与 bot 决策共用同一入口）。
    pub fn view_bytes(&self, seat: u8) -> Result<Vec<u8>, RoomError> {
        let state = self.state()?;
        self.seat_index(seat)?;
        Ok(self.engine.view(state, Seat(seat)).bytes)
    }

    /// 座位是否仍可提交动作（人类 + 在线）。
    pub fn is_human(&self, seat: u8) -> bool {
        self.players
            .get(seat as usize)
            .map(|p| *p == PlayerKind::Human)
            .unwrap_or(false)
    }

    /// 动作记录（回放录制的全量日志）。
    pub fn log(&self) -> &[ActionRecord] {
        &self.log
    }

    /// 当前行动座位的合法动作集（已归一化）。只由本座位手牌 + 桌面公共信息推出。
    ///
    /// 供 wasm 绑定层做 bot 决策自证与离线代打，不构成信息泄漏。
    pub fn legal_now(&self) -> Result<Vec<Action>, RoomError> {
        self.legal_normalized()
    }

    /// 本座位 bot 的决策上下文（`partner.state`）：搭档座位与搭档剩余手牌张数。
    pub fn bot_context(&self, seat: u8) -> BotContext {
        let teams = &self.engine.ruleset.seats.teams;
        let partner = teams
            .iter()
            .find(|team| team.contains(&seat))
            .and_then(|team| team.iter().copied().find(|&s| s != seat));
        let own_hand = self.own_hand_len(seat);
        let partner_hand = partner.map(|p| self.own_hand_len(p));
        BotContext {
            partner,
            partner_hand,
            own_hand,
        }
    }

    /// 某座位的剩余手牌张数（读 `hand` 区；座位未入座或未发牌时为 0）。
    fn own_hand_len(&self, seat: u8) -> usize {
        let Ok(state) = self.state() else {
            return 0;
        };
        // `hand` 区在规则集 `zones` 里的下标即 zoneId（`ZoneDef` 顺序实例化）。
        let hand_zone = self
            .engine
            .ruleset
            .zones
            .iter()
            .position(|z| z.id.0 == "hand")
            .unwrap_or(0) as u16;
        state
            .zones
            .iter()
            .find(|z| z.seat == seat && z.zone_id == hand_zone)
            .map(|z| z.cards.len())
            .unwrap_or(0)
    }

    /// 托管记录（诊断）。
    pub fn auto_actions(&self) -> &[AutoAction] {
        &self.auto
    }

    /// 当前 `roomSeq`（广播水位）。
    pub fn room_seq(&self) -> u64 {
        self.room_seq
    }

    /// 某座位已应用到的最后 `seatSeq`（快照 / 重连水位）。
    pub fn seat_seq(&self, seat: u8) -> Result<u64, RoomError> {
        let idx = self.seat_index(seat)?;
        Ok(self.seat_seq[idx])
    }

    /// 最近一次结算摘要。
    pub fn last_settle(&self) -> Option<&SettleSummary> {
        self.last_settle.as_ref()
    }

    /// 房间是否已中止（全员离线超时）。
    pub fn aborted(&self) -> bool {
        self.aborted
    }

    /// 人类提交动作。校验轮次 → 校验合法性 → `apply` → 记账 → 刷新窗口。
    pub fn submit(
        &mut self,
        seat: u8,
        action: Action,
        now_ms: u64,
    ) -> Result<StepOutcome, RoomError> {
        let idx = self.seat_index(seat)?;
        if !self.is_human(seat) {
            return Err(RoomError::NotHuman(seat));
        }
        let state = self.state.as_ref().ok_or(RoomError::NotStarted)?;
        if state.phase != PHASE_PLAY {
            return Err(RoomError::NotInPlay);
        }
        if state.turn_seat != seat {
            return Err(RoomError::WrongTurn {
                expected: state.turn_seat,
                actual: seat,
            });
        }
        self.apply_action(idx, action, now_ms, None)
    }

    /// 回放重演单条录制动作（续局重建用）。
    ///
    /// 与 [`Room::submit`] 的区别：不复核「人类 + 在线」（bot 座位与离线座位录下的动作同样要重演），
    /// 不推进时钟（窗口在重建完成后由调用方重开）。轮次与合法性仍由引擎校验。
    pub fn replay_apply(&mut self, seat: u8, action: Action) -> Result<StepOutcome, RoomError> {
        let idx = self.seat_index(seat)?;
        let state = self.state.as_ref().ok_or(RoomError::NotStarted)?;
        if state.phase != PHASE_PLAY {
            return Err(RoomError::NotInPlay);
        }
        if state.turn_seat != seat {
            return Err(RoomError::WrongTurn {
                expected: state.turn_seat,
                actual: seat,
            });
        }
        self.apply_action(idx, action, 0, None)
    }

    /// 驱动一步：bot 行动、断线托管、窗口超时托管都从这里出。
    ///
    /// 人类在线且未超时 → [`StepOutcome::Waiting`]，由客户端按时提交。
    pub fn step(&mut self, now_ms: u64) -> Result<StepOutcome, RoomError> {
        if self.aborted {
            return Err(RoomError::MatchEnded);
        }
        let Some(state) = self.state.as_ref() else {
            return Err(RoomError::NotStarted);
        };
        if state.phase == PHASE_SETTLE {
            if let Some(end) = &state.match_end {
                return Ok(StepOutcome::MatchEnded(end.clone()));
            }
            let summary = self.settle_summary().expect("settle 态必有摘要");
            return Ok(StepOutcome::Settled(summary));
        }
        if state.phase != PHASE_PLAY {
            return Err(RoomError::NotInPlay);
        }
        let seat = state.turn_seat;
        let idx = self.seat_index(seat)?;
        let deadline = self.turn_deadline_ms.unwrap_or(now_ms);

        // ① 断线 → 宽限期（`graceSec`）内等重连，超时转托管。
        if self.status[idx] == PlayerStatus::Offline {
            let since = self.offline_since[idx].unwrap_or(now_ms);
            let grace_deadline = since.saturating_add(self.grace_ms);
            if now_ms >= grace_deadline {
                return self.auto_play(idx, now_ms, AutoReason::Offline);
            }
            return Ok(StepOutcome::Waiting {
                seat,
                deadline_ms: grace_deadline,
            });
        }
        // ② bot → 用 view(seat) + 本座位合法集 + 推荐拆分决策（不合法退化为合法首条）。
        if self.players[idx] == PlayerKind::Bot {
            let view = self.view_bytes(seat)?;
            let legal = self.legal_normalized()?;
            let plan = self.plan_for(seat);
            let action = match &self.bots[idx] {
                Some(bot) => {
                    let ctx = self.bot_context(seat);
                    bot.decide_with_plan(&view, seat, &legal, &ctx, plan.as_ref())
                }
                // 没装实现也不能直接 `Pass`（领出态非法）：退到合法首条。
                None => legal.first().cloned().unwrap_or(Action::Pass),
            };
            return self.bot_action(idx, action, now_ms);
        }
        // ③ 人类超时 → 托管。
        if now_ms >= deadline {
            return self.auto_play(idx, now_ms, AutoReason::Timeout);
        }
        Ok(StepOutcome::Waiting {
            seat,
            deadline_ms: deadline,
        })
    }

    /// 全员离线超时检查；`now` 由调用方推进。
    pub fn tick(&mut self, now_ms: u64) -> Result<(), RoomError> {
        let all_offline =
            !self.status.is_empty() && self.status.iter().all(|s| *s == PlayerStatus::Offline);
        if all_offline {
            let since = *self.all_offline_since.get_or_insert(now_ms);
            if now_ms.saturating_sub(since) >= self.offline_all_ms {
                self.aborted = true;
            }
        } else {
            self.all_offline_since = None;
        }
        Ok(())
    }

    /// 座位断线：记断线时刻（宽限期从此刻起算），等 [`Room::reconnect`]。
    pub fn disconnect(&mut self, seat: u8, now_ms: u64) -> Result<(), RoomError> {
        let idx = self.seat_index(seat)?;
        self.status[idx] = PlayerStatus::Offline;
        self.offline_since[idx] = Some(now_ms);
        Ok(())
    }

    /// 座位重连：给增量判定 + 全量快照 + wire 帧；座位回到在线。
    pub fn reconnect(&mut self, seat: u8, last_seat_seq: u64) -> Result<ReconnectPlan, RoomError> {
        let idx = self.seat_index(seat)?;
        let state = self.state.as_ref().ok_or(RoomError::NotStarted)?;
        let decision = decide(&self.buffer, seat, last_seat_seq);
        let snapshot_reset = matches!(decision, ReconnectDecision::SnapshotReset);
        let view = self.engine.view(state, Seat(seat));
        let wire = RoomSnapshotWire::server_build(
            self.match_id.clone(),
            seat,
            state.round_no,
            self.ruleset_id.clone(),
            self.ruleset_version.clone(),
            self.room_seq,
            self.seat_seq[idx],
            self.engine.ruleset.view.schema_version,
            view.bytes.clone(),
            snapshot_reset,
        );
        let snapshot = RoomSnapshotState {
            state: state.clone(),
            snapshot_reset,
            room_seq: self.room_seq,
            round_no: state.round_no,
            view_hash: view_hash(seat, self.room_seq, &view.bytes),
            view_schema_version: self.engine.ruleset.view.schema_version,
        };
        self.status[idx] = PlayerStatus::Connected;
        self.offline_since[idx] = None;
        // 重连后按当前行动座位重开窗口（人类在线等提交，bot 由 step 驱动）。
        self.open_window(self.turn_deadline_ms.unwrap_or(0));
        Ok(ReconnectPlan {
            decision,
            snapshot,
            wire,
        })
    }

    /// 重开当前行动窗口（客户端重载后调用：动作已回放，但窗口 deadline 需要按新时钟重算）。
    pub fn resume_window(&mut self, now_ms: u64) -> Result<(), RoomError> {
        if self.state.is_none() {
            return Err(RoomError::NotStarted);
        }
        self.open_window(now_ms);
        Ok(())
    }

    /// 按座位取 wire 快照帧（服务端下发前自证 `viewHash`）。
    pub fn snapshot_wire(
        &self,
        seat: u8,
        snapshot_reset: bool,
    ) -> Result<RoomSnapshotWire, RoomError> {
        let idx = self.seat_index(seat)?;
        let state = self.state.as_ref().ok_or(RoomError::NotStarted)?;
        let view = self.engine.view(state, Seat(seat));
        Ok(RoomSnapshotWire::server_build(
            self.match_id.clone(),
            seat,
            state.round_no,
            self.ruleset_id.clone(),
            self.ruleset_version.clone(),
            self.room_seq,
            self.seat_seq[idx],
            self.engine.ruleset.view.schema_version,
            view.bytes,
            snapshot_reset,
        ))
    }

    /// 回放录制自证：把全量日志从首局起点重放，`stateHash` 必须与权威状态逐字节相等。
    pub fn replay_hash(&self) -> Result<[u8; 32], RoomError> {
        if self.state.is_none() {
            return Err(RoomError::NotStarted);
        }
        let mut replay = self.engine.deal(self.round_seed(1))?;
        for record in &self.log {
            if record.round_no != replay.round_no {
                // 跨局边界：按记录里的局号续发，再重放。
                while replay.round_no < record.round_no {
                    replay = self
                        .engine
                        .deal_next_round(&replay, self.round_seed(replay.round_no + 1))?;
                }
            }
            replay = self.engine.apply(replay, record.action.clone())?;
        }
        Ok(state_hash_of(&replay))
    }

    /// 当前权威状态哈希（与 [`Room::replay_hash`] 比对用）。
    pub fn state_hash(&self) -> Result<[u8; 32], RoomError> {
        let state = self.state.as_ref().ok_or(RoomError::NotStarted)?;
        Ok(state_hash_of(state))
    }

    // ---------- 内部 ----------

    fn seat_index(&self, seat: u8) -> Result<usize, RoomError> {
        let idx = seat as usize;
        if idx < self.players.len() {
            Ok(idx)
        } else {
            Err(RoomError::NoSuchSeat(seat))
        }
    }

    fn open_window(&mut self, now_ms: u64) {
        let in_play = self
            .state
            .as_ref()
            .map(|s| s.phase == PHASE_PLAY)
            .unwrap_or(false);
        self.turn_deadline_ms = if in_play {
            Some(now_ms.saturating_add(self.turn_ms))
        } else {
            None
        };
    }

    fn settle_summary(&self) -> Option<SettleSummary> {
        let state = self.state.as_ref()?;
        if state.phase != PHASE_SETTLE {
            return None;
        }
        Some(SettleSummary {
            round_no: state.round_no,
            ranks: state.ranks.clone(),
            winner_team: state.winner_team,
            completed_team: state.completed_team,
            step: state.settle_step,
            points: 0,
            match_end: state.match_end.clone(),
        })
    }

    /// 合法动作（归一化：出牌按牌 id 升序）。
    fn legal_normalized(&self) -> Result<Vec<Action>, RoomError> {
        let state = self.state.as_ref().ok_or(RoomError::NotStarted)?;
        let legal = self.engine.legal_actions(state)?;
        Ok(legal.into_iter().map(normalize_action).collect())
    }

    /// 本座位的一手牌推荐拆分（`recommend`，纯只读）：bot 的出牌规划与页面「组牌建议」共用。
    fn plan_for(&self, seat: u8) -> Option<HandPlan> {
        let state = self.state.as_ref()?;
        let layout = self
            .engine
            .recommend(state, gf_engine::contract::Seat(seat))
            .ok()?;
        let hand = self.engine.hand_cards_of(state, seat)?;
        let covered: Vec<u16> = layout
            .groups
            .iter()
            .flat_map(|g| g.cards.iter().copied())
            .collect();
        // 散张按牌力升序：bot 领出时「散张 vs 组」一起比，取最小的那一手。
        let mut singles: Vec<HandPlanCard> = hand
            .into_iter()
            .filter(|card| !covered.contains(card))
            .map(|card| HandPlanCard {
                card,
                power: self.engine.card_power(state, card),
            })
            .collect();
        singles.sort_unstable_by_key(|s| (s.power, s.card));
        // 跟单张的兜底：上家是单张（且不是自己）时，散张里挑第一张压得过的。
        let follow_single = match state.last_play.as_ref() {
            Some(lp) if lp.cards.len() == 1 && lp.seat != seat => {
                let last_power = self.engine.card_power(state, lp.cards[0]);
                singles
                    .iter()
                    .find(|s| s.power > last_power)
                    .map(|s| s.card)
            }
            _ => None,
        };
        Some(HandPlan {
            single_count: layout.single_count,
            groups: layout
                .groups
                .into_iter()
                .map(|g| HandPlanGroup {
                    power: g
                        .cards
                        .iter()
                        .copied()
                        .map(|c| self.engine.card_power(state, c))
                        .max()
                        .unwrap_or(0),
                    pattern_id: g.pattern_id,
                    cards: g.cards,
                })
                .collect(),
            singles,
            follow_single,
        })
    }

    fn bot_action(
        &mut self,
        idx: usize,
        action: Action,
        now_ms: u64,
    ) -> Result<StepOutcome, RoomError> {
        let legal = self.legal_normalized()?;
        let candidate = normalize_action(action);
        // bot 按推荐拆分出的组常常不在候选清单里（清单只给同型代表），
        // 这里按引擎口径复核一次，能过就走正常路；过不了才退化成合法首条。
        let acceptable = legal.contains(&candidate)
            || self.state.as_ref().is_some_and(|state| match &candidate {
                Action::Play { cards } => {
                    matches!(self.engine.preview_play(state, cards), Ok((_, true)))
                }
                Action::Pass => false,
            });
        if acceptable {
            return self.apply_action(idx, candidate, now_ms, None);
        }
        // bot 决策不合法：退化为合法首条并记托管（`BotIllegal`）。
        let fallback = legal.into_iter().next().unwrap_or(Action::Pass);
        self.apply_action(idx, fallback, now_ms, Some(AutoReason::BotIllegal))
    }

    fn auto_play(
        &mut self,
        idx: usize,
        now_ms: u64,
        reason: AutoReason,
    ) -> Result<StepOutcome, RoomError> {
        let legal = self.legal_normalized()?;
        let action = legal.into_iter().next().unwrap_or(Action::Pass);
        self.apply_action(idx, action, now_ms, Some(reason))
    }

    fn apply_action(
        &mut self,
        idx: usize,
        action: Action,
        now_ms: u64,
        auto: Option<AutoReason>,
    ) -> Result<StepOutcome, RoomError> {
        let seat = idx as u8;
        let legal = self.legal_normalized()?;
        let candidate = normalize_action(action.clone());
        if !legal.contains(&candidate) {
            // 合法集只给每种牌型的代表组合（单张只列最大一张、顺子只列一条），
            // 人类手选的同型牌因此常常不在清单里。这里按引擎口径复核一次：
            // 牌型成立、且跟牌时压得过上一手，就放行；结构不成型 / 压不过仍拒。
            let allow = match &candidate {
                Action::Play { cards } => self.state.as_ref().is_some_and(|state| {
                    matches!(self.engine.preview_play(state, cards), Ok((_, true)))
                }),
                Action::Pass => false,
            };
            if !allow {
                return Err(RoomError::Engine(format!(
                    "动作不在合法集内：{candidate:?}"
                )));
            }
        }
        let state = self.state.take().ok_or(RoomError::NotStarted)?;
        let round_no = state.round_no;
        let next = self.engine.apply(state, candidate.clone())?;

        // 记账：全局 roomSeq → 座位 seatSeq（跨局不归零）。
        let room_seq = self.room_seq;
        self.room_seq += 1;
        let seat_seq = self.seat_seq[idx];
        self.seat_seq[idx] += 1;
        let record = ActionRecord {
            seat_seq,
            room_seq,
            round_no,
            seat,
            request_id: room_seq,
            action: candidate.clone(),
            ts_ms: now_ms,
        };
        self.buffer.push(record.clone());
        self.log.push(record);
        if let Some(reason) = auto {
            self.auto.push(AutoAction {
                room_seq,
                seat,
                reason,
            });
        }

        let settled = next.phase == PHASE_SETTLE;
        self.state = Some(next);
        if settled {
            self.turn_deadline_ms = None;
            self.last_settle = self.settle_summary();
            let summary = self.last_settle.clone().expect("settle 态必有摘要");
            if let Some(end) = &summary.match_end {
                self.aborted = end.reason == gf_engine::contract::MatchEndReason::Aborted;
            }
            return Ok(StepOutcome::Settled(summary));
        }
        self.open_window(now_ms);
        Ok(StepOutcome::Applied {
            seat,
            action: candidate,
            auto,
        })
    }
}

/// 动作归一化：出牌牌序按牌 id 升序（与引擎判定的规范形态对齐）。
pub fn normalize_action(action: Action) -> Action {
    match action {
        Action::Play { cards } => {
            let mut cards = cards;
            cards.sort_unstable();
            Action::Play { cards }
        }
        Action::Pass => Action::Pass,
    }
}

/// 增量重放（对外暴露，客户端本地副本恢复用）。
pub fn replay(
    engine: &M0Engine,
    state: State,
    events: &[ActionRecord],
) -> Result<State, RoomError> {
    Ok(replay_delta(engine, state, events)?)
}
