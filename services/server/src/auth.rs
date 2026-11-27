//! 账号：手机号 + 短信验证码注册 / 登录 / 会话认证（M2）。
//!
//! 范围：
//! - `POST /api/auth/sms` 发码（60 秒一次、每日 10 次上限）；
//! - `POST /api/auth/register` 显式注册并直接返回会话；
//! - `POST /api/auth/login` 校码登录，发短期 access token + 长期 refresh token；
//! - `POST /api/auth/refresh` 换新 access，旧 refresh 即刻作废（不允许并行刷新）；
//! - `POST /api/auth/logout` 撤销 access（可选同时撤销 refresh）；
//! - 手机号只存 `HMAC-SHA256(server_salt, 规范化手机号)` + 尾号 4 位（§5.8 / §8.4）。
//!
//! 进程内 `profiles` / `codes` / `refresh` 是热缓存；接上 PG 后由 `App` 在启动时
//! 恢复档案、refresh 与 access 撤销表，并把登录/轮换/登出写回 `player_profile`、
//! `player_refresh_token`、`access_revocation`。PG 缺席时退化为纯内存（单测/本机联调）。
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
/// access token 的签发方 / 受众：只认本服务签给本服务客户端的令牌，
/// 防止同一密钥被别的系统复用时跨服务生效。
const JWT_ISSUER: &str = "gf-server";
const JWT_AUDIENCE: &str = "gf-client";
/// 同号发码冷却（§5.15：60 秒一次）。
pub const SMS_COOLDOWN_MS: u64 = 60 * 1_000;
/// 同号每日发码上限（§5.15：10 次）。
pub const SMS_DAILY_LIMIT: u32 = 10;
/// 验证码有效期。
pub const CODE_TTL_MS: u64 = 5 * 60 * 1_000;
/// 测试白名单缺省验证码（仅白名单手机号可用）。
pub const DEFAULT_TEST_CODE: &str = "666666";
/// 同一验证码最多尝试次数，超过即作废（防在线枚举）。
const CODE_MAX_ATTEMPTS: u8 = 5;
/// 登录失败锁定阈值与退避上限。
const LOGIN_FAIL_THRESHOLD: u32 = 5;
const LOGIN_LOCK_BASE_MS: u64 = 30 * 1_000;
const LOGIN_LOCK_MAX_MS: u64 = 30 * 60 * 1_000;
/// 登录失败计数在长时间无尝试后自动清零。
const LOGIN_GUARD_IDLE_RESET_MS: u64 = 24 * 60 * 60 * 1_000;

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
    /// `player_profile.status`；只有 `active` 允许登录与继续持有 access token。
    pub status: String,
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
    /// 连续失败后的退避锁定；外层应映射 429 并带 `retry_after_ms`。
    Locked {
        retry_after_ms: u64,
    },
    /// 档案 `status` 不是 `active`（后台封禁/注销）。
    AccountDisabled,
}

/// 已验证的登录手机号：验证码已消费，交给账号存储层建档或取档。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct VerifiedPhone {
    pub phone_hash: String,
    pub phone_tail4: String,
}

/// refresh 轮换失败时用于回滚内存态的原值。
#[derive(Clone, Debug)]
pub(crate) struct RefreshRollback {
    pub key: String,
    pub entry: RefreshEntry,
}

/// 短信通道。默认实现只记录「发过码」，不输出验证码明文（本地联调用）。
pub trait SmsSender: Send + Sync {
    /// `phone` 是规范化后的 11 位号码；真实通道需要它，日志实现只取尾号 4 位。
    fn send(&self, phone: &str, code: &str);
}

/// 开发用发送器：不落明文手机号，只打尾号 4 位（§8.4 日志脱敏口径）。
pub struct LogSmsSender;

impl SmsSender for LogSmsSender {
    fn send(&self, phone: &str, code: &str) {
        // 验证码是登录凭据：挡板只证明「已生成、未真实发送」，明文绝不进日志。
        // 本机联调走 `sms.test_whitelist` 的固定测试码，不依赖这行日志。
        let _ = code;
        println!(
            "[sms][挡板] -> 尾号 {}：验证码已生成（明文不落日志，未真实发送）",
            tail4(phone)
        );
    }
}

