//! 执行路径：发牌（`deal.shuffle`）→ 出牌（`action.require-legal`，牌型判定见
//! [`crate::patterns`]）→ `score-only` 结算。
//!
//! M1 本批已落地 `patterns[]` 已声明牌型的判定、枚举与压制比较；轮转方向 / 跳过已出完 /
//! 全过后领出口径 / 接风已按 `flow.turn` 接线。进贡/还贡/抗贡、`level.*` 升级档仍属后续批次，
//! 命中走保守空候选 / 内部错误，不在判定链路里发明规则。逢人配、三带二、级牌点力三条
//! hold 见 [`crate::patterns`] 顶部注释。
//!
//! 确定性纪律（§3.3 约束 2）：纯整数、无浮点、无 `SystemTime`/线程 RNG/文件网络 I/O；
//! 排序给全序；RNG 唯一来源是引擎注入的种子与计数器。

use crate::contract::{
    Action, Engine, EngineError, MatchEnd, MatchEndReason, PatternId, Seat, State,
    TributeEventRecord, TributeReturnPending, View, ZoneId,
};
use crate::hash::{seed_digest, LastPlay, RngState, TributeState, ZoneEntry};
use crate::patterns;
use crate::ruleset::model::{
    AfterAllPass, LevelTable, Settle, TributeResist, TributeWhen, TurnDirection, ZoneScope,
};
use crate::ruleset::params::PrimitiveParams;
use crate::ruleset::RuleSetBody;
use crate::settlement::safety_cap_winner;
use crate::view::{encode_canonical, ViewData, ViewLastPlay};
use std::collections::{BTreeMap, HashSet};
use std::sync::OnceLock;

/// phase 枚举序号（与 `model::Phase` 声明序一致：deal=0, tribute=1, play=2, settle=3）。
pub const PHASE_DEAL: u16 = 0;
pub const PHASE_TRIBUTE: u16 = 1;
pub const PHASE_PLAY: u16 = 2;
pub const PHASE_SETTLE: u16 = 3;

/// 合法动作候选峰值闸门（§5.7 `RS_QUOTA_EXCEEDED`，4096）。
const ACTION_QUOTA: usize = 4096;

/// M0 引擎：持有载入后的 RuleSetBody，实现四接口的最小路径。
pub struct M0Engine {
    pub ruleset: RuleSetBody,
    /// 载入期合成牌力阶梯（`synthetic_ladder` 结果）。只依赖规则集体、不依赖局级牌档位，
    /// 构造后缓存一次；枚举 / 压制比较都读这份缓存，不再每次重建。
    ladder: OnceLock<Vec<PatternId>>,
}

impl M0Engine {
    pub fn new(ruleset: RuleSetBody) -> Self {
        Self {
            ruleset,
            ladder: OnceLock::new(),
        }
    }

    /// 载入期缓存的合成阶梯；规则集体在引擎生命周期内不可变，缓存与局级牌档位无关，
    /// 换局 / 换 `attackingTeam` 不会命中旧档位。
    fn cached_ladder(&self) -> &[PatternId] {
        self.ladder
            .get_or_init(|| crate::ruleset::ladder::synthetic_ladder(&self.ruleset))
    }

    // ---------- 载入期派生：区号 / 牌 id / 牌力序 ----------

    /// 区号按 RuleSet 声明序分配并冻结（§5.12 第 796 行）。
    fn zone_index(&self, zone: &ZoneId) -> Option<u16> {
        self.ruleset
            .zones
            .iter()
            .position(|z| z.id == *zone)
            .map(|i| i as u16)
    }

    /// 发牌落点私有区：`flow.deal.zone` 缺省取唯一 private 区（§5.1 第 405 行）。
    fn hand_zone(&self) -> ZoneId {
        if let Some(deal) = self.ruleset.flow.as_ref().and_then(|f| f.deal.as_ref()) {
            if let Some(zone) = &deal.zone {
                return zone.clone();
            }
        }
        self.ruleset
            .zones
            .iter()
            .find(|z| z.scope == ZoneScope::Private)
            .map(|z| z.id.clone())
            .unwrap_or_else(|| ZoneId("hand".to_owned()))
    }

    pub(crate) fn hand_zone_index(&self) -> u16 {
        self.zone_index(&self.hand_zone()).unwrap_or(0)
    }

    /// 公共弃牌区：声明序第一个 shared 区。
    pub(crate) fn discard_zone_index(&self) -> Option<u16> {
        self.ruleset
            .zones
            .iter()
            .position(|z| z.scope == ZoneScope::Shared)
            .map(|i| i as u16)
    }

    /// 当前局牌力序上下文：打级队 `levels[attackingTeam]` 的级牌（`deck.ranks` 下标）；
    /// score-only 局无 `levelTable` 时返回 `None`，合成序退回 naturalOrder。
    fn rank_context(&self, state: &State) -> patterns::RankContext {
        let level_rank = match state.levels.get(state.attacking_team as usize) {
            Some(&lv) => self
                .ruleset
                .level_table
                .as_ref()
                .and_then(|t| t.ranks.get(lv as usize))
                .and_then(|k| self.ruleset.deck.ranks.iter().position(|d| d == k)),
            None => None,
        };
        patterns::RankContext::new(&self.ruleset, level_rank)
    }

    /// `action.require-legal` 参数（§5.2 第 524 行）：`force` 是否要求压过 lastPlay，
    /// `consume` 出牌扣牌区；规则集未声明时取默认 force=true、consume=hand。
    fn require_legal_params(&self) -> (bool, ZoneId) {
        self.ruleset
            .actions
            .iter()
            .find(|a| a.primitive == "action.require-legal")
            .and_then(|a| match &a.params {
                PrimitiveParams::RequireLegal { force, consume } => Some((*force, consume.clone())),
                _ => None,
            })
            .unwrap_or((true, ZoneId("hand".to_owned())))
    }

    /// 过 A 名次组合（§5.13「过 A」、§5.1 `level.pass.combos`）：读顶层 `level.pass.combos`；
    /// 缺省只在 `teamSize = 2` 时有定义 `[[1,2],[1,3]]`，其余规模缺省为空（L1 已拦）。
    fn pass_combos(&self) -> Vec<Vec<u8>> {
        match &self.ruleset.level {
            Some(level) => level.pass.combos.clone().unwrap_or_default(),
            None => match self.ruleset.seats.teams.first().map(|t| t.len()) {
                Some(2) => vec![vec![1, 2], vec![1, 3]],
                _ => Vec::new(),
            },
        }
    }

    /// `aFailStreak` 退回阈值（§5.13 行 1006）：读顶层 `level.pass.streakLimit`，基线 3。
    fn streak_limit(&self) -> u8 {
        self.ruleset
            .level
            .as_ref()
            .map(|l| l.pass.streak_limit)
            .unwrap_or(3)
    }

    /// 达阈值是否退回 `levelTable.initial` 并清零（§5.1 `resetOnFail`），基线 `true`。
    fn reset_on_fail(&self) -> bool {
        self.ruleset
            .level
            .as_ref()
            .map(|l| l.pass.reset_on_fail)
            .unwrap_or(true)
    }

    /// 安全局数上限（§5.13「场次终止」行 1008、§5.1 `matchCap.rounds`）：缺省 12，
    /// 取值 1–99；仅防死循环，不是胜负判据。
    pub(crate) fn match_cap_rounds(&self) -> u16 {
        self.ruleset
            .match_cap
            .as_ref()
            .map(|c| c.rounds)
            .unwrap_or(12)
    }

    /// `action.consume-cards` 参数（§5.2 第 525 行）：`zone` 扣牌区；`count` 在 v1
    /// 只允许 0（L1 闸门，count>0 标 v2 预留），运行期只取扣牌区。规则集未声明该动作时返回 `None`。
    fn consume_cards_zone(&self) -> Option<ZoneId> {
        self.ruleset
            .actions
            .iter()
            .find(|a| a.primitive == "action.consume-cards")
            .and_then(|a| match &a.params {
                PrimitiveParams::ConsumeCards { zone, .. } => Some(zone.clone()),
                _ => None,
            })
    }

    /// 本次出牌的扣牌区：显式 `action.consume-cards` 优先，否则取 `require-legal.consume`。
    /// `count>0` 在 v1 由 L1 载入期拒绝（第二十七次变更，v2 预留），运行期不再兜。
    fn resolve_consume_zone(&self) -> Result<ZoneId, EngineError> {
        if let Some(zone) = self.consume_cards_zone() {
            return Ok(zone);
        }
        Ok(self.require_legal_params().1)
    }

    fn rank_count(&self) -> usize {
        self.ruleset.deck.ranks.len()
    }

    fn suit_count(&self) -> usize {
        self.ruleset.deck.suits.len()
    }

    fn copies(&self) -> usize {
        self.ruleset.deck.copies as usize
    }

    /// 非王基础牌总数：`(ranks − exclude) × suits × copies`（m0-minimal 无 exclude）。
    fn base_total(&self) -> usize {
        self.rank_count() * self.suit_count() * self.copies()
    }

    fn joker_total(&self) -> usize {
        self.ruleset.deck.jokers as usize * self.copies()
    }

    fn deck_total(&self) -> usize {
        self.base_total() + self.joker_total()
    }

    // ---------- RNG（§5.2 第 551–563 行：20 字节 BLAKE3 + 拒绝采样） ----------

    // ---------- 相位机（§6.3 第 1074–1080 行转移表） ----------

    /// §6.3 相位转移表：相位机只按本表转移，不存在表外跳转。
    ///
    /// 已接线：`deal → play`（`tribute.enabled=false` 整段跳过，首局 / 抗贡同路径）、
    /// `play → settle`（某队全员出完）、`settle → deal`（`deal_next_round` 续发下一局，
    /// 携带 `levels` / `aFailStreak` / `attackingTeam` / 水位，局号 +1）。
    /// 表内声明但执行路径待落地（不自造转移）：
    /// - `deal → tribute`、`tribute → play`：进贡 / 还贡 / 抗贡分支（进贡原语未实现）；
    /// - 场次终止（过 A / `matchCap.rounds` 触发）仍待房间层接线。
    fn allowed_transition(from: u16, to: u16) -> bool {
        matches!(
            (from, to),
            (PHASE_DEAL, PHASE_TRIBUTE)
                | (PHASE_DEAL, PHASE_PLAY)
                | (PHASE_TRIBUTE, PHASE_PLAY)
                | (PHASE_PLAY, PHASE_SETTLE)
                | (PHASE_SETTLE, PHASE_DEAL)
        )
    }

    /// 校验并执行相位转移；表外跳转（跳相、回退）报 `Internal`，不静默接受。
    fn transition(&self, state: &mut State, to: u16) -> Result<(), EngineError> {
        if !Self::allowed_transition(state.phase, to) {
            return Err(EngineError::Internal {
                message: format!(
                    "非法相位转移: {} → {}（§6.3 转移表外跳转，跳相 / 回退）",
                    state.phase, to
                ),
            });
        }
        state.phase = to;
        Ok(())
    }

    // ---------- 发牌（Fisher–Yates 降序；按座位升序连续切分） ----------

    /// 由固定种子开局（场次首局）：洗牌并发到各座位私有区，相位进入 `play`（夹具省略 tribute，G6）。
    ///
    /// §6.2 裁定 B：`roundNo` 首局 = 1（不从 0 起）；`seq_watermark` 首局从 0 起；
    /// `levels` / `aFailStreak` 取载入初始值；抽牌前置决定首出座位与打级队。
    pub fn deal(&self, seed: u64) -> Result<State, EngineError> {
        let seat_count = self.ruleset.seats.count as usize;
        let total = self.deck_total();
        let per_seat = self.per_seat(total, seat_count)?;
        let stream = self.rng_stream();

        let mut rng = RngState {
            counter: 0,
            seed_digest: seed_digest(seed),
        };
        let mut deck: Vec<u16> = (0..total as u16).collect();

        // §5.2 第 560 行 / §5.13 第 876 行：场次首局先 `bounded(总张数)` 取牌 id 下标 `i`；
        // 抽出的牌不剔出牌堆、正常参与本局，随后才 Fisher–Yates 洗牌并按座位序连续切分；
        // 切分完按牌 id 定位持牌座位 → 该座位先出、其队即 `attackingTeam`（§5.12 第 822 行）。
        let drawn_card = Self::bounded(&mut rng, seed, stream, total as u64) as u16;
        self.shuffle_deck(&mut deck, &mut rng, seed, stream);

        // 按牌 id 反查持牌座位：洗牌后该牌落在第 `pos` 个槽位，座位 = `pos / perSeat`。
        let lead_seat = deck
            .iter()
            .position(|&card| card == drawn_card)
            .map(|pos| pos / per_seat)
            .unwrap_or(0) as u8;
        let attacking_team = self.team_of(lead_seat);

        let mut state = State {
            phase: PHASE_DEAL,
            turn_seat: lead_seat,
            attacking_team,
            levels: self.initial_levels(),
            seats: (0..seat_count as u8).collect(),
            zones: self.deal_zones(&deck, per_seat),
            last_play: None,
            seq_watermark: (0..seat_count as u8).map(|s| (s, 0)).collect(),
            tribute: Self::placeholder_tribute(),
            rng,
            a_fail_streak: vec![0; self.ruleset.seats.teams.len()],
            finish_order: Vec::new(),
            seed,
            passes_since_last_play: 0,
            winner_team: None,
            completed_team: None,
            ranks: Vec::new(),
            settle_step: None,
            tribute_resisted: false,
            tribute_events: Vec::new(),
            tribute_pending_returns: Vec::new(),
            tribute_leader: None,
            match_end: None,
            round_no: 1,
        };

        // §6.3 转移表：`tribute.enabled=false`（或抗贡成立）时 `deal → play` 整段跳过
        // 进贡；`enabled=true` 的 `deal → tribute` 路径待进贡原语落地后接入。
        self.transition(&mut state, PHASE_PLAY)?;
        Ok(state)
    }

    /// 跨局续发：上一局 `settle` 结束后开下一局（§6.2 裁定 B、§5.13「决定首出」）。
    ///
    /// - 只允许从 `PHASE_SETTLE` 续发；`roundNo = 上一局 +1`，`settle` 本身不改局号。
    /// - 跨局携带：`levels` / `aFailStreak` / `attackingTeam` / `seq_watermark` 原样延续；
    ///   座位水位不在换局时复位（否则第 2 局开头会被误判 `STALE_SEQ`）。
    /// - 非首局不抽牌、不额外消耗随机字；首出座位 = 上一局头游（`finishOrder[0]`）。
    /// - 牌区 / `lastPlay` / 名次 / 进贡占位 / 过牌计数按新局清空；RNG 按本局重新起算。
    /// - 发牌后进入 [`Self::resolve_tribute`]：`enabled=true` 且未抗贡走 `deal→tribute→play`，
    ///   抗贡 / `enabled=false` / 无上局名次走 `deal→play` 直转。
    pub fn deal_next_round(&self, prev: &State, seed: u64) -> Result<State, EngineError> {
        let mut state = self.build_round_state(prev, seed)?;
        self.resolve_tribute(&mut state, prev)?;
        Ok(state)
    }

    /// 续发局的发牌骨架：洗牌切分到各座位，相位停在 `PHASE_DEAL`，尚未做相位转移与进贡。
    /// 测试可先据此替换受控手牌，再调 [`Self::resolve_tribute`] 复现指定配对 / 抗贡局面。
    fn build_round_state(&self, prev: &State, seed: u64) -> Result<State, EngineError> {
        if prev.phase != PHASE_SETTLE {
            return Err(EngineError::Internal {
                message: "只有 settle 结束的局才能续发下一局".to_owned(),
            });
        }
        if let Some(end) = &prev.match_end {
            return Err(EngineError::Internal {
                message: format!(
                    "场次已终止（end_reason={}），不再开新局",
                    end.reason.code_str()
                ),
            });
        }
        let total = self.deck_total();
        let seat_count = self.ruleset.seats.count as usize;
        let per_seat = self.per_seat(total, seat_count)?;
        let stream = self.rng_stream();

        let mut rng = RngState {
            counter: 0,
            seed_digest: seed_digest(seed),
        };
        let mut deck: Vec<u16> = (0..total as u16).collect();
        // 非首局：不抽前置牌、不消耗那条随机字；洗牌序列照常走本局 stream/counter。
        self.shuffle_deck(&mut deck, &mut rng, seed, stream);

        let head_seat = prev
            .finish_order
            .first()
            .copied()
            .ok_or(EngineError::Internal {
                message: "settle 结束却没有名次序列".to_owned(),
            })?;

        let state = State {
            phase: PHASE_DEAL,
            turn_seat: head_seat,
            attacking_team: prev.attacking_team,
            levels: prev.levels.clone(),
            seats: (0..seat_count as u8).collect(),
            zones: self.deal_zones(&deck, per_seat),
            last_play: None,
            seq_watermark: prev.seq_watermark.clone(),
            tribute: Self::placeholder_tribute(),
            rng,
            a_fail_streak: prev.a_fail_streak.clone(),
            finish_order: Vec::new(),
            seed,
            passes_since_last_play: 0,
            winner_team: None,
            completed_team: None,
            ranks: Vec::new(),
            settle_step: None,
            tribute_resisted: false,
            tribute_events: Vec::new(),
            tribute_pending_returns: Vec::new(),
            tribute_leader: None,
            match_end: None,
            round_no: prev.round_no.saturating_add(1),
        };
        Ok(state)
    }

