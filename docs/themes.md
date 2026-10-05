# 主题开发与兼容

管理后台固定在 `/admin`，资源固定在 `/ui-assets`。主题替换公开展示页，内置默认主题支持原生功能。

## 原生主题

一个 ZIP 主题包：

```text
monitor-theme.json
dist/
  index.html
  assets/
```

```json
{
  "short": "my-theme",
  "name": "My theme",
  "version": "1.0.0",
  "description": "公开状态页",
  "author": "Your name"
}
```

使用同源 `/api/v1/me`、`nodes`、节点 `history`/`forecast`、`probes`、`network` 与 `stream`。根据 `authed` 控制管理入口；权限由服务端判断，主题隐藏控件不改变数据权限。

访客拿不到 Token、地址、主机名或私有备注。`metrics: null` 表示没有可用采样；`online: true` 但 `fresh: false` 表示连接存在而采样尚未到来/过时。不要将这两种情况当作零负载。

原生主题可自行提供 `config` 配置项数组；对应站点设置保存在 Hub，管理后台提供表单与 JSON 编辑器。

## Komari 公开页

导入 ZIP 中的 `komari-theme.json` 和 `dist/`。适配层提供：

- `/api/me`、`nodes`、`public`、`version`、`recent/{uuid}`。
- `/api/records/load`、`records/ping` 和 `task/ping`。
- `/api/clients` 的 `get` / `get <uuid>` 实时协议。
- `/api/rpc2` 的 HTTP 与 WebSocket 调用；`common:getNodes`、`getNodesLatestStatus`、身份/公开设置与版本。
- 新版 `public:listMetricDefinitions`、`queryMetrics`、`getPingMetricStats`。
- `theme_settings` 公开读取和已登录管理员的配置保存。

支持保存的平均值以及 CPU/吞吐峰值；原始分布、GPU、远程终端、插件和私有管理 RPC 不会伪造实现。依赖这些能力的主题需扩展或关闭相应功能。

累计流量采用桶末值并标记 `last`。对未保存的最大值、末值或分位数聚合返回明确错误；切换回平均值即可显示已有历史。探测指标 `ping.loss` 使用 0–1 比例，统计摘要的 `loss` 使用百分比。

## monitor 公开页

导入 `<short>/theme.json`、`dist/` 的 tar.gz，提供：

- `/api/me`、`/api/nodes` 的 `{ nodes, admin }` 响应。
- `/api/ws` 的推送快照。
- `/api/nodes/{id}/metrics` 的分钟/小时曲线、延迟及探测名称。
- `/api/themes/{short}/config` 的设置读写。

内部 UUID 与 monitor 的数字节点 ID 分开，数字 ID 不因删除而被重用。

## 包与配置边界

包最大 32 MiB，解压最大 64 MiB，单文件最大 8 MiB，至多 2000 个条目。只接受静态文件与目录，拒绝路径穿越、链接与特殊文件。构建产物必须含 `dist/index.html`；不在 Hub 内执行 npm 或主题服务端代码。

主题的 `short: default` 安装为 `<profile>-default`，以保护内置主题。安装和启用是两个动作，当前主题不能删除。配置项是公开数据，不应包含凭证或私有地址。

同名旧 API 根据当前主题 profile 返回对应格式，原生 `/api/v1` 始终稳定。第三方主题的 API 与版本有差异，具体兼容结果记录在 `verification.md`，不承诺所有未测试主题。
