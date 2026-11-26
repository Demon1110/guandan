//! `.gfdeck` 素材包：`GFDK` 容器 + 逐条 AEAD 加密 + 可选 Ed25519 签名。
//!
//! 布局：
//! ```text
//! +0   magic "GFDK"
//! +4   u16 格式版本
//! +6   u16 flags（bit0 压缩预留 / bit1 加密 / bit2 签名）
//! +8   u32 header_len
//! +12  header JSON（明文：themeId / themeVersion / license / keyId / entries）
//! ..   payload（每条 entry = nonce12 || ciphertext+tag16）
//! ..   signature（64B，仅 flags 含签名时）
//! ```

use std::collections::HashMap;
#[cfg(feature = "pack-write")]
use std::io::Write;
use std::path::Path;

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
#[cfg(feature = "pack-write")]
use ed25519_dalek::Signer;
use ed25519_dalek::{Signature, SigningKey, VerifyingKey};
#[cfg(feature = "pack-write")]
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::DeckError;
use crate::theme::{Theme, ThemeMeta};

pub const MAGIC: [u8; 4] = *b"GFDK";
pub const FORMAT_VERSION: u16 = 1;
pub const FLAG_COMPRESSED: u16 = 0b0001;
pub const FLAG_ENCRYPTED: u16 = 0b0010;
pub const FLAG_SIGNED: u16 = 0b0100;

const NONCE_LEN: usize = 12;
const TAG_LEN: usize = 16;
const SIG_LEN: usize = 64;

/// 内置默认密钥（方案 A）。拆成两段常量并在运行时拼接，避免以完整字面量出现在二进制里。
fn builtin_key_part1() -> [u8; 16] {
    [
        0x9f, 0x2c, 0x51, 0x8b, 0x37, 0xd4, 0x0e, 0x66, 0xa1, 0x73, 0xbd, 0x28, 0x5c, 0xe9, 0x04,
        0x8f,
    ]
}
fn builtin_key_part2() -> [u8; 16] {
    [
        0x13, 0xa7, 0x6d, 0xf0, 0x2b, 0x9e, 0x44, 0xc8, 0x71, 0x05, 0xda, 0x3f, 0x88, 0xb6, 0x2d,
        0x5e,
    ]
}

/// 内置默认密钥与 keyId。`themeKey` 由服务端下发时用 `key_id` 区分（方案 B 预留）。
pub fn builtin_key() -> ([u8; 32], String) {
    let mut key = [0u8; 32];
    key[..16].copy_from_slice(&builtin_key_part1());
    key[16..].copy_from_slice(&builtin_key_part2());
    (key, "builtin-v1".to_string())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub name: String,
    /// 相对 payload 起点
    pub offset: u32,
    /// 含 nonce 与 tag
    pub len: u32,
    /// 明文 sha256（hex）
    pub sha256: String,
    /// nonce（hex）
    pub nonce: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackHeader {
    pub theme_id: String,
    pub theme_version: u32,
    pub license: String,
    pub key_id: String,
    pub entries: Vec<Entry>,
}

#[derive(Debug, Clone, Copy)]
pub struct PackOptions<'a> {
    pub key: &'a [u8; 32],
    pub key_id: &'a str,
    pub signing_key: Option<&'a SigningKey>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackInfo {
    pub theme_id: String,
    pub theme_version: u32,
    pub license: String,
    pub key_id: String,
    pub entries: usize,
    pub encrypted: bool,
    pub signed: bool,
    pub bytes: u64,
}

/// 打包整个主题目录（只有 `pack-write` feature 下可用；客户端 wasm 不需要）。
#[cfg(feature = "pack-write")]
pub fn pack_dir(dir: &Path, out: &Path, opts: &PackOptions<'_>) -> Result<PackInfo, DeckError> {
    let theme = Theme::load_dir(dir)?;
    pack_theme(&theme, out, opts)
}

