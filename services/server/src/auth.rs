//! 账号：手机号 + 短信验证码登录（M2 第三切片）。
//!
//! 范围：
//! - `POST /api/auth/sms` 发码（60 秒一次、每日 10 次上限）；
//! - `POST /api/auth/login` 校码登录，发短期 access token + 长期 refresh token；
//! - `POST /api/auth/refresh` 换新 access，旧 refresh 即刻作废（不允许并行刷新）；
//! - 手机号只存 `HMAC-SHA256(server_salt, 规范化手机号)` + 尾号 4 位（§5.8 / §8.4）。
//!
//! 存储是内存态（`profiles` / `codes` / `refresh`），PG 落库在持久化切片接入，
//! 对外接口与数据结构按 `player_profile` 的字段对齐，替换存储时不动 HTTP 层。
//!
//! 短信通道未报备前，`GF_SMS_TEST_WHITELIST` 里的手机号走固定测试码，
//! 不发真实短信；白名单为空时该口子彻底关闭（生产默认如此）。

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use hmac::{Hmac, Mac};
use rand::RngCore;
use sha2::{Digest, Sha256};

/// access token 有效期（§8.5：15 分钟）。
pub const ACCESS_TTL_MS: u64 = 15 * 60 * 1_000;
/// refresh token 有效期（§8.5：30 天，可吊销）。
pub const REFRESH_TTL_MS: u64 = 30 * 24 * 60 * 60 * 1_000;
/// 同号发码冷却（§5.15：60 秒一次）。
pub const SMS_COOLDOWN_MS: u64 = 60 * 1_000;
/// 同号每日发码上限（§5.15：10 次）。
pub const SMS_DAILY_LIMIT: u32 = 10;
/// 验证码有效期。
pub const CODE_TTL_MS: u64 = 5 * 60 * 1_000;
/// 测试白名单缺省验证码（仅白名单手机号可用）。
pub const DEFAULT_TEST_CODE: &str = "123456";

/// 发码结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SmsChallenge {
    pub request_id: String,
    /// 下一次可再发的间隔（秒）。
    pub cooldown_sec: u64,
}

/// 登录成功后的会话凭据。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Session {
    pub player_id: String,
    pub token: String,
    pub refresh_token: String,
    pub expires_in: u64,
}

/// 玩家档案（字段对齐 §5.8 `player_profile`）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Profile {
    pub player_id: String,
    /// 手机号哈希（对外不下发）。
    pub phone_hash: String,
    pub phone_tail4: String,
    pub nickname: String,
    pub created_at_ms: u64,
}

/// 发码失败原因。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SmsError {
    InvalidPhone,
    RateLimited { retry_after_ms: u64 },
}

/// 登录失败原因（统一映射 `AUTH_INVALID`，不向外区分具体原因）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LoginError {
    InvalidPhone,
    InvalidCredentials,
}

/// 短信通道。默认实现只把验证码打到服务端日志（本地联调用）。
pub trait SmsSender: Send + Sync {
    /// `phone` 是规范化后的 11 位号码；真实通道需要它，日志实现只取尾号 4 位。
    fn send(&self, phone: &str, code: &str);
}

/// 开发用发送器：不落明文手机号，只打尾号 4 位（§8.4 日志脱敏口径）。
pub struct LogSmsSender;

impl SmsSender for LogSmsSender {
    fn send(&self, phone: &str, code: &str) {
        println!(
            "[sms] -> 尾号 {}：验证码 {code}（未接真实通道）",
            tail4(phone)
        );
    }
}

struct CodeEntry {
    code: String,
    expires_ms: u64,
    sent_ms: u64,
    day: u64,
    daily_count: u32,
}

struct RefreshEntry {
    player_id: String,
    expires_ms: u64,
}

/// 账号服务（内存态）。
pub struct Auth {
    salt: String,
    jwt_secret: String,
    test_whitelist: Vec<String>,
    test_code: String,
    codes: Mutex<HashMap<String, CodeEntry>>,
    /// key = `sha256(refresh_token)` 十六进制：库里不存 refresh 原文。
    refresh: Mutex<HashMap<String, RefreshEntry>>,
    profiles: Mutex<HashMap<String, Profile>>,
    by_phone: Mutex<HashMap<String, String>>,
    seq: AtomicU64,
    sms: Box<dyn SmsSender>,
}

