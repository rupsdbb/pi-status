// SPDX-License-Identifier: GPL-3.0-or-later
use crate::config::MountCfg;
use crate::state::Probe;
use crate::util;
use serde::Serialize;
use std::ffi::CString;
use std::fs;
use std::time::Duration;

#[derive(Serialize, Clone)]
pub struct System {
    pub uptime_secs: u64,
    pub boot_time: u64,
    pub load: [f64; 3],
    pub cpu_pct: Option<f64>,
    pub cpu_count: usize,
    pub temp_c: Option<f64>,
    pub mem: Mem,
    pub net: Option<Net>,
    pub throttled: Option<Probe<Throttled>>,
}

#[derive(Serialize, Clone, Default)]
pub struct Mem {
    pub total: u64,
    pub available: u64,
    pub used: u64,
    pub swap_total: u64,
    pub swap_used: u64,
}

#[derive(Serialize, Clone)]
pub struct Net {
    pub iface: String,
    pub rx_bps: f64,
    pub tx_bps: f64,
}

#[derive(Serialize, Clone)]
pub struct Throttled {
    pub raw: String,
    pub undervoltage_now: bool,
    pub freq_capped_now: bool,
    pub throttled_now: bool,
    pub soft_temp_limit_now: bool,
    pub undervoltage_seen: bool,
    pub freq_capped_seen: bool,
    pub throttled_seen: bool,
    pub soft_temp_limit_seen: bool,
}

#[derive(Serialize, Clone)]
pub struct Disk {
    pub label: String,
    pub path: String,
    pub device: Option<String>,
    pub total: u64,
    pub used: u64,
    pub avail: u64,
    pub error: Option<String>,
}

impl Disk {
    pub fn used_pct(&self) -> Option<f64> {
        let denom = self.used + self.avail;
        (self.error.is_none() && denom > 0).then(|| self.used as f64 * 100.0 / denom as f64)
    }
}

#[derive(Clone, Copy)]
pub struct CpuSample {
    total: u64,
    idle: u64,
}

pub fn read_cpu() -> Option<CpuSample> {
    let s = fs::read_to_string("/proc/stat").ok()?;
    let vals: Vec<u64> = s.lines().next()?.split_whitespace().skip(1).filter_map(|x| x.parse().ok()).collect();
    if vals.len() < 5 {
        return None;
    }
    // user nice system idle iowait irq softirq steal (guest is already counted in user)
    Some(CpuSample { total: vals.iter().take(8).sum(), idle: vals[3] + vals[4] })
}

pub fn cpu_pct(prev: &CpuSample, cur: &CpuSample) -> Option<f64> {
    let dt = cur.total.checked_sub(prev.total)?;
    let di = cur.idle.checked_sub(prev.idle)?;
    (dt > 0).then(|| (dt.saturating_sub(di)) as f64 * 100.0 / dt as f64)
}

pub fn cpu_count() -> usize {
    std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1)
}

pub fn read_temp(path: &str) -> Option<f64> {
    let v: f64 = fs::read_to_string(path).ok()?.trim().parse().ok()?;
    Some(v / 1000.0)
}

pub fn read_uptime() -> f64 {
    fs::read_to_string("/proc/uptime")
        .ok()
        .and_then(|s| s.split_whitespace().next()?.parse().ok())
        .unwrap_or(0.0)
}

pub fn read_load() -> [f64; 3] {
    let s = fs::read_to_string("/proc/loadavg").unwrap_or_default();
    let mut it = s.split_whitespace().map(|x| x.parse().unwrap_or(0.0));
    [it.next().unwrap_or(0.0), it.next().unwrap_or(0.0), it.next().unwrap_or(0.0)]
}

pub fn read_mem() -> Mem {
    let s = fs::read_to_string("/proc/meminfo").unwrap_or_default();
    let get = |key: &str| -> u64 {
        s.lines()
            .find(|l| l.starts_with(key) && l[key.len()..].starts_with(':'))
            .and_then(|l| l[key.len() + 1..].split_whitespace().next()?.parse::<u64>().ok())
            .unwrap_or(0)
            * 1024
    };
    let total = get("MemTotal");
    let available = get("MemAvailable");
    let swap_total = get("SwapTotal");
    Mem {
        total,
        available,
        used: total.saturating_sub(available),
        swap_total,
        swap_used: swap_total.saturating_sub(get("SwapFree")),
    }
}

