import {
  createContext,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
  type FormEvent,
  type ReactNode,
} from "react";
import {
  Link,
  NavLink,
  Navigate,
  Outlet,
  Route,
  Routes,
  useNavigate,
  useLocation,
  useParams,
  useSearchParams,
} from "react-router-dom";
import {
  Activity,
  ArrowDown,
  ArrowDownLeft,
  ArrowUp,
  ArrowUpRight,
  Bell,
  ChevronDown,
  ChevronRight,
  Cpu,
  Download,
  ExternalLink,
  Eye,
  EyeOff,
  Globe2,
  HardDrive,
  Layers3,
  Check,
  LogOut,
  Menu,
  MoreHorizontal,
  Network,
  Palette,
  Plus,
  Radio,
  RefreshCw,
  Search,
  Server,
  Settings2,
  ShieldCheck,
  SlidersHorizontal,
  Trash2,
  TrendingUp,
  Wifi,
  X,
} from "lucide-react";
import { api, bytes, date, duration, money, send } from "./api";
import {
  Badge,
  Chart,
  Confirm,
  CopyButton,
  Empty,
  ErrorBox,
  Field,
  Meter,
  Modal,
  NumberStat,
} from "./components";
import { useLiveNodes, useResource } from "./hooks";
import { NodeManagement } from "./NodeManagement";
import { DefaultThemePreview } from "./DefaultThemePreview";
import type {
  Event as MonitorEvent,
  Me,
  Metrics,
  Node,
  Probe,
  ProbeResult,
  Profile,
  Settings,
  Theme,
  ThemeField,
} from "./types";

type Context = {
  me: Me;
  nodes: Node[];
  nodeError: string | null;
  status: string;
  refresh: () => void;
  refreshMe: () => void;
  toast: (message: string) => void;
};
const AppContext = createContext<Context | null>(null);
function useApp() {
  return useContext(AppContext)!;
}
function percent(used: number, total: number) {
  return total > 0 ? Math.min(100, (used / total) * 100) : 0;
}
function trafficUsed(n: Node) {
  const t = n.traffic;
  return n.traffic_mode === "up"
    ? t.month_tx
    : n.traffic_mode === "down"
      ? t.month_rx
      : n.traffic_mode === "max"
        ? Math.max(t.month_rx, t.month_tx)
        : t.month_rx + t.month_tx;
}
function state(n: Node) {
  return n.online ? (n.fresh ? "在线" : "等待新采样") : "连接中断";
}
function nodeLink(n: Node, admin: boolean) {
  return `${admin ? "/admin" : ""}/node/${n.id}`;
}

