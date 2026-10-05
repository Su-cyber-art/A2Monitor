mod collect;
mod probe;
use anyhow::{Context, Result};
use clap::Parser;
use futures_util::{SinkExt, StreamExt};
use monitor_protocol::{Hello, Probe, Rpc, Sample, PROTOCOL_VERSION, REPLAY_SAMPLES};
use std::{
    collections::{BTreeMap, VecDeque},
    path::PathBuf,
    sync::Arc,
    time::Duration,
};
use tokio::{sync::mpsc, time::Instant};
use tokio_tungstenite::{
    connect_async_with_config,
    tungstenite::{client::IntoClientRequest, Message},
};

#[derive(Parser)]
#[command(version, about = "Monitor 只读采集 Agent；远程任务仅支持固定网络探测")]
struct Args {
    #[arg(long, env = "MONITOR_SERVER")]
    server: String,
    #[arg(long, env = "MONITOR_TOKEN", hide_env_values = true)]
    token: String,
    #[arg(long, env = "MONITOR_INTERVAL", default_value_t = 5)]
    interval: u64,
    #[arg(long, env = "MONITOR_IFACE", default_value = "")]
    iface: String,
    #[arg(long, default_value = "/proc")]
    proc_root: PathBuf,
    #[arg(long, default_value = "/sys")]
    sys_root: PathBuf,
    #[arg(long, help = "仅用于明文的受控测试环境；证书验证仍开启")]
    insecure: bool,
    #[arg(long, help = "允许在非 Linux 开发机上采集真实开发机指标")]
    dev: bool,
    #[arg(long, help = "禁用 Hub 下发的网络探测任务")]
    disable_probes: bool,
    #[arg(long, help = "探测目标前缀白名单，可重复指定")]
    allow_target: Vec<String>,
}
fn endpoint(server: &str, insecure: bool) -> Result<String> {
    let mut url = url::Url::parse(server)?;
    let scheme = match url.scheme() {
        "https" | "wss" => "wss",
        "http" | "ws" => "ws",
        _ => anyhow::bail!("server 必须使用 http/https/ws/wss"),
    };
    let local = url.host_str().is_some_and(|h| {
        h == "localhost"
            || h.trim_matches(['[', ']'])
                .parse::<std::net::IpAddr>()
                .is_ok_and(|ip| ip.is_loopback())
    });
    anyhow::ensure!(
        scheme == "wss" || local || insecure,
        "远程 Hub 默认需要 HTTPS/WSS"
    );
    anyhow::ensure!(
        url.username().is_empty() && url.password().is_none(),
        "server URL 不得包含凭证"
    );
    url.set_scheme(scheme)
        .map_err(|_| anyhow::anyhow!("无效 URL"))?;
    url.set_path("/api/v1/agent/ws");
    url.set_query(None);
    url.set_fragment(None);
    Ok(url.to_string())
}
type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;
type Ring = Arc<tokio::sync::Mutex<VecDeque<Sample>>>;
async fn write(socket: &mut Socket, message: Message) -> Result<()> {
    tokio::time::timeout(Duration::from_secs(10), socket.send(message))
        .await
        .context("Hub 写入超时")??;
    Ok(())
}
fn frame(method: &str, params: impl serde::Serialize) -> Message {
    Message::text(
        serde_json::to_string(&Rpc::notification(method, params))
            .expect("validated protocol payload"),
    )
}
async fn snapshot(ring: &Ring, reason: &str) -> Message {
    frame(
        "backfill",
        serde_json::json!({"reason":reason,"samples":ring.lock().await.iter().cloned().collect::<Vec<_>>()}),
    )
}
#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    let _ = rustls::crypto::ring::default_provider().install_default();
    let args = Args::parse();
    anyhow::ensure!(
        cfg!(target_os = "linux") || args.dev,
        "正式 Agent 仅支持 Linux；开发机测试可用 --dev"
    );
    anyhow::ensure!((1..=60).contains(&args.interval), "interval 应为 1–60 秒");
    let url = endpoint(&args.server, args.insecure)?;
    let ring: Ring = Arc::new(tokio::sync::Mutex::new(VecDeque::with_capacity(
        REPLAY_SAMPLES,
    )));
    let (sample_tx, mut sample_rx) = mpsc::channel::<()>(1);
    let mut collector = collect::Collector::new(
        args.proc_root.clone(),
        args.sys_root.clone(),
        args.iface.clone(),
    );
    let sampling_ring = ring.clone();
    tokio::spawn(async move {
        let _ = collector.collect();
        let mut tick = tokio::time::interval_at(
            Instant::now() + Duration::from_secs(1),
            Duration::from_secs(1),
        );
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tick.tick().await;
            match collector.collect() {
                Ok(sample) if sample.validate().is_ok() => {
                    let mut ring = sampling_ring.lock().await;
                    if ring.len() == REPLAY_SAMPLES {
                        ring.pop_front();
                    }
                    ring.push_back(sample);
                    drop(ring);
                    let _ = sample_tx.try_send(());
                }
                _ => tracing::warn!("采集失败，等待下一次采样"),
            }
        }
    });
    let mut facts_collector = collect::Collector::new(args.proc_root, args.sys_root, args.iface);
    let mut retry = 1u64;
    loop {
        let mut request = url.as_str().into_client_request()?;
        request.headers_mut().insert(
            "Authorization",
            format!("Bearer {}", args.token)
                .parse()
                .context("无效 Token")?,
        );
        let config = tokio_tungstenite::tungstenite::protocol::WebSocketConfig::default()
            .read_buffer_size(4096)
            .max_message_size(Some(1024 * 1024))
            .max_frame_size(Some(1024 * 1024));
        let connection = tokio::select! {v=tokio::time::timeout(Duration::from_secs(15),connect_async_with_config(request,Some(config),true))=>v,_=tokio::signal::ctrl_c()=>return Ok(())};
        if let Ok(Ok((mut socket, _))) = connection {
            let hello = Hello {
                protocol: PROTOCOL_VERSION,
                facts: facts_collector.facts(args.interval),
            };
            let initialized = write(&mut socket, frame("hello", hello)).await.is_ok()
                && write(&mut socket, snapshot(&ring, "reconnect").await)
                    .await
                    .is_ok();
            if initialized {
                tracing::info!("已连接 Hub");
                retry = 1;
                let (result_tx, mut result_rx) = mpsc::channel(128);
                let mut tasks = BTreeMap::<String, (Probe, tokio::task::JoinHandle<()>)>::new();
                let probe_gate = Arc::new(tokio::sync::Semaphore::new(8));
                let mut last_sent = Instant::now() - Duration::from_secs(args.interval);
                let mut last_frame = Instant::now();
                let mut heartbeat = tokio::time::interval(Duration::from_secs(20));
                let mut pressure = BTreeMap::<&str, Instant>::new();
                let mut captured = std::collections::BTreeSet::new();
                loop {
                    tokio::select! {
                        Some(())=sample_rx.recv()=>{
                            let Some(sample)=ring.lock().await.back().cloned()else{continue;};
                            if last_sent.elapsed()>=Duration::from_secs(args.interval){if write(&mut socket,frame("report",&sample)).await.is_err(){break;}last_sent=Instant::now();}
                            let mut capture=false;
                            for (kind,value) in [("cpu",sample.pressure.cpu),("memory",sample.pressure.memory),("io",sample.pressure.io)] {
                                if value.is_some_and(|v|v>=20.0){let since=pressure.entry(kind).or_insert_with(Instant::now);if since.elapsed()>=Duration::from_secs(35)&&captured.insert(kind){capture=true;}}
                                else{pressure.remove(kind);if captured.remove(kind){capture=true;}}
                            }
                            if capture&&write(&mut socket,snapshot(&ring,"pressure").await).await.is_err(){break;}
                        },
                        Some(result)=result_rx.recv()=>{if write(&mut socket,frame("probe.result",result)).await.is_err(){break;}},
                        incoming=socket.next()=>match incoming {
                            Some(Ok(Message::Text(text)))=>{
                                last_frame=Instant::now();
                                if let Ok(rpc)=serde_json::from_str::<Rpc>(&text){if rpc.method=="probe.tasks"&&!args.disable_probes{if let Ok(probes)=serde_json::from_value::<Vec<Probe>>(rpc.params){
                                    let probes:Vec<_>=probes.into_iter().filter(|p|p.validate().is_ok()&&(args.allow_target.is_empty()||args.allow_target.iter().any(|a|p.target.starts_with(a)))).take(monitor_protocol::MAX_PROBES).collect();
                                    tasks.retain(|_,(old,handle)|{let keep=probes.iter().any(|p|p==old);if !keep{handle.abort();}keep});
                                    for spec in probes {if !tasks.contains_key(&spec.id){let tx=result_tx.clone();let probe=spec.clone();let gate=probe_gate.clone();let handle=tokio::spawn(async move{let mut tick=tokio::time::interval(Duration::from_secs(probe.interval));tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);loop{tick.tick().await;let Ok(_permit)=gate.acquire().await else{break;};let result=probe::run(&probe).await;if tx.send(result).await.is_err(){break;}}});tasks.insert(spec.id.clone(),(spec,handle));}}
                                }}}
                            },
                            Some(Ok(Message::Ping(data)))=>{last_frame=Instant::now();if write(&mut socket,Message::Pong(data)).await.is_err(){break;}},
                            Some(Ok(Message::Pong(_)))=>last_frame=Instant::now(),
                            None|Some(Err(_))|Some(Ok(Message::Close(_)))=>break,_=>{}
                        },
                        _=heartbeat.tick()=>{if last_frame.elapsed()>Duration::from_secs(90)||write(&mut socket,Message::Ping(vec![].into())).await.is_err(){break;}},
                        _=tokio::signal::ctrl_c()=>{let _=write(&mut socket,Message::Close(None)).await;for (_,(_,task)) in tasks{task.abort();}return Ok(());}
                    }
                }
                for (_, (_, task)) in tasks {
                    task.abort();
                }
                tracing::warn!("Hub 连接中断，内存窗口持续采集，正在重连");
            }
        } else {
            tracing::warn!("Hub 连接失败，将重试");
        }
        tokio::select! {_=tokio::time::sleep(Duration::from_secs(retry))=>{},_=tokio::signal::ctrl_c()=>return Ok(())}
        retry = (retry * 2).min(30);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn secrets_never_travel_in_the_url_and_remote_plaintext_requires_opt_in() {
        assert!(endpoint("http://example.com", false).is_err());
        assert!(endpoint("http://127.0.0.1:1234", false).is_ok());
        assert_eq!(
            endpoint("https://example.com?token=secret", false).unwrap(),
            "wss://example.com/api/v1/agent/ws"
        );
    }
}
