//! 传输层帧封装：protobuf over WebSocket 的自研帧头（§4.1 D5「固定自研」）。
//!
//! 设计稿只定「protobuf over WS」，未定帧头格式；本模块是传输层的实现约定，
//! **不是** §5.9 的消息契约——消息体仍是 `gf.protocol.v1` 里的原样 protobuf。
//!
//! 帧布局：`[kind: u16 大端][protobuf body]`。一个 WS 二进制帧承载恰好一条消息；
//! 不设长度前缀（WS 帧自带边界）。
//!
//! 纪律：`kind` 只增不减；同一方向同一 `kind` 的语义不得复用。

/// 帧头长度（`kind` 占 2 字节）。
pub const HEADER_LEN: usize = 2;

/// 消息类型编号（C→S 1–9，S→C 11–19，留出空档便于后续插入）。
pub mod kind {
    /// `JoinRoom`
    pub const JOIN_ROOM: u16 = 1;
    /// `Reconnect`
    pub const RECONNECT: u16 = 2;
    /// `SubmitAction`
    pub const SUBMIT_ACTION: u16 = 3;
    /// `Heartbeat`
    pub const HEARTBEAT: u16 = 4;
    /// `RematchReady`
    pub const REMATCH_READY: u16 = 5;

    /// `JoinAck`
    pub const JOIN_ACK: u16 = 11;
    /// `RoomSnapshot`
    pub const ROOM_SNAPSHOT: u16 = 12;
    /// `RoomDelta`
    pub const ROOM_DELTA: u16 = 13;
    /// `ActionAck`
    pub const ACTION_ACK: u16 = 14;
    /// `PhaseChange`
    pub const PHASE_CHANGE: u16 = 15;
    /// `SettleResult`
    pub const SETTLE_RESULT: u16 = 16;
    /// `TrusteeState`
    pub const TRUSTEE_STATE: u16 = 17;
    /// `RematchState`
    pub const REMATCH_STATE: u16 = 18;
}

/// 帧编解码错误。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WireError {
    /// 不足 2 字节，取不出消息类型。
    Truncated,
    /// 消息类型未知（不在 [`kind`] 清单内）。
    UnknownKind(u16),
}

impl std::fmt::Display for WireError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WireError::Truncated => write!(f, "帧长度不足 {HEADER_LEN} 字节"),
            WireError::UnknownKind(k) => write!(f, "未知消息类型 {k}"),
        }
    }
}

impl std::error::Error for WireError {}

/// 组帧：`kind` 大端 2 字节 + 消息体。
pub fn encode(kind: u16, body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(HEADER_LEN + body.len());
    out.extend_from_slice(&kind.to_be_bytes());
    out.extend_from_slice(body);
    out
}

/// 拆帧：返回 `(kind, body)`。`kind` 不做白名单校验（由调用方按方向各自校验）。
pub fn split(frame: &[u8]) -> Result<(u16, &[u8]), WireError> {
    if frame.len() < HEADER_LEN {
        return Err(WireError::Truncated);
    }
    let kind = u16::from_be_bytes([frame[0], frame[1]]);
    Ok((kind, &frame[HEADER_LEN..]))
}

/// C→S 方向的消息类型是否合法。
pub fn is_client_kind(kind: u16) -> bool {
    matches!(
        kind,
        kind::JOIN_ROOM
            | kind::RECONNECT
            | kind::SUBMIT_ACTION
            | kind::HEARTBEAT
            | kind::REMATCH_READY
    )
}

/// S→C 方向的消息类型是否合法。
pub fn is_server_kind(kind: u16) -> bool {
    matches!(
        kind,
        kind::JOIN_ACK
            | kind::ROOM_SNAPSHOT
            | kind::ROOM_DELTA
            | kind::ACTION_ACK
            | kind::PHASE_CHANGE
            | kind::SETTLE_RESULT
            | kind::TRUSTEE_STATE
            | kind::REMATCH_STATE
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_header_and_body() {
        let body = [0x08u8, 0x01, 0x12, 0x03];
        let frame = encode(kind::JOIN_ROOM, &body);
        assert_eq!(frame.len(), HEADER_LEN + body.len());
        let (k, b) = split(&frame).expect("拆帧失败");
        assert_eq!(k, kind::JOIN_ROOM);
        assert_eq!(b, &body[..]);
    }

    #[test]
    fn empty_body_is_legal() {
        let frame = encode(kind::HEARTBEAT, &[]);
        let (k, b) = split(&frame).expect("空体帧应可拆");
        assert_eq!(k, kind::HEARTBEAT);
        assert!(b.is_empty());
    }

    #[test]
    fn truncated_frame_is_rejected() {
        assert_eq!(split(&[]), Err(WireError::Truncated));
        assert_eq!(split(&[0x00]), Err(WireError::Truncated));
    }

    #[test]
    fn kind_directions_do_not_overlap() {
        for k in [
            kind::JOIN_ROOM,
            kind::RECONNECT,
            kind::SUBMIT_ACTION,
            kind::HEARTBEAT,
            kind::REMATCH_READY,
        ] {
            assert!(is_client_kind(k), "C→S 类型 {k} 应合法");
            assert!(!is_server_kind(k), "C→S 类型 {k} 不应被判为服务端帧");
        }
        for k in [
            kind::JOIN_ACK,
            kind::ROOM_SNAPSHOT,
            kind::ROOM_DELTA,
            kind::ACTION_ACK,
            kind::PHASE_CHANGE,
            kind::SETTLE_RESULT,
            kind::TRUSTEE_STATE,
            kind::REMATCH_STATE,
        ] {
            assert!(is_server_kind(k), "S→C 类型 {k} 应合法");
            assert!(!is_client_kind(k), "S→C 类型 {k} 不应被判为客户端帧");
        }
    }

    #[test]
    fn unknown_kind_fails_closed() {
        assert_eq!(split(&[0x00, 0xFF]), Ok((255, &[][..])));
        assert!(!is_client_kind(255));
        assert!(!is_server_kind(255));
    }
}
