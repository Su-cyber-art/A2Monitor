# A2Monitor

自托管的只读服务器探针。Rust Agent 采集 Linux 状态，Rust/Axum Hub 保存数据，React 控制台将实时指标、网络体验与故障现场放在一起。

## 已实现

- 节点接入、分组、排序、隐藏、编辑及独立 Token 换发；凭证在创建/换发时显示一次，服务端只保存哈希。
- CPU、内存、交换、磁盘、网络、进程/连接数和 Linux PSI。Agent 主动连接 Hub，不监听端口，不执行远程 Shell，不自更新。
- 1 秒内存采样、默认 5 秒上报、10 分钟缓冲、重连补传；持续资源压力会上传细粒度现场。
- Hub 累加流量，识别重启/统计网卡变化；日/月周期、不同额度计算方式和带范围的用量预测。
- 分钟历史与小时汇总、1–365 天保留范围，CPU/吞吐峰值。
- TCP、DNS、TLS、HTTP 分段探测、IPv4/IPv6、动态任务和多节点异常关联。
- 故障时间线、按节点回放、恢复记录、独立的脱敏 HTML/JSON 诊断报告。
- 密码登录、14 天会话、注销、登录限流、服务端公开字段白名单。
- Telegram/Webhook 通知、离线宽限、恢复、流量、到期与顺延提醒；按渠道去重和重试。
- 内置后台和默认主题，ZIP/tar.gz 主题安装、切换及设置。提供 Komari 与 monitor 公开页接口适配。
- SQLite 在线备份、流式恢复与空间回收；systemd 安装/卸载及 Docker 构建。

## 下载与安装

