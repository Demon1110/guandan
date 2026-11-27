//! 回放包（§7.7）：把一局的录制动作、逐步 `stateHash` 和版本信息打成一个 zip。
//!
//! 包内文件与设计稿一致：`manifest.json` / `actions.jsonl` / `hash_trace.jsonl` /
//! `logs.txt` / `client_summary.json`。导出时可以自证一遍：从种子重放全部动作，
//! 末态哈希必须与房间权威状态逐字节相等。

use std::io::{Cursor, Write};

use gf_engine::contract::Engine as _;
use gf_table_runtime::Room;

use crate::store::action_line;

/// 导出结果：zip 字节 + 自证结论。
pub struct ReplayPackage {
    pub bytes: Vec<u8>,
    pub steps: usize,
    pub final_hash: String,
    /// 重放末态与房间权威状态是否一致。
    pub verified: bool,
}

/// 把房间的完整录制打成一个可离线复现的 zip。
///
/// 只在房间结束后调用（`log` 已含整场动作）；重放失败不抛错，而是把
/// `verified=false` 写进 manifest，方便排查包本身。
pub fn build(room: &Room) -> Result<ReplayPackage, String> {
    let log = room.log();
    let authoritative = room
        .state_hash()
        .map_err(|err| format!("读权威哈希失败：{err}"))?;

    let mut trace = Vec::with_capacity(log.len());
    let mut replay_error: Option<String> = None;
    let mut replay_hash = [0u8; 32];

    match replay_trace(room) {
        Ok((lines, last)) => {
            trace = lines;
            replay_hash = last;
        }
        Err(err) => replay_error = Some(err),
    }

    let verified = replay_error.is_none() && replay_hash == authoritative;
    let final_hash = hex(&authoritative);
    let steps = log.len();

    let action_lines: Vec<String> = log
        .iter()
        .map(|record| action_line(room.match_id(), record))
        .collect();
    let hash_lines: Vec<String> = trace
        .iter()
        .map(|(room_seq, round_no, hash)| {
            format!(
                "{{\"roomSeq\":{room_seq},\"roundNo\":{round_no},\"stateHash\":\"{}\"}}",
                hex(hash)
            )
        })
        .collect();

    let manifest = serde_json::json!({
        "matchId": room.match_id(),
        "seed": room.seed(),
        "rulesetId": room.ruleset_id(),
        "rulesetVersion": room.ruleset_version(),
        "engineVersion": "gf-engine/0.1.0",
        "interpreterVersion": "m0",
        "seats": (0..4).map(|seat| serde_json::json!({"seat": seat, "player": format!("p{}", seat + 1)})).collect::<Vec<_>>(),
        "steps": steps,
        "finalStateHash": final_hash,
        "replayStateHash": if replay_error.is_none() { hex(&replay_hash) } else { String::new() },
        "verified": verified,
        "replayError": replay_error,
    });

    let client_summary = serde_json::json!({
        "note": "客户端上报通道未接入；本包只含服务端录制",
        "seats": (0..4).map(|seat| serde_json::json!({"seat": seat, "platform": "unknown", "lastSeatSeq": 0, "lastRoomSeq": 0})).collect::<Vec<_>>(),
    });

    let bytes = zip_bytes(
        &manifest.to_string(),
        &action_lines,
        &hash_lines,
        &client_summary.to_string(),
    )?;
    Ok(ReplayPackage {
        bytes,
        steps,
        final_hash,
        verified,
    })
}

/// 一步重放：`roomSeq`、`roundNo`、该步后的 `stateHash`。
type TraceLine = (u64, u16, [u8; 32]);

