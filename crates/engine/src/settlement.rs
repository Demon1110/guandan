//! `SettleResult` 结算载体（§5.9 第 757 行、§5.12 视图编码六条、第二十四次变更）。
//!
//! - `result` 为 `bytes`，内容 canonical JSON，键序固定 `winner_team, ranks, points`；
//!   单行无空白、不写 `null`；新增结算字段只能追加在 `points` 之后，不改序不改名。
//! - `ranks` 按名次 1→n 列 seat；`points` 为整数。
//! - `reason` 为可选 ASCII 字符串，取值必须来自 §5.7 错误码全表；正常结算不出现。
//! - `rulesetVersion` 只作身份键（三元组相等判定），字符集 `[A-Za-z0-9._-]`、长度 1–32，
//!   禁止按字典序比大小。
//!
//! 纪律同 §5.12：序列化自写 canonical 编码器，不引入 `serde_json::Value`。

use crate::contract::ErrorCode;

/// 结算结果内部形状（`round_record.result` 与 `SettleResult.result` 同构）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settlement {
    /// 头游所在队队序号。
    pub winner_team: u8,
    /// 按名次 1→n 排列的 seat 数组。
    pub ranks: Vec<u8>,
    /// 整数分值；本批 rank-combo 基线不计分，取 0（score-diff 分值属后续批次）。
    pub points: i64,
}

impl Settlement {
    /// 编码为 `result` 字节：canonical JSON，键序 `winner_team, ranks, points`。
    pub fn encode_result_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.push(b'{');
        out.extend_from_slice(b"\"winner_team\":");
        out.extend_from_slice(self.winner_team.to_string().as_bytes());
        out.push(b',');
        out.extend_from_slice(b"\"ranks\":[");
        for (i, &seat) in self.ranks.iter().enumerate() {
            if i > 0 {
                out.push(b',');
            }
            out.extend_from_slice(seat.to_string().as_bytes());
        }
        out.push(b']');
        out.push(b',');
        out.extend_from_slice(b"\"points\":");
        out.extend_from_slice(self.points.to_string().as_bytes());
        out.push(b'}');
        out
    }
}

/// 结算广播帧（引擎侧投影；`roomSeq` / `levelsAfter` / `nextDealInMs` 由房间层承载）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SettleResult {
    /// 刚结束的这一局号（§6.2：`deal` 时才 +1）。
    pub round_no: u16,
    /// 结算结果 canonical JSON bytes。
    pub result: Settlement,
    /// 异常结算原因；正常结算为 `None`（不写空串、不写 null）。
    pub reason: Option<String>,
    /// 房间锁定的 RuleSet 身份版本串。
    pub ruleset_version: String,
}

/// `reason` 是否在 §5.7 错误码全表内（ASCII 字面值逐条比对；表外即服务端违约）。
pub fn is_valid_reason(reason: &str) -> bool {
    ErrorCode::all()
        .iter()
        .any(|code| code.code_str() == reason)
}

/// `rulesetVersion` 身份键校验：字符集 `[A-Za-z0-9._-]`、长度 1–32。
///
/// 只用于三元组相等判定；这里不做任何版本大小比较（§5.12 第 764 行）。
pub fn valid_ruleset_version(version: &str) -> bool {
    let len = version.len();
    (1..=32).contains(&len)
        && version.bytes().all(|b| {
            matches!(
                b,
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'.' | b'_' | b'-'
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn result_bytes_key_order_and_no_whitespace() {
        let s = Settlement {
            winner_team: 1,
            ranks: vec![1, 3, 0, 2],
            points: 0,
        };
        let bytes = s.encode_result_bytes();
        assert_eq!(bytes, br#"{"winner_team":1,"ranks":[1,3,0,2],"points":0}"#);
    }

    #[test]
    fn reason_table_all_codes_valid_and_unknown_rejected() {
        for code in ErrorCode::all() {
            assert!(
                is_valid_reason(code.code_str()),
                "{} 应在表内",
                code.code_str()
            );
        }
        assert!(!is_valid_reason("RS_NOT_A_CODE"));
        assert!(!is_valid_reason("rs_schema_invalid"));
        assert!(!is_valid_reason(""));
    }

    #[test]
    fn ruleset_version_charset_and_length() {
        assert!(valid_ruleset_version("guandan-classic-1.0.0"));
        assert!(valid_ruleset_version("m0-minimal-1.0.0"));
        assert!(!valid_ruleset_version(""));
        assert!(!valid_ruleset_version("含中文"));
        assert!(!valid_ruleset_version("bad/version"));
        assert!(!valid_ruleset_version("space in"));
        let long = "a".repeat(33);
        assert!(!valid_ruleset_version(&long));
        assert!(valid_ruleset_version(&"a".repeat(32)));
    }
}
