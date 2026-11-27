//! 房间快照 wire 编解码与服务端 `viewHash` 自证（§5.9 第 753 行 `RoomSnapshot`、
//! §5.12 第 895/924 行视图编码与字节序）。
//!
//! 字段序固定（不得改序、不得改名）：
//! `matchId, seat, roundNo, rulesetId, rulesetVersion, roomSeq, lastSeatSeq,
//! viewHash, viewSchemaVersion, view, snapshotReset`。
//!
//! - `view` 为 canonical JSON bytes（§5.12 六条）；本模块只搬运字节，不解析视图键集。
//! - `roundNo` 是快照头字段，**不进 `view`**，也不触发 `viewSchemaVersion` 升版。
//! - `viewHash` **只由服务端在下发前计算并自证**；客户端 `decode` 只透传哈希字节，
//!   不自行计算、不上报、不改写（§5.12 第 874 行、§7.2 P0）。
//! - 字节序固定 little-endian；整数定宽；字符串 UTF-8 不做 NFC 归一化。

use std::fmt;

use gf_engine::view::view_hash;

/// 快照 wire 编解码错误。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CodecError {
    /// 字节流提前结束，字段长度不足以解码。
    Truncated,
    /// 字符串 / 视图字节长度超出定宽长度前缀可表达范围。
    LengthOverflow,
}

impl fmt::Display for CodecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CodecError::Truncated => write!(f, "快照字节流不完整"),
            CodecError::LengthOverflow => write!(f, "字段长度超出编码上限"),
        }
    }
}

impl std::error::Error for CodecError {}

/// §5.9 `RoomSnapshot` 的 wire 形状（与协议帧字段一一对应）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoomSnapshotWire {
    /// 场次标识 `matchId`。
    pub match_id: String,
    /// 本快照所属座位 `seat`。
    pub seat: u8,
    /// 快照头 `roundNo`（当前局号；不进 `view`、不进 `stateHash`）。
    pub round_no: u16,
    /// 房间创建时锁定的规则集标识 `rulesetId`。
    pub ruleset_id: String,
    /// 房间创建时锁定的规则集版本 `rulesetVersion`（身份键，不按字典序比大小）。
    pub ruleset_version: String,
    /// 本帧房间广播序 `roomSeq`（与 `snapSeq` 同一序列）。
    pub room_seq: u64,
    /// 该座位本地已应用到的最后 `seatSeq` 水位 `lastSeatSeq`。
    pub last_seat_seq: u64,
    /// `viewHash`：`BLAKE3(seat ‖ roomSeq ‖ viewBytes)` 前 8 字节；服务端自证。
    pub view_hash: [u8; 8],
    /// `viewSchemaVersion`：本帧 `view` 用的白名单版本（取自规则集，房间锁定）。
    pub view_schema_version: u16,
    /// `view`：canonical JSON bytes。
    pub view: Vec<u8>,
    /// `snapshotReset`：`true` 表示客户端必须丢弃本地状态并接受本快照。
    pub snapshot_reset: bool,
}

impl RoomSnapshotWire {
    /// 服务端构造：计算并写入 `viewHash`（唯一允许计算哈希的入口）。
    ///
    /// 客户端路径不调用本构造函数，也不调用 [`Self::verify`]；哈希只透传。
    #[allow(clippy::too_many_arguments)] // 参数逐条对齐 §5.9 `RoomSnapshot` 帧字段，不合并
    pub fn server_build(
        match_id: String,
        seat: u8,
        round_no: u16,
        ruleset_id: String,
        ruleset_version: String,
        room_seq: u64,
        last_seat_seq: u64,
        view_schema_version: u16,
        view: Vec<u8>,
        snapshot_reset: bool,
    ) -> Self {
        let view_hash = view_hash(seat, room_seq, &view);
        Self {
            match_id,
            seat,
            round_no,
            ruleset_id,
            ruleset_version,
            room_seq,
            last_seat_seq,
            view_hash,
            view_schema_version,
            view,
            snapshot_reset,
        }
    }

