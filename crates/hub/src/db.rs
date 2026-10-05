use anyhow::{Context, Result};
use chrono::{Datelike, NaiveDate, TimeZone};
use monitor_protocol::{Event, Facts, NodeInput, Probe, ProbeResult, Sample, Settings};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::HashSet,
    path::Path,
    sync::{Arc, Mutex},
};

#[derive(Clone)]
pub struct Db {
    conn: Arc<Mutex<Connection>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Node {
    pub id: String,
    pub alias: i64,
    #[serde(flatten)]
    pub config: NodeInput,
    pub facts: Facts,
    pub created_at: i64,
    pub last_seen: i64,
    pub position: i64,
}

#[derive(Clone, Default, Serialize, Deserialize, Debug)]
pub struct Traffic {
    pub total_rx: i64,
    pub total_tx: i64,
    pub month_rx: i64,
    pub month_tx: i64,
    pub day_rx: i64,
    pub day_tx: i64,
    pub month_start: String,
    pub day: String,
}
impl Traffic {
    pub fn used(&self, mode: &str) -> i64 {
        traffic_used(mode, self.month_rx, self.month_tx)
    }
}
pub fn traffic_used(mode: &str, rx: i64, tx: i64) -> i64 {
    match mode {
        "up" => tx,
        "down" => rx,
        "max" => rx.max(tx),
        _ => rx.saturating_add(tx),
    }
}

pub fn calendar_period(at: i64, timezone: &str, reset: u32) -> (NaiveDate, NaiveDate, NaiveDate) {
    let tz: chrono_tz::Tz = timezone.parse().unwrap_or(chrono_tz::Asia::Taipei);
    let day = chrono::Utc
        .timestamp_millis_opt(at)
        .single()
        .unwrap_or_else(chrono::Utc::now)
        .with_timezone(&tz)
        .date_naive();
    let date = |year: i32, month: u32| {
        let first = NaiveDate::from_ymd_opt(year, month, 1).unwrap();
        let next = if month == 12 {
            NaiveDate::from_ymd_opt(year + 1, 1, 1)
        } else {
            NaiveDate::from_ymd_opt(year, month + 1, 1)
        }
        .unwrap();
        first
            .with_day(reset.clamp(1, 31).min((next - first).num_days() as u32))
            .unwrap()
    };
    let current = date(day.year(), day.month());
    let (previous, next) = if day >= current {
        (
            current,
            if day.month() == 12 {
                date(day.year() + 1, 1)
            } else {
                date(day.year(), day.month() + 1)
            },
        )
    } else {
        (
            if day.month() == 1 {
                date(day.year() - 1, 12)
            } else {
                date(day.year(), day.month() - 1)
            },
            current,
        )
    };
    (day, previous, next)
}

impl Db {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA foreign_keys=ON; PRAGMA busy_timeout=5000; PRAGMA cache_size=-8192;
        CREATE TABLE IF NOT EXISTS setting(key TEXT PRIMARY KEY,value TEXT NOT NULL);
        CREATE TABLE IF NOT EXISTS sequence(name TEXT PRIMARY KEY,value INTEGER NOT NULL);
        CREATE TABLE IF NOT EXISTS node(id TEXT PRIMARY KEY,alias INTEGER UNIQUE NOT NULL,data TEXT NOT NULL,facts TEXT NOT NULL DEFAULT '{}',token_hash TEXT UNIQUE NOT NULL,created INTEGER NOT NULL,last_seen INTEGER NOT NULL DEFAULT 0,position INTEGER NOT NULL);
        CREATE TABLE IF NOT EXISTS traffic(node_id TEXT PRIMARY KEY REFERENCES node(id) ON DELETE CASCADE,boot TEXT NOT NULL DEFAULT '',rx INTEGER NOT NULL DEFAULT 0,tx INTEGER NOT NULL DEFAULT 0,total_rx INTEGER NOT NULL DEFAULT 0,total_tx INTEGER NOT NULL DEFAULT 0,month_rx INTEGER NOT NULL DEFAULT 0,month_tx INTEGER NOT NULL DEFAULT 0,day_rx INTEGER NOT NULL DEFAULT 0,day_tx INTEGER NOT NULL DEFAULT 0,period TEXT NOT NULL DEFAULT '',day TEXT NOT NULL DEFAULT '');
        CREATE TABLE IF NOT EXISTS traffic_day(node_id TEXT REFERENCES node(id) ON DELETE CASCADE,day TEXT,rx INTEGER NOT NULL,tx INTEGER NOT NULL,PRIMARY KEY(node_id,day));
        CREATE TABLE IF NOT EXISTS metric(node_id TEXT REFERENCES node(id) ON DELETE CASCADE,time INTEGER,data TEXT NOT NULL,n INTEGER NOT NULL,cpu_max REAL NOT NULL,rx_max INTEGER NOT NULL,tx_max INTEGER NOT NULL,PRIMARY KEY(node_id,time));
        CREATE TABLE IF NOT EXISTS metric_hour(node_id TEXT REFERENCES node(id) ON DELETE CASCADE,time INTEGER,data TEXT NOT NULL,n INTEGER NOT NULL,cpu_max REAL NOT NULL,rx_max INTEGER NOT NULL,tx_max INTEGER NOT NULL,PRIMARY KEY(node_id,time));
        CREATE TABLE IF NOT EXISTS probe(id TEXT PRIMARY KEY,data TEXT NOT NULL);
        CREATE TABLE IF NOT EXISTS probe_record(node_id TEXT REFERENCES node(id) ON DELETE CASCADE,task_id TEXT REFERENCES probe(id) ON DELETE CASCADE,time INTEGER,data TEXT NOT NULL,PRIMARY KEY(node_id,task_id,time));
        CREATE INDEX IF NOT EXISTS probe_record_time ON probe_record(time);
        CREATE TABLE IF NOT EXISTS event(id TEXT PRIMARY KEY,time INTEGER NOT NULL,kind TEXT NOT NULL,resolved INTEGER,data TEXT NOT NULL);
        CREATE INDEX IF NOT EXISTS event_time ON event(time);
        CREATE TABLE IF NOT EXISTS replay(event_id TEXT REFERENCES event(id) ON DELETE CASCADE,node_id TEXT,time INTEGER,data TEXT NOT NULL,PRIMARY KEY(event_id,node_id,time));
        CREATE TABLE IF NOT EXISTS session(hash TEXT PRIMARY KEY,created INTEGER NOT NULL,expires INTEGER NOT NULL);
        CREATE TABLE IF NOT EXISTS notification(id TEXT PRIMARY KEY,event_id TEXT NOT NULL,data TEXT NOT NULL,attempt INTEGER NOT NULL DEFAULT 0,next_at INTEGER NOT NULL,delivered INTEGER NOT NULL DEFAULT 0);
        PRAGMA user_version=1;")?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }
    pub async fn call<T: Send + 'static>(
        &self,
        f: impl FnOnce(&mut Connection) -> Result<T> + Send + 'static,
    ) -> Result<T> {
        let db = self.clone();
        tokio::task::spawn_blocking(move || {
            let mut guard = db
                .conn
                .lock()
                .map_err(|_| anyhow::anyhow!("database lock poisoned"))?;
            f(&mut guard)
        })
        .await?
    }
    pub async fn settings(&self) -> Result<Settings> {
        self.call(|c| {
            Ok(
                c.query_row("SELECT value FROM setting WHERE key='settings'", [], |r| {
                    r.get::<_, String>(0)
                })
                .optional()?
                .map(|s| serde_json::from_str(&s))
                .transpose()?
                .unwrap_or_default(),
            )
        })
        .await
    }
    pub async fn save_settings(&self, settings: Settings) -> Result<()> {
        self.set("settings", serde_json::to_string(&settings)?)
            .await
    }
    pub async fn get(&self, key: &str) -> Result<Option<String>> {
        let key = key.to_owned();
        self.call(move |c| {
            Ok(
                c.query_row("SELECT value FROM setting WHERE key=?", [key], |r| r.get(0))
                    .optional()?,
            )
        })
        .await
    }
    pub async fn set(&self, key: &str, value: String) -> Result<()> {
        let key = key.to_owned();
        self.call(move|c|{c.execute("INSERT INTO setting(key,value) VALUES(?,?) ON CONFLICT(key) DO UPDATE SET value=excluded.value",params![key,value])?;Ok(())}).await
    }
    pub async fn nodes(&self) -> Result<Vec<Node>> {
        self.call(|c| {
            let mut q=c.prepare("SELECT id,alias,data,facts,created,last_seen,position FROM node ORDER BY position,created")?;
            let raw=q.query_map([],|r|Ok((r.get::<_,String>(0)?,r.get::<_,i64>(1)?,r.get::<_,String>(2)?,r.get::<_,String>(3)?,r.get::<_,i64>(4)?,r.get::<_,i64>(5)?,r.get::<_,i64>(6)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
            raw.into_iter().map(|(id,alias,data,facts,created_at,last_seen,position)|Ok(Node{id,alias,config:serde_json::from_str(&data)?,facts:serde_json::from_str(&facts)?,created_at,last_seen,position})).collect()
        }).await
    }
    pub async fn node(&self, id: &str) -> Result<Option<Node>> {
        let id = id.to_owned();
        self.call(move|c| {
            let raw=c.query_row("SELECT id,alias,data,facts,created,last_seen,position FROM node WHERE id=?1 OR CAST(alias AS TEXT)=?1 LIMIT 1",[id],|r|Ok((r.get::<_,String>(0)?,r.get::<_,i64>(1)?,r.get::<_,String>(2)?,r.get::<_,String>(3)?,r.get::<_,i64>(4)?,r.get::<_,i64>(5)?,r.get::<_,i64>(6)?))).optional()?;
            raw.map(|(id,alias,data,facts,created_at,last_seen,position)|Ok(Node{id,alias,config:serde_json::from_str(&data)?,facts:serde_json::from_str(&facts)?,created_at,last_seen,position})).transpose()
        }).await
    }
    pub async fn add_node(
        &self,
        id: String,
        config: NodeInput,
        token_hash: String,
    ) -> Result<Node> {
        let created = monitor_protocol::now_ms();
        self.call(move |c| {
            let tx = c.transaction()?;
            let alias: i64=tx.query_row("INSERT INTO sequence(name,value) VALUES('node', (SELECT COALESCE(MAX(alias),0)+1 FROM node)) ON CONFLICT(name) DO UPDATE SET value=MAX(sequence.value+1,(SELECT COALESCE(MAX(alias),0)+1 FROM node)) RETURNING value",[],|r|r.get(0))?;
            tx.execute(
                "INSERT INTO node(id,alias,data,token_hash,created,position) VALUES(?,?,?,?,?,?)",
                params![
                    id,
                    alias,
                    serde_json::to_string(&config)?,
                    token_hash,
                    created,
                    alias
                ],
            )?;
            tx.execute("INSERT INTO traffic(node_id) VALUES(?)", [&id])?;
            tx.commit()?;
            Ok(Node {
                id,
                alias,
                config,
                facts: Facts::default(),
                created_at: created,
                last_seen: 0,
                position: alias,
            })
        })
        .await
    }
    pub async fn edit_node(&self, id: String, config: NodeInput) -> Result<()> {
        self.call(move |c| {
            anyhow::ensure!(
                c.execute(
                    "UPDATE node SET data=? WHERE id=?",
                    params![serde_json::to_string(&config)?, id]
                )? == 1,
                "node not found"
            );
            Ok(())
        })
        .await
    }
    pub async fn delete_node(&self, id: String) -> Result<()> {
        self.call(move |c| {
            c.execute("DELETE FROM node WHERE id=?", [id])?;
            Ok(())
        })
        .await
    }
    pub async fn reorder(&self, ids: Vec<String>) -> Result<bool> {
        self.call(move |c| {
            let tx = c.transaction()?;
            let requested: HashSet<_> = ids.iter().cloned().collect();
            let existing: HashSet<String> = {
                let mut query = tx.prepare("SELECT id FROM node")?;
                let rows = query.query_map([], |row| row.get(0))?;
                rows.collect::<rusqlite::Result<_>>()?
            };
            if requested.len() != ids.len() || requested != existing {
                return Ok(false);
            }
            for (i, id) in ids.iter().enumerate() {
                tx.execute(
                    "UPDATE node SET position=? WHERE id=?",
                    params![i as i64, id],
                )?;
            }
            tx.commit()?;
            Ok(true)
        })
        .await
    }
    pub async fn token_node(&self, hash: String) -> Result<Option<String>> {
        self.call(move |c| {
            Ok(
                c.query_row("SELECT id FROM node WHERE token_hash=?", [hash], |r| {
                    r.get(0)
                })
                .optional()?,
            )
        })
        .await
    }
    pub async fn rotate_token(&self, id: String, hash: String) -> Result<()> {
        self.call(move |c| {
            c.execute("UPDATE node SET token_hash=? WHERE id=?", params![hash, id])?;
            Ok(())
        })
        .await
    }
    pub async fn facts(&self, id: String, facts: Facts) -> Result<()> {
        self.call(move |c| {
            c.execute(
                "UPDATE node SET facts=? WHERE id=?",
                params![serde_json::to_string(&facts)?, id],
            )?;
            Ok(())
        })
        .await
    }
    pub async fn ingest(
        &self,
        id: String,
        sample: Sample,
        timezone: String,
        reset: u32,
    ) -> Result<Traffic> {
        self.call(move|c| {
            sample.validate()?;
            let tx=c.transaction()?;
            let (boot,rx,tx_count,total_rx,total_tx,mut month_rx,mut month_tx,mut day_rx,mut day_tx,period,day):(String,i64,i64,i64,i64,i64,i64,i64,i64,String,String)=tx.query_row("SELECT boot,rx,tx,total_rx,total_tx,month_rx,month_tx,day_rx,day_tx,period,day FROM traffic WHERE node_id=?",[&id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?,r.get(7)?,r.get(8)?,r.get(9)?,r.get(10)?)))?;
            let now=monitor_protocol::now_ms();
            let (today,start,_)=calendar_period(now,&timezone,reset);
            if period!=start.to_string(){month_rx=0;month_tx=0;}
            if day!=today.to_string(){day_rx=0;day_tx=0;}
            let (current_rx,current_tx)=(sample.net_rx_total as i64,sample.net_tx_total as i64);
            let same=!boot.is_empty()&&boot==sample.boot_id&&current_rx>=rx&&current_tx>=tx_count&&sample.available.iter().any(|s|s=="network");
            let (delta_rx,delta_tx)=if same {(current_rx-rx,current_tx-tx_count)} else {(0,0)};
            let traffic=Traffic{total_rx:total_rx.saturating_add(delta_rx),total_tx:total_tx.saturating_add(delta_tx),month_rx:month_rx.saturating_add(delta_rx),month_tx:month_tx.saturating_add(delta_tx),day_rx:day_rx.saturating_add(delta_rx),day_tx:day_tx.saturating_add(delta_tx),month_start:start.to_string(),day:today.to_string()};
            if sample.available.iter().any(|s|s=="network") {
                tx.execute("UPDATE traffic SET boot=?,rx=?,tx=?,total_rx=?,total_tx=?,month_rx=?,month_tx=?,day_rx=?,day_tx=?,period=?,day=? WHERE node_id=?",params![sample.boot_id,current_rx,current_tx,traffic.total_rx,traffic.total_tx,traffic.month_rx,traffic.month_tx,traffic.day_rx,traffic.day_tx,traffic.month_start,traffic.day,id])?;
                tx.execute("INSERT INTO traffic_day VALUES(?,?,?,?) ON CONFLICT(node_id,day) DO UPDATE SET rx=MIN(9223372036854775807,rx+excluded.rx),tx=MIN(9223372036854775807,tx+excluded.tx)",params![id,today.to_string(),delta_rx,delta_tx])?;
            }
            tx.execute("UPDATE node SET last_seen=? WHERE id=?",params![now,id])?;
            let minute=now/60000*60000;
            let old:Option<(String,i64,f64,i64,i64)>=tx.query_row("SELECT data,n,cpu_max,rx_max,tx_max FROM metric WHERE node_id=? AND time=?",params![id,minute],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).optional()?;
            let (mut data,n,cpu_max,rx_max,tx_max)=if let Some((old,n,cm,rm,tm))=old {
                (merge_values(serde_json::from_str(&old)?,serde_json::to_value(&sample)?,n,1),n+1,cm.max(sample.cpu),rm.max(sample.net_rx as i64),tm.max(sample.net_tx as i64))
            } else {(public_sample(&sample),1,sample.cpu,sample.net_rx as i64,sample.net_tx as i64)};
            data["total_rx"]=json!(traffic.total_rx);data["total_tx"]=json!(traffic.total_tx);tx.execute("INSERT OR REPLACE INTO metric VALUES(?,?,?,?,?,?,?)",params![id,minute,data.to_string(),n,cpu_max,rx_max,tx_max])?;
            tx.commit()?;Ok(traffic)
        }).await
    }
    pub async fn traffic(&self, id: String, timezone: String, reset: u32) -> Result<Traffic> {
        self.call(move|c| {
            let (_,period,_)=calendar_period(monitor_protocol::now_ms(),&timezone,reset);
            let (day,_,_)=calendar_period(monitor_protocol::now_ms(),&timezone,reset);
            let mut t=c.query_row("SELECT total_rx,total_tx,month_rx,month_tx,day_rx,day_tx,period,day FROM traffic WHERE node_id=?",[id],|r|Ok(Traffic{total_rx:r.get(0)?,total_tx:r.get(1)?,month_rx:r.get(2)?,month_tx:r.get(3)?,day_rx:r.get(4)?,day_tx:r.get(5)?,month_start:r.get(6)?,day:r.get(7)?})).optional()?.unwrap_or_default();
            if t.month_start!=period.to_string(){t.month_rx=0;t.month_tx=0;t.month_start=period.to_string();}
            if t.day!=day.to_string(){t.day_rx=0;t.day_tx=0;t.day=day.to_string();} Ok(t)
        }).await
    }
    pub async fn daily_traffic(&self, id: String) -> Result<Vec<Value>> {
        self.call(move|c| {
            let mut q=c.prepare("SELECT day,rx,tx FROM traffic_day WHERE node_id=? ORDER BY day DESC LIMIT 14")?;
            let rows=q.query_map([id],|r|Ok(json!({"day":r.get::<_,String>(0)?,"rx":r.get::<_,i64>(1)?,"tx":r.get::<_,i64>(2)?})))?.collect::<rusqlite::Result<Vec<_>>>()?;Ok(rows)
        }).await
    }
    pub async fn history(&self, id: String, hours: u32, points: u32) -> Result<Value> {
        self.call(move|c| {
            let now=monitor_protocol::now_ms();let since=now-hours as i64*3600000;
            let step=if hours>168 {3600000} else {60000};
            let bucket=((hours as i64*3600000/points.max(1) as i64+step-1)/step).max(1)*step;
            let sql=if hours>168 {"SELECT time,data,n,cpu_max,rx_max,tx_max FROM metric_hour WHERE node_id=?1 AND time>=?2 UNION ALL SELECT time,data,n,cpu_max,rx_max,tx_max FROM metric WHERE node_id=?1 AND time>=?2 AND time>COALESCE((SELECT MAX(time)+3599999 FROM metric_hour WHERE node_id=?1),0) ORDER BY time"} else {"SELECT time,data,n,cpu_max,rx_max,tx_max FROM metric WHERE node_id=?1 AND time>=?2 ORDER BY time"};
            let mut q=c.prepare(sql)?;
            let rows=q.query_map(params![id,since],|r|Ok((r.get::<_,i64>(0)?,r.get::<_,String>(1)?,r.get::<_,i64>(2)?,r.get::<_,f64>(3)?,r.get::<_,i64>(4)?,r.get::<_,i64>(5)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
            let mut groups=std::collections::BTreeMap::<i64,(Value,i64,f64,i64,i64,u32)>::new();
            for (time,data,n,cm,rm,tm) in rows {
                let time=time/bucket*bucket;let data:Value=serde_json::from_str(&data)?;let observed_minutes=data["minutes"].as_u64().unwrap_or(1) as u32;
                groups.entry(time).and_modify(|(d,count,cpu,rx,tx,minutes)|{*d=merge_values(d.clone(),data.clone(),*count,n);*count+=n;*cpu=cpu.max(cm);*rx=(*rx).max(rm);*tx=(*tx).max(tm);*minutes+=observed_minutes;}).or_insert((data,n,cm,rm,tm,observed_minutes));
            }
            let records:Vec<Value>=groups.into_iter().map(|(time,(mut d,n,cm,rm,tm,minutes))|{d["time"]=json!(time);d["samples"]=json!(n);d["minutes"]=json!(minutes);d["cpu_max"]=json!(cm);d["net_rx_max"]=json!(rm);d["net_tx_max"]=json!(tm);d}).collect();
            Ok(json!({"step":bucket/1000,"records":records,"count":records.len(),"hours":hours}))
        }).await
    }
    pub async fn probes(&self) -> Result<Vec<Probe>> {
        self.call(|c| {
            let mut q = c.prepare("SELECT data FROM probe ORDER BY rowid")?;
            let rows = q
                .query_map([], |r| r.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            rows.into_iter()
                .map(|s| Ok(serde_json::from_str(&s)?))
                .collect()
        })
        .await
    }
    pub async fn save_probe(&self, probe: Probe) -> Result<()> {
        self.call(move|c|{c.execute("INSERT INTO probe(id,data) VALUES(?,?) ON CONFLICT(id) DO UPDATE SET data=excluded.data",params![probe.id,serde_json::to_string(&probe)?])?;Ok(())}).await
    }
    pub async fn delete_probe(&self, id: String) -> Result<()> {
        self.call(move |c| {
            c.execute("DELETE FROM probe WHERE id=?", [id])?;
            Ok(())
        })
        .await
    }
    pub async fn probe_result(&self, id: String, result: ProbeResult) -> Result<()> {
        self.call(move |c| {
            result.validate()?;
            c.execute(
                "INSERT OR REPLACE INTO probe_record VALUES(?,?,?,?)",
                params![
                    id,
                    result.task_id,
                    result.time,
                    serde_json::to_string(&result)?
                ],
            )?;
            Ok(())
        })
        .await
    }
    pub async fn probe_history(&self, id: Option<String>, hours: u32) -> Result<Vec<Value>> {
        self.call(move|c| {
            let mut q=c.prepare("SELECT node_id,data FROM probe_record WHERE time>=?1 AND (?2 IS NULL OR node_id=?2) ORDER BY time DESC LIMIT 30000")?;
            let rows=q.query_map(params![monitor_protocol::now_ms()-hours as i64*3600000,id],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
            rows.into_iter().rev().map(|(node,data)|{let mut data:Value=serde_json::from_str(&data)?;data["node_id"]=json!(node);Ok(data)}).collect()
        }).await
    }
    pub async fn add_event(&self, event: Event) -> Result<()> {
        self.call(move |c| {
            c.execute(
                "INSERT OR IGNORE INTO event VALUES(?,?,?,?,?)",
                params![
                    event.id,
                    event.time,
                    event.kind,
                    event.resolved_at,
                    serde_json::to_string(&event)?
                ],
            )?;
            Ok(())
        })
        .await
    }
    pub async fn events(&self) -> Result<Vec<Event>> {
        self.call(|c| {
            let mut q = c.prepare("SELECT data FROM event ORDER BY time DESC LIMIT 500")?;
            let rows = q
                .query_map([], |r| r.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            rows.into_iter()
                .map(|s| Ok(serde_json::from_str(&s)?))
                .collect()
        })
        .await
    }
    pub async fn resolve(&self, id: String) -> Result<()> {
        self.call(move |c| {
            let data: Option<String> = c
                .query_row("SELECT data FROM event WHERE id=?", [&id], |r| r.get(0))
                .optional()?;
            if let Some(data) = data {
                let mut e: Event = serde_json::from_str(&data)?;
                e.resolved_at = Some(monitor_protocol::now_ms());
                c.execute(
                    "UPDATE event SET data=?,resolved=? WHERE id=?",
                    params![serde_json::to_string(&e)?, e.resolved_at, id],
                )?;
            }
            Ok(())
        })
        .await
    }
    pub async fn save_replay(
        &self,
        event_id: String,
        node_id: String,
        samples: Vec<Sample>,
    ) -> Result<()> {
        self.call(move |c| {
            let tx = c.transaction()?;
            for sample in samples.into_iter().take(monitor_protocol::REPLAY_SAMPLES) {
                sample.validate()?;
                tx.execute(
                    "INSERT OR REPLACE INTO replay VALUES(?,?,?,?)",
                    params![
                        event_id,
                        node_id,
                        sample.time,
                        public_sample(&sample).to_string()
                    ],
                )?;
            }
            tx.execute("DELETE FROM replay WHERE event_id=?1 AND node_id=?2 AND time NOT IN (SELECT time FROM (SELECT time FROM replay WHERE event_id=?1 AND node_id=?2 ORDER BY ABS(time-(SELECT time FROM event WHERE id=?1)) LIMIT 360) UNION SELECT time FROM (SELECT time FROM replay WHERE event_id=?1 AND node_id=?2 ORDER BY time DESC LIMIT 360))",params![event_id,node_id])?;
            tx.commit()?;
            Ok(())
        })
        .await
    }
    pub async fn replay(&self, id: String) -> Result<Vec<Value>> {
        self.call(move |c| {
            let mut q = c.prepare(
                "SELECT node_id,data FROM replay WHERE event_id=? ORDER BY time LIMIT 2400",
            )?;
            let rows = q
                .query_map([id], |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            rows.into_iter()
                .map(|(node, data)| {
                    let mut data: Value = serde_json::from_str(&data)?;
                    data["node_id"] = json!(node);
                    Ok(data)
                })
                .collect()
        })
        .await
    }
    pub async fn add_session(&self, hash: String) -> Result<()> {
        self.call(move |c| {
            let now = monitor_protocol::now_ms();
            c.execute(
                "INSERT INTO session VALUES(?,?,?)",
                params![hash, now, now + 14 * 86400000],
            )?;
            Ok(())
        })
        .await
    }
    pub async fn valid_session(&self, hash: String) -> Result<bool> {
        self.call(move |c| {
            Ok(c.query_row(
                "SELECT 1 FROM session WHERE hash=? AND expires>?",
                params![hash, monitor_protocol::now_ms()],
                |_| Ok(true),
            )
            .optional()?
            .unwrap_or(false))
        })
        .await
    }
    pub async fn sessions(&self) -> Result<Vec<Value>> {
        self.call(|c|{let mut q=c.prepare("SELECT hash,created,expires FROM session WHERE expires>? ORDER BY created DESC")?;let rows=q.query_map([monitor_protocol::now_ms()],|r|Ok(json!({"id":r.get::<_,String>(0)?,"created_at":r.get::<_,i64>(1)?,"expires_at":r.get::<_,i64>(2)?})))?.collect::<rusqlite::Result<Vec<_>>>()?;Ok(rows)}).await
    }
    pub async fn remove_session(&self, hash: Option<String>) -> Result<()> {
        self.call(move |c| {
            if let Some(hash) = hash {
                c.execute("DELETE FROM session WHERE hash=?", [hash])?;
            } else {
                c.execute("DELETE FROM session", [])?;
            }
            Ok(())
        })
        .await
    }
    pub async fn maintain(&self, days: u32) -> Result<()> {
        self.call(move|c| {
            let now=monitor_protocol::now_ms();let before=(now/3600000-1)*3600000;
            let rows={let mut q=c.prepare("SELECT m.node_id,m.time,m.data,m.n,m.cpu_max,m.rx_max,m.tx_max FROM metric m LEFT JOIN metric_hour h ON h.node_id=m.node_id AND h.time=(m.time/3600000)*3600000 WHERE m.time<? AND h.node_id IS NULL ORDER BY m.node_id,m.time")?;let rows=q.query_map([before],|r|Ok((r.get::<_,String>(0)?,r.get::<_,i64>(1)?,r.get::<_,String>(2)?,r.get::<_,i64>(3)?,r.get::<_,f64>(4)?,r.get::<_,i64>(5)?,r.get::<_,i64>(6)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;rows};
            let mut groups=std::collections::BTreeMap::<(String,i64),(Value,i64,f64,i64,i64,u32)>::new();
            for (id,time,data,n,cm,rm,tm) in rows {let data:Value=serde_json::from_str(&data)?;groups.entry((id,time/3600000*3600000)).and_modify(|(d,count,cpu,rx,tx,minutes)|{*d=merge_values(d.clone(),data.clone(),*count,n);*count+=n;*cpu=cpu.max(cm);*rx=(*rx).max(rm);*tx=(*tx).max(tm);*minutes+=1;}).or_insert((data,n,cm,rm,tm,1));}
            let tx=c.transaction()?;
            for ((id,time),(mut data,n,cm,rm,tm,minutes)) in groups {data["minutes"]=json!(minutes);tx.execute("INSERT OR REPLACE INTO metric_hour VALUES(?,?,?,?,?,?,?)",params![id,time,data.to_string(),n,cm,rm,tm])?;}
            let since=now-days as i64*86400000;
            tx.execute("DELETE FROM metric WHERE time<?",[since.max(now-7*86400000)])?;
            tx.execute("DELETE FROM metric_hour WHERE time<?",[since])?;
            tx.execute("DELETE FROM probe_record WHERE time<?",[since])?;
            tx.execute("DELETE FROM event WHERE time<?",[since])?;
            tx.execute("DELETE FROM traffic_day WHERE day<date('now','-400 days')",[])?;
            tx.execute("DELETE FROM session WHERE expires<?",[now])?;
            tx.commit()?;Ok(())
        }).await
    }
    pub async fn backup(&self, path: std::path::PathBuf) -> Result<()> {
        self.call(move |c| {
            let mut target = Connection::open(path)?;
            let backup = rusqlite::backup::Backup::new(c, &mut target)?;
            backup.run_to_completion(128, std::time::Duration::from_millis(5), None)?;
            Ok(())
        })
        .await
    }
    pub async fn restore(&self, path: std::path::PathBuf) -> Result<()> {
        self.call(move |c| {
            let source =
                Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
            let integrity: String = source.query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
            anyhow::ensure!(integrity == "ok", "备份数据库损坏");
            let version: i64 = source.query_row("PRAGMA user_version", [], |r| r.get(0))?;
            anyhow::ensure!(version == 1, "备份版本不受支持");
            for (table, columns) in [
                ("setting", "key,value"), ("sequence", "name,value"),
                ("node", "id,alias,data,facts,token_hash,created,last_seen,position"),
                ("traffic", "node_id,boot,rx,tx,total_rx,total_tx,month_rx,month_tx,day_rx,day_tx,period,day"),
                ("traffic_day", "node_id,day,rx,tx"),
                ("metric", "node_id,time,data,n,cpu_max,rx_max,tx_max"),
                ("metric_hour", "node_id,time,data,n,cpu_max,rx_max,tx_max"),
                ("probe", "id,data"), ("probe_record", "node_id,task_id,time,data"),
                ("event", "id,time,kind,resolved,data"), ("replay", "event_id,node_id,time,data"),
                ("session", "hash,created,expires"), ("notification", "id,event_id,data,attempt,next_at,delivered"),
            ] {
                source
                    .prepare(&format!("SELECT {columns} FROM {table} LIMIT 0"))
                    .context("不是 Monitor 的备份")?;
            }
            anyhow::ensure!(!source.prepare("PRAGMA foreign_key_check")?.exists([])?, "备份中存在无效关联");
            let backup = rusqlite::backup::Backup::new(&source, c)?;
            backup.run_to_completion(128, std::time::Duration::from_millis(5), None)?;
            drop(backup);
            c.execute("DELETE FROM session", [])?;
            Ok(())
        })
        .await
    }
}

pub fn public_sample(sample: &Sample) -> Value {
    json!({"time":sample.time,"cpu":sample.cpu,"cpu_steal":sample.cpu_steal,"load":sample.load,
        "mem_total":sample.mem_total,"mem_used":sample.mem_used,"swap_total":sample.swap_total,"swap_used":sample.swap_used,
        "disk_total":sample.disk_total,"disk_used":sample.disk_used,"net_rx":sample.net_rx,"net_tx":sample.net_tx,
        "tcp":sample.tcp,"udp":sample.udp,"procs":sample.procs,"uptime":sample.uptime,"pressure":sample.pressure,"available":sample.available})
}
fn merge_values(mut old: Value, new: Value, old_n: i64, new_n: i64) -> Value {
    for key in ["total_rx", "total_tx"] {
        if new[key].is_number() {
            old[key] = new[key].clone();
        }
    }
    for key in [
        "cpu",
        "cpu_steal",
        "mem_total",
        "mem_used",
        "swap_total",
        "swap_used",
        "disk_total",
        "disk_used",
        "net_rx",
        "net_tx",
        "tcp",
        "udp",
        "procs",
        "uptime",
    ] {
        if let (Some(a), Some(b)) = (old[key].as_f64(), new[key].as_f64()) {
            old[key] = json!((a * old_n as f64 + b * new_n as f64) / (old_n + new_n) as f64);
        }
    }
    for i in 0..3 {
        if let (Some(a), Some(b)) = (old["load"][i].as_f64(), new["load"][i].as_f64()) {
            old["load"][i] = json!((a * old_n as f64 + b * new_n as f64) / (old_n + new_n) as f64);
        }
    }
    for key in ["cpu", "memory", "io"] {
        if let Some(b) = new["pressure"][key].as_f64() {
            let a = old["pressure"][key].as_f64().unwrap_or(b);
            old["pressure"][key] =
                json!((a * old_n as f64 + b * new_n as f64) / (old_n + new_n) as f64);
        }
    }
    old
}

#[cfg(test)]
mod tests {
    use super::*;
    async fn setup() -> (Db, String) {
        let db = Db::open(Path::new(":memory:")).unwrap();
        let id = "node".to_owned();
        db.add_node(
            id.clone(),
            NodeInput {
                name: "test".into(),
                ..Default::default()
            },
            "token".into(),
        )
        .await
        .unwrap();
        (db, id)
    }
    fn sample(boot: &str, rx: u64) -> Sample {
        Sample {
            time: monitor_protocol::now_ms(),
            boot_id: boot.into(),
            net_rx_total: rx,
            available: vec!["network".into()],
            ..Default::default()
        }
    }
    #[tokio::test]
    async fn traffic_survives_reconnect_and_rebases_after_reboot_or_nic_change() {
        let (db, id) = setup().await;
        for (boot, rx, expected) in [
            ("a", 1000, 0),
            ("a", 1200, 200),
            ("a", 1500, 500),
            ("b", 300000, 500),
            ("b", 300050, 550),
            ("b", 3, 550),
            ("b", 13, 560),
        ] {
            let t = db
                .ingest(id.clone(), sample(boot, rx), "Asia/Taipei".into(), 1)
                .await
                .unwrap();
            assert_eq!(t.total_rx, expected);
        }
    }
    #[tokio::test]
    async fn missing_network_reading_does_not_rebase_the_counter() {
        let (db, id) = setup().await;
        db.ingest(id.clone(), sample("a", 1000), "UTC".into(), 1)
            .await
            .unwrap();
        let mut absent = sample("a", 0);
        absent.available.clear();
        db.ingest(id.clone(), absent, "UTC".into(), 1)
            .await
            .unwrap();
        assert_eq!(
            db.ingest(id, sample("a", 1100), "UTC".into(), 1)
                .await
                .unwrap()
                .total_rx,
            100
        );
    }
    #[test]
    fn month_boundary_clamps_the_31st_and_respects_timezone() {
        let ts = chrono::DateTime::parse_from_rfc3339("2026-02-28T16:00:00Z")
            .unwrap()
            .timestamp_millis();
        let (day, start, end) = calendar_period(ts, "Asia/Taipei", 31);
        assert_eq!(day.to_string(), "2026-03-01");
        assert_eq!(start.to_string(), "2026-02-28");
        assert_eq!(end.to_string(), "2026-03-31");
    }
    #[tokio::test]
    async fn external_node_ids_are_not_reused_after_deletion() {
        let (db, id) = setup().await;
        let first = db.node(&id).await.unwrap().unwrap().alias;
        db.delete_node(id).await.unwrap();
        let second = db
            .add_node(
                "new-node".into(),
                NodeInput {
                    name: "new".into(),
                    ..Default::default()
                },
                "new-token".into(),
            )
            .await
            .unwrap();
        assert!(second.alias > first);
    }
    #[tokio::test]
    async fn hourly_rollup_preserves_observed_minutes_averages_and_peaks() {
        let (db, id) = setup().await;
        let node = id.clone();
        let hour = (monitor_protocol::now_ms() / 3600000 - 10) * 3600000;
        db.call(move|c|{for minute in 0..60i64{let data=json!({"time":hour+minute*60000,"cpu":minute as f64,"mem_used":100,"load":[0,0,0],"pressure":{"cpu":null,"memory":null,"io":null}});c.execute("INSERT INTO metric VALUES(?,?,?,?,?,?,?)",params![node,hour+minute*60000,data.to_string(),12,minute as f64,1,2])?;}Ok(())}).await.unwrap();
        db.maintain(30).await.unwrap();
        let history = db.history(id, 200, 1440).await.unwrap();
        let points = history["records"].as_array().unwrap();
        assert_eq!(points.len(), 1);
        assert_eq!(points[0]["minutes"], 60);
        assert_eq!(points[0]["cpu_max"], 59.);
        assert!((points[0]["cpu"].as_f64().unwrap() - 29.5).abs() < 0.001);
    }
}
