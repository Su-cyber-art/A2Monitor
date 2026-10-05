# Emerald 内置主题

默认公开主题采用 Komari Emerald 1.0.17，源代码位于 `themes/emerald`，上游版本和许可证记录在 `SOURCE.json`、`LICENSE`。这是发布包内置资源，不需要首次安装后再上传主题。

管理后台使用 `web` 的 React 界面，固定在 `/admin`。Emerald 首页和详情位于 `/`、`/instance/:id`；网络监测入口继续使用 `/network`。管理列表只保留一项内置“默认主题”；仍支持安装兼容的第三方主题。

构建需要 Node.js 24+、Rust 1.98+、Bun 1.3.14：

```bash
npm --prefix web ci
npm --prefix web run build
cd themes/emerald
bun install --frozen-lockfile
bun run lint
bun run build
```

发布构建由 `scripts/build.sh` 完成。发布目录同时包含 `web/dist` 后台和 `web/themes/emerald` 公开主题，Dockerfile 构建并复制这两套资源。

原始项目目录保持独立。内置副本增加了本系统的 RPC 适配、网络监测入口和历史请求的同源地址回退；保留原许可证。默认保留 Emerald 的渐变背景、地球视图与配色，关闭访客信息及公告；页脚只在配置备案信息后显示。移动端卡片为单列，桌面按宽度排列。

国旗及操作系统图标随主题一起打包，来源为 `SOURCE.json` 记录的 Komari Web 公共资源。从登录页、原生节点详情返回公开首页时使用完整页面跳转，以加载当前主题。

内置主题的文件不放入数据库备份。升级或重新安装发布包可恢复这些静态资源；主题配置仍由数据库管理。内置主题禁止通过主题删除接口移除。
