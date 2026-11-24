//! §5.12 定值向量验收（第 821–828 行）与 `viewHash`  smoke。
//!
//! 对编码器**实际产出**的字节算长度与 SHA-256，再与字面值比较；
//! 不把期望字节硬编码成「已知答案」当输入。

use gf_engine::contract::ZoneId;
use gf_engine::ruleset::model::ViewField;
use gf_engine::view::{encode_canonical, present_8, view_hash, ViewData, ViewLastPlay};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

const EXPECTED_SHA256: &str = "36e7bdc6d3ea4bba20ec571f3034c2663ec253909c56b4d649a2713c6aeeab51";
const EXPECTED_LEN: usize = 169;

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

#[test]
fn canonical_view_vector_matches_fixture() {
    // 白名单序：hand, seat.levels, discard, lastPlay, round.level, round.attackingTeam
    // （§5.12 第 882 行定值向量；第二十二次变更新增两键）。
    let fields = vec![
        ViewField::Zone(ZoneId("hand".to_owned())),
        ViewField::SeatLevels,
        ViewField::Zone(ZoneId("discard".to_owned())),
        ViewField::LastPlay,
        ViewField::RoundLevel,
        ViewField::RoundAttackingTeam,
    ];

    let mut zones = BTreeMap::new();
    zones.insert("hand".to_owned(), vec![0, 1, 2]);
    zones.insert("discard".to_owned(), vec![10, 11]);
    let data = ViewData {
        zones,
        seat_levels: Some(vec![2, 2]),
        last_play: Some(ViewLastPlay {
            seat: 1,
            pattern_id: "single".to_owned(),
            cards: vec![5],
        }),
        round_level: Some(2),
        round_attacking_team: Some(0),
    };

    let bytes = encode_canonical(&fields, &data);
    let actual = String::from_utf8_lossy(&bytes).into_owned();
    let actual_sha = sha256_hex(&bytes);

    println!(
        "canonical bytes ({}/{} bytes): {actual}",
        bytes.len(),
        EXPECTED_LEN
    );
    println!("actual SHA-256:   {actual_sha}");
    println!("expected SHA-256: {EXPECTED_SHA256}");

    assert_eq!(bytes.len(), EXPECTED_LEN, "字节长度与定值向量不符");
    assert_eq!(actual_sha, EXPECTED_SHA256, "SHA-256 指纹与定值向量不符");
}

#[test]
fn view_hash_is_16_hex_chars_and_deterministic() {
    let fields = vec![ViewField::Zone(ZoneId("hand".to_owned()))];
    let mut zones = BTreeMap::new();
    zones.insert("hand".to_owned(), vec![0, 1, 2]);
    let data = ViewData {
        zones,
        ..Default::default()
    };
    let bytes = encode_canonical(&fields, &data);

    let h1 = view_hash(1, 42, &bytes);
    let h2 = view_hash(1, 42, &bytes);
    assert_eq!(h1, h2);
    let hex = present_8(&h1);
    assert_eq!(hex.len(), 16);

    // seat / roomSeq 不同，viewHash 应不同。
    let h3 = view_hash(2, 42, &bytes);
    assert_ne!(h1, h3);
}

/// 无级牌状态（score-only 局）：`round.level` / `round.attackingTeam` 两键都不出现，
/// 不写 null、不留占位（§5.12 第 879–880 行）。
#[test]
fn round_keys_absent_without_level_state() {
    let fields = vec![ViewField::RoundLevel, ViewField::RoundAttackingTeam];
    let data = ViewData::default();
    let bytes = encode_canonical(&fields, &data);
    assert_eq!(bytes, b"{}");
    assert!(!bytes.windows(11).any(|w| w == b"round.level"));
}
