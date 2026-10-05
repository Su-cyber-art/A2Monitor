use anyhow::Result;
use monitor_protocol::{Facts, Pressure, Sample};
use std::{collections::BTreeMap, fs, path::PathBuf, time::Instant};
use sysinfo::{Disks, Networks, ProcessesToUpdate, System};

pub struct Collector {
    proc_root: PathBuf,
    sys_root: PathBuf,
    iface: String,
    previous_cpu: [u64; 8],
    previous_net: (u64, u64),
    previous_epoch: String,
    previous_at: Instant,
    seq: u64,
    system: System,
    networks: Networks,
    disks: Disks,
}
impl Collector {
    pub fn new(proc_root: PathBuf, sys_root: PathBuf, iface: String) -> Self {
        Self {
            proc_root,
            sys_root,
            iface,
            previous_cpu: [0; 8],
            previous_net: (0, 0),
            previous_epoch: String::new(),
            previous_at: Instant::now(),
            seq: 0,
            system: System::new(),
            networks: Networks::new_with_refreshed_list(),
            disks: Disks::new_with_refreshed_list(),
        }
    }
    pub fn facts(&mut self, interval: u64) -> Facts {
        let first = self.collect().unwrap_or_default();
        let addresses = if_addrs::get_if_addrs().unwrap_or_default();
        let ipv4 = addresses
            .iter()
            .filter(|i| !i.is_loopback())
            .find_map(|i| match i.addr {
                if_addrs::IfAddr::V4(ref v) => Some(v.ip.to_string()),
                _ => None,
            })
            .unwrap_or_default();
        let ipv6 = addresses
            .iter()
            .filter(|i| !i.is_loopback())
            .find_map(|i| match i.addr {
                if_addrs::IfAddr::V6(ref v) if !v.ip.is_unicast_link_local() => {
                    Some(v.ip.to_string())
                }
                _ => None,
            })
            .unwrap_or_default();
        let cpu = fs::read_to_string(self.proc_root.join("cpuinfo")).unwrap_or_default();
        let os = fs::read_to_string("/etc/os-release").unwrap_or_default();
        Facts {
            hostname: System::host_name().unwrap_or_default(),
            os: os
                .lines()
                .find_map(|l| {
                    l.strip_prefix("PRETTY_NAME=")
                        .map(|v| v.trim_matches('"').to_string())
                })
                .unwrap_or_else(|| System::long_os_version().unwrap_or_default()),
            kernel: System::kernel_version().unwrap_or_default(),
            arch: std::env::consts::ARCH.into(),
            virtualization: if fs::read_to_string("/sys/class/dmi/id/product_name")
                .unwrap_or_default()
                .to_lowercase()
                .contains("virtual")
            {
                "virtual".into()
            } else {
                String::new()
            },
            cpu_name: cpu
                .lines()
                .find_map(|l| {
                    l.strip_prefix("model name")
                        .and_then(|s| s.split_once(':'))
                        .map(|(_, v)| v.trim().to_string())
                })
                .unwrap_or_else(|| {
                    self.system
                        .cpus()
                        .first()
                        .map(|c| c.brand().into())
                        .unwrap_or_default()
                }),
            cpu_cores: std::thread::available_parallelism()
                .map(|n| n.get() as u32)
                .unwrap_or(1),
            mem_total: first.mem_total,
            swap_total: first.swap_total,
            disk_total: first.disk_total,
            ipv4,
            ipv6,
            agent_version: env!("CARGO_PKG_VERSION").into(),
            interval,
            iface: self.iface.clone(),
            platform: std::env::consts::OS.into(),
        }
    }
    pub fn collect(&mut self) -> Result<Sample> {
        self.seq += 1;
        if cfg!(target_os = "linux") {
            self.linux()
        } else {
            Ok(self.development())
        }
    }
    fn linux(&mut self) -> Result<Sample> {
        let now = Instant::now();
        let elapsed = now
            .duration_since(self.previous_at)
            .as_secs_f64()
            .max(0.001);
        let mut sample = Sample {
            time: monitor_protocol::now_ms(),
            seq: self.seq,
            ..Default::default()
        };
        let stat = fs::read_to_string(self.proc_root.join("stat"))?;
        let cpu = parse_cpu(&stat);
        let delta: [u64; 8] = std::array::from_fn(|i| cpu[i].saturating_sub(self.previous_cpu[i]));
        let total: u64 = delta.iter().sum();
        if total > 0 {
            sample.cpu = (total.saturating_sub(delta[3] + delta[4] + delta[7])) as f64
                / total as f64
                * 100.0;
            sample.cpu_steal = delta[7] as f64 / total as f64 * 100.0;
        }
        self.previous_cpu = cpu;
        sample.available.push("cpu".into());
        let mem = fs::read_to_string(self.proc_root.join("meminfo"))?;
        let values = parse_mem(&mem);
        sample.mem_total = *values.get("MemTotal").unwrap_or(&0);
        sample.mem_used = sample.mem_total.saturating_sub(
            *values
                .get("MemAvailable")
                .or_else(|| values.get("MemFree"))
                .unwrap_or(&0),
        );
        sample.swap_total = *values.get("SwapTotal").unwrap_or(&0);
        sample.swap_used = sample
            .swap_total
            .saturating_sub(*values.get("SwapFree").unwrap_or(&0));
        sample.available.extend(["memory".into(), "swap".into()]);
        sample.load = fs::read_to_string(self.proc_root.join("loadavg"))
            .unwrap_or_default()
            .split_whitespace()
            .take(3)
            .enumerate()
            .fold([0.; 3], |mut a, (i, v)| {
                a[i] = v.parse().unwrap_or(0.);
                a
            });
        sample.uptime = fs::read_to_string(self.proc_root.join("uptime"))
            .unwrap_or_default()
            .split_whitespace()
            .next()
            .unwrap_or("0")
            .parse::<f64>()
            .unwrap_or(0.) as u64;
        sample.procs = fs::read_dir(&self.proc_root)
            .map(|d| {
                d.filter_map(|e| e.ok())
                    .filter(|e| {
                        e.file_name()
                            .to_string_lossy()
                            .chars()
                            .all(|c| c.is_ascii_digit())
                    })
                    .count() as u32
            })
            .unwrap_or(0);
        sample.tcp = count_lines(self.proc_root.join("net/tcp"))
            + count_lines(self.proc_root.join("net/tcp6"));
        sample.udp = count_lines(self.proc_root.join("net/udp"))
            + count_lines(self.proc_root.join("net/udp6"));
        sample
            .available
            .extend(["load".into(), "connections".into(), "processes".into()]);
        sample.pressure = Pressure {
            cpu: read_pressure(self.proc_root.join("pressure/cpu")),
            memory: read_pressure(self.proc_root.join("pressure/memory")),
            io: read_pressure(self.proc_root.join("pressure/io")),
        };
        let network = fs::read_to_string(self.proc_root.join("net/dev"));
        let mut included = Vec::new();
        if let Ok(network) = network {
            for (name, rx, tx) in parse_net(&network) {
                if self.include(&name) {
                    included.push(name);
                    sample.net_rx_total = sample.net_rx_total.saturating_add(rx);
                    sample.net_tx_total = sample.net_tx_total.saturating_add(tx);
                }
            }
            sample.available.push("network".into());
        }
        included.sort();
        let boot = fs::read_to_string(self.proc_root.join("sys/kernel/random/boot_id"))
            .unwrap_or_else(|_| {
                stat.lines()
                    .find(|s| s.starts_with("btime "))
                    .unwrap_or("unknown")
                    .to_owned()
            });
        let signature = included
            .join(",")
            .bytes()
            .fold(0xcbf29ce484222325u64, |h, b| {
                (h ^ b as u64).wrapping_mul(0x100000001b3)
            });
        sample.boot_id = format!("{}/{signature:x}", boot.trim());
        if self.previous_epoch == sample.boot_id {
            sample.net_rx =
                (sample.net_rx_total.saturating_sub(self.previous_net.0) as f64 / elapsed) as u64;
            sample.net_tx =
                (sample.net_tx_total.saturating_sub(self.previous_net.1) as f64 / elapsed) as u64;
        }
        self.previous_epoch = sample.boot_id.clone();
        self.previous_net = (sample.net_rx_total, sample.net_tx_total);
        self.previous_at = now;
        let mut devices = std::collections::BTreeSet::new();
        for line in fs::read_to_string(self.proc_root.join("mounts"))
            .unwrap_or_default()
            .lines()
        {
            let fields: Vec<&str> = line.split_whitespace().collect();
            if fields.len() < 3
                || !fields[0].starts_with("/dev/")
                || !devices.insert(fields[0].to_owned())
            {
                continue;
            }
            let mount = fields[1].replace("\\040", " ");
            if let Ok(v) = rustix::fs::statvfs(mount.as_str()) {
                let total = v.f_blocks.saturating_mul(v.f_frsize);
                let free = v.f_bavail.saturating_mul(v.f_frsize);
                sample.disk_total = sample.disk_total.saturating_add(total);
                sample.disk_used = sample.disk_used.saturating_add(total.saturating_sub(free));
            }
        }
        if sample.disk_total > 0 {
            sample.available.push("disk".into());
        }
        sample.validate()?;
        Ok(sample)
    }
    fn include(&self, name: &str) -> bool {
        let names: Vec<&str> = self.iface.split(',').filter(|s| !s.is_empty()).collect();
        if names.iter().any(|n| n.strip_prefix('-') == Some(name)) {
            return false;
        }
        let only: Vec<&str> = names
            .iter()
            .copied()
            .filter(|n| !n.starts_with('-'))
            .collect();
        if !only.is_empty() {
            return only.contains(&name);
        }
        if [
            "lo",
            "docker",
            "veth",
            "br-",
            "virbr",
            "tun",
            "tap",
            "wg",
            "tailscale",
            "cni",
            "flannel",
            "fwbr",
            "fwpr",
        ]
        .iter()
        .any(|p| name.starts_with(p))
        {
            return false;
        }
        let sys = self.sys_root.join("class/net").join(name);
        if sys.join("bridge").exists() || sys.join("bonding").exists() {
            return false;
        }
        !fs::read_dir(sys).is_ok_and(|d| {
            d.filter_map(|e| e.ok())
                .any(|e| e.file_name().to_string_lossy().starts_with("lower_"))
        })
    }
    fn development(&mut self) -> Sample {
        self.system.refresh_cpu_usage();
        self.system.refresh_memory();
        self.system.refresh_processes(ProcessesToUpdate::All, true);
        self.networks.refresh(true);
        self.disks.refresh(true);
        let now = Instant::now();
        let elapsed = now
            .duration_since(self.previous_at)
            .as_secs_f64()
            .max(0.001);
        let (rx, tx) = self
            .networks
            .iter()
            .filter(|(name, _)| {
                !name.starts_with("lo") && !name.starts_with("utun") && !name.starts_with("awdl")
            })
            .fold((0u64, 0u64), |(r, t), (_, n)| {
                (
                    r.saturating_add(n.total_received()),
                    t.saturating_add(n.total_transmitted()),
                )
            });
        let (disk_total, disk_used) = self
            .disks
            .iter()
            .filter(|d| d.mount_point() == std::path::Path::new("/"))
            .fold((0, 0), |(t, u), d| {
                (
                    t + d.total_space(),
                    u + d.total_space() - d.available_space(),
                )
            });
        let s = Sample {
            time: monitor_protocol::now_ms(),
            seq: self.seq,
            boot_id: "development-host".into(),
            cpu: self.system.global_cpu_usage() as f64,
            mem_total: self.system.total_memory(),
            mem_used: self.system.used_memory(),
            swap_total: self.system.total_swap(),
            swap_used: self.system.used_swap(),
            disk_total,
            disk_used,
            net_rx: if self.seq > 1 {
                (rx.saturating_sub(self.previous_net.0) as f64 / elapsed) as u64
            } else {
                0
            },
            net_tx: if self.seq > 1 {
                (tx.saturating_sub(self.previous_net.1) as f64 / elapsed) as u64
            } else {
                0
            },
            net_rx_total: rx,
            net_tx_total: tx,
            procs: self.system.processes().len() as u32,
            uptime: System::uptime(),
            available: vec![
                "cpu".into(),
                "memory".into(),
                "swap".into(),
                "disk".into(),
                "network".into(),
                "processes".into(),
            ],
            ..Default::default()
        };
        self.previous_net = (rx, tx);
        self.previous_at = now;
        s
    }
}
pub fn parse_cpu(stat: &str) -> [u64; 8] {
    stat.lines()
        .next()
        .unwrap_or_default()
        .split_whitespace()
        .skip(1)
        .take(8)
        .enumerate()
        .fold([0; 8], |mut a, (i, v)| {
            a[i] = v.parse().unwrap_or(0);
            a
        })
}
pub fn parse_mem(mem: &str) -> BTreeMap<String, u64> {
    mem.lines()
        .filter_map(|l| {
            let (key, value) = l.split_once(':')?;
            Some((
                key.to_owned(),
                value
                    .split_whitespace()
                    .next()?
                    .parse::<u64>()
                    .ok()?
                    .saturating_mul(1024),
            ))
        })
        .collect()
}
pub fn parse_net(net: &str) -> Vec<(String, u64, u64)> {
    net.lines()
        .filter_map(|line| {
            let (name, data) = line.split_once(':')?;
            let values: Vec<&str> = data.split_whitespace().collect();
            Some((
                name.trim().to_owned(),
                values.first()?.parse().ok()?,
                values.get(8)?.parse().ok()?,
            ))
        })
        .collect()
}
fn count_lines(path: PathBuf) -> u32 {
    fs::read_to_string(path)
        .map(|s| s.lines().count().saturating_sub(1) as u32)
        .unwrap_or(0)
}
fn read_pressure(path: PathBuf) -> Option<f64> {
    let data = fs::read_to_string(path).ok()?;
    data.lines()
        .find(|l| l.starts_with("some "))?
        .split_whitespace()
        .find_map(|s| s.strip_prefix("avg10=").and_then(|v| v.parse().ok()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn linux_counters_use_the_correct_receive_and_transmit_columns() {
        assert_eq!(
            parse_net(" eth0: 123 1 2 3 4 5 6 7 456 8 9 0 0 0 0 0"),
            vec![("eth0".into(), 123, 456)]
        );
    }
    #[test]
    fn meminfo_is_bytes_and_absent_available_remains_absent() {
        let m = parse_mem("MemTotal: 1024 kB\nMemFree: 100 kB\nSwapTotal: 0 kB");
        assert_eq!(m["MemTotal"], 1048576);
        assert!(!m.contains_key("MemAvailable"));
    }
    #[test]
    fn explicit_iface_selection_takes_priority_over_default_filters() {
        let c = Collector::new("/proc".into(), "/sys".into(), "eth0,-eth1".into());
        assert!(c.include("eth0"));
        assert!(!c.include("eth1"));
        assert!(!c.include("lo"));
    }
}
