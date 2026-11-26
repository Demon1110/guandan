//! 服务端配置：默认值可跑本机联调，生产由环境变量覆盖。

use std::env;

/// 服务端运行参数（不含规则集：规则集由房间建桌时锁定）。
#[derive(Clone, Debug)]
pub struct ServerConfig {
    /// 监听地址，如 `127.0.0.1:8080`。
    pub bind: String,
    /// 客户端版本下限（semver；空串表示不校验，本机联调默认如此）。
    pub min_client_version: String,
    /// 房间主循环节拍（毫秒）。20Hz 足够覆盖 30s 行动窗口与 90s 宽限的判定精度。
    pub tick_ms: u64,
    /// 结算后自动开下一局的间隔（毫秒）。
    ///
    /// TODO(M2-OPEN-01)：联机「再来一局」触发方未裁定（会动 `actions.kind` 闭集），
    /// 当前按服务端定时续局，待裁定后改为客户端触发。
    pub next_deal_ms: u64,
    /// 心跳超时：超过该时长无任何消息即判掉线（§5.9：客户端 15s 一次、30s 判掉线）。
    pub heartbeat_timeout_ms: u64,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            bind: "127.0.0.1:8080".to_owned(),
            min_client_version: String::new(),
            tick_ms: 50,
            next_deal_ms: 5_000,
            heartbeat_timeout_ms: 30_000,
        }
    }
}

impl ServerConfig {
    /// 从环境变量读取（缺省回落默认值）。
    ///
    /// - `GF_BIND`
    /// - `GF_MIN_CLIENT_VERSION`
    /// - `GF_TICK_MS`
    /// - `GF_NEXT_DEAL_MS`
    /// - `GF_HEARTBEAT_TIMEOUT_MS`
    pub fn from_env() -> Self {
        let mut cfg = Self::default();
        if let Ok(v) = env::var("GF_BIND") {
            cfg.bind = v;
        }
        if let Ok(v) = env::var("GF_MIN_CLIENT_VERSION") {
            cfg.min_client_version = v;
        }
        if let Some(v) = env_u64("GF_TICK_MS") {
            cfg.tick_ms = v.max(5);
        }
        if let Some(v) = env_u64("GF_NEXT_DEAL_MS") {
            cfg.next_deal_ms = v;
        }
        if let Some(v) = env_u64("GF_HEARTBEAT_TIMEOUT_MS") {
            cfg.heartbeat_timeout_ms = v.max(1_000);
        }
        cfg
    }

    /// 客户端版本是否满足下限（§5.16）。下限为空表示不校验。
    pub fn accepts_version(&self, client_version: &str) -> bool {
        if self.min_client_version.trim().is_empty() {
            return true;
        }
        match (parse_version(&self.min_client_version), parse_version(client_version)) {
            (Some(min), Some(v)) => v >= min,
            // 下限写了但客户端版本解析不出来：按 CLIENT_TOO_OLD 处理（失败闭合）。
            _ => false,
        }
    }
}

fn env_u64(name: &str) -> Option<u64> {
    env::var(name).ok()?.trim().parse::<u64>().ok()
}

/// 解析 `MAJOR.MINOR.PATCH`；缺段按 0 补齐，多段只取前三段。
pub fn parse_version(text: &str) -> Option<(u64, u64, u64)> {
    let core = text.trim().split(['-', '+']).next()?;
    let mut parts = core.split('.');
    let major = parts.next()?.parse::<u64>().ok()?;
    let minor = parts.next().unwrap_or("0").parse::<u64>().ok()?;
    let patch = parts.next().unwrap_or("0").parse::<u64>().ok()?;
    Some((major, minor, patch))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_gate_defaults_to_open() {
        let cfg = ServerConfig::default();
        assert!(cfg.accepts_version(""));
        assert!(cfg.accepts_version("0.0.1"));
    }

    #[test]
    fn version_gate_orders_by_numbers_not_lexicographic() {
        let cfg = ServerConfig {
            min_client_version: "1.10.0".to_owned(),
            ..ServerConfig::default()
        };
        assert!(cfg.accepts_version("1.10.0"));
        assert!(cfg.accepts_version("1.11.2"));
        assert!(!cfg.accepts_version("1.9.9"), "字典序会误判 1.9 > 1.10");
        assert!(!cfg.accepts_version("not-a-version"), "解析失败必须失败闭合");
    }

    #[test]
    fn version_parser_tolerates_short_and_suffixed_forms() {
        assert_eq!(parse_version("2"), Some((2, 0, 0)));
        assert_eq!(parse_version("2.3"), Some((2, 3, 0)));
        assert_eq!(parse_version("2.3.4-beta.1"), Some((2, 3, 4)));
    }
}
