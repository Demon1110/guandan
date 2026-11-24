//! M0 确定性用例：native 与 wasm 编译同一份代码（§2.4 gate 第 1 项）。
//!
//! 引擎内不做文件 I/O（§3.3 约束 2）：夹具 YAML 由 `include_str!` 在编译期嵌入，
//! 双端读到的是同一份字节串。

use crate::contract::{Action, Engine, State};
use crate::engine::{M0Engine, PHASE_SETTLE};
use crate::hash::{state_hash, CanonicalState};
use crate::ruleset::from_yaml;

/// 固定种子（§5.13 RNG 唯一来源；N7 取 BLAKE3(seed‖counter) 前 8 字节）。
pub const M0_SEED: u64 = 0x0D15EA5E_00000001;

const M0_RULESET_YAML: &str = include_str!("../tests/fixtures/m0-minimal/ruleset.yaml");

/// 单步动作日志：seat / kind / cards（可逐条复现）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActionLogEntry {
    pub seat: u8,
    pub kind: String,
    pub cards: Vec<u16>,
}

/// 双端比对产物：最终状态、32 字节 stateHash、动作日志。
#[derive(Clone, Debug)]
pub struct GoldenOutcome {
    pub state: State,
    pub state_hash: [u8; 32],
    pub actions: Vec<ActionLogEntry>,
}

pub fn m0_ruleset_yaml() -> &'static str {
    M0_RULESET_YAML
}

pub fn build_engine() -> Result<M0Engine, String> {
    let body = from_yaml(M0_RULESET_YAML).map_err(|e| e.to_string())?;
    Ok(M0Engine { ruleset: body })
}

/// 固定策略：每步取 `legal_actions` 排序后的第一个（最小合法单张）；
/// 无合法动作则停止（m0 路径下 Pass 只在跟牌阶段出现）。跑到 `settle` 为止。
/// 策略固定即序列固定，日志可逐条复现。
pub fn run_m0_golden() -> Result<GoldenOutcome, String> {
    let engine = build_engine()?;
    let mut state = engine.deal(M0_SEED).map_err(|e| e.to_string())?;
    let mut actions = Vec::new();
    let mut steps = 0usize;

    loop {
        if state.phase == PHASE_SETTLE {
            break;
        }
        let legal = engine.legal_actions(&state).map_err(|e| e.to_string())?;
        if legal.is_empty() {
            break;
        }
        let action = legal.into_iter().next().unwrap();
        let log = match &action {
            Action::Play { cards } => ActionLogEntry {
                seat: state.turn_seat,
                kind: "play".to_owned(),
                cards: cards.clone(),
            },
            Action::Pass => ActionLogEntry {
                seat: state.turn_seat,
                kind: "pass".to_owned(),
                cards: Vec::new(),
            },
        };
        state = engine.apply(state, action).map_err(|e| e.to_string())?;
        actions.push(log);
        steps += 1;
        if steps > 10_000 {
            return Err("m0 golden 超出安全步数上限，疑似死循环".to_owned());
        }
    }

    let canonical = CanonicalState::from(&state);
    let state_hash = state_hash(&canonical);
    Ok(GoldenOutcome {
        state,
        state_hash,
        actions,
    })
}

// ---------- wasm C ABI 导出（仅 wasm32 目标） ----------

#[cfg(target_arch = "wasm32")]
#[no_mangle]
pub extern "C" fn gf_alloc(len: usize) -> *mut u8 {
    let mut buf = Vec::<u8>::with_capacity(len);
    let ptr = buf.as_mut_ptr();
    std::mem::forget(buf);
    ptr
}

#[cfg(target_arch = "wasm32")]
#[no_mangle]
pub extern "C" fn gf_free(ptr: *mut u8, len: usize) {
    if ptr.is_null() {
        return;
    }
    // 重建 Vec 释放；容量按 len 还原（本链路只释放定长 32 字节缓冲）。
    unsafe {
        let _ = Vec::<u8>::from_raw_parts(ptr, len, len);
    }
}

/// 跑完 m0 golden，返回 32 字节 stateHash 堆缓冲指针；宿主按指针读 32 字节。
#[cfg(target_arch = "wasm32")]
#[no_mangle]
pub extern "C" fn gf_m0_golden_hash() -> *mut u8 {
    let outcome = run_m0_golden().expect("m0 golden 运行失败");
    let mut buf = Vec::with_capacity(32);
    buf.extend_from_slice(&outcome.state_hash);
    let ptr = buf.as_mut_ptr();
    std::mem::forget(buf);
    ptr
}