/// 打包一个已加载的主题（目录或包内解出的均可）。
#[cfg(feature = "pack-write")]
pub fn pack_theme(
    theme: &Theme,
    out: &Path,
    opts: &PackOptions<'_>,
) -> Result<PackInfo, DeckError> {
    let meta = theme.meta();
    let cipher = Aes256Gcm::new_from_slice(opts.key)
        .map_err(|_| DeckError::PackCorrupt("密钥长度非法".into()))?;

    // theme.json 与素材一起入包：解包后能直接还原成可加载的主题目录
    let mut plain: Vec<(String, Vec<u8>)> = Vec::new();
    plain.push(("theme.json".to_string(), serde_json::to_vec_pretty(meta)?));
    for rel in meta.asset_paths() {
        plain.push((rel.clone(), theme.asset(&rel).as_bytes().to_vec()));
    }

    let mut payload: Vec<u8> = Vec::new();
    let mut entries: Vec<Entry> = Vec::with_capacity(plain.len());
    for (name, bytes) in &plain {
        let mut nonce = [0u8; NONCE_LEN];
        rand::thread_rng().fill_bytes(&mut nonce);
        let aad = aad(meta.id.as_str(), meta.version, name);
        let mut sealed = cipher
            .encrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: bytes,
                    aad: aad.as_bytes(),
                },
            )
            .map_err(|_| DeckError::PackDecrypt)?;
        let mut blob = Vec::with_capacity(NONCE_LEN + sealed.len());
        blob.extend_from_slice(&nonce);
        blob.append(&mut sealed);
        entries.push(Entry {
            name: name.clone(),
            offset: payload.len() as u32,
            len: blob.len() as u32,
            sha256: hex(&Sha256::digest(bytes)),
            nonce: hex(&nonce),
        });
        payload.extend_from_slice(&blob);
    }

    let header = PackHeader {
        theme_id: meta.id.clone(),
        theme_version: meta.version,
        license: meta.license.clone(),
        key_id: opts.key_id.to_string(),
        entries,
    };
    let header_json = serde_json::to_vec(&header)?;

    let mut flags = FLAG_ENCRYPTED;
    let mut signature: Option<[u8; SIG_LEN]> = None;
    if let Some(sk) = opts.signing_key {
        flags |= FLAG_SIGNED;
        let mut signed = Vec::new();
        signed.extend_from_slice(&header_json);
        signed.extend_from_slice(&payload);
        signature = Some(sk.sign(&signed).to_bytes());
    }

    let mut buf = Vec::with_capacity(12 + header_json.len() + payload.len() + SIG_LEN);
    buf.extend_from_slice(&MAGIC);
    buf.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
    buf.extend_from_slice(&flags.to_le_bytes());
    buf.extend_from_slice(&(header_json.len() as u32).to_le_bytes());
    buf.extend_from_slice(&header_json);
    buf.extend_from_slice(&payload);
    if let Some(sig) = signature {
        buf.extend_from_slice(&sig);
    }

    if let Some(parent) = out.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    let mut f = std::fs::File::create(out)?;
    f.write_all(&buf)?;

    Ok(PackInfo {
        theme_id: header.theme_id,
        theme_version: header.theme_version,
        license: header.license,
        key_id: header.key_id,
        entries: header.entries.len(),
        encrypted: true,
        signed: flags & FLAG_SIGNED != 0,
        bytes: buf.len() as u64,
    })
}

/// 从 32 字节公钥构造验签器（wasm 宿主只有字节，没有 PEM）。
pub fn verifying_key(bytes: &[u8]) -> Result<VerifyingKey, DeckError> {
    let arr: [u8; 32] = bytes.try_into().map_err(|_| DeckError::PackSignature)?;
    VerifyingKey::from_bytes(&arr).map_err(|_| DeckError::PackSignature)
}

/// 只读 header，不需要密钥。
pub fn read_info(pack: &Path) -> Result<PackInfo, DeckError> {
    info_from_bytes(&std::fs::read(pack)?)
}

