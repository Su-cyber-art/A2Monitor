use crate::{
    auth, compat,
    db::{calendar_period, traffic_used},
    error::{ApiError, ApiResult},
    runtime::{self, Shared},
    themes,
};
use axum::{
    extract::{DefaultBodyLimit, Multipart, Path, Query, State, WebSocketUpgrade},
    http::{header, HeaderMap, StatusCode, Uri},
    middleware,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;

pub fn router(app: Shared) -> Router {
    let admin = Router::new()
        .route("/nodes", post(add_node))
        .route("/nodes/order", post(order_nodes))
        .route(
            "/nodes/{id}",
            axum::routing::put(edit_node).delete(delete_node),
        )
        .route("/nodes/{id}/token", post(rotate_token))
        .route("/events", get(events))
        .route("/events/{id}", get(event_detail))
        .route("/events/{id}/export", get(export_event))
        .route("/settings", get(settings).put(save_settings))
        .route("/password", post(change_password))
        .route("/sessions", get(sessions).delete(delete_sessions))
        .route("/sessions/{id}", axum::routing::delete(delete_session))
        .route("/probes", post(save_probe))
        .route(
            "/probes/{id}",
            axum::routing::put(edit_probe).delete(delete_probe),
        )
        .route("/themes", get(list_themes).post(import_theme))
        .route("/themes/{id}", axum::routing::delete(delete_theme))
        .route("/themes/{id}/activate", post(activate_theme))
        .route(
            "/themes/{id}/config",
            get(theme_config).put(save_theme_config),
        )
        .route("/database", get(database_info))
        .route("/database/backup", get(backup))
        .route(
            "/database/restore",
            post(restore).layer(DefaultBodyLimit::max(1024 * 1024 * 1024)),
        )
        .route("/database/vacuum", post(vacuum))
        .route("/notifications/test", post(test_notification))
        .route_layer(middleware::from_fn_with_state(app.clone(), auth::admin));
    Router::new()
        .route(
            "/install.sh",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/x-shellscript; charset=utf-8")],
                    include_str!("../../../scripts/install-agent.sh"),
                )
            }),
        )
        .route("/agent/{arch}", get(agent_binary))
        .route(
            "/health",
            get(|| async { Json(json!({"ok":true,"version":env!("CARGO_PKG_VERSION")})) }),
        )
        .route("/api/v1/me", get(me))
        .route("/api/v1/auth/login", post(auth::login))
        .route("/api/v1/auth/logout", post(auth::logout))
        .route("/api/v1/nodes", get(nodes))
        .route("/api/v1/nodes/{id}/history", get(history))
        .route("/api/v1/nodes/{id}/forecast", get(forecast))
        .route("/api/v1/probes", get(probes))
        .route("/api/v1/themes/{id}/config", get(public_theme_config))
        .route("/api/v1/network", get(network))
        .route("/api/v1/stream", get(stream))
        .route("/api/v1/agent/ws", get(agent_ws))
        .nest("/api/v1/admin", admin)
        .merge(compat::routes(app.clone()))
        .fallback(frontend)
        .layer(DefaultBodyLimit::max(34 * 1024 * 1024))
        .layer(tower_http::compression::CompressionLayer::new())
        .with_state(app)
}
pub async fn readable(app: &Shared, headers: &HeaderMap) -> ApiResult<bool> {
    let admin = auth::authed(app, headers).await;
    if !admin && !app.settings.read().await.public_site {
        return Err(ApiError::unauthorized());
    }
    Ok(admin)
}
async fn me(State(app): State<Shared>, headers: HeaderMap) -> ApiResult<Json<Value>> {
    let s = app.settings.read().await.clone();
    Ok(Json(
        json!({"authed":auth::authed(&app,&headers).await,"site_name":s.site_name,"description":s.description,"public_site":s.public_site,"history_days":s.history_days,"theme":s.theme,"timezone":s.timezone,"demo":app.demo,"version":env!("CARGO_PKG_VERSION")}),
    ))
}
async fn nodes(State(app): State<Shared>, headers: HeaderMap) -> ApiResult<Json<Value>> {
    let admin = readable(&app, &headers).await?;
    Ok(Json(json!(app.nodes(admin).await?)))
}
pub async fn require_node(
    app: &Shared,
    headers: &HeaderMap,
    id: &str,
) -> ApiResult<crate::db::Node> {
    let admin = readable(app, headers).await?;
    let node = app.db.node(id).await?.ok_or_else(ApiError::not_found)?;
    if !admin && !node.config.public {
        return Err(ApiError::not_found());
    }
    Ok(node)
}
#[derive(Deserialize)]
pub struct Window {
    pub hours: Option<u32>,
    pub points: Option<u32>,
}
async fn history(
    State(app): State<Shared>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(window): Query<Window>,
) -> ApiResult<Json<Value>> {
    let node = require_node(&app, &headers, &id).await?;
    let hours = window
        .hours
        .unwrap_or(6)
        .clamp(1, app.settings.read().await.history_days * 24);
    Ok(Json(
        app.db
            .history(node.id, hours, window.points.unwrap_or(240).clamp(60, 1440))
            .await?,
    ))
}
async fn add_node(
    State(app): State<Shared>,
    Json(mut input): Json<monitor_protocol::NodeInput>,
) -> ApiResult<Json<Value>> {
    input.validate().map_err(|e| ApiError::bad(e.to_string()))?;
    let token = auth::token();
    let node = app
        .db
        .add_node(uuid::Uuid::new_v4().to_string(), input, auth::hash(&token))
        .await?;
    Ok(Json(json!({"node":node,"token":token,"token_once":true})))
}
async fn edit_node(
    State(app): State<Shared>,
    Path(id): Path<String>,
    Json(mut input): Json<monitor_protocol::NodeInput>,
) -> ApiResult<Json<Value>> {
    input.validate().map_err(|e| ApiError::bad(e.to_string()))?;
    app.db.edit_node(id, input).await?;
    Ok(Json(json!({"ok":true})))
}
async fn delete_node(State(app): State<Shared>, Path(id): Path<String>) -> ApiResult<Json<Value>> {
    app.disconnect(&id).await;
    app.db.delete_node(id.clone()).await?;
    app.live.write().await.remove(&id);
    Ok(Json(json!({"ok":true})))
}
async fn rotate_token(State(app): State<Shared>, Path(id): Path<String>) -> ApiResult<Json<Value>> {
    let token = auth::token();
    app.db.rotate_token(id.clone(), auth::hash(&token)).await?;
    app.disconnect(&id).await;
    Ok(Json(json!({"token":token,"token_once":true})))
}
async fn order_nodes(
    State(app): State<Shared>,
    Json(ids): Json<Vec<String>>,
) -> ApiResult<Json<Value>> {
    if !app.db.reorder(ids).await? {
        return Err(ApiError::bad("节点列表已变化，请刷新后重新排序"));
    }
    Ok(Json(json!({"ok":true})))
}
async fn agent_ws(
    State(app): State<Shared>,
    headers: HeaderMap,
    upgrade: WebSocketUpgrade,
) -> ApiResult<Response> {
    let token = headers
        .get(header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "))
        .ok_or_else(ApiError::unauthorized)?;
    let token_hash = auth::hash(token);
    let node = app
        .db
        .token_node(token_hash.clone())
        .await?
        .ok_or_else(ApiError::unauthorized)?;
    Ok(upgrade
        .max_message_size(1024 * 1024)
        .max_frame_size(1024 * 1024)
        .on_upgrade(move |socket| runtime::agent_socket(app, node, token_hash, socket)))
}
async fn stream(
    State(app): State<Shared>,
    headers: HeaderMap,
    upgrade: WebSocketUpgrade,
) -> ApiResult<Response> {
    readable(&app, &headers).await?;
    if !auth::same_origin(&headers) {
        return Err(ApiError(StatusCode::FORBIDDEN, "请求来源无效".into()));
    }
    Ok(upgrade.on_upgrade(move |mut socket| async move {
        let mut tick = tokio::time::interval(std::time::Duration::from_secs(2));
        loop {
            tick.tick().await;
            let Ok(admin) = readable(&app, &headers).await else {
                break;
            };
            let Ok(nodes) = app.nodes(admin).await else {
                break;
            };
            if socket
                .send(axum::extract::ws::Message::Text(
                    json!({"nodes":nodes,"time":monitor_protocol::now_ms()})
                        .to_string()
                        .into(),
                ))
                .await
                .is_err()
            {
                break;
            }
        }
    }))
}
async fn events(State(app): State<Shared>) -> ApiResult<Json<Value>> {
    Ok(Json(json!(app.db.events().await?)))
}
async fn event_detail(State(app): State<Shared>, Path(id): Path<String>) -> ApiResult<Json<Value>> {
    let event = app
        .db
        .events()
        .await?
        .into_iter()
        .find(|e| e.id == id)
        .ok_or_else(ApiError::not_found)?;
    Ok(Json(
        json!({"event":event,"samples":app.db.replay(id).await?}),
    ))
}
async fn export_event(State(app): State<Shared>, Path(id): Path<String>) -> ApiResult<Response> {
    let event = app
        .db
        .events()
        .await?
        .into_iter()
        .find(|e| e.id == id)
        .ok_or_else(ApiError::not_found)?;
    let mut samples = app.db.replay(id.clone()).await?;
    let mut aliases = std::collections::BTreeMap::<String, String>::new();
    for sample in &mut samples {
        let original = sample["node_id"].as_str().unwrap_or_default().to_owned();
        let next = format!("观察点 {}", aliases.len() + 1);
        let label = aliases.entry(original).or_insert(next).clone();
        sample["node_id"] = json!(label);
    }
    let timezone = app.settings.read().await.timezone.clone();
    let payload = json!({"schema":"monitor-diagnostic/v1","timezone":timezone,"exported_at":monitor_protocol::now_ms(),"event":{"kind":event.kind,"severity":event.severity,"time":event.time,"resolved_at":event.resolved_at,"summary":"本报告记录故障类型与资源采样，观察点已经匿名化。关联结果用于排查，不代表已确认根因。"},"samples":samples});
    let escaped = payload.to_string().replace('<', "\\u003c");
    let html = include_str!("report.html").replace("__MONITOR_DATA__", &escaped);
    Ok((
        [
            (header::CONTENT_TYPE, "text/html; charset=utf-8"),
            (
                header::CONTENT_DISPOSITION,
                &format!("attachment; filename=monitor-event-{id}.html"),
            ),
        ],
        html,
    )
        .into_response())
}
async fn settings(State(app): State<Shared>) -> ApiResult<Json<Value>> {
    let mut s = serde_json::to_value(app.settings.read().await.clone())
        .map_err(|e| ApiError::from(anyhow::anyhow!(e)))?;
    s["notify"]["telegram_token_set"] = json!(!s["notify"]["telegram_token"]
        .as_str()
        .unwrap_or("")
        .is_empty());
    s["notify"]["telegram_token"] = json!("");
    s["notify"]["webhook_headers"] = json!({});
    Ok(Json(s))
}
async fn save_settings(
    State(app): State<Shared>,
    Json(mut settings): Json<monitor_protocol::Settings>,
) -> ApiResult<Json<Value>> {
    if !(1..=365).contains(&settings.history_days)
        || !(10..=3600).contains(&settings.offline_grace_seconds)
        || !(1..=100).contains(&settings.traffic_threshold)
        || settings.site_name.chars().count() > 80
        || settings.description.len() > 1000
        || settings.timezone.parse::<chrono_tz::Tz>().is_err()
    {
        return Err(ApiError::bad("站点设置的范围无效"));
    }
    if !settings.notify.webhook_url.is_empty() {
        let url = reqwest::Url::parse(&settings.notify.webhook_url)
            .map_err(|_| ApiError::bad("Webhook URL 无效"))?;
        if !matches!(url.scheme(), "http" | "https") {
            return Err(ApiError::bad("Webhook 必须使用 HTTP/HTTPS"));
        }
    }
    let old = app.settings.read().await.clone();
    if settings.notify.telegram_token.is_empty() {
        settings.notify.telegram_token = old.notify.telegram_token;
    }
    if settings.notify.webhook_headers.is_empty() {
        settings.notify.webhook_headers = old.notify.webhook_headers;
    }
    if !themes::list(&app)
        .await?
        .iter()
        .any(|t| t["id"] == settings.theme)
    {
        return Err(ApiError::bad("主题不存在"));
    }
    app.db.save_settings(settings.clone()).await?;
    *app.settings.write().await = settings;
    Ok(Json(json!({"ok":true})))
}
#[derive(Deserialize)]
struct NewPassword {
    password: String,
}
async fn change_password(
    State(app): State<Shared>,
    Json(input): Json<NewPassword>,
) -> ApiResult<Json<Value>> {
    if !(12..=1024).contains(&input.password.len()) {
        return Err(ApiError::bad("密码需要 12–1024 字符"));
    }
    let hash = tokio::task::spawn_blocking(move || auth::password_hash(&input.password))
        .await
        .map_err(|e| ApiError::from(anyhow::anyhow!(e)))??;
    app.db.set("password_hash", hash).await?;
    app.db.remove_session(None).await?;
    Ok(Json(json!({"ok":true,"relogin":true})))
}
async fn sessions(State(app): State<Shared>, headers: HeaderMap) -> ApiResult<Json<Value>> {
    let current = auth::session(&headers).map(|s| auth::hash(&s));
    let rows = app
        .db
        .sessions()
        .await?
        .into_iter()
        .map(|mut row| {
            row["current"] = json!(current.as_deref() == row["id"].as_str());
            row
        })
        .collect::<Vec<_>>();
    Ok(Json(json!(rows)))
}
async fn delete_session(
    State(app): State<Shared>,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    app.db.remove_session(Some(id)).await?;
    Ok(Json(json!({"ok":true})))
}
async fn delete_sessions(State(app): State<Shared>) -> ApiResult<Json<Value>> {
    app.db.remove_session(None).await?;
    Ok(Json(json!({"ok":true})))
}
async fn probes(State(app): State<Shared>, headers: HeaderMap) -> ApiResult<Json<Value>> {
    let admin = readable(&app, &headers).await?;
    let visible = app
        .db
        .nodes()
        .await?
        .into_iter()
        .filter(|n| admin || n.config.public)
        .map(|n| n.id)
        .collect::<Vec<_>>();
    let probes = app
        .db
        .probes()
        .await?
        .into_iter()
        .filter(|p| admin || visible.iter().any(|id| p.assigned(id)))
        .map(|p| {
            let mut v = serde_json::to_value(p).unwrap();
            if !admin {
                v.as_object_mut().unwrap().remove("target");
                v.as_object_mut().unwrap().remove("node_ids");
            }
            v
        })
        .collect::<Vec<_>>();
    Ok(Json(json!(probes)))
}
async fn save_probe(
    State(app): State<Shared>,
    Json(mut probe): Json<monitor_protocol::Probe>,
) -> ApiResult<Json<Value>> {
    probe.id = uuid::Uuid::new_v4().to_string();
    store_probe(&app, probe).await
}
async fn edit_probe(
    State(app): State<Shared>,
    Path(id): Path<String>,
    Json(mut probe): Json<monitor_protocol::Probe>,
) -> ApiResult<Json<Value>> {
    probe.id = id;
    store_probe(&app, probe).await
}
async fn store_probe(app: &Shared, probe: monitor_protocol::Probe) -> ApiResult<Json<Value>> {
    probe.validate().map_err(|e| ApiError::bad(e.to_string()))?;
    let existing = app.db.probes().await?;
    if existing.len() >= 64 && !existing.iter().any(|p| p.id == probe.id) {
        return Err(ApiError::bad("最多支持 64 个探测任务"));
    }
    if probe.kind == monitor_protocol::ProbeKind::Http {
        let url = reqwest::Url::parse(&probe.target).map_err(|_| ApiError::bad("HTTP 目标无效"))?;
        if !matches!(url.scheme(), "http" | "https") {
            return Err(ApiError::bad("HTTP 目标无效"));
        }
    } else if probe.kind != monitor_protocol::ProbeKind::Dns
        && !probe
            .target
            .rsplit_once(':')
            .is_some_and(|(_, p)| p.parse::<u16>().is_ok())
    {
        return Err(ApiError::bad("目标需要 host:port"));
    }
    app.db.save_probe(probe.clone()).await?;
    app.push_tasks().await?;
    Ok(Json(json!(probe)))
}
async fn delete_probe(State(app): State<Shared>, Path(id): Path<String>) -> ApiResult<Json<Value>> {
    app.db.delete_probe(id).await?;
    app.push_tasks().await?;
    Ok(Json(json!({"ok":true})))
}
async fn network(
    State(app): State<Shared>,
    headers: HeaderMap,
    Query(window): Query<Window>,
) -> ApiResult<Json<Value>> {
    let admin = readable(&app, &headers).await?;
    let nodes = app.db.nodes().await?;
    let allowed = nodes
        .into_iter()
        .filter(|n| admin || n.config.public)
        .map(|n| n.id)
        .collect::<Vec<_>>();
    let mut records = app
        .db
        .probe_history(None, window.hours.unwrap_or(24).clamp(1, 168))
        .await?;
    records.retain(|r| {
        r["node_id"]
            .as_str()
            .is_some_and(|id| allowed.iter().any(|n| n == id))
    });
    if !admin {
        for r in &mut records {
            r.as_object_mut().unwrap().remove("error");
        }
    }
    Ok(Json(
        json!({"records":records,"profiles":network_profiles(&records)}),
    ))
}
pub fn network_profiles(records: &[Value]) -> Vec<Value> {
    let mut groups: BTreeMap<(String, String), Vec<&Value>> = BTreeMap::new();
    for r in records {
        groups
            .entry((
                r["node_id"].as_str().unwrap_or_default().into(),
                r["task_id"].as_str().unwrap_or_default().into(),
            ))
            .or_default()
            .push(r);
    }
    groups.into_iter().map(|((node_id,task_id),records)|{let mut values=records.iter().filter_map(|r|r["latency_ms"].as_f64()).collect::<Vec<_>>();values.sort_by(f64::total_cmp);let successes=records.iter().filter(|r|r["success"]==true).count();let stages=|key:&str|{let v=records.iter().filter_map(|r|r[key].as_f64()).collect::<Vec<_>>();if v.is_empty(){None}else{Some(v.iter().sum::<f64>()/v.len() as f64)}};json!({"node_id":node_id,"task_id":task_id,"samples":records.len(),"loss_percent":(records.len()-successes) as f64/records.len().max(1) as f64*100.0,"median_ms":values.get(values.len()/2),"p95_ms":values.get(((values.len() as f64*0.95).ceil() as usize).saturating_sub(1)),"dns_ms":stages("dns_ms"),"tcp_ms":stages("tcp_ms"),"tls_ms":stages("tls_ms"),"ttfb_ms":stages("ttfb_ms"),"last":records.last()})}).collect()
}
async fn forecast(
    State(app): State<Shared>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let node = require_node(&app, &headers, &id).await?;
    let s = app.settings.read().await.clone();
    let t = app
        .db
        .traffic(
            node.id.clone(),
            s.timezone.clone(),
            node.config.traffic_reset_day,
        )
        .await?;
    let daily = app.db.daily_traffic(node.id).await?;
    let (today, _, next) = calendar_period(
        monitor_protocol::now_ms(),
        &s.timezone,
        node.config.traffic_reset_day,
    );
    let values: Vec<f64> = daily
        .iter()
        .filter(|d| d["day"].as_str() != Some(&today.to_string()))
        .take(7)
        .map(|d| {
            traffic_used(
                &node.config.traffic_mode,
                d["rx"].as_i64().unwrap_or(0),
                d["tx"].as_i64().unwrap_or(0),
            ) as f64
        })
        .collect();
    let used = t.used(&node.config.traffic_mode);
    let days_left = (next - today).num_days();
    let mean = if values.is_empty() {
        0.
    } else {
        values.iter().sum::<f64>() / values.len() as f64
    };
    let deviation = if values.len() < 2 {
        0.
    } else {
        (values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / values.len() as f64).sqrt()
    };
    let forecast = if values.len() >= 3 {
        Some(used as f64 + mean * days_left as f64)
    } else {
        None
    };
    Ok(Json(
        json!({"used":used,"limit":node.config.traffic_limit,"next_reset":next.to_string(),"days_left":days_left,"observed_days":values.len(),"daily_mean":mean,"estimate":forecast,"low":forecast.map(|v|(v-deviation*days_left as f64).max(used as f64)),"high":forecast.map(|v|v+deviation*days_left as f64),"days_until_limit":if mean>0.&&node.config.traffic_limit>0{Some((node.config.traffic_limit as f64-used as f64).max(0.)/mean)}else{None},"confidence":if values.len()>=7{"established"}else{"warming_up"},"daily":daily}),
    ))
}
async fn list_themes(State(app): State<Shared>) -> ApiResult<Json<Value>> {
    Ok(Json(json!(themes::list(&app).await?)))
}
async fn import_theme(
    State(app): State<Shared>,
    mut multipart: Multipart,
) -> ApiResult<Json<Value>> {
    let field = multipart
        .next_field()
        .await
        .map_err(|e| ApiError::bad(e.to_string()))?
        .ok_or_else(|| ApiError::bad("缺少主题包"))?;
    let bytes = field
        .bytes()
        .await
        .map_err(|e| ApiError::bad(e.to_string()))?;
    let directory = app.themes_dir.clone();
    let info = tokio::task::spawn_blocking(move || themes::import(bytes.to_vec(), directory))
        .await
        .map_err(|e| ApiError::from(anyhow::anyhow!(e)))?
        .map_err(|e| ApiError::bad(e.to_string()))?;
    Ok(Json(info))
}
async fn delete_theme(State(app): State<Shared>, Path(id): Path<String>) -> ApiResult<Json<Value>> {
    if ["default", "native"].contains(&id.as_str())
        || id == app.settings.read().await.theme
        || !valid_id(&id)
    {
        return Err(ApiError::bad("不能删除内置或正在使用的主题"));
    }
    std::fs::remove_dir_all(app.themes_dir.join(id))
        .map_err(|e| ApiError::from(anyhow::anyhow!(e)))?;
    Ok(Json(json!({"ok":true})))
}
async fn public_theme_config(
    State(app): State<Shared>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    readable(&app, &headers).await?;
    if !valid_id(&id) {
        return Err(ApiError::bad("主题标识无效"));
    }
    Ok(Json(themes::config(&app, &id).await?))
}
fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}
async fn activate_theme(
    State(app): State<Shared>,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    if !themes::list(&app).await?.iter().any(|t| t["id"] == id) {
        return Err(ApiError::not_found());
    }
    let mut settings = app.settings.read().await.clone();
    settings.theme = id;
    app.db.save_settings(settings.clone()).await?;
    *app.settings.write().await = settings;
    Ok(Json(json!({"ok":true})))
}
async fn theme_config(State(app): State<Shared>, Path(id): Path<String>) -> ApiResult<Json<Value>> {
    if !valid_id(&id) {
        return Err(ApiError::bad("主题标识无效"));
    }
    Ok(Json(themes::config(&app, &id).await?))
}
pub async fn set_theme_config(app: &Shared, id: &str, config: Value) -> ApiResult<Json<Value>> {
    if !valid_id(id) || !config.is_object() || config.to_string().len() > 65536 {
        return Err(ApiError::bad("主题设置无效或过大"));
    }
    app.db
        .set(&format!("theme:{id}"), config.to_string())
        .await?;
    Ok(Json(json!({"ok":true})))
}
async fn save_theme_config(
    State(app): State<Shared>,
    Path(id): Path<String>,
    Json(config): Json<Value>,
) -> ApiResult<Json<Value>> {
    set_theme_config(&app, &id, config).await
}
async fn database_info(State(app): State<Shared>) -> ApiResult<Json<Value>> {
    let stats = app
        .db
        .call(|c| {
            let pages: i64 = c.query_row("PRAGMA page_count", [], |r| r.get(0))?;
            let size: i64 = c.query_row("PRAGMA page_size", [], |r| r.get(0))?;
            let free: i64 = c.query_row("PRAGMA freelist_count", [], |r| r.get(0))?;
            let metrics: i64 = c.query_row("SELECT COUNT(*) FROM metric", [], |r| r.get(0))?;
            Ok(json!({"bytes":pages*size,"reclaimable_bytes":free*size,"minute_records":metrics}))
        })
        .await?;
    Ok(Json(stats))
}
async fn backup(State(app): State<Shared>) -> ApiResult<Response> {
    let path = app
        .data_dir
        .join(format!("backup-{}.db", uuid::Uuid::new_v4()));
    app.db.backup(path.clone()).await?;
    let file = tokio::fs::File::open(&path)
        .await
        .map_err(|e| ApiError::from(anyhow::anyhow!(e)))?;
    // An open descriptor remains readable after unlink on the supported Unix hosts.
    #[cfg(unix)]
    {
        let _ = tokio::fs::remove_file(&path).await;
    }
    let body = axum::body::Body::from_stream(tokio_util::io::ReaderStream::new(file));
    Ok((
        [
            (header::CONTENT_TYPE, "application/octet-stream"),
            (
                header::CONTENT_DISPOSITION,
                "attachment; filename=monitor-backup.db",
            ),
        ],
        body,
    )
        .into_response())
}
async fn restore(State(app): State<Shared>, mut multipart: Multipart) -> ApiResult<Json<Value>> {
    let mut field = multipart
        .next_field()
        .await
        .map_err(|e| ApiError::bad(e.to_string()))?
        .ok_or_else(|| ApiError::bad("缺少备份文件"))?;
    let path = app
        .data_dir
        .join(format!("restore-{}.db", uuid::Uuid::new_v4()));
    let mut file = tokio::fs::File::create(&path)
        .await
        .map_err(|e| ApiError::from(anyhow::anyhow!(e)))?;
    let upload: ApiResult<()> = async {
        let mut total = 0usize;
        while let Some(chunk) = field
            .chunk()
            .await
            .map_err(|e| ApiError::bad(e.to_string()))?
        {
            total += chunk.len();
            if total > 1024 * 1024 * 1024 {
                return Err(ApiError::bad("备份不能超过 1 GiB"));
            }
            use tokio::io::AsyncWriteExt;
            file.write_all(&chunk)
                .await
                .map_err(|e| ApiError::from(anyhow::anyhow!(e)))?;
        }
        file.sync_all()
            .await
            .map_err(|e| ApiError::from(anyhow::anyhow!(e)))?;
        Ok(())
    }
    .await;
    drop(file);
    if let Err(error) = upload {
        let _ = tokio::fs::remove_file(path).await;
        return Err(error);
    }
    let result = app.db.restore(path.clone()).await;
    let _ = tokio::fs::remove_file(path).await;
    result.map_err(|e| ApiError::bad(e.to_string()))?;
    let ids = app.live.read().await.keys().cloned().collect::<Vec<_>>();
    for id in ids {
        app.disconnect(&id).await;
    }
    *app.settings.write().await = app.db.settings().await?;
    app.active_events.lock().await.clear();
    Ok(Json(json!({"ok":true,"relogin":true})))
}
async fn vacuum(State(app): State<Shared>) -> ApiResult<Json<Value>> {
    app.db
        .call(|c| {
            c.execute_batch("VACUUM; PRAGMA wal_checkpoint(TRUNCATE);")?;
            Ok(())
        })
        .await?;
    Ok(Json(json!({"ok":true})))
}
async fn test_notification(State(app): State<Shared>) -> ApiResult<Json<Value>> {
    let settings = app.settings.read().await.notify.clone();
    crate::notify::send(&settings,json!({"event":"test","title":"Monitor 测试通知","message":"通知渠道连接成功。","time":monitor_protocol::now_ms()})).await.map_err(|e|ApiError::bad(e.to_string()))?;
    Ok(Json(json!({"ok":true})))
}
async fn agent_binary(Path(arch): Path<String>) -> ApiResult<Response> {
    let native = match std::env::consts::ARCH {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        value => value,
    };
    if !cfg!(target_os = "linux") || arch != native {
        return Err(ApiError(
            StatusCode::NOT_FOUND,
            "此 Hub 尚未提供该 Linux 架构的 Agent".into(),
        ));
    }
    let path = std::env::current_exe()
        .map_err(|e| ApiError::from(anyhow::anyhow!(e)))?
        .with_file_name("monitor-agent");
    let bytes = tokio::fs::read(path).await.map_err(|_| {
        ApiError(
            StatusCode::NOT_FOUND,
            "Agent 二进制尚未安装到 Hub 旁边".into(),
        )
    })?;
    Ok(([(header::CONTENT_TYPE, "application/octet-stream")], bytes).into_response())
}
async fn frontend(State(app): State<Shared>, uri: Uri) -> Response {
    let path = uri.path();
    if path.starts_with("/api/") {
        return ApiError::not_found().into_response();
    }
    let builtin = path == "/admin"
        || path.starts_with("/admin/")
        || path.starts_with("/ui-assets/")
        || path == "/network"
        || path.starts_with("/node/");
    let theme = app.settings.read().await.theme.clone();
    let root = if builtin
        || theme == "native"
        || (theme == "default" && themes::builtin_root(&app, &theme).is_none())
    {
        app.frontend_dir.clone()
    } else {
        themes::root(&app, &theme).join("dist")
    };
    let asset = themes::resolve_asset(&root, path.trim_start_matches('/'))
        .or_else(|| {
            if path.starts_with("/themes/") {
                let parts = path.trim_start_matches("/themes/").split_once('/')?;
                if !valid_id(parts.0) {
                    return None;
                }
                let dir = themes::root(
                    &app,
                    if parts.0 == "default" {
                        &theme
                    } else {
                        parts.0
                    },
                );
                themes::resolve_asset(&dir, parts.1)
            } else {
                None
            }
        })
        .or_else(|| {
            if !path.rsplit('/').next().unwrap_or_default().contains('.') {
                themes::resolve_asset(&root, "index.html")
            } else {
                None
            }
        });
    if let Some(asset) = asset {
        match tokio::fs::read(&asset).await {
            Ok(bytes) => (
                [(
                    header::CONTENT_TYPE,
                    mime_guess::from_path(asset)
                        .first_or_octet_stream()
                        .to_string(),
                )],
                bytes,
            )
                .into_response(),
            Err(_) => StatusCode::NOT_FOUND.into_response(),
        }
    } else {
        (
            StatusCode::NOT_FOUND,
            "前端资源尚未构建，请运行 npm --prefix web run build",
        )
            .into_response()
    }
}
