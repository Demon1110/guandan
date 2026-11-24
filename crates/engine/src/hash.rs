//! `stateHash` 九段拼接（§5.12 第 791–802 行、第 833–838 行）。
//!
//! 纪律（§3.3 约束 2、§5.12「规范化与序列化」）：
//! - 九段固定顺序、显式字段序、little-endian、定宽整数；
//! - 字符串 UTF-8 不做 NFC 归一化；
//! - 不依赖 `HashMap` 迭代顺序与 struct 字段声明顺序；排序给全序；
//! - 哈希函数固定 BLAKE3-256（32 字节）；对外呈现取前 8 字节 = 16 个十六进制字符；
//! - 版本三元组（`engineVersion` / `interpreterVersion` / `rulesetVersion`）**不进哈希拼接**，
//!   与哈希并列旁挂（§5.12 第 793 行）。

/// 私有 / 公共区实例化条目（第 4 段 `zones`）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ZoneEntry {
    pub seat: u8,
    pub zone_id: u16,
    pub cards: Vec<u16>,
}

/// 本回合最后一次出牌（第 5 段 `lastPlay`）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LastPlay {
    pub seat: u8,
    pub pattern_id: String,
    /// `patternKey = (countKey, rankKey)` 两个 u32（§5.12 第 797 行、第 842 行）。
    pub pattern_key: (u32, u32),
    pub cards: Vec<u16>,
}

/// 进贡段（第 7 段 `tribute`）；无进贡取占位 0/0/空（§5.12 第 799 行）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TributeState {
    pub from_seat: u8,
    pub to_seat: u8,
    pub card: Option<u16>,
}

/// RNG 段（第 8 段 `rng`）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RngState {
    pub counter: u64,
    /// `seedDigest` = BLAKE3-256(seed 的 u64 定长小端编码) 前 8 字节。
    pub seed_digest: [u8; 8],
}

/// §5.12 第 791 行明确要求用这个名字的固定结构体。
///
/// 字段即 §5.12 第 2–10 项的九段；第 1 项版本三元组旁挂在 [`HashBundle`]。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CanonicalState {
    /// 第 2 段：phase 枚举序号 u16。
    pub phase: u16,
    /// 第 2 段：当前行动座位 u8。
    pub turn_seat: u8,
    /// 第 2 段：每队一个 u8，按队伍序号升序（存 `levelTable.ranks` 下标）。
    pub levels: Vec<u8>,
    /// 第 3 段：座位号升序；手牌不在本段重复计入。
    pub seats: Vec<u8>,
    /// 第 4 段：按 `(seat u8, zoneId u16)` 升序，`cards` 牌 id 升序。
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
    /// 第 10 段：按出完先后升序的座位序列；未出完的座位不列入。
    pub finish_order: Vec<u8>,
}

/// 单向只读投影：`State` 是唯一真源，`CanonicalState` 只做序列化（N6，§5.12 第 837 行）。
///
/// `seed` 原值与 `passes_since_last_play`（N11 运行期记账）不投影进九段。
impl From<&crate::contract::State> for CanonicalState {
    fn from(state: &crate::contract::State) -> Self {
        Self {
            phase: state.phase,
            turn_seat: state.turn_seat,
            levels: state.levels.clone(),
            seats: state.seats.clone(),
            zones: state.zones.clone(),
            last_play: state.last_play.clone(),
            seq_watermark: state.seq_watermark.clone(),
            tribute: state.tribute.clone(),
            rng: state.rng.clone(),
            a_fail_streak: state.a_fail_streak.clone(),
            finish_order: state.finish_order.clone(),
        }
    }
}

/// 版本三元组与哈希的并列存放载体（版本不进拼接，§5.12 第 793 行）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HashBundle {
    pub engine_version: String,
    pub interpreter_version: String,
    pub ruleset_version: String,
    pub state_hash: [u8; 32],
}

/// `seed`（u64 定长小端）的 BLAKE3-256 摘要前 8 字节（§5.12 第 800 行）。
pub fn seed_digest(seed: u64) -> [u8; 8] {
    let digest = blake3::hash(&seed.to_le_bytes());
    let mut out = [0u8; 8];
    out.copy_from_slice(&digest.as_bytes()[..8]);
    out
}