/// 只读 header，不需要密钥（字节来源，供 wasm 用）。
pub fn info_from_bytes(bytes: &[u8]) -> Result<PackInfo, DeckError> {
    let c = parse_container(bytes)?;
    Ok(PackInfo {
        theme_id: c.header.theme_id,
        theme_version: c.header.theme_version,
        license: c.header.license,
        key_id: c.header.key_id,
        entries: c.header.entries.len(),
        encrypted: c.flags & FLAG_ENCRYPTED != 0,
        signed: c.flags & FLAG_SIGNED != 0,
        bytes: c.total,
    })
}

/// 解密并组装主题（内存态，不落盘）。
pub fn open(
    pack: &Path,
    key: &[u8; 32],
    verify_key: Option<&VerifyingKey>,
) -> Result<Theme, DeckError> {
    open_bytes(&std::fs::read(pack)?, key, verify_key)
}

/// 解密并组装主题（字节来源，不落盘）。wasm 侧从这里进。
pub fn open_bytes(
    bytes: &[u8],
    key: &[u8; 32],
    verify_key: Option<&VerifyingKey>,
) -> Result<Theme, DeckError> {
    let c = parse_container(bytes)?;
    if c.flags & FLAG_SIGNED != 0 {
        let Some(vk) = verify_key else {
            return Err(DeckError::PackSignature);
        };
        let Some(sig_bytes) = c.sig else {
            return Err(DeckError::PackCorrupt("缺签名".into()));
        };
        let mut signed = Vec::with_capacity(c.header_json.len() + c.payload.len() + 1);
        signed.extend_from_slice(&c.header_json);
        signed.extend_from_slice(&c.payload);
        vk.verify_strict(&signed, &Signature::from_bytes(&sig_bytes))
            .map_err(|_| DeckError::PackSignature)?;
    }
    let plain = decrypt_entries(&c.header, c.payload, c.flags, key)?;
    let assets: HashMap<String, String> = plain
        .iter()
        .filter(|(n, _)| n != "theme.json")
        .map(|(n, b)| (n.clone(), String::from_utf8(b.clone()).unwrap_or_default()))
        .collect();
    let theme_json = plain
        .iter()
        .find(|(n, _)| n == "theme.json")
        .map(|(_, b)| String::from_utf8_lossy(b).to_string())
        .ok_or_else(|| DeckError::PackCorrupt("缺少 theme.json".into()))?;
    let meta = ThemeMeta::parse(&theme_json)?;
    let origin = format!(
        "gfdeck:{}@{}（{}）",
        c.header.theme_id, c.header.theme_version, c.header.key_id
    );
    Theme::from_parts(meta, assets, origin)
}

/// 解包到目录：还原成可直接 `Theme::load_dir` 的主题目录。
pub fn unpack(
    pack: &Path,
    out_dir: &Path,
    key: &[u8; 32],
    verify_key: Option<&VerifyingKey>,
) -> Result<PackInfo, DeckError> {
    if out_dir.exists() {
        return Err(DeckError::PackExists(out_dir.display().to_string()));
    }
    let info = read_info(pack)?;
    let theme = open(pack, key, verify_key)?;
    std::fs::create_dir_all(out_dir)?;
    std::fs::write(
        out_dir.join("theme.json"),
        serde_json::to_vec_pretty(theme.meta())?,
    )?;
    for rel in theme.meta().asset_paths() {
        let path = out_dir.join(&rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, theme.asset(&rel))?;
    }
    Ok(info)
}

/// 容器解析结果。
struct Container {
    header: PackHeader,
    header_json: Vec<u8>,
    payload: Vec<u8>,
    flags: u16,
    sig: Option<[u8; SIG_LEN]>,
    total: u64,
}

