//! 大厅：房间码与匹配队列（M2 第二切片，`design_draft.md` §5.15）。
//!
//! 本薄片只做「内存态」的组局入口，落库/Redis 留在 PG/Redis 切片：
//! - 房间码：`POST /api/room/create` 出码、`POST /api/room/join` 凭码换座位；
//! - 匹配队列：`POST /api/match/queue` 入队、`GET /api/match/queue/{id}` 轮询、`DELETE` 退队；
//!   满四人即成房（v1 不推 WS 匹配事件，轮询 `matched` 后客户端凭 `matchId` 接 WS）。
//!
//! 设计口径：
//! - `mode → matchType` 严格按 §5.15 真值表，不同 `mode` 的队列**不得混排**；
//! - 房间码只在同一房间内复用一份，过期即失效（时长由 `GF_ROOM_CODE_TTL_MS` 控制）；
//! - 队列 FIFO（先到先配），超时状态由 `GF_MATCH_TIMEOUT_MS` 控制。
//!
//! 随机数取自 `RandomState`（进程启动时由 OS 播种），只用于房间码/票号/场次号，
//! 不用于任何鉴权材料——账号切片的令牌必须换用密码学随机源。

use std::collections::HashMap;
use std::hash::{BuildHasher, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};

/// 房间码长度（6 位可口头转述，容量 1.2 亿，远大于单实例房间量）。
pub const CODE_LEN: usize = 6;
/// 房间码字母表：去掉易混的 `I`/`L`/`O`/`0`/`1`。
const CODE_ALPHABET: &[u8] = b"ABCDEFGHJKMNPQRSTUVWXYZ23456789";
/// 房间码缺省有效期（24 小时；房间结束前一直可加入）。
pub const DEFAULT_CODE_TTL_MS: u64 = 24 * 60 * 60 * 1000;
/// 匹配排队缺省超时（120 秒）。
pub const DEFAULT_QUEUE_TIMEOUT_MS: u64 = 120_000;
/// 匹配票/已配票在内存里的保留时长（仅防内存无界增长，不影响语义）。
const TICKET_RETAIN_MS: u64 = 30 * 60 * 1000;

/// 组局模式（§5.15 `mode`）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Mode {
    Ranked,
    Casual,
    Mixed,
}

impl Mode {
    /// 解析 `ranked|casual|mixed`；其它取值返回 `None`。
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "ranked" => Some(Mode::Ranked),
            "casual" => Some(Mode::Casual),
            "mixed" => Some(Mode::Mixed),
            _ => None,
        }
    }

    /// 线上字面值（回显给客户端，别的地方不要另造）。
    pub fn as_str(self) -> &'static str {
        match self {
            Mode::Ranked => "ranked",
            Mode::Casual => "casual",
            Mode::Mixed => "mixed",
        }
    }

    /// `mode → matchType`（§5.15 真值表；`matchType` 定义只在 §5.8 `match_record`）。
    pub fn match_type(self) -> &'static str {
        match self {
            Mode::Ranked => "human-ranked",
            Mode::Casual => "human-casual",
            Mode::Mixed => "mixed",
        }
    }
}

/// 匹配票状态（`GET /api/match/queue/{ticketId}` 的 `state`）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TicketState {
    Waiting,
    Matched { match_id: String },
    Timeout,
}

impl TicketState {
    /// 线上字面值。
    pub fn as_str(&self) -> &'static str {
        match self {
            TicketState::Waiting => "waiting",
            TicketState::Matched { .. } => "matched",
            TicketState::Timeout => "timeout",
        }
    }
}

struct CodeSlot {
    match_id: String,
    expires_ms: u64,
}

struct Ticket {
    game_id: String,
    mode: Mode,
    state: TicketState,
    enqueued_ms: u64,
    changed_ms: u64,
}

/// 房码与匹配队列的内存态。
#[derive(Default)]
pub struct Lobby {
    codes: HashMap<String, CodeSlot>,
    tickets: HashMap<String, Ticket>,
    /// 等待中的票号，按入队顺序（FIFO）。
    queue: Vec<String>,
    seq: AtomicU64,
}

impl Lobby {
    pub fn new() -> Self {
        Self::default()
    }