    // ---------- 进贡 / 还贡 / 抗贡（§5.13 行 954–964、§6.3 行 1114–1116） ----------

    /// 进贡相位：贡方自动交最大牌，受贡方停在 `tribute` 相位挑一张还贡。
    ///
    /// - `enabled=false`、无上局名次（首局口径）：`deal → play` 直转，第 7 段保持占位；
    /// - 抗贡成立：进贡与还贡均不发生、不发事件、`deal → play` 直转，`tribute_resisted=true`；
    /// - 正常：按 `pairs[]` 的 `when` 分支逐对进贡，`deal → tribute`，受贡方依次提交
    ///   [`Action::TributeReturn`]，还完最后一对才 `tribute → play`。
    ///
    /// 首出座位：贡牌最大者（双下两对时比较两张贡牌牌力，同力取先配对的一方）；
    /// 抗贡 / 无进贡仍由上局头游首出。
    ///
    /// 双下两对的第 7 段（单组 vs 序列）编码冲突未裁决：这里只把第一对写入 `state.tribute`
    /// 哈希段，第二对仅落在 `tribute_events`（诊断、不进哈希），见批次挂账。
    fn resolve_tribute(&self, state: &mut State, prev: &State) -> Result<(), EngineError> {
        let Some(cfg) = self.ruleset.tribute.as_ref() else {
            return self.transition(state, PHASE_PLAY);
        };
        if !cfg.enabled {
            return self.transition(state, PHASE_PLAY);
        }

        // 名次 `r`（1-based，1 = 头游）→ 座位；无上局名次返回 `None`（首局口径）。
        let rank_seat = |r: u8| prev.finish_order.get((r - 1) as usize).copied();
        let Some(head_seat) = rank_seat(1) else {
            return self.transition(state, PHASE_PLAY);
        };

        // 双下判据：上局第 3、4 名同队（§5.13 行 960）。
        let double_loss = match (rank_seat(3), rank_seat(4)) {
            (Some(a), Some(b)) => self.team_of(a) == self.team_of(b),
            _ => false,
        };
        let when = if double_loss {
            TributeWhen::DoubleLoss
        } else {
            TributeWhen::SingleLoss
        };

        let Some(all_pairs) = cfg.pairs.as_ref() else {
            return self.transition(state, PHASE_PLAY);
        };
        let active: Vec<&crate::ruleset::model::TributePair> =
            all_pairs.iter().filter(|p| p.when == when).collect();
        if active.is_empty() {
            return self.transition(state, PHASE_PLAY);
        }

        // 抗贡：贡方（本分支所有 payer）合计持 2 张大王即抗贡，不要求同一人手里。
        let payers: Vec<u8> = match active
            .iter()
            .map(|p| rank_seat(p.payer))
            .collect::<Option<Vec<_>>>()
        {
            Some(payers) => payers,
            None => return self.transition(state, PHASE_PLAY),
        };
        let resisted = matches!(cfg.resist, Some(TributeResist::TwoBigJokers))
            && payers
                .iter()
                .map(|&s| self.big_joker_count(state, s))
                .sum::<usize>()
                >= 2;
        if resisted {
            state.tribute_resisted = true;
            // 整段跳过：不发事件、第 7 段占位保持；deal → play 直转。
            return self.transition(state, PHASE_PLAY);
        }

        let ctx = self.rank_context(state);
        let wild = patterns::wildcard_ids(&self.ruleset, &ctx);
        let mut pending: Vec<TributeReturnPending> = Vec::new();
        let mut leader: Option<(u8, u32)> = None;

        for pair in active {
            let payer = rank_seat(pair.payer).ok_or_else(|| EngineError::Internal {
                message: format!("进贡配对名次 {} 在上局名次序列中缺失", pair.payer),
            })?;
            let receiver = rank_seat(pair.receiver).ok_or_else(|| EngineError::Internal {
                message: format!("进贡配对名次 {} 在上局名次序列中缺失", pair.receiver),
            })?;

            let pay_card =
                self.select_pay_card(state, payer, &wild)
                    .ok_or_else(|| EngineError::Internal {
                        message: format!("seat {payer} 无牌可进贡",),
                    })?;
            self.move_card(state, payer, receiver, pay_card)?;
            state.tribute_events.push(TributeEventRecord {
                from: payer,
                to: receiver,
                card: pay_card,
                is_return: false,
            });
            // 第 7 段单组：仅记第一对（双下第二对的哈希承载待上级裁决）。
            if state.tribute.card.is_none() {
                state.tribute = TributeState {
                    from_seat: payer,
                    to_seat: receiver,
                    card: Some(pay_card),
                };
            }
            // 贡牌最大者首出；双下两张同力时，保留先配对的贡方。
            let power = self.card_power(state, pay_card);
            if leader.map_or(true, |(_, best)| power > best) {
                leader = Some((payer, power));
            }
            pending.push(TributeReturnPending { payer, receiver });
        }

        state.tribute_leader = leader.map(|(seat, _)| seat);
        state.tribute_pending_returns = pending;

        // 正常路径：deal → tribute，停在第一位受贡方的还贡动作上。
        self.transition(state, PHASE_TRIBUTE)?;
        state.turn_seat = state
            .tribute_pending_returns
            .first()
            .map(|p| p.receiver)
            .unwrap_or_else(|| state.tribute_leader.unwrap_or(head_seat));
        Ok(())
    }

    /// 座位手牌区大王张数（抗贡判定用，§5.13 行 962）。
    fn big_joker_count(&self, state: &State, seat: u8) -> usize {
        let Some(hand) = self.hand_cards(state, seat) else {
            return 0;
        };
        let big0 = patterns::joker_id(&self.ruleset, true, 0);
        let big1 = patterns::joker_id(&self.ruleset, true, 1);
        hand.iter().filter(|&&c| c == big0 || c == big1).count()
    }

    /// 选贡牌：除红桃级牌（百搭）外点数最大，同点数取牌 id 最小（§5.13 行 961）。
    fn select_pay_card(
        &self,
        state: &State,
        seat: u8,
        wild: &std::collections::HashSet<u16>,
    ) -> Option<u16> {
        let hand = self.hand_cards(state, seat)?;
        // 同点数取牌 id 最小：权重降序，权重相同再按 `MAX-c` 降序（即 id 升序）。
        hand.iter()
            .copied()
            .filter(|c| !wild.contains(c))
            .max_by_key(|&c| (self.card_weight(c), u32::from(u16::MAX - c)))
    }

    /// 还贡可选牌：当前受贡方手里不大于 10 的牌；若一张都没有（极端手牌），
    /// 退化为整手牌，避免还贡相位卡死。王不参与“≤10”候选。
    pub fn tribute_return_cards(&self, state: &State) -> Vec<u16> {
        let seat = state.turn_seat;
        let Some(hand) = self.hand_cards(state, seat) else {
            return Vec::new();
        };
        let ten_index = self
            .ruleset
            .deck
            .ranks
            .iter()
            .position(|k| k.as_str() == "10")
            .unwrap_or(8);
        let under_ten: Vec<u16> = hand
            .iter()
            .copied()
            .filter(|c| match self.card_rank_index(*c) {
                Some(ri) => ri <= ten_index,
                None => false,
            })
            .collect();
        if under_ten.is_empty() {
            hand.clone()
        } else {
            under_ten
        }
    }

    /// 还贡默认牌：按规则集 `return` 口径选（默认不大于 10 的最小牌），bot / 托管用。
    pub fn tribute_return_default(&self, state: &State) -> Option<u16> {
        let cfg = self.ruleset.tribute.as_ref()?;
        self.select_return_card(state, state.turn_seat, cfg)
    }

    /// 还贡动作：受贡方从手牌挑一张还给对应贡方；还完最后一对才进 play。
    fn apply_tribute_return(
        &self,
        mut state: State,
        action: Action,
    ) -> Result<State, EngineError> {
        let Action::TributeReturn { card } = action else {
            return Err(EngineError::IllegalAction {
                reason: "还贡相位只接受 TributeReturn 动作".to_owned(),
            });
        };
        let Some(pending) = state.tribute_pending_returns.first().copied() else {
            return Err(EngineError::IllegalAction {
                reason: "当前没有待还贡的配对".to_owned(),
            });
        };
        let seat = state.turn_seat;
        if seat != pending.receiver {
            return Err(EngineError::IllegalAction {
                reason: format!(
                    "还贡应由受贡方 seat {} 提交，当前 turnSeat={seat}",
                    pending.receiver
                ),
            });
        }
        let allowed = self.tribute_return_cards(&state);
        if !allowed.contains(&card) {
            return Err(EngineError::IllegalAction {
                reason: format!("牌 {card} 不在 seat {seat} 的可还贡牌中"),
            });
        }
        self.move_card(&mut state, pending.receiver, pending.payer, card)?;
        state.tribute_events.push(TributeEventRecord {
            from: pending.receiver,
            to: pending.payer,
            card,
            is_return: true,
        });
        state.tribute_pending_returns.remove(0);
        self.bump_watermark(&mut state, seat);

        if let Some(next) = state.tribute_pending_returns.first().copied() {
            state.turn_seat = next.receiver;
            return Ok(state);
        }

        let leader = state
            .tribute_leader
            .ok_or_else(|| EngineError::Internal {
                message: "进贡完成却没有贡牌最大者".to_owned(),
            })?;
        self.transition(&mut state, PHASE_PLAY)?;
        state.turn_seat = leader;
        Ok(state)
    }

    /// 还贡选牌：`ceiling=under-ten` 取不大于 10 的牌；全大于 10 按 `fallback`
    /// （`smallest` / `largest`）兜底（§5.13 行 963）。
    fn select_return_card(
        &self,
        state: &State,
        seat: u8,
        cfg: &crate::ruleset::model::TributeConfig,
    ) -> Option<u16> {
        let hand = self.hand_cards(state, seat)?;
        let Some(ret) = cfg.return_.as_ref() else {
            return hand.first().copied();
        };
        let ten_index = self
            .ruleset
            .deck
            .ranks
            .iter()
            .position(|k| k.as_str() == "10")
            .unwrap_or(8);
        let under_ten: Vec<u16> = hand
            .iter()
            .copied()
            .filter(|c| match self.card_rank_index(*c) {
                Some(ri) => ri <= ten_index,
                None => false, // 王不参与 under-ten 兜底
            })
            .collect();
        let pool = match ret.ceiling {
            crate::ruleset::model::ReturnCeiling::UnderTen => {
                if under_ten.is_empty() {
                    hand.clone()
                } else {
                    under_ten
                }
            }
            crate::ruleset::model::ReturnCeiling::None => hand.clone(),
        };
        match ret.fallback {
            crate::ruleset::model::ReturnFallback::Smallest => pool
                .iter()
                .min_by_key(|&&c| (self.card_weight(c), u32::from(c)))
                .copied(),
            crate::ruleset::model::ReturnFallback::Largest => pool
                .iter()
                .max_by_key(|&&c| (self.card_weight(c), u32::from(u16::MAX - c)))
                .copied(),
        }
    }

    /// 把一张牌从 `from` 手牌区移到 `to` 手牌区；牌不在源区报 `Internal`。
    fn move_card(&self, state: &mut State, from: u8, to: u8, card: u16) -> Result<(), EngineError> {
        let idx = self.hand_zone_index();
        let from_pos = state
            .zones
            .iter()
            .position(|z| z.seat == from && z.zone_id == idx)
            .ok_or_else(|| EngineError::Internal {
                message: format!("seat {from} 手牌区缺失"),
            })?;
        let pos = state.zones[from_pos]
            .cards
            .iter()
            .position(|&c| c == card)
            .ok_or_else(|| EngineError::Internal {
                message: format!("牌 {card} 不在 seat {from} 手牌区"),
            })?;
        state.zones[from_pos].cards.remove(pos);
        let to_zone = state
            .zones
            .iter_mut()
            .find(|z| z.seat == to && z.zone_id == idx)
            .ok_or_else(|| EngineError::Internal {
                message: format!("seat {to} 手牌区缺失"),
            })?;
        to_zone.cards.push(card);
        Ok(())
    }

    /// 基础牌点数下标；王返回 `None`。
    fn card_rank_index(&self, card: u16) -> Option<usize> {
        if card as usize >= self.base_total() {
            return None;
        }
        let b = card as usize / self.copies();
        Some(b % self.rank_count())
    }

    /// 牌力权重：基础牌取 `naturalOrder` 位置，小王 / 大王居尾；用于选贡牌 / 还贡兜底比较。
    fn card_weight(&self, card: u16) -> u32 {
        match self.card_rank_index(card) {
            Some(ri) => {
                let key = &self.ruleset.deck.ranks[ri];
                self.ruleset
                    .natural_order
                    .iter()
                    .position(|k| k == key)
                    .unwrap_or(ri) as u32
            }
            None => {
                let base = self.base_total();
                let slot = (card as usize - base) / self.copies();
                let tail = self.ruleset.natural_order.len();
                (tail - 2 + slot) as u32
            }
        }
    }

    /// 每局每座发牌张数；不守恒直接按内部不变量报错。
    fn per_seat(&self, total: usize, seat_count: usize) -> Result<usize, EngineError> {
        let per_seat = self
            .ruleset
            .flow
            .as_ref()
            .and_then(|f| f.deal.as_ref())
            .and_then(|d| d.per_seat)
            .map(|p| p as usize)
            .unwrap_or(total / seat_count);
        if per_seat * seat_count != total {
            return Err(EngineError::Internal {
                message: format!(
                    "发牌张数不守恒: perSeat={per_seat} seatCount={seat_count} total={total}（{per_seat}×{seat_count}≠{total}）"
                ),
            });
        }
        Ok(per_seat)
    }

    /// Fisher–Yates 降序：`for i in (1..n).rev() { j = bounded(i+1); swap(a[i], a[j]) }`，
    /// j ∈ 0..=i（§5.2 第 558 行）。
    fn shuffle_deck(&self, deck: &mut [u16], rng: &mut RngState, seed: u64, stream: u32) {
        for i in (1..deck.len()).rev() {
            let j = Self::bounded(rng, seed, stream, i as u64 + 1) as usize;
            deck.swap(i, j);
        }
    }

    /// 按座位序连续切分牌堆到私有手牌区（弃牌区另置一条空 ZoneEntry）。
    fn deal_zones(&self, deck: &[u16], per_seat: usize) -> Vec<ZoneEntry> {
        let hand_idx = self.hand_zone_index();
        let mut zones = Vec::with_capacity(self.ruleset.seats.count as usize + 1);
        for seat in 0..self.ruleset.seats.count as usize {
            let start = seat * per_seat;
            zones.push(ZoneEntry {
                seat: seat as u8,
                zone_id: hand_idx,
                cards: deck[start..start + per_seat].to_vec(),
            });
        }
        if let Some(discard_idx) = self.discard_zone_index() {
            zones.push(ZoneEntry {
                seat: 0,
                zone_id: discard_idx,
                cards: Vec::new(),
            });
        }
        zones
    }

    /// 载入初始级牌下标：`levelTable.initial` 在 `ranks` 中的位置；无级牌表取占位 0。
    fn initial_levels(&self) -> Vec<u8> {
        let team_count = self.ruleset.seats.teams.len();
        match &self.ruleset.level_table {
            Some(table) => {
                let idx = table
                    .ranks
                    .iter()
                    .position(|rank| rank == &table.initial)
                    .map(|i| i as u8)
                    .unwrap_or(0);
                vec![idx; team_count]
            }
            None => vec![0; team_count],
        }
    }

    fn team_of(&self, seat: u8) -> u8 {
        self.ruleset
            .seats
            .teams
            .iter()
            .position(|team| team.contains(&seat))
            .unwrap_or(0) as u8
    }

    fn placeholder_tribute() -> TributeState {
        TributeState {
            from_seat: 0,
            to_seat: 0,
            card: None,
        }
    }

    /// `flow.rng.stream`（0–3，默认 0）；同一局内 `deal.shuffle` 与 `rng.pick` 共用同一条
    /// `(seed, stream, counter)` 序列，各自不从 0 重来（§5.2 第 560 行）。
    fn rng_stream(&self) -> u32 {
        self.ruleset
            .flow
            .as_ref()
            .and_then(|f| f.rng.as_ref())
            .and_then(|r| r.stream)
            .unwrap_or(0)
    }

    /// 取下一个随机字：`BLAKE3-256(seed_le64 ‖ stream_le32 ‖ counter_le64)` 前 8 字节按
    /// little-endian 解成 u64；输入是 20 字节定长拼接，不含其它字段（§5.2 第 555 行）。
    /// 每消耗一个随机字（含被拒绝的那次）`counter += 1`，单调不跳号；禁止批量预取、
    /// 禁止并行预取（§5.2 第 556 行）。
    fn next_word(rng: &mut RngState, seed: u64, stream: u32) -> u64 {
        let mut msg = [0u8; 20];
        msg[0..8].copy_from_slice(&seed.to_le_bytes());
        msg[8..12].copy_from_slice(&stream.to_le_bytes());
        msg[12..20].copy_from_slice(&rng.counter.to_le_bytes());
        let digest = blake3::hash(&msg);
        let mut bytes = [0u8; 8];
        bytes.copy_from_slice(&digest.as_bytes()[..8]);
        rng.counter += 1;
        u64::from_le_bytes(bytes)
    }

