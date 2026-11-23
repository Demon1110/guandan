//! §5.12 定值向量验收（第 821–828 行）与 `viewHash`  smoke。
//!
//! 对编码器**实际产出**的字节算长度与 SHA-256，再与字面值比较；
//! 不把期望字节硬编码成「已知答案」当输入。

use gf_engine::contract::ZoneId;
use gf_engine::ruleset::model::ViewField;
use gf_engine::view::{encode_canonical, present_8, view_hash, ViewData, ViewLastPlay};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

const EXPECTED_SHA256: &str = "9e0b756d600f8eb6b74b098f804735c752a2232f0ca3772673a3a4da51afcc72";
const EXPECTED_LEN: usize = 129;

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

#[test]
fn canonical_view_vector_matches_fixture() {
    // 白名单序：hand, seat.levels, discard, lastPlay（§5.12 第 821 行）。
    let fields = vec![
        ViewField::Zone(ZoneId("hand".to_owned())),
        ViewField::SeatLevels,
        ViewField::Zone(ZoneId("discard".to_owned())),
        ViewField::LastPlay,
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
        seat_levels: None,
        last_play: None,
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
