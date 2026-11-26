// 生成 L4 黄金用例：用 guandan-classic 夹具跑一局确定性对局（领出必出、跟牌一律过），
// 输出可直接入库的文本 fixture（seed / 每步动作 / 终局 stateHash）。
//
// 用法：cargo run -p gf-engine --example classic_golden > crates/engine/tests/fixtures/guandan-classic/golden/round1.txt
use gf_engine::contract::{Action, Engine, State};
use gf_engine::engine::{M0Engine, PHASE_SETTLE};
use gf_engine::hash::{canonical_bytes, state_hash, CanonicalState};
use gf_engine::ruleset::model::RuleSetBody;
use gf_engine::ruleset::parse::from_yaml;

const RULESET_YAML: &str = include_str!("../tests/fixtures/guandan-classic/ruleset.yaml");
const SEED: u64 = 0x5145_4E31;

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// 确定性策略：有 `pass` 就过（跟牌态），否则出牌数最少的第一条合法动作（领出态）。
fn pick(legal: &[Action]) -> Action {
    if legal.iter().any(|a| matches!(a, Action::Pass)) {
        return Action::Pass;
    }
    legal
        .iter()
        .filter(|a| matches!(a, Action::Play { .. }))
        .min_by_key(|a| match a {
            Action::Play { cards } => cards.len(),
            Action::Pass => 0,
            Action::TributeReturn { .. } => 1,
        })
        .cloned()
        .unwrap_or(Action::Pass)
}

fn main() {
    let body: RuleSetBody = from_yaml(RULESET_YAML).expect("夹具解析失败");
    let engine = M0Engine::new(body.clone());
    let mut state: State = engine.deal(SEED).expect("发牌失败");

    let mut lines = vec![
        "# guandan-classic L4 golden，输出由 crates/engine/examples/classic_golden.rs 生成，不得手改"
            .to_owned(),
        format!("ruleset={}", body.ruleset_id),
        format!("seed={SEED:016x}"),
    ];
    let mut steps = 0u32;
    while state.phase != PHASE_SETTLE {
        assert!(steps < 3000, "步数超界，疑似死循环");
        let seat = state.turn_seat;
        let legal = engine.legal_actions(&state).expect("合法动作枚举失败");
        let action = pick(&legal);
        let line = match &action {
            Action::Pass => format!("{seat} pass"),
            Action::Play { cards } => {
                let ids: Vec<String> = cards.iter().map(|c| c.to_string()).collect();
                format!("{seat} {}", ids.join(","))
            }
            Action::TributeReturn { card } => format!("{seat} return {card}"),
        };
        lines.push(line);
        state = engine.apply(state, action).expect("apply 失败");
        steps += 1;
    }
    let hash = state_hash(&CanonicalState::from(&state));
    let bytes = canonical_bytes(&CanonicalState::from(&state));
    lines.push(format!("steps={steps}"));
    lines.push(format!("phase={}", state.phase));
    lines.push(format!("rounds={}", state.round_no));
    lines.push(format!("finishOrder={:?}", state.finish_order));
    lines.push(format!("bytes={}", bytes.len()));
    lines.push(format!("hash={}", hex(&hash)));
    println!("{}", lines.join("\n"));
}
