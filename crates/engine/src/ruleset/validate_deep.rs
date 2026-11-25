//! L2 / L3 / L4 校验（§5.4 第 607–609 行）。
//!
//! - **L2 引用完整性 + 静态牌堆**：ID 全部可解析、`lifecycle` 每个「有动作的 phase」都有
//!   可执行原语（按 `kind` 判定，不看 id 字符串）、`seats.teams` 覆盖全部座位且不重叠、
//!   牌堆张数公式自洽且不超过各 `hand` 区容量之和。
//! - **L3 可玩性**：随机发牌 + 随机合法动作跑 `l3_rounds` 局，必须全部终局、非法动作率 0、
//!   单次 `legal_actions` 候选数不超过 4096；失败局记 seed 与最小动作序列。
//! - **L4 黄金回归**：按黄金用例逐条重放，`stateHash` 必须一致。
//!
//! 未跑完的级别不进 `ran_levels`（§5.4 第 578 行语义），失败也不伪造通过。

use crate::contract::{Action, Engine, ErrorCode, Finding, Level, Validation};
use crate::engine::{M0Engine, PHASE_SETTLE};
use crate::hash::{state_hash, CanonicalState};
use crate::ruleset::model::{ActionKind, LegalFrom, Phase, RuleSetBody, Settle, ZoneScope};
use std::collections::HashSet;

/// L3 的跑法与上限（默认值即 §5.4 判据：200 局、候选峰值 4096）。
#[derive(Clone, Debug)]
pub struct ValidateOptions {
    pub l3_rounds: usize,
    pub l3_seed: u64,
    /// 单局动作上界（超过即判 `RS_NO_TERMINATION`）。
    pub l3_step_cap: usize,
    /// 单次 `legal_actions` 候选数上界（§5.4：超 4096 即 L3 失败）。
    pub l3_candidate_cap: usize,
}

impl Default for ValidateOptions {
    fn default() -> Self {
        Self {
            l3_rounds: 200,
            l3_seed: 0x51ED_A1CF_0000_0001,
            l3_step_cap: 2000,
            l3_candidate_cap: 4096,
        }
    }
}

/// 黄金用例（L4）：一条 action log + 期望的终局 `stateHash`。
#[derive(Clone, Debug)]
pub struct GoldenCase {
    pub name: String,
    pub ruleset_id: String,
    pub seed: u64,
    pub actions: Vec<Action>,
    pub expected_hash: [u8; 32],
}

/// 把文本黄金夹具（`crates/engine/examples/classic_golden.rs` 的输出格式）解析成用例。
///
/// 格式：`#` 注释行；`ruleset=<id>`；`seed=<16 位 hex>`；每步 `<seat> pass` 或
/// `<seat> <cardId,cardId,...>`；`hash=<64 位 hex>`（其余 `k=v` 行忽略）。
pub fn parse_golden(name: &str, src: &str) -> Result<GoldenCase, String> {
    let mut ruleset_id = String::new();
    let mut seed = 0u64;
    let mut hash: Option<[u8; 32]> = None;
    let mut actions = Vec::new();
    for line in src.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(rest) = line.strip_prefix("ruleset=") {
            ruleset_id = rest.to_owned();
            continue;
        }
        if let Some(rest) = line.strip_prefix("seed=") {
            seed = u64::from_str_radix(rest, 16).map_err(|e| format!("seed 解析失败：{e}"))?;
            continue;
        }
        if let Some(rest) = line.strip_prefix("hash=") {
            if rest.len() != 64 {
                return Err(format!("hash 长度应为 64，实际 {}", rest.len()));
            }
            let mut bytes = [0u8; 32];
            for i in 0..32 {
                bytes[i] = u8::from_str_radix(&rest[i * 2..i * 2 + 2], 16)
                    .map_err(|e| format!("hash 第 {i} 字节解析失败：{e}"))?;
            }
            hash = Some(bytes);
            continue;
        }
        // 其余 `k=v` 是诊断字段（steps / phase / rounds / finishOrder / bytes）。
        if line
            .split(' ')
            .next()
            .is_some_and(|first| first.contains('='))
        {
            continue;
        }
        // 动作行。
        let mut parts = line.splitn(2, ' ');
        let seat: u8 = parts
            .next()
            .ok_or_else(|| "动作行缺座位".to_owned())?
            .parse()
            .map_err(|e| format!("座位解析失败：{e}"))?;
        let _ = seat; // 座位由引擎状态决定，重放时不比对（非法会由 apply 报错）。
        let payload = parts.next().ok_or_else(|| "动作行缺动作".to_owned())?;
        if payload == "pass" {
            actions.push(Action::Pass);
        } else {
            let cards = payload
                .split(',')
                .map(|s| s.parse::<u16>().map_err(|e| format!("牌 id 解析失败：{e}")))
                .collect::<Result<Vec<_>, _>>()?;
            if cards.is_empty() {
                return Err("出牌动作牌列表为空".to_owned());
            }
            actions.push(Action::Play { cards });
        }
    }
    let expected_hash = hash.ok_or_else(|| "缺 hash= 行".to_owned())?;
    if ruleset_id.is_empty() {
        return Err("缺 ruleset= 行".to_owned());
    }
    Ok(GoldenCase {
        name: name.to_owned(),
        ruleset_id,
        seed,
        actions,
        expected_hash,
    })
}