    /// 服务端下发前自证：按 `seat ‖ roomSeq ‖ viewBytes` 重算 `viewHash`，与帧内值比对。
    /// 仅服务端调用；客户端不重算视图哈希（§5.12 第 874 行）。
    pub fn verify(&self) -> bool {
        view_hash(self.seat, self.room_seq, &self.view) == self.view_hash
    }

    /// 编码为 wire 字节（字段序固定）。长度前缀装不下时返回错误，绝不截断字段。
    pub fn encode(&self) -> Result<Vec<u8>, CodecError> {
        let mut out = Vec::new();
        push_string(&mut out, &self.match_id)?;
        out.push(self.seat);
        out.extend_from_slice(&self.round_no.to_le_bytes());
        push_string(&mut out, &self.ruleset_id)?;
        push_string(&mut out, &self.ruleset_version)?;
        out.extend_from_slice(&self.room_seq.to_le_bytes());
        out.extend_from_slice(&self.last_seat_seq.to_le_bytes());
        out.extend_from_slice(&self.view_hash);
        out.extend_from_slice(&self.view_schema_version.to_le_bytes());
        push_bytes(&mut out, &self.view)?;
        out.push(u8::from(self.snapshot_reset));
        Ok(out)
    }

    /// 解码 wire 字节；只透传 `viewHash`，**不重算**。
    pub fn decode(bytes: &[u8]) -> Result<Self, CodecError> {
        let mut pos = 0usize;
        let match_id = pop_string(bytes, &mut pos)?;
        let seat = u8_at(bytes, &mut pos)?;
        let round_no = u16_at(bytes, &mut pos)?;
        let ruleset_id = pop_string(bytes, &mut pos)?;
        let ruleset_version = pop_string(bytes, &mut pos)?;
        let room_seq = u64_at(bytes, &mut pos)?;
        let last_seat_seq = u64_at(bytes, &mut pos)?;
        let mut view_hash = [0u8; 8];
        view_hash.copy_from_slice(take(bytes, &mut pos, 8)?);
        let view_schema_version = u16_at(bytes, &mut pos)?;
        let view = pop_bytes(bytes, &mut pos)?;
        let snapshot_reset = u8_at(bytes, &mut pos)? != 0;
        if pos != bytes.len() {
            return Err(CodecError::Truncated);
        }
        Ok(Self {
            match_id,
            seat,
            round_no,
            ruleset_id,
            ruleset_version,
            room_seq,
            last_seat_seq,
            view_hash,
            view_schema_version,
            view,
            snapshot_reset,
        })
    }
}

fn push_string(out: &mut Vec<u8>, value: &str) -> Result<(), CodecError> {
    let bytes = value.as_bytes();
    // 字符串长度上限 u16（身份键 / matchId 均为短串）；装不下就报错，不能只截前缀。
    let len = u16::try_from(bytes.len()).map_err(|_| CodecError::LengthOverflow)?;
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(bytes);
    Ok(())
}

fn push_bytes(out: &mut Vec<u8>, value: &[u8]) -> Result<(), CodecError> {
    let len = u32::try_from(value.len()).map_err(|_| CodecError::LengthOverflow)?;
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(value);
    Ok(())
}

fn take<'a>(bytes: &'a [u8], pos: &mut usize, n: usize) -> Result<&'a [u8], CodecError> {
    let end = pos.checked_add(n).ok_or(CodecError::LengthOverflow)?;
    if end > bytes.len() {
        return Err(CodecError::Truncated);
    }
    let slice = &bytes[*pos..end];
    *pos = end;
    Ok(slice)
}

fn u8_at(bytes: &[u8], pos: &mut usize) -> Result<u8, CodecError> {
    Ok(take(bytes, pos, 1)?[0])
}

fn u16_at(bytes: &[u8], pos: &mut usize) -> Result<u16, CodecError> {
    let slice = take(bytes, pos, 2)?;
    Ok(u16::from_le_bytes([slice[0], slice[1]]))
}

