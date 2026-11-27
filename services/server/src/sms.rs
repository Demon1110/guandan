//! 短信发送器：开发用日志通道 + 云片（Yunpian）真实通道。
//!
//! 真实发送放在独立 `std::thread` 里消费队列：HTTP 请求耗时 100ms 级，
//! 不能让发码接口（持着验证码表锁）跟着一起阻塞，也不能在异步运行时里做阻塞 IO。
//!
//! 云片两种发送方式（同一个 apikey）：
//! - 智能匹配已报备模板：`POST /v2/sms/single_send.json`，`text` 带签名与正文；
//! - 指定模板：`POST /v2/sms/tpl_single_send.json`，传 `tpl_id` 与 `tpl_value`。
//!
//! 配置见 `config.toml` 的 `[sms]` 段；密钥不落代码。

use std::sync::mpsc::{self, Sender};
use std::thread;
use std::time::Duration;

use crate::auth::{LogSmsSender, SmsSender};
use crate::config::SmsConfig;

/// 云片「智能匹配模板」接口。
const YUNPIAN_SINGLE_SEND: &str = "/v2/sms/single_send.json";
/// 云片「指定模板」接口。
const YUNPIAN_TPL_SINGLE_SEND: &str = "/v2/sms/tpl_single_send.json";

/// 按配置选发送器：只有「云片 + 有 apikey + 真发开关打开」才走真实通道，
/// 其余一律日志通道（挡板）。挡板状态在启动日志里明确写出来。
pub fn build_sender(cfg: &SmsConfig) -> Box<dyn SmsSender> {
    if cfg.provider == "yunpian" && !cfg.api_key.trim().is_empty() && cfg.send_enabled {
        match YunpianSender::spawn(cfg) {
            Ok(sender) => {
                println!(
                    "[sms] 云片通道已启用（{}，模板 {}，真发白名单 {} 个号）",
                    cfg.endpoint,
                    if cfg.template_id == 0 {
                        "智能匹配".to_owned()
                    } else {
                        cfg.template_id.to_string()
                    },
                    cfg.send_whitelist.len()
                );
                Box::new(sender)
            }
            Err(err) => {
                eprintln!("[sms] 云片发送线程起不来（退回日志通道）：{err}");
                Box::new(LogSmsSender)
            }
        }
    } else {
        // 挡板：验证码照常生成、照常能登录（白名单号），但一条真实短信都不发。
        println!("[sms] 挡板开启：不调用云片，验证码只打进服务端日志");
        Box::new(LogSmsSender)
    }
}

/// 云片发送器：`send` 只入队，真正的 HTTP 调用在后台线程。
pub struct YunpianSender {
    tx: Sender<Job>,
    /// 允许真实接收短信的号码（已规范化 11 位）；空 = 谁都不发。
    allowed: Vec<String>,
}

struct Job {
    phone: String,
    code: String,
}

impl YunpianSender {
    fn spawn(cfg: &SmsConfig) -> std::io::Result<Self> {
        let (tx, rx) = mpsc::channel::<Job>();
        let allowed: Vec<String> = cfg
            .send_whitelist
            .iter()
            .filter_map(|phone| normalize_digits(phone))
            .collect();
        let cfg = cfg.clone();
        thread::Builder::new()
            .name("gf-sms-yunpian".to_owned())
            .spawn(move || worker(cfg, rx))?;
        Ok(Self { tx, allowed })
    }

    /// 只入队；号码不在发送白名单就直接拦下（防误发给无关用户）。
    fn enqueue(&self, phone: &str, code: &str) {
        if !is_send_allowed(&self.allowed, phone) {
            eprintln!(
                "[sms] 挡板拦截：尾号 {} 不在 send_whitelist，拒绝真实发送",
                tail4(phone)
            );
            return;
        }
        let job = Job {
            phone: phone.to_owned(),
            code: code.to_owned(),
        };
        if self.tx.send(job).is_err() {
            eprintln!("[sms] 云片队列不可用，尾号 {} 的验证码未发出", tail4(phone));
        }
    }
}

