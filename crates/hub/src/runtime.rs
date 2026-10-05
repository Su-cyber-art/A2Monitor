use crate::db::{public_sample, Db, Node};
use anyhow::Result;
use axum::extract::ws::{Message, WebSocket};
use futures_util::StreamExt;
use monitor_protocol::{Event, Hello, ProbeResult, Rpc, Sample, Settings};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, VecDeque},
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};
use tokio::sync::{mpsc, Mutex, RwLock, Semaphore};

pub type Shared = Arc<App>;
pub enum Control {
    Frame(Rpc),
    Close,
}
pub struct Live {
    pub tag: u64,
    pub connected: bool,
    pub sender: mpsc::Sender<Control>,
    pub sample: Option<Sample>,
    pub received: i64,
    pub ring: VecDeque<Sample>,
    pub thresholds: BTreeMap<String, i64>,
}
pub struct App {
    pub db: Db,
    pub settings: RwLock<Settings>,
    pub live: RwLock<BTreeMap<String, Live>>,
    pub latest_probes: RwLock<BTreeMap<(String, String), ProbeResult>>,
    pub active_events: Mutex<BTreeMap<String, String>>,
    pub tag: AtomicU64,
    pub login_gate: Semaphore,
    pub login_attempts: Mutex<BTreeMap<String, (u32, i64)>>,
    pub themes_dir: PathBuf,
    pub frontend_dir: PathBuf,
    pub data_dir: PathBuf,
    pub demo: bool,
}
impl App {
    pub async fn new(
        db: Db,
        data_dir: PathBuf,
        frontend_dir: PathBuf,
        demo: bool,
    ) -> Result<Shared> {
        let settings = db.settings().await?;
        let active_events = db
            .events()
            .await?
            .into_iter()
            .filter(|e| e.resolved_at.is_none() && e.node_ids.len() == 1)
            .map(|e| (format!("{}:{}", e.node_ids[0], e.kind), e.id))
            .collect();
        Ok(Arc::new(Self {
            db,
            settings: RwLock::new(settings),
            live: RwLock::new(BTreeMap::new()),
            latest_probes: RwLock::new(BTreeMap::new()),
            active_events: Mutex::new(active_events),
            tag: AtomicU64::new(1),
            login_gate: Semaphore::new(1),
            login_attempts: Mutex::new(BTreeMap::new()),
            themes_dir: data_dir.join("themes"),
            frontend_dir,
            data_dir,
            demo,
        }))
    }
    pub async fn nodes(self: &Arc<Self>, admin: bool) -> Result<Vec<Value>> {
        let nodes = self.db.nodes().await?;
        let settings = self.settings.read().await.clone();
        let mut views = vec![];
        for node in nodes.into_iter().filter(|n| admin || n.config.public) {
            let traffic = self
                .db
                .traffic(
                    node.id.clone(),
                    settings.timezone.clone(),
                    node.config.traffic_reset_day,
                )
                .await?;
            let live = self.live.read().await;
            let current = live.get(&node.id);
            let online = current.is_some_and(|l| l.connected);
            let fresh = current.is_some_and(|l| {
                l.connected
                    && monitor_protocol::now_ms() - l.received
                        < ((node.facts.interval.max(5) * 3) as i64 * 1000).max(30000)
            });
            let metrics = current.and_then(|l| l.sample.as_ref()).map(public_sample);
            let mut view = json!({"id":node.id,"alias":node.alias,"position":node.position,"name":node.config.name,"group":node.config.group,"region":node.config.region,"public":node.config.public,"public_remark":node.config.public_remark,"price":node.config.price,"currency":node.config.currency,"billing_days":node.config.billing_days,"expires_at":node.config.expires_at,"traffic_limit":node.config.traffic_limit,"traffic_mode":node.config.traffic_mode,"traffic_reset_day":node.config.traffic_reset_day,"auto_renew":node.config.auto_renew,"online":online,"fresh":fresh,"last_seen":node.last_seen,"metrics":metrics,"traffic":traffic,"facts":{"os":node.facts.os,"kernel":node.facts.kernel,"arch":node.facts.arch,"cpu_name":node.facts.cpu_name,"cpu_cores":node.facts.cpu_cores,"mem_total":node.facts.mem_total,"swap_total":node.facts.swap_total,"disk_total":node.facts.disk_total,"virtualization":node.facts.virtualization,"platform":node.facts.platform,"agent_version":node.facts.agent_version}});
            if admin {
                view["remark"] = json!(node.config.remark);
                view["notify"] = json!(node.config.notify);
                view["facts"] = serde_json::to_value(&node.facts)?;
            }
            views.push(view);
        }
        Ok(views)
    }
    pub async fn push_tasks(&self) -> Result<()> {
        let probes = self.db.probes().await?;
        let live = self.live.read().await;
        for (id, client) in live.iter().filter(|(_, l)| l.connected) {
            let tasks: Vec<_> = probes
                .iter()
                .filter(|p| p.assigned(id))
                .take(monitor_protocol::MAX_PROBES)
                .cloned()
                .collect();
            let _ = client
                .sender
                .try_send(Control::Frame(Rpc::notification("probe.tasks", tasks)));
        }
        Ok(())
    }
    pub async fn disconnect(&self, id: &str) {
        if let Some(client) = self.live.read().await.get(id) {
            let _ = client.sender.try_send(Control::Close);
        }
    }
    pub async fn event(
        &self,
        node_ids: Vec<String>,
        kind: &str,
        severity: &str,
        title: String,
        detail: String,
        evidence: Value,
    ) -> Result<String> {
        let event = Event {
            id: uuid::Uuid::new_v4().to_string(),
            node_ids: node_ids.clone(),
            kind: kind.into(),
            severity: severity.into(),
            title,
            detail,
            time: monitor_protocol::now_ms(),
            resolved_at: None,
            evidence,
        };
        self.db.add_event(event.clone()).await?;
        let samples = {
            let live = self.live.read().await;
            node_ids
                .iter()
                .filter_map(|id| {
                    live.get(id)
                        .map(|l| (id.clone(), l.ring.iter().cloned().collect::<Vec<_>>()))
                })
                .collect::<Vec<_>>()
        };
        for (node, samples) in samples {
            self.db.save_replay(event.id.clone(), node, samples).await?;
        }
        Ok(event.id)
    }
    pub async fn resolve(&self, node: &str, kind: &str) -> Result<()> {
        if let Some(event) = self
            .active_events
            .lock()
            .await
            .remove(&format!("{node}:{kind}"))
        {
            self.db.resolve(event).await?;
        }
        Ok(())
    }
    pub async fn ingest(&self, node: &Node, sample: Sample) -> Result<()> {
        sample.validate()?;
        anyhow::ensure!(
            (sample.time - monitor_protocol::now_ms()).abs() < 86400000,
            "sample clock differs by more than a day"
        );
        let settings = self.settings.read().await.clone();
        self.db
            .ingest(
                node.id.clone(),
                sample.clone(),
                settings.timezone,
                node.config.traffic_reset_day,
            )
            .await?;
        let mut signals = vec![];
        let mut reset = false;
        {
            let mut live = self.live.write().await;
            let Some(client) = live.get_mut(&node.id) else {
                return Ok(());
            };
            if let Some(previous) = &client.sample {
                reset = previous.boot_id != sample.boot_id;
            }
            client.received = monitor_protocol::now_ms();
            client.sample = Some(sample.clone());
            if client.ring.len() == 120 {
                client.ring.pop_front();
            }
            client.ring.push_back(sample.clone());
            for (kind, value) in [
                ("cpu_pressure", sample.pressure.cpu),
                ("memory_pressure", sample.pressure.memory),
                ("io_pressure", sample.pressure.io),
            ] {
                if value.is_some_and(|v| v >= 20.0) {
                    let since = client.thresholds.entry(kind.into()).or_insert(sample.time);
                    if sample.time - *since >= 30000 {
                        signals.push((kind, true, value.unwrap()));
                    }
                } else {
                    client.thresholds.remove(kind);
                    signals.push((kind, false, 0.0));
                }
            }
        }
        if reset {
            self.event(
                vec![node.id.clone()],
                "counter_reset",
                "info",
                format!("{} 的计数器基线变化", node.config.name),
                "可能发生了机器重启或统计网卡变化；累计流量保留，重新建立计数基线。".into(),
                json!({"cause":"reboot_or_interface_change"}),
            )
            .await?;
        }
        for (kind, active, value) in signals {
            let key = format!("{}:{kind}", node.id);
            if active {
                if !self.active_events.lock().await.contains_key(&key) {
                    let label = match kind {
                        "cpu_pressure" => "CPU 调度",
                        "memory_pressure" => "内存",
                        _ => "I/O",
                    };
                    let id=self.event(vec![node.id.clone()],kind,"warning",format!("{} 出现持续的{label}压力",node.config.name),format!("PSI 等待比例持续超过 20%，当前 {value:.1}%。这是资源争用的线索，可结合服务延迟进一步判断。"),json!({"pressure":value,"threshold":20,"duration_seconds":30})).await?;
                    self.active_events.lock().await.insert(key, id);
                }
            } else {
                self.resolve(&node.id, kind).await?;
            }
        }
        let active: Vec<String> = self
            .active_events
            .lock()
            .await
            .iter()
            .filter(|(k, _)| k.starts_with(&format!("{}:", node.id)))
            .map(|(_, v)| v.clone())
            .collect();
        for event in active {
            self.db
                .save_replay(event, node.id.clone(), vec![sample.clone()])
                .await?;
        }
        Ok(())
    }
    pub async fn probe_result(&self, node: &Node, result: ProbeResult) -> Result<()> {
        result.validate()?;
        let probes = self.db.probes().await?;
        let Some(probe) = probes
            .iter()
            .find(|p| p.id == result.task_id && p.assigned(&node.id))
        else {
            return Ok(());
        };
        self.db
            .probe_result(node.id.clone(), result.clone())
            .await?;
        self.latest_probes
            .write()
            .await
            .insert((node.id.clone(), result.task_id.clone()), result.clone());
        let latest = self.latest_probes.read().await;
        let now = monitor_protocol::now_ms();
        let observations: Vec<_> = latest
            .iter()
            .filter(|((_, task), r)| {
                *task == probe.id && now - r.time < (probe.interval as i64 * 3000).max(15000)
            })
            .collect();
        let failed: Vec<String> = observations
            .iter()
            .filter(|(_, r)| !r.success)
            .map(|((node, _), _)| node.clone())
            .collect();
        let healthy = observations.iter().filter(|(_, r)| r.success).count();
        drop(latest);
        let key = format!("correlation:{}", probe.id);
        if failed.len() >= 2 && !self.active_events.lock().await.contains_key(&key) {
            let id=self.event(failed.clone(),"network_correlation","warning",format!("{}：多个观察点同时异常",probe.name),format!("{} 个节点探测失败，{} 个节点成功。候选原因是目标服务或共享网络路径异常；这些观察不足以证明根因。",failed.len(),healthy),json!({"probe_id":probe.id,"failed_nodes":failed,"healthy_observers":healthy,"hypothesis":"target_or_shared_path","confidence":"candidate"})).await?;
            self.active_events.lock().await.insert(key, id);
        } else if failed.len() < 2 {
            if let Some(id) = self.active_events.lock().await.remove(&key) {
                self.db.resolve(id).await?;
            }
        }
        Ok(())
    }
}

