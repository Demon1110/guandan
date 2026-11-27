# tools/replay-viewer

M2 的离线回放器（§7.7）：读服务端导出的回放包，用同一份 `gf-engine` 从种子重演整场，
逐步比对 `stateHash`，最后核对 manifest 的末态哈希。

## 用法

```powershell
# 1) 取包（需要登录 token，且必须是这桌的玩家；只对已结束房间返回）
curl.exe -H "Authorization: Bearer $token" -o m-123.zip http://127.0.0.1:8080/api/replays/m-123

# 2) 离线重放
cargo run -p gf-replay-viewer -- .\m-123.zip
```

退出码：`0` 一致；`1` 有分歧或末态不符；`2` 包不合法。

## 包结构

| 文件 | 内容 |
| --- | --- |
| `manifest.json` | `matchId`、`seed`、规则集身份与版本、座位别名（`p1..p4`）、步数、末态哈希、`verified` |
| `actions.jsonl` | 与 Redis 动作流同构的一行一条动作（含 `roomSeq` / `seatSeq` / `roundNo`） |
| `hash_trace.jsonl` | 每步 `{roomSeq, roundNo, stateHash}` |
| `logs.txt` | 占位说明（当前未按局切分结构化日志） |
| `client_summary.json` | 客户端上报位占位（尚未接入） |

## 边界

- 只复现服务端权威状态，不渲染牌桌；观战 UI 后续只读这个包，不订阅在线房间。
- 重放走 `Room::replay_apply`：轮次与合法性仍由引擎校验，托管录下的动作也能重演。
- 跨局按 `roundNo` 续发牌，序号跨局不归零的口径与 §6.2 一致。
