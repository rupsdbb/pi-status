// SPDX-License-Identifier: GPL-3.0-or-later
use crate::config::Config;
use crate::state::{Fast, Probe, Slow};
use serde::Serialize;

#[derive(Serialize)]
pub struct Alert {
    /// "crit", "warn" or "info"
    pub level: &'static str,
    /// Which card the alert belongs to, so the UI can link to it.
    pub source: &'static str,
    pub text: String,
}

fn rank(level: &str) -> u8 {
    match level {
        "crit" => 0,
        "warn" => 1,
        _ => 2,
    }
}

pub fn compute(cfg: &Config, fast: &Fast, slow: &Slow) -> Vec<Alert> {
    let t = &cfg.main.thresholds;
    let mut out = Vec::new();
    let mut add = |level, source, text: String| out.push(Alert { level, source, text });

    for e in &cfg.errors {
        add("warn", "config", format!("Config: {}", e.lines().next().unwrap_or(e)));
    }

    if let Some(s) = &fast.system {
        if let Some(c) = s.temp_c {
            if c >= t.temp_crit {
                add("crit", "system", format!("CPU temperature is {c:.1} °C"));
            } else if c >= t.temp_warn {
                add("warn", "system", format!("CPU temperature is {c:.1} °C"));
            }
        }
        if let Some(c) = s.cpu_pct.filter(|&c| c >= t.cpu_warn) {
            add("warn", "system", format!("CPU usage is {c:.0}%"));
        }
        if s.mem.total > 0 {
            let pct = s.mem.used as f64 * 100.0 / s.mem.total as f64;
            if pct >= t.mem_warn {
                add("warn", "system", format!("Memory usage is {pct:.0}%"));
            }
        }
        match &s.throttled {
            Some(Probe::Ok(th)) => {
                if th.undervoltage_now {
                    add("crit", "power", "Under-voltage detected right now".into());
                } else if th.undervoltage_seen {
                    add("warn", "power", "Under-voltage occurred since boot".into());
                }
                if th.throttled_now || th.freq_capped_now {
                    add("warn", "power", "CPU is currently throttled".into());
                }
            }
            Some(Probe::Err { error }) => add("warn", "power", format!("Throttle status unavailable: {error}")),
            None => {}
        }
    }

    for d in &fast.storage {
        if let Some(e) = &d.error {
            add("warn", "storage", format!("{} ({}): {e}", d.label, d.path));
        } else if let Some(p) = d.used_pct() {
            if p >= t.disk_crit {
                add("crit", "storage", format!("{} is {p:.0}% full", d.label));
            } else if p >= t.disk_warn {
                add("warn", "storage", format!("{} is {p:.0}% full", d.label));
            }
        }
    }

    match &fast.ups {
        Some(Probe::Ok(u)) => {
            let p = u.percent as f64;
            if u.on_battery {
                add("warn", "power", format!("Running on battery ({}%)", u.percent));
            }
            if p <= t.battery_crit {
                add("crit", "power", format!("Battery critically low ({}%)", u.percent));
            } else if p <= t.battery_warn && u.on_battery {
                add("warn", "power", format!("Battery low ({}%)", u.percent));
            }
        }
        Some(Probe::Err { error }) => add("warn", "power", format!("UPS unavailable: {error}")),
        None => {}
    }

    for s in &fast.services {
        match (s.load.as_str(), s.active.as_str()) {
            ("not-found", _) => add("warn", "services", format!("{}: unit '{}' not found", s.name, s.unit)),
            (_, "failed") => add("crit", "services", format!("{} has failed", s.name)),
            (_, "inactive") => add("warn", "services", format!("{} is stopped", s.name)),
            (_, "active") => {}
            (_, "unknown") => add("warn", "services", format!("{}: status unknown", s.name)),
            (_, other) => add("info", "services", format!("{} is {other}", s.name)),
        }
    }

    match &slow.bitcoin {
        Some(Probe::Ok(b)) => {
            if b.ibd {
                add("info", "bitcoin", format!("Bitcoin is syncing ({:.2}%)", b.progress * 100.0));
            }
            if b.peers == 0 {
                add("warn", "bitcoin", "Bitcoin node has no peers".into());
            }
            for w in &b.warnings {
                add("warn", "bitcoin", format!("Bitcoin: {w}"));
            }
        }
        Some(Probe::Err { error }) => add("warn", "bitcoin", format!("Bitcoin RPC: {error}")),
        None => {}
    }

    for l in &cfg.main.logs {
        if let Some(d) = slow.logs.get(&l.id) {
            if let Some(e) = &d.error {
                add("warn", "logs", format!("{}: {e}", l.name));
            } else if l.alert && !d.lines.is_empty() {
                add("warn", "logs", format!("{}: {} entries", l.name, d.lines.len()));
            }
        }
    }

    for p in &cfg.panels {
        if let Some(e) = slow.panels.get(&p.id).and_then(|d| d.error.as_ref()) {
            add("warn", "panels", format!("{}: {e}", p.name));
        }
    }

    out.sort_by_key(|a| rank(a.level));
    out
}