pub async fn agent_socket(app: Shared, node_id: String, token_hash: String, mut socket: WebSocket) {
    let tag = app.tag.fetch_add(1, Ordering::Relaxed);
    let (tx, mut rx) = mpsc::channel(64);
    {
        let mut live = app.live.write().await;
        if let Some(old) = live.get(&node_id) {
            let _ = old.sender.try_send(Control::Close);
        }
        let old = live.remove(&node_id);
        live.insert(
            node_id.clone(),
            Live {
                tag,
                connected: true,
                sender: tx,
                sample: old.as_ref().and_then(|l| l.sample.clone()),
                received: 0,
                ring: old.map(|l| l.ring).unwrap_or_default(),
                thresholds: BTreeMap::new(),
            },
        );
    }
    let _ = app.resolve(&node_id, "offline").await;
    let _ = app.push_tasks().await;
    let mut heartbeat = tokio::time::interval(std::time::Duration::from_secs(20));
    let mut last_frame = tokio::time::Instant::now();
    let mut last_report = tokio::time::Instant::now() - std::time::Duration::from_secs(1);
    let mut greeted = false;
    loop {
        tokio::select! {
            frame=socket.next()=>{match frame {
                Some(Ok(Message::Text(text)))=>{
                    if app.db.token_node(token_hash.clone()).await.ok().flatten().as_deref()!=Some(node_id.as_str()) || app.live.read().await.get(&node_id).is_none_or(|client|client.tag!=tag){break;}
                    last_frame=tokio::time::Instant::now();let result:Result<()>=async {
                        let frame:Rpc=serde_json::from_str(&text)?;anyhow::ensure!(frame.jsonrpc=="2.0","invalid protocol");
                        let node=app.db.node(&node_id).await?.ok_or_else(||anyhow::anyhow!("node removed"))?;
                        match frame.method.as_str(){
                            "hello" if !greeted=>{let hello:Hello=serde_json::from_value(frame.params)?;anyhow::ensure!(hello.protocol==monitor_protocol::PROTOCOL_VERSION,"unsupported agent protocol");hello.facts.validate()?;app.db.facts(node_id.clone(),hello.facts).await?;greeted=true;},
                            "report" if greeted&&last_report.elapsed()>=std::time::Duration::from_millis(500)=>{app.ingest(&node,serde_json::from_value(frame.params)?).await?;last_report=tokio::time::Instant::now();},
                            "probe.result" if greeted=>{app.probe_result(&node,serde_json::from_value(frame.params)?).await?;},
                            "backfill" if greeted=>{
                                let reason=frame.params.get("reason").and_then(Value::as_str).unwrap_or("reconnect");
                                let raw=frame.params.get("samples").cloned().unwrap_or_else(||frame.params.clone());
                                let samples:Vec<Sample>=serde_json::from_value(raw)?;
                                anyhow::ensure!(samples.len()<=monitor_protocol::REPLAY_SAMPLES,"backfill too large");
                                for sample in &samples{sample.validate()?;anyhow::ensure!((sample.time-monitor_protocol::now_ms()).abs()<86400000,"invalid backfill time");}
                                let now=monitor_protocol::now_ms();
                                let events=app.db.events().await?;
                                let targets:Vec<_>=if reason=="pressure"{events.into_iter().filter(|e|e.node_ids.contains(&node_id)&&["cpu_pressure","memory_pressure","io_pressure"].contains(&e.kind.as_str())&&e.resolved_at.is_none_or(|time|now-time<120000)).take(3).collect()}else{events.into_iter().filter(|e|e.kind=="offline"&&e.node_ids.contains(&node_id)).take(1).collect()};
                                for event in targets{let clipped=samples.iter().filter(|s|s.time>=event.time-600000&&s.time<=event.resolved_at.unwrap_or(event.time+180000)+120000).cloned().collect();app.db.save_replay(event.id,node_id.clone(),clipped).await?;}
                            },
                            _=>{}
                        }Ok(())
                    }.await;if let Err(error)=result{tracing::warn!(node=%node_id,"Agent frame rejected: {error}");}
                },Some(Ok(Message::Ping(data)))=>{last_frame=tokio::time::Instant::now();if socket.send(Message::Pong(data)).await.is_err(){break;}},Some(Ok(Message::Pong(_)))=>last_frame=tokio::time::Instant::now(),None|Some(Err(_))|Some(Ok(Message::Close(_)))=>break,_=>{}
            }},
            Some(control)=rx.recv()=>{match control{Control::Frame(frame)=>{if socket.send(Message::Text(serde_json::to_string(&frame).unwrap_or_default().into())).await.is_err(){break;}},Control::Close=>{let _=socket.send(Message::Close(None)).await;break;}}},
            _=heartbeat.tick()=>{if last_frame.elapsed()>std::time::Duration::from_secs(90)||socket.send(Message::Ping(vec![].into())).await.is_err(){break;}}
        }
    }
    let ended = {
        let mut live = app.live.write().await;
        if let Some(client) = live.get_mut(&node_id) {
            if client.tag == tag {
                client.connected = false;
                true
            } else {
                false
            }
        } else {
            false
        }
    };
    if ended {
        if let Ok(Some(node)) = app.db.node(&node_id).await {
            let key = format!("{node_id}:offline");
            if !app.active_events.lock().await.contains_key(&key) {
                if let Ok(id)=app.event(vec![node_id.clone()],"offline","warning",format!("{} 的监控连接中断",node.config.name),"Agent 与 Hub 的连接已断开；这不单独证明机器宕机。等待重连或其他观察点验证。".into(),json!({"last_seen":node.last_seen,"state":"monitoring_link_interrupted"})).await{app.active_events.lock().await.insert(key,id);}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn public_nodes_exclude_host_identity_and_private_notes() {
        let db = Db::open(std::path::Path::new(":memory:")).unwrap();
        let node = db
            .add_node(
                "node".into(),
                monitor_protocol::NodeInput {
                    name: "public".into(),
                    public: true,
                    remark: "private-note".into(),
                    ..Default::default()
                },
                "secret-hash".into(),
            )
            .await
            .unwrap();
        db.facts(
            node.id,
            monitor_protocol::Facts {
                hostname: "secret-host".into(),
                ipv4: "192.0.2.1".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let app = App::new(
            db,
            PathBuf::from("/tmp/monitor-test"),
            PathBuf::new(),
            false,
        )
        .await
        .unwrap();
        let public = app.nodes(false).await.unwrap();
        let data = serde_json::to_string(&public).unwrap();
        assert!(
            !data.contains("secret-host")
                && !data.contains("192.0.2.1")
                && !data.contains("private-note")
                && !data.contains("secret-hash")
        );
        assert!(app.nodes(true).await.unwrap()[0].get("remark").is_some());
    }
}