/// 内置黄金用例（编译期内嵌，保证 L4 不依赖运行期文件路径）。
pub fn builtin_golden() -> Vec<GoldenCase> {
    let src = include_str!("../../tests/fixtures/guandan-classic/golden/round1.txt");
    match parse_golden("guandan-classic/round1", src) {
        Ok(case) => vec![case],
        Err(e) => {
            // 夹具坏了属于仓库缺陷，直接 panic 而不是静默跳过 L4。
            panic!("内置黄金夹具解析失败：{e}");
        }
    }
}

/// L2：引用完整性 + 静态牌堆。
pub fn validate_l2(ruleset: &RuleSetBody, out: &mut Validation) {
    let baseline = out.findings.len();
    let mut push = |code: ErrorCode, location: &str, message: String| {
        out.findings.push(Finding {
            level: Level::L2,
            code,
            severity: code.default_severity(),
            location: location.to_owned(),
            message,
        });
    };

    // ① seats.teams 覆盖全部座位、互不重叠（`RS_TEAM_COVERAGE_INVALID`）。
    let mut covered = HashSet::new();
    let mut overlap = Vec::new();
    let mut out_of_range = Vec::new();
    for (i, team) in ruleset.seats.teams.iter().enumerate() {
        for seat in team {
            if *seat >= ruleset.seats.count {
                out_of_range.push(format!("teams[{i}] 含越界座位 {seat}"));
            }
            if !covered.insert(*seat) {
                overlap.push(*seat);
            }
        }
    }
    let missing: Vec<u8> = (0..ruleset.seats.count)
        .filter(|s| !covered.contains(s))
        .collect();
    if !missing.is_empty() || !overlap.is_empty() || !out_of_range.is_empty() {
        push(
            ErrorCode::RsTeamCoverageInvalid,
            "seats.teams",
            format!(
                "队伍必须覆盖全部 {} 个座位且不重叠：缺座 {:?}、重复 {:?}{}{}",
                ruleset.seats.count,
                missing,
                overlap,
                if out_of_range.is_empty() { "" } else { "、" },
                out_of_range.join("、")
            ),
        );
    }

    // ② 引用完整性：ID 能在索引里找到。
    let pattern_ids: HashSet<&str> = ruleset.patterns.iter().map(|p| p.id.0.as_str()).collect();
    let zone_ids: HashSet<&str> = ruleset.zones.iter().map(|z| z.id.0.as_str()).collect();
    for (i, pid) in ruleset.compare.ladder.iter().enumerate() {
        if !pattern_ids.contains(pid.0.as_str()) {
            push(
                ErrorCode::RsRefMissing,
                &format!("compare.ladder[{i}]"),
                format!("牌型 `{}` 不在 patterns 索引里", pid.0),
            );
        }
    }
    for (i, action) in ruleset.actions.iter().enumerate() {
        if let LegalFrom::Zone(zone) = &action.legal_from {
            if !zone_ids.contains(zone.0.as_str()) {
                push(
                    ErrorCode::RsRefMissing,
                    &format!("actions[{i}].legalFrom"),
                    format!("区 `{}` 不在 zones 索引里", zone.0),
                );
            }
        }
    }
    if let Some(deal) = ruleset.flow.as_ref().and_then(|f| f.deal.as_ref()) {
        if let Some(zone) = &deal.zone {
            if !zone_ids.contains(zone.0.as_str()) {
                push(
                    ErrorCode::RsRefMissing,
                    "flow.deal.zone",
                    format!("区 `{}` 不在 zones 索引里", zone.0),
                );
            }
        }
    }

    // ③ lifecycle：有动作的 phase 必须有合法入口（按 kind 判定，不看 id 字符串）。
    // 按 kind 判定：`kind=play` 的动作以 `legalFrom: patterns` 指向 play 段即可，
    // 不能要求字符串恰好等于 phase 名（第三十四次变更的口径）。
    let play_phase_has_entry = ruleset.actions.iter().any(|a| a.kind == ActionKind::Play);
    if ruleset.lifecycle.contains(&Phase::Play) && !play_phase_has_entry {
        push(
            ErrorCode::RsPhaseUnimplemented,
            "lifecycle",
            "play 段没有任何 `kind=play` 的动作指向它（零动作段只有 deal/tribute/settle 豁免）"
                .to_owned(),
        );
    }

    // ④ 条件必填：`settle = level-up` 时必须有级牌表。
    if matches!(ruleset.settle, Settle::LevelUp { .. }) && ruleset.level_table.is_none() {
        push(
            ErrorCode::RsRefMissing,
            "levelTable",
            "settle 引用 level-up 时必须提供唯一 levelTable".to_owned(),
        );
    }

    // ⑤ 静态牌堆：公式张数自洽，且不超过 hand 区容量之和。
    let ranks = ruleset
        .deck
        .ranks
        .len()
        .saturating_sub(ruleset.deck.exclude.len());
    let formula = ranks * ruleset.deck.suits.len() * ruleset.deck.copies as usize
        + ruleset.deck.jokers as usize * ruleset.deck.copies as usize;
    if let Some(assert) = ruleset.flow.as_ref().and_then(|f| f.assert.as_ref()) {
        if let Some(declared) = assert.card_count {
            if declared as usize != formula {
                push(
                    ErrorCode::RsDeckCountMismatch,
                    "flow.assert.cardCount",
                    format!(
                        "声明总张数 {declared} 与公式 (ranks−exclude)×suits×copies + jokers×copies = {formula} 不符"
                    ),
                );
            }
        }
    }
    // 私有区按 (seat, zone) 实例化，容量上限要和座位数相乘才是整桌张数上界。
    let hand_capacity: i32 = ruleset
        .zones
        .iter()
        .filter(|z| z.scope == ZoneScope::Private)
        .map(|z| z.capacity)
        .filter(|c| *c > 0)
        .sum::<i32>()
        .saturating_mul(i32::from(ruleset.seats.count));
    if hand_capacity > 0 && formula as i32 > hand_capacity {
        push(
            ErrorCode::RsDeckCountMismatch,
            "zones",
            format!("牌堆 {formula} 张超过各私有（手牌）区容量之和 {hand_capacity}"),
        );
    }

    // 只有一条 findings 都没有才算「L2 跑完」（§5.4 第 578 行语义）。
    if out.findings.len() == baseline {
        out.ran_levels.insert(Level::L2);
    }
}

