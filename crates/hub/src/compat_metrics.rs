use crate::{
    api::{readable, require_node},
    error::{ApiError, ApiResult},
    runtime::Shared,
};
use axum::http::HeaderMap;
use serde_json::{json, Value};

const METRICS: &[(&str, &str, &str)] = &[
    ("cpu.usage", "cpu", "%"),
    ("memory.used", "mem_used", "bytes"),
    ("swap.used", "swap_used", "bytes"),
    ("disk.used", "disk_used", "bytes"),
    ("load.average", "load", ""),
    ("net.in.rate", "net_rx", "bytes/s"),
    ("net.out.rate", "net_tx", "bytes/s"),
    ("net.total.up", "total_tx", "bytes"),
    ("net.total.down", "total_rx", "bytes"),
    ("process.count", "procs", "count"),
    ("connections.tcp", "tcp", "count"),
    ("connections.udp", "udp", "count"),
    ("ping.latency_ms", "", "ms"),
    ("ping.loss", "", "ratio"),
];
fn iso(time: i64) -> String {
    chrono::DateTime::from_timestamp_millis(time)
        .unwrap_or_default()
        .to_rfc3339()
}
pub fn definitions(days: u32) -> Value {
    json!(METRICS.iter().map(|(name,_,unit)|json!({"name":name,"unit":unit,"type":if name.starts_with("net.total."){"counter"}else{"gauge"},"description":name,"retention_days":days})).collect::<Vec<_>>())
}
fn range(params: &Value, days: u32) -> ApiResult<(i64, i64, u32)> {
    let now = monitor_protocol::now_ms();
    let parse = |key: &str| {
        params[key]
            .as_str()
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .map(|d| d.timestamp_millis())
    };
    let end = parse("end")
        .or_else(|| parse("end_time"))
        .unwrap_or(now)
        .min(now);
    let hours = params["hours"]
        .as_f64()
        .unwrap_or(6.0)
        .clamp(1.0 / 60.0, days as f64 * 24.);
    let start = parse("start")
        .or_else(|| parse("start_time"))
        .unwrap_or(end - (hours * 3600000.) as i64)
        .max(now - days as i64 * 86400000);
    if start >= end {
        return Err(ApiError::bad("时间范围无效"));
    }
    Ok((
        start,
        end,
        ((now - start) as f64 / 3600000.).ceil().max(1.) as u32,
    ))
}
async fn entities(
    app: &Shared,
    headers: &HeaderMap,
    params: &Value,
) -> ApiResult<Vec<crate::db::Node>> {
    let admin = readable(app, headers).await?;
    if let Some(id) = params["entity_id"].as_str() {
        return Ok(vec![require_node(app, headers, id).await?]);
    }
    let requested = params["entity_ids"].as_array();
    Ok(app
        .db
        .nodes()
        .await?
        .into_iter()
        .filter(|n| {
            (admin || n.config.public)
                && requested.is_none_or(|ids| ids.iter().any(|id| id == &n.id))
        })
        .take(100)
        .collect())
}
pub async fn query(app: &Shared, headers: &HeaderMap, params: &Value) -> ApiResult<Value> {
    let days = app.settings.read().await.history_days;
    let (start, end, hours) = range(params, days)?;
    let points = params["max_points"].as_u64().unwrap_or(600).clamp(60, 1440) as u32;
    let keys: Vec<String> = params["metric_keys"]
        .as_array()
        .or_else(|| params["metrics"].as_array())
        .map(|k| {
            k.iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .take(32)
                .collect()
        })
        .unwrap_or_else(|| vec![params["metric_key"].as_str().unwrap_or("cpu.usage").into()]);
    let algorithm = params["aggregation"].as_str().unwrap_or("avg");
    if !["avg", "max", "last"].contains(&algorithm) {
        return Err(ApiError::bad("当前兼容层支持 avg、max、last 聚合"));
    }
    for key in &keys {
        if key.starts_with("ping.") || key.starts_with("net.total.") {
            continue;
        }
        if algorithm == "last"
            || (algorithm == "max"
                && !["cpu.usage", "net.in.rate", "net.out.rate"].contains(&key.as_str()))
        {
            return Err(ApiError::bad(format!(
                "未保存 {key} 的 {algorithm} 历史值，请使用 avg 聚合"
            )));
        }
    }
    let mut series = Vec::new();
    let probes = app.db.probes().await?;
    for node in entities(app, headers, params).await? {
        let history = app.db.history(node.id.clone(), hours, points).await?;
        let records = history["records"].as_array().cloned().unwrap_or_default();
        for key in &keys {
            if key == "ping.latency_ms" || key == "ping.loss" {
                let results = app
                    .db
                    .probe_history(Some(node.id.clone()), hours.min(168))
                    .await?;
                for (i, probe) in probes
                    .iter()
                    .enumerate()
                    .filter(|(_, p)| p.assigned(&node.id))
                {
                    let tags = json!({"task_id":(i+1).to_string()});
                    let selected: Vec<_> = results
                        .iter()
                        .filter(|r| {
                            r["task_id"] == probe.id
                                && r["time"].as_i64().is_some_and(|t| t >= start && t <= end)
                        })
                        .collect();
                    let stride = selected.len().div_ceil(points as usize).max(1);
                    let data = selected.chunks(stride).map(|chunk| {
                        let values: Vec<f64> = if key == "ping.loss" {
                            chunk.iter().map(|r| if r["success"] == true { 0. } else { 1. }).collect()
                        } else {
                            chunk.iter().filter(|r| r["success"] == true).filter_map(|r| r["latency_ms"].as_f64()).collect()
                        };
                        let value = if algorithm == "last" {
                            if key == "ping.latency_ms" && chunk.last().is_some_and(|r| r["success"] != true) { None } else { values.last().copied() }
                        } else if algorithm == "max" {
                            values.iter().copied().max_by(f64::total_cmp)
                        } else if values.is_empty() { None } else { Some(values.iter().sum::<f64>() / values.len() as f64) };
                        json!({"time":iso(chunk[0]["time"].as_i64().unwrap()),"value":value,"count":chunk.len(),"tags":tags})
                    }).collect::<Vec<_>>();
                    series.push(json!({"metric_key":key,"entity_id":node.id,"tags":tags,"unit":if key=="ping.loss"{"ratio"}else{"ms"},"type":"gauge","points":data,"count":data.len(),"interval_seconds":probe.interval,"downsampled":stride>1,"downsample_algorithm":algorithm}));
                }
                continue;
            }
            let Some((_, field, unit)) = METRICS.iter().find(|(name, _, _)| *name == key) else {
                continue;
            };
            let data=records.iter().filter(|r|r["time"].as_i64().is_some_and(|t|t>=start&&t<=end)).map(|r|{
                let value=if *field=="load"{r["load"][0].as_f64()}else if algorithm=="max"&&*field=="cpu"{r["cpu_max"].as_f64()}else if algorithm=="max"&&*field=="net_rx"{r["net_rx_max"].as_f64()}else if algorithm=="max"&&*field=="net_tx"{r["net_tx_max"].as_f64()}else{r[*field].as_f64()};
                json!({"time":iso(r["time"].as_i64().unwrap()),"value":value,"count":r["samples"]})
            }).collect::<Vec<_>>();
            let counter = key.starts_with("net.total.");
            series.push(json!({"metric_key":key,"entity_id":node.id,"type":if counter{"counter"}else{"gauge"},"unit":unit,"points":data,"count":data.len(),"interval_seconds":history["step"],"downsampled":true,"downsample_algorithm":if counter{"last"}else{algorithm},"retention_days":days}));
        }
    }
    Ok(
        json!({"start":iso(start),"end":iso(end),"count":series.len(),"series":series,"server_downsample_default":true,"default_points":600}),
    )
}
pub async fn ping_stats(app: &Shared, headers: &HeaderMap, params: &Value) -> ApiResult<Value> {
    let (start, end, hours) = range(params, app.settings.read().await.history_days)?;
    let probes = app.db.probes().await?;
    let mut stats = vec![];
    for node in entities(app, headers, params).await? {
        let records = app
            .db
            .probe_history(Some(node.id.clone()), hours.min(168))
            .await?;
        for (i, probe) in probes
            .iter()
            .enumerate()
            .filter(|(_, p)| p.assigned(&node.id))
        {
            let records = records
                .iter()
                .filter(|r| {
                    r["task_id"] == probe.id
                        && r["time"].as_i64().is_some_and(|t| t >= start && t <= end)
                })
                .collect::<Vec<_>>();
            let mut valid = records
                .iter()
                .filter(|r| r["success"] == true)
                .filter_map(|r| r["latency_ms"].as_f64())
                .collect::<Vec<_>>();
            valid.sort_by(f64::total_cmp);
            let average = if valid.is_empty() {
                None
            } else {
                Some(valid.iter().sum::<f64>() / valid.len() as f64)
            };
            let p50 = valid.get(valid.len() / 2).copied();
            let p99 = valid
                .get(((valid.len() as f64 * 0.99).ceil() as usize).saturating_sub(1))
                .copied();
            let stddev = average.map(|avg| {
                (valid.iter().map(|v| (v - avg).powi(2)).sum::<f64>() / valid.len() as f64).sqrt()
            });
            stats.push(json!({"entity_id":node.id,"task_id":(i+1).to_string(),"name":probe.name,"type":probe.kind,"interval":probe.interval,"tags":{"task_id":(i+1).to_string()},"total":records.len(),"valid":valid.len(),"loss":if records.is_empty(){0.0}else{(records.len()-valid.len()) as f64/records.len() as f64*100.0},"min":valid.first(),"max":valid.last(),"avg":average,"p50":p50,"p99":p99,"stddev":stddev,"latest":records.last().filter(|r|r["success"]==true).map(|r|r["latency_ms"].clone()),"p99_p50_ratio":match(p99,p50){(Some(a),Some(b))if b>0.0=>a/b,_=>0.0}}));
        }
    }
    Ok(json!({"start":iso(start),"end":iso(end),"count":stats.len(),"stats":stats}))
}