    /// 生成场次号（`m-` + 8 位小写十六进制）。调用方负责与既有房间查重。
    pub fn new_match_id(&self) -> String {
        format!("m-{:08x}", random_u64() as u32)
    }

    /// 出房间码：同一 `match_id` 复用未过期的那一份，避免多次分享出多个码。
    pub fn issue_code(&mut self, match_id: &str, now_ms: u64, ttl_ms: u64) -> String {
        self.sweep_codes(now_ms);
        if let Some((code, _)) = self
            .codes
            .iter()
            .find(|(_, slot)| slot.match_id == match_id)
        {
            return code.clone();
        }
        // 撞码概率极低，但仍按容量给足重试；连续撞说明随机源坏了，扩一位兜底。
        for _ in 0..32 {
            let code = random_code(CODE_LEN);
            if !self.codes.contains_key(&code) {
                self.codes.insert(
                    code.clone(),
                    CodeSlot {
                        match_id: match_id.to_owned(),
                        expires_ms: now_ms.saturating_add(ttl_ms),
                    },
                );
                return code;
            }
        }
        let code = random_code(CODE_LEN + 1);
        self.codes.insert(
            code.clone(),
            CodeSlot {
                match_id: match_id.to_owned(),
                expires_ms: now_ms.saturating_add(ttl_ms),
            },
        );
        code
    }

    /// 凭码找房间；大小写不敏感（客户端可能全小写输入）。
    pub fn resolve_code(&mut self, code: &str, now_ms: u64) -> Option<String> {
        self.sweep_codes(now_ms);
        self.codes
            .get(&code.trim().to_ascii_uppercase())
            .map(|slot| slot.match_id.clone())
    }

    /// 入队，返回票号。
    pub fn enqueue(&mut self, game_id: &str, mode: Mode, now_ms: u64) -> String {
        self.sweep_tickets(now_ms);
        let id = format!("t-{:08x}{:04x}", random_u64() as u32, self.seqz() as u16);
        self.tickets.insert(
            id.clone(),
            Ticket {
                game_id: game_id.to_owned(),
                mode,
                state: TicketState::Waiting,
                enqueued_ms: now_ms,
                changed_ms: now_ms,
            },
        );
        self.queue.push(id.clone());
        id
    }

    /// 退队：只允许 `waiting` 的票退，已配对的票退不掉（已经成房）。
    pub fn cancel(&mut self, ticket_id: &str, now_ms: u64) -> bool {
        match self.tickets.get_mut(ticket_id) {
            Some(ticket) if ticket.state == TicketState::Waiting => {
                ticket.state = TicketState::Timeout;
                ticket.changed_ms = now_ms;
                self.queue.retain(|id| id != ticket_id);
                true
            }
            _ => false,
        }
    }

    /// 轮询：等待超时即转 `timeout` 并出队。
    pub fn poll(
        &mut self,
        ticket_id: &str,
        now_ms: u64,
        timeout_ms: u64,
    ) -> Option<(TicketState, Option<String>)> {
        self.expire_waiting(now_ms, timeout_ms);
        let ticket = self.tickets.get(ticket_id)?;
        Some(match &ticket.state {
            TicketState::Matched { match_id } => {
                (TicketState::Matched { match_id: match_id.clone() }, Some(match_id.clone()))
            }
            state => (state.clone(), None),
        })
    }

    /// 尝试凑一桌：够 `seats` 张同 `(game_id, mode)` 的等待票时，成房并返回
    /// `(matchId, 票号列表)`；票随即置为 `matched`。
    ///
    /// 调用方拿到的 `match_id` 必须真的建出房间（本方法只生成号，不回落）；
    /// 由于 `Lobby` 在 `App` 里由同一把锁串行，不会出现两桌抢同一批票。
    pub fn form_group(
        &mut self,
        game_id: &str,
        mode: Mode,
        seats: usize,
        now_ms: u64,
        timeout_ms: u64,
        avoid: &[String],
    ) -> Option<(String, Vec<String>)> {
        self.expire_waiting(now_ms, timeout_ms);
        let candidates: Vec<String> = self
            .queue
            .iter()
            .filter(|id| {
                self.tickets
                    .get(*id)
                    .is_some_and(|t| t.state == TicketState::Waiting && t.game_id == game_id && t.mode == mode)
            })
            .cloned()
            .collect();
        if candidates.len() < seats {
            return None;
        }
        let group: Vec<String> = candidates.into_iter().take(seats).collect();
        // 场次号 32 位随机，撞上已有房间的概率极低；仍给出重试闭环。
        let mut match_id = self.new_match_id();
        for _ in 0..16 {
            if !avoid.contains(&match_id) {
                break;
            }
            match_id = self.new_match_id();
        }
        if avoid.contains(&match_id) {
            return None;
        }
        for id in &group {
            if let Some(ticket) = self.tickets.get_mut(id) {
                ticket.state = TicketState::Matched {
                    match_id: match_id.clone(),
                };
                ticket.changed_ms = now_ms;
            }
        }
        self.queue.retain(|id| !group.contains(id));
        Some((match_id, group))
    }