/// 真实发送白名单判定：号码规范化成 11 位数字再比，`+86` 前缀也能匹配。
pub fn is_send_allowed(allowed: &[String], phone: &str) -> bool {
    match normalize_digits(phone) {
        Some(digits) => allowed
            .iter()
            .any(|item| normalize_digits(item).as_deref() == Some(digits.as_str())),
        None => false,
    }
}

fn normalize_digits(raw: &str) -> Option<String> {
    let digits: String = raw.chars().filter(|c| c.is_ascii_digit()).collect();
    let digits = digits
        .strip_prefix("86")
        .filter(|rest| rest.len() == 11)
        .unwrap_or(&digits);
    if digits.len() == 11 && digits.starts_with('1') {
        Some(digits.to_owned())
    } else {
        None
    }
}

impl SmsSender for YunpianSender {
    fn send(&self, phone: &str, code: &str) {
        self.enqueue(phone, code);
    }
}

fn worker(cfg: SmsConfig, rx: mpsc::Receiver<Job>) {
    let client = match reqwest::blocking::Client::builder()
        .timeout(Duration::from_millis(cfg.timeout_ms))
        .build()
    {
        Ok(client) => client,
        Err(err) => {
            eprintln!("[sms] HTTP 客户端构造失败，云片通道停用：{err}");
            return;
        }
    };
    for job in rx {
        let (url, form) = yunpian_request(&cfg, &job.phone, &job.code);
        match client.post(&url).form(&form).send() {
            Ok(response) => {
                let status = response.status();
                let body = response.text().unwrap_or_default();
                match parse_yunpian_reply(&body) {
                    Ok(()) => println!("[sms] 云片已受理：尾号 {}", tail4(&job.phone)),
                    Err(detail) => eprintln!(
                        "[sms] 云片拒发：尾号 {}，HTTP {status}，{detail}",
                        tail4(&job.phone)
                    ),
                }
            }
            Err(err) => eprintln!("[sms] 云片请求失败：尾号 {}，{err}", tail4(&job.phone)),
        }
    }
}

/// 拼出 `(完整 URL, 表单字段)`；纯函数，便于单测。
pub fn yunpian_request(
    cfg: &SmsConfig,
    phone: &str,
    code: &str,
) -> (String, Vec<(String, String)>) {
    let path = if cfg.template_id == 0 {
        YUNPIAN_SINGLE_SEND
    } else {
        YUNPIAN_TPL_SINGLE_SEND
    };
    let url = format!("{}{path}", cfg.endpoint.trim_end_matches('/'));
    let mut form = vec![
        ("apikey".to_owned(), cfg.api_key.clone()),
        ("mobile".to_owned(), phone.to_owned()),
    ];
    if cfg.template_id == 0 {
        form.push(("text".to_owned(), yunpian_text(cfg, code)));
    } else {
        form.push(("tpl_id".to_owned(), cfg.template_id.to_string()));
        form.push((
            "tpl_value".to_owned(),
            format!("{}={code}", cfg.template_param),
        ));
    }
    (url, form)
}

/// 智能匹配模式的正文：`【签名】正文`，正文里的 `{code}` 换成验证码。
pub fn yunpian_text(cfg: &SmsConfig, code: &str) -> String {
    let body = cfg.text_template.replace("{code}", code);
    format!("【{}】{body}", cfg.sign.trim())
}

/// 云片回执：`{"code":0,"msg":"OK",...}`；`code != 0` 视为失败。
pub fn parse_yunpian_reply(body: &str) -> Result<(), String> {
    let value: serde_json::Value =
        serde_json::from_str(body).map_err(|err| format!("回执不是 JSON：{err}"))?;
    // 云片正常回 `code: 0`（整数），个别网关会包成字符串，两种都认。
    let code = value
        .get("code")
        .and_then(|v| v.as_i64().or_else(|| v.as_str()?.parse().ok()))
        .unwrap_or(-1);
    if code == 0 {
        return Ok(());
    }
    // 云片 msg 偶尔带号码，这里只报错误码与精简 msg，不原样回显整段回执。
    let msg = value
        .get("msg")
        .and_then(|v| v.as_str())
        .unwrap_or("未知错误");
    Err(format!("code={code}，msg={msg}"))
}