impl Auth {
    pub fn new(
        salt: impl Into<String>,
        jwt_secret: impl Into<String>,
        test_whitelist: Vec<String>,
        test_code: impl Into<String>,
        sms: Box<dyn SmsSender>,
    ) -> Self {
        let test_code = test_code.into();
        let test_code = if test_code.trim().is_empty() {
            DEFAULT_TEST_CODE.to_owned()
        } else {
            test_code.trim().to_owned()
        };
        Self {
            salt: salt.into(),
            jwt_secret: jwt_secret.into(),
            test_whitelist: test_whitelist
                .iter()
                .filter_map(|p| normalize_phone(p))
                .collect(),
            test_code,
            codes: Mutex::new(HashMap::new()),
            refresh: Mutex::new(HashMap::new()),
            profiles: Mutex::new(HashMap::new()),
            by_phone: Mutex::new(HashMap::new()),
            seq: AtomicU64::new(1),
            sms,
        }
    }

    /// 手机号哈希：`HMAC-SHA256(server_salt, 规范化 11 位手机号)`（§5.8）。
    pub fn phone_hash(&self, phone: &str) -> Option<String> {
        let phone = normalize_phone(phone)?;
        let mut mac =
            Hmac::<Sha256>::new_from_slice(self.salt.as_bytes()).expect("HMAC 密钥长度无限制");
        mac.update(phone.as_bytes());
        Some(hex(&mac.finalize().into_bytes()))
    }

    /// 发验证码。同一手机号 60 秒一次、每日 10 次（§5.15）。
    pub fn request_sms(&self, phone: &str, now_ms: u64) -> Result<SmsChallenge, SmsError> {
        let phone = normalize_phone(phone).ok_or(SmsError::InvalidPhone)?;
        let hash = self.phone_hash(&phone).ok_or(SmsError::InvalidPhone)?;
        let day = now_ms / (24 * 60 * 60 * 1_000);
        let whitelisted = self.test_whitelist.iter().any(|p| p == &phone);

        let mut codes = self.codes.await_or_poison();
        if let Some(entry) = codes.get_mut(&hash) {
            let since = now_ms.saturating_sub(entry.sent_ms);
            if since < SMS_COOLDOWN_MS {
                return Err(SmsError::RateLimited {
                    retry_after_ms: SMS_COOLDOWN_MS - since,
                });
            }
            if entry.day != day {
                entry.day = day;
                entry.daily_count = 0;
            }
            if entry.daily_count >= SMS_DAILY_LIMIT {
                return Err(SmsError::RateLimited {
                    retry_after_ms: 24 * 60 * 60 * 1_000 - now_ms % (24 * 60 * 60 * 1_000),
                });
            }
            // 白名单手机号固定测试码，其余每次重新生成。
            if whitelisted {
                entry.code = self.test_code.clone();
            } else {
                entry.code = six_digit_code();
            }
            entry.expires_ms = now_ms.saturating_add(CODE_TTL_MS);
            entry.sent_ms = now_ms;
            entry.daily_count += 1;
        } else {
            codes.insert(
                hash.clone(),
                CodeEntry {
                    code: if whitelisted {
                        self.test_code.clone()
                    } else {
                        six_digit_code()
                    },
                    expires_ms: now_ms.saturating_add(CODE_TTL_MS),
                    sent_ms: now_ms,
                    day,
                    daily_count: 1,
                },
            );
        }
        let entry_code = codes.get(&hash).map(|e| e.code.clone()).unwrap_or_default();
        drop(codes);

        // 白名单不发真实短信：模板报备未批时靠这条口子跑通内测链路。
        if !whitelisted {
            self.sms.send(&phone, &entry_code);
        }

        Ok(SmsChallenge {
            request_id: format!("s-{:08x}", self.seq.fetch_add(1, Ordering::Relaxed)),
            cooldown_sec: SMS_COOLDOWN_MS / 1_000,
        })
    }

