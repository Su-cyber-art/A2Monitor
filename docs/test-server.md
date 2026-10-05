# 隔离测试部署

建议使用独立的测试目录、服务名称和回环端口，避免影响服务器已有服务。真实 Linux 部署的验收项目见 [verification.md](verification.md)。

示例目录与服务：

| 项目 | 示例 |
|---|---|
| Hub 安装目录 | `/opt/monitor-dev/hub` |
| Agent 安装目录 | `/opt/monitor-dev/agent` |
| Hub 服务 | `monitor-dev-hub` |
| Agent 服务 | `monitor-dev-agent` |
| Hub 监听 | `127.0.0.1:38180` |

通过 SSH 转发访问测试后台：

```bash
ssh -N -L 38180:127.0.0.1:38180 root@server.example.com
```

打开 `http://127.0.0.1:38180/admin`。首次启动密码在 Hub 日志中；凭证文件应设置为 `0600`，运行数据保存在源码以外的独立目录。

查看服务状态：

```bash
systemctl status monitor-dev-hub monitor-dev-agent
journalctl -u monitor-dev-hub -u monitor-dev-agent -n 100 --no-pager
```

生产部署的 HTTPS 和反向代理配置见 [README](../README.md)。
