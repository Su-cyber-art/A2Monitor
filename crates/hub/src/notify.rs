use crate::{db::calendar_period, runtime::Shared};
use anyhow::{ensure, Result};
use chrono::TimeZone;
use monitor_protocol::{Event, NotifySettings};
use rusqlite::params;
use serde_json::{json, Value};

fn channels(settings: &NotifySettings) -> Vec<&'static str> {
    let mut names = vec![];
    if !settings.telegram_token.is_empty() && !settings.telegram_chat_id.is_empty() {
        names.push("telegram");
    }
    if !settings.webhook_url.is_empty() {
        names.push("webhook");
    }
    names
}
async fn send_one(settings: &NotifySettings, channel: &str, payload: &Value) -> Result<()> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    let response = if channel == "telegram" {
        ensure!(
            !settings.telegram_token.is_empty() && !settings.telegram_chat_id.is_empty(),
            "Telegram 尚未配置"
        );
        client.post(format!("https://api.telegram.org/bot{}/sendMessage",settings.telegram_token)).json(&json!({"chat_id":settings.telegram_chat_id,"text":format!("{}\n{}",payload["title"].as_str().unwrap_or("Monitor"),payload["message"].as_str().unwrap_or(""))})).send().await?
    } else {
        ensure!(!settings.webhook_url.is_empty(), "Webhook 尚未配置");
        let mut request = client.post(&settings.webhook_url).json(payload);
        for (name, value) in &settings.webhook_headers {
            request = request.header(name, value);
        }
        request.send().await?
    };
    ensure!(
        response.status().is_success(),
        "通知渠道返回 HTTP {}",
        response.status().as_u16()
    );
    Ok(())
}
pub async fn send(settings: &NotifySettings, payload: Value) -> Result<()> {
    let names = channels(settings);
    ensure!(!names.is_empty(), "请先配置通知渠道");
    for name in names {
        send_one(settings, name, &payload).await?;
    }
    Ok(())
}
async fn enqueue(app: &Shared, event: &Event, kind: &str) -> Result<()> {
    let settings = app.settings.read().await.notify.clone();
    let names = channels(&settings);
    for channel in names {
        let id = format!("{}:{kind}:{channel}", event.id);
        let event_id = event.id.clone();
        let data=json!({"channel":channel,"payload":{"event":kind,"title":event.title,"message":event.detail,"time":event.time}}).to_string();
        app.db
            .call(move |c| {
                c.execute(
                    "INSERT OR IGNORE INTO notification(id,event_id,data,next_at) VALUES(?,?,?,?)",
                    params![id, event_id, data, monitor_protocol::now_ms()],
                )?;
                Ok(())
            })
            .await?;
    }
    Ok(())
}
pub async fn worker(app: Shared) {
    let mut tick = tokio::time::interval(std::time::Duration::from_secs(15));
    loop {
        tick.tick().await;
        if let Err(e) = cycle(&app).await {
            tracing::error!("通知检查失败: {e}");
        }
    }
}
async fn cycle(app: &Shared) -> Result<()> {
    let settings = app.settings.read().await.clone();
    let now = monitor_protocol::now_ms();
    let nodes = app.db.nodes().await?;
    for event in app
        .db
        .events()
        .await?
        .into_iter()
        .filter(|e| e.kind == "offline")
    {
        let enabled = event
            .node_ids
            .iter()
            .any(|id| nodes.iter().any(|n| n.id == *id && n.config.notify));
        if !enabled {
            continue;
        }
        if event.resolved_at.is_none()
            && now - event.time >= settings.offline_grace_seconds as i64 * 1000
        {
            enqueue(app, &event, "offline").await?;
        }
        if let Some(resolved) = event.resolved_at {
            let key = format!("{}:offline:%", event.id);
            let delivered = app
                .db
                .call(move |c| {
                    Ok(c.query_row(
                        "SELECT COUNT(*) FROM notification WHERE id LIKE ? AND delivered=1",
                        [key],
                        |r| r.get::<_, i64>(0),
                    )? > 0)
                })
                .await?;
            if delivered {
                let recovery = Event {
                    title: "监控连接已恢复".into(),
                    detail: "已发送离线通知的节点重新连接，指标恢复采集。".into(),
                    time: resolved,
                    ..event
                };
                enqueue(app, &recovery, "online").await?;
            }
        }
    }
    for node in &nodes {
        let traffic = app
            .db
            .traffic(
                node.id.clone(),
                settings.timezone.clone(),
                node.config.traffic_reset_day,
            )
            .await?;
        if node.config.traffic_limit > 0 {
            for threshold in [settings.traffic_threshold, 100] {
                let used = traffic.used(&node.config.traffic_mode);
                if used as f64 >= node.config.traffic_limit as f64 * threshold as f64 / 100.0 {
                    let key = format!(
                        "traffic-alert:{}:{}:{threshold}",
                        node.id, traffic.month_start
                    );
                    if app.db.get(&key).await?.is_none() {
                        let id=app.event(vec![node.id.clone()],"traffic","warning",format!("{} 的本期流量达到 {threshold}%",node.config.name),format!("已用 {:.2} GiB，本期额度 {:.2} GiB。",used as f64/1073741824.,node.config.traffic_limit as f64/1073741824.),json!({"used":used,"limit":node.config.traffic_limit,"threshold":threshold})).await?;
                        app.db.set(&key, id.clone()).await?;
                        if let Some(e) = app.db.events().await?.into_iter().find(|e| e.id == id) {
                            enqueue(app, &e, "traffic").await?;
                        }
                    }
                }
            }
        }
        if let Some(date) = node
            .config
            .expires_at
            .as_ref()
            .and_then(|d| chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").ok())
        {
            let (today, _, _) = calendar_period(now, &settings.timezone, 1);
            let days = (date - today).num_days();
            let timezone: chrono_tz::Tz =
                settings.timezone.parse().unwrap_or(chrono_tz::Asia::Taipei);
            let local = chrono::Utc
                .timestamp_millis_opt(now)
                .single()
                .unwrap()
                .with_timezone(&timezone);
            use chrono::Timelike;
            if (0..=7).contains(&days) && local.hour() >= 9 {
                let key = format!("expiry:{}:{today}", node.id);
                if app.db.get(&key).await?.is_none() {
                    let id = app
                        .event(
                            vec![node.id.clone()],
                            "expiry",
                            "info",
                            format!("{} 将在 {days} 天后到期", node.config.name),
                            format!("到期日为 {date}。"),
                            json!({"expires_at":date.to_string(),"days":days}),
                        )
                        .await?;
                    app.db.set(&key, id.clone()).await?;
                    if let Some(e) = app.db.events().await?.into_iter().find(|e| e.id == id) {
                        enqueue(app, &e, "expiry").await?;
                    }
                }
            }
            if days < 0
                && node.config.auto_renew
                && node.config.billing_days > 0
                && app
                    .live
                    .read()
                    .await
                    .get(&node.id)
                    .is_some_and(|l| l.connected)
            {
                let mut input = node.config.clone();
                let step = chrono::Duration::days(input.billing_days as i64);
                let mut next = date;
                for _ in 0..10000 {
                    if next >= today {
                        break;
                    }
                    next += step;
                }
                input.expires_at = Some(next.to_string());
                app.db.edit_node(node.id.clone(), input).await?;
                let id = app
                    .event(
                        vec![node.id.clone()],
                        "renewal",
                        "info",
                        format!("{} 已顺延到期日", node.config.name),
                        format!("节点仍在线，到期日由 {date} 顺延为 {next}。"),
                        json!({"old":date.to_string(),"new":next.to_string()}),
                    )
                    .await?;
                if let Some(e) = app.db.events().await?.into_iter().find(|e| e.id == id) {
                    enqueue(app, &e, "renew").await?;
                }
            }
        }
    }
    let pending=app.db.call(move|c|{let mut q=c.prepare("SELECT id,data,attempt FROM notification WHERE delivered=0 AND next_at<=? LIMIT 20")?;let rows=q.query_map([now],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,i64>(2)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;Ok(rows)}).await?;
    for (id, data, attempt) in pending {
        let data: Value = serde_json::from_str(&data)?;
        let result = send_one(
            &settings.notify,
            data["channel"].as_str().unwrap_or("webhook"),
            &data["payload"],
        )
        .await;
        let delivered = if result.is_ok() {
            1
        } else if attempt >= 2 {
            -1
        } else {
            0
        };
        if result.is_err() {
            tracing::warn!("通知发送失败，将按渠道重试；attempt={attempt}");
        }
        app.db
            .call(move |c| {
                c.execute(
                    "UPDATE notification SET delivered=?,attempt=attempt+1,next_at=? WHERE id=?",
                    params![
                        delivered,
                        monitor_protocol::now_ms() + 30000 * (attempt + 1),
                        id
                    ],
                )?;
                Ok(())
            })
            .await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{db::Db, runtime::App};
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    };

    #[tokio::test]
    async fn webhook_retries_deduplicates_and_sends_recovery_only_after_delivery() {
        let calls = Arc::new(AtomicUsize::new(0));
        let received = Arc::new(Mutex::new(Vec::<Value>::new()));
        let handler_calls = calls.clone();
        let handler_received = received.clone();
        let router = axum::Router::new().route(
            "/hook",
            axum::routing::post(move |axum::Json(payload): axum::Json<Value>| {
                let calls = handler_calls.clone();
                let received = handler_received.clone();
                async move {
                    received.lock().unwrap().push(payload);
                    if calls.fetch_add(1, Ordering::SeqCst) == 0 {
                        axum::http::StatusCode::SERVICE_UNAVAILABLE
                    } else {
                        axum::http::StatusCode::NO_CONTENT
                    }
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/hook", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(&dir.path().join("test.db")).unwrap();
        db.add_node(
            "node".into(),
            monitor_protocol::NodeInput {
                name: "Webhook fixture".into(),
                notify: true,
                ..Default::default()
            },
            "hash".into(),
        )
        .await
        .unwrap();
        let app = App::new(db, dir.path().into(), dir.path().into(), false)
            .await
            .unwrap();
        app.settings.write().await.notify.webhook_url = url;
        app.settings.write().await.offline_grace_seconds = 10;
        let event = Event {
            id: "offline-fixture".into(),
            node_ids: vec!["node".into()],
            kind: "offline".into(),
            severity: "warning".into(),
            title: "Offline".into(),
            detail: "Monitoring link interrupted".into(),
            time: monitor_protocol::now_ms() - 20000,
            resolved_at: None,
            evidence: json!({}),
        };
        app.db.add_event(event).await.unwrap();
        cycle(&app).await.unwrap();
        // Enqueued records can become due just after the cycle's time snapshot.
        cycle(&app).await.unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        cycle(&app).await.unwrap();
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "retry must respect its delay"
        );
        app.db
            .call(|c| {
                c.execute("UPDATE notification SET next_at=0", [])?;
                Ok(())
            })
            .await
            .unwrap();
        cycle(&app).await.unwrap();
        cycle(&app).await.unwrap();
        assert_eq!(
            calls.load(Ordering::SeqCst),
            2,
            "delivered offline alert must not repeat"
        );
        app.db.resolve("offline-fixture".into()).await.unwrap();
        cycle(&app).await.unwrap();
        cycle(&app).await.unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 3);
        assert_eq!(received.lock().unwrap()[2]["event"], "online");
        // A brief outage resolved during the grace period should stay quiet.
        app.db
            .add_event(Event {
                id: "brief-fixture".into(),
                node_ids: vec!["node".into()],
                kind: "offline".into(),
                severity: "warning".into(),
                title: "Brief".into(),
                detail: String::new(),
                time: monitor_protocol::now_ms() - 1000,
                resolved_at: Some(monitor_protocol::now_ms()),
                evidence: json!({}),
            })
            .await
            .unwrap();
        cycle(&app).await.unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 3);
        task.abort();
    }
}
