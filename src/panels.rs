// SPDX-License-Identifier: GPL-3.0-or-later
use crate::config::{PanelCfg, PanelFormat};
use crate::state::now_unix;
use crate::util;
use serde::Serialize;
use std::time::Duration;

#[derive(Serialize, Clone)]
pub struct Item {
    pub key: String,
    pub value: String,
}

#[derive(Serialize, Clone)]
pub struct PanelData {
    pub items: Vec<Item>,
    pub lines: Vec<String>,
    pub updated: u64,
    pub error: Option<String>,
}

pub fn collect(cfg: &PanelCfg) -> PanelData {
    let mut d = PanelData { items: Vec::new(), lines: Vec::new(), updated: now_unix(), error: None };

    let out = match util::sh(&cfg.cmd, Duration::from_secs(cfg.timeout.max(1))) {
        Ok(o) if o.success => o.stdout,
        Ok(o) => {
            d.error = Some(o.failure());
            o.stdout
        }
        Err(e) => {
            d.error = Some(e);
            return d;
        }
    };

    for line in out.lines() {
        if cfg.format == PanelFormat::Kv {
            if line.trim().is_empty() {
                continue;
            }
            let split = line.split_once(" : ").or_else(|| line.split_once(':')).or_else(|| line.split_once('='));
            if let Some((k, v)) = split {
                d.items.push(Item { key: k.trim().into(), value: v.trim().into() });
                continue;
            }
        }
        d.lines.push(line.to_string());
    }
    while d.lines.last().is_some_and(|l| l.trim().is_empty()) {
        d.lines.pop();
    }
    d
}