export default function App() {
  const meResource = useResource<Me>("/me");
  const me = meResource.value;
  const live = useLiveNodes(
    !!me && (me.authed || me.public_site),
    !!me?.authed,
  );
  const [message, setMessage] = useState("");
  useEffect(() => {
    const unauthorized = () => {
      meResource.refresh();
      setMessage("会话已失效，请重新登录。");
    };
    window.addEventListener("monitor:unauthorized", unauthorized);
    return () =>
      window.removeEventListener("monitor:unauthorized", unauthorized);
  }, [meResource.refresh]);
  useEffect(() => {
    if (me) document.title = me.site_name;
  }, [me]);
  useEffect(() => {
    if (!message) return;
    const timer = setTimeout(() => setMessage(""), 4500);
    return () => clearTimeout(timer);
  }, [message]);
  if (!me)
    return (
      <div className="startup">
        <Logo />
        <p>{meResource.error || "正在连接监控中心…"}</p>
        {meResource.error && (
          <button className="button" onClick={meResource.refresh}>
            重新连接
          </button>
        )}
      </div>
    );
  const context: Context = {
    me,
    nodes: live.nodes,
    nodeError: live.error,
    status: live.status,
    refresh: live.refresh,
    refreshMe: meResource.refresh,
    toast: setMessage,
  };
  return (
    <AppContext.Provider value={context}>
      <Routes>
        <Route
          path="/admin"
          element={me.authed ? <ConsoleLayout /> : <Login />}
        >
          <Route index element={<Navigate to="/admin/nodes" replace />} />
          <Route path="nodes" element={<NodesPage />} />
          <Route path="node/:id" element={<NodeSettingsRedirect />} />
          <Route path="network" element={<NetworkPage admin />} />
          <Route path="events" element={<EventsPage />} />
          <Route path="themes" element={<ThemesPage />} />
          <Route path="settings" element={<SettingsPage />} />
          <Route path="*" element={<Navigate to="/admin" replace />} />
        </Route>
        <Route
          path="/"
          element={me.public_site || me.authed ? <PublicLayout /> : <Login />}
        >
          <Route index element={<Overview />} />
          <Route path="node/:id" element={<NodeDetail />} />
          <Route path="network" element={<NetworkPage />} />
          <Route path="*" element={<Navigate to="/" replace />} />
        </Route>
      </Routes>
      {message && (
        <div className="toast" role="status">
          <ShieldCheck size={18} />
          {message}
          <button aria-label="关闭提示" onClick={() => setMessage("")}>
            <X size={14} />
          </button>
        </div>
      )}
    </AppContext.Provider>
  );
}
function Logo({ name = "Monitor" }: { name?: string }) {
  return (
    <div className="logo">
      <span className="logo-mark">
        <Activity size={24} />
      </span>
      <span>{name}</span>
    </div>
  );
}
function LiveBadge() {
  const { status, me } = useApp();
  return (
    <div className="live-state">
      <span className={`status-dot ${status === "live" ? "" : "amber"}`} />
      {me.demo
        ? "演示环境"
        : status === "live"
          ? "实时同步"
          : status === "polling"
            ? "轮询同步"
            : "连接中"}
    </div>
  );
}
function ConsoleLayout() {
  const { me, refresh, refreshMe } = useApp();
  const [menu, setMenu] = useState(false);
  const navigate = useNavigate();
  const location = useLocation();
  const nav = [
    { to: "/admin/nodes", name: "节点管理", icon: Server },
    { to: "/admin/network", name: "网络任务", icon: Network },
    { to: "/admin/events", name: "故障事件", icon: Activity },
    { to: "/admin/themes", name: "主题", icon: Palette },
    { to: "/admin/settings", name: "系统设置", icon: Settings2 },
  ];
  return (
    <div className="console">
      <aside className={`sidebar ${menu ? "open" : ""}`}>
        <Link to="/admin">
          <Logo name={me.site_name} />
        </Link>
        <nav>
          {nav.map((item) => (
            <NavLink
              key={item.to}
              to={item.to}
              onClick={() => setMenu(false)}
            >
              {({ isActive }) => (
                <>
                  <item.icon size={18} />
                  <span>{item.name}</span>
                  {isActive && <i />}
                </>
              )}
            </NavLink>
          ))}
        </nav>
        <div className="sidebar-bottom">
          <a href="/" target="_blank" rel="noopener noreferrer">
            <ExternalLink size={15} />
            公开状态页
          </a>
          <button
            onClick={async () => {
              await send("/auth/logout", {});
              refreshMe();
              navigate("/admin");
            }}
          >
            <LogOut size={15} />
            退出登录
          </button>
        </div>
      </aside>
      <div className="console-main">
        <header className="console-header">
          <div className="breadcrumb">
            <button
              className="icon-button mobile-menu"
              aria-label="打开导航"
              onClick={() => setMenu(!menu)}
            >
              <Menu size={21} />
            </button>
            <span>{me.site_name}</span>
            <ChevronRight size={12} />
            <strong>{nav.find((item) => location.pathname.startsWith(item.to))?.name || "节点管理"}</strong>
          </div>
          <div className="header-actions">
            <LiveBadge />
            <button
              className="icon-button"
              aria-label="刷新数据"
              onClick={refresh}
            >
              <RefreshCw size={17} />
            </button>
          </div>
        </header>
        <main className="content">
          <Outlet />
        </main>
      </div>
      {menu && (
        <button
          className="sidebar-scrim"
          aria-label="关闭导航"
          onClick={() => setMenu(false)}
        />
      )}
    </div>
  );
}
function PublicLayout() {
  const { me, refresh } = useApp();
  return (
    <div className="public-layout">
      <header className="public-header">
        <a href="/">
          <Logo name={me.site_name} />
        </a>
        <nav>
          <a href="/">
            节点总览
          </a>
          <NavLink to="/network">网络监测</NavLink>
        </nav>
        <div className="header-actions">
          <LiveBadge />
          <button
            className="icon-button"
            aria-label="刷新数据"
            onClick={refresh}
          >
            <RefreshCw size={17} />
          </button>
          <Link className="button small" to="/admin">
            管理后台
            <ArrowUpRight size={14} />
          </Link>
        </div>
      </header>
      <main className="public-content">
        <Outlet />
      </main>
    </div>
  );
}
function Login() {
  const { me, refreshMe } = useApp();
  const [password, setPassword] = useState(""),
    [visible, setVisible] = useState(false),
    [busy, setBusy] = useState(false),
    [error, setError] = useState<string | null>(null);
  const navigate = useNavigate();
  const login = async (e: FormEvent) => {
    e.preventDefault();
    setBusy(true);
    try {
      await send("/auth/login", { password });
      refreshMe();
      navigate("/admin");
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setBusy(false);
    }
  };
  return (
    <div className="login-layout">
      <div className="login-form">
        <Logo name={me.site_name} />
        <h1>管理员登录</h1>
        <form onSubmit={login}>
          <Field label="管理员密码">
            <div className="password-input">
              <input
                type={visible ? "text" : "password"}
                autoComplete="current-password"
                required
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                placeholder="输入管理员密码"
              />
              <button
                type="button"
                aria-label={visible ? "隐藏密码" : "显示密码"}
                onClick={() => setVisible(!visible)}
              >
                {visible ? <EyeOff size={18} /> : <Eye size={18} />}
              </button>
            </div>
          </Field>
          <ErrorBox error={error} />
          <button className="button primary full" disabled={busy}>
            {busy ? "正在验证…" : "登录"}
            <ArrowUpRight size={17} />
          </button>
        </form>
        <details className="login-help">
          <summary>密码帮助</summary>
          <p>
            初始密码在 Hub 首次启动日志中。
            <br />
            忘记密码可通过服务器上的重置命令更新。
          </p>
        </details>
        {me.public_site && (
          <a className="text-link" href="/">
            查看公开状态页
            <ChevronRight size={14} />
          </a>
        )}
      </div>
    </div>
  );
}
function PageTitle({
  title,
  description,
  action,
}: {
  title: string;
  description?: string;
  action?: ReactNode;
}) {
  return (
    <div className="page-title">
      <div>
        <h1>{title}</h1>
        {description && <p>{description}</p>}
      </div>
      {action}
    </div>
  );
}
function Overview({ admin = false }: { admin?: boolean }) {
  const { me, nodes, nodeError } = useApp();
  const [group, setGroup] = useState<string | null>(null),
    [query, setQuery] = useState(""),
    [layout, setLayout] = useState("cards"),
    [adding, setAdding] = useState(false);
  const filtered = nodes.filter(
    (n) =>
      (group === null || n.group === group) &&
      `${n.name} ${n.group} ${n.region}`
        .toLowerCase()
        .includes(query.toLowerCase()),
  );
  const groups = [...new Set(nodes.map((n) => n.group).filter(Boolean))];
  const online = filtered.filter((n) => n.online && n.fresh);
  const down = filtered.reduce(
      (sum, n) => sum + (n.online ? n.metrics?.net_rx || 0 : 0),
      0,
    ),
    up = filtered.reduce(
      (sum, n) => sum + (n.online ? n.metrics?.net_tx || 0 : 0),
      0,
    );
  const meanCPU = online.length
    ? online.reduce((sum, n) => sum + (n.metrics?.cpu || 0), 0) / online.length
    : 0;
  return (
    <>
      {admin ? <PageTitle
        title="总览"
        action={
            <button className="button primary" onClick={() => setAdding(true)}>
              <Plus size={17} />
              添加节点
            </button>
        }
      /> : <h1 className="sr-only">节点总览</h1>}
      <ErrorBox error={nodeError} />
      {me.demo && (
        <div className="demo-banner">
          演示数据
        </div>
      )}
      <div className="stats-grid">
        <NumberStat
          label="在线节点"
          value={
            <>
              {online.length}
              <span className="stat-divider"> / {filtered.length}</span>
            </>
          }
          icon={<Server size={17} />}
        />
        <NumberStat
          label="下载速率"
          value={<>{bytes(down)}<span className="unit">/s</span></>}
          icon={<ArrowDownLeft size={18} />}
        />
        <NumberStat
          label="上传速率"
          value={<>{bytes(up)}<span className="unit">/s</span></>}
          icon={<ArrowUpRight size={18} />}
        />
        <NumberStat
          label="平均 CPU"
          value={
            <>
              {online.length?meanCPU.toFixed(1):"—"}
              <span className="unit">%</span>
            </>
          }
          icon={<Cpu size={17} />}
        />
      </div>
      <div className="section-toolbar">
        <div className="group-tabs">
          <button
            className={group === null ? "active" : ""}
            onClick={() => setGroup(null)}
          >
            全部节点<span>{nodes.length}</span>
          </button>
          {groups.map((g) => (
            <button
              key={g}
              className={group === g ? "active" : ""}
              onClick={() => setGroup(g)}
            >
              {g}
            </button>
          ))}
        </div>
        <div className="toolbar-actions">
          <div className="search">
            <Search size={15} />
            <input
              aria-label="搜索节点"
              placeholder="搜索节点…"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
            />
          </div>
          <div className="view-switch">
            <button
              aria-label="卡片视图"
              className={layout === "cards" ? "active" : ""}
              onClick={() => setLayout("cards")}
            >
              <Layers3 size={15} />
            </button>
            <button
              aria-label="列表视图"
              className={layout === "table" ? "active" : ""}
              onClick={() => setLayout("table")}
            >
              <Menu size={16} />
            </button>
          </div>
        </div>
      </div>
      {!filtered.length ? (
        <Empty
          title={nodes.length ? "未找到节点" : "暂无节点"}
          action={
            admin && !nodes.length ? (
              <button
                className="button primary"
                onClick={() => setAdding(true)}
              >
                <Plus size={16} />
                添加节点
              </button>
            ) : undefined
          }
        />
      ) : layout === "cards" ? (
        <div className="node-grid">
          {filtered.map((n) => (
            <NodeCard key={n.id} node={n} admin={admin} />
          ))}
        </div>
      ) : (
        <NodeTable nodes={filtered} admin={admin} />
      )}
      {adding && <NodeEditor onClose={() => setAdding(false)} />}
    </>
  );
}
function NodeCard({ node: n, admin }: { node: Node; admin: boolean }) {
  const m = n.metrics;
  const cpu = m?.available.includes("cpu") ? m.cpu : null;
  const mem = m?.available.includes("memory")
    ? percent(m.mem_used, m.mem_total)
    : null;
  return (
    <Link
      className={`node-card ${n.online ? "" : "offline"}`}
      to={nodeLink(n, admin)}
    >
      <div className="node-card-head">
        <span className="node-emblem">{n.region || <Server size={18} />}</span>
        <div>
          <h3>{n.name}</h3>
          <span>
            {n.facts.os || "等待基础信息"} · {n.facts.cpu_cores || "—"} 核
          </span>
        </div>
        <span
          className={`status-dot ${n.online ? (n.fresh ? "" : "amber") : "red"}`}
          title={state(n)}
        />
      </div>
      <div className="node-card-labels">
        <Badge>{n.group || "未分组"}</Badge>
        <span>{n.online ? duration(m?.uptime || 0) : "最后状态保留"}</span>
      </div>
      <div className="resource-row">
        <div>
          <span>CPU</span>
          <strong>{cpu === null ? "—" : `${cpu.toFixed(1)}%`}</strong>
        </div>
        <Meter value={cpu} muted={!n.online} />
      </div>
      <div className="resource-row">
        <div>
          <span>内存</span>
          <strong>{mem === null ? "—" : `${mem.toFixed(1)}%`}</strong>
        </div>
        <Meter value={mem} muted={!n.online} />
      </div>
      <div className="node-network">
        <span>
          <ArrowDown size={13} />
          {bytes(n.online ? m?.net_rx || 0 : 0)}
          <small>/s</small>
        </span>
        <span>
          <ArrowUp size={13} />
          {bytes(n.online ? m?.net_tx || 0 : 0)}
          <small>/s</small>
        </span>
      </div>
      {(n.price > 0 || n.public_remark) && <div className="node-card-footer">
        <span>
          {n.price > 0
            ? `${money(n.price, n.currency)} / ${n.billing_days || 30} 天`
            : n.public_remark}
        </span>
        <ChevronRight size={16} />
      </div>}
    </Link>
  );
}
function NodeTable({
  nodes,
  admin,
  actions,
}: {
  nodes: Node[];
  admin: boolean;
  actions?: (n: Node) => ReactNode;
}) {
  return (
    <div className="table-wrap">
      <table>
        <thead>
          <tr>
            <th>节点</th>
            <th>状态</th>
            <th>CPU / 内存</th>
            <th>网络速率</th>
            <th>本期流量</th>
            {actions && <th className="right">操作</th>}
          </tr>
        </thead>
        <tbody>
          {nodes.map((n) => (
            <tr key={n.id}>
              <td>
                <Link className="table-node" to={nodeLink(n, admin)}>
                  <span className="node-emblem small">
                    {n.region || <Server size={14} />}
                  </span>
                  <span>
                    <strong>{n.name}</strong>
                    <small>{n.group || "未分组"}</small>
                  </span>
                </Link>
              </td>
              <td>
                <Badge tone={n.online ? (n.fresh ? "green" : "amber") : "red"}>
                  <span
                    className={`status-dot ${n.online ? (n.fresh ? "" : "amber") : "red"}`}
                  />
                  {state(n)}
                </Badge>
              </td>
              <td>
                <div className="table-metric">
                  {n.metrics?.cpu.toFixed(1) ?? "—"}%
                  <span>
                    {" "}
                    /{" "}
                    {n.metrics
                      ? percent(
                          n.metrics.mem_used,
                          n.metrics.mem_total,
                        ).toFixed(1)
                      : "—"}
                    %
                  </span>
                </div>
              </td>
              <td>
                <div className="table-network">
                  <span>↓ {bytes(n.metrics?.net_rx || 0)}/s</span>
                  <small>↑ {bytes(n.metrics?.net_tx || 0)}/s</small>
                </div>
              </td>
              <td>
                {bytes(trafficUsed(n))}
                {n.traffic_limit > 0 && (
                  <small className="block muted">
                    额度 {bytes(n.traffic_limit)}
                  </small>
                )}
              </td>
              {actions && <td className="right">{actions(n)}</td>}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
function NodesPage() {
  const { nodes, nodeError, refresh, toast } = useApp();
  const [params, setParams] = useSearchParams();
  const [editing, setEditing] = useState<Node | null | undefined>(undefined),
    [deleting, setDeleting] = useState<Node | null>(null),
    [install, setInstall] = useState<{ node: Node; token: string } | null>(
      null,
    ),
    [error, setError] = useState<string | null>(null),
    [reconnecting, setReconnecting] = useState<Node | null>(null),
    [updating, setUpdating] = useState<string | null>(null);
  useEffect(() => {
    const id = params.get("edit");
    if (id && editing === undefined) {
      const node = nodes.find((n) => n.id === id);
      if (node) setEditing(node);
    }
  }, [params, nodes, editing]);
  const closeEditor = () => {
    setEditing(undefined);
    if (params.has("edit")) setParams({}, { replace: true });
  };
  const rotate = async (n: Node) => {
    try {
      const result = await send<{ token: string }>(
        `/admin/nodes/${n.id}/token`,
        {},
      );
      setInstall({ node: n, token: result.token });
      toast("旧 Token 已失效，请用新命令重新接入。");
    } catch (e) {
      setError((e as Error).message);
    }
  };
  return (
    <>
      <PageTitle
        title="节点管理"
        action={
          <button className="button primary" onClick={() => setEditing(null)}>
            <Plus size={17} />
            添加节点
          </button>
        }
      />
      <ErrorBox error={error || nodeError} />
      {nodes.length ? (
        <NodeManagement
          nodes={nodes}
          updating={updating}
          onEdit={setEditing}
          onInstall={setReconnecting}
          onDelete={setDeleting}
          onReorder={async (ids) => {
            setError(null);
            await send("/admin/nodes/order", ids);
            refresh();
            toast("排序已保存");
          }}
          onVisibility={async (n) => {
            setUpdating(n.id);
            setError(null);
            try {
              await send(`/admin/nodes/${n.id}`, { ...n, public: !n.public }, "PUT");
              refresh();
              toast(n.public ? "节点已隐藏" : "节点已公开");
            } catch (e) {
              setError((e as Error).message);
            } finally {
              setUpdating(null);
            }
          }}
        />
      ) : (
        <Empty
          title="暂无节点"
          action={
            <button className="button primary" onClick={() => setEditing(null)}>
              添加节点
            </button>
          }
        />
      )}
      {editing !== undefined && (
        <NodeEditor
          node={editing || undefined}
          onClose={closeEditor}
        />
      )}{" "}
      {deleting && (
        <Confirm
          title={`删除 ${deleting.name}`}
          detail="节点、指标历史和流量累计会一起删除。此操作无法撤销。"
          onClose={() => setDeleting(null)}
          onConfirm={async () => {
            await api(`/admin/nodes/${deleting.id}`, { method: "DELETE" });
            refresh();
            toast("节点已删除");
          }}
        />
      )}
      {install && (
        <InstallModal {...install} onClose={() => setInstall(null)} />
      )}
      {reconnecting && (
        <Confirm
          title={`重新接入 ${reconnecting.name}`}
          detail="将更新安装凭证。需要在服务器执行新命令后，节点才能重新上线。"
          onClose={() => setReconnecting(null)}
          onConfirm={async () => { await rotate(reconnecting); }}
        />
      )}
    </>
  );
}
function NodeSettingsRedirect() {
  const { id } = useParams();
  return <Navigate to={`/admin/nodes?edit=${encodeURIComponent(id || "")}`} replace />;
}
type NodeForm = Omit<
  Node,
  | "id"
  | "alias"
  | "position"
  | "facts"
  | "traffic"
  | "metrics"
  | "online"
  | "fresh"
  | "last_seen"
>;
const defaultNode: NodeForm = {
  name: "",
  group: "",
  region: "",
  public: true,
  public_remark: "",
  remark: "",
  notify: true,
  price: 0,
  currency: "USD",
  billing_days: 30,
  expires_at: null,
  auto_renew: false,
  traffic_limit: 0,
  traffic_mode: "sum",
  traffic_reset_day: 1,
};
function FormTabs({ id, value, onChange, tabs }: {
  id: string; value: string; onChange: (value: string) => void;
  tabs: { id: string; label: string; icon?: ReactNode }[];
}) {
  const buttons = useRef<(HTMLButtonElement | null)[]>([]);
  return <div className="form-tabs" role="tablist" aria-label="设置分类">
    {tabs.map((tab, i) => <button key={tab.id} ref={(el) => { buttons.current[i] = el; }}
      type="button" role="tab" id={`${id}-${tab.id}`} aria-controls={`${id}-panel-${tab.id}`}
      aria-selected={value === tab.id} tabIndex={value === tab.id ? 0 : -1}
      className={value === tab.id ? "active" : ""} onClick={() => onChange(tab.id)}
      onKeyDown={(e) => {
        let next = i;
        if (e.key === "ArrowRight") next = (i + 1) % tabs.length;
        else if (e.key === "ArrowLeft") next = (i - 1 + tabs.length) % tabs.length;
        else if (e.key === "Home") next = 0;
        else if (e.key === "End") next = tabs.length - 1;
        else return;
        e.preventDefault(); onChange(tabs[next].id); buttons.current[next]?.focus();
      }}>{tab.icon}{tab.label}</button>)}
  </div>;
}
function NodeEditor({ node, onClose }: { node?: Node; onClose: () => void }) {
  const { refresh, toast } = useApp();
  const [input, setInput] = useState<NodeForm>(node ? { ...defaultNode, ...node } : defaultNode);
  const [tab, setTab] = useState("general");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [installed, setInstalled] = useState<{ node: Node; token: string } | null>(null);
  const set = <K extends keyof NodeForm>(key: K, value: NodeForm[K]) => setInput((v) => ({ ...v, [key]: value }));
  const submit = async (e: FormEvent) => {
    e.preventDefault();
    if (!input.name.trim()) { setTab("general"); setError("请输入节点名称"); return; }
    setBusy(true); setError(null);
    try {
      if (node) {
        await send(`/admin/nodes/${node.id}`, input, "PUT");
        refresh(); toast("节点设置已保存"); onClose();
      } else {
        const result = await send<{ node: Node; token: string }>("/admin/nodes", input);
        refresh(); setInstalled(result);
      }
    } catch (e) { setError((e as Error).message); }
    finally { setBusy(false); }
  };
  if (installed) return <InstallModal {...installed} onClose={onClose} />;
  return <Modal title={node ? "编辑节点" : "添加节点"} onClose={onClose} wide>
    <FormTabs id="node-editor" value={tab} onChange={setTab} tabs={[
      {id:"general",label:"基本信息",icon:<Server size={14}/>},
      {id:"billing",label:"费用与到期",icon:<TrendingUp size={14}/>},
      {id:"traffic",label:"流量设置",icon:<Network size={14}/>},
    ]} />
    <form onSubmit={submit}>
      <div id={`node-editor-panel-${tab}`} role="tabpanel" aria-labelledby={`node-editor-${tab}`}>
      {tab === "general" && <>
        <div className="form-grid">
          <Field label="节点名称"><input required maxLength={80} value={input.name} onChange={(e)=>set("name",e.target.value)} placeholder="节点名称" /></Field>
          <Field label="分组"><input maxLength={32} value={input.group} onChange={(e)=>set("group",e.target.value)} placeholder="分组名称" /></Field>
          <Field label="地区代码"><input maxLength={12} value={input.region} onChange={(e)=>set("region",e.target.value.toUpperCase())} placeholder="SG / JP / TW" /></Field>
          <Field label="公开备注"><input value={input.public_remark} onChange={(e)=>set("public_remark",e.target.value)} placeholder="访客可见" /></Field>
        </div>
        <Field label="私有备注"><textarea rows={3} value={input.remark || ""} onChange={(e)=>set("remark",e.target.value)} placeholder="仅管理员可见" /></Field>
        <div className="checkbox-row">
          <label><input type="checkbox" checked={input.public} onChange={(e)=>set("public",e.target.checked)} />公开展示</label>
          <label><input type="checkbox" checked={input.notify} onChange={(e)=>set("notify",e.target.checked)} />离线与恢复通知</label>
        </div>
      </>}
      {tab === "billing" && <>
        <div className="form-grid">
          <Field label="价格"><input type="number" min="0" step=".01" value={input.price} onChange={(e)=>set("price",+e.target.value)} /></Field>
          <Field label="货币"><select value={input.currency} onChange={(e)=>set("currency",e.target.value)}>{["USD","CNY","TWD","HKD","EUR","JPY"].map(v=><option key={v}>{v}</option>)}</select></Field>
          <Field label="付款周期（天）"><input type="number" min="0" max="36500" value={input.billing_days} onChange={(e)=>set("billing_days",+e.target.value)} /></Field>
          <Field label="到期日"><input type="date" value={input.expires_at || ""} onChange={(e)=>set("expires_at",e.target.value || null)} /></Field>
        </div>
        <label className="checkbox-field"><input type="checkbox" checked={input.auto_renew} onChange={(e)=>set("auto_renew",e.target.checked)} />在线时顺延到期日</label>
      </>}
      {tab === "traffic" && <div className="form-grid">
        <Field label="每月流量额度（GiB）" help="0 表示不限额"><input type="number" min="0" step="1" value={input.traffic_limit/1073741824} onChange={(e)=>set("traffic_limit",Math.round(+e.target.value*1073741824))} /></Field>
        <Field label="流量计算方式"><select value={input.traffic_mode} onChange={(e)=>set("traffic_mode",e.target.value)}><option value="sum">上下行相加</option><option value="max">取较大值</option><option value="up">仅上行</option><option value="down">仅下行</option></select></Field>
        <Field label="每月重置日"><input type="number" min="1" max="31" value={input.traffic_reset_day} onChange={(e)=>set("traffic_reset_day",+e.target.value)} /></Field>
      </div>}
      </div>
      <ErrorBox error={error} />
      <div className="dialog-actions"><button type="button" className="button" onClick={onClose}>取消</button><button className="button primary" disabled={busy}>{busy ? "保存中…" : node ? "保存修改" : "添加节点"}</button></div>
    </form>
  </Modal>;
}
function InstallModal({
  node,
  token,
  onClose,
}: {
  node: Node;
  token: string;
  onClose: () => void;
}) {
  const [server, setServer] = useState(location.origin),
    [interval, setInterval] = useState(5),
    [iface, setIface] = useState("");
  const quote = (s: string) => `'${s.replaceAll("'", "'\\''")}'`;
  const command = `curl -fsSL ${quote(server.replace(/\/$/, "") + "/install.sh")} -o /tmp/monitor-install.sh && sudo sh /tmp/monitor-install.sh --server ${quote(server)} --token ${quote(token)} --interval ${interval}${iface ? ` --iface ${quote(iface)}` : ""}`;
  return (
    <Modal title={`接入 ${node.name}`} onClose={onClose} wide>
      <p className="dialog-copy">
        在目标 Linux 服务器上执行安装命令。接入凭证仅显示这一次，请妥善保存。
      </p>
      <div className="form-grid">
        <Field label="Hub 地址" help="其他机器接入时填写 Hub 的 HTTPS 地址。">
          <input
            type="url"
            value={server}
            onChange={(e) => setServer(e.target.value)}
          />
        </Field>
        <Field label="上报间隔（秒）">
          <input
            type="number"
            min="1"
            max="60"
            value={interval}
            onChange={(e) => setInterval(+e.target.value)}
          />
        </Field>
        <Field label="统计网卡（可选）" help="例如 eth0，或 -eth1 排除网卡">
          <input
            value={iface}
            onChange={(e) => setIface(e.target.value)}
            placeholder="留空使用默认规则"
          />
        </Field>
      </div>
      <div className="code-block">
        <code>{command}</code>
        <CopyButton value={command} label="复制安装命令" />
      </div>
      <div className="dialog-actions">
        <button className="button primary" onClick={onClose}>
          已保存，完成
        </button>
      </div>
    </Modal>
  );
}

function NodeDetail({ admin = false }: { admin?: boolean }) {
  const { id } = useParams();
  const { nodes, me } = useApp();
  const node = nodes.find((n) => n.id === id);
  const [hours, setHours] = useState(6),
    [tab, setTab] = useState("resources");
  const history = useResource<{ records: Metrics[]; step: number }>(
    id ? `/nodes/${id}/history?hours=${hours}&points=240` : null,
    15000,
  );
  const forecast = useResource<{
    used: number;
    limit: number;
    next_reset: string;
    days_left: number;
    observed_days: number;
    estimate: number | null;
    low: number | null;
    high: number | null;
    daily_mean: number;
    days_until_limit: number | null;
    daily: { day: string; rx: number; tx: number }[];
  }>(id ? `/nodes/${id}/forecast` : null, 60000);
  if (!node)
    return (
      <>
        <Link className="back-link" to={admin ? "/admin" : "/"} reloadDocument={!admin}>
          ← 返回节点列表
        </Link>
        <Empty
          title="等待节点信息"
          detail="节点不存在、不可见，或实时数据还未载入。"
        />
        <ErrorBox error={history.error} />
      </>
    );
  const m = node.metrics;
  const records = history.value?.records || [];
  return (
    <>
      <Link className="back-link" to={admin ? "/admin" : "/"} reloadDocument={!admin}>
        ← 返回节点列表
      </Link>
      <PageTitle
        title={node.name}
        description={`${node.facts.os || "等待基础信息"} · ${node.facts.cpu_name || "未知 CPU"} · ${node.facts.cpu_cores || "—"} 核`}
        action={
          <Badge tone={node.online ? (node.fresh ? "green" : "amber") : "red"}>
            <span className={`status-dot ${node.online ? "" : "red"}`} />
            {state(node)}
          </Badge>
        }
      />
      <div className="detail-meta">
        <Badge>{node.region || "未设置地区"}</Badge>
        <Badge>{node.group || "未分组"}</Badge>
        <span>运行 {duration(m?.uptime || 0)}</span>
        <span>最后采样 {date(m?.time || node.last_seen, me.timezone)}</span>
        {admin && node.facts.ipv4 && <span>{node.facts.ipv4}</span>}
      </div>
      <div className="stats-grid">
        <NumberStat
          label="CPU 使用率"
          value={
            <>
              {m?.available.includes("cpu") ? m.cpu.toFixed(1) : "—"}
              <span className="unit">%</span>
            </>
          }
          hint={node.facts.platform === "linux" ? `调度抢占 ${m?.cpu_steal.toFixed(1) ?? "—"}%` : "开发机采样 · 无抢占指标"}
          icon={<Cpu size={17} />}
        />
        <NumberStat
          label="内存使用"
          value={m?.available.includes("memory") ? bytes(m.mem_used) : "—"}
          hint={`总计 ${bytes(m?.mem_total || node.facts.mem_total)}`}
          icon={<Layers3 size={17} />}
        />
        <NumberStat
          label="磁盘使用"
          value={m?.available.includes("disk") ? bytes(m.disk_used) : "不可用"}
          hint={`总计 ${bytes(m?.disk_total || node.facts.disk_total)}`}
          icon={<HardDrive size={17} />}
        />
        <NumberStat
          label="本期流量"
          value={bytes(trafficUsed(node))}
          hint={
            node.traffic_limit ? `额度 ${bytes(node.traffic_limit)}` : "不限额"
          }
          icon={<Network size={17} />}
        />
      </div>
      <div className="section-toolbar">
        <div className="group-tabs">
          <button
            className={tab === "resources" ? "active" : ""}
            onClick={() => setTab("resources")}
          >
            资源曲线
          </button>
          <button
            className={tab === "network" ? "active" : ""}
            onClick={() => setTab("network")}
          >
            网络体验
          </button>
          <button
            className={tab === "traffic" ? "active" : ""}
            onClick={() => setTab("traffic")}
          >
            流量预算
          </button>
        </div>
        <select
          className="period-select"
          aria-label="历史时间范围"
          value={hours}
          onChange={(e) => setHours(+e.target.value)}
        >
          {[1, 6, 24, 168, 720]
            .filter((h) => h <= me.history_days * 24)
            .map((h) => (
              <option key={h} value={h}>
                {h < 24 ? `最近 ${h} 小时` : `最近 ${h / 24} 天`}
              </option>
            ))}
        </select>
      </div>
      <ErrorBox error={history.error} />
      {tab === "resources" && (
        <>
          <div className="chart-grid">
            <section className="panel">
              <div className="panel-head">
                <h3>CPU 使用率</h3>
                <span>平均与峰值</span>
              </div>
              <Chart
                data={records}
                series={[
                  { key: "cpu", label: "平均", color: "#171717" },
                  { key: "cpu_max", label: "峰值", color: "#737373", dash: "5 3" },
                ]}
                max={100}
                format={(v) => `${v.toFixed(0)}%`}
                timezone={me.timezone}
              />
            </section>
            <section className="panel">
              <div className="panel-head">
                <h3>内存使用</h3>
                <span>不含可回收缓存</span>
              </div>
              <Chart
                data={records}
                series={[
                  { key: "mem_used", label: "已用内存", color: "#525252" },
                ]}
                format={(v) => bytes(v, 0)}
                timezone={me.timezone}
              />
            </section>
            <section className="panel">
              <div className="panel-head">
                <h3>网络吞吐</h3>
                <span>接收 / 发送</span>
              </div>
              <Chart
                data={records}
                series={[
                  { key: "net_rx", label: "下载", color: "#171717" },
                  { key: "net_tx", label: "上传", color: "#737373", dash: "5 3" },
                ]}
                format={(v) => bytes(v, 0) + "/s"}
                timezone={me.timezone}
              />
            </section>
            <section className="panel">
              <div className="panel-head">
                <h3>资源等待压力</h3>
                <span title="Linux PSI：任务因资源争用而等待的时间比例">
                  PSI · avg10
                </span>
              </div>
              <Chart
                data={records}
                series={[
                  { key: "pressure.cpu", label: "CPU 调度", color: "#171717" },
                  { key: "pressure.memory", label: "内存", color: "#525252", dash: "5 3" },
                  { key: "pressure.io", label: "I/O", color: "#737373", dash: "2 3" },
                ]}
                max={100}
                format={(v) => v.toFixed(0) + "%"}
                timezone={me.timezone}
              />
              {m && Object.values(m.pressure).every((v) => v === null) && (
                <p className="panel-note">
                  此主机未提供 PSI，缺失项不会按零压力处理。
                </p>
              )}
            </section>
          </div>
          <section className="panel detail-information">
            <h3>基础信息</h3>
            <dl>
              {Object.entries({
                系统架构: node.facts.arch,
                内核: node.facts.kernel,
                虚拟化: node.facts.virtualization || "未识别",
                连接数: m?.available.includes("connections") ? `${m.tcp} TCP · ${m.udp} UDP` : "不可用",
                进程数: m?.procs ?? "—",
                "Agent 版本": node.facts.agent_version || "—",
                ...(admin
                  ? {
                      私有备注: node.remark || "—",
                      统计网卡: node.facts.iface || "默认规则",
                    }
                  : { 公开备注: node.public_remark || "—" }),
              }).map(([label, value]) => (
                <div key={label}>
                  <dt>{label}</dt>
                  <dd>{value}</dd>
                </div>
              ))}
            </dl>
          </section>
        </>
      )}
      {tab === "network" && (
        <NetworkView nodeId={node.id} hours={Math.min(hours, 168)} />
      )}
      {tab === "traffic" && (
        <>
          <ErrorBox error={forecast.error} />
          <div className="forecast-grid">
            <section className="panel forecast-main">
              <h2>
                {bytes(trafficUsed(node))}
                <span>
                  {" "}
                  / {node.traffic_limit ? bytes(node.traffic_limit) : "不限额"}
                </span>
              </h2>
              <Meter
                value={
                  node.traffic_limit
                    ? percent(trafficUsed(node), node.traffic_limit)
                    : 0
                }
              />
              <div className="forecast-caption">
                <span>每月 {node.traffic_reset_day} 日重置</span>
                <span>{forecast.value?.days_left ?? "—"} 天后进入新周期</span>
              </div>
              <div className="forecast-insight">
                <TrendingUp size={22} />
                <div>
                  <strong>
                    {forecast.value?.estimate !== null &&
                    forecast.value?.estimate !== undefined
                      ? `本期预计使用 ${bytes(forecast.value.estimate)}`
                      : "暂无用量预测"}
                  </strong>
                  <p>
                    {forecast.value?.observed_days &&
                    forecast.value.observed_days >= 3
                      ? `参考最近 ${forecast.value.observed_days} 个历史观测日，估计范围 ${bytes(forecast.value.low || 0)} – ${bytes(forecast.value.high || 0)}。`
                      : "至少需要 3 天记录。"}
                  </p>
                </div>
              </div>
            </section>
            <section className="panel">
              <div className="panel-head">
                <h3>累计流量</h3>
              </div>
              <div className="traffic-totals">
                <div>
                  <span>累计下载</span>
                  <strong>{bytes(node.traffic.total_rx)}</strong>
                </div>
                <div>
                  <span>累计上传</span>
                  <strong>{bytes(node.traffic.total_tx)}</strong>
                </div>
                <div>
                  <span>今日下载</span>
                  <strong>{bytes(node.traffic.day_rx)}</strong>
                </div>
                <div>
                  <span>今日上传</span>
                  <strong>{bytes(node.traffic.day_tx)}</strong>
                </div>
              </div>
            </section>
          </div>
        </>
      )}
    </>
  );
}

function NetworkPage({ admin = false }: { admin?: boolean }) {
  const [adding, setAdding] = useState<Probe | null | undefined>(undefined),
    [deleting, setDeleting] = useState<Probe | null>(null);
  const tasks = useResource<Probe[]>("/probes", 15000);
  const { toast } = useApp();
  return (
    <>
      <PageTitle
        title={admin ? "网络任务" : "网络监测"}
        action={
          admin ? (
            <button className="button primary" onClick={() => setAdding(null)}>
              <Plus size={17} />
              添加任务
            </button>
          ) : undefined
        }
      />
      {!admin && <NetworkView />}
      <ErrorBox error={tasks.error} />
      {admin && (
        <section className="panel task-panel">
          <div className="panel-head">
            <h3>探测任务</h3>
          </div>
          {tasks.value?.length ? (
            <div className="table-wrap">
              <table>
                <thead>
                  <tr>
                    <th>任务</th>
                    <th>目标</th>
                    <th>地址族</th>
                    <th>间隔</th>
                    <th className="right">操作</th>
                  </tr>
                </thead>
                <tbody>
                  {tasks.value.map((p) => (
                    <tr key={p.id}>
                      <td>
                        <strong>{p.name}</strong>
                        <small className="block muted">
                          {p.kind.toUpperCase()} ·{" "}
                          {p.enabled ? "已启用" : "已停用"}
                        </small>
                      </td>
                      <td>
                        <code>{p.target}</code>
                      </td>
                      <td>{p.family ? `IPv${p.family}` : "自动"}</td>
                      <td>{p.interval}s</td>
                      <td className="right">
                        <div className="row-actions">
                          <button
                            className="button small"
                            onClick={() => setAdding(p)}
                          >
                            编辑
                          </button>
                          <button
                            className="icon-button destructive"
                            aria-label={`删除 ${p.name}`}
                            onClick={() => setDeleting(p)}
                          >
                            <Trash2 size={15} />
                          </button>
                        </div>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          ) : (
            <Empty
              title="暂无探测任务"
            />
          )}
        </section>
      )}
      {adding !== undefined && (
        <ProbeEditor
          probe={adding || undefined}
          onClose={() => {
            setAdding(undefined);
            tasks.refresh();
          }}
        />
      )}
      {deleting && (
        <Confirm
          title="删除探测任务"
          detail={`删除 ${deleting.name} 及其历史记录，并停止 Agent 上的任务。`}
          onClose={() => setDeleting(null)}
          onConfirm={async () => {
            await api(`/admin/probes/${deleting.id}`, { method: "DELETE" });
            tasks.refresh();
            toast("探测任务已删除");
          }}
        />
      )}
    </>
  );
}
function NetworkView({
  nodeId,
  hours = 24,
}: {
  nodeId?: string;
  hours?: number;
}) {
  const { nodes, me } = useApp();
  const [taskId, setTaskId] = useState("all");
  const [trace, setTrace] = useState("");
  const network = useResource<{ records: ProbeResult[]; profiles: Profile[] }>(
      `/network?hours=${hours}`,
      15000,
    ),
    tasks = useResource<Probe[]>("/probes", 30000);
  const profiles = (network.value?.profiles || []).filter(
    (p) =>
      (!nodeId || p.node_id === nodeId) &&
      (taskId === "all" || p.task_id === taskId),
  );
  const records = (network.value?.records || []).filter(
    (r) =>
      (!nodeId || r.node_id === nodeId) &&
      (taskId === "all" || r.task_id === taskId),
  );
  const samples = profiles.reduce((n, p) => n + p.samples, 0),
    loss = samples
      ? profiles.reduce((n, p) => n + p.loss_percent * p.samples, 0) / samples
      : 0;
  const p95 = profiles.filter((p) => p.p95_ms !== null);
  const selectedTrace = profiles.find(p=>`${p.node_id}:${p.task_id}`===trace)||profiles[0];
  const traceRecords = records.filter(r=>r.node_id===selectedTrace?.node_id&&r.task_id===selectedTrace?.task_id);
  return (
    <>
      <div className="network-summary">
        <NumberStat
          label="探测节点"
          value={new Set(profiles.map((p) => p.node_id)).size}
          icon={<Globe2 size={18} />}
        />
        <NumberStat
          label="探测样本"
          value={samples.toLocaleString()}
          hint={`最近 ${hours} 小时`}
          icon={<Radio size={18} />}
        />
        <NumberStat
          label="连接失败率"
          value={
            <>
              {samples ? loss.toFixed(1) : "—"}
              <span className="unit">%</span>
            </>
          }
          icon={<Wifi size={18} />}
        />
        <NumberStat
          label="平均 P95 延迟"
          value={
            <>
              {p95.length
                ? (
                    p95.reduce((n, p) => n + (p.p95_ms || 0), 0) / p95.length
                  ).toFixed(0)
                : "—"}
              <span className="unit">ms</span>
            </>
          }
          icon={<Activity size={18} />}
        />
      </div>
      <ErrorBox error={network.error} />
      <section className="panel">
        <div className="panel-head">
          <div>
            <h3>分段耗时</h3>
          </div>
          <select
            aria-label="筛选探测任务"
            value={taskId}
            onChange={(e) => setTaskId(e.target.value)}
          >
            <option value="all">全部任务</option>
            {tasks.value?.map((p) => (
              <option key={p.id} value={p.id}>
                {p.name}
              </option>
            ))}
          </select>
        </div>
        {profiles.length ? (
          <div className="table-wrap">
            <table>
              <thead>
                <tr>
                  <th>观察点 / 任务</th>
                  <th>DNS</th>
                  <th>TCP</th>
                  <th>TLS</th>
                  <th>首字节</th>
                  <th>P95</th>
                  <th>失败率</th>
                </tr>
              </thead>
              <tbody>
                {profiles.map((p) => (
                  <tr key={`${p.node_id}:${p.task_id}`}>
                    <td>
                      <strong>
                        {nodes.find((n) => n.id === p.node_id)?.name || "节点"}
                      </strong>
                      <small className="block muted">
                        {tasks.value?.find((t) => t.id === p.task_id)?.name ||
                          "探测"}{" "}
                        · {p.last.family ? `IPv${p.last.family}` : "自动"}
                      </small>
                    </td>
                    {[p.dns_ms, p.tcp_ms, p.tls_ms, p.ttfb_ms, p.p95_ms].map(
                      (v, i) => (
                        <td key={i} className="numeric">
                          {v === null ? "—" : `${v.toFixed(1)} ms`}
                        </td>
                      ),
                    )}
                    <td>
                      <Badge
                        tone={
                          p.loss_percent > 10
                            ? "red"
                            : p.loss_percent > 0
                              ? "amber"
                              : "green"
                        }
                      >
                        {p.loss_percent.toFixed(1)}%
                      </Badge>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        ) : (
          <Empty
            title="暂无探测结果"
          />
        )}
      </section>
      <section className="panel network-chart">
        <div className="panel-head">
          <h3>延迟变化</h3>
          <select aria-label="选择观察序列" value={selectedTrace?`${selectedTrace.node_id}:${selectedTrace.task_id}`:""} onChange={e=>setTrace(e.target.value)}>
            {!profiles.length&&<option value="">暂无数据</option>}
            {profiles.map(p=><option key={`${p.node_id}:${p.task_id}`} value={`${p.node_id}:${p.task_id}`}>{nodes.find(n=>n.id===p.node_id)?.name||'节点'} · {tasks.value?.find(t=>t.id===p.task_id)?.name||'探测'}</option>)}
          </select>
        </div>
        <Chart
          data={traceRecords.map((r) => ({
            ...r,
            latency_ms: r.success ? r.latency_ms : null,
          }))}
          series={[
            { key: "latency_ms", label: "完整探测耗时", color: "#171717" },
          ]}
          format={(v) => `${v.toFixed(0)}ms`}
          timezone={me.timezone}
        />
      </section>
    </>
  );
}
function ProbeEditor({
  probe,
  onClose,
}: {
  probe?: Probe;
  onClose: () => void;
}) {
  const { nodes, toast } = useApp();
  const [input, setInput] = useState<Probe>(
      probe || {
        id: "",
        name: "",
        kind: "tcp",
        target: "",
        family: 0,
        interval: 60,
        timeout_ms: 3000,
        node_ids: [],
        all_nodes: true,
        enabled: true,
      },
    ),
    [busy, setBusy] = useState(false),
    [error, setError] = useState<string | null>(null);
  return (
    <Modal title={probe ? "编辑探测" : "添加探测"} onClose={onClose} wide>
      <form
        onSubmit={async (e) => {
          e.preventDefault();
          setBusy(true);
          try {
            await send(
              probe ? `/admin/probes/${probe.id}` : "/admin/probes",
              input,
              probe ? "PUT" : "POST",
            );
            toast("探测任务已保存并下发");
            onClose();
          } catch (e) {
            setError((e as Error).message);
          } finally {
            setBusy(false);
          }
        }}
      >
        <div className="form-grid">
          <Field label="任务名称">
            <input
              required
              value={input.name}
              onChange={(e) =>
                setInput((v) => ({ ...v, name: e.target.value }))
              }
            />
          </Field>
          <Field label="探测类型">
            <select
              value={input.kind}
              onChange={(e) =>
                setInput((v) => ({
                  ...v,
                  kind: e.target.value as Probe["kind"],
                }))
              }
            >
              <option value="tcp">TCP 连接</option>
              <option value="dns">DNS 解析</option>
              <option value="tls">TLS 握手</option>
              <option value="http">HTTP 分段体验</option>
            </select>
          </Field>
          <Field
            label="目标地址"
            help={
              input.kind === "http"
                ? "完整 http/https URL"
                : input.kind === "dns"
                  ? "域名，例如 example.com"
                  : "host:port，例如 example.com:443"
            }
          >
            <input
              required
              value={input.target || ""}
              onChange={(e) =>
                setInput((v) => ({ ...v, target: e.target.value }))
              }
            />
          </Field>
          <Field label="地址族">
            <select
              value={input.family}
              onChange={(e) =>
                setInput((v) => ({ ...v, family: +e.target.value }))
              }
            >
              <option value="0">自动</option>
              <option value="4">IPv4</option>
              <option value="6">IPv6</option>
            </select>
          </Field>
          <Field label="探测间隔（秒）">
            <input
              type="number"
              min="5"
              max="3600"
              value={input.interval}
              onChange={(e) =>
                setInput((v) => ({ ...v, interval: +e.target.value }))
              }
            />
          </Field>
          <Field label="总超时（毫秒）">
            <input
              type="number"
              min="100"
              max="10000"
              value={input.timeout_ms}
              onChange={(e) =>
                setInput((v) => ({ ...v, timeout_ms: +e.target.value }))
              }
            />
          </Field>
        </div>
        <div className="checkbox-row">
          <label>
            <input
              type="checkbox"
              checked={input.enabled}
              onChange={(e) =>
                setInput((v) => ({ ...v, enabled: e.target.checked }))
              }
            />
            启用任务
          </label>
          <label>
            <input
              type="checkbox"
              checked={input.all_nodes}
              onChange={(e) =>
                setInput((v) => ({ ...v, all_nodes: e.target.checked }))
              }
            />
            分配给全部节点及新节点
          </label>
        </div>
        {!input.all_nodes && (
          <div className="node-checkboxes">
            {nodes.map((n) => (
              <label key={n.id}>
                <input
                  type="checkbox"
                  checked={input.node_ids?.includes(n.id) || false}
                  onChange={(e) =>
                    setInput((v) => ({
                      ...v,
                      node_ids: e.target.checked
                        ? [...(v.node_ids || []), n.id]
                        : (v.node_ids || []).filter((id) => id !== n.id),
                    }))
                  }
                />
                {n.name}
              </label>
            ))}
          </div>
        )}
        <ErrorBox error={error} />
        <div className="dialog-actions">
          <button type="button" className="button" onClick={onClose}>
            取消
          </button>
          <button className="button primary" disabled={busy}>
            {busy ? "保存中…" : "保存并下发"}
          </button>
        </div>
      </form>
    </Modal>
  );
}

function EventsPage() {
  const { me, nodes } = useApp();
  const events = useResource<MonitorEvent[]>("/admin/events", 10000);
  const [filter, setFilter] = useState("all"),
    [selected, setSelected] = useState<MonitorEvent | null>(null);
  const list = (events.value || []).filter(
    (e) =>
      filter === "all" ||
      (filter === "active" ? !e.resolved_at : !!e.resolved_at),
  );
  return (
    <>
      <PageTitle
        title="故障事件"
      />
      <div className="stats-grid three">
        <NumberStat
          label="记录事件"
          value={events.value?.length || 0}
          icon={<Activity size={17} />}
        />
        <NumberStat
          label="未恢复"
          value={
            events.value?.filter(
              (e) => !e.resolved_at && e.severity === "warning",
            ).length || 0
          }
          icon={<Bell size={17} />}
        />
        <NumberStat
          label="已恢复"
          value={events.value?.filter((e) => e.resolved_at).length || 0}
          icon={<ShieldCheck size={17} />}
        />
      </div>
      <div className="section-toolbar">
        <div className="group-tabs">
          {[
            ["all", "全部事件"],
            ["active", "未恢复"],
            ["resolved", "已恢复"],
          ].map(([key, label]) => (
            <button
              key={key}
              className={filter === key ? "active" : ""}
              onClick={() => setFilter(key)}
            >
              {label}
            </button>
          ))}
        </div>
        <button className="button small" onClick={events.refresh}>
          <RefreshCw size={14} />
          刷新
        </button>
      </div>
      <ErrorBox error={events.error} />
      {list.length ? (
        <div className="timeline">
          {list.map((event) => (
            <button
              className="timeline-event"
              key={event.id}
              onClick={() => setSelected(event)}
            >
              <div
                className={`event-marker ${event.resolved_at ? "resolved" : event.severity}`}
              >
                <Activity size={17} />
              </div>
              <div className="event-content">
                <div>
                  <h3>{event.title}</h3>
                  <Badge
                    tone={
                      event.resolved_at
                        ? "green"
                        : event.severity === "warning"
                          ? "amber"
                          : "neutral"
                    }
                  >
                    {event.resolved_at
                      ? "已恢复"
                      : event.severity === "warning"
                        ? "待观察"
                        : "记录"}
                  </Badge>
                </div>
                <p>{event.detail}</p>
                <span>
                  {date(event.time, me.timezone)} ·{" "}
                  {event.node_ids
                    .map(
                      (id) =>
                        nodes.find((n) => n.id === id)?.name || "已删除节点",
                    )
                    .join("、")}
                </span>
              </div>
              <ChevronRight size={18} />
            </button>
          ))}
        </div>
      ) : (
        <Empty
          title="暂无事件"
        />
      )}
      {selected && (
        <EventDetail event={selected} onClose={() => setSelected(null)} />
      )}
    </>
  );
}
function EventDetail({
  event,
  onClose,
}: {
  event: MonitorEvent;
  onClose: () => void;
}) {
  const { me, nodes } = useApp();
  const detail = useResource<{
    event: MonitorEvent;
    samples: (Metrics & { node_id: string })[];
  }>(`/admin/events/${event.id}`);
  const [cursor, setCursor] = useState(0);
  const [source, setSource] = useState("");
  const allSamples = detail.value?.samples || [];
  const sources=[...new Set(allSamples.map(s=>s.node_id))];
  const selectedSource=sources.includes(source)?source:sources[0];
  const samples=allSamples.filter(s=>s.node_id===selectedSource);
  const current = samples[Math.min(cursor, samples.length - 1)];
  return (
    <Modal title="故障现场回放" onClose={onClose} wide>
      <div className="event-detail-heading">
        <Badge tone={event.resolved_at ? "green" : "amber"}>
          {event.resolved_at ? "已恢复" : "持续观察"}
        </Badge>
        <h3>{event.title}</h3>
        <p>{event.detail}</p>
        <small>
          {date(event.time, me.timezone)}
          {event.resolved_at && ` → ${date(event.resolved_at, me.timezone)}`}
        </small>
      </div>
      <ErrorBox error={detail.error} />
      {sources.length>1&&<Field label="回放节点"><select value={selectedSource} onChange={e=>{setSource(e.target.value);setCursor(0);}}>{sources.map(id=><option key={id} value={id}>{nodes.find(n=>n.id===id)?.name||id.slice(0,8)}</option>)}</select></Field>}
      <Chart
        data={samples}
        series={[
          { key: "cpu", label: "CPU", color: "#171717" },
          { key: "pressure.io", label: "I/O 等待", color: "#737373", dash: "5 3" },
        ]}
        max={100}
        format={(v) => v.toFixed(0) + "%"}
        timezone={me.timezone}
      />
      {samples.length > 0 && (
        <>
          <div className="replay-control">
            <span>{date(current?.time || 0, me.timezone)}</span>
            <input
              aria-label="回放时间位置"
              type="range"
              min="0"
              max={samples.length - 1}
              value={Math.min(cursor, samples.length - 1)}
              onChange={(e) => setCursor(+e.target.value)}
            />
            <span>{samples.length} 个采样</span>
          </div>
          <div className="replay-values">
            <span>
              CPU <strong>{current?.available.includes("cpu") ? `${current.cpu.toFixed(1)}%` : "不可用"}</strong>
            </span>
            <span>
              内存 <strong>{current?.available.includes("memory") ? bytes(current.mem_used) : "不可用"}</strong>
            </span>
            <span>
              下载 <strong>{current?.available.includes("network") ? `${bytes(current.net_rx)}/s` : "不可用"}</strong>
            </span>
            <span>
              I/O 等待{" "}
              <strong>
                {current?.pressure.io === null
                  ? "不可用"
                  : `${current?.pressure.io?.toFixed(1)}%`}
              </strong>
            </span>
          </div>
        </>
      )}
      <details className="inline-help">
        <summary>采样说明</summary>
        <p>
          采样来自 Hub 窗口或 Agent 补传。
          进程重启或断电可能造成数据缺口。
        </p>
      </details>
      <div className="dialog-actions">
        <a
          className="button"
          href={`/api/v1/admin/events/${event.id}/export`}
          download
        >
          <Download size={16} />
          导出脱敏诊断包
        </a>
        <button className="button primary" onClick={onClose}>
          完成
        </button>
      </div>
    </Modal>
  );
}

function ThemesPage() {
  const { me, refreshMe, toast } = useApp();
  const themes = useResource<Theme[]>("/admin/themes");
  const [busy, setBusy] = useState(false),
    [error, setError] = useState<string | null>(null),
    [editing, setEditing] = useState<Theme | null>(null),
    [deleting, setDeleting] = useState<Theme | null>(null);
  const upload = async (file?: File) => {
    if (!file) return;
    setBusy(true);
    setError(null);
    try {
      const data = new FormData();
      data.set("file", file);
      await api("/admin/themes", { method: "POST", body: data });
      themes.refresh();
      toast("主题已安装，可以启用或配置");
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setBusy(false);
    }
  };
  return (
    <>
      <PageTitle
        title="主题"
        action={
          <label className="button primary" title="支持 ZIP、tar.gz 主题包">
            <Plus size={17} />
            {busy ? "安装中…" : "安装主题"}
            <input
              hidden
              type="file"
              accept=".zip,.gz,.tgz"
              disabled={busy}
              onChange={(e) => {
                void upload(e.target.files?.[0]);
                e.target.value = "";
              }}
            />
          </label>
        }
      />
      <ErrorBox error={error || themes.error} />
      <div className="theme-grid">
        {themes.value?.map((t) => (
          <section
            className={`theme-card ${t.builtin ? "theme-default" : ""} ${me.theme === t.id ? "selected" : ""}`}
            key={t.id}
          >
            <div className={`theme-art ${t.profile}`}>
              {t.builtin ? <DefaultThemePreview /> : <>
              <div className="mini-sidebar" />
              <div className="mini-body">
                <div />
                <div className="mini-cards">
                  <i />
                  <i />
                  <i />
                </div>
                <div className="mini-chart">
                  <svg viewBox="0 0 200 45">
                    <path
                      d="M0 38L20 30L40 35L60 18L80 26L100 9L120 23L140 14L160 20L180 5L200 11"
                      fill="none"
                      stroke="currentColor"
                      strokeWidth="2"
                    />
                  </svg>
                </div>
              </div>
              </>}
              <Badge>
                {t.builtin ? "内置" : "已安装"}
              </Badge>
            </div>
            <div className="theme-info">
              <div>
                <h3>{t.name}</h3>
                {me.theme === t.id && <Badge tone="green"><Check size={11} />使用中</Badge>}
              </div>
              {!t.builtin && t.description && <p>{t.description}</p>}
              <small>
                {t.version || "未声明版本"}
                {t.builtin ? " · 内置主题" : ""}
              </small>
              <div className="theme-actions">
                <button
                  className="button small"
                  disabled={me.theme === t.id}
                  onClick={async () => {
                    try {
                      await send(`/admin/themes/${t.id}/activate`, {});
                      refreshMe();
                      toast("公开页主题已切换");
                    } catch (e) {
                      setError((e as Error).message);
                    }
                  }}
                >
                  {me.theme === t.id ? "已启用" : "启用主题"}
                </button>
                <button className={`button small ${t.builtin ? "primary" : ""}`} onClick={() => setEditing(t)}>
                  <Settings2 size={14} />
                  主题设置
                </button>
                <a className="button small" href="/" target="_blank" rel="noopener noreferrer"><ExternalLink size={14} />预览</a>
                {!t.builtin && me.theme !== t.id && (
                  <button
                    className="icon-button destructive"
                    aria-label={`删除 ${t.name}`}
                    onClick={() => setDeleting(t)}
                  >
                    <Trash2 size={15} />
                  </button>
                )}
              </div>
            </div>
          </section>
        ))}
      </div>
      {editing && (
        <ThemeConfig theme={editing} onClose={() => setEditing(null)} />
      )}{" "}
      {deleting && (
        <Confirm
          title="删除主题"
          detail={`删除 ${deleting.name} 的静态资源。保留的设置不会影响其他主题。`}
          onClose={() => setDeleting(null)}
          onConfirm={async () => {
            await api(`/admin/themes/${deleting.id}`, { method: "DELETE" });
            themes.refresh();
            toast("主题已删除");
          }}
        />
      )}
    </>
  );
}
function localText(value: ThemeField["name"]) {
  return typeof value === "string"
    ? value
    : value?.["zh-CN"] || value?.en || "";
}
function ThemeConfig({
  theme,
  onClose,
}: {
  theme: Theme;
  onClose: () => void;
}) {
  const { toast } = useApp();
  const resource = useResource<Record<string, unknown>>(
    `/admin/themes/${theme.id}/config`,
  );
  const [values, setValues] = useState<Record<string, unknown>>({}),
    [error, setError] = useState<string | null>(null),
    [busy, setBusy] = useState(false),
    [raw, setRaw] = useState(false);
  useEffect(() => {
    if (resource.value) setValues(resource.value);
  }, [resource.value]);
  const config = theme.manifest?.configuration;
  const fields = Array.isArray(config?.data)
    ? config.data
    : theme.manifest?.config || [];
  const defaults = Object.fromEntries(
    fields.filter((f) => f.key).map((f) => [f.key!, f.default]),
  );
  const merged = { ...defaults, ...values };
  const [jsonText, setJsonText] = useState("{}");
  useEffect(() => setJsonText(JSON.stringify(values, null, 2)), [values]);
  return (
    <Modal title={`${theme.name} · 设置`} onClose={onClose} wide>
      <p className="dialog-copy">
        这些设置会对全部访客生效。访客自己的偏好仍由主题保存。
      </p>
      <form
        onSubmit={async (e) => {
          e.preventDefault();
          setBusy(true);
          try {
            await send(
              `/admin/themes/${theme.id}/config`,
              raw ? JSON.parse(jsonText) : values,
              "PUT",
            );
            toast("主题设置已保存");
            onClose();
          } catch (e) {
            setError((e as Error).message);
          } finally {
            setBusy(false);
          }
        }}
      >
        {fields.length && !raw ? (
          <div className="form-grid">
            {fields.map((f, i) => {
              const key = f.key,
                label = f.label || localText(f.name) || key || "";
              if (f.type === "title")
                return (
                  <h3 key={i} className="form-section-title">
                    {label}
                  </h3>
                );
              if (!key) return null;
              const update = (value: unknown) =>
                setValues((v) => ({ ...v, [key]: value }));
              if (["boolean", "switch"].includes(f.type))
                return (
                  <label className="checkbox-field" key={key}>
                    <input
                      type="checkbox"
                      checked={!!merged[key]}
                      onChange={(e) => update(e.target.checked)}
                    />
                    {label}
                  </label>
                );
              if (f.type === "select") {
                const options =
                  typeof f.options === "string"
                    ? f.options.split(",").map((v) => ({ value: v, label: v }))
                    : f.options || [];
                return (
                  <Field key={key} label={label}>
                    <select
                      value={String(merged[key] ?? "")}
                      onChange={(e) => update(e.target.value)}
                    >
                      {options.map((o) => (
                        <option key={o.value} value={o.value}>
                          {o.label}
                        </option>
                      ))}
                    </select>
                  </Field>
                );
              }
              return (
                <Field key={key} label={label} help={localText(f.help)}>
                  {f.type === "text" || f.type === "richtext" ? (
                    <textarea
                      value={String(merged[key] ?? "")}
                      onChange={(e) => update(e.target.value)}
                    />
                  ) : (
                    <input
                      type={f.type === "number" ? "number" : "text"}
                      min={f.min}
                      max={f.max}
                      value={
                        typeof merged[key] === "object"
                          ? JSON.stringify(merged[key])
                          : String(merged[key] ?? "")
                      }
                      onChange={(e) =>
                        update(
                          f.type === "number"
                            ? +e.target.value
                            : e.target.value,
                        )
                      }
                    />
                  )}
                </Field>
              );
            })}
          </div>
        ) : (
          <Field label="主题设置 JSON">
            <textarea
              className="json-editor"
              rows={12}
              value={jsonText}
              onChange={(e) => setJsonText(e.target.value)}
            />
          </Field>
        )}
        <button
          className="text-link"
          type="button"
          onClick={() => setRaw(!raw)}
        >
          {raw ? "返回配置表单" : "编辑原始 JSON"}
        </button>
        <ErrorBox error={error || resource.error} />
        <div className="dialog-actions">
          <button type="button" className="button" onClick={onClose}>
            取消
          </button>
          <button className="button primary" disabled={busy}>
            {busy ? "保存中…" : "保存设置"}
          </button>
        </div>
      </form>
    </Modal>
  );
}

function SettingsPage() {
  const { me, refreshMe, toast } = useApp();
  const [params, setParams] = useSearchParams();
  const tab = ["site", "notify", "security", "data"].includes(params.get("tab") || "") ? params.get("tab")! : "site";
  const resource = useResource<Settings>("/admin/settings");
  const sessions =
    useResource<
      { id: string; created_at: number; expires_at: number; current: boolean }[]
    >("/admin/sessions");
  const database = useResource<{
    bytes: number;
    reclaimable_bytes: number;
    minute_records: number;
  }>("/admin/database");
  const [input, setInput] = useState<Settings | null>(null),
    [error, setError] = useState<string | null>(null),
    [busy, setBusy] = useState(false),
    [password, setPassword] = useState(""),
    [restoreFile, setRestoreFile] = useState<File | null>(null);
  useEffect(() => {
    if (resource.value) setInput(resource.value);
  }, [resource.value]);
  if (!input) return <ErrorBox error={resource.error} />;
  const set = <K extends keyof Settings>(key: K, value: Settings[K]) =>
    setInput((v) => (v ? { ...v, [key]: value } : v));
  const notify = <K extends keyof Settings["notify"]>(
    key: K,
    value: Settings["notify"][K],
  ) =>
    setInput((v) => (v ? { ...v, notify: { ...v.notify, [key]: value } } : v));
  return (
    <>
      <PageTitle
        title="系统设置"
        action={<Badge>{me.version}</Badge>}
      />
      <FormTabs id="settings" value={tab} onChange={(value)=>setParams({tab:value}, {replace:true})} tabs={[
        {id:"site",label:"站点设置",icon:<Globe2 size={14}/>},
        {id:"notify",label:"通知渠道",icon:<Bell size={14}/>},
        {id:"security",label:"账户安全",icon:<ShieldCheck size={14}/>},
        {id:"data",label:"数据管理",icon:<HardDrive size={14}/>},
      ]} />
      <ErrorBox error={error || resource.error} />
      <form
        onSubmit={async (e) => {
          e.preventDefault();
          setBusy(true);
          try {
            await send("/admin/settings", input, "PUT");
            refreshMe();
            resource.refresh();
            toast("设置已保存");
          } catch (e) {
            setError((e as Error).message);
          } finally {
            setBusy(false);
          }
        }}
      >
        <section className="panel settings-panel" id="settings-panel-site" role="tabpanel" aria-labelledby="settings-site" hidden={tab !== "site"}>
          <div className="settings-heading">
            <Settings2 size={21} />
            <div>
              <h3>站点与历史</h3>
            </div>
          </div>
          <div className="form-grid">
            <Field label="站点名称">
              <input
                required
                value={input.site_name}
                onChange={(e) => set("site_name", e.target.value)}
              />
            </Field>
            <Field label="站点描述">
              <input
                value={input.description}
                onChange={(e) => set("description", e.target.value)}
              />
            </Field>
            <Field label="历史保留天数">
              <input
                type="number"
                min="1"
                max="365"
                value={input.history_days}
                onChange={(e) => set("history_days", +e.target.value)}
              />
            </Field>
            <Field label="统计时区">
              <select
                value={input.timezone}
                onChange={(e) => set("timezone", e.target.value)}
              >
                {[
                  "Asia/Taipei",
                  "Asia/Shanghai",
                  "Asia/Tokyo",
                  "Asia/Singapore",
                  "UTC",
                  "Europe/Berlin",
                  "America/New_York",
                ].map((t) => (
                  <option key={t}>{t}</option>
                ))}
              </select>
            </Field>
          </div>
          <label className="checkbox-field">
            <input
              type="checkbox"
              checked={input.public_site}
              onChange={(e) => set("public_site", e.target.checked)}
            />
            开放公开状态页
          </label>
        </section>
        <section className="panel settings-panel" id="settings-panel-notify" role="tabpanel" aria-labelledby="settings-notify" hidden={tab !== "notify"}>
          <div className="settings-heading">
            <Bell size={21} />
            <div>
              <h3>通知渠道</h3>
            </div>
          </div>
          <div className="form-grid">
            <Field
              label="离线通知宽限（秒）"
              help="宽限期内恢复连接，不发送离线通知。"
            >
              <input
                type="number"
                min="10"
                max="3600"
                value={input.offline_grace_seconds}
                onChange={(e) => set("offline_grace_seconds", +e.target.value)}
              />
            </Field>
            <Field label="流量提醒阈值（%）">
              <input
                type="number"
                min="1"
                max="100"
                value={input.traffic_threshold}
                onChange={(e) => set("traffic_threshold", +e.target.value)}
              />
            </Field>
            <Field
              label="Telegram Bot Token"
              help={
                input.notify.telegram_token_set
                  ? "已配置，留空沿用已保存的 Token"
                  : "未配置"
              }
            >
              <input
                type="password"
                autoComplete="new-password"
                value={input.notify.telegram_token}
                onChange={(e) => notify("telegram_token", e.target.value)}
                placeholder={
                  input.notify.telegram_token_set
                    ? "••••••••（留空保留）"
                    : "Bot Token"
                }
              />
            </Field>
            <Field label="Telegram Chat ID">
              <input
                value={input.notify.telegram_chat_id}
                onChange={(e) => notify("telegram_chat_id", e.target.value)}
                placeholder="留空停用 Telegram"
              />
            </Field>
            <Field label="Webhook URL">
              <input
                type="url"
                value={input.notify.webhook_url}
                onChange={(e) => notify("webhook_url", e.target.value)}
                placeholder="https://…"
              />
            </Field>
            <Field label="Webhook 请求头 JSON" help="留空沿用已保存的请求头。">
              <input
                defaultValue=""
                onChange={(e) => {
                  try {
                    notify(
                      "webhook_headers",
                      JSON.parse(e.target.value || "{}"),
                    );
                    setError(null);
                  } catch {
                    setError("请求头需要合法的 JSON 对象");
                  }
                }}
                placeholder={'{"Authorization":"Bearer …"}'}
              />
            </Field>
          </div>
          <div className="panel-actions">
            <button
              type="button"
              className="button"
              onClick={async () => {
                try {
                  await send("/admin/notifications/test", {});
                  toast("测试通知发送成功");
                } catch (e) {
                  setError((e as Error).message);
                }
              }}
            >
              <Bell size={15} />
              发送测试通知
            </button>
          </div>
        </section>
        <div className="save-bar" hidden={tab !== "site" && tab !== "notify"}>
          <button className="button primary" disabled={busy}>
            {busy ? "保存中…" : "保存设置"}
          </button>
        </div>
      </form>
      <div className="settings-bottom-grid">
        <section className="panel settings-panel" id="settings-panel-data" role="tabpanel" aria-labelledby="settings-data" hidden={tab !== "data"}>
          <div className="settings-heading">
            <HardDrive size={21} />
            <div>
              <h3>数据与备份</h3>
              <p>备份包含节点和通知凭证，请私下保管。</p>
            </div>
          </div>
          <div className="db-stats">
            <div>
              <span>数据库大小</span>
              <strong>{bytes(database.value?.bytes || 0)}</strong>
            </div>
            <div>
              <span>可回收空间</span>
              <strong>{bytes(database.value?.reclaimable_bytes || 0)}</strong>
            </div>
          </div>
          <div className="panel-actions wrap">
            <a className="button" href="/api/v1/admin/database/backup" download>
              <Download size={15} />
              导出备份
            </a>
            <label className="button">
              恢复备份
              <input
                hidden
                type="file"
                accept=".db"
                onChange={(e) => {
                  setRestoreFile(e.target.files?.[0] || null);
                  e.target.value = "";
                }}
              />
            </label>
            <button
              className="button"
              onClick={async () => {
                try {
                  await send("/admin/database/vacuum", {});
                  database.refresh();
                  toast("空间回收完成");
                } catch (e) {
                  setError((e as Error).message);
                }
              }}
            >
              回收空间
            </button>
          </div>
        </section>
        <section className="panel settings-panel" id="settings-panel-security" role="tabpanel" aria-labelledby="settings-security" hidden={tab !== "security"}>
          <div className="settings-heading">
            <ShieldCheck size={21} />
            <div>
              <h3>管理员密码</h3>
              <p>更新密码会注销全部登录会话。</p>
            </div>
          </div>
          <form
            onSubmit={async (e) => {
              e.preventDefault();
              try {
                await send("/admin/password", { password });
                refreshMe();
                toast("密码已更新，请重新登录");
              } catch (e) {
                setError((e as Error).message);
              }
            }}
          >
            <Field label="新密码（至少 12 字符）">
              <input
                type="password"
                autoComplete="new-password"
                minLength={12}
                required
                value={password}
                onChange={(e) => setPassword(e.target.value)}
              />
            </Field>
            <button className="button" disabled={!password}>
              更新密码
            </button>
          </form>
        </section>
      </div>
      <section className="panel settings-panel" hidden={tab !== "security"}>
        <div className="panel-head">
          <h3>登录会话</h3>
          <span>14 天有效，可随时注销</span>
        </div>
        <ErrorBox error={sessions.error} />
        <div className="table-wrap">
          <table>
            <thead>
              <tr>
                <th>会话</th>
                <th>创建时间</th>
                <th>到期时间</th>
                <th className="right">操作</th>
              </tr>
            </thead>
            <tbody>
              {sessions.value?.map((s) => (
                <tr key={s.id}>
                  <td>
                    <code>{s.id.slice(0, 10)}…</code>
                    {s.current && <Badge tone="green">当前</Badge>}
                  </td>
                  <td>{date(s.created_at, me.timezone)}</td>
                  <td>{date(s.expires_at, me.timezone)}</td>
                  <td className="right">
                    <button
                      className="button small"
                      onClick={async () => {
                        await api(`/admin/sessions/${s.id}`, {
                          method: "DELETE",
                        });
                        sessions.refresh();
                        if (s.current) refreshMe();
                      }}
                    >
                      注销
                    </button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </section>
      {restoreFile && (
        <Confirm
          title="恢复数据库备份"
          detail="将整体替换节点、历史、设置和密码；全部会话失效，Agent 会重新连接。确认使用这份备份？"
          onClose={() => setRestoreFile(null)}
          onConfirm={async () => {
            const form = new FormData();
            form.set("file", restoreFile);
            await api("/admin/database/restore", {
              method: "POST",
              body: form,
            });
            refreshMe();
            toast("备份已恢复，请重新登录");
          }}
        />
      )}
    </>
  );
}
