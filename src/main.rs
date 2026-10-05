// SPDX-License-Identifier: GPL-3.0-or-later
mod alerts;
mod bitcoin;
mod config;
mod http;
mod logs;
mod panels;
mod services;
mod state;
mod system;
mod ups;
mod util;

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

const USAGE: &str = "\
pi-status — Raspberry Pi status dashboard

Usage: pi-status [OPTIONS]

Options:
  -c, --config DIR   Configuration directory
                     (default: $PI_STATUS_CONFIG, else /etc/pi-status)
      --check        Validate the configuration and exit
      --once         Collect everything once, print the JSON snapshot and exit
  -h, --help         Show this help
  -V, --version      Show version
";

fn main() -> ExitCode {
    let mut dir = std::env::var_os("PI_STATUS_CONFIG")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/etc/pi-status"));
    let mut check = false;
    let mut once = false;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-c" | "--config" => match args.next() {
                Some(d) => dir = d.into(),
                None => {
                    eprintln!("error: {arg} needs a directory");
                    return ExitCode::from(2);
                }
            },
            "--check" => check = true,
            "--once" => once = true,
            "-h" | "--help" => {
                print!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            "-V" | "--version" => {
                println!("pi-status {}", env!("CARGO_PKG_VERSION"));
                return ExitCode::SUCCESS;
            }
            other => {
                eprintln!("error: unknown argument '{other}'\n\n{USAGE}");
                return ExitCode::from(2);
            }
        }
    }

    let cfg = match config::Config::load(&dir, None) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        }
    };

    if check {
        println!("config dir : {}", dir.display());
        println!("listen     : {}", cfg.main.listen);
        println!("services   : {}", cfg.services.len());
        println!("panels     : {}", cfg.panels.len());
        println!("logs       : {}", cfg.main.logs.len());
        for e in &cfg.errors {
            eprintln!("\nerror: {e}");
        }
        return if cfg.errors.is_empty() {
            println!("OK");
            ExitCode::SUCCESS
        } else {
            ExitCode::FAILURE
        };
    }

    for e in &cfg.errors {
        eprintln!("config: {e}");
    }

    let listen = cfg.main.listen.clone();
    let shared = Arc::new(state::Shared::new(dir, cfg));

    if once {
        shared.collect_once();
        println!("{}", serde_json::to_string_pretty(&shared.snapshot()).unwrap_or_default());
        return ExitCode::SUCCESS;
    }

    state::spawn_collectors(&shared);
    eprintln!("pi-status {} listening on http://{listen}", env!("CARGO_PKG_VERSION"));

    match http::serve(shared, &listen) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: cannot listen on {listen}: {e}");
            ExitCode::FAILURE
        }
    }
}
