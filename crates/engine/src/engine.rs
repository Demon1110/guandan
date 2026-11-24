//! M0 最小执行路径：发牌（`deal.shuffle`）→ 单张出牌（`pattern.same-rank count:1`
//! + `action.require-legal`）→ `score-only` 结算。
//!
//! 只覆盖 `m0-minimal` 夹具用到的原语；其余原语（进贡/还贡/抗贡、炸弹/顺子/同花顺/
//! 四王炸、`level.*`、`compare.*`、未用到的 `turn.*` 分支）属 M1，本批不实现，
//! 命中即走保守空候选 / 内部错误，不在判定链路里发明规则。
//!
//! 确定性纪律（§3.3 约束 2）：纯整数、无浮点、无 `SystemTime`/线程 RNG/文件网络 I/O；
//! 排序给全序；RNG 唯一来源是引擎注入的种子与计数器。

use crate::contract::{Action, Engine, EngineError, Seat, State, View, ZoneId};
use crate::hash::{seed_digest, LastPlay, RngState, TributeState, ZoneEntry};
use crate::ruleset::model::ZoneScope;
use crate::ruleset::params::PrimitiveParams;
use crate::ruleset::RuleSetBody;
use crate::view::{encode_canonical, ViewData, ViewLastPlay};

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
}

impl M0Engine {
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

    fn hand_zone_index(&self) -> u16 {
        self.zone_index(&self.hand_zone()).unwrap_or(0)
    }