/// xorshift64*：L3 用的确定性伪随机（同一 seed 必得同一局）。
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed | 1)
    }

    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

/// L3：随机合法动作跑 `opts.l3_rounds` 局，要求全部终局且候选数不爆。
pub fn validate_l3(ruleset: &RuleSetBody, opts: &ValidateOptions, out: &mut Validation) {
    let engine = M0Engine::new(ruleset.clone());
    let mut peak_candidates = 0usize;
    let mut failures: Vec<String> = Vec::new();

    for round in 0..opts.l3_rounds {
        let round_seed = opts
            .l3_seed
            .wrapping_mul(0x9E37_79B9_7F4A_7C15)
            .wrapping_add(round as u64);
        let mut rng = Rng::new(round_seed);
        let mut state = match engine.deal(round_seed) {
            Ok(s) => s,
            Err(e) => {
                failures.push(format!("第 {round} 局发牌失败：{e:?}"));
                continue;
            }
        };
        let mut trail: Vec<String> = Vec::new();
        let mut steps = 0usize;
        let mut fatal = None;
        while state.phase != PHASE_SETTLE {
            if steps >= opts.l3_step_cap {
                fatal = Some(format!(
                    "第 {round} 局超过 {} 步仍未终局（seed={round_seed:016x}）",
                    opts.l3_step_cap
                ));
                break;
            }
            let legal = match engine.legal_actions(&state) {
                Ok(l) => l,
                Err(e) => {
                    fatal = Some(format!("第 {round} 局 legal_actions 失败：{e:?}"));
                    break;
                }
            };
            if legal.is_empty() {
                fatal = Some(format!(
                    "第 {round} 局出现空合法集（seed={round_seed:016x}，第 {steps} 步）"
                ));
                break;
            }
            peak_candidates = peak_candidates.max(legal.len());
            if legal.len() > opts.l3_candidate_cap {
                fatal = Some(format!(
                    "第 {round} 局候选数 {} 超过上限 {}（seed={round_seed:016x}）",
                    legal.len(),
                    opts.l3_candidate_cap
                ));
                break;
            }
            let pick = legal[rng.below(legal.len())].clone();
            trail.push(format!("{steps}:{:?}", pick));
            match engine.apply(state, pick) {
                Ok(next) => state = next,
                Err(e) => {
                    // 只从 legal 里取动作，理论上不可能失败；一旦失败就是非法动作率 > 0。
                    fatal = Some(format!(
                        "第 {round} 局合法集里的动作被 apply 拒绝（seed={round_seed:016x}）：{e:?}"
                    ));
                    break;
                }
            }
            steps += 1;
        }
        if let Some(msg) = fatal {
            // §5.4 要求失败可复现：记录 seed 与最小动作序列（截断到 40 步，避免报告爆掉）。
            let tail: Vec<String> = trail.iter().rev().take(40).rev().cloned().collect();
            failures.push(format!("{msg}\n  末段动作序列：{}", tail.join(" | ")));
        }
    }

    if !failures.is_empty() {
        let (code, note) = if failures.iter().any(|f| f.contains("超过")) {
            (ErrorCode::RsNoTermination, "存在无法终局的局")
        } else {
            (ErrorCode::RsIllegalActionRate, "存在非法动作或空合法集")
        };
        out.findings.push(Finding {
            level: Level::L3,
            code,
            severity: code.default_severity(),
            location: "l3/dry-run".to_owned(),
            message: format!(
                "{note}（{}/{} 局失败）：\n{}",
                failures.len(),
                opts.l3_rounds,
                failures.join("\n")
            ),
        });
        return; // 有失败就不记 ran_levels（没跑完的级别不算通过）。
    }
    out.ran_levels.insert(Level::L3);
    let _ = peak_candidates;
}

