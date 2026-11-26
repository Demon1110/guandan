//! 素材包验收：加密往返、错钥拒绝、篡改检测、签名、header 只读。

use std::path::PathBuf;

use ed25519_dalek::{SigningKey, VerifyingKey};
use gf_deckart::pack::{builtin_key, open, pack_dir, pack_theme, read_info, unpack, PackOptions};
use gf_deckart::{Face, LevelState, Rank, Suit, Theme};

fn theme_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../themes/defaults")
}

fn tmp(tag: &str) -> PathBuf {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("gfdeck-{tag}-{stamp}"))
}

fn sign_key() -> SigningKey {
    SigningKey::from_bytes(&[7u8; 32])
}

#[test]
fn pack_unpack_roundtrip_renders_identically() {
    let dir = theme_dir();
    let out = tmp("roundtrip").with_extension("gfdeck");
    let raw_key = builtin_key();
    let opts = PackOptions {
        key: &raw_key.0,
        key_id: &raw_key.1,
        signing_key: Some(&sign_key()),
    };
    let info = pack_dir(&dir, &out, &opts).unwrap();
    assert!(info.encrypted);
    assert!(info.signed);
    assert_eq!(info.entries, 24, "theme.json + 23 个素材");

    let direct = Theme::load_dir(&dir).unwrap();
    let from_pack = open(&out, &raw_key.0, Some(&sign_key().verifying_key())).unwrap();
    assert_eq!(from_pack.fingerprint(), direct.fingerprint());

    let level = Some(LevelState::new(Rank::new(8).unwrap()));
    let face = Face::normal(Suit::Heart, Rank::new(8).unwrap());
    let a = gf_deckart::render_face(&direct, face, level).unwrap();
    let b = gf_deckart::render_face(&from_pack, face, level).unwrap();
    assert_eq!(a, b, "包内渲染必须与目录渲染逐字节一致");
}

#[test]
fn unpack_restores_loadable_directory() {
    let dir = theme_dir();
    let out = tmp("unpack").with_extension("gfdeck");
    let target = tmp("unpack-out");
    let (key, key_id) = builtin_key();
    pack_dir(
        &dir,
        &out,
        &PackOptions {
            key: &key,
            key_id: &key_id,
            signing_key: None,
        },
    )
    .unwrap();
    unpack(&out, &target, &key, None).unwrap();

    let restored = Theme::load_dir(&target).unwrap();
    assert_eq!(restored.meta().id, "defaults");
    assert_eq!(restored.meta().asset_paths().len(), 23);
}

#[test]
fn wrong_key_is_rejected() {
    let dir = theme_dir();
    let out = tmp("wrongkey").with_extension("gfdeck");
    let (key, key_id) = builtin_key();
    pack_dir(
        &dir,
        &out,
        &PackOptions {
            key: &key,
            key_id: &key_id,
            signing_key: None,
        },
    )
    .unwrap();
    let mut other = key;
    other[0] ^= 0xFF;
    let err = open(&out, &other, None).unwrap_err();
    assert_eq!(err.code(), "DK-PACK-DECRYPT");
}

#[test]
fn tampered_payload_is_detected() {
    let dir = theme_dir();
    let out = tmp("tamper").with_extension("gfdeck");
    let (key, key_id) = builtin_key();
    pack_dir(
        &dir,
        &out,
        &PackOptions {
            key: &key,
            key_id: &key_id,
            signing_key: None,
        },
    )
    .unwrap();
    let mut bytes = std::fs::read(&out).unwrap();
    let last = bytes.len() - 1;
    bytes[last] ^= 0x01;
    std::fs::write(&out, &bytes).unwrap();
    let err = open(&out, &key, None).unwrap_err();
    assert_eq!(err.code(), "DK-PACK-DECRYPT", "改动密文必须解不开");
}

#[test]
fn signature_requires_matching_public_key() {
    let dir = theme_dir();
    let out = tmp("sig").with_extension("gfdeck");
    let (key, key_id) = builtin_key();
    let sk = sign_key();
    pack_dir(
        &dir,
        &out,
        &PackOptions {
            key: &key,
            key_id: &key_id,
            signing_key: Some(&sk),
        },
    )
    .unwrap();
    let good: VerifyingKey = sign_key().verifying_key();
    assert!(open(&out, &key, Some(&good)).is_ok());

    let bad = SigningKey::from_bytes(&[9u8; 32]).verifying_key();
    let err = open(&out, &key, Some(&bad)).unwrap_err();
    assert_eq!(err.code(), "DK-PACK-SIGNATURE");

    let err = open(&out, &key, None).unwrap_err();
    assert_eq!(err.code(), "DK-PACK-SIGNATURE", "签名包不给公钥必须拒绝");
}