fn u64_at(bytes: &[u8], pos: &mut usize) -> Result<u64, CodecError> {
    let slice = take(bytes, pos, 8)?;
    let mut array = [0u8; 8];
    array.copy_from_slice(slice);
    Ok(u64::from_le_bytes(array))
}

fn pop_string(bytes: &[u8], pos: &mut usize) -> Result<String, CodecError> {
    let len = usize::from(u16_at(bytes, pos)?);
    let slice = take(bytes, pos, len)?;
    String::from_utf8(slice.to_vec()).map_err(|_| CodecError::Truncated)
}

fn pop_bytes(bytes: &[u8], pos: &mut usize) -> Result<Vec<u8>, CodecError> {
    let len = usize::try_from(u32::from_le_bytes({
        let slice = take(bytes, pos, 4)?;
        let mut array = [0u8; 4];
        array.copy_from_slice(slice);
        array
    }))
    .map_err(|_| CodecError::LengthOverflow)?;
    Ok(take(bytes, pos, len)?.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;
    use gf_engine::cases::classic_engine;
    use gf_engine::contract::{Engine, Seat};

    fn sample(snapshot_reset: bool, round_no: u16) -> RoomSnapshotWire {
        let engine = classic_engine().expect("classic 构造失败");
        let state = engine.deal(gf_engine::cases::M0_SEED).expect("发牌失败");
        let view = engine.view(&state, Seat(0));
        RoomSnapshotWire::server_build(
            "match-0001".to_owned(),
            0,
            round_no,
            "guandan-classic".to_owned(),
            "guandan-classic-1.0.0".to_owned(),
            42,
            7,
            engine.ruleset.view.schema_version,
            view.bytes,
            snapshot_reset,
        )
    }

    #[test]
    fn snapshot_reset_true_roundtrips_all_head_fields() {
        let frame = sample(true, 3);
        let decoded =
            RoomSnapshotWire::decode(&frame.encode().expect("编码成功")).expect("解码成功");
        assert_eq!(decoded, frame);
        assert!(decoded.snapshot_reset);
        // roundNo 在头字段，不在 view 字节里；viewSchemaVersion 仍为 2（classic 白名单）。
        assert_eq!(decoded.round_no, 3);
        assert_eq!(decoded.view_schema_version, 2);
        assert!(!decoded.view.windows(2).any(|w| w == b"\"roundNo\""));
        assert!(frame.verify(), "服务端重算 viewHash 应与帧内一致");
    }

    #[test]
    fn snapshot_reset_false_roundtrips_and_hash_self_consistent() {
        let frame = sample(false, 1);
        let decoded =
            RoomSnapshotWire::decode(&frame.encode().expect("编码成功")).expect("解码成功");
        assert_eq!(decoded, frame);
        assert!(!decoded.snapshot_reset);
        assert_eq!(decoded.round_no, 1);
        assert!(decoded.verify());
    }

    #[test]
    fn decode_does_not_recompute_view_hash() {
        let frame = sample(false, 2);
        let mut decoded =
            RoomSnapshotWire::decode(&frame.encode().expect("编码成功")).expect("解码成功");
        // 客户端透传哈希：改写帧内哈希后，decode 仍原样带出（不做比对、不重算）。
        decoded.view_hash[0] ^= 0xFF;
        assert!(decoded.view_hash[0] != frame.view_hash[0]);
        assert!(!decoded.verify(), "被篡改的哈希服务端自证应失败");
    }

    #[test]
    fn truncated_payload_rejected() {
        let frame = sample(true, 1);
        let bytes = frame.encode().expect("编码成功");
        let short = &bytes[..bytes.len() - 1];
        assert!(matches!(
            RoomSnapshotWire::decode(short),
            Err(CodecError::Truncated)
        ));
    }

    #[test]
    fn oversized_string_is_error_not_truncated_prefix() {
        let mut frame = sample(false, 1);
        frame.match_id = "m".repeat(usize::from(u16::MAX) + 1);
        assert!(matches!(frame.encode(), Err(CodecError::LengthOverflow)));
    }
}
