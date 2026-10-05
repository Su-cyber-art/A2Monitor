export interface Metrics {
  time: number;
  cpu: number;
  cpu_steal: number;
  load: number[];
  mem_total: number;
  mem_used: number;
  swap_total: number;
  swap_used: number;
  disk_total: number;
  disk_used: number;
  net_rx: number;
  net_tx: number;
  tcp: number;
  udp: number;
  procs: number;
  uptime: number;
  pressure: { cpu: number | null; memory: number | null; io: number | null };
  available: string[];
  cpu_max?: number;
  net_rx_max?: number;
  net_tx_max?: number;
  samples?: number;
  minutes?: number;
}
export interface Node {
  id: string;
  alias: number;
  position: number;
  name: string;
  group: string;
  region: string;
  public: boolean;
  public_remark: string;
  remark?: string;
  notify?: boolean;
  price: number;
  currency: string;
  billing_days: number;
  expires_at: string | null;
  auto_renew: boolean;
  traffic_limit: number;
  traffic_mode: string;
  traffic_reset_day: number;
  online: boolean;
  fresh: boolean;
  last_seen: number;
  metrics: Metrics | null;
  traffic: {
    total_rx: number;
    total_tx: number;
    month_rx: number;
    month_tx: number;
    day_rx: number;
    day_tx: number;
    month_start: string;
  };
  facts: {
    os: string;
    kernel: string;
    arch: string;
    cpu_name: string;
    cpu_cores: number;
    mem_total: number;
    swap_total: number;
    disk_total: number;
    virtualization: string;
    platform: string;
    agent_version: string;
    hostname?: string;
    ipv4?: string;
    ipv6?: string;
    interval?: number;
    iface?: string;
  };
}
export interface Me {
  authed: boolean;
  site_name: string;
  description: string;
  public_site: boolean;
  history_days: number;
  theme: string;
  timezone: string;
  demo: boolean;
  version: string;
}
export interface Event {
  id: string;
  node_ids: string[];
  kind: string;
  severity: string;
  title: string;
  detail: string;
  time: number;
  resolved_at: number | null;
  evidence: Record<string, unknown>;
}
export interface Probe {
  id: string;
  name: string;
  kind: "tcp" | "dns" | "tls" | "http";
  target?: string;
  family: number;
  interval: number;
  timeout_ms: number;
  node_ids?: string[];
  all_nodes: boolean;
  enabled: boolean;
}
export interface ProbeResult {
  node_id: string;
  task_id: string;
  time: number;
  success: boolean;
  family: number;
  latency_ms: number | null;
  dns_ms: number | null;
  tcp_ms: number | null;
  tls_ms: number | null;
  ttfb_ms: number | null;
  status_code: number | null;
  error?: string | null;
}
export interface Profile {
  node_id: string;
  task_id: string;
  samples: number;
  loss_percent: number;
  median_ms: number | null;
  p95_ms: number | null;
  dns_ms: number | null;
  tcp_ms: number | null;
  tls_ms: number | null;
  ttfb_ms: number | null;
  last: ProbeResult;
}
export interface Theme {
  id: string;
  name: string;
  description: string;
  profile: string;
  version: string;
  builtin?: boolean;
  manifest?: {
    config?: ThemeField[];
    configuration?: { type?: string; data?: ThemeField[] | string };
  };
}
export interface ThemeField {
  key?: string;
  type: string;
  name?: string | Record<string, string>;
  label?: string;
  default?: unknown;
  help?: string | Record<string, string>;
  options?: string | { value: string; label: string }[];
  min?: number;
  max?: number;
}
export interface Settings {
  site_name: string;
  description: string;
  public_site: boolean;
  history_days: number;
  timezone: string;
  theme: string;
  offline_grace_seconds: number;
  traffic_threshold: number;
  notify: {
    telegram_token: string;
    telegram_token_set?: boolean;
    telegram_chat_id: string;
    webhook_url: string;
    webhook_headers: Record<string, string>;
  };
}
