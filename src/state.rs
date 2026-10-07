// SPDX-License-Identifier: GPL-3.0-or-later
use crate::config::{self, Config};
use crate::{alerts, bitcoin, logs, panels, services, system, ups};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Lock helpers that survive poisoning: a panic in one collector must not take
/// the HTTP workers (or the other collectors) down with it.
fn rd<T>(l: &RwLock<T>) -> RwLockReadGuard<'_, T> {
    l.read().unwrap_or_else(|e| e.into_inner())
}
fn wr<T>(l: &RwLock<T>) -> RwLockWriteGuard<'_, T> {
    l.write().unwrap_or_else(|e| e.into_inner())
}
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Run one collector iteration; a panic is logged and the loop carries on.
fn guarded(name: &str, f: impl FnOnce()) {
    if std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).is_err() {
        eprintln!("{name} collector panicked; continuing");
    }
}

pub fn now_unix() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// A collector result: serialises as the value itself, or `{"error": "..."}`.
#[derive(Serialize, Clone)]
#[serde(untagged)]
pub enum Probe<T> {
    Ok(T),
    Err { error: String },
}

impl<T> From<Result<T, String>> for Probe<T> {
    fn from(r: Result<T, String>) -> Self {
        match r {
            Ok(v) => Probe::Ok(v),
            Err(error) => Probe::Err { error },
        }
    }
}

#[derive(Serialize, Default)]
pub struct History {
    pub cpu: VecDeque<f64>,
    pub temp: VecDeque<f64>,
    pub mem: VecDeque<f64>,
    pub load: VecDeque<f64>,
    pub rx: VecDeque<f64>,
    pub tx: VecDeque<f64>,
}

impl History {
    fn push(&mut self, max: usize, s: &system::System) {
        let mem = (s.mem.total > 0).then(|| s.mem.used as f64 * 100.0 / s.mem.total as f64);
        let (rx, tx) = (s.net.as_ref().map(|n| n.rx_bps), s.net.as_ref().map(|n| n.tx_bps));
        // (series, value, decimals kept) — rounding keeps the JSON payload small
        for (q, v, dp) in [
            (&mut self.cpu, s.cpu_pct, 1),
            (&mut self.temp, s.temp_c, 1),
            (&mut self.mem, mem, 1),
            (&mut self.load, Some(s.load[0]), 2),
            (&mut self.rx, rx, 0),
            (&mut self.tx, tx, 0),
        ] {
            let k = 10f64.powi(dp);
            q.push_back(v.map(|x| (x * k).round() / k).unwrap_or(f64::NAN)); // NaN serialises as null
            while q.len() > max {
                q.pop_front();
            }
        }
    }
}

#[derive(Default)]
pub struct Fast {
    pub updated: u64,
    pub system: Option<system::System>,
    pub storage: Vec<system::Disk>,
    pub ups: Option<Probe<ups::Ups>>,
    pub services: Vec<services::ServiceStatus>,
    pub history: History,
}

#[derive(Default)]
pub struct Slow {
    pub bitcoin: Option<Probe<bitcoin::Bitcoin>>,
    pub logs: HashMap<String, logs::LogData>,
    pub panels: HashMap<String, panels::PanelData>,
}

struct VersionEntry {
    value: Result<String, String>,
    key: Option<config::VersionCfg>,
    at: Instant,
}

#[derive(Default)]
struct Sampler {
    prev_cpu: Option<system::CpuSample>,
    prev_net: Option<(Instant, u64, u64)>,
}

impl Sampler {
    /// Take baseline CPU/network counters so the very first sample already has rates.
    fn primed(net_iface: &str) -> Sampler {
        let s = Sampler {
            prev_cpu: system::read_cpu(),
            prev_net: system::read_net(net_iface).map(|(rx, tx)| (Instant::now(), rx, tx)),
        };
        thread::sleep(Duration::from_millis(500));
        s
    }
}

pub struct Shared {
    dir: PathBuf,
    config: RwLock<Arc<Config>>,
    hostname: String,
    started: u64,
    pub fast: RwLock<Fast>,
    pub slow: RwLock<Slow>,
    versions: Mutex<HashMap<String, VersionEntry>>,
    pub refresh_versions: AtomicBool,
}

impl Shared {
    pub fn new(dir: PathBuf, cfg: Config) -> Shared {
        Shared {
            dir,
            config: RwLock::new(Arc::new(cfg)),
            hostname: system::read_hostname(),
            started: now_unix(),
            fast: RwLock::default(),
            slow: RwLock::default(),
            versions: Mutex::default(),
            refresh_versions: AtomicBool::new(false),
        }
    }

