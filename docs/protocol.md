# 协议与数据语义

## 传输

Agent 向 `/api/v1/agent/ws` 发起 WSS 握手，Token 仅放在 `Authorization: Bearer` 头。握手验证节点身份，后续帧再次验证 Token 与连接代次；换发或删除节点会断开旧连接。

采用 JSON-RPC 2.0 notifications，`jsonrpc: "2.0"`、`method`、`params`，没有远程执行方法。

| 方向 | 方法 | 内容 |
|---|---|---|
| Agent → Hub | `hello` | `protocol: 1` 与静态 `facts`，每个连接一次 |
| Agent → Hub | `report` | 一个经过范围验证的 `Sample` |
| Agent → Hub | `probe.result` | 固定探测任务的结果 |
| Agent → Hub | `backfill` | `{ reason: "reconnect" | "pressure", samples: [...] }`，至多 600 个采样 |
| Hub → Agent | `probe.tasks` | 当前节点获准执行的任务清单，至多 64 项 |

双方使用 WebSocket Ping/Pong。Agent 发送时有 10 秒期限、连接尝试有 15 秒期限；沉默连接会被主动重建。重试退避最多 30 秒，内存采样独立于连接状态继续运行。

## 采样

时间统一为 UTC Unix 毫秒。容量为字节，速率为 B/s，CPU/PSI 为百分比，uptime 为秒。

CPU 来自 Linux `/proc/stat` 的差分，虚拟机 steal 单独报告；内存为 `MemTotal - MemAvailable`。默认磁盘按 `/dev/` 设备去重，以 `statvfs` 统计。网卡默认排除回环、常见隧道和重复统计的堆叠设备，转发或特殊网络应指定 `--iface`。

`available` 列出实际成功读取的指标类别。PSI 缺失为 `null`，不能解释为零压力。开发机模式不伪造 Linux 连接数或 PSI。

## 历史与流量

Hub 将常规上报聚合到分钟，完整小时在结束后再汇总。分钟明细至多保留 7 天，小时按站点的 1–365 天保留范围清理。每个桶保留 CPU、下载和上传峰值，常规字段为观测样本的平均值。

流量存储与历史分开：`boot_id` 包含开机标识与统计网卡集合摘要。同一周期用非负计数器差累加；首次读取、标识变化或回退只建立基线。日/月边界使用站点时区，月重置日超出当月天数时落在该月最后一天。

## 故障与诊断

持续 30 秒以上的 PSI 等待比例 ≥20% 产生资源压力线索。Agent 在持续压力时和压力结束时上传内存窗口。断联记录说明监控链路中断，恢复后可以将缓存补入对应事件。

回放按节点分开，保留接近事件起点的窗口及较新的窗口，数量受到上限约束。多节点同时访问同一目标失败时，记录“目标或共享路径异常”的候选线索，不宣称已证实根因。

诊断导出使用独立白名单，匿名化节点标识，不含主机名、地址、Token、私有备注、原始计数器或任意事件证据对象。文件含本地 SVG 图表、滑动回放、记录表和 JSON 下载，不需要连接 Hub。

## 固定网络探测

- DNS：解析耗时；目标为域名。
- TCP：解析与建连分别计时；目标为 `host:port`。
- TLS：在 TCP 基础上验证证书并记录握手耗时。
- HTTP：HTTP/1.1 GET，记录 DNS、TCP、可选 TLS 与首字节；只读取有界状态行，不下载完整正文，不追踪跳转。

任务间隔为 5–3600 秒，总时限 100–10000 毫秒，Agent 至多同时运行 8 个探测。4/6 指定地址族，0 自动选择；需要比较两个地址族时建立独立任务。

## 原生 HTTP 接口

| 接口 | 用途 |
|---|---|
| `GET /api/v1/me` | 站点、公开开关、登录状态、版本 |
| `GET /api/v1/nodes` | 根据登录身份返回节点快照 |
| `GET /api/v1/nodes/{id}/history?hours=6&points=240` | 有界历史查询 |
| `GET /api/v1/nodes/{id}/forecast` | 流量观测与估计范围 |
| `GET /api/v1/probes` | 访客不获得探测目标和分配列表 |
| `GET /api/v1/network?hours=24` | 可见节点的分段统计与样本 |
| `GET /api/v1/stream` | 每 2 秒更新的身份过滤快照 |
| `POST /api/v1/auth/login` / `auth/logout` | 密码登录与退出 |
| `/api/v1/admin/*` | 必须有有效管理会话；更改同时验证来源 |

管理节点、探测、主题、设置和数据的请求格式可在 `web/src/types.ts`、`crates/protocol/src/lib.rs` 和控制台请求中查看。JSON 错误返回 `{ "error": "可读说明" }`。