[下载 v0.1.0](https://github.com/Su-cyber-art/A2Monitor/releases/tag/v0.1.0)。当前提供已验证的 Linux x86_64 安装包，包含 Hub、Agent、后台和内置默认主题。

```bash
curl -fLO https://github.com/Su-cyber-art/A2Monitor/releases/download/v0.1.0/a2monitor-v0.1.0-linux-amd64.tar.gz
curl -fLO https://github.com/Su-cyber-art/A2Monitor/releases/download/v0.1.0/a2monitor-v0.1.0-linux-amd64.tar.gz.sha256
sha256sum -c a2monitor-v0.1.0-linux-amd64.tar.gz.sha256
mkdir a2monitor-release
tar -xzf a2monitor-v0.1.0-linux-amd64.tar.gz -C a2monitor-release
sudo sh a2monitor-release/scripts/install-hub.sh --package a2monitor-v0.1.0-linux-amd64.tar.gz
```

安装后查看初始后台密码：`sudo journalctl -u monitor-hub --no-pager`。默认访问地址为 `http://127.0.0.1:28080/admin`；通过 SSH 转发预览或配置 HTTPS 反向代理后使用。ARM64 的源码和构建路径已提供，当前发布包仅包含 Linux x86_64。

## 开发运行

需要 Rust 1.98+、Node.js 24+、Bun 1.3.14。生产 Agent 支持 Linux；macOS 仅通过显式 `--dev` 用于真实开发机验证。

```bash
npm --prefix web ci
npm --prefix web run build
sh scripts/build-theme.sh
cargo build --workspace
./target/debug/monitor-hub
```

打开 `http://127.0.0.1:28080/admin`。首次启动在终端显示随机管理员密码。数据默认放在 `./data/monitor.db`。可用 `MONITOR_ADMIN_PASSWORD` 在首次启动时指定至少 12 字符的密码，已有数据库不会被该变量改写。

前端开发时：

```bash
# 先保持 Hub 在 28080 运行
npm --prefix web run dev
```

Vite 默认通过代理访问本地 Hub。可用 `MONITOR_HUB=https://hub.example.com` 指定开发数据源。

## 接入服务器

在后台添加节点，保存一次性 Token 和生成的安装命令。Agent 二进制需与 Hub 放在同一个 `bin/` 目录中，安装器从 Hub 的 `/agent/amd64` 或 `/agent/arm64` 下载与 Hub 相同架构的 Linux 程序。

直接运行也可以：

```bash
MONITOR_TOKEN='<node-token>' ./monitor-agent --server https://hub.example.com
```

常用参数：

| 参数 | 用途 |
|---|---|
| `--interval 5` | 1–60 秒的上报间隔；内存采样仍为 1 秒 |
| `--iface eth0` | 只统计指定网卡；`-eth1` 表示排除 |
| `--disable-probes` | 本地禁止网络探测 |
| `--allow-target <prefix>` | 限制 Hub 下发的探测目标前缀，可重复指定 |
| `--proc-root` / `--sys-root` | 指定只读的 Linux 采集路径 |
| `--dev` | 在非 Linux 开发机上运行；缺失的 Linux 指标保留为不可用 |

Agent 默认要求远程 HTTPS/WSS。回环地址可以使用 HTTP；`--insecure` 仅允许受控测试中的远程明文连接，仍验证 TLS 证书。

## Linux 发布包和 systemd

在目标 Linux 架构上构建：

```bash
sh scripts/build.sh
sudo sh scripts/install-hub.sh --package artifacts/monitor-linux-x86_64.tar.gz
```

Hub 使用专用账户，默认只监听 `127.0.0.1:28080`，数据在 `/opt/monitor-hub/data`。通过反向代理提供 HTTPS：

```caddyfile
hub.example.com {
    reverse_proxy 127.0.0.1:28080
}
```

使用 Nginx 时，要传递 WebSocket Upgrade、原始 Host、`X-Forwarded-Proto`，并将 `X-Forwarded-For` 覆盖为真实客户地址。主题包请求体至少 34 MiB；大备份恢复需要放行相应体积。Hub 只信任来自回环代理的来源地址头。

安装 Agent：

```bash
curl -fsSL https://hub.example.com/install.sh -o install-agent.sh
sudo sh install-agent.sh --server https://hub.example.com --token '<node-token>'
```

两个安装器都支持 `--prefix`、`--service`，便于隔离测试实例。卸载 Agent：

```bash
sudo sh scripts/install-agent.sh --uninstall
```

卸载 Hub 保留数据库：

```bash
sudo sh scripts/install-hub.sh --uninstall
```

重置管理密码（同时注销全部会话）：

```bash
sudo -u monitor-hub /opt/monitor-hub/bin/monitor-hub \
  --data /opt/monitor-hub/data --reset-password
```

## Docker

```bash
docker build -t monitor:local .
docker run -d --name monitor \
  -p 127.0.0.1:28080:28080 \
  -v monitor-data:/data --restart unless-stopped monitor:local
docker logs monitor
```

镜像包含 Hub、同架构 Agent 与已构建前端，以非 root 用户运行。正式 Agent 建议使用宿主机上的 systemd 安装器。

## 主题和接口

原生接口固定在 `/api/v1`，管理接口在 `/api/v1/admin`。已有主题使用独立的兼容层，当前主题 profile 决定同名旧路径的响应格式。

公开页使用基于 Emerald 的内置“默认主题”，管理后台独立；发布包和 Docker 镜像均包含主题资源。集成方式见 [Emerald 内置主题](docs/emerald-integration.md)。

详见 [主题开发与兼容](docs/themes.md) 和 [协议与数据语义](docs/protocol.md)。兼容范围为公开展示接口，带后台、远程终端、插件或私有接口的主题需要单独适配；不把旧主题包转换成功当作完整兼容的证据。

## 验证

```bash
cargo test --workspace
cargo fmt --all -- --check
npm --prefix web test
npm --prefix web run build
sh -n scripts/install-agent.sh scripts/install-hub.sh scripts/build.sh scripts/build-theme.sh
```

协议和系统测试覆盖计数器重置、缺失读取、时区边界、小时聚合、公开字段隔离、鉴权/限流、真实 HTTP/WebSocket、Token 换发、回放、主题边界和备份恢复。浏览器与 Linux 部署记录见 [验收记录](docs/verification.md)。

## 数据边界

- 内存缓冲只能补偿 Agent 存活期间的断联；进程重启或断电会丢失窗口。累计流量在同一计数器周期内可以补偿断联期间的增长。
- 重启、统计网卡变化或计数器回退时重新对基线；开机至首次有效上报之间的流量不会被猜测补算。
- “监控连接中断”不等于机器宕机；多个节点异常仅形成候选原因。Hub 自身停机时无法发送通知，需独立观察点验证。
- 预测参考已有观测日，至少三个日期才给出估计，不是账单或确定承诺。
- 分钟/小时历史保留平均值与部分峰值，无法恢复未保存的原始分布。高级统计聚合需要额外的采样和存储。
- 备份含密码哈希、节点凭证哈希和通知凭证；诊断报告是另外生成的白名单数据包。

MIT License。架构和公开接口规范参考 [Komari](https://github.com/komari-monitor/komari) 与 [monitor](https://github.com/monitor-probe/monitor)；兼容验证使用的第三方主题保留其原许可和版权。