    pub fn config(&self) -> Arc<Config> {
        rd(&self.config).clone()
    }

    fn maybe_reload(&self, sig: &mut Vec<(PathBuf, Option<SystemTime>)>) {
        let new = config::signature(&self.dir);
        if new == *sig {
            return;
        }
        *sig = new;
        let old = self.config();
        match Config::load(&self.dir, Some(&old)) {
            Ok(c) => {
                for e in &c.errors {
                    eprintln!("config: {e}");
                }
                if c.main.listen != old.main.listen {
                    eprintln!("config: 'listen' changed; restart pi-status to apply it");
                }
                eprintln!("config reloaded: {} services, {} panels, {} logs", c.services.len(), c.panels.len(), c.main.logs.len());
                *wr(&self.config) = Arc::new(c);
            }
            Err(e) => eprintln!("config reload failed: {e}"),
        }
    }

    fn sample_fast(&self, sm: &mut Sampler) {
        let cfg = self.config();
        let m = &cfg.main;

        let uptime = system::read_uptime();
        let cpu = system::read_cpu();
        let cpu_pct = match (&sm.prev_cpu, &cpu) {
            (Some(p), Some(c)) => system::cpu_pct(p, c),
            _ => None,
        };
        sm.prev_cpu = cpu;

        let net = system::read_net(&m.system.net_iface).and_then(|(rx, tx)| {
            let now = Instant::now();
            let prev = sm.prev_net.replace((now, rx, tx));
            let (t, prx, ptx) = prev?;
            let dt = now.duration_since(t).as_secs_f64();
            (dt > 0.0).then(|| system::Net {
                iface: if m.system.net_iface.is_empty() { "auto".into() } else { m.system.net_iface.clone() },
                rx_bps: rx.saturating_sub(prx) as f64 / dt,
                tx_bps: tx.saturating_sub(ptx) as f64 / dt,
            })
        });

        let sys = system::System {
            uptime_secs: uptime as u64,
            boot_time: now_unix().saturating_sub(uptime as u64),
            load: system::read_load(),
            cpu_pct,
            cpu_count: system::cpu_count(),
            temp_c: system::read_temp(&m.system.thermal_zone),
            mem: system::read_mem(),
            net,
            throttled: system::read_throttled(&m.system.throttled_cmd),
        };
        let storage = m.storage.iter().map(system::disk).collect();
        let ups = m.ups.enabled.then(|| ups::read(m.ups.bus, m.ups.address).into());
        let svcs = services::collect(&cfg.services, uptime);

        let mut f = wr(&self.fast);
        f.history.push(m.history.max(2), &sys);
        f.system = Some(sys);
        f.storage = storage;
        f.ups = ups;
        f.services = svcs;
        f.updated = now_unix();
    }

    fn sample_slow(&self, last: &mut HashMap<String, Instant>) {
        let cfg = self.config();
        let m = &cfg.main;
        let mut due = |key: String, secs: u64| -> bool {
            let ok = last.get(&key).is_none_or(|t| t.elapsed() >= Duration::from_secs(secs.max(1)));
            if ok {
                last.insert(key, Instant::now());
            }
            ok
        };

        if !m.bitcoin.enabled {
            wr(&self.slow).bitcoin = None;
        } else if due("bitcoin".into(), m.bitcoin.interval) {
            let b = bitcoin::collect(&m.bitcoin).into();
            wr(&self.slow).bitcoin = Some(b);
        }

        for l in &m.logs {
            if due(format!("log:{}", l.id), l.interval) {
                let d = logs::collect(l);
                wr(&self.slow).logs.insert(l.id.clone(), d);
            }
        }
        for p in &cfg.panels {
            if due(format!("panel:{}", p.id), p.interval) {
                let d = panels::collect(p);
                wr(&self.slow).panels.insert(p.id.clone(), d);
            }
        }

        let mut s = wr(&self.slow);
        s.logs.retain(|k, _| m.logs.iter().any(|l| &l.id == k));
        s.panels.retain(|k, _| cfg.panels.iter().any(|p| &p.id == k));
    }

    fn sample_versions(&self, force: bool) {
        let cfg = self.config();
        let ttl = Duration::from_secs(cfg.main.version_refresh.max(60));
        for s in &cfg.services {
            let stale = match lock(&self.versions).get(&s.id) {
                None => true,
                Some(e) => force || e.key != s.version || e.at.elapsed() >= ttl,
            };
            if stale {
                let value = match &s.version {
                    Some(v) => services::version(v),
                    None => Err(String::new()), // no [version] section: nothing to show
                };
                let entry = VersionEntry { value, key: s.version.clone(), at: Instant::now() };
                lock(&self.versions).insert(s.id.clone(), entry);
            }
        }
        lock(&self.versions).retain(|k, _| cfg.services.iter().any(|s| &s.id == k));
    }