    /// 某队列当前等待人数（巡检 / 测试用）。
    pub fn waiting(&self, game_id: &str, mode: Mode) -> usize {
        self.queue
            .iter()
            .filter(|id| {
                self.tickets.get(*id).is_some_and(|t| {
                    t.state == TicketState::Waiting && t.game_id == game_id && t.mode == mode
                })
            })
            .count()
    }

    /// 当前有效房间码数（巡检 / 测试用）。
    pub fn code_count(&self) -> usize {
        self.codes.len()
    }

    fn seqz(&self) -> u64 {
        self.seq.fetch_add(1, Ordering::Relaxed)
    }

    fn sweep_codes(&mut self, now_ms: u64) {
        self.codes.retain(|_, slot| slot.expires_ms > now_ms);
    }

    fn sweep_tickets(&mut self, now_ms: u64) {
        let dead: Vec<String> = self
            .tickets
            .iter()
            .filter(|(_, t)| now_ms.saturating_sub(t.changed_ms) > TICKET_RETAIN_MS)
            .map(|(id, _)| id.clone())
            .collect();
        for id in dead {
            self.tickets.remove(&id);
        }
        self.queue.retain(|id| self.tickets.contains_key(id));
    }

    fn expire_waiting(&mut self, now_ms: u64, timeout_ms: u64) {
        let expired: Vec<String> = self
            .queue
            .iter()
            .filter(|id| {
                self.tickets.get(*id).is_some_and(|t| {
                    t.state == TicketState::Waiting
                        && now_ms.saturating_sub(t.enqueued_ms) >= timeout_ms
                })
            })
            .cloned()
            .collect();
        for id in &expired {
            if let Some(ticket) = self.tickets.get_mut(id) {
                ticket.state = TicketState::Timeout;
                ticket.changed_ms = now_ms;
            }
        }
        self.queue.retain(|id| !expired.contains(id));
    }
}

/// 生成 `len` 位房间码。
fn random_code(len: usize) -> String {
    let mut out = String::with_capacity(len);
    let mut bits = random_u64();
    for _ in 0..len {
        if bits == 0 {
            bits = random_u64();
        }
        out.push(CODE_ALPHABET[(bits % CODE_ALPHABET.len() as u64) as usize] as char);
        bits /= CODE_ALPHABET.len() as u64;
    }
    out
}

