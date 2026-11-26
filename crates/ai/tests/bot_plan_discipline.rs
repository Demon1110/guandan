//! 「bot 按 recommend 最优拆分出牌」的真实对局审计。
//!
//! 单元用例只能证明 `plan` 里的纯函数口径对；这里把 L0 包一层审计 bot，
//! 在一整局四 bot 对局里逐手核对两件事：
//! 1. **不拆对子**：出牌要么正好是拆分里某一整组，要么全是散张；
//! 2. **从最小单元出**：领出时出的就是拆分里牌力最小的那一手（同牌力先走张数多的）。
//!
//! 任何一手违反都会记进审计日志，测试结尾按两类分别断言。

use std::sync::{Arc, Mutex};

use gf_ai::{view_scan, L0Bot};
use gf_engine::contract::Action;
use gf_table_runtime::{
    AutoReason, BotContext, BotPolicy, HandPlan, Room, RoomConfig, StepOutcome,
};

#[derive(Default)]
struct Audit {
    /// 拆组出牌（违反「不拆对子」）。
    splits: Mutex<Vec<String>>,
    /// 领出没走最小单元（违反「从最小出」）。
    leads_not_smallest: Mutex<Vec<String>>,
    /// 计入统计的领出 / 跟牌手数。
    leads: Mutex<u32>,
    follows: Mutex<u32>,
}

struct AuditBot {
    logs: Arc<Audit>,
}

impl BotPolicy for AuditBot {
    fn decide(&self, view_bytes: &[u8], seat: u8) -> Action {
        L0Bot.decide(view_bytes, seat)
    }

    fn decide_with_legal(&self, view_bytes: &[u8], seat: u8, legal: &[Action]) -> Action {
        L0Bot.decide_with_legal(view_bytes, seat, legal)
    }

    fn decide_with_plan(
        &self,
        view_bytes: &[u8],
        seat: u8,
        legal: &[Action],
        ctx: &BotContext,
        plan: Option<&HandPlan>,
    ) -> Action {
        let action = L0Bot.decide_with_plan(view_bytes, seat, legal, ctx, plan);
        let (Action::Play { cards }, Some(plan)) = (&action, plan) else {
            return action;
        };
        let sorted_cards = {
            let mut v = cards.clone();
            v.sort_unstable();
            v
        };

        // 1) 不拆对子：整组拿走，或者全落在散张里。
        let exact_group = plan.groups.iter().any(|g| {
            let mut v = g.cards.clone();
            v.sort_unstable();
            v == sorted_cards
        });
        let all_loose = cards
            .iter()
            .all(|c| plan.singles.iter().any(|s| s.card == *c));
        if !exact_group && !all_loose {
            self.logs
                .splits
                .lock()
                .unwrap()
                .push(format!("seat {seat} 出 {cards:?} 既不是整组也不是散张"));
        }

        // 2) 领出必须走最小单元：复算一遍 `lead_from_plan` 的挑选口径。
        if view_scan::is_following(view_bytes, seat) {
            *self.logs.follows.lock().unwrap() += 1;
        } else {
            *self.logs.leads.lock().unwrap() += 1;
            let mut best: Option<(u32, usize, Vec<u16>)> = None;
            let mut consider = |unit_power: u32, unit: &[u16]| {
                if unit.is_empty() {
                    return;
                }
                let mut v = unit.to_vec();
                v.sort_unstable();
                let better = match &best {
                    None => true,
                    Some((b_power, b_len, b_cards)) => {
                        unit_power < *b_power
                            || (unit_power == *b_power
                                && (v.len() > *b_len || (v.len() == *b_len && v < *b_cards)))
                    }
                };
                if better {
                    best = Some((unit_power, v.len(), v));
                }
            };
            for g in &plan.groups {
                if !g.pattern_id.starts_with("bomb") && g.pattern_id != "joker-bomb" {
                    consider(g.power, &g.cards);
                }
            }
            for single in &plan.singles {
                consider(single.power, std::slice::from_ref(&single.card));
            }
            if let Some((_, len, expected)) = best {
                if expected != sorted_cards || len != cards.len() {
                    self.logs.leads_not_smallest.lock().unwrap().push(format!(
                        "seat {seat} 领出 {cards:?}，按拆分应先走 {expected:?}"
                    ));
                }
            }
        }

        action
    }
}

/// 四 bot 无人值守推完一局，返回动作数与审计日志。
fn drive(room: &mut Room, logs: &Arc<Audit>) -> u64 {
    let mut now_ms = 1_000u64;
    let mut actions = 0u64;
    loop {
        match room.step(now_ms).expect("step 失败") {
            StepOutcome::Applied { .. } => {
                actions += 1;
                now_ms += 1_000;
            }
            StepOutcome::Waiting { seat, .. } => panic!("纯 bot 局不得等待人类：seat={seat}"),
            StepOutcome::Settled(_) | StepOutcome::MatchEnded(_) => break,
        }
        assert!(actions < 4000, "动作数超上界");
    }
    let _ = logs;
    actions
}

#[test]
fn l0_bot_plays_by_plan_without_splitting_groups() {
    let logs = Arc::new(Audit::default());
    let mut room = Room::new(RoomConfig::all_bots("match-plan-audit", 0x19)).expect("建桌失败");
    for seat in 0..4u8 {
        room.attach_bot(
            seat,
            Box::new(AuditBot {
                logs: Arc::clone(&logs),
            }),
        )
        .expect("挂 bot 失败");
    }
    room.start(0).expect("发牌失败");

    let actions = drive(&mut room, &logs);
    assert!(actions > 30, "动作数太少：{actions}");

    let splits = logs.splits.lock().unwrap().clone();
    let lead_violations = logs.leads_not_smallest.lock().unwrap().clone();
    let lead_count = *logs.leads.lock().unwrap();
    let follow_count = *logs.follows.lock().unwrap();
    println!(
        "动作 {actions} 手：领出 {lead_count} / 跟牌 {follow_count}；拆组 {} 手",
        splits.len()
    );
    for line in splits.iter().take(10) {
        println!("  拆组：{line}");
    }
    for line in lead_violations.iter().take(10) {
        println!("  领出偏离：{line}");
    }

    assert!(lead_count > 0, "一局里应当出现领出");
    assert!(splits.is_empty(), "L0 不许把成组的牌拆开打");
    assert!(lead_violations.is_empty(), "领出应当先走拆分里最小的那一手");

    // 房间侧也不该记任何 bot 非法退化（plan 出的手必须能过 preview 复核）。
    let illegal = room
        .auto_actions()
        .iter()
        .filter(|a| a.reason == AutoReason::BotIllegal)
        .count();
    assert_eq!(illegal, 0, "按拆分出的手不应被房间判非法");
}
