# Step9 就绪度评审报告

| 字段 | 取值 |
| --- | --- |
| 评审者 | egm-readiness + egm-codebase_details + egm-style |
| 步骤 | 9 |
| 范围 | DetailedRange（契约字段、失败路径、迁移、测试的可编码性） |
| 对象类型 | technical |
| 只读 | 是 |
| 隔离模式 | fresh-subagent-query-leak（平台透传用户原始请求为“继续”，不含实质设计信息，属已知 L2 泄漏，结论据此降权） |
| 状态 | complete |

口径说明：code_check 记 N/A——本仓库为空仓库、无任何源码，未做代码签名核对，本报告只评设计是否足以被编码。
评审依据仅来自 design_draft.md 与 docs/assets/license.md；review_packet.md 第 7 节列出的外部 URL 未抓取。
未读取 reviews\ 目录下其他评审者的任何报告。

## Findings

| Finding | 严重度 | 位置 | 问题 | 依据 | 建议 |
| --- | --- | --- | --- | --- | --- |
| F9-01 | Major | §5.9 牌桌协议表 L549-L560；§4.5 账号/匹配行 L280-L282 | 玩家侧账号、短信验证、token 刷新、匹配队列、大厅列表的接口/消息契约全文缺失，工程师无法直接落 proto 与服务端路由 | §5.9 仅含 JoinRoom、Reconnect、SubmitAction、Heartbeat 四条 C→S 与 RoomSnapshot、RoomDelta、ActionAck、PhaseChange、SettleResult、TrusteeState 六条 S→C；全文未定义发送验证码、验证码登录/登出、access/refresh token 刷新、加入/退出匹配队列、匹配成功推送、大厅玩法或房间列表的消息名、字段与方向。§4.5 L280-L282 仅描述行为（“手机号 + 验证码 → 短期 JWT + 刷新 token”“required/desired/optional 三段匹配”），未给契约 | 补一张玩家侧 API/消息契约表，逐接口给出方向、路径或消息名、字段名、字段类型、错误码；至少覆盖：发送短信验证码、验证码登录、刷新 token、加入匹配、取消匹配、匹配成功/座位分配推送、大厅玩法列表；并明确 JoinRoom 省略 seat? 时座位由哪条消息分配 |
| F9-02 | Major | §5.9 SubmitAction 行 L553、RoomDelta 行 L556；§5.1 actions 行 L360 | SubmitAction.payload 与 RoomDelta.events[] 均未给分类型 schema，出牌/过牌/声明等动作的载荷字段无法直接编码 | 协议表 payload 仅写 `payload`，未按 kind（play/pass/claim/declare，见 L360）拆分字段：出牌载荷的牌 id 字段名与类型、pass 的空载荷、claim/declare 的载荷结构全文未定义；events[] 未列事件变体名与每事件字段。对比 §6.5 L741 的 ActionRecord 有字段表，广播事件结构全文无对应定义；RoomDelta 的 `seq` 也未说明是座位水位 seq 还是全局 roomSeq（§6.2 L703 区分了两者） | 在 §5.9 补 payload 联合结构：按 kind 列字段名/类型（牌 id 类型对齐 §5.12 L612 的 u16）；补 RoomDelta 事件枚举及每事件字段表；明确 RoomDelta.seq 取全局 roomSeq，并与座位水位字段分开命名 |
| F9-03 | Major | §5.7 错误码表 L492-L507；引用处 §5.9 L551、§8.6 L953 | 两条被正文引用的错误码未进错误码目录，触发条件与处置未定义 | §5.9 JoinRoom 行写 “clientVersion 低于 minClientVersion 返回 CLIENT_TOO_OLD”；§8.6 写 “panic 转为该动作失败并记 RS_INTERNAL_PANIC”；但 §5.7 表内无 CLIENT_TOO_OLD、无 RS_INTERNAL_PANIC 两条，其 HTTP/协议状态、客户端处置、指标归属均缺失 | 在 §5.7 补入 CLIENT_TOO_OLD 与 RS_INTERNAL_PANIC：分别写明触发条件、返回的 HTTP 状态或协议处置、客户端动作（跳下载/置动作失败）与归属指标 |
| F9-04 | Major | §5.9 Heartbeat 行 L554；§6.2 心跳条 L700 | 心跳方向与离线判定阈值两处互相冲突，实现者无法判断采用哪一套 | L554：Heartbeat 为 C→S，“15 秒一次，3 次丢失判掉线”；L700：“服务端 15 秒一次 ping，客户端 20 秒内无 pong 判离线”。一处为客户端主动上报、丢失 3 次（约 45 秒）判离线；另一处为服务端发起 ping、20 秒无 pong 判离线，发起方向与超时阈值均不一致 | 定死单一心跳模型：发起方（C→S 还是 S→C）、间隔秒数、判离线的连续丢失次数或秒数阈值、双向超时动作；§5.9 与 §6.2 改为引用同一组参数，不再各写一套 |
| F9-05 | Major | §5.1 zones 行 L352；§5.1 YAML 示例 L374-L376；§5.4 L2 行 L446 | zones.capacity 的口径（座位级还是房间级）未定义，且示例数值违反自身守恒式，L2 牌数守恒校验无法直接落地 | L352 不变量写 “deck.ranks × deck.suits × deck.copies = Σ private.capacity + Σ shared.capacity”；示例 L375-L376 为 hand(private, 27)、discard(shared, 999)，Σ=1026≠108（2 副 108 张，见 L570/L641）；文档也未说明 4 个座位的 hand 是由同一 zone 按座位实例化还是需写 4 条 zone，capacity 27 是单座位容量还是房间总量无法判定 | 明确 capacity 口径：若为座位级容量，写出 zone 按座位实例化的规则与守恒式的求和方式（per-seat × seat count）；若为房间总量，把示例 hand 容量改为 108；同时说明 shared 无穷区（999）在守恒式中如何计值，并使 YAML 示例通过 L2 校验 |
| F9-06 | Major | §6.3 计时器表 L710-L717、情形表 L719-L724；§6.4 L726-L733 | 单步超时 tick 与玩家动作同时到达的竞态无唯一裁决规则；不确定：需确认服务端单写者队列是否在 tick 边界天然序列化，若否则必须显式裁决 | 全文未规定倒计时到点与待处理 SubmitAction 同时发生时谁生效：超时自动 pass/最小合法牌与已在路上的动作谁先入状态、已受理动作是否作废即将触发的超时托管、超时后到达的动作按哪条错误码拒绝，均无策略。§6.4 只定义幂等与水位，未覆盖超时抢跑；§6.8 L791 称“单房间单写者”但未写明 tick 与动作在队列中的排序规则 | 补一条竞态裁决规则：以服务端 tick 截止时刻为准，截止前已入队并通过合法性校验的动作生效、本次超时动作作废；截止后到达的动作按 STALE_SEQ 或非法动作拒绝并给错误码；写明 tick 与动作在单写者队列中的排队次序 |
| F9-07 | Major | 设计 §1.2 C6 行 L29；设计 §11.1 R10 行 L1176；license.md 收录规则 1 L7 | 对“有署名要求的素材”是否可收录，设计稿与授权台账结论相反，素材合规口径无法执行 | license.md L7：“只收 CC0 或明确允许商用且无需署名的素材；有署名要求的一律不收录，改用自绘”；设计 L29：“有署名要求但允许商用的（如 OFL 字体）可收，但必须随分发保留许可文本”；L1176 R10 同义：“允许商用但有署名要求的（如 OFL 字体）可收，但必须随分发保留许可文本”。两处对署名义务素材一拒一收 | 二选一收口：要么把 license.md L7 改为允许“允许商用且署名义务可随分发保留”的素材，并在台账中补署名留存位置与义务清单；要么把设计稿 L29、L1176 改为署名要求素材一律不收/自绘，与台账对齐；字体节（license.md L17-L21）同步该口径 |
| F9-08 | Minor | §6.7 邀请码段 L785；§5.8 业务与账务表 L523-L532 | invite_code（含批次）表未进存储结构清单，主键、唯一约束与索引缺失 | L785 定义邀请码存 invite_code 表（code, batch_id, used_by, used_at, expires_at），并描述批次 {batchId, count, expiresAt, maxUsesPerCode}；§5.8 业务表清单 L525-L532 无 invite_code，也无批次表；字段类型、code 唯一索引、过期/已用约束未列 | 在 §5.8 补 invite_code 表（及批次表或批次字段归属），列全字段类型、code 唯一索引、expires_at/used_at 约束与一码一用的数据库保障方式 |
| F9-09 | Minor | §9.8 L1061、L1063；§5.8 external_identity 行 L532、round_record 行 L527 | 预留外部标识列两处命名不一致；round_record 预留的 source 字段未落进表结构 | L1061 写 external_identity(external_id, provider, player_id)，L532 写 external_identity(player_id, provider, external_uid, verified_at)，同一列 external_id/external_uid 两名；L1063 写 round_record 允许 source 字段区分 native/imported，但 L527 round_record 字段清单无 source | 统一 external_identity 列名（external_uid 与 external_id 取其一）；把 round_record.source（枚举 native/imported）补进 §5.8 round_record 字段表并注明默认值 |
| F9-10 | Minor | §7.2 “4 个 P0 信号的清单” L828；§7.5 告警分级 P0 行 L855 | 进程崩溃是否属 P0，两处清单不一致 | L828 列四个 P0：statehash_mismatch_total>0、结算账不平、全服无法建房、“进程崩溃（process_crash_total > 0 且伴随房间丢失）”；L855 P0 仅列“状态漂移、结算账不平、全服无法建房”，无进程崩溃，响应渠道（电话/短信+飞书）因此覆盖范围两节不同 | 统一 P0 枚举：在 §7.5 补入进程崩溃信号（含“伴随房间丢失”的限定），或在 §7.2 将进程崩溃降为 P1；两节引用同一份 P0 信号清单 |
| F9-11 | Minor | §8.3 第三条 L929；§8.1 导出规则段 L914；§2.2 后台最小管理台行 L136 | §8.3 残留已删除的“工单关联客服”导出权限，与角色矩阵和不做工单的范围冲突 | L929：“导出回放包含他人手牌，属敏感操作：仅管理员与工单关联的客服可导”；L914：“原先‘客服仅本人工单关联局’的条件随工单系统一起删除”；L136 明确不做工单系统；§8.1 L899-L908 角色仅管理员/研发/只读，可导出回放的为管理员与研发 | 将 L929 改为回放导出仅管理员与研发可导（与 §8.1 矩阵一致），删除“工单关联客服”表述；导出审计字段仍按 §8.8 落 audit_log |
| F9-12 | Minor | §6.2 重连条 L702；§10.2 断线恢复用例行 L1101；§10.1 故障注入行 L1080 | 长断线快照重置（snapshotReset）分支无对应用例，验收结果不可观测 | L702 规定 lastSeq 早于 200 条缓冲起点时“不回放增量，直接下发新快照并令客户端丢弃本地状态（响应里带 snapshotReset: true）”；L1101 故障注入只写“随机断网 5s、随机杀房间进程”，判据为恢复后 stateHash 与断线前一致，未覆盖缓冲溢出的重置路径，也未断言 snapshotReset 标记与本地态丢弃行为 | 在 §10.2（或 §10.1 故障注入层）增一条用例：构造断线期间事件超过 200 条，断言服务端返回 snapshotReset:true、客户端清空本地状态后加载快照、快照 viewHash/stateHash 校验通过，列出可观测结果 |
| F9-13 | Minor | §10.2 R8 行 L1094 | AI 胜率回归的验收基准未定义，±10% 无法判定 | L1094：“胜率矩阵：三档 vs 基线 bot，各 200 局固定种子；每档胜率落在 ±10%”。未写明胜率基准是什么（相对 50% 平局基线，还是相对基线 bot 自身胜率），也未给 easy/normal/hard 各自的目标区间；200 局样本下 ±10% 的口径直接影响用例通过/失败 | 写明各档位目标胜率区间（如 easy/normal/hard 分别对应的胜率上下限），或明确 ±10% 相对的基准值与对手强度；固定种子清单与每局判定结果可从回放包复现 |
| F9-14 | Nit | §1.5 显式假设表 L90（A12）、L91（A14） | 假设编号缺 A13，与本表“表外编号不存在”的声明不符 | L90 为 A12，L91 直接跳到 A14，全文无 A13 条目；L75 声明“正文以 A 编号引用假设时，均指向本表；表外的编号不存在” | 补一条 A13 条目并给出内容，或将 A14–A17 顺延重排为连续编号，正文引用同步更新 |