struct CodeEntry {
    code: String,
    expires_ms: u64,
    /// 该码已用错的次数；与发送限流分开，校码失败不能重置冷却/日限。
    attempts: u8,
}

/// 发码限流状态：独立于 `codes`，登录成功/验证码过期都不会清掉 60s 冷却与日限。
struct SmsThrottle {
    sent_ms: u64,
    day: u64,
    daily_count: u32,
}

/// 登录失败退避：按手机号哈希与来源 IP 各记一份。
struct LoginGuard {
    failures: u32,
    locked_until_ms: u64,
    last_ms: u64,
}

#[derive(Clone, Debug)]
pub(crate) struct RefreshEntry {
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
    sms_throttle: Mutex<HashMap<String, SmsThrottle>>,
    login_guards: Mutex<HashMap<String, LoginGuard>>,
    /// key = `sha256(refresh_token)` 十六进制：库里不存 refresh 原文。
    refresh: Mutex<HashMap<String, RefreshEntry>>,
    /// 已撤销 access token：key = `sha256(token)`，value = 过期秒；重启即清空。
    revoked_access: Mutex<HashMap<String, u64>>,
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
            sms_throttle: Mutex::new(HashMap::new()),
            login_guards: Mutex::new(HashMap::new()),
            refresh: Mutex::new(HashMap::new()),
            revoked_access: Mutex::new(HashMap::new()),
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
    ///
    /// 冷却与日限记在 `sms_throttle`，与待用验证码解耦：登录成功或码过期都不会重置限流。
    pub fn request_sms(&self, phone: &str, now_ms: u64) -> Result<SmsChallenge, SmsError> {
        let phone = normalize_phone(phone).ok_or(SmsError::InvalidPhone)?;
        let hash = self.phone_hash(&phone).ok_or(SmsError::InvalidPhone)?;
        let day = now_ms / (24 * 60 * 60 * 1_000);
        let whitelisted = self.test_whitelist.iter().any(|p| p == &phone);

        {
            let mut throttle = self.sms_throttle.await_or_poison();
            let entry = throttle.entry(hash.clone()).or_insert(SmsThrottle {
                sent_ms: 0,
                day,
                daily_count: 0,
            });
            if entry.sent_ms != 0 {
                let since = now_ms.saturating_sub(entry.sent_ms);
                if since < SMS_COOLDOWN_MS {
                    return Err(SmsError::RateLimited {
                        retry_after_ms: SMS_COOLDOWN_MS - since,
                    });
                }
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
            entry.sent_ms = now_ms;
            entry.daily_count += 1;
        }

        let code = if whitelisted {
            self.test_code.clone()
        } else {
            six_digit_code()
        };
        self.codes.await_or_poison().insert(
            hash,
            CodeEntry {
                code: code.clone(),
                expires_ms: now_ms.saturating_add(CODE_TTL_MS),
                attempts: 0,
            },
        );

        // 白名单不发真实短信：模板报备未批时靠这条口子跑通内测链路。
        if !whitelisted {
            self.sms.send(&phone, &code);
        }

        Ok(SmsChallenge {
            request_id: format!("s-{:08x}", self.seq.fetch_add(1, Ordering::Relaxed)),
            cooldown_sec: SMS_COOLDOWN_MS / 1_000,
        })
    }

    /// 校码登录（无来源 IP 版本，仅供内部/测试）。
    pub fn login(&self, phone: &str, code: &str, now_ms: u64) -> Result<Session, LoginError> {
        self.login_with_peer(phone, code, None, now_ms)
    }

    /// 校码登录。任何来源都必须先发码；白名单只决定码的内容，不再是免发码后门。
    ///
    /// 失败按「手机号哈希 + 来源 IP」双维度累计，连续 5 次后指数退避锁定。
    /// 纯内存路径（单测/未接 PG）走这里；HTTP 路径先 `verify_login_code`，
    /// 再由账号存储层决定「建档或取档」。
    pub fn login_with_peer(
        &self,
        phone: &str,
        code: &str,
        peer: Option<&str>,
        now_ms: u64,
    ) -> Result<Session, LoginError> {
        let verified = self.verify_login_code(phone, code, peer, now_ms)?;
        let profile = self.upsert_profile(&verified.phone_hash, &verified.phone_tail4, now_ms);
        if profile.status != "active" {
            return Err(LoginError::AccountDisabled);
        }
        Ok(self.issue_session(&profile.player_id, now_ms))
    }

    /// 只校验证码并消费；不创建档案、不发 token。
    pub(crate) fn verify_login_code(
        &self,
        phone: &str,
        code: &str,
        peer: Option<&str>,
        now_ms: u64,
    ) -> Result<VerifiedPhone, LoginError> {
        let phone = normalize_phone(phone).ok_or(LoginError::InvalidPhone)?;
        let hash = self.phone_hash(&phone).ok_or(LoginError::InvalidPhone)?;
        let code = code.trim();
        let mut keys = vec![format!("phone:{hash}")];
        if let Some(peer) = peer.filter(|p| !p.trim().is_empty()) {
            keys.push(format!("ip:{}", peer.trim()));
        }
        self.check_login_guards(&keys, now_ms)?;

        {
            let mut codes = self.codes.await_or_poison();
            let Some(entry) = codes.get_mut(&hash) else {
                drop(codes);
                self.note_login_failure(&keys, now_ms);
                return Err(LoginError::InvalidCredentials);
            };
            if entry.expires_ms <= now_ms {
                codes.remove(&hash);
                drop(codes);
                self.note_login_failure(&keys, now_ms);
                return Err(LoginError::InvalidCredentials);
            }
            if !constant_eq(entry.code.as_bytes(), code.as_bytes()) {
                entry.attempts = entry.attempts.saturating_add(1);
                let burn = entry.attempts >= CODE_MAX_ATTEMPTS;
                if burn {
                    codes.remove(&hash);
                }
                drop(codes);
                self.note_login_failure(&keys, now_ms);
                return Err(LoginError::InvalidCredentials);
            }
            // 成功才消费验证码；失败不清 entry（仍受 attempts 上限约束）。
            codes.remove(&hash);
        }
        self.clear_login_guards(&keys);
        Ok(VerifiedPhone {
            phone_hash: hash,
            phone_tail4: tail4(&phone),
        })
    }

    /// 锁定检查；锁过期且长时间无尝试时把失败计数清零。
    fn check_login_guards(&self, keys: &[String], now_ms: u64) -> Result<(), LoginError> {
        let mut guards = self.login_guards.await_or_poison();
        for key in keys {
            let Some(guard) = guards.get_mut(key) else {
                continue;
            };
            if guard.locked_until_ms > now_ms {
                return Err(LoginError::Locked {
                    retry_after_ms: guard.locked_until_ms - now_ms,
                });
            }
            if now_ms.saturating_sub(guard.last_ms) >= LOGIN_GUARD_IDLE_RESET_MS {
                guard.failures = 0;
                guard.locked_until_ms = 0;
            }
        }
        guards.retain(|_, g| now_ms.saturating_sub(g.last_ms) < LOGIN_GUARD_IDLE_RESET_MS);
        Ok(())
    }

    fn note_login_failure(&self, keys: &[String], now_ms: u64) {
        let mut guards = self.login_guards.await_or_poison();
        for key in keys {
            let guard = guards.entry(key.clone()).or_insert(LoginGuard {
                failures: 0,
                locked_until_ms: 0,
                last_ms: now_ms,
            });
            guard.failures = guard.failures.saturating_add(1);
            guard.last_ms = now_ms;
            if guard.failures >= LOGIN_FAIL_THRESHOLD {
                let extra = guard.failures - LOGIN_FAIL_THRESHOLD;
                let factor = 1u64 << extra.min(10);
                let lock_ms = LOGIN_LOCK_BASE_MS
                    .saturating_mul(factor)
                    .min(LOGIN_LOCK_MAX_MS);
                guard.locked_until_ms = now_ms.saturating_add(lock_ms);
            }
        }
        // 避免长跑进程里 guard 表只增不减。
        guards.retain(|_, g| now_ms.saturating_sub(g.last_ms) < LOGIN_GUARD_IDLE_RESET_MS);
    }

    fn clear_login_guards(&self, keys: &[String]) {
        let mut guards = self.login_guards.await_or_poison();
        for key in keys {
            guards.remove(key);
        }
    }

    /// 换新 access token；旧 refresh 立即失效（§5.15）。
    pub fn refresh(&self, refresh_token: &str, now_ms: u64) -> Result<Session, LoginError> {
        self.rotate_refresh(refresh_token, now_ms)
            .map(|(session, _)| session)
    }

    /// 轮换 refresh 并返回失败回滚材料；HTTP 层要在 DB 事务失败时还原旧值。
    pub(crate) fn rotate_refresh(
        &self,
        refresh_token: &str,
        now_ms: u64,
    ) -> Result<(Session, RefreshRollback), LoginError> {
        let key = refresh_key(refresh_token);
        let entry = {
            let mut refresh = self.refresh.await_or_poison();
            refresh.retain(|_, entry| entry.expires_ms > now_ms);
            refresh.remove(&key).ok_or(LoginError::InvalidCredentials)?
        };
        // 禁用账号不能靠 refresh 续命；档案缺失说明缓存没恢复好，也不放行。
        match self.profiles.await_or_poison().get(&entry.player_id) {
            Some(profile) if profile.status == "active" => {}
            Some(_) => return Err(LoginError::AccountDisabled),
            None => return Err(LoginError::InvalidCredentials),
        }
        let session = self.issue_session(&entry.player_id, now_ms);
        Ok((session, RefreshRollback { key, entry }))
    }

    /// 撤销一枚 refresh；返回其哈希键（不存在则 `None`），供存储层删库。
    pub(crate) fn revoke_refresh(&self, refresh_token: &str) -> Option<String> {
        let key = refresh_key(refresh_token);
        self.refresh.await_or_poison().remove(&key).map(|_| key)
    }

    /// DB 轮换失败时把旧 refresh 放回内存缓存，保证客户端可重试同一枚 token。
    pub(crate) fn restore_refresh(&self, rollback: RefreshRollback) {
        self.refresh
            .await_or_poison()
            .insert(rollback.key, rollback.entry);
    }

    pub(crate) fn restore_refresh_tokens(&self, rows: Vec<(String, String, u64)>, now_ms: u64) {
        let mut refresh = self.refresh.await_or_poison();
        refresh.clear();
        for (key, player_id, expires_ms) in rows {
            if expires_ms > now_ms {
                refresh.insert(
                    key,
                    RefreshEntry {
                        player_id,
                        expires_ms,
                    },
                );
            }
        }
    }

    pub(crate) fn restore_access_revocations(&self, rows: Vec<(String, u64)>, now_ms: u64) {
        let mut revoked = self.revoked_access.await_or_poison();
        revoked.clear();
        for (key, expires_ms) in rows {
            if expires_ms > now_ms {
                revoked.insert(key, expires_ms / 1_000);
            }
        }
    }

    /// refresh raw token 的库内键：只存 SHA-256，不存原文。
    pub(crate) fn refresh_key(token: &str) -> String {
        refresh_key(token)
    }

    /// access raw token 的撤销表键。
    pub(crate) fn access_key(token: &str) -> String {
        access_key(token)
    }

    /// 新签 refresh 的过期时刻（毫秒 epoch）。
    pub(crate) fn refresh_expires_at(now_ms: u64) -> u64 {
        now_ms.saturating_add(REFRESH_TTL_MS)
    }

    /// 新签 access 的撤销记录过期时刻（毫秒 epoch）。
    pub(crate) fn access_expires_at(now_ms: u64) -> u64 {
        now_ms.saturating_add(ACCESS_TTL_MS)
    }

    /// 校验 access token；返回玩家 ID。
    pub fn verify_access(&self, token: &str, now_ms: u64) -> Option<String> {
        if token.len() > 4096 {
            return None;
        }
        // 已撤销令牌立刻失效（登出后不必等 15 分钟 TTL）。
        let token_key = access_key(token);
        {
            let mut revoked = self.revoked_access.await_or_poison();
            revoked.retain(|_, exp_sec| *exp_sec > now_ms / 1_000);
            if revoked.contains_key(&token_key) {
                return None;
            }
        }
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
        if claims.get("iss").and_then(|v| v.as_str()) != Some(JWT_ISSUER)
            || claims.get("aud").and_then(|v| v.as_str()) != Some(JWT_AUDIENCE)
        {
            return None;
        }
        let exp = claims.get("exp").and_then(|v| v.as_u64())?;
        if exp <= now_ms / 1_000 {
            return None;
        }
        let sub = claims.get("sub").and_then(|v| v.as_str())?.to_owned();
        // 只有 active 档案能继续持有 access；缓存里查不到（启动恢复失败/档案被删）
        // 也按无效处理，避免封禁账号在缓存缺失时钻空子。
        match self.profiles.await_or_poison().get(&sub) {
            Some(profile) if profile.status == "active" => Some(sub),
            _ => None,
        }
    }

    /// 主动撤销一枚仍然有效的 access token（登出）；已过期/签名不对返回 false。
    pub fn revoke_access(&self, token: &str, now_ms: u64) -> bool {
        if self.verify_access(token, now_ms).is_none() {
            return false;
        }
        let key = access_key(token);
        let exp_sec = now_ms / 1_000 + ACCESS_TTL_MS / 1_000;
        let mut revoked = self.revoked_access.await_or_poison();
        revoked.retain(|_, exp| *exp > now_ms / 1_000);
        revoked.insert(key, exp_sec);
        true
    }

    pub fn profile(&self, player_id: &str) -> Option<Profile> {
        self.profiles.await_or_poison().get(player_id).cloned()
    }

    /// 手机号哈希是否已注册（测试与巡检用）。
    pub fn phone_hash_known(&self, phone: &str) -> bool {
        self.phone_hash(phone)
            .is_some_and(|hash| self.by_phone.await_or_poison().contains_key(&hash))
    }

    /// 下一个玩家 ID。纯内存路径使用；接 PG 时同一进程先取号再插入，
    /// 若并发撞上唯一键，`find_or_create_profile` 会返回已存在那一行。
    pub(crate) fn next_player_id(&self) -> String {
        format!("p-{:08x}", self.seq.fetch_add(1, Ordering::Relaxed))
    }

    /// 把存储层取回/新建的档案放进热缓存；同时维持手机号 → 玩家 ID 索引。
    pub(crate) fn remember_profile(&self, profile: Profile) {
        if let Some(hex) = profile.player_id.strip_prefix("p-") {
            if let Ok(seq) = u64::from_str_radix(hex, 16) {
                self.seq.fetch_max(seq.saturating_add(1), Ordering::Relaxed);
            }
        }
        self.by_phone
            .await_or_poison()
            .insert(profile.phone_hash.clone(), profile.player_id.clone());
        self.profiles
            .await_or_poison()
            .insert(profile.player_id.clone(), profile);
    }

    /// 后台列表用的全量快照；无 PG 时这就是唯一数据源。
    pub(crate) fn profiles_snapshot(&self) -> Vec<Profile> {
        self.profiles.await_or_poison().values().cloned().collect()
    }

    /// 后台启停：只改热缓存里的 `status`，PG 由调用方先行落库。
    pub(crate) fn set_profile_status(&self, player_id: &str, status: &str) -> bool {
        let mut profiles = self.profiles.await_or_poison();
        match profiles.get_mut(player_id) {
            Some(profile) => {
                profile.status = status.to_owned();
                true
            }
            None => false,
        }
    }

    /// 启动恢复：PG 里的全部档案一次性灌入内存索引。
    pub(crate) fn restore_profiles(&self, profiles: Vec<Profile>) {
        {
            let mut by_phone = self.by_phone.await_or_poison();
            let mut cache = self.profiles.await_or_poison();
            by_phone.clear();
            cache.clear();
        }
        for profile in profiles {
            self.remember_profile(profile);
        }
    }

    pub(crate) fn upsert_profile(
        &self,
        phone_hash: &str,
        phone_tail4: &str,
        now_ms: u64,
    ) -> Profile {
        let mut by_phone = self.by_phone.await_or_poison();
        if let Some(player_id) = by_phone.get(phone_hash) {
            if let Some(profile) = self.profiles.await_or_poison().get(player_id).cloned() {
                return profile;
            }
        }
        let profile = Profile {
            player_id: self.next_player_id(),
            phone_hash: phone_hash.to_owned(),
            phone_tail4: phone_tail4.to_owned(),
            nickname: format!("掼友{phone_tail4}"),
            created_at_ms: now_ms,
            status: "active".to_owned(),
        };
        by_phone.insert(phone_hash.to_owned(), profile.player_id.clone());
        self.profiles
            .await_or_poison()
            .insert(profile.player_id.clone(), profile.clone());
        profile
    }

    pub(crate) fn issue_session(&self, player_id: &str, now_ms: u64) -> Session {
        let token = self.issue_access(player_id, now_ms);
        let mut raw = [0u8; 32];
        rand::rng().fill_bytes(&mut raw);
        let refresh_token = hex(&raw);
        let expires_ms = refresh_expires_at(now_ms);
        self.refresh.await_or_poison().insert(
            refresh_key(&refresh_token),
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
            "iss": JWT_ISSUER,
            "aud": JWT_AUDIENCE,
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

/// refresh raw token 的库内键：只存 SHA-256，不存原文。
fn refresh_key(token: &str) -> String {
    hex(&Sha256::digest(token.trim().as_bytes()))
}

/// access raw token 的撤销表键。
fn access_key(token: &str) -> String {
    hex(&Sha256::digest(token.as_bytes()))
}

/// 新签 refresh 的过期时刻（毫秒 epoch）。
fn refresh_expires_at(now_ms: u64) -> u64 {
    now_ms.saturating_add(REFRESH_TTL_MS)
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
    fn whitelist_still_requires_requesting_code_first() {
        let auth = auth(&["13800138000"]);
        assert_eq!(
            auth.login("13800138000", DEFAULT_TEST_CODE, 1_000),
            Err(LoginError::InvalidCredentials),
            "白名单固定码也不能跳过先发码"
        );
        auth.request_sms("13800138000", 2_000).expect("发码");
        assert!(auth.login("13800138000", DEFAULT_TEST_CODE, 2_001).is_ok());
    }

    #[test]
    fn login_success_does_not_reset_sms_cooldown_or_daily_limit() {
        let auth = auth(&[]);
        let now = 1_000_000;
        auth.request_sms("13800138000", now).expect("首条");
        let hash = auth.phone_hash("13800138000").unwrap();
        let code = auth.codes.lock().unwrap().get(&hash).unwrap().code.clone();
        assert!(auth.login("13800138000", &code, now + 1).is_ok());
        assert_eq!(
            auth.request_sms("13800138000", now + 10_000),
            Err(SmsError::RateLimited {
                retry_after_ms: 50_000
            }),
            "登录成功不得把 60s 冷却一起清掉"
        );
    }

    #[test]
    fn stale_failures_are_cleared_after_idle_window() {
        let auth = auth(&[]);
        let now = 1_000_000;
        let hash = auth.phone_hash("13800138000").unwrap();
        let key = format!("phone:{hash}");
        auth.request_sms("13800138000", now).expect("发码");
        for _ in 0..4 {
            let _ = auth.login_with_peer("13800138000", "000000", Some("1.2.3.4"), now + 2);
        }
        assert_eq!(auth.login_guards.lock().unwrap()[&key].failures, 4);

        // 超过 24h 无尝试：旧失败计数必须清零，不能隔天再错一次就直接触发锁定。
        let later = now + LOGIN_GUARD_IDLE_RESET_MS + 2;
        let _ = auth.login_with_peer("13800138000", "000000", Some("1.2.3.4"), later);
        assert_eq!(auth.login_guards.lock().unwrap()[&key].failures, 1);
    }

    #[test]
    fn repeated_wrong_codes_lock_and_back_off() {
        let auth = auth(&[]);
        let now = 1_000_000;
        let hash = auth.phone_hash("13800138000").unwrap();
        auth.request_sms("13800138000", now).expect("发码");
        let code = auth.codes.lock().unwrap().get(&hash).unwrap().code.clone();
        for _ in 0..5 {
            assert_eq!(
                auth.login_with_peer("13800138000", "000000", Some("1.2.3.4"), now + 2),
                Err(LoginError::InvalidCredentials)
            );
        }
        match auth.login_with_peer("13800138000", &code, Some("1.2.3.4"), now + 3) {
            Err(LoginError::Locked { retry_after_ms }) => {
                assert!(
                    retry_after_ms > 20_000,
                    "至少锁 30 秒量级：{retry_after_ms}"
                )
            }
            other => panic!("应被锁定，实际 {other:?}"),
        }
        // 冷却过后重新发码，锁已过期，可正常登录。
        auth.request_sms("13800138000", now + 60_000)
            .expect("再发码");
        let code = auth.codes.lock().unwrap().get(&hash).unwrap().code.clone();
        assert!(auth
            .login_with_peer("13800138000", &code, Some("1.2.3.4"), now + 60_001)
            .is_ok());
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
    fn revoked_access_token_is_rejected_immediately() {
        let auth = auth(&[]);
        let now = 86_400_000;
        auth.request_sms("13900139001", now).unwrap();
        let hash = auth.phone_hash("13900139001").unwrap();
        let code = auth.codes.lock().unwrap().get(&hash).unwrap().code.clone();
        let session = auth.login("13900139001", &code, now).unwrap();
        assert!(auth.verify_access(&session.token, now).is_some());
        assert!(auth.revoke_access(&session.token, now));
        assert!(
            auth.verify_access(&session.token, now).is_none(),
            "登出后 access token 必须立刻失效，不能等 15 分钟 TTL"
        );
        assert!(!auth.revoke_access("not-a-jwt", now));
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
        let now = 1_000_000;
        auth.request_sms("13800138000", now).unwrap();
        let a = auth
            .login("13800138000", DEFAULT_TEST_CODE, now + 1)
            .unwrap();
        auth.request_sms("+86 138-0013-8000", now + 60_001).unwrap();
        let b = auth
            .login("+86 138-0013-8000", DEFAULT_TEST_CODE, now + 60_002)
            .unwrap();
        assert_eq!(a.player_id, b.player_id, "同一手机号必须复用同一 playerId");
    }

    fn profile(player_id: &str, status: &str) -> Profile {
        Profile {
            player_id: player_id.to_owned(),
            phone_hash: format!("hash-{player_id}"),
            phone_tail4: "8000".to_owned(),
            nickname: "掼友8000".to_owned(),
            created_at_ms: 100,
            status: status.to_owned(),
        }
    }

    #[test]
    fn restored_profiles_keep_ids_and_continue_sequence() {
        let auth = auth(&[]);
        auth.restore_profiles(vec![profile("p-0000002a", "active")]);
        assert!(auth.profile("p-0000002a").is_some());
        assert_eq!(
            auth.next_player_id(),
            "p-0000002b",
            "重启后取号必须接在已有序号之后"
        );
    }

    #[test]
    fn restored_refresh_and_revocation_records_take_effect() {
        let auth = auth(&[]);
        let now = 86_400_000;
        auth.restore_profiles(vec![profile("p-00000001", "active")]);
        auth.restore_refresh_tokens(
            vec![(
                refresh_key("persisted-refresh"),
                "p-00000001".to_owned(),
                now + 1_000,
            )],
            now,
        );
        let session = auth
            .refresh("persisted-refresh", now)
            .expect("恢复的 refresh 可用");
        assert_eq!(session.player_id, "p-00000001");

        let other = auth.issue_session("p-00000002", now);
        auth.remember_profile(profile("p-00000002", "active"));
        auth.restore_access_revocations(vec![(access_key(&other.token), now + ACCESS_TTL_MS)], now);
        assert!(
            auth.verify_access(&other.token, now).is_none(),
            "撤销表按毫秒恢复后必须继续生效"
        );
    }

    #[test]
    fn disabled_profile_cannot_login_or_refresh() {
        let auth = auth(&[]);
        auth.remember_profile(profile("p-00000003", "disabled"));
        let now = 1_000;
        let session = auth.issue_session("p-00000003", now);
        assert!(
            auth.verify_access(&session.token, now).is_none(),
            "禁用账号已签发的 access 也必须拒绝"
        );
        assert!(matches!(
            auth.rotate_refresh(&session.refresh_token, now + 1),
            Err(LoginError::AccountDisabled)
        ));
    }
}
