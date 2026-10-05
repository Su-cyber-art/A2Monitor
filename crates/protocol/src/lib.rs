use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const PROTOCOL_VERSION: u16 = 1;
pub const MAX_PROBES: usize = 64;
pub const REPLAY_SAMPLES: usize = 600;

pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Facts {
    pub hostname: String,
    pub os: String,
    pub kernel: String,
    pub arch: String,
    pub virtualization: String,
    pub cpu_name: String,
    pub cpu_cores: u32,
    pub mem_total: u64,
    pub swap_total: u64,
    pub disk_total: u64,
    pub ipv4: String,
    pub ipv6: String,
    pub agent_version: String,
    pub interval: u64,
    pub iface: String,
    pub platform: String,
}
impl Facts {
    pub fn validate(&self) -> Result<()> {
        for (value, limit) in [
            (&self.hostname, 256),
            (&self.os, 256),
            (&self.kernel, 128),
            (&self.arch, 32),
            (&self.virtualization, 128),
            (&self.cpu_name, 256),
            (&self.agent_version, 64),
            (&self.iface, 512),
            (&self.platform, 32),
        ] {
            ensure!(value.len() <= limit, "fact string too long");
        }
        ensure!(
            (1..=60).contains(&self.interval) && self.cpu_cores <= 4096,
            "invalid fact configuration"
        );
        for value in [self.mem_total, self.swap_total, self.disk_total] {
            ensure!(value <= i64::MAX as u64, "fact capacity out of range");
        }
        if !self.ipv4.is_empty() {
            ensure!(
                self.ipv4.parse::<std::net::Ipv4Addr>().is_ok(),
                "invalid IPv4 fact"
            );
        }
        if !self.ipv6.is_empty() {
            ensure!(
                self.ipv6.parse::<std::net::Ipv6Addr>().is_ok(),
                "invalid IPv6 fact"
            );
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Pressure {
    pub cpu: Option<f64>,
    pub memory: Option<f64>,
    pub io: Option<f64>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Sample {
    pub time: i64,
    pub seq: u64,
    pub boot_id: String,
    pub cpu: f64,
    pub cpu_steal: f64,
    pub load: [f64; 3],
    pub mem_total: u64,
    pub mem_used: u64,
    pub swap_total: u64,
    pub swap_used: u64,
    pub disk_total: u64,
    pub disk_used: u64,
    pub net_rx: u64,
    pub net_tx: u64,
    pub net_rx_total: u64,
    pub net_tx_total: u64,
    pub tcp: u32,
    pub udp: u32,
    pub procs: u32,
    pub uptime: u64,
    pub pressure: Pressure,
    pub available: Vec<String>,
}

impl Sample {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.time > 0 && self.boot_id.len() <= 256,
            "invalid sample identity"
        );
        for value in [self.cpu, self.cpu_steal]
            .into_iter()
            .chain(self.pressure.cpu)
            .chain(self.pressure.memory)
            .chain(self.pressure.io)
        {
            ensure!(
                value.is_finite() && (0.0..=100.0).contains(&value),
                "invalid percentage"
            );
        }
        ensure!(
            self.load
                .iter()
                .all(|v| v.is_finite() && *v >= 0.0 && *v < 1e6),
            "invalid load"
        );
        for value in [
            self.mem_total,
            self.mem_used,
            self.swap_total,
            self.swap_used,
            self.disk_total,
            self.disk_used,
            self.net_rx,
            self.net_tx,
            self.net_rx_total,
            self.net_tx_total,
            self.uptime,
        ] {
            ensure!(value <= i64::MAX as u64, "counter out of range");
        }
        ensure!(
            self.mem_used <= self.mem_total
                && self.swap_used <= self.swap_total
                && self.disk_used <= self.disk_total,
            "usage exceeds capacity"
        );
        ensure!(
            self.available.len() <= 32 && self.available.iter().all(|s| s.len() < 40),
            "invalid availability"
        );
        Ok(())
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProbeKind {
    #[default]
    Tcp,
    Dns,
    Tls,
    Http,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Probe {
    pub id: String,
    pub name: String,
    pub kind: ProbeKind,
    pub target: String,
    pub family: u8,
    pub interval: u64,
    pub timeout_ms: u64,
    pub node_ids: Vec<String>,
    pub all_nodes: bool,
    pub enabled: bool,
}
impl Default for Probe {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            kind: ProbeKind::Tcp,
            target: String::new(),
            family: 0,
            interval: 60,
            timeout_ms: 3000,
            node_ids: vec![],
            all_nodes: true,
            enabled: true,
        }
    }
}
impl Probe {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            !self.name.trim().is_empty() && self.name.chars().count() <= 80,
            "probe name must contain 1–80 characters"
        );
        ensure!(
            !self.target.is_empty()
                && self.target.len() <= 512
                && !self.target.chars().any(char::is_whitespace),
            "invalid probe target"
        );
        ensure!([0, 4, 6].contains(&self.family), "family must be 0, 4 or 6");
        ensure!(
            (5..=3600).contains(&self.interval) && (100..=10000).contains(&self.timeout_ms),
            "invalid probe timing"
        );
        ensure!(self.node_ids.len() <= 10000, "too many node assignments");
        Ok(())
    }
    pub fn assigned(&self, id: &str) -> bool {
        self.enabled && (self.all_nodes || self.node_ids.iter().any(|n| n == id))
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ProbeResult {
    pub task_id: String,
    pub time: i64,
    pub success: bool,
    pub family: u8,
    pub latency_ms: Option<f64>,
    pub dns_ms: Option<f64>,
    pub tcp_ms: Option<f64>,
    pub tls_ms: Option<f64>,
    pub ttfb_ms: Option<f64>,
    pub status_code: Option<u16>,
    pub error: Option<String>,
}
impl ProbeResult {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            !self.success || self.latency_ms.is_some(),
            "successful probes require a latency"
        );
        ensure!(
            self.task_id.len() <= 80 && self.time > 0,
            "invalid probe result"
        );
        for value in [
            self.latency_ms,
            self.dns_ms,
            self.tcp_ms,
            self.tls_ms,
            self.ttfb_ms,
        ]
        .into_iter()
        .flatten()
        {
            ensure!(
                value.is_finite() && (0.0..=120000.0).contains(&value),
                "invalid probe latency"
            );
        }
        ensure!(
            self.error.as_ref().is_none_or(|v| v.len() <= 512),
            "probe error too long"
        );
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Hello {
    pub protocol: u16,
    pub facts: Facts,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Rpc {
    pub jsonrpc: String,
    pub method: String,
    #[serde(default)]
    pub params: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
}
impl Rpc {
    pub fn notification(method: &str, params: impl Serialize) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            method: method.into(),
            params: serde_json::to_value(params).expect("serializable protocol type"),
            id: None,
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct NodeInput {
    pub name: String,
    pub group: String,
    pub region: String,
    pub public: bool,
    pub public_remark: String,
    pub remark: String,
    pub price: f64,
    pub currency: String,
    pub billing_days: u32,
    pub expires_at: Option<String>,
    pub auto_renew: bool,
    pub traffic_limit: u64,
    pub traffic_mode: String,
    pub traffic_reset_day: u32,
    pub notify: bool,
}
impl NodeInput {
    pub fn validate(&mut self) -> Result<()> {
        self.name = self.name.trim().to_owned();
        ensure!(
            !self.name.is_empty() && self.name.chars().count() <= 80,
            "节点名称需要 1–80 个字"
        );
        ensure!(
            self.group.chars().count() <= 32
                && self.region.len() <= 12
                && self.public_remark.len() <= 400
                && self.remark.len() <= 4000,
            "备注或分组过长"
        );
        ensure!(
            self.price.is_finite() && (0.0..=1e9).contains(&self.price),
            "价格无效"
        );
        if self.currency.is_empty() {
            self.currency = "USD".into();
        }
        ensure!(
            self.currency.len() == 3 && self.currency.bytes().all(|b| b.is_ascii_uppercase()),
            "货币需要三个大写字母"
        );
        if self.traffic_reset_day == 0 {
            self.traffic_reset_day = 1;
        }
        ensure!(
            self.traffic_reset_day <= 31 && self.billing_days <= 36500,
            "周期无效"
        );
        if self.traffic_mode.is_empty() {
            self.traffic_mode = "sum".into();
        }
        ensure!(
            ["sum", "max", "up", "down"].contains(&self.traffic_mode.as_str()),
            "流量计算方式无效"
        );
        ensure!(self.traffic_limit <= i64::MAX as u64, "流量额度过大");
        if let Some(date) = &self.expires_at {
            chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d")?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct NotifySettings {
    pub telegram_token: String,
    pub telegram_chat_id: String,
    pub webhook_url: String,
    pub webhook_headers: std::collections::BTreeMap<String, String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub site_name: String,
    pub description: String,
    pub public_site: bool,
    pub history_days: u32,
    pub timezone: String,
    pub theme: String,
    pub offline_grace_seconds: u64,
    pub traffic_threshold: u32,
    pub notify: NotifySettings,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            site_name: "Monitor".into(),
            description: "每一次变化，都有迹可循。".into(),
            public_site: true,
            history_days: 30,
            timezone: "Asia/Taipei".into(),
            theme: "default".into(),
            offline_grace_seconds: 180,
            traffic_threshold: 80,
            notify: NotifySettings::default(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Event {
    pub id: String,
    pub node_ids: Vec<String>,
    pub kind: String,
    pub severity: String,
    pub title: String,
    pub detail: String,
    pub time: i64,
    pub resolved_at: Option<i64>,
    pub evidence: Value,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn malformed_usage_and_negative_percentages_are_rejected() {
        let mut s = Sample {
            time: now_ms(),
            boot_id: "boot".into(),
            mem_total: 100,
            mem_used: 200,
            ..Default::default()
        };
        assert!(s.validate().is_err());
        s.mem_used = 50;
        s.cpu = -1.0;
        assert!(s.validate().is_err());
        s.cpu = 1.0;
        s.pressure.io = Some(f64::NAN);
        assert!(s.validate().is_err());
    }
    #[test]
    fn resource_absence_is_distinct_from_zero() {
        let s: Sample =
            serde_json::from_value(serde_json::json!({"time":1,"pressure":{"cpu":0.0}})).unwrap();
        assert_eq!(s.pressure.cpu, Some(0.0));
        assert_eq!(s.pressure.io, None);
    }
}