/// 定宽小端字节写入器。
struct Writer(Vec<u8>);

impl Writer {
    fn new() -> Self {
        Self(Vec::new())
    }
    fn u8(&mut self, v: u8) {
        self.0.push(v);
    }
    fn u16(&mut self, v: u16) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn u64(&mut self, v: u64) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn slice(&mut self, b: &[u8]) {
        self.0.extend_from_slice(b);
    }
    /// 字符串：u16 长度前缀 + UTF-8 字节（不归一化）。
    fn str(&mut self, s: &str) {
        self.u16(s.len() as u16);
        self.slice(s.as_bytes());
    }
}

/// 把 [`CanonicalState`] 按九段固定顺序序列化为定宽小端字节流。
///
/// N2（长度前缀宽度，§5.12 第 838 行）：段内元素个数一律 `u8` 前缀（`levels`/`seats`/
/// `zones`/`seqWatermark`/`finishOrder` 等）；牌 id 数组与字符串（`patternId`）一律 `u16`
/// 前缀（小端，先计数后元素）。宽度属编码定义的一部分，双端必须同一宽度。
pub fn canonical_bytes(state: &CanonicalState) -> Vec<u8> {
    let mut w = Writer::new();

    // 第 2 段：phase / turnSeat / levels。
    w.u16(state.phase);
    w.u8(state.turn_seat);
    w.u8(state.levels.len() as u8);
    for &level in &state.levels {
        w.u8(level);
    }

    // 第 3 段：seats（座位号升序）。
    let mut seats = state.seats.clone();
    seats.sort_unstable();
    w.u8(seats.len() as u8);
    for &seat in &seats {
        w.u8(seat);
    }

    // 第 4 段：zones 按 (seat, zoneId) 升序；cards 牌 id 升序。
    let mut zones = state.zones.clone();
    zones.sort_by_key(|a| (a.seat, a.zone_id));
    // N2：段内元素个数 u8 前缀；区内牌 id 数组才是 u16 前缀（§5.12 第 838 行）。
    w.u8(zones.len() as u8);
    for zone in &zones {
        w.u8(zone.seat);
        w.u16(zone.zone_id);
        let mut cards = zone.cards.clone();
        cards.sort_unstable();
        w.u16(cards.len() as u16);
        for &card in &cards {
            w.u16(card);
        }
    }

    // 第 5 段：lastPlay（ presence 标记 + 字段序）。
    match &state.last_play {
        None => w.u8(0),
        Some(last) => {
            w.u8(1);
            w.u8(last.seat);
            w.str(&last.pattern_id);
            w.u32(last.pattern_key.0);
            w.u32(last.pattern_key.1);
            let mut cards = last.cards.clone();
            cards.sort_unstable();
            w.u16(cards.len() as u16);
            for &card in &cards {
                w.u16(card);
            }
        }
    }

    // 第 6 段：seqWatermark 按座位号升序。
    let mut watermark = state.seq_watermark.clone();
    watermark.sort_by_key(|a| a.0);
    w.u8(watermark.len() as u8);
    for (seat, seq) in &watermark {
        w.u8(*seat);
        w.u64(*seq);
    }

    // 第 7 段：tribute（占位 0/0/空）。
    w.u8(state.tribute.from_seat);
    w.u8(state.tribute.to_seat);
    match state.tribute.card {
        None => w.u8(0),
        Some(card) => {
            w.u8(1);
            w.u16(card);
        }
    }

    // 第 8 段：rng。
    w.u64(state.rng.counter);
    w.slice(&state.rng.seed_digest);

    // 第 9 段：aFailStreak 每队一个 u8，按队伍序号升序——顺序即队索引，
    // 不能按值排序（§5.12 第 801 行）；传入序列即队序。
    w.u8(state.a_fail_streak.len() as u8);
    for &value in &state.a_fail_streak {
        w.u8(value);
    }

    // 第 10 段：finishOrder 按出完先后排列——名次信息就在顺序里，
    // 不能按座位号排序（§5.12 第 802 行）；传入序列即出完先后。
    w.u8(state.finish_order.len() as u8);
    for &seat in &state.finish_order {
        w.u8(seat);
    }

    w.0
}