/// 从首局种子重放全部录制动作，返回每步 `(roomSeq, roundNo, stateHash)` 与末态哈希。
fn replay_trace(room: &Room) -> Result<(Vec<TraceLine>, [u8; 32]), String> {
    let engine = room.engine();
    let mut state = engine
        .deal(room.round_seed(1))
        .map_err(|err| format!("首发牌失败：{err}"))?;
    let mut lines = Vec::with_capacity(room.log().len());
    for record in room.log() {
        while state.round_no < record.round_no {
            state = engine
                .deal_next_round(&state, room.round_seed(state.round_no + 1))
                .map_err(|err| format!("续局失败（round {}）：{err}", state.round_no + 1))?;
        }
        state = engine
            .apply(state, record.action.clone())
            .map_err(|err| format!("重放失败（roomSeq {}）：{err}", record.room_seq))?;
        let hash = gf_table_runtime::state_hash_of(&state);
        lines.push((record.room_seq, record.round_no, hash));
    }
    Ok((lines, gf_table_runtime::state_hash_of(&state)))
}

fn zip_bytes(
    manifest: &str,
    actions: &[String],
    hashes: &[String],
    client_summary: &str,
) -> Result<Vec<u8>, String> {
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    // 行以 `\n` 结尾，空局也保留空文件语义。
    let mut actions_body = actions.join("\n");
    if !actions_body.is_empty() {
        actions_body.push('\n');
    }
    let mut hashes_body = hashes.join("\n");
    if !hashes_body.is_empty() {
        hashes_body.push('\n');
    }
    put_entry(&mut writer, options, "manifest.json", manifest)?;
    put_entry(&mut writer, options, "actions.jsonl", &actions_body)?;
    put_entry(&mut writer, options, "hash_trace.jsonl", &hashes_body)?;
    put_entry(
        &mut writer,
        options,
        "logs.txt",
        "本包只保留服务端录制；结构化日志经 stdout 输出，未按局切片。\n",
    )?;
    put_entry(&mut writer, options, "client_summary.json", client_summary)?;
    let cursor = writer
        .finish()
        .map_err(|err| format!("收尾 zip 失败：{err}"))?;
    Ok(cursor.into_inner())
}

fn put_entry(
    writer: &mut zip::ZipWriter<Cursor<Vec<u8>>>,
    options: zip::write::SimpleFileOptions,
    name: &str,
    body: &str,
) -> Result<(), String> {
    writer
        .start_file(name, options)
        .map_err(|err| format!("写 zip 条目 {name} 失败：{err}"))?;
    writer
        .write_all(body.as_bytes())
        .map_err(|err| format!("写 zip 内容 {name} 失败：{err}"))
}

fn hex(bytes: &[u8; 32]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use gf_table_runtime::RoomConfig;
    use std::io::Read;

    fn short_room() -> Room {
        let mut room = Room::new(RoomConfig {
            match_id: "m-replay".to_owned(),
            ruleset_version: gf_table_runtime::DEFAULT_RULESET_VERSION.to_owned(),
            seed: 7,
            players: vec![gf_table_runtime::PlayerKind::Human; 4],
        })
        .expect("建桌失败");
        room.start(0).expect("发牌失败");
        let turn = room.state().expect("状态").turn_seat;
        let action = room.legal_now().expect("取合法动作失败").remove(0);
        room.submit(turn, action, 1).expect("首手必合法");
        room
    }

    #[test]
    fn package_has_all_files_and_verifies() {
        let room = short_room();
        let pkg = build(&room).expect("导出失败");
        assert!(pkg.verified, "重放末态必须与权威一致");
        assert_eq!(pkg.steps, 1);

        let mut zip = zip::ZipArchive::new(Cursor::new(pkg.bytes)).expect("zip 可读");
        let names: Vec<String> = (0..zip.len())
            .map(|i| zip.by_index(i).unwrap().name().to_owned())
            .collect();
        for expected in [
            "manifest.json",
            "actions.jsonl",
            "hash_trace.jsonl",
            "logs.txt",
            "client_summary.json",
        ] {
            assert!(names.iter().any(|name| name == expected), "缺 {expected}");
        }

        let mut manifest = String::new();
        zip.by_name("manifest.json")
            .unwrap()
            .read_to_string(&mut manifest)
            .unwrap();
        let value: serde_json::Value = serde_json::from_str(&manifest).unwrap();
        assert_eq!(value["verified"], true);
        assert_eq!(value["seed"], 7);
        assert_eq!(value["finalStateHash"], pkg.final_hash);
    }
}