    /// 公共弃牌区：声明序第一个 shared 区。
    fn discard_zone_index(&self) -> Option<u16> {
        self.ruleset
            .zones
            .iter()
            .position(|z| z.scope == ZoneScope::Shared)
            .map(|i| i as u16)
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

    /// N13：`rankKey` = 该点数在 `naturalOrder`（+ `levelInsert` 合成序；m0 无级牌，
    /// 即 `naturalOrder`）里的 0 基下标。`countKey` 对单张恒为 1。
    fn rank_key(&self, card: u16) -> u32 {
        let base_total = self.base_total();
        if (card as usize) < base_total {
            let base = (card as usize) / self.copies();
            let rank_idx = base % self.rank_count();
            let rank = self.ruleset.deck.ranks[rank_idx].as_str();
            self.ruleset
                .natural_order
                .iter()
                .position(|r| r.as_str() == rank)
                .unwrap_or(rank_idx) as u32
        } else {
            // 王排在序尾：little-joker 在前、big-joker 在后，各 `copies` 张。
            let joker_slot = (card as usize - base_total) / self.copies();
            let tail = self.ruleset.natural_order.len();
            tail.saturating_sub(2).saturating_add(joker_slot) as u32
        }
    }

    // ---------- RNG（N7：算法文档未定义，取 BLAKE3(seed‖counter) 前 8 字节） ----------

    // ---------- 发牌（N8 Fisher–Yates；N9 按座位升序连续切分） ----------

    /// 由固定种子开局：洗牌并发到各座位私有区，相位进入 `play`（夹具省略 tribute，G6）。
    pub fn deal(&self, seed: u64) -> Result<State, EngineError> {
        let seat_count = self.ruleset.seats.count as usize;
        let total = self.deck_total();
        let per_seat = self
            .ruleset
            .flow
            .as_ref()
            .and_then(|f| f.deal.as_ref())
            .and_then(|d| d.per_seat)
            .map(|p| p as usize)
            .unwrap_or(total / seat_count);

        // 牌 id 载入期一次性分配并冻结（§5.12 第 811 行）：suits 声明序 × ranks 自然序，
        // 同一基础牌的副本占相邻 id；王排在序尾。
        let mut deck: Vec<u16> = (0..total as u16).collect();

        let mut rng = RngState {
            counter: 0,
            seed_digest: seed_digest(seed),
        };
        // N8：Fisher–Yates 从尾向头；j = rng.pick(i+1)。
        for i in (1..deck.len()).rev() {
            let j = self.rng_pick_seed(&mut rng, seed, i as u64 + 1) as usize;
            deck.swap(i, j);
        }

        let hand_idx = self.hand_zone_index();
        let discard_idx = self.discard_zone_index();

        let mut zones = Vec::with_capacity(seat_count + 1);
        // N9：洗牌后按座位号升序连续切分，座位 s 取 [s*perSeat, (s+1)*perSeat)。
        for seat in 0..seat_count {
            let start = seat * per_seat;
            let cards = deck[start..start + per_seat].to_vec();
            zones.push(ZoneEntry {
                seat: seat as u8,
                zone_id: hand_idx,
                cards,
            });
        }
        if let Some(discard_idx) = discard_idx {
            zones.push(ZoneEntry {
                seat: 0,
                zone_id: discard_idx,
                cards: Vec::new(),
            });
        }

        Ok(State {
            phase: PHASE_PLAY,
            turn_seat: 0, // §5.13 第 856 行：v1 固定座位 0 先出
            // G4：score-only 局 `seat.levels` 空序列、键不出现。
            levels: Vec::new(),
            seats: (0..seat_count as u8).collect(),
            zones,
            last_play: None,
            seq_watermark: (0..seat_count as u8).map(|s| (s, 0)).collect(),
            tribute: TributeState {
                from_seat: 0,
                to_seat: 0,
                card: None,
            },
            rng,
            a_fail_streak: vec![0; self.ruleset.seats.teams.len()],
            finish_order: Vec::new(),
            seed,
            passes_since_last_play: 0,
        })
    }

    /// N7 变体：发牌期需要 seed 原值进 BLAKE3 输入（`rng_pick` 的通用形式见上）。
    fn rng_pick_seed(&self, rng: &mut RngState, seed: u64, n: u64) -> u64 {
        let mut msg = Vec::with_capacity(16);
        msg.extend_from_slice(&seed.to_le_bytes());
        msg.extend_from_slice(&rng.counter.to_le_bytes());
        let digest = blake3::hash(&msg);
        let mut bytes = [0u8; 8];
        bytes.copy_from_slice(&digest.as_bytes()[..8]);
        rng.counter += 1;
        let v = u64::from_le_bytes(bytes);
        if n == 0 {
            0
        } else {
            v % n
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

    /// `flow.turn.skipFinished`（§5.2，默认 `true`）：轮转是否跳过已出完座位。
    fn skip_finished(&self) -> bool {
        self.ruleset
            .flow
            .as_ref()
            .and_then(|f| f.turn.as_ref())
            .and_then(|t| t.skip_finished)
            .unwrap_or(true)
    }

    /// 顺时针下一未出完座位；全出完则回原座（兜底，不应走到）。
    fn next_seat(&self, state: &State, from: u8) -> u8 {
        let count = self.ruleset.seats.count;
        let skip = self.skip_finished();
        let mut seat = from;
        loop {
            seat = (seat + 1) % count;
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

    /// 找 `pattern.same-rank count:1` 单张牌型定义（m0-minimal 唯一在役牌型）。
    fn single_pattern(&self) -> Option<&crate::ruleset::model::PatternDef> {
        self.ruleset
            .patterns
            .iter()
            .find(|p| matches!(p.params, PrimitiveParams::SameRank { count: 1 }))
    }

    // ---------- legal_actions ----------

    fn legal_actions_impl(&self, state: &State) -> Result<Vec<Action>, EngineError> {
        let mut out: Vec<Action> = Vec::new();
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

        // 候选四元组：(countKey, rankKey, cards 牌 id 序列, primitiveId)，全序（§5.12 第 842 行）。
        let mut candidates: Vec<(u32, u32, Vec<u16>, String, Action)> = Vec::new();

        if let Some(pattern) = self.single_pattern() {
            let pattern_id = pattern.id.0.clone();
            for &card in &hand {
                let rank_key = self.rank_key(card);
                // 领出（lastPlay 为空）任意单张合法；跟牌必须点数压过上家单张。
                let legal = match &state.last_play {
                    None => true,
                    Some(last) => rank_key > last.pattern_key.1,
                };
                if legal {
                    candidates.push((
                        1,
                        rank_key,
                        vec![card],
                        pattern_id.clone(),
                        Action::Play { cards: vec![card] },
                    ));
                }
            }
        }
        // TODO(M1-GAP-patterns)：pair/triple/straight/plate/steel/bomb/straight-flush/
        // joker-bomb 等牌型原语本批不实现；命中这些原语的候选不在此枚举。

        candidates.sort_by(|a, b| (&a.0, &a.1, &a.2, &a.3).cmp(&(&b.0, &b.1, &b.2, &b.3)));
        for candidate in candidates {
            out.push(candidate.4);
        }

        // 过牌：仅跟牌阶段合法（领出不能 pass）。Pass 不参与牌型四元组排序，排在所有 Play 之后。
        if state.last_play.is_some() {
            out.push(Action::Pass);
        }
        // TODO(M1-GAP-pass-rule)：接风（partnerLead）、全过回最后出牌者之外的轮转分支
        // 属 M1；本批只实现顺时针跳过已出完座位 + 全过回领头。

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
        if state.phase != PHASE_PLAY {
            return Err(EngineError::IllegalAction {
                reason: "当前不在 play 相位".to_owned(),
            });
        }
        let seat = state.turn_seat;

        match action {
            Action::Play { cards } => {
                // 落子前复检合法性，防止越权动作进入状态迁移。
                let legal = self.legal_actions_impl(&state)?;
                let legal_play = legal.iter().any(|a| match a {
                    Action::Play { cards: legal_cards } => *legal_cards == cards,
                    Action::Pass => false,
                });
                if !legal_play {
                    return Err(EngineError::IllegalAction {
                        reason: format!("seat {seat} 提交的出牌不在合法集合内"),
                    });
                }

                let hand_idx = self.hand_zone_index();
                let hand = state
                    .zones
                    .iter_mut()
                    .find(|z| z.seat == seat && z.zone_id == hand_idx)
                    .ok_or_else(|| EngineError::Internal {
                        message: format!("seat {seat} 私有手牌区缺失"),
                    })?;
                for &card in &cards {
                    let pos = hand.cards.iter().position(|c| *c == card).ok_or_else(|| {
                        EngineError::IllegalAction {
                            reason: format!("牌 {card} 不在 seat {seat} 手牌中"),
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

                let rank_key = self.rank_key(cards[0]);
                let pattern_id = self
                    .single_pattern()
                    .map(|p| p.id.0.clone())
                    .unwrap_or_else(|| "single".to_owned());
                state.last_play = Some(LastPlay {
                    seat,
                    pattern_id,
                    pattern_key: (1, rank_key),
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
                // §5.13 第 877 行：其余座位连续 pass 完后，由最后出牌者重新领出。
                let all_passed =
                    state.passes_since_last_play as usize >= active.len().saturating_sub(1);
                if all_passed {
                    let last_seat = state.last_play.as_ref().unwrap().seat;
                    // N14：新墩起始 lastPlay 清空为 None（§5.12 第 819 行：本回合无人出过牌时该键不出现）。
                    state.last_play = None;
                    state.passes_since_last_play = 0;
                    // 领头已出完时，顺延到下一个未出完座位（接风/队友领出的完整规则属 M1）。
                    state.turn_seat = if state.finish_order.contains(&last_seat) {
                        self.next_seat(&state, last_seat)
                    } else {
                        last_seat
                    };
                } else {
                    state.turn_seat = self.next_seat(&state, seat);
                }
            }
        }

        Ok(state)
    }

    /// score-only 结算：只记名次、不升级（§5.1 第 393 行、§5.13「局终止与名次」）。
    fn settle(&self, state: &mut State) -> Result<(), EngineError> {
        // 已出完按 finish_order；未出完按剩余手牌张数升序，同数按座位号小者在前
        // （§5.13 第 884 行）。
        let mut unfinished: Vec<(u8, usize)> = state
            .seats
            .iter()
            .copied()
            .filter(|s| !state.finish_order.contains(s))
            .map(|s| (s, self.hand_cards(state, s).map(|h| h.len()).unwrap_or(0)))
            .collect();
        unfinished.sort_by(|a, b| a.1.cmp(&b.1).then(a.0.cmp(&b.0)));
        for (seat, _) in unfinished {
            state.finish_order.push(seat);
        }
        // G4：score-only 不挂等级表，levels 保持空序列；aFailStreak 不推进。
        state.phase = PHASE_SETTLE;
        state.last_play = None;
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
        if let Some(last) = &state.last_play {
            data.last_play = Some(ViewLastPlay {
                seat: last.seat,
                pattern_id: last.pattern_id.clone(),
                cards: last.cards.clone(),
            });
        }
        // G4：score-only 的 levels 为空序列，视图键不出现。
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cases::M0_SEED;
    use crate::contract::Engine;

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
            if legal.is_empty() {
                break;
            }
            let action = legal.into_iter().next().unwrap();
            state = engine.apply(state, action).expect("落子失败");
        }
        assert_eq!(state.phase, PHASE_SETTLE, "应跑到 settle");
    }
}