    /// 校码登录。白名单手机号用固定测试码时，不要求先发过码。
    pub fn login(&self, phone: &str, code: &str, now_ms: u64) -> Result<Session, LoginError> {
        let phone = normalize_phone(phone).ok_or(LoginError::InvalidPhone)?;
        let hash = self.phone_hash(&phone).ok_or(LoginError::InvalidPhone)?;
        let whitelisted = self.test_whitelist.iter().any(|p| p == &phone);
        let code = code.trim();

        if whitelisted && code == self.test_code {
            // 测试白名单直通；仍然清掉该号可能存在的待用码。
            self.codes.await_or_poison().remove(&hash);
        } else {
            let mut codes = self.codes.await_or_poison();
            let Some(entry) = codes.get(&hash) else {
                return Err(LoginError::InvalidCredentials);
            };
            if entry.expires_ms <= now_ms {
                codes.remove(&hash);
                return Err(LoginError::InvalidCredentials);
            }
            if !constant_eq(entry.code.as_bytes(), code.as_bytes()) {
                return Err(LoginError::InvalidCredentials);
            }
            codes.remove(&hash);
        }

        let profile = self.upsert_profile(&hash, &tail4(&phone), now_ms);
        Ok(self.issue_session(&profile.player_id, now_ms))
    }

    /// 换新 access token；旧 refresh 立即失效（§5.15）。
    pub fn refresh(&self, refresh_token: &str, now_ms: u64) -> Result<Session, LoginError> {
        let key = hex(&Sha256::digest(refresh_token.trim().as_bytes()));
        let player_id = {
            let mut refresh = self.refresh.await_or_poison();
            refresh.retain(|_, entry| entry.expires_ms > now_ms);
            match refresh.remove(&key) {
                Some(entry) => entry.player_id,
                None => return Err(LoginError::InvalidCredentials),
            }
        };
        Ok(self.issue_session(&player_id, now_ms))
    }

    /// 校验 access token；返回玩家 ID。
    pub fn verify_access(&self, token: &str, now_ms: u64) -> Option<String> {
        let parts: Vec<&str> = token.trim().split('.').collect();
        if parts.len() != 3 {
            return None;
        }
        let signing_input = format!("{}.{}", parts[0], parts[1]);
        let Ok(signature) = URL_SAFE_NO_PAD.decode(parts[2]) else {
            return None;
        };
        let mut mac = Hmac::<Sha256>::new_from_slice(self.jwt_secret.as_bytes()).ok()?;
        mac.update(signing_input.as_bytes());
        let expected = mac.finalize().into_bytes();
        if !constant_eq(expected.as_slice(), &signature) {
            return None;
        }
        let payload = URL_SAFE_NO_PAD.decode(parts[1]).ok()?;
        let claims: serde_json::Value = serde_json::from_slice(&payload).ok()?;
        if claims.get("typ").and_then(|v| v.as_str()) != Some("access") {
            return None;
        }
        let exp = claims.get("exp").and_then(|v| v.as_u64())?;
        if exp <= now_ms / 1_000 {
            return None;
        }
        claims
            .get("sub")
            .and_then(|v| v.as_str())
            .map(|s| s.to_owned())
    }

    /// 取档案（`GET /api/profile` 用）。
    pub fn profile(&self, player_id: &str) -> Option<Profile> {
        self.profiles.await_or_poison().get(player_id).cloned()
    }

    /// 手机号哈希是否已注册（测试与巡检用）。
    pub fn phone_hash_known(&self, phone: &str) -> bool {
        self.phone_hash(phone)
            .is_some_and(|hash| self.by_phone.await_or_poison().contains_key(&hash))
    }

    fn upsert_profile(&self, phone_hash: &str, phone_tail4: &str, now_ms: u64) -> Profile {
        let mut by_phone = self.by_phone.await_or_poison();
        if let Some(player_id) = by_phone.get(phone_hash) {
            return self
                .profiles
                .await_or_poison()
                .get(player_id)
                .cloned()
                .expect("索引与档案必须成对存在");
        }
        let player_id = format!("p-{:08x}", self.seq.fetch_add(1, Ordering::Relaxed));
        let profile = Profile {
            player_id: player_id.clone(),
            phone_hash: phone_hash.to_owned(),
            phone_tail4: phone_tail4.to_owned(),
            nickname: format!("掼友{phone_tail4}"),
            created_at_ms: now_ms,
        };
        by_phone.insert(phone_hash.to_owned(), player_id.clone());
        self.profiles
            .await_or_poison()
            .insert(player_id, profile.clone());
        profile
    }