fn tail4(phone: &str) -> String {
    let digits: String = phone.chars().filter(|c| c.is_ascii_digit()).collect();
    let start = digits.len().saturating_sub(4);
    digits[start..].to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sms(template_id: u64, param: &str) -> SmsConfig {
        SmsConfig {
            provider: "yunpian".to_owned(),
            api_key: "test-key".to_owned(),
            sign: "掼蛋".to_owned(),
            text_template: "您的验证码是{code}，5分钟内有效。".to_owned(),
            template_id,
            template_param: param.to_owned(),
            endpoint: "https://sms.yunpian.com/".to_owned(),
            timeout_ms: 5_000,
            send_enabled: true,
            send_whitelist: vec!["13800138000".to_owned()],
            test_whitelist: Vec::new(),
            test_code: "666666".to_owned(),
        }
    }

    #[test]
    fn text_mode_hits_single_send_with_signed_body() {
        let cfg = sms(0, "#code");
        let (url, form) = yunpian_request(&cfg, "13800138000", "654321");
        assert_eq!(url, "https://sms.yunpian.com/v2/sms/single_send.json");
        let text = form
            .iter()
            .find(|(key, _)| key == "text")
            .map(|(_, value)| value.clone())
            .expect("正文必填");
        assert_eq!(text, "【掼蛋】您的验证码是654321，5分钟内有效。");
        assert!(form.iter().all(|(key, _)| key != "tpl_id"));
    }

    #[test]
    fn template_mode_hits_tpl_endpoint_with_param_pair() {
        let cfg = sms(376_861, "#code#");
        let (url, form) = yunpian_request(&cfg, "13800138000", "654321");
        assert_eq!(url, "https://sms.yunpian.com/v2/sms/tpl_single_send.json");
        assert_eq!(
            form.iter()
                .find(|(key, _)| key == "tpl_id")
                .map(|(_, value)| value.as_str()),
            Some("376861")
        );
        assert_eq!(
            form.iter()
                .find(|(key, _)| key == "tpl_value")
                .map(|(_, value)| value.as_str()),
            Some("#code#=654321")
        );
        assert!(form.iter().all(|(key, _)| key != "text"));
    }

    #[test]
    fn reply_code_zero_is_ok_and_others_carry_detail() {
        assert!(parse_yunpian_reply(r#"{"code":0,"msg":"OK","count":1}"#).is_ok());
        assert!(
            parse_yunpian_reply(r#"{"code":"0","msg":"OK"}"#).is_ok(),
            "有些网关把 code 包成字符串"
        );
        let err = parse_yunpian_reply(r#"{"code":22,"msg":"参数缺失"}"#).expect_err("非 0 要报错");
        assert!(err.contains("22") && err.contains("参数缺失"));
        assert!(parse_yunpian_reply("not json").is_err());
    }

    #[test]
    fn default_provider_stays_on_log_channel() {
        let cfg = SmsConfig::default();
        assert_eq!(cfg.provider, "log");
        assert!(!cfg.send_enabled, "默认必须是挡板，不能真发");
        assert!(cfg.send_whitelist.is_empty());
    }

    #[test]
    fn send_whitelist_blocks_everyone_else() {
        let allowed = vec!["15330213553".to_owned()];
        assert!(is_send_allowed(&allowed, "15330213553"));
        assert!(is_send_allowed(&allowed, "+86 153-3021-3553"));
        assert!(!is_send_allowed(&allowed, "13800138000"), "别的号必须拦下");
        assert!(!is_send_allowed(&[], "15330213553"), "空白名单 = 谁都不发");
        assert!(!is_send_allowed(&allowed, "12345"), "畸形号码不发");
    }
}
