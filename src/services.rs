// SPDX-License-Identifier: GPL-3.0-or-later
use crate::config::{ServiceCfg, VersionCfg};
use crate::util;
use serde::Serialize;
use std::collections::HashMap;
use std::process::Command;
use std::time::Duration;

#[derive(Serialize, Clone)]
pub struct ServiceStatus {
    pub id: String,
    pub name: String,
    pub group: String,
    pub unit: String,
    pub link: Option<String>,
    pub description: String,
    /// loaded / not-found / masked / ...
    pub load: String,
    /// active / inactive / failed / activating / ...
    pub active: String,
    /// running / exited / dead / ...
    pub sub: String,
    pub active_secs: Option<u64>,
    pub memory: Option<u64>,
    pub restarts: Option<u64>,
    pub pid: Option<u64>,
}

const PROPS: &str = "Id,Description,LoadState,ActiveState,SubState,ActiveEnterTimestampMonotonic,MemoryCurrent,NRestarts,MainPID";

/// Status of every configured unit from a single `systemctl show` call.
pub fn collect(services: &[ServiceCfg], uptime: f64) -> Vec<ServiceStatus> {
    if services.is_empty() {
        return Vec::new();
    }
    // Ask once per distinct unit; systemctl prints one block per argument, in order.
    let mut units: Vec<&str> = Vec::new();
    for s in services {
        if !units.contains(&s.unit.as_str()) {
            units.push(&s.unit);
        }
    }
    let mut cmd = Command::new("systemctl");
    cmd.args(["show", "--no-pager", "--property", PROPS, "--"]);
    cmd.args(&units);

    let (blocks, err): (Vec<HashMap<String, String>>, Option<String>) = match util::run(cmd, Duration::from_secs(10)) {
        Ok(o) if o.success => (
            o.stdout
                .split("\n\n")
                .filter(|b| !b.trim().is_empty())
                .map(|b| {
                    b.lines()
                        .filter_map(|l| l.split_once('='))
                        .map(|(k, v)| (k.to_string(), v.to_string()))
                        .collect()
                })
                .collect(),
            None,
        ),
        Ok(o) => (Vec::new(), Some(o.failure())),
        Err(e) => (Vec::new(), Some(e)),
    };
    let aligned = blocks.len() == units.len();
    let empty = HashMap::new();

    services
        .iter()
        .map(|s| {
            let p = match units.iter().position(|u| *u == s.unit) {
                Some(i) if aligned => &blocks[i],
                _ => &empty,
            };
            let get = |k: &str| p.get(k).map(String::as_str).unwrap_or("");
            let num = |k: &str| get(k).parse::<u64>().ok().filter(|&n| n != u64::MAX);
            let active = if aligned { get("ActiveState").to_string() } else { "unknown".into() };
            let since_boot = num("ActiveEnterTimestampMonotonic").filter(|&n| n > 0);
            ServiceStatus {
                id: s.id.clone(),
                name: s.name.clone(),
                group: s.group.clone(),
                unit: s.unit.clone(),
                link: s.link.clone(),
                description: match &err {
                    Some(e) => format!("systemctl: {e}"),
                    None => get("Description").to_string(),
                },
                load: get("LoadState").to_string(),
                sub: get("SubState").to_string(),
                active_secs: since_boot
                    .filter(|_| active == "active")
                    .map(|us| (uptime - us as f64 / 1e6).max(0.0) as u64),
                memory: num("MemoryCurrent"),
                restarts: num("NRestarts"),
                pid: num("MainPID").filter(|&n| n > 0),
                active,
            }
        })
        .collect()
}

/// The service's version, or why it couldn't be determined (shown in the UI).
pub fn version(v: &VersionCfg) -> Result<String, String> {
    if let Some(f) = &v.fixed {
        return Ok(f.clone());
    }
    let cmd = v.cmd.as_deref().ok_or("no `cmd` or `fixed` in [version]")?;
    let o = util::sh(cmd, Duration::from_secs(15))?;
    let found = if v.raw {
        // raw output of a failed command would be an error message, not a version
        o.success.then(|| o.stdout.lines().map(str::trim).find(|l| !l.is_empty()).map(String::from)).flatten()
    } else {
        util::extract_version(&format!("{}\n{}", o.stdout, o.stderr))
    };
    found.ok_or_else(|| if o.success { "no version number in the output".into() } else { o.failure() })
}
