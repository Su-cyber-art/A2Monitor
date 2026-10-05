use crate::{
    api::{readable, require_node, set_theme_config},
    auth,
    error::{ApiError, ApiResult},
    runtime::Shared,
    themes,
};
use axum::{
    extract::{Path, Query, State, WebSocketUpgrade},
    http::{HeaderMap, StatusCode},
    response::Response,
    routing::{get, post},
    Json, Router,
};
use futures_util::StreamExt;
use serde_json::{json, Value};
use std::collections::BTreeMap;

pub fn routes(_: Shared) -> Router<Shared> {
    Router::new()
        .route("/api/me", get(me))
        .route("/api/nodes", get(nodes))
        .route("/api/public", get(public))
        .route("/api/version", get(version))
        .route("/api/rpc2", get(rpc_ws).post(rpc_http))
        .route("/api/clients", get(legacy_ws))
        .route("/api/ws", get(monitor_ws))
        .route("/api/records/load", get(load))
        .route("/api/records/ping", get(ping))
        .route("/api/task/ping", get(ping_tasks))
        .route("/api/recent/{id}", get(recent))
        .route("/api/nodes/{id}/metrics", get(monitor_history))
        .route(
            "/api/themes/{id}/config",
            get(monitor_config).put(monitor_save_config),
        )
        .route("/api/admin/theme/settings", post(komari_save_config))
}
fn envelope(data: Value) -> Value {
    json!({"status":"success","message":"","data":data})
}
fn iso(time: i64) -> String {
    chrono::DateTime::from_timestamp_millis(time)
        .unwrap_or_default()
        .to_rfc3339()
}
fn num(value: &Value, key: &str) -> f64 {
    value[key].as_f64().unwrap_or(0.0)
}
pub fn komari_node(node: &Value) -> Value {
    let facts = &node["facts"];
    let mut n = json!({"uuid":node["id"],"updated_at":iso(node["last_seen"].as_i64().unwrap_or(0)),"name":node["name"],"cpu_name":facts["cpu_name"],"cpu_cores":facts["cpu_cores"],"cpu_physical_cores":0,"virtualization":facts["virtualization"],"arch":facts["arch"],"os":facts["os"],"kernel_version":facts["kernel"],"gpu_name":"","region":node["region"],"public_remark":node["public_remark"],"mem_total":facts["mem_total"],"swap_total":facts["swap_total"],"disk_total":facts["disk_total"],"weight":node.get("position").unwrap_or(&node["alias"]),"price":node["price"],"billing_cycle":node["billing_days"],"auto_renewal":node["auto_renew"],"currency":node["currency"],"expired_at":node["expires_at"],"group":node["group"],"tags":"","hidden":!node["public"].as_bool().unwrap_or(false),"traffic_limit":node["traffic_limit"],"traffic_limit_type":node["traffic_mode"]});
    if node.get("remark").is_some() {
        n["remark"] = node["remark"].clone();
        n["ipv4"] = facts["ipv4"].clone();
        n["ipv6"] = facts["ipv6"].clone();
        n["version"] = facts["agent_version"].clone();
    }
    n
}
pub fn komari_record(node: &Value, sample: &Value) -> Value {
    let traffic = &node["traffic"];
    json!({"client":node["id"],"time":iso(sample["time"].as_i64().unwrap_or(0)),"cpu":num(sample,"cpu"),"ram":num(sample,"mem_used") as u64,"ram_total":num(sample,"mem_total") as u64,"swap":num(sample,"swap_used") as u64,"swap_total":num(sample,"swap_total") as u64,"disk":num(sample,"disk_used") as u64,"disk_total":num(sample,"disk_total") as u64,"load":sample["load"][0].as_f64().unwrap_or(0.0),"load5":sample["load"][1].as_f64().unwrap_or(0.0),"load15":sample["load"][2].as_f64().unwrap_or(0.0),"temp":0,"net_in":num(sample,"net_rx") as u64,"net_out":num(sample,"net_tx") as u64,"net_total_up":sample.get("total_tx").unwrap_or(&traffic["total_tx"]),"net_total_down":sample.get("total_rx").unwrap_or(&traffic["total_rx"]),"connections":num(sample,"tcp"),"connections_udp":num(sample,"udp"),"process":num(sample,"procs"),"online":node["online"],"uptime":sample["uptime"],"ping":{}})
}
pub fn nested_record(node: &Value) -> Value {
    let m = &node["metrics"];
    json!({"cpu":{"usage":num(m,"cpu")},"ram":{"total":num(m,"mem_total"),"used":num(m,"mem_used")},"swap":{"total":num(m,"swap_total"),"used":num(m,"swap_used")},"disk":{"total":num(m,"disk_total"),"used":num(m,"disk_used")},"load":{"load1":m["load"][0].as_f64().unwrap_or(0.),"load5":m["load"][1].as_f64().unwrap_or(0.),"load15":m["load"][2].as_f64().unwrap_or(0.)},"network":{"up":num(m,"net_tx"),"down":num(m,"net_rx"),"totalUp":node["traffic"]["total_tx"],"totalDown":node["traffic"]["total_rx"]},"connections":{"tcp":num(m,"tcp"),"udp":num(m,"udp")},"uptime":m["uptime"],"process":num(m,"procs"),"message":"","updated_at":iso(m["time"].as_i64().unwrap_or(0))})
}
async fn komari_latest(app: &Shared, admin: bool, params: &Value) -> anyhow::Result<Value> {
    let nodes = app.nodes(admin).await?;
    let mut map = serde_json::Map::new();
    for node in nodes {
        let id = node["id"].as_str().unwrap_or_default();
        if params["uuid"].as_str().is_some_and(|s| s != id) {
            continue;
        }
        map.insert(id.into(), komari_record(&node, &node["metrics"]));
    }
    if let Some(id) = params["uuid"].as_str() {
        Ok(map.get(id).cloned().unwrap_or(Value::Null))
    } else {
        Ok(Value::Object(map))
    }
}
async fn komari_public(app: &Shared) -> anyhow::Result<Value> {
    let s = app.settings.read().await.clone();
    let settings = themes::config(app, &s.theme).await?;
    Ok(
        json!({"site_name":s.site_name,"sitename":s.site_name,"description":s.description,"theme":s.theme,"theme_settings":settings,"private_site":!s.public_site,"send_ip_addr_to_guest":false,"custom_head":"","custom_body":"","enable_ping_chart":true,"show_traffic":true,"show_expiry":true,"show_price":true,"record_enabled":true,"record_preserve_time":s.history_days*24,"ping_record_preserve_time":168,"disable_password_login":false,"oauth_enable":false,"oauth_provider":"","allow_cors":false}),
    )
}
async fn me(State(app): State<Shared>, headers: HeaderMap) -> ApiResult<Json<Value>> {
    let admin = auth::authed(&app, &headers).await;
    let s = app.settings.read().await.clone();
    if themes::profile(&app).await == "monitor" {
        Ok(Json(
            json!({"authed":admin,"site_name":s.site_name,"site":s.site_name,"public_page":s.public_site,"history_days":s.history_days}),
        ))
    } else {
        Ok(Json(
            json!({"logged_in":admin,"username":if admin{"admin"}else{"Guest"},"uuid":if admin{"admin"}else{""},"sso_type":"","sso_id":"","2fa_enabled":false}),
        ))
    }
}
async fn public(State(app): State<Shared>) -> ApiResult<Json<Value>> {
    Ok(Json(envelope(komari_public(&app).await?)))
}
async fn version() -> Json<Value> {
    Json(envelope(
        json!({"version":"1.3.0-monitor-compat","hash":"monitor","native_version":env!("CARGO_PKG_VERSION")}),
    ))
}
async fn nodes(State(app): State<Shared>, headers: HeaderMap) -> ApiResult<Json<Value>> {
    let admin = readable(&app, &headers).await?;
    if themes::profile(&app).await == "monitor" {
        Ok(Json(monitor_snapshot(&app, admin).await?))
    } else {
        Ok(Json(envelope(json!(app
            .nodes(admin)
            .await?
            .iter()
            .map(komari_node)
            .collect::<Vec<_>>()))))
    }
}
async fn load_data(app: &Shared, headers: &HeaderMap, params: &Value) -> ApiResult<Value> {
    let id = params["uuid"]
        .as_str()
        .ok_or_else(|| ApiError::bad("缺少 uuid"))?;
    let node = require_node(app, headers, id).await?;
    let admin = readable(app, headers).await?;
    let view = app
        .nodes(admin)
        .await?
        .into_iter()
        .find(|n| n["id"] == node.id)
        .ok_or_else(ApiError::not_found)?;
    let hours = params["hours"]
        .as_u64()
        .unwrap_or(6)
        .clamp(1, app.settings.read().await.history_days as u64 * 24);
    let history = app.db.history(node.id, hours as u32, 600).await?;
    let records = history["records"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| komari_record(&view, s))
        .collect::<Vec<_>>();
    Ok(
        json!({"records":records,"count":records.len(),"load_type":params["load_type"].as_str().unwrap_or("all"),"gpu_devices":[],"has_gpu_data":false}),
    )
}
async fn load(
    State(app): State<Shared>,
    headers: HeaderMap,
    Query(query): Query<BTreeMap<String, String>>,
) -> ApiResult<Json<Value>> {
    let params = query_value(query);
    Ok(Json(envelope(load_data(&app, &headers, &params).await?)))
}
fn query_value(query: BTreeMap<String, String>) -> Value {
    Value::Object(
        query
            .into_iter()
            .map(|(k, v)| {
                let value = if k == "hours" || k == "task_id" {
                    v.parse::<u64>().map(|n| json!(n)).unwrap_or(json!(v))
                } else {
                    json!(v)
                };
                (k, value)
            })
            .collect(),
    )
}
async fn ping_data(app: &Shared, headers: &HeaderMap, params: &Value) -> ApiResult<Value> {
    let admin = readable(app, headers).await?;
    let views = app.nodes(admin).await?;
    let visible = views
        .iter()
        .filter_map(|v| v["id"].as_str())
        .collect::<Vec<_>>();
    let uuid = params["uuid"].as_str().map(str::to_owned);
    if let Some(id) = &uuid {
        require_node(app, headers, id).await?;
    }
    let tasks = app.db.probes().await?;
    let hours = params["hours"].as_u64().unwrap_or(6).clamp(1, 168) as u32;
    let records=app.db.probe_history(uuid,hours).await?.into_iter().filter(|r|r["node_id"].as_str().is_some_and(|id|visible.contains(&id))).filter_map(|r|{let index=tasks.iter().position(|p|p.id==r["task_id"])?;let task=index+1;if params["task_id"].as_u64().is_some_and(|id|id!=task as u64){return None;}Some(json!({"client":r["node_id"],"task_id":task,"time":iso(r["time"].as_i64().unwrap_or(0)),"value":if r["success"]==true{r["latency_ms"].as_f64().unwrap_or(0.)}else{-1.0}}))}).collect::<Vec<_>>();
    Ok(json!({"records":records,"count":records.len(),"tasks":[] ,"basic_info":[]}))
}
async fn ping(
    State(app): State<Shared>,
    headers: HeaderMap,
    Query(query): Query<BTreeMap<String, String>>,
) -> ApiResult<Json<Value>> {
    Ok(Json(envelope(
        ping_data(&app, &headers, &query_value(query)).await?,
    )))
}
async fn public_tasks(app: &Shared, headers: &HeaderMap) -> ApiResult<Value> {
    let admin = readable(app, headers).await?;
    let visible = app
        .nodes(admin)
        .await?
        .into_iter()
        .filter_map(|v| v["id"].as_str().map(str::to_owned))
        .collect::<Vec<_>>();
    Ok(json!(app.db.probes().await?.iter().enumerate().filter(|(_,p)|visible.iter().any(|id|p.assigned(id))).map(|(i,p)|json!({"id":i+1,"name":p.name,"type":match p.kind{monitor_protocol::ProbeKind::Http=>"http",_=>"tcp"},"interval":p.interval,"clients":visible.iter().filter(|id|p.assigned(id)).collect::<Vec<_>>(),"default_on":true,"weight":i})).collect::<Vec<_>>()))
}
async fn ping_tasks(State(app): State<Shared>, headers: HeaderMap) -> ApiResult<Json<Value>> {
    Ok(Json(envelope(public_tasks(&app, &headers).await?)))
}
async fn recent(
    State(app): State<Shared>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let node = require_node(&app, &headers, &id).await?;
    let admin = readable(&app, &headers).await?;
    let view = app
        .nodes(admin)
        .await?
        .into_iter()
        .find(|n| n["id"] == node.id)
        .unwrap();
    Ok(Json(envelope(json!([nested_record(&view)]))))
}
async fn rpc_dispatch(app: &Shared, headers: &HeaderMap, request: Value) -> Value {
    let id = request.get("id").cloned().unwrap_or(Value::Null);
    let method = request["method"].as_str().unwrap_or("");
    let method = method.strip_prefix("rpc.").unwrap_or(method);
    let params = &request["params"];
    let result:ApiResult<Value>=async{
        let admin=readable(app,headers).await?;
        if method.starts_with("admin:"){return Err(ApiError::bad("主题兼容层仅提供公开展示接口"));}
        match method.split(':').next_back().unwrap_or(""){
            "getNodes"|"getNodesInformation"=>{let views=app.nodes(admin).await?;let map=views.iter().map(|v|(v["id"].as_str().unwrap().to_owned(),komari_node(v))).collect::<serde_json::Map<_,_>>();Ok(if let Some(id)=params["uuid"].as_str(){map.get(id).cloned().unwrap_or(Value::Null)}else{Value::Object(map)})},
            "getNodesLatestStatus"=>Ok(komari_latest(app,admin,params).await?),
            "listMetricDefinitions"=>Ok(crate::compat_metrics::definitions(app.settings.read().await.history_days)),
            "queryMetrics"=>crate::compat_metrics::query(app,headers,params).await,
            "getPingMetricStats"=>crate::compat_metrics::ping_stats(app,headers,params).await,
            "getMe"=>Ok(json!({"logged_in":admin,"username":if admin{"admin"}else{"Guest"},"2fa_enabled":false})),
            "getPublicInfo"|"getPublicSettings"=>Ok(komari_public(app).await?),
            "getRecordsByUUID"|"getClientLoadRecords"=>load_data(app,headers,params).await,
            "getRecords"=>if params["type"]=="ping"{ping_data(app,headers,params).await}else{load_data(app,headers,params).await},
            "getPingRecords"=>ping_data(app,headers,params).await,
            "getPublicPingTasks"|"getPingTasks"=>public_tasks(app,headers).await,
            "getVersion"|"getBackendVersion"=>Ok(json!({"version":"1.3.0-monitor-compat","hash":"monitor"})),
            "getNodeRecentStatus"=>{
                let id=params["uuid"].as_str().unwrap_or_default();
                let node=require_node(app,headers,id).await?;
                let view=app.nodes(admin).await?.into_iter().find(|n|n["id"]==node.id).ok_or_else(ApiError::not_found)?;
                let limit=params["limit"].as_u64().unwrap_or(120).clamp(1,120) as usize;
                let live=app.live.read().await;
                let records=live.get(&node.id).map(|n|n.ring.iter().rev().take(limit).collect::<Vec<_>>().into_iter().rev().map(|s|komari_record(&view,&crate::db::public_sample(s))).collect::<Vec<_>>()).unwrap_or_default();
                Ok(json!({"count":records.len(),"records":records}))
            },
            "getClientRecentRecords"=>{let id=params["uuid"].as_str().unwrap_or_default();require_node(app,headers,id).await?;let view=app.nodes(admin).await?.into_iter().find(|n|n["id"]==id).ok_or_else(ApiError::not_found)?;Ok(json!([nested_record(&view)]))},
            "ping"|"heartbeat"=>Ok(json!("pong")),
            _=>Err(ApiError::bad("该主题调用了尚未适配的接口"))
        }
    }.await;
    match result {
        Ok(result) => json!({"jsonrpc":"2.0","id":id,"result":result}),
        Err(error) => {
            json!({"jsonrpc":"2.0","id":id,"error":{"code":if error.0==StatusCode::UNAUTHORIZED{-32001}else{-32601},"message":error.1}})
        }
    }
}
async fn rpc_http(
    State(app): State<Shared>,
    headers: HeaderMap,
    Json(request): Json<Value>,
) -> Json<Value> {
    if let Some(batch) = request.as_array() {
        let mut replies = vec![];
        for item in batch.iter().take(32) {
            replies.push(rpc_dispatch(&app, &headers, item.clone()).await);
        }
        Json(json!(replies))
    } else {
        Json(rpc_dispatch(&app, &headers, request).await)
    }
}
async fn rpc_ws(
    State(app): State<Shared>,
    headers: HeaderMap,
    upgrade: WebSocketUpgrade,
) -> ApiResult<Response> {
    readable(&app, &headers).await?;
    if !auth::same_origin(&headers) {
        return Err(ApiError(StatusCode::FORBIDDEN, "请求来源无效".into()));
    }
    Ok(upgrade
        .max_message_size(256 * 1024)
        .on_upgrade(move |mut socket| async move {
            while let Some(Ok(frame)) = socket.next().await {
                if let axum::extract::ws::Message::Text(text) = frame {
                    if let Ok(request) = serde_json::from_str(&text) {
                        let response = rpc_dispatch(&app, &headers, request).await;
                        if socket
                            .send(axum::extract::ws::Message::Text(
                                response.to_string().into(),
                            ))
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                }
            }
        }))
}
async fn legacy_snapshot(app: &Shared, admin: bool, id: Option<&str>) -> anyhow::Result<Value> {
    let nodes = app.nodes(admin).await?;
    let online = nodes
        .iter()
        .filter(|n| n["online"] == true && id.is_none_or(|id| n["id"] == id))
        .map(|n| n["id"].clone())
        .collect::<Vec<_>>();
    let map = nodes
        .iter()
        .filter(|n| id.is_none_or(|id| n["id"] == id))
        .map(|n| (n["id"].as_str().unwrap().to_owned(), nested_record(n)))
        .collect::<serde_json::Map<_, _>>();
    Ok(envelope(json!({"online":online,"data":map})))
}
async fn legacy_ws(
    State(app): State<Shared>,
    headers: HeaderMap,
    upgrade: WebSocketUpgrade,
) -> ApiResult<Response> {
    readable(&app, &headers).await?;
    if !auth::same_origin(&headers) {
        return Err(ApiError(StatusCode::FORBIDDEN, "请求来源无效".into()));
    }
    Ok(upgrade.on_upgrade(move |mut socket| async move {
        while let Some(Ok(axum::extract::ws::Message::Text(text))) = socket.next().await {
            let Ok(admin) = readable(&app, &headers).await else {
                break;
            };
            let id = text.strip_prefix("get ").map(str::trim);
            if text == "get" || id.is_some() {
                if let Ok(snapshot) = legacy_snapshot(&app, admin, id).await {
                    if socket
                        .send(axum::extract::ws::Message::Text(
                            snapshot.to_string().into(),
                        ))
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
            }
        }
    }))
}
async fn monitor_snapshot(app: &Shared, admin: bool) -> anyhow::Result<Value> {
    let views = app.nodes(admin).await?;
    let (today, _, _) = crate::db::calendar_period(
        monitor_protocol::now_ms(),
        &app.settings.read().await.timezone,
        1,
    );
    let nodes = views
        .into_iter()
        .map(|n| {
            let mut v = n.clone();
            v["id"] = n["alias"].clone();
            v["country"] = n["region"].clone();
            v["cpu_name"] = n["facts"]["cpu_name"].clone();
            v["cpu_cores"] = n["facts"]["cpu_cores"].clone();
            v["os"] = n["facts"]["os"].clone();
            v["kernel"] = n["facts"]["kernel"].clone();
            v["arch"] = n["facts"]["arch"].clone();
            v["virt"] = n["facts"]["virtualization"].clone();
            for key in ["mem_total", "swap_total", "disk_total"] {
                v[key] = n["facts"][key].clone();
            }
            for key in [
                "total_rx", "total_tx", "month_rx", "month_tx", "day_rx", "day_tx",
            ] {
                v[key] = n["traffic"][key].clone();
            }
            v["last_seen"] = json!(n["last_seen"].as_i64().unwrap_or(0) / 1000);
            v["billing_cycle"] = json!(if n["billing_days"].as_u64().unwrap_or(0) == 365 {
                "yearly"
            } else {
                "monthly"
            });
            v["agent_version"] = n["facts"]["agent_version"].clone();
            v["expires_in"] = n["expires_at"]
                .as_str()
                .and_then(|s| chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").ok())
                .map(|d| json!((d - today).num_days()))
                .unwrap_or(Value::Null);
            if n["online"] != true {
                v["metrics"] = Value::Null;
            } else if let Some(m) = v["metrics"].as_object_mut() {
                m.insert("total_rx".into(), n["traffic"]["total_rx"].clone());
                m.insert("total_tx".into(), n["traffic"]["total_tx"].clone());
                m.insert("month_rx".into(), n["traffic"]["month_rx"].clone());
                m.insert("month_tx".into(), n["traffic"]["month_tx"].clone());
            }
            if admin {
                v["hostname"] = n["facts"]["hostname"].clone();
                v["ipv4"] = n["facts"]["ipv4"].clone();
                v["ipv6"] = n["facts"]["ipv6"].clone();
            }
            v
        })
        .collect::<Vec<_>>();
    Ok(json!({"nodes":nodes,"admin":admin}))
}
async fn monitor_ws(
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
            if let Ok(snapshot) = monitor_snapshot(&app, admin).await {
                if socket
                    .send(axum::extract::ws::Message::Text(
                        snapshot.to_string().into(),
                    ))
                    .await
                    .is_err()
                {
                    break;
                }
            } else {
                break;
            }
        }
    }))
}
async fn monitor_history(
    State(app): State<Shared>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(query): Query<BTreeMap<String, String>>,
) -> ApiResult<Json<Value>> {
    let node = require_node(&app, &headers, &id).await?;
    let hours = query
        .get("hours")
        .and_then(|s| s.parse().ok())
        .unwrap_or(6)
        .clamp(1, app.settings.read().await.history_days * 24);
    let points = query
        .get("points")
        .and_then(|s| s.parse().ok())
        .unwrap_or(240)
        .clamp(60, 1440);
    let mut history = app.db.history(node.id.clone(), hours, points).await?;
    let metrics = history["records"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .map(|m| {
            m["ts"] = json!(m["time"].as_i64().unwrap_or(0) / 1000);
            m.clone()
        })
        .collect::<Vec<_>>();
    let probes = app.db.probes().await?;
    let step = history["step"].as_i64().unwrap_or(60).max(60);
    let results = app.db.probe_history(Some(node.id), hours.min(168)).await?;
    let mut groups = std::collections::BTreeMap::<(usize, i64), Vec<&Value>>::new();
    let mut totals = std::collections::BTreeMap::<usize, (usize, usize)>::new();
    for r in &results {
        let Some(i) = probes.iter().position(|p| p.id == r["task_id"]) else {
            continue;
        };
        let time = r["time"].as_i64().unwrap_or(0) / 1000 / step * step;
        groups.entry((i, time)).or_default().push(r);
        let count = totals.entry(i).or_default();
        count.0 += 1;
        if r["success"] != true {
            count.1 += 1;
        }
    }
    let ping=groups.into_iter().map(|((i,time),records)|{
        let mut values=records.iter().filter(|r|r["success"]==true).filter_map(|r|r["latency_ms"].as_f64()).collect::<Vec<_>>();values.sort_by(f64::total_cmp);
        let latency=values.get(values.len()/2).copied();let band=if values.is_empty(){Value::Null}else{json!([values.first(),values.last()])};
        json!({"task_id":i+1,"ts":time,"latency":latency,"band":band,"loss":(records.len()-values.len()) as f64/records.len() as f64*100.0})
    }).collect::<Vec<_>>();
    let names = probes
        .iter()
        .enumerate()
        .map(|(i, p)| ((i + 1).to_string(), json!(p.name)))
        .collect::<serde_json::Map<_, _>>();
    let loss = totals
        .into_iter()
        .map(|(i, (total, failed))| {
            (
                (i + 1).to_string(),
                json!(failed as f64 / total as f64 * 100.0),
            )
        })
        .collect::<serde_json::Map<_, _>>();
    Ok(Json(
        json!({"step":history["step"],"metrics":metrics,"ping":ping,"probes":names,"loss":loss}),
    ))
}
async fn monitor_config(
    State(app): State<Shared>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    readable(&app, &headers).await?;
    let id = config_id(&app, id).await;
    Ok(Json(themes::config(&app, &id).await?))
}
async fn monitor_save_config(
    State(app): State<Shared>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(config): Json<Value>,
) -> ApiResult<Json<Value>> {
    if !auth::authed(&app, &headers).await {
        return Err(ApiError::unauthorized());
    }
    if !auth::same_origin(&headers) {
        return Err(ApiError(StatusCode::FORBIDDEN, "请求来源无效".into()));
    }
    let id = config_id(&app, id).await;
    set_theme_config(&app, &id, config).await
}
async fn komari_save_config(
    State(app): State<Shared>,
    headers: HeaderMap,
    Query(query): Query<BTreeMap<String, String>>,
    Json(config): Json<Value>,
) -> ApiResult<Json<Value>> {
    if !auth::authed(&app, &headers).await {
        return Err(ApiError::unauthorized());
    }
    if !auth::same_origin(&headers) {
        return Err(ApiError(StatusCode::FORBIDDEN, "请求来源无效".into()));
    }
    let theme = query
        .get("theme")
        .cloned()
        .unwrap_or(app.settings.read().await.theme.clone());
    let theme = config_id(&app, theme).await;
    set_theme_config(&app, &theme, config).await
}

async fn config_id(app: &Shared, id: String) -> String {
    let selected = app.settings.read().await.theme.clone();
    if id == "default" && ["monitor-default", "komari-default"].contains(&selected.as_str()) {
        selected
    } else {
        id
    }
}
