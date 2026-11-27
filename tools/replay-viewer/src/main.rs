//! 离线回放器：读 `GET /api/replays/{matchId}` 导出的回放包，用同一份引擎从种子重演，
//! 逐步比对 `stateHash`，最后核对 manifest 里的末态哈希。
//!
//! 用法：`replay-viewer <matchId.zip>`，退出码 0 = 复现一致，1 = 有分歧，2 = 包不合法。

use std::fs::File;
use std::io::Read;
use std::process::ExitCode;

use gf_engine::contract::Action;
use gf_table_runtime::{PlayerKind, Room, RoomConfig};

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let Some(path) = args.next() else {
        eprintln!("用法：replay-viewer <回放包.zip>");
        return ExitCode::from(2);
    };
    match replay(&path) {
        Ok(summary) => {
            println!("{summary}");
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("回放失败：{err}");
            ExitCode::from(1)
        }
    }
}

fn replay(path: &str) -> Result<String, String> {
    let file = File::open(path).map_err(|err| format!("打不开 {path}：{err}"))?;
    let mut zip = zip::ZipArchive::new(file).map_err(|err| format!("不是合法 zip：{err}"))?;

    let manifest: serde_json::Value = serde_json::from_str(&read_entry(&mut zip, "manifest.json")?)
        .map_err(|err| format!("manifest.json 不是合法 JSON：{err}"))?;
    let match_id = manifest["matchId"].as_str().unwrap_or("unknown");
    let seed = manifest["seed"]
        .as_u64()
        .ok_or("manifest 缺 seed，无法重建发牌")?;
    let ruleset_version = manifest["rulesetVersion"]
        .as_str()
        .ok_or("manifest 缺 rulesetVersion")?;

    let mut room = Room::new(RoomConfig {
        match_id: match_id.to_owned(),
        ruleset_version: ruleset_version.to_owned(),
        seed,
        players: vec![PlayerKind::Human; 4],
    })
    .map_err(|err| format!("按 manifest 建桌失败：{err}"))?;
    room.start(0).map_err(|err| format!("首发牌失败：{err}"))?;

    let traces: Vec<String> = read_entry(&mut zip, "hash_trace.jsonl")?
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(str::to_owned)
        .collect();

    let mut applied = 0usize;
    let mut first_mismatch: Option<String> = None;
    for (index, line) in read_entry(&mut zip, "actions.jsonl")?
        .lines()
        .filter(|line| !line.trim().is_empty())
        .enumerate()
    {
        let record: serde_json::Value = serde_json::from_str(line)
            .map_err(|err| format!("第 {} 行动作不是 JSON：{err}", index + 1))?;
        let seat = u8::try_from(record["seat"].as_u64().ok_or("动作缺 seat")?)
            .map_err(|_| "seat 越界".to_owned())?;
        let round_no = u16::try_from(record["roundNo"].as_u64().unwrap_or(0)).unwrap_or(0);
        let room_seq = record["roomSeq"].as_u64().unwrap_or(0);

        let mut current_round = room
            .state()
            .map_err(|err| format!("读状态失败：{err}"))?
            .round_no;
        while current_round < round_no {
            room.next_round(0)
                .map_err(|err| format!("第 {round_no} 局续局失败：{err}"))?;
            current_round = room
                .state()
                .map_err(|err| format!("读状态失败：{err}"))?
                .round_no;
        }

        let action = parse_action(&record)?;
        room.replay_apply(seat, action)
            .map_err(|err| format!("roomSeq {room_seq} 重演被拒：{err}"))?;
        applied += 1;

        let actual = hex(&room
            .state_hash()
            .map_err(|err| format!("算哈希失败：{err}"))?);
        if let Some(expected_line) = traces.get(index) {
            let expected = serde_json::from_str::<serde_json::Value>(expected_line)
                .ok()
                .and_then(|value| value["stateHash"].as_str().map(str::to_owned));
            if expected.as_deref() != Some(actual.as_str()) && first_mismatch.is_none() {
                first_mismatch = Some(format!(
                    "首个分歧在 roomSeq {room_seq}（第 {} 步）：包内 {}，重放 {}",
                    index + 1,
                    expected.unwrap_or_else(|| "<缺>".to_owned()),
                    actual
                ));
            }
        }
    }

    let actual_final = hex(&room
        .state_hash()
        .map_err(|err| format!("算末态哈希失败：{err}"))?);
    let expected_final = manifest["finalStateHash"].as_str().unwrap_or("");
    let mut notes = vec![
        format!("场次 {match_id}：重演 {applied} 步"),
        format!("末态 stateHash {actual_final}"),
    ];
    if let Some(mismatch) = first_mismatch {
        return Err(format!("{mismatch}\n{}", notes.join("\n")));
    }
    if expected_final != actual_final {
        return Err(format!(
            "末态哈希不一致：包内 {expected_final}，重放 {actual_final}"
        ));
    }
    let verified = manifest["verified"].as_bool().unwrap_or(false);
    if !verified {
        notes.push("注意：服务端导出时自证未通过，包仍可能可用".to_owned());
    }
    notes.push("结果：逐步哈希一致，可离线复现".to_owned());
    Ok(notes.join("\n"))
}

fn parse_action(record: &serde_json::Value) -> Result<Action, String> {
    match record["kind"].as_str().unwrap_or("") {
        "play" => {
            let cards = record["payload"]["cards"]
                .as_array()
                .ok_or("play 缺 cards")?
                .iter()
                .map(|card| {
                    card.as_u64()
                        .and_then(|value| u16::try_from(value).ok())
                        .ok_or_else(|| "牌 id 越界".to_owned())
                })
                .collect::<Result<Vec<u16>, String>>()?;
            Ok(Action::Play { cards })
        }
        "pass" => Ok(Action::Pass),
        "tribute-return" => {
            let card = record["payload"]["card"]
                .as_u64()
                .and_then(|value| u16::try_from(value).ok())
                .ok_or("tribute-return 缺 card")?;
            Ok(Action::TributeReturn { card })
        }
        other => Err(format!("未知动作 kind：{other}")),
    }
}

fn read_entry(zip: &mut zip::ZipArchive<File>, name: &str) -> Result<String, String> {
    let mut entry = zip
        .by_name(name)
        .map_err(|err| format!("包内缺 {name}：{err}"))?;
    let mut body = String::new();
    entry
        .read_to_string(&mut body)
        .map_err(|err| format!("读 {name} 失败：{err}"))?;
    Ok(body)
}

fn hex(bytes: &[u8; 32]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