    /// 有界取值 `bounded(n)`（n ≥ 1）：`limit = ⌊2^64 / n⌋ * n`，随机字 `< limit` 才返回
    /// `word % n`，否则 `counter += 1` 取下一个字重试（拒绝采样，循环到取到为止；
    /// §5.2 第 557 行）；禁止直接用 `word % n` 回避模偏。
    fn bounded(rng: &mut RngState, seed: u64, stream: u32, n: u64) -> u64 {
        debug_assert!(n >= 1);
        let limit = ((1u128 << 64) / n as u128) * n as u128;
        loop {
            let word = Self::next_word(rng, seed, stream);
            if (word as u128) < limit {
                return word % n;
            }
        }
    }

    // ---------- 手牌 / 轮转 / 终止辅助 ----------

    fn hand_cards<'a>(&self, state: &'a State, seat: u8) -> Option<&'a Vec<u16>> {
        let hand_idx = self.hand_zone_index();
        state
            .zones
            .iter()
            .find(|z| z.seat == seat && z.zone_id == hand_idx)
            .map(|z| &z.cards)
    }

    fn bump_watermark(&self, state: &mut State, seat: u8) {
        if let Some(seq) = state.seq_watermark.iter_mut().find(|(s, _)| *s == seat) {
            seq.1 += 1;
        }
    }

    /// `flow.turn.skipFinished`（§5.2 第 529 行，默认 `true`）：轮转是否跳过已出完座位。
    fn skip_finished(&self) -> bool {
        self.ruleset
            .flow
            .as_ref()
            .and_then(|f| f.turn.as_ref())
            .and_then(|t| t.skip_finished)
            .unwrap_or(true)
    }

    /// `flow.turn.direction`（§5.2 第 527 行，默认 `clockwise`）：下一座位的轮转方向。
    fn turn_direction(&self) -> TurnDirection {
        self.ruleset
            .flow
            .as_ref()
            .and_then(|f| f.turn.as_ref())
            .and_then(|t| t.direction)
            .unwrap_or(TurnDirection::Clockwise)
    }

    /// `flow.turn.partnerLead`（§5.13 第 896 行、§5.10 第 783 行，默认 `true`）：接风开关。
    fn partner_lead(&self) -> bool {
        self.ruleset
            .flow
            .as_ref()
            .and_then(|f| f.turn.as_ref())
            .and_then(|t| t.partner_lead)
            .unwrap_or(true)
    }

    /// `flow.turn.afterAllPass.to`（§5.2 第 528 行，默认 `last-player`）：
    /// 其余座位连续 pass 完后的领出座位口径。
    fn after_all_pass(&self) -> AfterAllPass {
        self.ruleset
            .flow
            .as_ref()
            .and_then(|f| f.turn.as_ref())
            .and_then(|t| t.after_all_pass)
            .unwrap_or(AfterAllPass::LastPlayer)
    }

    /// 同队另一座位；无阵营或队伍只有一人时返回 `None`。
    fn teammate(&self, seat: u8) -> Option<u8> {
        self.ruleset
            .seats
            .teams
            .iter()
            .find(|team| team.contains(&seat))
            .and_then(|team| team.iter().find(|&&s| s != seat).copied())
    }

    /// 按 `flow.turn.direction` 推进到下一个座位；`skipFinished=true` 时跳过已出完座位。
    /// 座位数取 `seats.count`，不写死 4；全员出完时回原座兜底（不应走到）。
    fn next_seat(&self, state: &State, from: u8) -> u8 {
        let count = self.ruleset.seats.count;
        let skip = self.skip_finished();
        let step = match self.turn_direction() {
            TurnDirection::Clockwise => 1i32,
            TurnDirection::Counterclockwise => -1,
        };
        let mut seat = from;
        loop {
            seat = ((seat as i32 + step).rem_euclid(count as i32)) as u8;
            if !skip || !state.finish_order.contains(&seat) {
                return seat;
            }
            if seat == from {
                return from;
            }
        }
    }

    fn team_both_finished(&self, state: &State) -> bool {
        self.ruleset
            .seats
            .teams
            .iter()
            .any(|team| team.iter().all(|s| state.finish_order.contains(s)))
    }

    // ---------- legal_actions ----------

    /// 把枚举记录归并为最终动作列表：同 `patternId + patternKey` 只留一条规范代表，
    /// 代表选定序 ① rankKey 最大 ② 百搭用量最少 ③ 真牌牌 id 升序字典序最小；
    /// 再按四元组全序排序，跟牌态末尾追加 `Pass`。生产枚举与参照枚举共用本归并路径，
    /// 差分比较直接落在最终 `Vec<Action>`（含顺序）上。
    fn actions_from_records(
        &self,
        records: Vec<patterns::Recognized>,
        state: &State,
        force: bool,
        wild_ids: &HashSet<u16>,
        ladder: &[PatternId],
    ) -> Vec<Action> {
        struct Entry {
            count_key: u32,
            rank_key: u32,
            wild_count: usize,
            true_cards: Vec<u16>,
            cards: Vec<u16>,
            pattern_id: String,
            action: Action,
        }
        let mut groups: BTreeMap<(String, u32, u32), Entry> = BTreeMap::new();

        for rec in records {
            // 领出（lastPlay 为空）任意成型牌型合法；跟牌时：force=true 只保留能压过
            // lastPlay 的候选，force=false 只校牌型合法、不要求压过（§5.2 第 524 行）。
            let legal = match &state.last_play {
                None => true,
                Some(last) if force => patterns::beats(ladder, &self.ruleset, &rec, last),
                Some(_) => true,
            };
            if !legal {
                continue;
            }
            let mut true_cards: Vec<u16> = rec
                .cards
                .iter()
                .copied()
                .filter(|c| !wild_ids.contains(c))
                .collect();
            true_cards.sort_unstable();
            let wild_count = rec.cards.len() - true_cards.len();
            let key = (rec.pattern_id.clone(), rec.pattern_key.0, rec.pattern_key.1);
            let entry = Entry {
                count_key: rec.pattern_key.0,
                rank_key: rec.pattern_key.1,
                wild_count,
                true_cards,
                cards: rec.cards.clone(),
                pattern_id: rec.pattern_id.clone(),
                action: Action::Play { cards: rec.cards },
            };
            match groups.entry(key) {
                std::collections::btree_map::Entry::Occupied(mut slot) => {
                    let cur = slot.get();
                    let take = entry.rank_key > cur.rank_key
                        || (entry.rank_key == cur.rank_key && entry.wild_count < cur.wild_count)
                        || (entry.rank_key == cur.rank_key
                            && entry.wild_count == cur.wild_count
                            && entry.true_cards < cur.true_cards);
                    if take {
                        *slot.get_mut() = entry;
                    }
                }
                std::collections::btree_map::Entry::Vacant(slot) => {
                    slot.insert(entry);
                }
            }
        }

        let mut candidates: Vec<(u32, u32, Vec<u16>, String, Action)> = groups
            .into_values()
            .map(|e| (e.count_key, e.rank_key, e.cards, e.pattern_id, e.action))
            .collect();

        candidates.sort_by(|a, b| (&a.0, &a.1, &a.2, &a.3).cmp(&(&b.0, &b.1, &b.2, &b.3)));
        let mut out: Vec<Action> = candidates.into_iter().map(|c| c.4).collect();

        // 过牌：仅跟牌阶段合法（领出不能 pass）。Pass 不参与牌型四元组排序，排在所有 Play 之后。
        if state.last_play.is_some() {
            out.push(Action::Pass);
        }
        out
    }

    /// 本座位手牌（供房间层补推荐拆分的散张用；不进视图、不外发）。
    pub fn hand_cards_of(&self, state: &State, seat: u8) -> Option<Vec<u16>> {
        self.hand_cards(state, seat).cloned()
    }

    /// 单张牌的牌力（当前局合成牌力序，`levelInsert` 已生效，王最大）。
    ///
    /// bot 的「先走最小的」「散张压不压得上家」都按这个数比，而不是按原始点数猜：
    /// 级牌被提升到 A 之后，用原始点数比会把级牌当成小牌。
    pub fn card_power(&self, state: &State, card: u16) -> u32 {
        let ctx = self.rank_context(state);
        match self.card_rank_index(card) {
            Some(ri) => ctx.rank_key(&self.ruleset, ri),
            // 王不在 `deck.ranks` 里：小王小、大王大，统一排在所有基础牌之后。
            None => 1_000 + u32::from(card),
        }
    }

    /// 预检一手牌：按 `apply` 的同口径判定「这组牌此刻能否作为 Play 提交」，不推进状态。
    ///
    /// 返回 `(patternId, 能否压过上家)`；不在手牌 / 结构不成型 / 压不过上家都返回错误。
    /// 前端用它决定「出牌」按钮是否可点——`legal_actions` 只是候选清单，顺子、三带二这类
    /// 多组合牌型不会把用户手里的每个具体花色组合都枚举出来。
    pub fn preview_play(
        &self,
        state: &State,
        cards: &[u16],
    ) -> Result<(String, bool), EngineError> {
        if state.phase != PHASE_PLAY {
            return Err(EngineError::IllegalAction {
                reason: "当前不在 play 相位".to_owned(),
            });
        }
        if cards.is_empty() {
            return Err(EngineError::IllegalAction {
                reason: "未选牌".to_owned(),
            });
        }
        let seat = state.turn_seat;
        let Some(hand) = self.hand_cards(state, seat) else {
            return Err(EngineError::Internal {
                message: format!("seat {seat} 手牌区缺失"),
            });
        };
        let mut seen: Vec<u16> = Vec::with_capacity(cards.len());
        for &card in cards {
            if !hand.contains(&card) {
                return Err(EngineError::IllegalAction {
                    reason: format!("牌 {card} 不在 seat {seat} 的手牌中"),
                });
            }
            if seen.contains(&card) {
                return Err(EngineError::IllegalAction {
                    reason: format!("牌 {card} 重复提交"),
                });
            }
            seen.push(card);
        }
        let ctx = self.rank_context(state);
        let ladder = self.cached_ladder();
        let rec = patterns::recognize(ladder, &self.ruleset, &ctx, cards).ok_or_else(|| {
            EngineError::IllegalAction {
                reason: format!("seat {seat} 选中的牌不构成任何已声明牌型"),
            }
        })?;
        let force = self.require_legal_params().0;
        let beats = match (&state.last_play, force) {
            (Some(last), true) => patterns::beats(ladder, &self.ruleset, &rec, last),
            _ => true,
        };
        Ok((rec.pattern_id.clone(), beats))
    }

    fn legal_actions_impl(&self, state: &State) -> Result<Vec<Action>, EngineError> {
        if state.phase == PHASE_TRIBUTE {
            let cards = self.tribute_return_cards(state);
            return Ok(cards
                .into_iter()
                .map(|card| Action::TributeReturn { card })
                .collect());
        }
        let out: Vec<Action> = Vec::new();
        if state.phase != PHASE_PLAY {
            return Ok(out);
        }
        let seat = state.turn_seat;

        // 轮转不变量：turnSeat 不得指向已出完座位（skipFinished 应在推进时跳过）。
        if state.finish_order.contains(&seat) {
            return Err(EngineError::Internal {
                message: format!(
                    "turnSeat={seat} 已在 finishOrder 中，轮转未跳过已出完座位: {:?}",
                    state.finish_order
                ),
            });
        }

        let Some(hand) = self.hand_cards(state, seat) else {
            return Err(EngineError::Internal {
                message: format!("seat {seat} 手牌区缺失"),
            });
        };
        let hand = hand.clone();

        // 候选四元组：(countKey, rankKey, cards 牌 id 序列, patternId)，全序（§5.12 第 842 行）。
        let force = self.require_legal_params().0;
        let ctx = self.rank_context(state);
        let ladder = self.cached_ladder();
        let wild_ids = patterns::wildcard_ids(&self.ruleset, &ctx);
        let records = patterns::enumerate(ladder, &self.ruleset, &ctx, &hand, true);
        let out = self.actions_from_records(records, state, force, &wild_ids, ladder);
        // 接风（`flow.turn.partnerLead`）是出牌落子后的领出座位解析，不在候选枚举侧展开；
        // 判定见 `apply_impl` 的 Play 分支。

        // N15（待裁决）：文档未写「无合法动作且非终局」的处置口径；取保守读法——
        // 报 Internal，不 invent 自动过牌 / 自动终局。静默结束会把轮转 bug 伪装成正常终局。
        if out.is_empty() && !self.team_both_finished(state) {
            let hands: Vec<String> = state
                .seats
                .iter()
                .map(|s| {
                    let n = self.hand_cards(state, *s).map(|h| h.len()).unwrap_or(0);
                    format!("seat{s}={n}")
                })
                .collect();
            return Err(EngineError::Internal {
                message: format!(
                    "非终局态下合法动作为空: phase={} turnSeat={} finishOrder={:?} hands=[{}]",
                    state.phase,
                    seat,
                    state.finish_order,
                    hands.join(",")
                ),
            });
        }

        if out.len() > ACTION_QUOTA {
            return Err(EngineError::QuotaExceeded);
        }
        Ok(out)
    }

    // ---------- apply ----------

    fn apply_impl(&self, mut state: State, action: Action) -> Result<State, EngineError> {
        if state.phase == PHASE_TRIBUTE {
            return self.apply_tribute_return(state, action);
        }
        if state.phase != PHASE_PLAY {
            return Err(EngineError::IllegalAction {
                reason: "当前不在 play 相位".to_owned(),
            });
        }
        let seat = state.turn_seat;

        match action {
            Action::TributeReturn { .. } => {
                return Err(EngineError::IllegalAction {
                    reason: "还贡动作只能在 tribute 相位提交".to_owned(),
                });
            }
            Action::Play { cards } => {
                // §5.9 第 203 行：出牌只传牌 id，`patternId` 由服务端判定。复检按牌结构
                // 判定牌型并判定能否压过 lastPlay，不再要求提交的 cards 精确等于枚举清单。
                let ctx = self.rank_context(&state);
                let ladder = self.cached_ladder();
                let rec =
                    patterns::recognize(ladder, &self.ruleset, &ctx, &cards).ok_or_else(|| {
                        EngineError::IllegalAction {
                            reason: format!("seat {seat} 提交的牌不构成任何已声明牌型"),
                        }
                    })?;
                let force = self.require_legal_params().0;
                if force {
                    if let Some(last) = &state.last_play {
                        if !patterns::beats(ladder, &self.ruleset, &rec, last) {
                            return Err(EngineError::IllegalAction {
                                reason: format!(
                                    "seat {seat} 提交的牌压不过上家 {}（{:?}）",
                                    last.pattern_id, last.pattern_key
                                ),
                            });
                        }
                    }
                }

                // 扣牌区读 require-legal.consume（或 consume-cards.zone），不再写死 hand。
                let consume_zone = self.resolve_consume_zone()?;
                let consume_idx =
                    self.zone_index(&consume_zone)
                        .ok_or_else(|| EngineError::Internal {
                            message: format!("扣牌区 {} 未在 zones 声明", consume_zone.0),
                        })?;
                let hand = state
                    .zones
                    .iter_mut()
                    .find(|z| z.seat == seat && z.zone_id == consume_idx)
                    .ok_or_else(|| EngineError::Internal {
                        message: format!("seat {seat} 扣牌区 {} 缺失", consume_zone.0),
                    })?;
                for &card in &cards {
                    let pos = hand.cards.iter().position(|c| *c == card).ok_or_else(|| {
                        EngineError::IllegalAction {
                            reason: format!(
                                "牌 {card} 不在 seat {seat} 的 {} 区中",
                                consume_zone.0
                            ),
                        }
                    })?;
                    hand.cards.remove(pos);
                }

                // 牌移入公共弃牌区。
                if let Some(discard_idx) = self.discard_zone_index() {
                    if let Some(discard) = state.zones.iter_mut().find(|z| z.zone_id == discard_idx)
                    {
                        discard.cards.extend_from_slice(&cards);
                    }
                }

                // `last_play` 写服务端判定出的真实 `pattern_id` 与 `pattern_key`，
                // 不再硬写 `(1, rank_key(cards[0]))`（§5.12 第 797、842 行）。
                state.last_play = Some(LastPlay {
                    seat,
                    pattern_id: rec.pattern_id,
                    pattern_key: rec.pattern_key,
                    cards,
                });
                state.passes_since_last_play = 0;

                let hand_now = self.hand_cards(&state, seat).map(|h| h.len()).unwrap_or(0);
                if hand_now == 0 && !state.finish_order.contains(&seat) {
                    state.finish_order.push(seat);
                }
                self.bump_watermark(&mut state, seat);

                if self.team_both_finished(&state) {
                    self.settle(&mut state)?;
                } else {
                    // 出牌后先按正常轮转推进；接风在本墩全过后由 Pass 分支统一解析
                    // （第二十六次变更：出完本人是最后压牌者、其余仍有牌座位都过 → 搭档领出）。
                    state.turn_seat = self.next_seat(&state, seat);
                }
            }
            Action::Pass => {
                if state.last_play.is_none() {
                    return Err(EngineError::IllegalAction {
                        reason: "领出阶段不能过牌".to_owned(),
                    });
                }
                state.passes_since_last_play += 1;
                self.bump_watermark(&mut state, seat);

                let active: Vec<u8> = state
                    .seats
                    .iter()
                    .copied()
                    .filter(|s| !state.finish_order.contains(s))
                    .collect();
                // §5.13 第 877 行：其余座位连续 pass 完后，按 `flow.turn.afterAllPass.to` 领出。
                let all_passed =
                    state.passes_since_last_play as usize >= active.len().saturating_sub(1);
                if all_passed {
                    let last_seat = state.last_play.as_ref().unwrap().seat;
                    // N14：新墩起始 lastPlay 清空为 None（§5.12 第 819 行：本回合无人出过牌时该键不出现）。
                    state.last_play = None;
                    state.passes_since_last_play = 0;
                    // 判定顺序唯一（第二十六次变更）：① 先判接风——partnerLead 开启且最后
                    // 出牌者本人已出完，由其搭档领出；搭档亦已出完则沿方向顺延到下一个仍
                    // 有牌座位。② 接风不成立再读 afterAllPass：last-player 回最后出牌者；
                    // next-player 从最后出牌者起沿方向顺延，跳过已出完座位。
                    state.turn_seat =
                        if self.partner_lead() && state.finish_order.contains(&last_seat) {
                            match self.teammate(last_seat) {
                                Some(mate) if !state.finish_order.contains(&mate) => mate,
                                _ => self.next_seat(&state, last_seat),
                            }
                        } else {
                            match self.after_all_pass() {
                                AfterAllPass::LastPlayer => {
                                    if state.finish_order.contains(&last_seat) {
                                        self.next_seat(&state, last_seat)
                                    } else {
                                        last_seat
                                    }
                                }
                                AfterAllPass::NextPlayer => self.next_seat(&state, last_seat),
                            }
                        };
                } else {
                    state.turn_seat = self.next_seat(&state, seat);
                }
            }
        }

        Ok(state)
    }

    /// 名次落定（§5.13「局终止与名次」、第二十六次变更）：已出完按 `finish_order`；
    /// 未出完座位——只剩一个直接取末名次；全部同属一队（双下）时，从头游起沿
    /// `flow.turn.direction` 依次倒排：第一个遇到的未出完座位记末游，第二个记倒数第二。
    /// **禁止用剩余手牌张数排名次**；张数不进判定链。
    fn finalize_ranks(&self, state: &State) -> Vec<u8> {
        let mut ranks = state.finish_order.clone();
        let unfinished: Vec<u8> = state
            .seats
            .iter()
            .copied()
            .filter(|s| !ranks.contains(s))
            .collect();
        if unfinished.is_empty() {
            return ranks;
        }
        let count = self.ruleset.seats.count;
        let step = match self.turn_direction() {
            TurnDirection::Clockwise => 1i32,
            TurnDirection::Counterclockwise => -1,
        };
        let mut along: Vec<u8> = Vec::new();
        let mut cur = ranks[0];
        for _ in 0..count {
            if unfinished.contains(&cur) {
                along.push(cur);
            }
            cur = ((cur as i32 + step).rem_euclid(count as i32)) as u8;
        }
        // along 首遇 = 末游；倒序追加（倒数第二 → 末游）到名次序列。
        along.reverse();
        ranks.extend(along);
        ranks
    }

    /// 结算四步（§5.13 行 998，顺序固定）：① 定名次；② 判过 A；③ 未过 A 在 A 上累计
    /// `aFailStreak`、达限退回 initial；④ 非 A 按 step 推进 levels、跨 A 封顶。score-only
    /// 只跑 ①，levels/streak 保持 N18 常量占位。
    fn settle(&self, state: &mut State) -> Result<(), EngineError> {
        // §6.3：`play → settle`，经相位机校验，表外跳转报错不静默接受。
        self.transition(state, PHASE_SETTLE)?;

        // 终止判定用：先于名次补位，按终局瞬间的 finish_order 找全员已出完的队
        // （补位后两队都会进名次序列，不能事后再筛）。
        let completed_team = self
            .ruleset
            .seats
            .teams
            .iter()
            .enumerate()
            .find(|(_, team)| team.iter().all(|s| state.finish_order.contains(s)))
            .map(|(i, _)| i as u8)
            .ok_or_else(|| EngineError::Internal {
                message: "终局没有队伍全员进入名次".to_owned(),
            })?;

        // ① 定名次（第二十六次变更口径，不按手牌张数）。
        let ranks = self.finalize_ranks(state);
        state.finish_order = ranks.clone();
        state.ranks = ranks.clone();

        let head = *ranks.first().ok_or_else(|| EngineError::Internal {
            message: "终局名次序列为空".to_owned(),
        })?;
        let winner_team = self
            .ruleset
            .seats
            .teams
            .iter()
            .position(|t| t.contains(&head))
            .map(|i| i as u8)
            .ok_or_else(|| EngineError::Internal {
                message: format!("头游座位 {head} 未归属任何队伍"),
            })?;

        // rankCombo = 胜方队各名次升序；名次分配不影响 step（第二十六次变更 ⑤）。
        let mut combo: Vec<u8> = self.ruleset.seats.teams[winner_team as usize]
            .iter()
            .map(|&s| ranks.iter().position(|&x| x == s).map(|p| p as u8 + 1))
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| EngineError::Internal {
                message: "胜方队成员未全部进入名次序列".to_owned(),
            })?;
        combo.sort_unstable();

        // ④ 按 rank-combo 查 `upgrade[]` 取 step；查不到组合不报默认档，报 Internal。
        // score-only 局只跑第 ① 步，step 保持 None。
        let (table, step): (Option<&LevelTable>, Option<u8>) = match &self.ruleset.settle {
            Settle::LevelUp { .. } => {
                let table =
                    self.ruleset
                        .level_table
                        .as_ref()
                        .ok_or_else(|| EngineError::Internal {
                            message: "level-up 结算缺少 levelTable".to_owned(),
                        })?;
                let upgrade = table
                    .upgrade
                    .as_ref()
                    .ok_or_else(|| EngineError::Internal {
                        message: "level-up 结算缺少 upgrade[]".to_owned(),
                    })?;
                let step = upgrade
                    .iter()
                    .find(|row| row.team_ranks == combo)
                    .map(|row| row.step)
                    .ok_or_else(|| EngineError::Internal {
                        message: format!(
                            "rankCombo {combo:?} 未在 levelTable.upgrade[] 枚举，不许猜默认档"
                        ),
                    })?;
                (Some(table), Some(step))
            }
            Settle::ScoreOnly => (None, None),
        };

        state.winner_team = Some(winner_team);
        state.completed_team = Some(completed_team);
        state.settle_step = step;

        // ②③④ 队级牌推进（§5.13 结算四步，顺序固定不得调换）。score-only（N18）只跑
        // 第 ① 步定名次：levels 保持 initial 常量占位、aFailStreak 全队恒 0（deal 已初始化），
        // 二者照常进快照与 stateHash 第 2/9 段，不省略段、不用 0xFF；段集固定 10 段、
        // 与 RuleSet 无关。
        let mut passed_a = false;
        if let Some(table) = table {
            let a_index = table.ranks.len().saturating_sub(1) as u8;
            let initial_index = table
                .ranks
                .iter()
                .position(|rank| rank == &table.initial)
                .map(|i| i as u8)
                .unwrap_or(0);
            let level = state
                .levels
                .get(winner_team as usize)
                .copied()
                .unwrap_or(initial_index);
            let on_a = level == a_index;

            // ② 判过 A：该队 `levels=A` 且本方名次组合在 `pass.combos` 内即过 A，本场随即
            // 结束；计数不再使用、不参与后续判定，`levels` 保持 A。
            passed_a = on_a
                && self
                    .pass_combos()
                    .iter()
                    .any(|c| c.as_slice() == combo.as_slice());

            if passed_a {
                // 过 A：本场随即结束，`match_record` 载波在结算末段统一落 `state.match_end`。
            } else if on_a {
                // ③ 未过 A 且该队 `levels=A`：`aFailStreak += 1` 写入；写入后达到 `streakLimit`
                // 则退回 `levelTable.initial` 并清零；未达阈值保持 A 不退档。不在 A 上打的局
                // 不改动 streak（此时它恒为 0）。
                let limit = self.streak_limit();
                let next = state.a_fail_streak[winner_team as usize].saturating_add(1);
                if self.reset_on_fail() && next >= limit {
                    state.levels[winner_team as usize] = initial_index;
                    state.a_fail_streak[winner_team as usize] = 0;
                } else {
                    state.a_fail_streak[winner_team as usize] = next.min(3);
                }
            } else {
                // ④ 该队 `levels≠A`：按 step 推进 `levels`；跨 A 停在 A，超额升级数作废、不累积。
                let step = step.unwrap_or(0);
                state.levels[winner_team as usize] = level.saturating_add(step).min(a_index);
            }
        }

        // 场次终止判定（§5.13 行 1008）：① 过 A → `passed-a`；② 未过 A 且本局号触达
        // `matchCap.rounds` → `safety-cap`（胜方只比 `levels`，同档平局 `None`）。
        // `aborted` 在 v1 房间层无触发源，仅保留枚举与载波，不在此自动置位。
        if passed_a {
            state.match_end = Some(MatchEnd {
                winner_team: Some(winner_team),
                reason: MatchEndReason::PassedA,
            });
        } else if state.round_no >= self.match_cap_rounds() {
            state.match_end = Some(MatchEnd {
                winner_team: safety_cap_winner(&state.levels),
                reason: MatchEndReason::SafetyCap,
            });
        }

        // §5.13「决定首出」：settle 结束把 `attackingTeam` 置为头游所在队（下一局打谁的级牌）。
        state.attacking_team = winner_team;

        // N17（第三十五次变更）：转移成立那一刻清空 `lastPlay`，settle 全段保持 None；
        // 结算四步不读它。下一局 deal 后按新局重新累积。
        state.last_play = None;
        state.passes_since_last_play = 0;
        Ok(())
    }

    // ---------- view ----------

    fn view_impl(&self, state: &State, seat: Seat) -> View {
        let fields = self.ruleset.view.fields.clone();
        let mut data = ViewData::default();
        let hand_idx = self.hand_zone_index();
        if let Some(hand) = state
            .zones
            .iter()
            .find(|z| z.seat == seat.0 && z.zone_id == hand_idx)
        {
            data.zones
                .insert(self.hand_zone().0.clone(), hand.cards.clone());
        }
        // 队级状态：score-only 局 `seat.levels` 与两个 `round.*` 键一并不出现
        // （§5.12 字段级编码表缺值行；N18 只把 levels/streak 留在快照与哈希，不进视图白名单）。
        let level_up = matches!(&self.ruleset.settle, Settle::LevelUp { .. });
        if level_up && !state.levels.is_empty() {
            data.seat_levels = Some(state.levels.clone());
            data.round_level = state.levels.get(state.attacking_team as usize).copied();
            data.round_attacking_team = Some(state.attacking_team);
        }
        // N17（第三十五次变更）：本局已终局（phase=settle）与「本回合无人出过牌」同一
        // 条件，`lastPlay` 键都不出现；结算四步不读 lastPlay。
        if state.phase != PHASE_SETTLE {
            if let Some(last) = &state.last_play {
                data.last_play = Some(ViewLastPlay {
                    seat: last.seat,
                    pattern_id: last.pattern_id.clone(),
                    cards: last.cards.clone(),
                });
            }
        }
        let bytes = encode_canonical(&fields, &data);
        View { bytes }
    }
}

