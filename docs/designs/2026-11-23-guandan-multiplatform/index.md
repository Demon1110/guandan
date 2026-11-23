# 掼蛋多端游戏框架

| 项 | 内容 |
| --- | --- |
| 设计标识 | 2026-11-23-guandan-multiplatform |
| 对象类型 | technical |
| 当前状态 | 相 4 裁决收敛中：round-01 172 条、round-02 107 条已裁决 settled 并入草案（21 条关键决策 D1–D21 + 10 条未决 U1–U12）；round-03 评审完成 91 条（Blocker 6 / Major 50 / Minor 33 / Nit 2），本轮裁决 PROPOSE FIX 全部接受、正文修订已并入；收敛末尾连续 clean 0 轮（要求 ≥2），已用 3/5 轮 |
| 唯一实现依据 | design_draft.md（评审收敛后） |
| 已废弃文档 | `docs/开发计划.md`（v1 初版规划，基于 Java+Node 与 TS 引擎，已被本设计取代；勿再引用其技术选型） |

## 产物

| 文件 | 作用 |
| --- | --- |
| concepts_brief.md | 概念简报：问题、目标、约束、否决方案、假设 |
| design_draft.md | 设计草案：唯一实现依据 |
| review_packet.md | 金鱼评审包：只读清单与评审规则（每轮重新生成） |
| manifest.json | 评审清单与隔离方式记录 |
| reviews/round-01/ | 第 1 轮五步评审报告（172 条发现） |
| reviews/round-02/ | 第 2 轮（修订后复核，判定是否 clean） |
| decision_record.md | 裁决台账与收敛记录 |
| handoff_boundary.md | 交接边界：实现阶段允许写入的范围 |