/// 计算 `stateHash`（BLAKE3-256，32 字节全量；CI 双端按 32 字节逐字节比对）。
pub fn state_hash(state: &CanonicalState) -> [u8; 32] {
    let bytes = canonical_bytes(state);
    *blake3::hash(&bytes).as_bytes()
}

/// 组装 [`HashBundle`]：版本三元组旁挂，不进哈希拼接。
pub fn bundle(
    state: &CanonicalState,
    engine_version: impl Into<String>,
    interpreter_version: impl Into<String>,
    ruleset_version: impl Into<String>,
) -> HashBundle {
    HashBundle {
        engine_version: engine_version.into(),
        interpreter_version: interpreter_version.into(),
        ruleset_version: ruleset_version.into(),
        state_hash: state_hash(state),
    }
}

/// 32 字节哈希对外呈现：前 8 字节 → 16 个十六进制字符（§5.12 第 837 行）。
pub fn present_8(hash: &[u8; 32]) -> String {
    hash[..8].iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_state() -> CanonicalState {
        CanonicalState {
            phase: 3,
            turn_seat: 1,
            levels: vec![2, 2],
            seats: vec![0, 1, 2, 3],
            zones: vec![
                ZoneEntry {
                    seat: 0,
                    zone_id: 0,
                    cards: vec![2, 0, 1],
                },
                ZoneEntry {
                    seat: 1,
                    zone_id: 0,
                    cards: vec![5],
                },
            ],
            last_play: Some(LastPlay {
                seat: 1,
                pattern_id: "single".to_owned(),
                pattern_key: (1, 12),
                cards: vec![5],
            }),
            seq_watermark: vec![(0, 100), (1, 101), (2, 99), (3, 98)],
            tribute: TributeState {
                from_seat: 0,
                to_seat: 0,
                card: None,
            },
            rng: RngState {
                counter: 7,
                seed_digest: seed_digest(42),
            },
            a_fail_streak: vec![0, 0],
            finish_order: vec![],
        }
    }

    #[test]
    fn version_triple_does_not_affect_state_hash() {
        let state = sample_state();
        let a = bundle(&state, "1.0.0", "1.2.0", "m0-minimal@1.0.0");
        let b = bundle(&state, "9.9.9", "2.0.0", "other@9.9.9");
        assert_eq!(a.state_hash, b.state_hash);
    }

    #[test]
    fn seed_digest_is_stable_8_bytes() {
        let d1 = seed_digest(42);
        let d2 = seed_digest(42);
        assert_eq!(d1, d2);
        assert_eq!(d1.len(), 8);
        assert_ne!(seed_digest(1), seed_digest(2));
    }

    /// P0 回归：`aFailStreak` 按队索引保序，值互换必须得到不同哈希（§5.12 第 801 行）。
    #[test]
    fn fail_streak_order_is_significant() {
        let mut a = sample_state();
        a.a_fail_streak = vec![1, 0];
        let mut b = sample_state();
        b.a_fail_streak = vec![0, 1];
        assert_ne!(state_hash(&a), state_hash(&b));
    }

    /// P0 回归：`finishOrder` 按出完先后保序，顺序互换必须得到不同哈希（§5.12 第 802 行）。
    #[test]
    fn finish_order_is_significant() {
        let mut a = sample_state();
        a.finish_order = vec![1, 3];
        let mut b = sample_state();
        b.finish_order = vec![3, 1];
        assert_ne!(state_hash(&a), state_hash(&b));
    }

    /// 对照：`seats`/`zones` 文档明确要求升序，乱序输入仍应得到同一哈希。
    #[test]
    fn zones_and_seats_disorder_input_hash_equal() {
        let a = sample_state();
        let mut b = sample_state();
        b.seats = vec![3, 1, 0, 2];
        b.zones = vec![
            ZoneEntry {
                seat: 1,
                zone_id: 0,
                cards: vec![5],
            },
            ZoneEntry {
                seat: 0,
                zone_id: 0,
                cards: vec![2, 0, 1],
            },
        ];
        assert_eq!(state_hash(&a), state_hash(&b));
    }
}
