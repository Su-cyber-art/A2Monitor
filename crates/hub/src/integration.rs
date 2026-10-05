use crate::{
    api, auth,
    db::Db,
    runtime::{App, Shared},
};
use futures_util::{SinkExt, StreamExt};
use monitor_protocol::{Facts, Hello, Rpc, Sample, PROTOCOL_VERSION};
use serde_json::{json, Value};
use std::{net::SocketAddr, time::Duration};

struct Server {
    url: String,
    app: Shared,
    task: tokio::task::JoinHandle<()>,
    _dir: tempfile::TempDir,
}
impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}
async fn server() -> Server {
    let dir = tempfile::tempdir().unwrap();
    let db = Db::open(&dir.path().join("test.db")).unwrap();
    db.set(
        "password_hash",
        auth::password_hash("monitor-integration-password").unwrap(),
    )
    .await
    .unwrap();
    let frontend = dir.path().join("frontend");
    std::fs::create_dir_all(&frontend).unwrap();
    std::fs::write(frontend.join("index.html"), "<h1>Native admin host</h1>").unwrap();
    let app = App::new(db, dir.path().into(), frontend, false)
        .await
        .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let router = api::router(app.clone());
    let task = tokio::spawn(async move {
        axum::serve(
            listener,
            router.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap();
    });
    Server {
        url,
        app,
        task,
        _dir: dir,
    }
}
async fn login(server: &Server) -> String {
    let response = reqwest::Client::new()
        .post(format!("{}/api/v1/auth/login", server.url))
        .json(&json!({"password":"monitor-integration-password"}))
        .send()
        .await
        .unwrap();
    assert!(response.status().is_success());
    response.headers()["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned()
}
async fn add(server: &Server, cookie: &str, name: &str, public: bool) -> Value {
    let response = reqwest::Client::new()
        .post(format!("{}/api/v1/admin/nodes", server.url))
        .header("Cookie", cookie)
        .json(&json!({"name":name,"public":public,"remark":"private-note"}))
        .send()
        .await
        .unwrap();
    assert!(response.status().is_success());
    response.json().await.unwrap()
}
async fn wait_sample(server: &Server, id: &str) {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if server
                .app
                .live
                .read()
                .await
                .get(id)
                .is_some_and(|l| l.sample.is_some())
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn node_order_is_atomic_and_matches_public_theme_weights() {
    let server = server().await;
    let cookie = login(&server).await;
    let client = reqwest::Client::new();
    let mut ids = Vec::new();
    for name in ["A", "B", "C"] {
        ids.push(
            add(&server, &cookie, name, true).await["node"]["id"]
                .as_str()
                .unwrap()
                .to_owned(),
        );
    }
    let desired = vec![ids[2].clone(), ids[0].clone(), ids[1].clone()];
    client
        .post(format!("{}/api/v1/admin/nodes/order", server.url))
        .header("Cookie", &cookie)
        .json(&desired)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    let ordered = server.app.db.nodes().await.unwrap();
    assert_eq!(
        ordered.iter().map(|n| n.id.clone()).collect::<Vec<_>>(),
        desired
    );
    assert_eq!(
        ordered.iter().map(|n| n.alias).collect::<Vec<_>>(),
        vec![3, 1, 2]
    );
    let public: Value = client
        .get(format!("{}/api/nodes", server.url))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let public = public["data"].as_array().unwrap();
    assert_eq!(
        public
            .iter()
            .map(|n| n["uuid"].as_str().unwrap())
            .collect::<Vec<_>>(),
        desired.iter().map(String::as_str).collect::<Vec<_>>()
    );
    assert_eq!(
        public
            .iter()
            .map(|n| n["weight"].as_i64().unwrap())
            .collect::<Vec<_>>(),
        vec![0, 1, 2]
    );
    for invalid in [
        vec![ids[2].clone(), ids[2].clone(), ids[1].clone()],
        vec![ids[2].clone()],
        vec![ids[2].clone(), ids[0].clone(), "missing".into()],
    ] {
        assert_eq!(
            client
                .post(format!("{}/api/v1/admin/nodes/order", server.url))
                .header("Cookie", &cookie)
                .json(&invalid)
                .send()
                .await
                .unwrap()
                .status(),
            400
        );
        assert_eq!(
            server
                .app
                .db
                .nodes()
                .await
                .unwrap()
                .iter()
                .map(|n| n.id.clone())
                .collect::<Vec<_>>(),
            desired
        );
    }
}

#[tokio::test]
async fn real_http_and_websocket_flow_protects_private_nodes_and_rotated_tokens() {
    let server = server().await;
    let cookie = login(&server).await;
    let client = reqwest::Client::new();
    let public = add(&server, &cookie, "Public node", true).await;
    let hidden = add(&server, &cookie, "Hidden node", false).await;
    let id = public["node"]["id"].as_str().unwrap();
    let token = public["token"].as_str().unwrap();
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    let mut request = format!("{}/api/v1/agent/ws", server.url.replace("http://", "ws://"))
        .into_client_request()
        .unwrap();
    request
        .headers_mut()
        .insert("Authorization", format!("Bearer {token}").parse().unwrap());
    let (mut socket, _) = tokio_tungstenite::connect_async(request.clone())
        .await
        .unwrap();
    socket
        .send(tokio_tungstenite::tungstenite::Message::text(
            serde_json::to_string(&Rpc::notification(
                "hello",
                Hello {
                    protocol: PROTOCOL_VERSION,
                    facts: Facts {
                        hostname: "private-host".into(),
                        ipv4: "192.0.2.55".into(),
                        interval: 5,
                        ..Default::default()
                    },
                },
            ))
            .unwrap(),
        ))
        .await
        .unwrap();
    let sample = Sample {
        time: monitor_protocol::now_ms(),
        boot_id: "test-boot".into(),
        cpu: 32.,
        mem_total: 1024,
        mem_used: 512,
        net_rx_total: 1000,
        available: vec!["cpu".into(), "memory".into(), "network".into()],
        ..Default::default()
    };
    socket
        .send(tokio_tungstenite::tungstenite::Message::text(
            serde_json::to_string(&Rpc::notification("report", sample)).unwrap(),
        ))
        .await
        .unwrap();
    wait_sample(&server, id).await;
    let body: Value = client
        .get(format!("{}/api/v1/nodes", server.url))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(body.as_array().unwrap().len(), 1);
    assert_eq!(body[0]["metrics"]["cpu"], 32.);
    let serialized = body.to_string();
    for private in [
        "private-host",
        "192.0.2.55",
        "private-note",
        token,
        "net_rx_total",
        "boot_id",
    ] {
        assert!(!serialized.contains(private), "leaked {private}");
    }
    assert_eq!(
        client
            .get(format!(
                "{}/api/v1/nodes/{}/history",
                server.url,
                hidden["node"]["id"].as_str().unwrap()
            ))
            .send()
            .await
            .unwrap()
            .status(),
        404
    );
    assert_eq!(
        client
            .get(format!("{}/api/v1/admin/events", server.url))
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    let compatibility: Value = client
        .post(format!("{}/api/rpc2", server.url))
        .json(&json!({"jsonrpc":"2.0","id":7,"method":"common:getNodesLatestStatus","params":{}}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(compatibility["result"][id]["cpu"], 32.);
    for (method, params) in [
        ("rpc.ping", json!({})),
        ("common:getBackendVersion", json!({})),
        ("common:getNodeRecentStatus", json!({"uuid":id})),
        (
            "common:getRecords",
            json!({"type":"load","uuid":id,"hours":1}),
        ),
    ] {
        let response: Value = client
            .post(format!("{}/api/rpc2", server.url))
            .json(&json!({"jsonrpc":"2.0","id":8,"method":method,"params":params}))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert!(response.get("error").is_none(), "{method}: {response}");
        if method == "common:getNodeRecentStatus" {
            assert_eq!(response["result"]["count"], 1);
            assert_eq!(response["result"]["records"][0]["cpu"], 32.);
        }
    }
    assert!(compatibility["result"]
        .get(hidden["node"]["id"].as_str().unwrap())
        .is_none());
    let rotate = client
        .post(format!("{}/api/v1/admin/nodes/{id}/token", server.url))
        .header("Cookie", &cookie)
        .json(&json!({}))
        .send()
        .await
        .unwrap();
    assert!(rotate.status().is_success());
    let closed = tokio::time::timeout(Duration::from_secs(5), async {
        while let Some(frame) = socket.next().await {
            if matches!(frame, Ok(tokio_tungstenite::tungstenite::Message::Close(_))) {
                return true;
            }
        }
        true
    })
    .await
    .unwrap();
    assert!(closed);
    let rejected = tokio_tungstenite::connect_async(request).await;
    assert!(rejected.is_err());
}

#[tokio::test]
async fn csrf_login_limits_and_backup_restore_are_enforced_end_to_end() {
    let server = server().await;
    let client = reqwest::Client::new();
    let cookie = login(&server).await;
    add(&server, &cookie, "Backup node", true).await;
    let forged = client
        .post(format!("{}/api/v1/admin/nodes", server.url))
        .header("Cookie", &cookie)
        .header("Origin", "https://evil.example")
        .json(&json!({"name":"forged"}))
        .send()
        .await
        .unwrap();
    assert_eq!(forged.status(), 403);
    let bytes = client
        .get(format!("{}/api/v1/admin/database/backup", server.url))
        .header("Cookie", &cookie)
        .send()
        .await
        .unwrap()
        .bytes()
        .await
        .unwrap();
    assert!(bytes.starts_with(b"SQLite format 3"));
    add(&server, &cookie, "Will disappear on restore", true).await;
    let form = reqwest::multipart::Form::new().part(
        "file",
        reqwest::multipart::Part::bytes(bytes.to_vec()).file_name("backup.db"),
    );
    let restored = client
        .post(format!("{}/api/v1/admin/database/restore", server.url))
        .header("Cookie", &cookie)
        .multipart(form)
        .send()
        .await
        .unwrap();
    assert!(
        restored.status().is_success(),
        "{}",
        restored.text().await.unwrap()
    );
    assert_eq!(server.app.db.nodes().await.unwrap().len(), 1);
    assert_eq!(
        client
            .get(format!("{}/api/v1/admin/settings", server.url))
            .header("Cookie", &cookie)
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    for _ in 0..5 {
        assert_eq!(
            client
                .post(format!("{}/api/v1/auth/login", server.url))
                .json(&json!({"password":"wrong-password"}))
                .send()
                .await
                .unwrap()
                .status(),
            401
        );
    }
    assert_eq!(
        client
            .post(format!("{}/api/v1/auth/login", server.url))
            .json(&json!({"password":"monitor-integration-password"}))
            .send()
            .await
            .unwrap()
            .status(),
        429
    );
}

#[tokio::test]
async fn pressure_events_preserve_replay_without_private_identity() {
    let server = server().await;
    let cookie = login(&server).await;
    let created = add(&server, &cookie, "pressure node", true).await;
    let id = created["node"]["id"].as_str().unwrap().to_owned();
    let node = server.app.db.node(&id).await.unwrap().unwrap();
    let (sender, _) = tokio::sync::mpsc::channel(16);
    server.app.live.write().await.insert(
        id.clone(),
        crate::runtime::Live {
            tag: 1,
            connected: true,
            sender,
            sample: None,
            received: 0,
            ring: Default::default(),
            thresholds: Default::default(),
        },
    );
    let now = monitor_protocol::now_ms();
    for offset in [0, 10000, 31000] {
        server
            .app
            .ingest(
                &node,
                Sample {
                    time: now + offset,
                    boot_id: "boot-private".into(),
                    pressure: monitor_protocol::Pressure {
                        io: Some(45.),
                        ..Default::default()
                    },
                    ..Default::default()
                },
            )
            .await
            .unwrap();
    }
    let events = server.app.db.events().await.unwrap();
    let event = events.iter().find(|e| e.kind == "io_pressure").unwrap();
    let replay = server.app.db.replay(event.id.clone()).await.unwrap();
    assert!(replay.len() >= 3);
    assert!(!serde_json::to_string(&replay)
        .unwrap()
        .contains("boot-private"));
    server
        .app
        .ingest(
            &node,
            Sample {
                time: now + 32000,
                boot_id: "boot-private".into(),
                pressure: monitor_protocol::Pressure {
                    io: Some(0.),
                    ..Default::default()
                },
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert!(server
        .app
        .db
        .events()
        .await
        .unwrap()
        .iter()
        .find(|e| e.id == event.id)
        .unwrap()
        .resolved_at
        .is_some());
    let exported = reqwest::Client::new()
        .get(format!(
            "{}/api/v1/admin/events/{}/export",
            server.url, event.id
        ))
        .header("Cookie", cookie)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(exported.contains("monitor-diagnostic/v1"));
    assert!(exported.contains("观察点 1"));
    for private in [id.as_str(), "boot-private", "pressure node", "private-note"] {
        assert!(!exported.contains(private), "diagnostic leaked {private}");
    }
}

#[tokio::test]
async fn imported_theme_serves_public_page_without_replacing_the_admin_host() {
    use std::io::Write;
    let server = server().await;
    let cookie = login(&server).await;
    let client = reqwest::Client::new();
    let mut archive = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    archive
        .start_file(
            "komari-theme.json",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
    archive
        .write_all(br#"{"short":"default","name":"Fixture"}"#)
        .unwrap();
    archive
        .start_file("dist/index.html", zip::write::SimpleFileOptions::default())
        .unwrap();
    archive
        .write_all(b"<!doctype html><h1>Fixture public theme</h1>")
        .unwrap();
    let bytes = archive.finish().unwrap().into_inner();
    let form = reqwest::multipart::Form::new().part(
        "file",
        reqwest::multipart::Part::bytes(bytes).file_name("fixture.zip"),
    );
    let imported = client
        .post(format!("{}/api/v1/admin/themes", server.url))
        .header("Cookie", &cookie)
        .multipart(form)
        .send()
        .await
        .unwrap();
    assert!(
        imported.status().is_success(),
        "{}",
        imported.text().await.unwrap()
    );
    client
        .post(format!(
            "{}/api/v1/admin/themes/komari-default/activate",
            server.url
        ))
        .header("Cookie", &cookie)
        .json(&json!({}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    assert!(client
        .get(&server.url)
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap()
        .contains("Fixture public theme"));
    assert!(client
        .get(format!("{}/admin", server.url))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap()
        .contains("Native admin host"));
    let unknown = client
        .get(format!("{}/api/not-a-real-endpoint", server.url))
        .send()
        .await
        .unwrap();
    assert_eq!(unknown.status(), 404);
    assert!(!unknown
        .text()
        .await
        .unwrap()
        .contains("Fixture public theme"));
    client
        .post(format!(
            "{}/api/admin/theme/settings?theme=default",
            server.url
        ))
        .header("Cookie", &cookie)
        .json(&json!({"accent":"green"}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    let config: Value = client
        .get(format!(
            "{}/api/v1/themes/komari-default/config",
            server.url
        ))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(config["accent"], "green");
}

#[tokio::test]
async fn metric_compatibility_preserves_units_peaks_loss_and_visibility() {
    use monitor_protocol::{Probe, ProbeKind, ProbeResult};
    let server = server().await;
    let cookie = login(&server).await;
    let node = add(&server, &cookie, "Visible metrics", true).await;
    let hidden = add(&server, &cookie, "Private metrics", false).await;
    let id = node["node"]["id"].as_str().unwrap();
    for cpu in [40., 80.] {
        server
            .app
            .db
            .ingest(
                id.into(),
                Sample {
                    time: monitor_protocol::now_ms(),
                    boot_id: "private-epoch".into(),
                    cpu,
                    available: vec!["cpu".into()],
                    ..Default::default()
                },
                "UTC".into(),
                1,
            )
            .await
            .unwrap();
    }
    server
        .app
        .db
        .save_probe(Probe {
            id: "dns-task".into(),
            name: "DNS test".into(),
            kind: ProbeKind::Dns,
            target: "private-target.example".into(),
            ..Default::default()
        })
        .await
        .unwrap();
    let now = monitor_protocol::now_ms();
    for (index, latency) in [Some(10.), Some(30.), None].into_iter().enumerate() {
        server
            .app
            .db
            .probe_result(
                id.into(),
                ProbeResult {
                    task_id: "dns-task".into(),
                    time: now - 5000 + index as i64,
                    success: latency.is_some(),
                    latency_ms: latency,
                    ..Default::default()
                },
            )
            .await
            .unwrap();
    }
    let client = reqwest::Client::new();
    let rpc = |method: &str, params: Value| {
        client
            .post(format!("{}/api/rpc2", server.url))
            .json(&json!({"jsonrpc":"2.0","id":1,"method":method,"params":params}))
            .send()
    };
    let metrics: Value = rpc(
        "public:queryMetrics",
        json!({"hours":1,"metric_keys":["cpu.usage"]}),
    )
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
    let series = metrics["result"]["series"].as_array().unwrap();
    assert_eq!(series.len(), 1);
    let observations = series[0]["points"].as_array().unwrap();
    let count: f64 = observations
        .iter()
        .map(|p| p["count"].as_f64().unwrap())
        .sum();
    let mean: f64 = observations
        .iter()
        .map(|p| p["value"].as_f64().unwrap() * p["count"].as_f64().unwrap())
        .sum::<f64>()
        / count;
    assert_eq!(mean, 60.);
    assert_eq!(series[0]["unit"], "%");
    let peak: Value = rpc(
        "public:queryMetrics",
        json!({"hours":1,"metric_keys":["cpu.usage"],"aggregation":"max"}),
    )
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
    assert_eq!(
        peak["result"]["series"][0]["points"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["value"].as_f64().unwrap())
            .max_by(f64::total_cmp),
        Some(80.)
    );
    let unsupported: Value = rpc(
        "public:queryMetrics",
        json!({"metric_keys":["memory.used"],"aggregation":"max"}),
    )
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
    assert!(unsupported.get("error").is_some());
    let stats: Value = rpc(
        "public:getPingMetricStats",
        json!({"entity_id":id,"hours":1}),
    )
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
    let stat = &stats["result"]["stats"][0];
    assert_eq!(stat["type"], "dns");
    assert_eq!(stat["total"], 3);
    assert!((stat["loss"].as_f64().unwrap() - 100. / 3.).abs() < 1e-9);
    assert_eq!(stat["avg"], 20.);
    assert!(stat["latest"].is_null());
    let denied: Value = rpc(
        "public:queryMetrics",
        json!({"entity_id":hidden["node"]["id"]}),
    )
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
    assert!(denied.get("error").is_some());
    let legacy: Value = client
        .get(format!("{}/api/nodes/1/metrics?hours=1", server.url))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(legacy["probes"]["1"], "DNS test");
    assert!(legacy["ping"][0]["latency"].is_number());
    assert!(legacy["ping"][0]["band"].is_array());
    for value in [metrics, stats, legacy] {
        let text = value.to_string();
        assert!(!text.contains("private-target.example"));
        assert!(!text.contains("private-epoch"));
        assert!(!text.contains("private-note"));
    }
}

#[tokio::test]
async fn incomplete_backup_is_rejected_without_overwriting_the_live_database() {
    let server = server().await;
    let cookie = login(&server).await;
    add(&server, &cookie, "Preserved node", true).await;
    let path = server._dir.path().join("invalid.db");
    server.app.db.backup(path.clone()).await.unwrap();
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute("DROP TABLE notification", [])
        .unwrap();
    let form = reqwest::multipart::Form::new().part(
        "file",
        reqwest::multipart::Part::bytes(std::fs::read(path).unwrap()).file_name("invalid.db"),
    );
    let response = reqwest::Client::new()
        .post(format!("{}/api/v1/admin/database/restore", server.url))
        .header("Cookie", &cookie)
        .multipart(form)
        .send()
        .await
        .unwrap();
    assert!(!response.status().is_success());
    assert_eq!(
        server.app.db.nodes().await.unwrap()[0].config.name,
        "Preserved node"
    );
    assert!(
        auth::authed(
            &server.app,
            &[("cookie".parse().unwrap(), cookie.parse().unwrap())]
                .into_iter()
                .collect()
        )
        .await
    );
}

#[tokio::test]
async fn reconnect_backfill_attaches_to_the_outage_and_keeps_counter_identity_private() {
    use tokio_tungstenite::tungstenite::{client::IntoClientRequest, Message};
    let server = server().await;
    let created = add(&server, &login(&server).await, "Reconnect fixture", true).await;
    let id = created["node"]["id"].as_str().unwrap();
    let mut request = format!("{}/api/v1/agent/ws", server.url.replace("http://", "ws://"))
        .into_client_request()
        .unwrap();
    request.headers_mut().insert(
        "Authorization",
        format!("Bearer {}", created["token"].as_str().unwrap())
            .parse()
            .unwrap(),
    );
    let (mut first, _) = tokio_tungstenite::connect_async(request.clone())
        .await
        .unwrap();
    let hello = || {
        Message::text(
            serde_json::to_string(&Rpc::notification(
                "hello",
                Hello {
                    protocol: PROTOCOL_VERSION,
                    facts: Facts {
                        interval: 5,
                        ..Default::default()
                    },
                },
            ))
            .unwrap(),
        )
    };
    first.send(hello()).await.unwrap();
    first
        .send(Message::text(
            serde_json::to_string(&Rpc::notification(
                "report",
                Sample {
                    time: monitor_protocol::now_ms(),
                    boot_id: "private-boot".into(),
                    cpu: 20.,
                    ..Default::default()
                },
            ))
            .unwrap(),
        ))
        .await
        .unwrap();
    wait_sample(&server, id).await;
    first.close(None).await.unwrap();
    let event = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Some(event) = server
                .app
                .db
                .events()
                .await
                .unwrap()
                .into_iter()
                .find(|e| e.kind == "offline")
            {
                break event;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let (mut second, _) = tokio_tungstenite::connect_async(request).await.unwrap();
    second.send(hello()).await.unwrap();
    let time = monitor_protocol::now_ms() - 1000;
    second.send(Message::text(serde_json::to_string(&Rpc::notification("backfill", json!({
        "reason":"reconnect","samples":[Sample { time, cpu: 77., boot_id: "private-boot".into(), ..Default::default() }]
    }))).unwrap())).await.unwrap();
    let replay = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let replay = server.app.db.replay(event.id.clone()).await.unwrap();
            if replay.iter().any(|r| r["time"] == time && r["cpu"] == 77.) {
                break replay;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert!(!serde_json::to_string(&replay)
        .unwrap()
        .contains("private-boot"));
    assert!(server
        .app
        .db
        .events()
        .await
        .unwrap()
        .iter()
        .find(|e| e.id == event.id)
        .unwrap()
        .resolved_at
        .is_some());
}

#[tokio::test]
async fn network_correlation_requires_multiple_observers_and_resolves_after_recovery() {
    use monitor_protocol::{Probe, ProbeResult};
    let server = server().await;
    let cookie = login(&server).await;
    let first = add(&server, &cookie, "Observer A", true).await;
    let second = add(&server, &cookie, "Observer B", true).await;
    server
        .app
        .db
        .save_probe(Probe {
            id: "shared-target".into(),
            name: "Shared service".into(),
            target: "example.com:443".into(),
            ..Default::default()
        })
        .await
        .unwrap();
    let nodes = [first, second]
        .into_iter()
        .map(|n| n["node"]["id"].as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    for (index, id) in nodes.iter().enumerate() {
        let node = server.app.db.node(id).await.unwrap().unwrap();
        server
            .app
            .probe_result(
                &node,
                ProbeResult {
                    task_id: "shared-target".into(),
                    time: monitor_protocol::now_ms(),
                    success: false,
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        let count = server
            .app
            .db
            .events()
            .await
            .unwrap()
            .iter()
            .filter(|e| e.kind == "network_correlation")
            .count();
        assert_eq!(
            count, index,
            "a single failure must not imply a shared cause"
        );
    }
    let node = server.app.db.node(&nodes[0]).await.unwrap().unwrap();
    server
        .app
        .probe_result(
            &node,
            ProbeResult {
                task_id: "shared-target".into(),
                time: monitor_protocol::now_ms(),
                success: true,
                latency_ms: Some(5.),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let events = server.app.db.events().await.unwrap();
    let event = events
        .iter()
        .find(|e| e.kind == "network_correlation")
        .unwrap();
    assert_eq!(event.evidence["confidence"], "candidate");
    assert!(event.resolved_at.is_some());
}

#[tokio::test]
async fn bundled_default_is_the_only_builtin_and_admin_routes_remain_available() {
    let server = server().await;
    let root = server
        .app
        .frontend_dir
        .parent()
        .unwrap()
        .join("themes/emerald");
    std::fs::create_dir_all(root.join("dist")).unwrap();
    std::fs::write(
        root.join("komari-theme.json"),
        br#"{"short":"Emerald","name":"Emerald","configuration":{"data":[]}}"#,
    )
    .unwrap();
    std::fs::write(root.join("dist/index.html"), "<h1>Bundled Emerald</h1>").unwrap();
    assert_eq!(server.app.settings.read().await.theme, "default");
    assert_eq!(crate::themes::profile(&server.app).await, "komari");
    let client = reqwest::Client::new();
    assert!(client
        .get(&server.url)
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap()
        .contains("Bundled Emerald"));
    assert!(client
        .get(format!("{}/admin", server.url))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap()
        .contains("Native admin host"));
    let cookie = login(&server).await;
    let themes: Value = client
        .get(format!("{}/api/v1/admin/themes", server.url))
        .header("Cookie", &cookie)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(themes
        .as_array()
        .unwrap()
        .iter()
        .any(|t| t["id"] == "default" && t["builtin"] == true && t["profile"] == "komari"));
    assert_eq!(themes.as_array().unwrap().len(), 1);
    assert_eq!(themes[0]["name"], "默认主题");
    for id in ["default", "native"] {
        assert_eq!(
            client
                .delete(format!("{}/api/v1/admin/themes/{id}", server.url))
                .header("Cookie", &cookie)
                .send()
                .await
                .unwrap()
                .status(),
            400
        );
    }
    let defaults: Value = client
        .get(format!("{}/api/v1/themes/default/config", server.url))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(defaults["visitorInfoCardEnabled"], false);
    client
        .put(format!("{}/api/v1/admin/themes/default/config", server.url))
        .header("Cookie", &cookie)
        .json(&json!({"earthViewMode":"hide"}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    assert_eq!(
        crate::themes::config(&server.app, "default").await.unwrap()["earthViewMode"],
        "hide"
    );
    assert_eq!(
        client
            .post(format!(
                "{}/api/v1/admin/themes/native/activate",
                server.url
            ))
            .header("Cookie", &cookie)
            .json(&json!({}))
            .send()
            .await
            .unwrap()
            .status(),
        404
    );
    assert!(client
        .get(&server.url)
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap()
        .contains("Bundled Emerald"));
    assert_eq!(
        client
            .get(format!("{}/themes/%2e%2e/test.db", server.url))
            .send()
            .await
            .unwrap()
            .status(),
        404
    );
}
