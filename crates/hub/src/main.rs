mod api;
mod auth;
mod compat;
mod compat_metrics;
mod db;
mod error;
#[cfg(test)]
mod integration;
mod notify;
mod runtime;
mod themes;
use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;

#[derive(Parser)]
#[command(version, about = "Monitor · 只读服务器监控与故障证据")]
struct Args {
    #[arg(long, env = "MONITOR_LISTEN", default_value = "127.0.0.1:28080")]
    listen: std::net::SocketAddr,
    #[arg(long, env = "MONITOR_DATA", default_value = "data")]
    data: PathBuf,
    #[arg(long, env = "MONITOR_FRONTEND", default_value = "web/dist")]
    frontend: PathBuf,
    #[arg(long, env = "MONITOR_ADMIN_PASSWORD", hide_env_values = true)]
    admin_password: Option<String>,
    #[arg(long)]
    reset_password: bool,
    #[arg(long, help = "生成独立的演示数据库，页面明确标记为演示")]
    demo: bool,
}
#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    let args = Args::parse();
    std::fs::create_dir_all(&args.data)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&args.data, std::fs::Permissions::from_mode(0o700))?;
    }
    let path = args
        .data
        .join(if args.demo { "demo.db" } else { "monitor.db" });
    let db = db::Db::open(&path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    }
    if args.reset_password || db.get("password_hash").await?.is_none() {
        let password = args
            .admin_password
            .unwrap_or_else(|| auth::token()[..24].to_owned());
        anyhow::ensure!(password.len() >= 12, "管理员密码至少需要 12 字符");
        db.set("password_hash", auth::password_hash(&password)?)
            .await?;
        db.remove_session(None).await?;
        println!("Monitor initial administrator password: {password}");
        if args.reset_password {
            return Ok(());
        }
    }
    let app = runtime::App::new(db, args.data, args.frontend, args.demo).await?;
    if args.demo {
        seed_demo(&app).await?;
        tokio::spawn(demo_loop(app.clone()));
    }
    tokio::spawn(notify::worker(app.clone()));
    let maintenance = app.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(3600));
        loop {
            interval.tick().await;
            let days = maintenance.settings.read().await.history_days;
            if let Err(e) = maintenance.db.maintain(days).await {
                tracing::error!("历史数据维护失败: {e}");
            }
        }
    });
    let router = api::router(app.clone());
    let listener = tokio::net::TcpListener::bind(args.listen).await?;
    tracing::info!(address=%args.listen,"Monitor Hub ready");
    axum::serve(
        listener,
        router.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(async move {
        let _ = tokio::signal::ctrl_c().await;
        let ids = app.live.read().await.keys().cloned().collect::<Vec<_>>();
        for id in ids {
            app.disconnect(&id).await;
        }
    })
    .await?;
    Ok(())
}
async fn seed_demo(app: &runtime::Shared) -> Result<()> {
    if !app.db.nodes().await?.is_empty() {
        return Ok(());
    }
    for (i, (name, region, group)) in [
        ("Singapore · Compute 01", "SG", "Production"),
        ("Tokyo · Edge 02", "JP", "Production"),
        ("Frankfurt · Archive 03", "DE", "Storage"),
        ("Taipei · Lab 04", "TW", "Lab"),
    ]
    .into_iter()
    .enumerate()
    {
        let id = format!("demo-node-{}", i + 1);
        let input = monitor_protocol::NodeInput {
            name: name.into(),
            group: group.into(),
            region: region.into(),
            public: true,
            price: 12.0 + i as f64 * 4.,
            currency: "USD".into(),
            billing_days: 30,
            traffic_limit: 1099511627776,
            traffic_reset_day: 1,
            traffic_mode: "sum".into(),
            expires_at: Some(
                (chrono::Utc::now() + chrono::Duration::days(12 + i as i64 * 3))
                    .date_naive()
                    .to_string(),
            ),
            ..Default::default()
        };
        app.db
            .add_node(id.clone(), input, auth::hash(&auth::token()))
            .await?;
        app.db
            .facts(
                id.clone(),
                monitor_protocol::Facts {
                    hostname: format!("demo-{i}"),
                    os: "Debian GNU/Linux 13".into(),
                    arch: "x86_64".into(),
                    cpu_name: "AMD EPYC · Demo".into(),
                    cpu_cores: 4,
                    mem_total: 4294967296,
                    disk_total: 85899345920,
                    platform: "demo".into(),
                    agent_version: env!("CARGO_PKG_VERSION").into(),
                    interval: 5,
                    ..Default::default()
                },
            )
            .await?;
        let now = monitor_protocol::now_ms();
        app.db
            .call(move |c| {
                for minute in 0..360i64 {
                    let time = (now / 60000 - 360 + minute) * 60000;
                    let wave = (minute as f64 / 18. + i as f64).sin();
                    let sample = monitor_protocol::Sample {
                        time,
                        boot_id: "demo".into(),
                        cpu: 18. + wave * 10. + i as f64 * 5.,
                        mem_total: 4294967296,
                        mem_used: (1400000000. + wave * 100000000.) as u64,
                        disk_total: 85899345920,
                        disk_used: 28000000000,
                        net_rx: (2000000. + wave * 1600000.) as u64,
                        net_tx: (1000000. + wave * 500000.) as u64,
                        load: [0.3, 0.4, 0.3],
                        ..Default::default()
                    };
                    c.execute(
                        "INSERT OR REPLACE INTO metric VALUES(?,?,?,?,?,?,?)",
                        rusqlite::params![
                            id,
                            time,
                            db::public_sample(&sample).to_string(),
                            12,
                            sample.cpu,
                            sample.net_rx,
                            sample.net_tx
                        ],
                    )?;
                }
                Ok(())
            })
            .await?;
    }
    Ok(())
}
async fn demo_loop(app: runtime::Shared) {
    let mut tick = tokio::time::interval(std::time::Duration::from_secs(5));
    let mut seq = 1u64;
    loop {
        tick.tick().await;
        seq += 1;
        let Ok(nodes) = app.db.nodes().await else {
            continue;
        };
        for (i, node) in nodes.into_iter().enumerate().filter(|(i, _)| *i < 3) {
            let (tx, mut rx) = tokio::sync::mpsc::channel(16);
            tokio::spawn(async move { while rx.recv().await.is_some() {} });
            if !app.live.read().await.contains_key(&node.id) {
                app.live.write().await.insert(
                    node.id.clone(),
                    runtime::Live {
                        tag: 0,
                        connected: true,
                        sender: tx,
                        sample: None,
                        received: 0,
                        ring: std::collections::VecDeque::new(),
                        thresholds: std::collections::BTreeMap::new(),
                    },
                );
            }
            let wave = (seq as f64 / 10. + i as f64).sin();
            let sample = monitor_protocol::Sample {
                time: monitor_protocol::now_ms(),
                seq,
                boot_id: format!("demo-{i}"),
                cpu: 18. + wave * 10. + i as f64 * 5.,
                mem_total: 4294967296,
                mem_used: (1400000000. + wave * 100000000.) as u64,
                disk_total: 85899345920,
                disk_used: 28000000000,
                net_rx: (2000000. + wave * 1600000.) as u64,
                net_tx: (1000000. + wave * 500000.) as u64,
                net_rx_total: seq * 10000000,
                net_tx_total: seq * 4000000,
                uptime: 86400 + seq * 5,
                load: [0.3, 0.4, 0.3],
                pressure: monitor_protocol::Pressure {
                    cpu: Some(0.3),
                    memory: Some(0.0),
                    io: Some(0.1),
                },
                available: vec![
                    "cpu".into(),
                    "memory".into(),
                    "disk".into(),
                    "network".into(),
                ],
                ..Default::default()
            };
            let _ = app.ingest(&node, sample).await;
        }
    }
}