/// L4：黄金用例逐条重放，`stateHash` 必须一致。
pub fn validate_l4(ruleset: &RuleSetBody, cases: &[GoldenCase], out: &mut Validation) {
    let engine = M0Engine::new(ruleset.clone());
    let mut mismatches: Vec<String> = Vec::new();
    for case in cases {
        if case.ruleset_id != ruleset.ruleset_id {
            mismatches.push(format!(
                "`{}` 绑定规则集 `{}`，与当前 `{}` 不符",
                case.name, case.ruleset_id, ruleset.ruleset_id
            ));
            continue;
        }
        let mut state = match engine.deal(case.seed) {
            Ok(s) => s,
            Err(e) => {
                mismatches.push(format!("`{}` 发牌失败：{e:?}", case.name));
                continue;
            }
        };
        let mut first_diff: Option<(usize, String)> = None;
        for (i, action) in case.actions.iter().enumerate() {
            match engine.apply(state.clone(), action.clone()) {
                Ok(next) => state = next,
                Err(e) => {
                    first_diff = Some((i, format!("{e:?}")));
                    break;
                }
            }
        }
        if let Some((i, err)) = first_diff {
            mismatches.push(format!("`{}` 第 {i} 步重放失败：{err}", case.name));
            continue;
        }
        let actual = state_hash(&CanonicalState::from(&state));
        if actual != case.expected_hash {
            let hex = |b: &[u8; 32]| b.iter().map(|x| format!("{x:02x}")).collect::<String>();
            mismatches.push(format!(
                "`{}` stateHash 不一致：实际={} 期望={}",
                case.name,
                hex(&actual),
                hex(&case.expected_hash)
            ));
        }
    }
    if mismatches.is_empty() {
        out.ran_levels.insert(Level::L4);
    } else {
        out.findings.push(Finding {
            level: Level::L4,
            code: ErrorCode::RsL4Mismatch,
            severity: ErrorCode::RsL4Mismatch.default_severity(),
            location: "l4/golden".to_owned(),
            message: mismatches.join("\n"),
        });
    }
}