#[test]
fn info_is_readable_without_key() {
    let dir = theme_dir();
    let out = tmp("info").with_extension("gfdeck");
    let (key, key_id) = builtin_key();
    pack_dir(
        &dir,
        &out,
        &PackOptions {
            key: &key,
            key_id: &key_id,
            signing_key: None,
        },
    )
    .unwrap();
    let info = read_info(&out).unwrap();
    assert_eq!(info.theme_id, "defaults");
    let expect_version = Theme::load_dir(&dir).unwrap().meta().version;
    assert_eq!(info.theme_version, expect_version);
    assert_eq!(info.key_id, "builtin-v1");
    assert!(!info.signed);
    assert!(info.bytes > 0);
}

#[test]
fn truncated_pack_is_rejected() {
    let dir = theme_dir();
    let out = tmp("trunc").with_extension("gfdeck");
    let (key, key_id) = builtin_key();
    pack_dir(
        &dir,
        &out,
        &PackOptions {
            key: &key,
            key_id: &key_id,
            signing_key: None,
        },
    )
    .unwrap();
    let bytes = std::fs::read(&out).unwrap();
    std::fs::write(&out, &bytes[..bytes.len() / 2]).unwrap();
    let err = read_info(&out).unwrap_err();
    assert_eq!(err.code(), "DK-PACK-CORRUPT");
}

#[test]
fn not_a_pack_is_rejected() {
    let out = tmp("magic").with_extension("gfdeck");
    std::fs::write(&out, b"not a gfdeck at all").unwrap();
    assert_eq!(read_info(&out).unwrap_err().code(), "DK-PACK-CORRUPT");
}

#[test]
fn corrupted_theme_dir_is_rejected() {
    // 缺素材的目录必须在加载期就报错，而不是渲染时静默出半张牌
    let dir = tmp("bad-theme");
    std::fs::create_dir_all(dir.join("suits")).unwrap();
    let src = theme_dir();
    std::fs::copy(src.join("theme.json"), dir.join("theme.json")).unwrap();
    let err = Theme::load_dir(&dir).unwrap_err();
    assert_eq!(err.code(), "DK-ASSET-MISSING");
}

#[test]
fn path_traversal_is_rejected() {
    let dir = tmp("traversal");
    std::fs::create_dir_all(dir.join("suits")).unwrap();
    let mut meta = std::fs::read_to_string(theme_dir().join("theme.json")).unwrap();
    meta = meta.replace("\"base\": \"base.svg\"", "\"base\": \"../base.svg\"");
    std::fs::write(dir.join("theme.json"), meta).unwrap();
    let err = Theme::load_dir(&dir).unwrap_err();
    assert_eq!(err.code(), "DK-THEME-INVALID");
}

#[test]
fn pack_theme_accepts_loaded_theme() {
    // 包 → 内存主题 → 再打包，指纹与条目数必须一致（用于「换密钥重发」场景）
    let dir = theme_dir();
    let first = tmp("repack-1").with_extension("gfdeck");
    let second = tmp("repack-2").with_extension("gfdeck");
    let (k1, id1) = builtin_key();
    pack_dir(
        &dir,
        &first,
        &PackOptions {
            key: &k1,
            key_id: &id1,
            signing_key: None,
        },
    )
    .unwrap();
    let theme = open(&first, &k1, None).unwrap();
    let mut k2 = [3u8; 32];
    k2[0] = 1;
    let info = pack_theme(
        &theme,
        &second,
        &PackOptions {
            key: &k2,
            key_id: "rotated",
            signing_key: None,
        },
    )
    .unwrap();
    assert_eq!(info.theme_id, "defaults");
    assert_eq!(info.key_id, "rotated");
    let theme2 = open(&second, &k2, None).unwrap();
    assert_eq!(theme2.fingerprint(), theme.fingerprint());
}