    pub fn collect_once(&self) {
        let mut sm = Sampler::primed(&self.config().main.system.net_iface);
        self.sample_fast(&mut sm);
        self.sample_slow(&mut HashMap::new());
        self.sample_versions(true);
    }

    pub fn snapshot(&self) -> Value {
        let cfg = self.config();
        let fast = rd(&self.fast);
        let slow = rd(&self.slow);

        let services: Vec<Value> = {
            let versions = lock(&self.versions);
            fast.services
                .iter()
                .map(|s| {
                    let mut v = serde_json::to_value(s).unwrap_or_default();
                    let e = versions.get(&s.id).map(|e| &e.value);
                    v["version"] = e.and_then(|r| r.as_ref().ok().cloned()).into();
                    v["version_error"] = e.and_then(|r| r.as_ref().err().filter(|m| !m.is_empty()).cloned()).into();
                    v
                })
                .collect()
        };

        let logs: Vec<Value> = cfg
            .main
            .logs
            .iter()
            .map(|l| {
                let d = slow.logs.get(&l.id);
                json!({
                    "id": l.id,
                    "name": l.name,
                    "alert": l.alert,
                    "count": d.map(|d| d.lines.len()),
                    "last": d.and_then(|d| d.lines.last()),
                    "updated": d.map(|d| d.updated),
                    "error": d.and_then(|d| d.error.as_ref()),
                })
            })
            .collect();

        let panels: Vec<Value> = cfg
            .panels
            .iter()
            .map(|p| {
                let mut v = json!({ "id": p.id, "name": p.name, "format": p.format, "wide": p.wide });
                if let Some(d) = slow.panels.get(&p.id) {
                    v["data"] = serde_json::to_value(d).unwrap_or_default();
                }
                v
            })
            .collect();

        json!({
            "title": cfg.main.title,
            "hostname": self.hostname,
            "version": env!("CARGO_PKG_VERSION"),
            "now": now_unix(),
            "started": self.started,
            "updated": fast.updated,
            "interval": cfg.main.interval.max(1),
            "thresholds": cfg.main.thresholds,
            "ui": cfg.main.ui,
            "alerts": alerts::compute(&cfg, &fast, &slow),
            "system": fast.system,
            "history": fast.history,
            "storage": fast.storage,
            "ups": fast.ups,
            "services": services,
            "bitcoin": slow.bitcoin,
            "logs": logs,
            "panels": panels,
        })
    }

    pub fn log(&self, id: &str) -> Option<Value> {
        let cfg = self.config();
        let l = cfg.main.logs.iter().find(|l| l.id == id)?;
        let slow = rd(&self.slow);
        let d = slow.logs.get(id);
        Some(json!({
            "id": l.id,
            "name": l.name,
            "lines": d.map(|d| d.lines.as_slice()).unwrap_or(&[]),
            "updated": d.map(|d| d.updated),
            "error": d.and_then(|d| d.error.as_ref()),
        }))
    }
}

pub fn spawn_collectors(sh: &Arc<Shared>) {
    // Take the first fast sample before the HTTP server starts, so the very first
    // page load never sees an empty snapshot (e.g. "no services configured").
    let mut sm = Sampler::primed(&sh.config().main.system.net_iface);
    guarded("fast", || sh.sample_fast(&mut sm));

    let s = sh.clone();
    thread::Builder::new()
        .name("fast".into())
        .spawn(move || {
            let mut sig = config::signature(&s.dir);
            let mut started = Instant::now();
            loop {
                // fixed cadence: subtract the time the previous sample took
                let iv = Duration::from_secs(s.config().main.interval.max(1));
                thread::sleep(iv.saturating_sub(started.elapsed()));
                started = Instant::now();
                guarded("fast", || {
                    s.maybe_reload(&mut sig);
                    s.sample_fast(&mut sm);
                });
            }
        })
        .expect("spawn fast collector");

    let s = sh.clone();
    thread::Builder::new()
        .name("slow".into())
        .spawn(move || {
            let mut last = HashMap::new();
            loop {
                guarded("slow", || s.sample_slow(&mut last));
                thread::sleep(Duration::from_secs(1));
            }
        })
        .expect("spawn slow collector");

    let s = sh.clone();
    thread::Builder::new()
        .name("versions".into())
        .spawn(move || loop {
            let force = s.refresh_versions.swap(false, Ordering::SeqCst);
            guarded("versions", || s.sample_versions(force));
            thread::sleep(Duration::from_secs(2));
        })
        .expect("spawn version collector");
}