impl Engine for M0Engine {
    fn legal_actions(&self, state: &State) -> Result<Vec<Action>, EngineError> {
        self.legal_actions_impl(state)
    }

    fn apply(&self, state: State, action: Action) -> Result<State, EngineError> {
        self.apply_impl(state, action)
    }

    fn view(&self, state: &State, seat: Seat) -> View {
        self.view_impl(state, seat)
    }

    fn recommend(
        &self,
        state: &State,
        seat: Seat,
    ) -> Result<crate::recommend::Layout, EngineError> {
        let hand = self
            .hand_cards(state, seat.0)
            .ok_or_else(|| EngineError::Internal {
                message: format!("seat {} 手牌区缺失", seat.0),
            })?
            .clone();
        let ctx = self.rank_context(state);
        let ladder = self.cached_ladder();
        Ok(crate::recommend::recommend_layout(
            &self.ruleset,
            ladder,
            &ctx,
            &hand,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cases::M0_SEED;
    use crate::contract::{Engine, ErrorCode, Level};
    use crate::hash::{canonical_bytes, CanonicalState};
    use crate::ruleset::{from_yaml, validate};
    use std::fs;
    use std::path::PathBuf;

    fn engine() -> M0Engine {
        crate::cases::build_engine().expect("m0-minimal 夹具构造失败")
    }

    #[test]
    fn next_seat_skips_finished_seat() {
        let engine = engine();
        let state = engine.deal(M0_SEED).expect("发牌失败");

        let mut one_finished = state.clone();
        one_finished.finish_order.push(0);
        // 从 seat 3 顺时针推进，应跳过已出完的 seat 0，落到 seat 1。
        assert_eq!(engine.next_seat(&one_finished, 3), 1);

        let mut two_finished = state.clone();
        two_finished.finish_order = vec![0, 1];
        assert_eq!(engine.next_seat(&two_finished, 3), 2);
    }

    #[test]
    fn empty_legal_non_terminal_reports_internal() {
        let engine = engine();
        let mut state = engine.deal(M0_SEED).expect("发牌失败");
        let hand_idx = engine.hand_zone_index();
        // 摆非终局空手牌局面：清掉 seat 0 手牌但不记入 finishOrder，领出态无 lastPlay。
        state
            .zones
            .iter_mut()
            .find(|z| z.seat == 0 && z.zone_id == hand_idx)
            .expect("seat 0 手牌区")
            .cards
            .clear();
        state.turn_seat = 0;

        let err = engine
            .legal_actions(&state)
            .expect_err("非终局空候选必须报 Internal");
        match err {
            EngineError::Internal { .. } => {}
            other => panic!("应为 Internal，实际 {other:?}"),
        }
    }

    #[test]
    fn turn_seat_never_points_to_finished_during_run() {
        let engine = engine();
        let mut state = engine.deal(M0_SEED).expect("发牌失败");
        for _ in 0..10_000 {
            if state.phase == PHASE_SETTLE {
                break;
            }
            assert!(
                !state.finish_order.contains(&state.turn_seat),
                "turnSeat={} 不应在 finishOrder={:?} 中",
                state.turn_seat,
                state.finish_order
            );
            let legal = engine
                .legal_actions(&state)
                .expect("对局中途不应出现非终局空候选");
            let action = legal
                .into_iter()
                .next()
                .expect("非终局空候选应由引擎报 Internal");
            state = engine.apply(state, action).expect("落子失败");
        }
        assert_eq!(state.phase, PHASE_SETTLE, "应跑到 settle");
    }

    #[test]
    fn deal_rejects_non_conservative_per_seat() {
        // 把夹具 perSeat 27 改成 26：26×4=104 ≠ 108。L1 不拦（26 在 1–108 值域内），
        // 引擎发牌期必须报 Internal，而不是整除兜底静默截断、丢牌破 zones 守恒
        // （§5.4 `RS_ZONE_CONSERVATION` 口径）。
        let yaml = crate::cases::m0_ruleset_yaml().replace("perSeat: 27", "perSeat: 26");
        let body = from_yaml(&yaml).expect("修改后的夹具应能构造");
        let engine = M0Engine::new(body);
        let err = engine
            .deal(M0_SEED)
            .expect_err("perSeat×座位数≠总张数必须报 Internal");
        match err {
            EngineError::Internal { .. } => {}
            other => panic!("应为 Internal，实际 {other:?}"),
        }
    }

    #[test]
    fn m0_deal_keeps_initial_levels_placeholder_and_zero_streak() {
        let engine = engine();
        let state = engine.deal(M0_SEED).expect("发牌失败");
        // N18：score-only 局 levels 每队取 initial 常量占位（无级牌表即下标 0），
        // aFailStreak 全队恒 0；不省略段、不用 0xFF。
        assert_eq!(state.levels, vec![0, 0]);
        assert_eq!(state.a_fail_streak, vec![0, 0]);
        // 首局抽牌定先手；attacking_team = 先手座位所属队。
        let team = engine
            .ruleset
            .seats
            .teams
            .iter()
            .position(|t| t.contains(&state.turn_seat))
            .expect("先手座位应归属某一队");
        assert_eq!(state.attacking_team, team as u8);
    }

    #[test]
    fn score_only_settle_keeps_placeholder_and_fixed_segment_set() {
        // N18：score-only 跑到 settle 后 levels 仍为 initial 常量占位、streak 全队恒 0；
        // 二者照常进快照与 stateHash 第 2/9 段，段集固定 10 段、与 RuleSet 无关，不省略段、
        // 不用 0xFF。
        let outcome = crate::cases::run_m0_golden().expect("m0 golden 运行失败");
        assert_eq!(outcome.state.levels, vec![0, 0]);
        assert_eq!(outcome.state.a_fail_streak, vec![0, 0]);
        let canonical = CanonicalState::from(&outcome.state);
        let bytes = canonical_bytes(&canonical);
        // 第 2 段 levels 队位前缀 = 2；第 9 段 streak 队位前缀 = 2（队位数），均非省略。
        assert_eq!(outcome.state.levels.len(), 2);
        assert_eq!(outcome.state.a_fail_streak.len(), 2);
        assert!(!bytes.is_empty());
    }

    // ---------- 名次方向（第二十六次变更 ④） ----------

    #[test]
    fn double_loss_ranks_follow_direction_from_head() {
        let engine = engine();
        let mut state = engine.deal(M0_SEED).expect("发牌失败");
        // 队 0（座位 0、2）双下：0 头游、2 二游；未出完为队 1 的 seat1、seat3。
        state.finish_order = vec![0, 2];
        // 顺时针从头游 0 起：首遇未出完 seat1 = 末游（第 4 名），次遇 seat3 = 三游（第 3 名）。
        let ranks = engine.finalize_ranks(&state);
        assert_eq!(ranks, vec![0, 2, 3, 1]);
    }

    // ---------- levels 增量与封顶 A（§5.13 行 1004） ----------

    /// 构造 classic 终局前状态：覆写 `finish_order` / `levels` / `aFailStreak`，
    /// 供 settle 函数级用例（构造 State → 调 settle → 断言队级状态）。
    fn classic_settle_state(
        levels: Vec<u8>,
        streak: Vec<u8>,
        finish_order: Vec<u8>,
    ) -> (M0Engine, State) {
        let engine = crate::cases::classic_engine().expect("classic 构造失败");
        let mut state = engine.deal(M0_SEED).expect("classic 发牌失败");
        state.finish_order = finish_order;
        state.levels = levels;
        state.a_fail_streak = streak;
        (engine, state)
    }

    // ---------- 进贡 / 还贡 / 抗贡（§5.13 行 954–964） ----------

    /// 构造一局已结算的前驱局：`phase=settle`，名次序列按 `finish_order`（名次 1→n）。
    fn settled_prev(engine: &M0Engine, finish_order: Vec<u8>) -> State {
        let mut state = engine.deal(M0_SEED).expect("前驱局发牌失败");
        state.phase = PHASE_SETTLE;
        state.finish_order = finish_order;
        state
    }

    /// 替换指定座位手牌区（测试受控局面用）。
    fn set_hand(engine: &M0Engine, state: &mut State, seat: u8, cards: Vec<u16>) {
        let idx = engine.hand_zone_index();
        for zone in state.zones.iter_mut() {
            if zone.seat == seat && zone.zone_id == idx {
                zone.cards = cards.clone();
            }
        }
    }

    /// 黑桃基础牌（花色 0、副本 0），rank 为 `deck.ranks` 下标。
    fn tcard(engine: &M0Engine, rank: usize) -> u16 {
        patterns::card_id(&engine.ruleset, rank, 0, 0)
    }

    #[test]
    fn tribute_single_loss_pays_max_then_returns_under_ten() {
        // 名次 1=0、2=1、3=2、4=3；3、4 不同队 → 单贡：第 4 名 seat3 → 第 1 名 seat0。
        let engine = classic_engine();
        let prev = settled_prev(&engine, vec![0, 1, 2, 3]);
        let mut state = engine.build_round_state(&prev, M0_SEED).expect("续发骨架");
        // 贡方 seat3：3(1)、7(5)、K(11)；受贡方 seat0：2(0)、J(9)。
        set_hand(
            &engine,
            &mut state,
            3,
            vec![tcard(&engine, 1), tcard(&engine, 5), tcard(&engine, 11)],
        );
        set_hand(
            &engine,
            &mut state,
            0,
            vec![tcard(&engine, 0), tcard(&engine, 9)],
        );

        engine
            .resolve_tribute(&mut state, &prev)
            .expect("进贡");
        assert_eq!(state.phase, PHASE_TRIBUTE, "贡牌交完停在还贡相位");
        assert_eq!(state.turn_seat, 0, "受贡方先挑还贡牌");
        assert_eq!(state.tribute_events.len(), 1, "此时只记了进贡");
        assert_eq!(state.tribute_pending_returns.len(), 1);

        let pay = &state.tribute_events[0];
        assert_eq!((pay.from, pay.to, pay.is_return), (3, 0, false));
        assert_eq!(pay.card, tcard(&engine, 11), "贡牌 = 除百搭外点数最大（K）");

        state = engine
            .apply(
                state,
                Action::TributeReturn {
                    card: tcard(&engine, 0),
                },
            )
            .expect("受贡方还 2");

        assert_eq!(state.phase, PHASE_PLAY);
        assert_eq!(state.turn_seat, 3, "贡牌最大者（seat3）先出");
        assert!(!state.tribute_resisted);
        assert_eq!(state.tribute_events.len(), 2);

        let ret = &state.tribute_events[1];
        assert_eq!((ret.from, ret.to, ret.is_return), (0, 3, true));
        assert_eq!(ret.card, tcard(&engine, 0), "玩家可自选不大于 10 的牌（这里还 2）");

        assert_eq!(state.tribute.from_seat, 3);
        assert_eq!(state.tribute.to_seat, 0);
        assert_eq!(state.tribute.card, Some(tcard(&engine, 11)));
    }

    #[test]
    fn tribute_return_allows_any_under_ten_and_rejects_over_ten() {
        // 受贡方手里 2、5、9、J：还贡可选任意不大于 10 的牌，J 不合法。
        let engine = classic_engine();
        let prev = settled_prev(&engine, vec![0, 1, 2, 3]);
        let mut state = engine.build_round_state(&prev, M0_SEED).expect("续发骨架");
        set_hand(
            &engine,
            &mut state,
            3,
            vec![tcard(&engine, 11), tcard(&engine, 12)],
        );
        set_hand(
            &engine,
            &mut state,
            0,
            vec![
                tcard(&engine, 0),
                tcard(&engine, 3),
                tcard(&engine, 7),
                tcard(&engine, 9),
            ],
        );
        engine.resolve_tribute(&mut state, &prev).expect("进贡");

        let legal = engine.legal_actions(&state).expect("还贡候选");
        let allowed: Vec<u16> = legal
            .into_iter()
            .filter_map(|a| match a {
                Action::TributeReturn { card } => Some(card),
                _ => None,
            })
            .collect();
        assert!(allowed.contains(&tcard(&engine, 0)));
        assert!(allowed.contains(&tcard(&engine, 3)));
        assert!(allowed.contains(&tcard(&engine, 7)));
        assert!(!allowed.contains(&tcard(&engine, 9)), "J(9) 大于 10");

        let chosen = tcard(&engine, 7);
        state = engine
            .apply(state, Action::TributeReturn { card: chosen })
            .expect("自定义还贡 9");
        assert_eq!(state.tribute_events.last().unwrap().card, chosen);
        assert_eq!(state.phase, PHASE_PLAY);
    }


    #[test]
    fn tribute_resist_two_big_jokers_skips_whole_segment() {
        // 单贡：贡方 seat3 合计持 2 张大王 → 抗贡，进贡 / 还贡均不发生。
        let engine = classic_engine();
        let prev = settled_prev(&engine, vec![0, 1, 2, 3]);
        let mut state = engine.build_round_state(&prev, M0_SEED).expect("续发骨架");
        let big0 = patterns::joker_id(&engine.ruleset, true, 0);
        let big1 = patterns::joker_id(&engine.ruleset, true, 1);
        set_hand(&engine, &mut state, 3, vec![big0, big1]);

        engine.resolve_tribute(&mut state, &prev).expect("抗贡判定");

        assert!(state.tribute_resisted, "两张大王 → 抗贡成立");
        assert!(state.tribute_events.is_empty(), "抗贡不发 TributeEvent");
        assert_eq!(
            state.tribute,
            TributeState {
                from_seat: 0,
                to_seat: 0,
                card: None
            },
            "第 7 段取占位值"
        );
        assert_eq!(state.phase, PHASE_PLAY, "抗贡：deal → play 直转");
        assert_eq!(state.turn_seat, 0);
    }

    #[test]
    fn tribute_double_loss_pairs_positionally_without_crossing() {
        // 名次 1=1、2=3、3=0、4=2；3、4（seat0、seat2）同队 → 双下，对位进贡：
        // 第 4 名 seat2 → 第 1 名 seat1；第 3 名 seat0 → 第 2 名 seat3。
        let engine = classic_engine();
        let prev = settled_prev(&engine, vec![1, 3, 0, 2]);
        let mut state = engine.build_round_state(&prev, M0_SEED).expect("续发骨架");
        // payer seat2（名次4）：3(1)、K(11)；receiver seat1（名次1）：2(0)。
        set_hand(
            &engine,
            &mut state,
            2,
            vec![tcard(&engine, 1), tcard(&engine, 11)],
        );
        set_hand(&engine, &mut state, 1, vec![tcard(&engine, 0)]);
        // payer seat0（名次3）：7(5)、A(12)；receiver seat3（名次2）：2(0)。
        set_hand(
            &engine,
            &mut state,
            0,
            vec![tcard(&engine, 5), tcard(&engine, 12)],
        );
        set_hand(&engine, &mut state, 3, vec![tcard(&engine, 0)]);

        engine
            .resolve_tribute(&mut state, &prev)
            .expect("双下进贡");
        assert_eq!(state.phase, PHASE_TRIBUTE);
        assert_eq!(state.turn_seat, 1, "第一对受贡方先还贡");
        assert!(!state.tribute_resisted);
        assert_eq!(state.tribute_events.len(), 2, "两对先各交一张贡牌");
        assert_eq!(state.tribute_pending_returns.len(), 2);

        // 第一对：seat2 → seat1（K），seat1 还 2 回 seat2。
        assert_eq!(
            (
                state.tribute_events[0].from,
                state.tribute_events[0].to,
                state.tribute_events[0].card
            ),
            (2, 1, tcard(&engine, 11))
        );
        state = engine
            .apply(
                state,
                Action::TributeReturn {
                    card: tcard(&engine, 0),
                },
            )
            .expect("seat1 还贡");
        assert_eq!(state.phase, PHASE_TRIBUTE);
        assert_eq!(state.turn_seat, 3, "轮到第二对受贡方");
        assert_eq!(state.tribute_events.len(), 3);

        // 第二对：seat0 → seat3（A），seat3 还 2 回 seat0。
        assert_eq!(
            (
                state.tribute_events[1].from,
                state.tribute_events[1].to,
                state.tribute_events[1].card
            ),
            (0, 3, tcard(&engine, 12))
        );
        state = engine
            .apply(
                state,
                Action::TributeReturn {
                    card: tcard(&engine, 0),
                },
            )
            .expect("seat3 还贡");

        assert_eq!(state.phase, PHASE_PLAY);
        assert_eq!(state.turn_seat, 0, "贡牌最大者（A，seat0）先出");
        assert_eq!(state.tribute_events.len(), 4, "两对：各一贡一还");
        assert_eq!(
            (
                state.tribute_events[2].from,
                state.tribute_events[2].to,
                state.tribute_events[2].card
            ),
            (1, 2, tcard(&engine, 0))
        );
        assert_eq!(
            (
                state.tribute_events[3].from,
                state.tribute_events[3].to,
                state.tribute_events[3].card
            ),
            (3, 0, tcard(&engine, 0))
        );
        // 第 7 段单组：仅记第一对；第二对的哈希承载属待裁决冲突，此处不断言第二对入哈希。
        assert_eq!(state.tribute.from_seat, 2);
        assert_eq!(state.tribute.to_seat, 1);
        assert_eq!(state.tribute.card, Some(tcard(&engine, 11)));
    }

    /// 手牌快照（排序后比较，避免被实现细节里的牌序整理影响）。
    fn sorted_hand(engine: &M0Engine, state: &State, seat: u8) -> Vec<u16> {
        let mut cards = engine.hand_cards(state, seat).cloned().unwrap_or_default();
        cards.sort_unstable();
        cards
    }

    /// 全场手牌总张数（守恒校验用）。
    fn total_hand_len(engine: &M0Engine, state: &State) -> usize {
        (0..4u8)
            .map(|s| engine.hand_cards(state, s).map(|h| h.len()).unwrap_or(0))
            .sum()
    }

    #[test]
    fn tribute_single_loss_refreshes_both_hands_in_place() {
        // 进贡后「手牌刷新」：张数不变、贡牌离开贡方、还贡牌回到贡方，受贡方反之。
        let engine = classic_engine();
        let prev = settled_prev(&engine, vec![0, 1, 2, 3]);
        let mut state = engine.build_round_state(&prev, M0_SEED).expect("续发骨架");
        let payer = vec![tcard(&engine, 1), tcard(&engine, 5), tcard(&engine, 11)]; // 3、7、K
        let receiver = vec![tcard(&engine, 0), tcard(&engine, 9)]; // 2、J
        set_hand(&engine, &mut state, 3, payer.clone());
        set_hand(&engine, &mut state, 0, receiver.clone());
        let total_before = total_hand_len(&engine, &state);

        engine
            .resolve_tribute(&mut state, &prev)
            .expect("进贡");
        state = engine
            .apply(
                state,
                Action::TributeReturn {
                    card: tcard(&engine, 0),
                },
            )
            .expect("还贡 2");

        assert_eq!(
            total_before,
            total_hand_len(&engine, &state),
            "进贡不改变全场张数"
        );
        assert_eq!(
            sorted_hand(&engine, &state, 3),
            {
                let mut v = vec![tcard(&engine, 0), tcard(&engine, 1), tcard(&engine, 5)];
                v.sort_unstable();
                v
            },
            "贡方 3、7、K → 交出 K、收到还贡 2"
        );
        assert_eq!(
            sorted_hand(&engine, &state, 0),
            {
                let mut v = vec![tcard(&engine, 9), tcard(&engine, 11)];
                v.sort_unstable();
                v
            },
            "受贡方 2、J → 交出 2、收到贡牌 K"
        );
        // 事件里记的牌必须真的换了位置，而不是只写事件。
        assert!(engine
            .hand_cards(&state, 0)
            .expect("seat 0 手牌区")
            .contains(&state.tribute_events[0].card));
        assert!(!engine
            .hand_cards(&state, 3)
            .expect("seat 3 手牌区")
            .contains(&state.tribute_events[0].card));
    }

    #[test]
    fn tribute_resist_leaves_all_hands_untouched() {
        // 抗贡：整个进贡段跳过，各座位手牌与发牌结果逐张一致（视图刷新后不会少牌）。
        let engine = classic_engine();
        let prev = settled_prev(&engine, vec![0, 1, 2, 3]);
        let mut state = engine.build_round_state(&prev, M0_SEED).expect("续发骨架");
        let big0 = patterns::joker_id(&engine.ruleset, true, 0);
        let big1 = patterns::joker_id(&engine.ruleset, true, 1);
        set_hand(&engine, &mut state, 3, vec![big0, big1]);
        let before: Vec<Vec<u16>> = (0..4u8).map(|s| sorted_hand(&engine, &state, s)).collect();

        engine.resolve_tribute(&mut state, &prev).expect("抗贡判定");

        for seat in 0..4u8 {
            assert_eq!(
                sorted_hand(&engine, &state, seat),
                before[seat as usize],
                "抗贡后座位 {seat} 手牌应与发牌一致"
            );
        }
        assert!(state.tribute.card.is_none());
    }

    #[test]
    fn partner_lead_clears_finisher_hand_and_keeps_other_hands() {
        // 接风前的「手牌刷新」：出完者手牌清空，其余三家一张不动（含接风领出那一刻）。
        let engine = engine();
        let mut state = partner_lead_scenario(&engine);
        let before: Vec<Vec<u16>> = (0..4u8).map(|s| sorted_hand(&engine, &state, s)).collect();
        let card = before[0][0];
        state = engine
            .apply(state, Action::Play { cards: vec![card] })
            .expect("seat 0 出完");
        assert!(
            sorted_hand(&engine, &state, 0).is_empty(),
            "出完者手牌应清空"
        );
        for seat in 1..4u8 {
            assert_eq!(
                sorted_hand(&engine, &state, seat),
                before[seat as usize],
                "座位 {seat} 手牌不应因他人出完而变化"
            );
        }
        assert_eq!(state.turn_seat, 1, "出牌后仍按常规轮转");

        state = engine.apply(state, Action::Pass).expect("seat 1 过牌");
        state = engine.apply(state, Action::Pass).expect("seat 2 过牌");
        assert_eq!(state.turn_seat, 2, "全过后搭档接风领出");
        assert!(state.last_play.is_none());
        for seat in 1..4u8 {
            assert_eq!(
                sorted_hand(&engine, &state, seat),
                before[seat as usize],
                "接风解析不应改动任何在局座位的手牌"
            );
        }
    }

    #[test]
    fn tribute_disabled_skips_entire_segment() {
        // m0-minimal：`tribute.enabled=false`，deal → play 直转，无事件、无抗贡标记。
        let engine = engine();
        let prev = settled_prev(&engine, vec![0, 1, 2, 3]);
        let state = engine
            .deal_next_round(&prev, M0_SEED)
            .expect("禁用口径续发");
        assert_eq!(state.phase, PHASE_PLAY);
        assert!(state.tribute_events.is_empty());
        assert!(!state.tribute_resisted);
        assert_eq!(state.tribute.card, None);
    }

    // ---------- 场次终止（§5.13 行 1008、§6.5 `match_record`） ----------

    #[test]
    fn match_end_passed_a_carries_winner_and_reason() {
        // team0 在 A（下标 12）打 1+3（seat0 头游、seat2 三游）→ 过 A，`passed-a`。
        let (engine, mut state) = classic_settle_state(vec![12, 0], vec![0, 0], vec![0, 1, 2, 3]);
        state.round_no = 1;
        engine.settle(&mut state).expect("settle 失败");

        let end = state.match_end.expect("过 A 应落场次终止载波");
        assert_eq!(end.reason, MatchEndReason::PassedA);
        assert_eq!(end.reason.code_str(), "passed-a");
        assert_eq!(end.winner_team, Some(0));
    }

    #[test]
    fn match_end_safety_cap_compares_levels_only() {
        // 未过 A、局号触达上限 12 → `safety-cap`；胜方只比 levels（5 > 3 → team0）。
        let (engine, mut state) = classic_settle_state(vec![5, 3], vec![0, 0], vec![0, 1, 2, 3]);
        state.round_no = engine.match_cap_rounds();
        engine.settle(&mut state).expect("settle 失败");

        let end = state.match_end.expect("达上限应落 safety-cap 载波");
        assert_eq!(end.reason, MatchEndReason::SafetyCap);
        assert_eq!(end.reason.code_str(), "safety-cap");
        assert_eq!(end.winner_team, Some(0));
    }

    #[test]
    fn match_end_safety_cap_tie_is_draw_with_empty_winner() {
        // 同档平局：`winner_team` 为空；`aFailStreak` 不参与比较。平局纯判定见
        // `settlement::safety_cap_winner` 单测（升级后同档在 level-up 局难以构造，引擎侧
        // 只接载波，同档比较不重复实现）。
        let winner = safety_cap_winner(&[3, 3]);
        assert_eq!(winner, None, "同档记平局，胜方为空");
    }

    #[test]
    fn aborted_reason_is_carrier_only_without_engine_trigger() {
        // `aborted` v1 房间层无触发源：只验证枚举字面值与载波可构造，不在引擎内自动置位。
        let end = MatchEnd {
            winner_team: None,
            reason: MatchEndReason::Aborted,
        };
        assert_eq!(end.reason.code_str(), "aborted");
        assert_eq!(end.winner_team, None);
    }

    #[test]
    fn match_not_ended_below_cap_and_not_passed_a_keeps_open() {
        // 未过 A、局号未达上限：不终止，`match_end` 为空，可继续续发。
        let (engine, mut state) = classic_settle_state(vec![2, 0], vec![0, 0], vec![0, 1, 2, 3]);
        state.round_no = 1;
        engine.settle(&mut state).expect("settle 失败");
        assert!(state.match_end.is_none());
    }

    #[test]
    fn deal_next_round_rejected_after_match_ended() {
        // 场次终止后再续发必须被拒，不再开新局。
        let (engine, mut ended) = classic_settle_state(vec![12, 0], vec![0, 0], vec![0, 1, 2, 3]);
        ended.round_no = 1;
        engine.settle(&mut ended).expect("settle 失败");
        assert!(ended.match_end.is_some());

        let result = engine.deal_next_round(&ended, M0_SEED);
        assert!(
            matches!(result, Err(EngineError::Internal { .. })),
            "终止后续发应报 Internal"
        );
    }

    #[test]
    fn level_up_caps_at_a_with_excess_discarded() {
        // 队 0 在 K（下标 11）打 1+3 → step=2；跨 A 停在 A（下标 12），超额 1 档作废、不累积。
        let (engine, mut state) = classic_settle_state(vec![11, 0], vec![0, 0], vec![0, 1, 2, 3]);
        engine.settle(&mut state).expect("settle 失败");
        assert_eq!(state.levels, vec![12, 0], "到达 A 即停，超额升级数作废");
        assert_eq!(state.settle_step, Some(2));
        assert_eq!(state.a_fail_streak, vec![0, 0]);
    }

    #[test]
    fn level_up_steps_from_initial_within_table() {
        // 基线对照：initial（下标 0）打 1+3 → +2，落到下标 2；未触 A，streak 不动。
        let (engine, mut state) = classic_settle_state(vec![0, 0], vec![0, 0], vec![0, 1, 2, 3]);
        engine.settle(&mut state).expect("settle 失败");
        assert_eq!(state.levels, vec![2, 0]);
        assert_eq!(state.a_fail_streak, vec![0, 0]);
    }

    // ---------- 过 A 判定与 aFailStreak（§5.13 行 1006） ----------

    #[test]
    fn pass_a_combo_1_2_ends_match_and_keeps_level() {
        // 在 A（下标 12）打 1+2（双下）：算过 A；levels 保持 A，streak 不再使用。
        let (engine, mut state) = classic_settle_state(vec![12, 0], vec![0, 0], vec![0, 2, 1, 3]);
        engine.settle(&mut state).expect("settle 失败");
        assert_eq!(state.levels, vec![12, 0]);
        assert_eq!(state.a_fail_streak, vec![0, 0]);
        assert_eq!(state.winner_team, Some(0));
    }

    #[test]
    fn pass_a_combo_1_3_ends_match_and_keeps_level() {
        // 在 A 打 1+3：算过 A；levels 保持 A。
        let (engine, mut state) = classic_settle_state(vec![12, 0], vec![0, 0], vec![0, 1, 2, 3]);
        engine.settle(&mut state).expect("settle 失败");
        assert_eq!(state.levels, vec![12, 0]);
        assert_eq!(state.a_fail_streak, vec![0, 0]);
    }

    #[test]
    fn fail_a_combo_1_4_keeps_a_and_increments_streak() {
        // 在 A 打 1+4（队友末游）：不算过 A；levels 保持 A 不退档，streak 0 → 1。
        let (engine, mut state) = classic_settle_state(vec![12, 0], vec![0, 0], vec![0, 1, 3, 2]);
        engine.settle(&mut state).expect("settle 失败");
        assert_eq!(state.levels, vec![12, 0], "未过 A 不退档，保持 A");
        assert_eq!(state.a_fail_streak, vec![1, 0]);
    }

    #[test]
    fn fail_a_three_consecutive_rounds_returns_to_initial() {
        // 连续 3 局在 A 未过（1+4）：第 3 局写入后达到 streakLimit=3，levels 退回
        // initial（下标 0）、streak 清零。本用例逐局手工构造 State，只测结算纯函数；
        // 跨局携带由 `deal_next_round` 承担，不在此复现连续局。
        let finish = vec![0, 1, 3, 2];
        // 第 1 局：streak 0 → 1，仍在 A。
        let (engine, mut s1) = classic_settle_state(vec![12, 0], vec![0, 0], finish.clone());
        engine.settle(&mut s1).expect("第 1 局 settle 失败");
        assert_eq!(s1.levels, vec![12, 0]);
        assert_eq!(s1.a_fail_streak, vec![1, 0]);
        // 第 2 局：streak 1 → 2，仍在 A。
        let (_engine, mut s2) = classic_settle_state(vec![12, 0], vec![1, 0], finish.clone());
        engine.settle(&mut s2).expect("第 2 局 settle 失败");
        assert_eq!(s2.levels, vec![12, 0]);
        assert_eq!(s2.a_fail_streak, vec![2, 0]);
        // 第 3 局：streak 2 → 3，达阈值退回 initial 并清零。
        let (_engine, mut s3) = classic_settle_state(vec![12, 0], vec![2, 0], finish);
        engine.settle(&mut s3).expect("第 3 局 settle 失败");
        assert_eq!(s3.levels, vec![0, 0], "满 3 局未过，退回起点 2（下标 0）");
        assert_eq!(s3.a_fail_streak, vec![0, 0]);
    }

    // ---------- matchCap 安全局数上限（§5.13 行 1008） ----------

    #[test]
    fn match_cap_rounds_defaults_to_twelve_and_reads_fixture() {
        // m0-minimal 未声明 matchCap → 缺省 12；classic 夹具 rounds=12。
        assert_eq!(engine().match_cap_rounds(), 12);
        let classic = crate::cases::classic_engine().expect("classic 构造失败");
        assert_eq!(classic.match_cap_rounds(), 12);
    }

    // ---------- N17：settle 期视图屏蔽 lastPlay ----------

    #[test]
    fn settle_view_omits_last_play_key() {
        let engine = engine();
        let mut state = engine.deal(M0_SEED).expect("发牌失败");
        // 人为摆成终局相位、last_play 仍带值（模拟转移成立那一刻）；视图必须屏蔽该键。
        state.phase = PHASE_SETTLE;
        state.last_play = Some(LastPlay {
            seat: 0,
            pattern_id: "single".to_owned(),
            pattern_key: (1, 0),
            cards: vec![1],
        });
        let view = engine.view(&state, Seat(0));
        assert!(
            !view.bytes.windows(9).any(|w| w == b"lastPlay\""),
            "phase=settle 期间 lastPlay 键不出现：{}",
            String::from_utf8_lossy(&view.bytes)
        );
    }

    // ---------- 第一笔：轮转方向 / skipFinished / 全过后领出 / 接风 ----------

    fn m0_yaml() -> String {
        crate::cases::m0_ruleset_yaml().to_owned()
    }

    fn engine_from(yaml: &str) -> M0Engine {
        let body = from_yaml(yaml).expect("内联 YAML 构造 RuleSet 失败");
        M0Engine::new(body)
    }

    #[test]
    fn counterclockwise_rotates_to_previous_seat() {
        // §5.2 第 527–530 行：direction=counterclockwise 时逆时针推进。
        let yaml = m0_yaml().replace("direction: clockwise", "direction: counterclockwise");
        let engine = engine_from(&yaml);
        let mut state = engine.deal(M0_SEED).expect("发牌失败");
        assert_eq!(state.turn_seat, 0);
        let legal = engine.legal_actions(&state).expect("枚举合法动作失败");
        let action = legal.into_iter().next().expect("领出态应有候选");
        state = engine.apply(state, action).expect("落子失败");
        // 逆时针从 seat 0 落到 seat 3（顺时针会落到 seat 1）。
        assert_eq!(state.turn_seat, 3);
    }

    #[test]
    fn skip_finished_false_keeps_finished_seat_in_rotation() {
        // §5.2 第 529 行：skipFinished=false 时已出完座位仍参与轮转。
        let yaml = m0_yaml().replace("skipFinished: true", "skipFinished: false");
        let engine = engine_from(&yaml);
        let state = engine.deal(M0_SEED).expect("发牌失败");
        let mut one_finished = state.clone();
        one_finished.finish_order.push(0);
        // 从 seat 3 顺时针、不跳过已出完的 seat 0：回到 seat 0。
        assert_eq!(engine.next_seat(&one_finished, 3), 0);
    }

    /// 构造「最后压牌者 seat 2、轮到 seat 1、连续过牌中」的中局局面。
    fn all_passed_seed_state(engine: &M0Engine) -> State {
        let mut state = engine.deal(M0_SEED).expect("发牌失败");
        state.last_play = Some(LastPlay {
            seat: 0,
            pattern_id: "single".to_owned(),
            pattern_key: (1, 0),
            cards: vec![0],
        });
        state.turn_seat = 1;
        state.passes_since_last_play = 0;
        state
    }

    #[test]
    fn after_all_pass_next_player_rotates_off_last_player() {
        // §5.2 第 528 行：afterAllPass.to=next-player，三家连续 pass 后不回最后出牌者，
        // 按轮转方向落到下一座位。
        let yaml = m0_yaml().replace("afterAllPass: last-player", "afterAllPass: next-player");
        let engine = engine_from(&yaml);
        let mut state = all_passed_seed_state(&engine);
        // seat 1、2、3 连续 pass；第三家 pass 后全过。
        state = engine.apply(state, Action::Pass).expect("seat 1 过牌");
        assert_eq!(state.turn_seat, 2);
        state = engine.apply(state, Action::Pass).expect("seat 2 过牌");
        assert_eq!(state.turn_seat, 3);
        state = engine.apply(state, Action::Pass).expect("seat 3 过牌");
        // 顺时针从最后出牌者 seat 0 顺延到 seat 1。
        assert_eq!(state.turn_seat, 1);
        assert!(state.last_play.is_none());
    }

    #[test]
    fn after_all_pass_last_player_returns_to_last_player() {
        // 默认 last-player：全过后回到最后出牌者 seat 0。
        let engine = engine();
        let mut state = all_passed_seed_state(&engine);
        state = engine.apply(state, Action::Pass).expect("seat 1 过牌");
        state = engine.apply(state, Action::Pass).expect("seat 2 过牌");
        state = engine.apply(state, Action::Pass).expect("seat 3 过牌");
        assert_eq!(state.turn_seat, 0);
        assert!(state.last_play.is_none());
    }

    /// 接风局面：teams=[[0,2],[1,3]]，seat 0 只剩一张全场点数最大的单张；出完本人即本墩
    /// 最后压牌者，其搭档 seat 2 是否接风由 `partnerLead` 与全过后解析决定（第二十六次变更）。
    fn partner_lead_scenario(engine: &M0Engine) -> State {
        let mut state = engine.deal(M0_SEED).expect("发牌失败");
        let hand_idx = engine.hand_zone_index();
        let mut hands: Vec<Vec<u16>> = (0..4u8)
            .map(|s| {
                state
                    .zones
                    .iter()
                    .find(|z| z.seat == s && z.zone_id == hand_idx)
                    .map(|z| z.cards.clone())
                    .unwrap_or_default()
            })
            .collect();
        // 取全场可识别单张中点数键最大的一张留给 seat 0，保证能压过 (1,0)。
        let mut best: Option<(u32, u16)> = None;
        let ctx = engine.rank_context(&state);
        let ladder = engine.cached_ladder();
        for hand in &hands {
            for &card in hand {
                if let Some(rec) = patterns::recognize(ladder, &engine.ruleset, &ctx, &[card]) {
                    let key = rec.pattern_key.1;
                    if best.map(|(bk, _)| key > bk).unwrap_or(true) {
                        best = Some((key, card));
                    }
                }
            }
        }
        let best_card = best.expect("全场应有可识别单张").1;
        for hand in &mut hands {
            hand.retain(|&c| c != best_card);
        }
        hands[0] = vec![best_card];
        if hands[2].len() > 2 {
            hands[2].truncate(2);
        }
        for seat in 0..4u8 {
            let zone = state
                .zones
                .iter_mut()
                .find(|z| z.seat == seat && z.zone_id == hand_idx)
                .expect("手牌区应存在");
            zone.cards = hands[seat as usize].clone();
        }
        let mate_card = hands[2][0];
        state.last_play = Some(LastPlay {
            seat: 2,
            pattern_id: "single".to_owned(),
            pattern_key: (1, 0),
            cards: vec![mate_card],
        });
        state.turn_seat = 0;
        state.passes_since_last_play = 0;
        state
    }

    #[test]
    fn partner_lead_true_makes_mate_lead_next_round() {
        // 第二十六次变更：出完最后一手牌的本人是最后压牌者，其余仍有牌座位都过 → 搭档
        // 领出；接风在全过瞬间解析，出牌后先按正常轮转，不直接跳搭档。
        let engine = engine(); // partnerLead: true
        let mut state = partner_lead_scenario(&engine);
        let card = state
            .zones
            .iter()
            .find(|z| z.seat == 0 && z.zone_id == engine.hand_zone_index())
            .expect("seat 0 手牌区")
            .cards[0];
        state = engine
            .apply(state, Action::Play { cards: vec![card] })
            .expect("seat 0 出完");
        assert!(state.finish_order.contains(&0));
        // 出牌后先按正常轮转落到 seat 1。
        assert_eq!(state.turn_seat, 1);
        // seat 1、seat 2 连续过牌后全过；last_seat=0 已出完，搭档 seat 2 接风领出。
        state = engine.apply(state, Action::Pass).expect("seat 1 过牌");
        assert_eq!(state.turn_seat, 2);
        state = engine.apply(state, Action::Pass).expect("seat 2 过牌");
        assert_eq!(state.turn_seat, 2, "全过后搭档 seat 2 接风领出");
        assert!(state.last_play.is_none());
    }

    #[test]
    fn partner_lead_false_rotates_past_mate() {
        // partnerLead: false 时不接风，出牌后按常规轮转到 seat 1。
        let yaml = m0_yaml().replace("partnerLead: true", "partnerLead: false");
        let engine = engine_from(&yaml);
        let mut state = partner_lead_scenario(&engine);
        let card = state
            .zones
            .iter()
            .find(|z| z.seat == 0 && z.zone_id == engine.hand_zone_index())
            .expect("seat 0 手牌区")
            .cards[0];
        state = engine
            .apply(state, Action::Play { cards: vec![card] })
            .expect("seat 0 出完");
        assert!(state.finish_order.contains(&0));
        assert_eq!(state.turn_seat, 1);
    }

    /// 构造全过解析局面：`last` 为本墩最后出牌者，`finished` 为已出完座位，轮到 `turn`。
    fn pass_resolution_state(engine: &M0Engine, last: u8, finished: &[u8], turn: u8) -> State {
        let mut state = engine.deal(M0_SEED).expect("发牌失败");
        state.finish_order = finished.to_vec();
        state.last_play = Some(LastPlay {
            seat: last,
            pattern_id: "single".to_owned(),
            pattern_key: (1, 0),
            cards: vec![0],
        });
        state.turn_seat = turn;
        state.passes_since_last_play = 0;
        state
    }

    #[test]
    fn partner_lead_skipped_when_last_presser_still_active() {
        // 最后压牌者本人仍有牌（期间有人压过）→ 接风不成立；默认 last-player 回最后出牌者，
        // 不落到其搭档。teams=[[0,2],[1,3]]，last=1，搭档应为 seat 3。
        let engine = engine();
        let mut state = pass_resolution_state(&engine, 1, &[], 2);
        for seat in [2u8, 3, 0] {
            let msg = format!("seat {seat} 过牌");
            state = engine.apply(state, Action::Pass).expect(&msg);
        }
        assert_eq!(state.turn_seat, 1, "last-player 回最后出牌者 seat 1");
        assert_ne!(
            state.turn_seat, 3,
            "last_presser 未出完，不接风给搭档 seat 3"
        );
        assert!(state.last_play.is_none());
    }

    #[test]
    fn partner_lead_takes_precedence_over_next_player() {
        // 判定顺序：接风优先于 afterAllPass。配置 next-player 时，最后出牌者已出完仍由搭档
        // 接风；只有接风不成立才按 next-player 顺延。
        let yaml = m0_yaml().replace("afterAllPass: last-player", "afterAllPass: next-player");
        let engine = engine_from(&yaml);
        let mut state = pass_resolution_state(&engine, 0, &[0], 1);
        state = engine.apply(state, Action::Pass).expect("seat 1 过牌");
        assert_eq!(state.turn_seat, 2);
        state = engine.apply(state, Action::Pass).expect("seat 2 过牌");
        assert_eq!(
            state.turn_seat, 2,
            "接风成立时 next-player 让位，搭档 seat 2 领出"
        );
        assert!(state.last_play.is_none());
    }

    // ---------- 第二笔：require-legal force / consume 与消耗区 ----------

    fn zone_index_of(engine: &M0Engine, zone_id: &str) -> u16 {
        engine
            .ruleset
            .zones
            .iter()
            .position(|z| z.id.0 == zone_id)
            .expect("测试区内应声明该区号") as u16
    }

    #[test]
    fn force_false_allows_unbeaten_play_force_true_rejects() {
        // §5.2 第 524 行：force=false 只校牌型合法、不要求压过；force=true 必须能压过。
        let loose = engine_from(&m0_yaml().replace("force: true", "force: false"));
        let mut state = loose.deal(M0_SEED).expect("发牌失败");
        // lastPlay 取大王单张（点力最高），seat 0 手里任一普通单张都压不过。
        let big_joker = patterns::joker_id(&loose.ruleset, true, 0);
        state.last_play = Some(LastPlay {
            seat: 1,
            pattern_id: "single".to_owned(),
            pattern_key: (1, 13),
            cards: vec![big_joker],
        });
        state.turn_seat = 0;
        // 受控手牌：首局抽牌前置后洗牌序列已变，手牌内容不能依赖随机发牌结果；
        // 固定放一张普通 2，确保两手引擎（force=false / force=true）判定的都是低牌。
        let low = patterns::card_id(&loose.ruleset, 0, 0, 0);
        let hand_idx = loose.hand_zone_index();
        state
            .zones
            .iter_mut()
            .find(|z| z.seat == 0 && z.zone_id == hand_idx)
            .expect("seat 0 手牌区")
            .cards = vec![low];

        // force=false：枚举保留该压不过的单张，落子成功。
        let legal = loose.legal_actions(&state).expect("枚举合法动作失败");
        assert!(
            legal
                .iter()
                .any(|a| matches!(a, Action::Play { cards } if cards == &[low])),
            "force=false 应允许压不过的成型单张"
        );
        let after = loose
            .apply(state.clone(), Action::Play { cards: vec![low] })
            .expect("force=false 压不过也应能出");
        assert_eq!(after.turn_seat, 1);

        // force=true：同一手压不过，枚举不给 Play，落子被拒。
        let strict = engine();
        let mut state2 = state.clone();
        state2.last_play = state.last_play.clone();
        let legal2 = strict.legal_actions(&state2).expect("枚举合法动作失败");
        assert!(
            legal2.iter().all(|a| !matches!(a, Action::Play { .. })),
            "force=true 不应给出压不过的候选"
        );
        let err = strict
            .apply(state2, Action::Play { cards: vec![low] })
            .expect_err("force=true 压不过必须拒绝");
        match err {
            EngineError::IllegalAction { .. } => {}
            other => panic!("应为 IllegalAction，实际 {other:?}"),
        }
    }

    #[test]
    fn consume_zone_takes_cards_from_declared_zone() {
        // §5.2 第 524 行：consume 指向非 hand 区时，从该区扣牌，手牌区不动。
        let yaml = m0_yaml()
            .replace("consume: hand", "consume: stash")
            .replace(
                "  - { id: discard, scope: shared,  capacity: -1 }",
                "  - { id: stash,   scope: private, capacity: 27 }\n  - { id: discard, scope: shared,  capacity: -1 }",
            );
        let engine = engine_from(&yaml);
        let mut state = engine.deal(M0_SEED).expect("发牌失败");
        let hand_idx = engine.hand_zone_index();
        let stash_idx = zone_index_of(&engine, "stash");

        // 从 seat 0 手牌移一张到 stash 区，stash 区条目手动补齐（发牌只落 hand）。
        let card = state
            .zones
            .iter_mut()
            .find(|z| z.seat == 0 && z.zone_id == hand_idx)
            .expect("seat 0 手牌区")
            .cards
            .remove(0);
        state.zones.push(ZoneEntry {
            seat: 0,
            zone_id: stash_idx,
            cards: vec![card],
        });
        let hand_before = state
            .zones
            .iter()
            .find(|z| z.seat == 0 && z.zone_id == hand_idx)
            .expect("seat 0 手牌区")
            .cards
            .len();

        // 领出态 lastPlay=None，直接出 stash 里这张。
        state = engine
            .apply(state, Action::Play { cards: vec![card] })
            .expect("从 stash 扣牌应成功");

        let stash_left = state
            .zones
            .iter()
            .find(|z| z.seat == 0 && z.zone_id == stash_idx)
            .expect("seat 0 stash 区")
            .cards
            .len();
        let hand_after = state
            .zones
            .iter()
            .find(|z| z.seat == 0 && z.zone_id == hand_idx)
            .expect("seat 0 手牌区")
            .cards
            .len();
        assert_eq!(stash_left, 0, "牌应从 stash 扣走");
        assert_eq!(hand_after, hand_before, "手牌区不应被动");
        let total: usize = state.zones.iter().map(|z| z.cards.len()).sum();
        assert_eq!(total, 108, "终局牌数守恒");
    }

    #[test]
    fn consume_cards_count_zero_takes_played_card_count() {
        // §5.2 第 525 行：consume-cards count=0 按本次出牌张数扣牌。
        let yaml = m0_yaml().replace(
            "  - { kind: pass, primitive: action.pass, legalFrom: play, params: {} }",
            "  - { kind: pass, primitive: action.pass, legalFrom: play, params: {} }\n  - { kind: play, primitive: action.consume-cards, legalFrom: play, params: { zone: hand, count: 0 } }",
        );
        let engine = engine_from(&yaml);
        let mut state = engine.deal(M0_SEED).expect("发牌失败");
        let hand_idx = engine.hand_zone_index();
        let discard_idx = engine.discard_zone_index().expect("应有弃牌区");
        let legal = engine.legal_actions(&state).expect("枚举合法动作失败");
        let action = legal.into_iter().next().expect("领出态应有候选");
        let cards = match &action {
            Action::Play { cards } => cards.clone(),
            Action::Pass => unreachable!("领出候选不含 Pass"),
            Action::TributeReturn { .. } => unreachable!("play 相位候选不含还贡"),
        };
        let hand_before = state
            .zones
            .iter()
            .find(|z| z.seat == 0 && z.zone_id == hand_idx)
            .expect("seat 0 手牌区")
            .cards
            .len();
        let discard_before = state
            .zones
            .iter()
            .find(|z| z.zone_id == discard_idx)
            .expect("弃牌区")
            .cards
            .len();

        state = engine.apply(state, action).expect("count=0 按出牌张数扣牌");

        let hand_after = state
            .zones
            .iter()
            .find(|z| z.seat == 0 && z.zone_id == hand_idx)
            .expect("seat 0 手牌区")
            .cards
            .len();
        let discard_after = state
            .zones
            .iter()
            .find(|z| z.zone_id == discard_idx)
            .expect("弃牌区")
            .cards
            .len();
        assert_eq!(hand_before - hand_after, cards.len(), "手牌按出牌张数扣减");
        assert_eq!(
            discard_after - discard_before,
            cards.len(),
            "弃牌区按出牌张数增加"
        );
        let total: usize = state.zones.iter().map(|z| z.cards.len()).sum();
        assert_eq!(total, 108, "牌数守恒");
    }

    #[test]
    fn consume_cards_count_positive_rejected_at_l1() {
        // 第二十七次变更：v1 `consume-cards.count` 只允许 0；count>0 在载入期即
        // RS_SCHEMA_INVALID，不进运行期。固定消耗 N 张（哪几张 + 候选形态）属 v2 预留。
        let yaml = m0_yaml().replace(
            "  - { kind: pass, primitive: action.pass, legalFrom: play, params: {} }",
            "  - { kind: pass, primitive: action.pass, legalFrom: play, params: {} }\n  - { kind: play, primitive: action.consume-cards, legalFrom: play, params: { zone: hand, count: 1 } }",
        );
        let body = from_yaml(&yaml).expect("YAML 反序列化应成功，交由 L1 闸门判定");
        let findings = validate(&body, &[Level::L1]).findings;
        let hit = findings
            .iter()
            .any(|f| f.code == ErrorCode::RsSchemaInvalid && f.location.contains("count"));
        assert!(
            hit,
            "count>0 应在 L1 载入期报 RS_SCHEMA_INVALID，不进运行期: {findings:?}"
        );
    }

    #[test]
    fn phase_transition_allows_declared_edges_only() {
        let engine = engine_from(&m0_yaml());
        let mut state = engine.deal(M0_SEED).expect("发牌失败");
        // `deal → play`：tribute 整段跳过，开局即进入 play。
        assert_eq!(state.phase, PHASE_PLAY);

        // 正向：`play → settle` 允许。
        engine
            .transition(&mut state, PHASE_SETTLE)
            .expect("play→settle 应在表内允许");
        assert_eq!(state.phase, PHASE_SETTLE);

        // 表内其余已声明边（执行路径待对应阶段落地，先收白名单）。
        assert!(M0Engine::allowed_transition(PHASE_DEAL, PHASE_PLAY));
        assert!(M0Engine::allowed_transition(PHASE_DEAL, PHASE_TRIBUTE));
        assert!(M0Engine::allowed_transition(PHASE_TRIBUTE, PHASE_PLAY));
        assert!(M0Engine::allowed_transition(PHASE_SETTLE, PHASE_DEAL));
    }

    #[test]
    fn illegal_phase_transition_rejected_not_silently_accepted() {
        let engine = engine_from(&m0_yaml());
        let mut state = engine.deal(M0_SEED).expect("发牌失败");
        // 当前处于 play：回退到 deal / tribute 均必须报错，不静默接受。
        for to in [PHASE_DEAL, PHASE_TRIBUTE] {
            let err = engine
                .transition(&mut state, to)
                .expect_err("相位回退必须报错");
            assert!(matches!(err, EngineError::Internal { .. }));
        }
        // 表外跳相 / 回退组合：deal→settle、tribute→settle、settle→play、play→deal。
        let rejected = [
            (PHASE_DEAL, PHASE_SETTLE),
            (PHASE_TRIBUTE, PHASE_SETTLE),
            (PHASE_SETTLE, PHASE_PLAY),
            (PHASE_PLAY, PHASE_DEAL),
        ];
        for (from, to) in rejected {
            assert!(
                !M0Engine::allowed_transition(from, to),
                "{from}→{to} 应在表外"
            );
            let mut staged = state.clone();
            staged.phase = from;
            let err = engine
                .transition(&mut staged, to)
                .expect_err("表外相位跳转必须报错");
            assert!(matches!(err, EngineError::Internal { .. }));
        }
    }

    // ---------- enum-path-rewrite 差分测试：新旧枚举逐条相等（含顺序） ----------

    fn classic_engine() -> M0Engine {
        let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        p.push("tests/fixtures/guandan-classic/ruleset.yaml");
        let src = fs::read_to_string(p).expect("classic 夹具读取失败");
        let body = from_yaml(&src).expect("classic 夹具构造失败");
        M0Engine::new(body)
    }

    fn classic_engine_copies(copies: u8) -> M0Engine {
        let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        p.push("tests/fixtures/guandan-classic/ruleset.yaml");
        let src = fs::read_to_string(p).expect("classic 夹具读取失败");
        let total = (13 * 4 + 2) * copies as usize;
        let per_seat = total / 4;
        let src = src
            .replace("copies: 2", &format!("copies: {copies}"))
            .replace("perSeat: 27", &format!("perSeat: {per_seat}"))
            .replace("cardCount: 108", &format!("cardCount: {total}"));
        let body = from_yaml(&src).expect("变体夹具构造失败");
        M0Engine::new(body)
    }

    fn diff_card(r: &RuleSetBody, rank: usize, suit: usize, copy: u8) -> u16 {
        patterns::card_id(r, rank, suit, copy)
    }

    fn diff_wild(r: &RuleSetBody, copy: u8) -> u16 {
        diff_card(r, 0, 1, copy)
    }

    fn diff_state_with(
        engine: &M0Engine,
        seat: u8,
        hand: Vec<u16>,
        last: Option<LastPlay>,
    ) -> State {
        let mut state = engine.deal(M0_SEED).expect("发牌失败");
        let idx = engine.hand_zone_index();
        for z in state.zones.iter_mut() {
            if z.seat == seat && z.zone_id == idx {
                z.cards = hand.clone();
            }
        }
        state.turn_seat = seat;
        state.last_play = last;
        state.passes_since_last_play = 0;
        state
    }

    /// 生产枚举与参照枚举走同一归并路径，最终 `Vec<Action>`（含顺序）逐条比对。
    fn assert_enum_equivalent(engine: &M0Engine, state: &State, hand: &[u16]) {
        let new_actions = engine
            .legal_actions(state)
            .expect("生产路径 legal_actions 应成功");
        let ctx = engine.rank_context(state);
        let ladder = engine.cached_ladder();
        let wild_ids = patterns::wildcard_ids(&engine.ruleset, &ctx);
        let force = engine.require_legal_params().0;
        let legacy = patterns::legacy_ref::enumerate_legacy(&engine.ruleset, &ctx, hand);
        let legacy_actions = engine.actions_from_records(legacy, state, force, &wild_ids, ladder);
        assert_eq!(
            new_actions, legacy_actions,
            "新旧枚举不一致: hand={hand:?} last={:?}",
            state.last_play
        );
    }

    /// 同花顺候选差分锁（第三十次变更「同输出」纪律）：`patterns::straight_flush_candidates`
    /// （按花色 × 窗口构造）必须与 `enumerate` 全量枚举的同花顺子集**逐条相同**。
    fn assert_flush_candidates_equivalent(engine: &M0Engine, state: &State, hand: &[u16]) {
        use crate::ruleset::shape_plan::BombRole;
        let ctx = engine.rank_context(state);
        let ladder = engine.cached_ladder();
        let mut reference: Vec<Vec<u16>> =
            patterns::enumerate(ladder, &engine.ruleset, &ctx, hand, false)
                .into_iter()
                .filter(|rec| rec.role == BombRole::Flush)
                .map(|rec| rec.cards)
                .collect();
        reference.sort();
        reference.dedup();
        let constructed = patterns::straight_flush_candidates(ladder, &engine.ruleset, &ctx, hand);
        assert_eq!(constructed, reference, "同花顺候选不一致: hand={hand:?}");
    }

    #[test]
    fn straight_flush_candidates_match_enumerate() {
        let engine = classic_engine();
        let r = &engine.ruleset;
        let idx = engine.hand_zone_index();

        // ① 随机发牌：64 种子 × 4 座位 = 256 手。
        let mut checked = 0usize;
        for seed in 0..64u64 {
            let state = engine
                .deal(M0_SEED ^ seed.wrapping_mul(0x9e37_79b9))
                .expect("发牌失败");
            for seat in 0..4u8 {
                let hand: Vec<u16> = state
                    .zones
                    .iter()
                    .find(|z| z.seat == seat && z.zone_id == idx)
                    .map(|z| z.cards.clone())
                    .unwrap_or_default();
                assert_flush_candidates_equivalent(&engine, &state, &hand);
                checked += 1;
            }
        }
        assert_eq!(checked, 256);

        // ② 定向手牌：同花成窗、缺孔靠百搭补、同点多副本、百搭本身在窗内、整手全百搭。
        let cards_of = |spec: &[(usize, usize, u8)]| -> Vec<u16> {
            spec.iter()
                .map(|&(rank, suit, copy)| diff_card(r, rank, suit, copy))
                .collect()
        };
        let wild_only: Vec<u16> = (0..2u8).map(|c| diff_wild(r, c)).collect();
        let cases: Vec<Vec<u16>> = vec![
            // ♠3-4-5-6 缺 7：7 由百搭（♥2 级牌）补。
            cards_of(&[(1, 0, 0), (2, 0, 0), (3, 0, 0), (4, 0, 0), (0, 1, 0)]),
            // ♠3-4-5-6-7 双副本齐全。
            cards_of(&[
                (1, 0, 0),
                (1, 0, 1),
                (2, 0, 0),
                (2, 0, 1),
                (3, 0, 0),
                (3, 0, 1),
                (4, 0, 0),
                (4, 0, 1),
                (5, 0, 0),
                (5, 0, 1),
            ]),
            // 四个花色各一段，逼出跨花色择优。
            cards_of(&[
                (1, 0, 0),
                (2, 0, 0),
                (3, 1, 0),
                (4, 1, 0),
                (5, 2, 0),
                (6, 2, 0),
                (7, 3, 0),
                (8, 3, 0),
            ]),
            // 级牌（♥2）本身落在窗内 + 另有一张百搭。
            cards_of(&[
                (1, 0, 0),
                (2, 0, 0),
                (3, 0, 0),
                (4, 0, 0),
                (0, 1, 0),
                (0, 1, 1),
            ]),
            // 整手全百搭：无花可定，候选必须为空。
            wild_only,
        ];
        for hand in &cases {
            let state = diff_state_with(&engine, 0, hand.clone(), None);
            assert_flush_candidates_equivalent(&engine, &state, hand);
        }
    }

    /// 合成牌力序的两条边界（§5.13 第 911/932 行、`levelInsert: above-A`）：
    /// **打 2 时 2 顶到 A 之上；不打 2 时 2 就是自然序最低位**（不是「2 恒为次高档」）。
    #[test]
    fn rank_key_two_is_top_only_when_level_is_two() {
        let engine = classic_engine();
        let r = &engine.ruleset;
        let deck_idx = |name: &str| {
            r.deck
                .ranks
                .iter()
                .position(|k| k.0 == name)
                .unwrap_or_else(|| panic!("deck 缺点数 {name}"))
        };
        let (two, three, a) = (deck_idx("2"), deck_idx("3"), deck_idx("A"));

        // ① 首局打 2（夹具 `levelTable.initial = "2"`）：2 从序首抽出插到 A 之后。
        let state = engine.deal(M0_SEED).expect("发牌失败");
        let ctx = engine.rank_context(&state);
        assert!(
            ctx.rank_key(r, two) > ctx.rank_key(r, a),
            "打 2 时 2 必须大于 A"
        );
        assert_eq!(ctx.rank_key(r, three), 1, "打 2 时 3 是合成序最低位");
        assert_eq!(
            ctx.rank_key(r, two),
            ctx.rank_key(r, a) + 1,
            "2 紧贴在 A 之上、小王之下"
        );

        // ② 改成打 5：2 回到自然序最低位，5 顶到 A 之上。
        let mut state5 = engine.deal(M0_SEED).expect("发牌失败");
        let level_table = r.level_table.as_ref().expect("classic 有 levelTable");
        let five_level = level_table
            .ranks
            .iter()
            .position(|k| k.0 == "5")
            .expect("levelTable 缺 5");
        for lv in state5.levels.iter_mut() {
            *lv = five_level as u8;
        }
        let ctx5 = engine.rank_context(&state5);
        let five = deck_idx("5");
        assert_eq!(ctx5.rank_key(r, two), 1, "不打 2 时 2 必须是最小");
        assert!(
            ctx5.rank_key(r, two) < ctx5.rank_key(r, three),
            "不打 2 时 2 小于 3"
        );
        assert!(
            ctx5.rank_key(r, five) > ctx5.rank_key(r, a),
            "打 5 时 5 必须大于 A"
        );

        // ③ 组牌型只认自然序：级牌不提升，故 A 恒为自然序最高非王点数。
        assert_eq!(
            ctx5.natural_rank_key(r, a),
            r.deck.ranks.len() as u32,
            "自然序里 A 仍在 13 档"
        );
        assert_eq!(ctx5.natural_rank_key(r, two), 1, "自然序里 2 恒为最低");
    }

    struct DiffRng(u64);

    impl DiffRng {
        fn next(&mut self) -> u64 {
            self.0 = self
                .0
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            self.0
        }
    }

    #[test]
    fn differential_random_hands_four_seats_two_contexts() {
        let engine = classic_engine();
        let r = &engine.ruleset;
        let mut deck: Vec<u16> = (0..13usize)
            .flat_map(|rank| {
                (0..4usize)
                    .flat_map(move |suit| (0..2u8).map(move |copy| diff_card(r, rank, suit, copy)))
            })
            .collect();
        deck.push(patterns::joker_id(r, false, 0));
        deck.push(patterns::joker_id(r, false, 1));
        deck.push(patterns::joker_id(r, true, 0));
        deck.push(patterns::joker_id(r, true, 1));

        /// 抽样规模：200 手牌 × 4 座位 × 领出/跟牌两种上下文。
        const SAMPLES: usize = 200;
        let mut rng = DiffRng(M0_SEED);
        let mut checked = 0usize;
        for _ in 0..SAMPLES {
            let k = 1 + (rng.next() % 27) as usize;
            for i in 0..deck.len() {
                let j = (rng.next() as usize) % deck.len();
                deck.swap(i, j);
            }
            let hand: Vec<u16> = deck[..k].to_vec();
            for seat in 0..4u8 {
                let lead = diff_state_with(&engine, seat, hand.clone(), None);
                assert_enum_equivalent(&engine, &lead, &hand);
                checked += 1;
                let last = LastPlay {
                    seat: (seat + 3) % 4,
                    pattern_id: "single".to_owned(),
                    pattern_key: (1, 1),
                    cards: vec![hand[0]],
                };
                let mid = diff_state_with(&engine, seat, hand.clone(), Some(last));
                assert_enum_equivalent(&engine, &mid, &hand);
                checked += 1;
            }
        }
        eprintln!("差分随机抽样: {checked} 个 (手牌×4 座位×领出/跟牌) 全部一致");
        assert_eq!(checked, SAMPLES * 4 * 2);
    }

    #[test]
    fn differential_boundary_hands() {
        let engine = classic_engine();
        let r = &engine.ruleset;
        // 边界手牌集：满 8 张同点 / 级牌入组牌型 / 百搭多孔 / triple-pair / 同花顺多解 / 剩 1–2 张。
        let hands: Vec<Vec<u16>> = vec![
            // 同点数满 8 张（bomb-8，两副四花色两副本）。
            (0..4usize)
                .flat_map(|s| [diff_card(r, 0, s, 0), diff_card(r, 0, s, 1)])
                .collect(),
            // 级牌入组牌型 2-3-4-5-6（级牌 = 2，rank 0..4）。
            (0..5usize).map(|i| diff_card(r, i, i % 4, 0)).collect(),
            // 百搭多孔：真牌 3、5 各一张 + 两张百搭，竞争多个顺子窗口。
            vec![
                diff_card(r, 1, 0, 0),
                diff_card(r, 3, 1, 0),
                diff_wild(r, 0),
                diff_wild(r, 1),
            ],
            // triple-pair：主牌两张真 + 百搭补主；对子一张真 + 百搭补对。
            vec![
                diff_card(r, 0, 0, 0),
                diff_card(r, 0, 0, 1),
                diff_wild(r, 0),
                diff_card(r, 2, 1, 0),
                diff_wild(r, 1),
            ],
            // 同花顺多解：同花 4 张 + 百搭补第五孔。
            vec![
                diff_card(r, 1, 0, 0),
                diff_card(r, 2, 0, 0),
                diff_card(r, 3, 0, 0),
                diff_card(r, 4, 0, 0),
                diff_wild(r, 0),
            ],
            // 剩 1 张。
            vec![diff_card(r, 5, 2, 0)],
            // 剩 2 张。
            vec![diff_card(r, 5, 2, 0), diff_card(r, 6, 2, 0)],
        ];

        for hand in &hands {
            for seat in 0..4u8 {
                let lead = diff_state_with(&engine, seat, hand.clone(), None);
                assert_enum_equivalent(&engine, &lead, hand);
                let last = LastPlay {
                    seat: (seat + 3) % 4,
                    pattern_id: "single".to_owned(),
                    pattern_key: (1, 1),
                    cards: vec![hand[0]],
                };
                let mid = diff_state_with(&engine, seat, hand.clone(), Some(last));
                assert_enum_equivalent(&engine, &mid, hand);
            }
        }

        // 整手全百搭 1/2/3/5/8 张（copies=8 变体，凑得齐 8 张百搭）。
        let eng8 = classic_engine_copies(8);
        let r8 = &eng8.ruleset;
        for n in [1usize, 2, 3, 5, 8] {
            let hand: Vec<u16> = (0..n).map(|c| diff_wild(r8, c as u8)).collect();
            for seat in 0..4u8 {
                let lead = diff_state_with(&eng8, seat, hand.clone(), None);
                assert_enum_equivalent(&eng8, &lead, &hand);
            }
        }
    }

    // ---------- pattern-compose 等价性差分：8 预置形状 vs compose 镜像（第三十三次变更） ----------

    fn compose_engine() -> M0Engine {
        let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        p.push("tests/fixtures/guandan-compose/ruleset.yaml");
        let src = fs::read_to_string(p).expect("compose 夹具读取失败");
        let body = from_yaml(&src).expect("compose 夹具构造失败");
        M0Engine::new(body)
    }

    fn compose_engine_copies(copies: u8) -> M0Engine {
        let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        p.push("tests/fixtures/guandan-compose/ruleset.yaml");
        let src = fs::read_to_string(p).expect("compose 夹具读取失败");
        let total = (13 * 4 + 2) * copies as usize;
        let per_seat = total / 4;
        let src = src
            .replace("copies: 2", &format!("copies: {copies}"))
            .replace("perSeat: 27", &format!("perSeat: {per_seat}"))
            .replace("cardCount: 108", &format!("cardCount: {total}"));
        let body = from_yaml(&src).expect("compose 变体夹具构造失败");
        M0Engine::new(body)
    }

    /// 用 preset 侧状态的座位 / 手牌 / 跟牌上下文，在 mirror 引擎上重建等价状态。
    fn mirror_state_with(engine: &M0Engine, other: &State) -> State {
        let seat = other.turn_seat;
        let idx = engine.hand_zone_index();
        let hand: Vec<u16> = other
            .zones
            .iter()
            .find(|z| z.seat == seat && z.zone_id == idx)
            .map(|z| z.cards.clone())
            .unwrap_or_default();
        diff_state_with(engine, seat, hand, other.last_play.clone())
    }

    /// preset（8 既有原语）与 compose 镜像在同一手牌、同一上下文下逐条比对 `Vec<Action>`（含顺序）。
    fn assert_compose_equivalent(
        preset: &M0Engine,
        mirror: &M0Engine,
        state: &State,
        hand: &[u16],
    ) {
        let a = preset
            .legal_actions(state)
            .expect("preset legal_actions 应成功");
        let bstate = mirror_state_with(mirror, state);
        let b = mirror
            .legal_actions(&bstate)
            .expect("mirror legal_actions 应成功");
        assert_eq!(
            a, b,
            "preset/compose 枚举不一致: hand={hand:?} last={:?}",
            state.last_play
        );
    }

    #[test]
    fn differential_compose_boundary_hands() {
        let preset = classic_engine();
        let mirror = compose_engine();
        let r = &preset.ruleset;
        // 覆盖：轮子 rankKey=0、级牌入组 2-3-4-5-6、百搭多孔多窗口、triple-pair 主牌口径
        // （带对 A 不提升）、同花顺多解、8 张炸、剩 1–2 张。
        let hands: Vec<Vec<u16>> = vec![
            // 轮子 A-2-3-4-5（非同花，rankKey 恒 0）。
            vec![
                diff_card(r, 12, 0, 0),
                diff_card(r, 0, 1, 0),
                diff_card(r, 1, 2, 0),
                diff_card(r, 2, 3, 0),
                diff_card(r, 3, 0, 0),
            ],
            // 级牌入组牌型 2-3-4-5-6（级牌 = 2，rank 0..4）。
            (0..5usize).map(|i| diff_card(r, i, i % 4, 0)).collect(),
            // 百搭多孔：真牌 3、5 各一张 + 两张百搭，竞争多个顺子窗口。
            vec![
                diff_card(r, 1, 0, 0),
                diff_card(r, 3, 1, 0),
                diff_wild(r, 0),
                diff_wild(r, 1),
            ],
            // triple-pair：主牌两张真 + 百搭补主；对子一张真 + 百搭补对（带对 A 不提升）。
            vec![
                diff_card(r, 0, 0, 0),
                diff_card(r, 0, 0, 1),
                diff_wild(r, 0),
                diff_card(r, 12, 1, 0),
                diff_wild(r, 1),
            ],
            // 同花顺多解：同花 4 张 + 百搭补第五孔。
            vec![
                diff_card(r, 1, 0, 0),
                diff_card(r, 2, 0, 0),
                diff_card(r, 3, 0, 0),
                diff_card(r, 4, 0, 0),
                diff_wild(r, 0),
            ],
            // 同点数满 8 张（bomb-8，两副四花色两副本）。
            (0..4usize)
                .flat_map(|s| [diff_card(r, 0, s, 0), diff_card(r, 0, s, 1)])
                .collect(),
            // 剩 1 张。
            vec![diff_card(r, 5, 2, 0)],
            // 剩 2 张。
            vec![diff_card(r, 5, 2, 0), diff_card(r, 6, 2, 0)],
        ];

        let mut checked = 0usize;
        for hand in &hands {
            for seat in 0..4u8 {
                let lead = diff_state_with(&preset, seat, hand.clone(), None);
                assert_compose_equivalent(&preset, &mirror, &lead, hand);
                checked += 1;
                let last = LastPlay {
                    seat: (seat + 3) % 4,
                    pattern_id: "single".to_owned(),
                    pattern_key: (1, 1),
                    cards: vec![hand[0]],
                };
                let mid = diff_state_with(&preset, seat, hand.clone(), Some(last));
                assert_compose_equivalent(&preset, &mirror, &mid, hand);
                checked += 1;
            }
        }

        // 整手全百搭 1/2/3/5/8 张（copies=8 变体，凑得齐 8 张百搭）。
        let p8 = classic_engine_copies(8);
        let m8 = compose_engine_copies(8);
        let r8 = &p8.ruleset;
        for n in [1usize, 2, 3, 5, 8] {
            let hand: Vec<u16> = (0..n).map(|c| diff_wild(r8, c as u8)).collect();
            for seat in 0..4u8 {
                let lead = diff_state_with(&p8, seat, hand.clone(), None);
                assert_compose_equivalent(&p8, &m8, &lead, &hand);
                checked += 1;
            }
        }
        eprintln!("compose 边界差分: {checked} 个上下文全部一致");
        assert_eq!(checked, hands.len() * 4 * 2 + 5 * 4);
    }

    #[test]
    fn differential_compose_random_hands() {
        let preset = classic_engine();
        let mirror = compose_engine();
        let r = &preset.ruleset;
        let mut deck: Vec<u16> = (0..13usize)
            .flat_map(|rank| {
                (0..4usize)
                    .flat_map(move |suit| (0..2u8).map(move |copy| diff_card(r, rank, suit, copy)))
            })
            .collect();
        deck.push(patterns::joker_id(r, false, 0));
        deck.push(patterns::joker_id(r, false, 1));
        deck.push(patterns::joker_id(r, true, 0));
        deck.push(patterns::joker_id(r, true, 1));

        const SAMPLES: usize = 200;
        let mut rng = DiffRng(M0_SEED);
        let mut checked = 0usize;
        for _ in 0..SAMPLES {
            let k = 1 + (rng.next() % 27) as usize;
            for i in 0..deck.len() {
                let j = (rng.next() as usize) % deck.len();
                deck.swap(i, j);
            }
            let hand: Vec<u16> = deck[..k].to_vec();
            for seat in 0..4u8 {
                let lead = diff_state_with(&preset, seat, hand.clone(), None);
                assert_compose_equivalent(&preset, &mirror, &lead, &hand);
                checked += 1;
                let last = LastPlay {
                    seat: (seat + 3) % 4,
                    pattern_id: "single".to_owned(),
                    pattern_key: (1, 1),
                    cards: vec![hand[0]],
                };
                let mid = diff_state_with(&preset, seat, hand.clone(), Some(last));
                assert_compose_equivalent(&preset, &mirror, &mid, &hand);
                checked += 1;
            }
        }
        eprintln!("compose 随机差分: {checked} 个 (手牌×4 座位×领出/跟牌) 全部一致");
        assert_eq!(checked, SAMPLES * 4 * 2);
    }
}