/// Cumulative (rx, tx) bytes for `iface`. When empty, sums the physical
/// interfaces only (bridges, veth pairs and tunnels would double-count traffic),
/// falling back to everything except `lo` if none are found.
pub fn read_net(iface: &str) -> Option<(u64, u64)> {
    let s = fs::read_to_string("/proc/net/dev").ok()?;
    let names: Vec<&str> = s.lines().skip(2).filter_map(|l| Some(l.split_once(':')?.0.trim())).collect();
    let physical: Vec<&str> = names
        .iter()
        .copied()
        .filter(|n| std::path::Path::new(&format!("/sys/class/net/{n}/device")).exists())
        .collect();
    let wanted = |name: &str| {
        if !iface.is_empty() {
            name == iface
        } else if !physical.is_empty() {
            physical.contains(&name)
        } else {
            name != "lo"
        }
    };
    let mut found = false;
    let (mut rx, mut tx) = (0u64, 0u64);
    for line in s.lines().skip(2) {
        let Some((name, rest)) = line.split_once(':') else { continue };
        if wanted(name.trim()) {
            let f: Vec<u64> = rest.split_whitespace().filter_map(|x| x.parse().ok()).collect();
            if f.len() >= 9 {
                rx += f[0];
                tx += f[8];
                found = true;
            }
        }
    }
    found.then_some((rx, tx))
}

pub fn read_throttled(cmd: &str) -> Option<Probe<Throttled>> {
    if cmd.trim().is_empty() {
        return None;
    }
    let res = util::sh(cmd, Duration::from_secs(5)).and_then(|o| {
        if !o.success {
            return Err(o.failure());
        }
        let raw = o.stdout.trim().rsplit('=').next().unwrap_or("").trim().to_string();
        let v = u32::from_str_radix(raw.trim_start_matches("0x"), 16)
            .map_err(|_| format!("unexpected output: {}", o.stdout.trim()))?;
        let bit = |n: u32| v & (1 << n) != 0;
        Ok(Throttled {
            raw: format!("0x{v:x}"),
            undervoltage_now: bit(0),
            freq_capped_now: bit(1),
            throttled_now: bit(2),
            soft_temp_limit_now: bit(3),
            undervoltage_seen: bit(16),
            freq_capped_seen: bit(17),
            throttled_seen: bit(18),
            soft_temp_limit_seen: bit(19),
        })
    });
    Some(res.into())
}

pub fn read_hostname() -> String {
    fs::read_to_string("/proc/sys/kernel/hostname").map(|s| s.trim().to_string()).unwrap_or_default()
}

/// Source device of the filesystem mounted exactly at `path`, if any.
fn mount_source(path: &str) -> Option<String> {
    let unescape = |s: &str| s.replace("\\040", " ").replace("\\011", "\t").replace("\\134", "\\");
    fs::read_to_string("/proc/self/mounts")
        .ok()?
        .lines()
        .filter_map(|l| {
            let mut f = l.split_whitespace();
            let (dev, mnt) = (f.next()?, f.next()?);
            (unescape(mnt) == path).then(|| unescape(dev))
        })
        .next_back()
}

pub fn disk(m: &MountCfg) -> Disk {
    let mut d = Disk {
        label: m.label.clone().unwrap_or_else(|| m.path.clone()),
        path: m.path.clone(),
        device: None,
        total: 0,
        used: 0,
        avail: 0,
        error: None,
    };

    let path = if m.path.len() > 1 { m.path.trim_end_matches('/') } else { &m.path };
    d.device = mount_source(path);
    if d.device.is_none() {
        d.error = Some("not mounted".into());
        return d;
    }

    let Ok(c) = CString::new(path.as_bytes()) else {
        d.error = Some("invalid path".into());
        return d;
    };
    let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(c.as_ptr(), &mut st) } != 0 {
        d.error = Some(std::io::Error::last_os_error().to_string());
        return d;
    }
    let frsize = st.f_frsize as u64;
    d.total = st.f_blocks as u64 * frsize;
    d.used = (st.f_blocks as u64).saturating_sub(st.f_bfree as u64) * frsize;
    d.avail = st.f_bavail as u64 * frsize;
    d
}
