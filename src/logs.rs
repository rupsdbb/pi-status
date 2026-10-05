// SPDX-License-Identifier: GPL-3.0-or-later
use crate::config::LogCfg;
use crate::state::now_unix;
use crate::util;
use serde::Serialize;
use std::process::Command;
use std::time::Duration;

#[derive(Serialize, Clone)]
pub struct LogData {
    pub lines: Vec<String>,
    pub updated: u64,
    pub error: Option<String>,
}

pub fn collect(cfg: &LogCfg) -> LogData {
    let mut cmd = Command::new("journalctl");
    cmd.args(["--no-pager", "--quiet", "--output", "short-iso"]).args(&cfg.args);
    // Without filters, let journalctl return only the tail instead of the whole range.
    let has_limit = cfg.args.iter().any(|a| a == "-n" || a.starts_with("--lines") || (a.starts_with("-n") && a.len() > 2));
    if cfg.include.is_empty() && cfg.exclude.is_empty() && !has_limit {
        cmd.args(["--lines", &cfg.max_lines.to_string()]);
    }

    let include: Vec<String> = cfg.include.iter().map(|s| s.to_lowercase()).collect();
    let exclude: Vec<String> = cfg.exclude.iter().map(|s| s.to_lowercase()).collect();

    let (lines, error) = match util::run(cmd, Duration::from_secs(20)) {
        Ok(o) => {
            let mut lines: Vec<String> = o
                .stdout
                .lines()
                .filter(|l| {
                    let lc = l.to_lowercase();
                    (include.is_empty() || include.iter().any(|p| lc.contains(p.as_str())))
                        && !exclude.iter().any(|p| lc.contains(p.as_str()))
                })
                .map(String::from)
                .collect();
            if lines.len() > cfg.max_lines {
                lines.drain(..lines.len() - cfg.max_lines);
            }
            // journalctl exits 1 when nothing matches; only treat stderr-with-no-output as an error
            let error = (!o.success && o.stdout.trim().is_empty() && !o.stderr.trim().is_empty()).then(|| o.failure());
            (lines, error)
        }
        Err(e) => (Vec::new(), Some(e)),
    };

    LogData { lines, updated: now_unix(), error }
}
