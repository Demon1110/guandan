# M1 覆盖率基线（分支统计）

统计时间：2026-11-25。工具链：`cargo-llvm-cov 0.9.2` + `llvm-tools-preview`（stable 与 nightly 均装）+ RustC nightly（`--branch` 只在 nightly 上可用）。

命令：

```powershell
pwsh -NoProfile -File scripts/coverage.ps1
# 等价于：
cargo +nightly llvm-cov --workspace --branch --summary-only `
  --ignore-filename-regex "(client-wasm|generated)"
```

排除口径：`crates/client-wasm/src/lib.rs`（wasm C ABI 导出层，host 测试进程执行不到，另有 native/wasm 双产物 `stateHash` gate）与 `crates/protocol/src/generated/*`（protobuf 生成代码）。除此之外的全部业务代码都计入。

## 总账

| 指标 | 覆盖 | 未覆盖 |
| --- | --- | --- |
| 行 | **86.01%**（6976 / 8111） | 1135 |
| 分支 | **72.07%**（924 / 1282） | 358 |
| 区域 | 86.33% | 1310 / 9584 |
| 函数 | 86.43% | 121 / 892 |

**未达 90% 分支目标**：按 90% 口径还需再覆盖约 **230 个分支**（1282 × 0.9 − 924 ≈ 230）。

## 分文件（按未覆盖分支数排序，前 9）

| 文件 | 行覆盖 | 分支覆盖 | 未覆盖分支 |
| --- | --- | --- | --- |
| `engine/src/patterns.rs` | 92.43% | 73.14% | 94 |
| `engine/src/ruleset/validate.rs` | 72.22% | 69.77% | 52 |
| `engine/src/engine.rs` | 94.17% | 77.00% | 46 |
| `table-runtime/src/room.rs` | 75.05% | 52.70% | 35 |
| `engine/src/recommend.rs` | 86.27% | 79.17% | 30 |
| `engine/src/ruleset/validate_deep.rs` | 75.91% | 66.67% | 28 |
| `engine/src/ruleset/model.rs` | 75.50% | 50.00% | 14 |
| `ai/src/view_scan.rs` | 92.74% | 61.11% | 14 |
| `engine/src/cases.rs` | 54.64% | 50.00% | 15 |

余下分散在 `ruleset/params.rs`（2）、`ai/src/l1.rs`（8）、`ai/src/shape.rs`（4）、`recovery.rs`（2）、`snapshot.rs`（1）、`view.rs`（2）、`hash.rs`（0）等。

## 缺口性质与补测顺序

1. **`patterns.rs`（94）**：牌型识别/比较的边界——空牌、超长连、跨花色同花顺、王混入组牌型、`compare` 的阶梯等号；可用表驱动一次扫全。
2. **`validate.rs` + `validate_deep.rs`（80）**：各类规则冲突的失败分支（`RS_*` finding 的每条构造）。
3. **`engine.rs`（46）**：相位守卫、`apply` 的非法动作分支、进贡/接风的异常组合。
4. **`room.rs`（35）**：`RoomError` 七个变体、超时/离线/场次终止/非人类提交/错误座位。
5. **`recommend.rs`（30）**：权重缺键、空手、单牌只剩王等退化输入。

估算：前 4 项覆盖后可到约 **85–88%** 分支；补齐最后 2 项才能到 90%。这部分是纯补测工作（不改实现），建议单独立一批评测任务并挂到 CI。

## 与 gate 的关系

覆盖率目前**不进 M1 硬 gate**（只在 CI 里出报告）。README 与 `docs/开发计划.md` 已按此口径记录；等分支覆盖到 90% 再改成阻断门禁。