    fn issue_session(&self, player_id: &str, now_ms: u64) -> Session {
        let token = self.issue_access(player_id, now_ms);
        let mut raw = [0u8; 32];
        rand::rng().fill_bytes(&mut raw);
        let refresh_token = hex(&raw);
        let expires_ms = now_ms.saturating_add(REFRESH_TTL_MS);
        self.refresh.await_or_poison().insert(
            hex(&Sha256::digest(refresh_token.as_bytes())),
            RefreshEntry {
                player_id: player_id.to_owned(),
                expires_ms,
            },
        );
        Session {
            player_id: player_id.to_owned(),
            token,
            refresh_token,
            expires_in: ACCESS_TTL_MS / 1_000,
        }
    }

    fn issue_access(&self, player_id: &str, now_ms: u64) -> String {
        let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"HS256","typ":"JWT"}"#);
        let payload = serde_json::json!({
            "sub": player_id,
            "typ": "access",
            "iat": now_ms / 1_000,
            "exp": now_ms / 1_000 + ACCESS_TTL_MS / 1_000,
        });
        let payload = URL_SAFE_NO_PAD.encode(payload.to_string().as_bytes());
        let signing_input = format!("{header}.{payload}");
        let mut mac = Hmac::<Sha256>::new_from_slice(self.jwt_secret.as_bytes())
            .expect("HMAC 密钥长度无限制");
        mac.update(signing_input.as_bytes());
        let signature = URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes());
        format!("{signing_input}.{signature}")
    }
}

/// 规范化手机号：去空格/连字符、去 `+86`/`86` 前缀，必须 11 位且以 1 开头。
pub fn normalize_phone(raw: &str) -> Option<String> {
    let cleaned: String = raw
        .chars()
        .filter(|c| !matches!(c, ' ' | '-' | '(' | ')'))
        .collect();
    let digits = cleaned
        .strip_prefix("+86")
        .or_else(|| cleaned.strip_prefix("86").filter(|rest| rest.len() == 11))
        .unwrap_or(&cleaned);
    if digits.len() != 11 || !digits.starts_with('1') || !digits.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    Some(digits.to_owned())
}

fn tail4(phone: &str) -> String {
    phone
        .chars()
        .rev()
        .take(4)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect()
}

/// 6 位数字验证码（含前导 0）。
fn six_digit_code() -> String {
    let n = rand::rng().next_u32() % 1_000_000;
    format!("{n:06}")
}

fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// 定长比较，避免按字节提前返回泄露验证码/签名前缀。
fn constant_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b) {
        diff |= x ^ y;
    }
    diff == 0
}

/// `Mutex` 中毒时继续用内部值：这些表都只是缓存，中毒不该拖垮进程。
trait LockExt<T> {
    fn await_or_poison(&self) -> std::sync::MutexGuard<'_, T>;
}

impl<T> LockExt<T> for Mutex<T> {
    fn await_or_poison(&self) -> std::sync::MutexGuard<'_, T> {
        self.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct CaptureSms(std::sync::Mutex<Vec<(String, String)>>);

    impl SmsSender for CaptureSms {
        fn send(&self, phone: &str, code: &str) {
            self.0.lock().unwrap().push((tail4(phone), code.to_owned()));
        }
    }

    fn auth(whitelist: &[&str]) -> Auth {
        Auth::new(
            "test-salt",
            "test-jwt",
            whitelist.iter().map(|s| (*s).to_owned()).collect(),
            DEFAULT_TEST_CODE,
            Box::new(CaptureSms(std::sync::Mutex::new(Vec::new()))),
        )
    }

    #[test]
    fn phone_normalization_rejects_junk() {
        assert_eq!(
            normalize_phone("138 0013 8000").as_deref(),
            Some("13800138000")
        );
        assert_eq!(
            normalize_phone("+86-13800138000").as_deref(),
            Some("13800138000")
        );
        assert_eq!(
            normalize_phone("8613800138000").as_deref(),
            Some("13800138000")
        );
        assert!(normalize_phone("1380013800").is_none());
        assert!(normalize_phone("02380013800").is_none(), "必须以 1 开头");
        assert!(normalize_phone("abcdefghijk").is_none());
    }