/// 纯内存解析 `.gfdeck` 容器（不读文件，wasm 可以直接喂字节）。
fn parse_container(bytes: &[u8]) -> Result<Container, DeckError> {
    if bytes.len() < 12 {
        return Err(DeckError::PackCorrupt("文件过短".into()));
    }
    if bytes[0..4] != MAGIC {
        return Err(DeckError::PackCorrupt("magic 不符（不是 .gfdeck）".into()));
    }
    let version = u16::from_le_bytes([bytes[4], bytes[5]]);
    if version != FORMAT_VERSION {
        return Err(DeckError::PackCorrupt(format!("格式版本不支持: {version}")));
    }
    let flags = u16::from_le_bytes([bytes[6], bytes[7]]);
    let header_len = u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]) as usize;
    let header_end = 12usize
        .checked_add(header_len)
        .ok_or_else(|| DeckError::PackCorrupt("header_len 溢出".into()))?;
    if header_end > bytes.len() {
        return Err(DeckError::PackCorrupt("header 越界".into()));
    }
    let header_json = bytes[12..header_end].to_vec();
    let header: PackHeader = serde_json::from_slice(&header_json)
        .map_err(|e| DeckError::PackCorrupt(format!("header 解析失败: {e}")))?;
    let payload_end = if flags & FLAG_SIGNED != 0 {
        bytes
            .len()
            .checked_sub(SIG_LEN)
            .ok_or_else(|| DeckError::PackCorrupt("签名区越界".into()))?
    } else {
        bytes.len()
    };
    if payload_end < header_end {
        return Err(DeckError::PackCorrupt("payload 区为空".into()));
    }
    let payload = bytes[header_end..payload_end].to_vec();
    let sig = if flags & FLAG_SIGNED != 0 {
        let mut s = [0u8; SIG_LEN];
        s.copy_from_slice(&bytes[payload_end..]);
        Some(s)
    } else {
        None
    };
    // 条目边界必须落在 payload 内
    for e in &header.entries {
        let end = (e.offset as usize)
            .checked_add(e.len as usize)
            .ok_or_else(|| DeckError::PackCorrupt(format!("条目 {} 越界", e.name)))?;
        if end > payload.len() {
            return Err(DeckError::PackCorrupt(format!(
                "条目 {} 越界（{}+{} > {}）",
                e.name,
                e.offset,
                e.len,
                payload.len()
            )));
        }
    }
    Ok(Container {
        header,
        header_json,
        payload,
        flags,
        sig,
        total: bytes.len() as u64,
    })
}

fn decrypt_entries(
    header: &PackHeader,
    payload: Vec<u8>,
    flags: u16,
    key: &[u8; 32],
) -> Result<Vec<(String, Vec<u8>)>, DeckError> {
    if flags & FLAG_ENCRYPTED == 0 {
        return Err(DeckError::PackCorrupt("未加密的包尚不支持读取".into()));
    }
    let cipher = Aes256Gcm::new_from_slice(key)
        .map_err(|_| DeckError::PackCorrupt("密钥长度非法".into()))?;
    let mut out = Vec::with_capacity(header.entries.len());
    for e in &header.entries {
        let start = e.offset as usize;
        let end = start + e.len as usize;
        let blob = &payload[start..end];
        if blob.len() < NONCE_LEN + TAG_LEN {
            return Err(DeckError::PackCorrupt(format!("条目 {} 过短", e.name)));
        }
        let nonce = &blob[..NONCE_LEN];
        let aad = aad(&header.theme_id, header.theme_version, &e.name);
        let plain = cipher
            .decrypt(
                Nonce::from_slice(nonce),
                Payload {
                    msg: &blob[NONCE_LEN..],
                    aad: aad.as_bytes(),
                },
            )
            .map_err(|_| DeckError::PackDecrypt)?;
        if hex(&Sha256::digest(&plain)) != e.sha256 {
            return Err(DeckError::PackCorrupt(format!(
                "条目 {} 摘要不符（内容被改写）",
                e.name
            )));
        }
        out.push((e.name.clone(), plain));
    }
    Ok(out)
}

/// AEAD 附加数据：绑定主题身份与条目名，防止条目被互换或跨主题搬运。
fn aad(theme_id: &str, version: u32, name: &str) -> String {
    format!("gfdeck:v1:{theme_id}@{version}:{name}")
}

fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        use std::fmt::Write as _;
        write!(&mut s, "{b:02x}").ok();
    }
    s
}
