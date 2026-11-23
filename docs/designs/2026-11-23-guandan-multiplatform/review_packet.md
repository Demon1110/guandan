# 金鱼评审包（EGM）

## 1. 你在做什么

你是本次评审的金鱼：一个没有参与过任何前置讨论的评审者。你只能依据本包和下面
“允许阅读清单”里的文件形成判断。你没有看过设计者的对话，这是刻意的——请把文档
读不懂的地方当成文档的问题报出来，而不是替它补全。

## 2. 事实

| 项 | 值 |
| --- | --- |
| 设计标识 | 2026-11-23-guandan-multiplatform |
| 设计文档 | docs/designs/2026-11-23-guandan-multiplatform/design_draft.md |
| 对象类型 | technical |
| 是否核对代码 | 否（只评文档自身完整性） |
| 允许阅读文件数 | 2 |
| 生成时间 | 2026-11-23 15:26 |

## 3. 允许阅读清单（白名单，只有这些）

- `docs/designs/2026-11-23-guandan-multiplatform/design_draft.md`
- `docs/assets/license.md`

清单外文件、父对话、项目里其他文档一律不得读取。需要清单外的文件才能判断时，
在报告里写“需要额外文件：<路径>，理由：……”，并标记本次评审为 incomplete。

## 4. 找不到文件时

必需文件缺失或读不出来，报告里写 incomplete 并列出缺什么，不要凭印象给结论。

## 5. 输出契约

- 只读：不修改任何文件。
- 报告写到 reviews/round-01/。
- 报告开头必须有元信息表：评审者 / 步骤 / 范围 / 对象类型 / 只读 / 隔离模式 / 状态。
- 问题写在 `## Findings` 下，逐行一行一条，六列：Finding / 严重度 / 位置 / 问题 / 依据 / 建议。
- Finding 编号格式 `F{step}-NN`（NN 从 01 递增），本轮内不得重号。
- 严重度只用 Blocker / Major / Minor / Nit。

## 6. 硬约束

1. 不确定就标注“不确定 + 需要什么才能确定”，不要编。
2. 每条问题必须能被第三人按“位置 + 依据”复现，写不出位置或依据的不要写。
3. 不要提“建议补充文档/建议加强测试”这类无法验收的意见。
4. 不要复述文档内容充当评审结论。
5. 不评价文档作者，只评价设计。

## 7. 清单外引用（需人批准后才可读）

- `https://heroiclabs.com/opensource/`
- `https://heroiclabs.com/docs/nakama/getting-started/console/`
- `https://colyseus.io/`
- `https://docs.colyseus.io/`
- `https://ldt.indienova.com/u/levelplay/blogread/5363`
- `https://caisual.com/blog/colyseus-vs-playroom-vs-photon-vs-nakama`
- `https://heroiclabs.com/docs/nakama/guides/`

## 8. 缺失引用（文档提到但当前不存在）

- 无