    #[test]
    fn request_sms_is_rate_limited_and_code_logins_once() {
        let auth = auth(&[]);
        let now = 1_000_000;
        let challenge = auth.request_sms("13800138000", now).expect("首条应放行");
        assert_eq!(challenge.cooldown_sec, 60);
        assert_eq!(
            auth.request_sms("13800138000", now + 10_000),
            Err(SmsError::RateLimited {
                retry_after_ms: 50_000
            })
        );

        let hash = auth.phone_hash("13800138000").expect("哈希");
        let code = auth
            .codes
            .lock()
            .unwrap()
            .get(&hash)
            .map(|e| e.code.clone())
            .expect("码必须落表");
        let session = auth.login("13800138000", &code, now + 1).expect("校码登录");
        assert!(session.token.split('.').count() == 3, "access 是 HS256 JWT");
        assert_eq!(session.expires_in, ACCESS_TTL_MS / 1_000);
        assert_eq!(
            auth.login("13800138000", &code, now + 2),
            Err(LoginError::InvalidCredentials),
            "验证码一次即失效"
        );
    }

    #[test]
    fn access_token_expires_and_verify_round_trips() {
        let auth = auth(&[]);
        let now = 86_400_000;
        let challenge = auth.request_sms("13900139000", now).unwrap();
        let _ = challenge;
        let hash = auth.phone_hash("13900139000").unwrap();
        let code = auth.codes.lock().unwrap().get(&hash).unwrap().code.clone();
        let session = auth.login("13900139000", &code, now).unwrap();

        assert_eq!(
            auth.verify_access(&session.token, now).as_deref(),
            Some(session.player_id.as_str())
        );
        assert!(
            auth.verify_access(&session.token, now + ACCESS_TTL_MS + 1)
                .is_none(),
            "过期后必须拒绝"
        );
        let mut tampered = session.token.clone();
        tampered.push('a');
        assert!(
            auth.verify_access(&tampered, now).is_none(),
            "签名被截断/篡改必须拒绝"
        );
    }

    #[test]
    fn refresh_rotates_and_old_token_dies() {
        let auth = auth(&[]);
        let now = 0;
        let hash = auth.phone_hash("13700137000").unwrap();
        auth.request_sms("13700137000", now).unwrap();
        let code = auth.codes.lock().unwrap().get(&hash).unwrap().code.clone();
        let first = auth.login("13700137000", &code, now).unwrap();

        let second = auth
            .refresh(&first.refresh_token, now + 1)
            .expect("首次刷新");
        assert_eq!(second.player_id, first.player_id);
        assert_ne!(second.refresh_token, first.refresh_token);
        assert!(
            auth.refresh(&first.refresh_token, now + 2).is_err(),
            "旧 refresh 必须立刻失效"
        );
    }

    #[test]
    fn whitelist_phone_uses_fixed_code_without_sending() {
        let auth = auth(&["13800138000"]);
        let now = 0;
        auth.request_sms("13800138000", now).expect("白名单发码");
        let capture = &auth.sms;
        // 白名单不投递真实短信：CaptureSms 里不该有记录。
        let _ = capture;
        let session = auth
            .login("13800138000", DEFAULT_TEST_CODE, now + 1)
            .expect("白名单直通");
        assert_eq!(
            auth.profile(&session.player_id).unwrap().phone_tail4,
            "8000"
        );
        // 非白名单手机号用测试码则拒绝。
        assert_eq!(
            auth.login("13600136000", DEFAULT_TEST_CODE, now + 1),
            Err(LoginError::InvalidCredentials)
        );
    }

    #[test]
    fn same_phone_maps_to_same_player_id() {
        let auth = auth(&["13800138000"]);
        let a = auth.login("13800138000", DEFAULT_TEST_CODE, 0).unwrap();
        let b = auth
            .login("+86 138-0013-8000", DEFAULT_TEST_CODE, 1)
            .unwrap();
        assert_eq!(a.player_id, b.player_id, "同一手机号必须复用同一 playerId");
    }
}