/// 非密码学随机数：`RandomState` 由 OS 播种，每次哈希掺入自增计数与纳秒时间。
fn random_u64() -> u64 {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
    hasher.write_u64(COUNTER.fetch_add(1, Ordering::Relaxed));
    hasher.write_u128(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0),
    );
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_is_six_chars_from_safe_alphabet_and_unique() {
        let mut lobby = Lobby::new();
        let mut seen = std::collections::HashSet::new();
        for i in 0..64 {
            let code = lobby.issue_code(&format!("m-{i}"), 0, DEFAULT_CODE_TTL_MS);
            assert_eq!(code.len(), CODE_LEN);
            assert!(
                code.chars()
                    .all(|c| CODE_ALPHABET.contains(&(c as u8))),
                "码字符必须来自安全字母表：{code}"
            );
            assert!(seen.insert(code), "房间码必须唯一");
        }
    }

    #[test]
    fn code_is_reused_for_same_room_and_expires() {
        let mut lobby = Lobby::new();
        let first = lobby.issue_code("m-a", 0, 1_000);
        let again = lobby.issue_code("m-a", 10, 1_000);
        assert_eq!(first, again, "同一房间必须复用同一份码");

        assert_eq!(lobby.resolve_code(&first.to_ascii_lowercase(), 999).as_deref(), Some("m-a"));
        assert_eq!(lobby.resolve_code(&first, 1_000), None, "到期即失效");
        assert_eq!(lobby.code_count(), 0, "过期码必须被回收");
        assert_eq!(lobby.resolve_code("ZZZZZZ", 0), None);
    }

    #[test]
    fn queue_is_fifo_and_does_not_mix_modes() {
        let mut lobby = Lobby::new();
        let a = lobby.enqueue("guandan", Mode::Casual, 0);
        let b = lobby.enqueue("guandan", Mode::Ranked, 0);
        let c = lobby.enqueue("guandan", Mode::Casual, 0);
        let d = lobby.enqueue("guandan", Mode::Casual, 0);

        assert!(
            lobby
                .form_group(
                    "guandan",
                    Mode::Casual,
                    4,
                    0,
                    DEFAULT_QUEUE_TIMEOUT_MS,
                    &[],
                )
                .is_none(),
            "三张票不够开桌"
        );
        let e = lobby.enqueue("guandan", Mode::Casual, 0);
        let (match_id, group) = lobby
            .form_group(
                "guandan",
                Mode::Casual,
                4,
                0,
                DEFAULT_QUEUE_TIMEOUT_MS,
                &[],
            )
            .expect("四张同模式票应当成房");
        assert_eq!(group, vec![a, c, d, e], "必须按入队顺序取前四张");
        assert!(!group.contains(&b), "ranked 票不得混进 casual 房");

        for id in &group {
            assert_eq!(
                lobby.poll(id, 0, DEFAULT_QUEUE_TIMEOUT_MS),
                Some((TicketState::Matched { match_id: match_id.clone() }, Some(match_id.clone())))
            );
        }
        assert_eq!(lobby.waiting("guandan", Mode::Casual), 0);
        assert_eq!(lobby.waiting("guandan", Mode::Ranked), 1);
    }

    #[test]
    fn waiting_ticket_times_out_and_leaves_queue() {
        let mut lobby = Lobby::new();
        let id = lobby.enqueue("guandan", Mode::Mixed, 0);
        assert_eq!(
            lobby.poll(&id, 5_000, DEFAULT_QUEUE_TIMEOUT_MS),
            Some((TicketState::Waiting, None))
        );
        assert_eq!(
            lobby.poll(&id, DEFAULT_QUEUE_TIMEOUT_MS, DEFAULT_QUEUE_TIMEOUT_MS),
            Some((TicketState::Timeout, None))
        );
        assert_eq!(lobby.waiting("guandan", Mode::Mixed), 0, "超时票必须出队");
        assert!(
            lobby
                .form_group(
                    "guandan",
                    Mode::Mixed,
                    4,
                    DEFAULT_QUEUE_TIMEOUT_MS,
                    DEFAULT_QUEUE_TIMEOUT_MS,
                    &[],
                )
                .is_none(),
            "超时票不得再参与成房"
        );
    }

    #[test]
    fn cancel_only_works_while_waiting() {
        let mut lobby = Lobby::new();
        let a = lobby.enqueue("guandan", Mode::Casual, 0);
        assert!(lobby.cancel(&a, 1));
        assert!(!lobby.cancel(&a, 2), "已退队的票不能再退");
        assert_eq!(lobby.poll(&a, 2, DEFAULT_QUEUE_TIMEOUT_MS).map(|(s, _)| s), Some(TicketState::Timeout));

        let mut ids = Vec::new();
        for _ in 0..4 {
            ids.push(lobby.enqueue("guandan", Mode::Casual, 0));
        }
        assert!(
            lobby
                .form_group("guandan", Mode::Casual, 4, 0, DEFAULT_QUEUE_TIMEOUT_MS, &[])
                .is_some()
        );
        assert!(!lobby.cancel(&ids[0], 2), "已配对的票不得退队");
    }

    #[test]
    fn poll_unknown_ticket_is_none() {
        let mut lobby = Lobby::new();
        assert!(lobby.poll("t-deadbeef", 0, DEFAULT_QUEUE_TIMEOUT_MS).is_none());
    }
}

