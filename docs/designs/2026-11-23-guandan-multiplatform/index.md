# 掼蛋多端游戏框架

| 项 | 内容 |
| --- | --- |
| 设计标识 | 2026-11-23-guandan-multiplatform |
| 对象类型 | technical |
| 当前状态 | 相 4 裁决已应用 + round-06-lite 轻量轮已跑（2026-11-23）：六轮共 555 条 finding（172 / 107 / 91 / 95 / 79 / 11）全部有裁决。round-05 79 条按 21 条裁决落地（53 条并入改写正文、26 条 Minor/Nit 按 `DECLINE` 记账）；round-06-lite 11 条 = 0 Blocker / 3 Major / 6 Minor / 2 Nit，3 条 Major 均属**补缺类**已修（F9-01 `§5.1` `tribute` 类型列补 `return`、F9-02 `flow.deal` 补 `zone`、F9-03 顶层补 `level` 段），8 条 Minor/Nit 裁决 `DECLINE`（其修正已随同批落地、不再回改）。**六轮 `clean` 全部为「否」**；收敛按用户裁决通过而非工具口径——原始要求「末尾连续两轮无 B/M、总轮次 ≤5」，用户两次放宽：其一「此前各轮 B/M 全部 `settled` + 轻量轮无新 B/M」，其二「补缺类 Major 修完即视为通过」。`egm_kit.py check` 仍报「收敛条件未达成：需要连续两轮无 Blocker 且无 Major」，如实保留未做假收敛。草案 1565 行（§12.2 含第十次变更，以及 2026-11-24 的「第十一次变更（M0 实现期口径对齐）」8 条——**全部是暂定口径、待用户逐条追认**，见 `decision_record.md` 未决问题第 9 行）；工期基线 34～38 → **40～44 周**（+6）。`handoff_boundary.md` 已签署（2026-11-23，用户本人，见该文件签字栏），**边界生效、实现可开工**；实现期发现设计缺陷只能回 EGM 走变更。**M0 已开工（2026-11-24）**：`§2.4` 四接口按最终形状锁定（`legal_actions` 为 `Result`）、L1 校验器与 `m0-minimal`/`guandan-classic` 两份夹具、三份非法 RuleSet 反例、`stateHash` 九段拼接与 canonical view 编码已落码并入仓，定值向量 `9e0b756d…cc72`（129 字节）由实现真算对上 |
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
| reviews/round-03/ · round-04/ · round-05/ | 第 3–5 轮五步评审报告（91 / 95 / 79 条发现，均已裁决 `settled`） |
| reviews/round-06-lite/ | 第 6 轮轻量验证（仅 Step9 就绪度单步，不重跑 Step5–8）：11 条发现 = 0 Blocker / 3 Major / 6 Minor / 2 Nit，结论「暂不可开工」，3 条 Major 已按补缺类修完 |
| decision_record.md | 裁决台账与收敛记录（含 round-06-lite 11 行与收敛口径补充裁决） |
| handoff_boundary.md | 交接边界：允许写入路径、禁止触碰、完成判据、8 条残余风险；已签署（2026-11-23，用户本人），边界生效 |
