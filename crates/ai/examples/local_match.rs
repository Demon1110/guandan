//! M1 硬 gate 的 CLI 演示：四 bot 建桌 → 发牌 → 出牌 → 结算 → 再来一局，全程无人值守。
//!
//! 运行：`cargo run -p gf-ai --example local_match -- [seed]`
//!
//! 输出每一步的座位与动作，最后打印两局结算摘要、`stateHash` 与回放自证。

use gf_ai::L0Bot;
use gf_engine::contract::Action;
use gf_engine::hash::present_8;
use gf_table_runtime::{Room, RoomConfig, StepOutcome};

fn main() {
    let seed = std::env::args()
        .nth(1)
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0x51ED_A101);

    let mut room = Room::new(RoomConfig::all_bots("match-local-demo", seed)).expect("建桌失败");
    for seat in 0..4u8 {
        room.attach_bot(seat, Box::new(L0Bot)).expect("装 bot 失败");
    }
    room.start(0).expect("发牌失败");
    println!("建桌完成：4 座位全 bot（AI-L0），种子 {seed:#x}，规则集经典掼蛋");

    let mut now_ms = 1000u64;
    for round in 1..=2u16 {
        println!("\n=== 第 {round} 局 ===");
        loop {
            match room.step(now_ms).expect("step 失败") {
                StepOutcome::Applied { seat, action, .. } => {
                    match action {
                        Action::Play { cards } => println!("座位 {seat} 出牌 {cards:?}"),
                        Action::Pass => println!("座位 {seat} 过牌"),
                    }
                    now_ms += 1000;
                }
                StepOutcome::Waiting { seat, deadline_ms } => {
                    panic!("纯 bot 局不得等待人类：seat={seat} deadline={deadline_ms}");
                }
                StepOutcome::Settled(summary) => {
                    println!(
                        "结算：名次 {:?}，头游所在队 {:?}，先出完队 {:?}，升级档 {:?}，分值 {}",
                        summary.ranks,
                        summary.winner_team,
                        summary.completed_team,
                        summary.step,
                        summary.points
                    );
                    println!(
                        "stateHash = {}（动作 {} 条，roomSeq {}）",
                        present_8(&room.state_hash().expect("哈希失败")),
                        room.log().len(),
                        room.room_seq()
                    );
                    break;
                }
                StepOutcome::MatchEnded(end) => {
                    println!("场次终止：{end:?}");
                    return;
                }
            }
        }
        if round == 1 {
            room.next_round(now_ms).expect("再来一局失败");
            println!(
                "再来一局：第 2 局已发牌（局号 {}）",
                room.state().expect("有状态").round_no
            );
        }
    }

    // 回放录制自证：全量日志重放必须与权威状态逐字节一致。
    let replay = room.replay_hash().expect("重放失败");
    let authoritative = room.state_hash().expect("哈希失败");
    println!(
        "\n回放自证：重放哈希 {} vs 权威哈希 {} → {}",
        present_8(&replay),
        present_8(&authoritative),
        if replay == authoritative {
            "一致"
        } else {
            "不一致"
        }
    );
    // 双产物（native / wasm）比对用：打全长 64 位十六进制，供 CI 的脚本抓取。
    println!("fullHash={}", hex_full(&authoritative));
    println!("records={}", room.log().len());
}

fn hex_full(bytes: &[u8; 32]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
