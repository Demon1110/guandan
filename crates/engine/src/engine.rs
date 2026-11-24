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

use crate::contract::{Action, Engine, EngineError, PatternId, Seat, State, View, ZoneId};
use crate::hash::{seed_digest, LastPlay, RngState, TributeState, ZoneEntry};
use crate::patterns;
use crate::ruleset::model::{AfterAllPass, TurnDirection, ZoneScope};
use crate::ruleset::params::PrimitiveParams;
use crate::ruleset::RuleSetBody;
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
    /// `play → settle`（某队全员出完）。
    /// 表内声明但执行路径待落地（不自造转移）：
    /// - `deal → tribute`、`tribute → play`：进贡 / 还贡 / 抗贡分支（进贡原语未实现）；
    /// - `settle → deal`：下一局开局，依赖 ⑧ 跨局携带与场次计数。
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

        // §5.4 运行时牌数断言 / `RS_ZONE_CONSERVATION`（设计稿第 574、642 行）：
        // 载入期就该拦下「perSeat × 座位数 ≠ 总张数」的配置；若整除兜底静默截断，
        // 多出来的牌会被静默丢掉、zones 守恒当场破，引擎不做这种静默兜底。
        if per_seat * seat_count != total {
            return Err(EngineError::Internal {
                message: format!(
                    "发牌张数不守恒: perSeat={per_seat} seatCount={seat_count} total={total}（{per_seat}×{seat_count}≠{total}）"
                ),
            });
        }

        // 牌 id 载入期一次性分配并冻结（§5.12 第 811 行）：suits 声明序 × ranks 自然序，
        // 同一基础牌的副本占相邻 id；王排在序尾。
        let mut deck: Vec<u16> = (0..total as u16).collect();

        let stream = self.rng_stream();
        let mut rng = RngState {
            counter: 0,
            seed_digest: seed_digest(seed),
        };

        // §5.2 第 560 行 / §5.13 第 876 行：场次首局先 `bounded(总张数)` 取牌 id 下标 `i`；
        // 抽出的牌不剔出牌堆、正常参与本局，随后才 Fisher–Yates 洗牌并按座位序连续切分；
        // 切分完按牌 id 定位持牌座位 → 该座位先出、其队即 `attackingTeam`（§5.12 第 822 行）。
        //
        // TODO(M0-GAP-SEQ-CROSS-ROUND)：State 目前没有场次/局数计数（跨局携带是未做的 ⑧），
        // 本笔只能「每局按首局处理」；非首局不抽牌的判据待局数字段落地后再补，此处不造计数。
        let drawn_card = Self::bounded(&mut rng, seed, stream, total as u64) as u16;

        // Fisher–Yates 降序：`for i in (1..n).rev() { j = bounded(i+1); swap(a[i], a[j]) }`，
        // j ∈ 0..=i（§5.2 第 558 行）。
        for i in (1..deck.len()).rev() {
            let j = Self::bounded(&mut rng, seed, stream, i as u64 + 1) as usize;
            deck.swap(i, j);
        }

        // 按牌 id 反查持牌座位：洗牌后该牌落在第 `pos` 个槽位，座位 = `pos / perSeat`。
        let lead_seat = deck
            .iter()
            .position(|&card| card == drawn_card)
            .map(|pos| pos / per_seat)
            .unwrap_or(0) as u8;
        let attacking_team = self
            .ruleset
            .seats
            .teams
            .iter()
            .position(|team| team.contains(&lead_seat))
            .unwrap_or(0) as u8;

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

        // §5.13 第 897 行：两队级牌都从 `levelTable.initial` 起，`levels` 存
        // `levelTable.ranks` 的下标；G4 score-only 局无 `levelTable`，保持空序列不崩。
        let team_count = self.ruleset.seats.teams.len();
        let levels = match &self.ruleset.level_table {
            Some(table) => {
                let idx = table
                    .ranks
                    .iter()
                    .position(|rank| rank == &table.initial)
                    .map(|i| i as u8)
                    .unwrap_or(0);
                vec![idx; team_count]
            }
            None => Vec::new(),
        };

        let mut state = State {
            phase: PHASE_DEAL,
            turn_seat: lead_seat,
            attacking_team,
            levels,
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
            // TODO(M0-GAP-N18)：score-only 局 `levels` 为空序列（G4），而 `aFailStreak` 仍按队位数
            // 保留 2 个队位；§5.13 第 892、894 行的 aFailStreak 推进依附于 level 过 A 判定，
            // score-only 局该字段是清零、占位还是省略未定义。现按队位数填 0，待裁决，本批不改行为。
            a_fail_streak: vec![0; self.ruleset.seats.teams.len()],
            finish_order: Vec::new(),
            seed,
            passes_since_last_play: 0,
            // TODO(M0-GAP-SEQ-CROSS-ROUND)：跨局序号连续待 ⑧ 落地；首局占位 0，不进哈希。
            round_no: 0,
        };

        // §6.3 转移表：`tribute.enabled=false`（或抗贡成立）时 `deal → play` 整段跳过
        // 进贡；`enabled=true` 的 `deal → tribute` 路径待进贡原语落地后接入。
        self.transition(&mut state, PHASE_PLAY)?;
        Ok(state)
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

    fn legal_actions_impl(&self, state: &State) -> Result<Vec<Action>, EngineError> {
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
        let records = patterns::enumerate(ladder, &self.ruleset, &ctx, &hand);
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
        if state.phase != PHASE_PLAY {
            return Err(EngineError::IllegalAction {
                reason: "当前不在 play 相位".to_owned(),
            });
        }
        let seat = state.turn_seat;

        match action {
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

    /// score-only 结算：只记名次、不升级（§5.1 第 393 行、§5.13「局终止与名次」）。
    fn settle(&self, state: &mut State) -> Result<(), EngineError> {
        // §6.3：`play → settle`，经相位机校验，表外跳转报错不静默接受。
        self.transition(state, PHASE_SETTLE)?;
        // 已出完按 finish_order；未出完座位的名次分配（双下按座位方向倒排）属 ⑥ 结算批次，
        // 本批不落地。第二十六次变更已作废「按剩余手牌张数升序、同数按座位号」：张数不进
        // 判定链，这里只按座位号中性补位到 4 座，方向口径待 ⑥ 第一批接线。
        // TODO(M0-GAP-rank-direction)：双下名次从头游起沿 direction 倒排。
        let mut unfinished: Vec<u8> = state
            .seats
            .iter()
            .copied()
            .filter(|s| !state.finish_order.contains(s))
            .collect();
        unfinished.sort_unstable();
        for seat in unfinished {
            state.finish_order.push(seat);
        }
        // G4：score-only 不挂等级表，levels 保持空序列；aFailStreak 不推进。
        // TODO(M0-GAP-N17)：局终口径文档未定义——settle 时 `lastPlay` 应清空为 None 还是保留
        // 最后一墩；§5.12 第 819 行只规定「本回合无人出过牌时该键不出现」，未写局终快照口径。
        // 现取清空，待裁决，本批不改行为。
        state.last_play = None;
        // TODO(M0-GAP-ATTACK-TEAM-NEXT)：settle 应把 `attacking_team` 置为头游（finish_order[0]）
        // 所在队属，属用户明确推迟的 ⑥ 四步那一批；本笔只开局抽牌定首出，结算不改队属。
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
        // 队级状态：score-only 局 `levels` 为空，`seat.levels` 与两个 `round.*` 键
        // 一并不出现（§5.12 字段级编码表缺值行）；无级牌状态不写 null、不留占位。
        if !state.levels.is_empty() {
            data.seat_levels = Some(state.levels.clone());
            data.round_level = state.levels.get(state.attacking_team as usize).copied();
            data.round_attacking_team = Some(state.attacking_team);
        }
        if let Some(last) = &state.last_play {
            data.last_play = Some(ViewLastPlay {
                seat: last.seat,
                pattern_id: last.pattern_id.clone(),
                cards: last.cards.clone(),
            });
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cases::M0_SEED;
    use crate::contract::{Engine, ErrorCode, Level};
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
    fn m0_deal_keeps_empty_levels_and_sets_attacking_team() {
        let engine = engine();
        let state = engine.deal(M0_SEED).expect("发牌失败");
        // G4 score-only 局无 levelTable：levels 保持空序列，不许崩。
        assert!(state.levels.is_empty());
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
